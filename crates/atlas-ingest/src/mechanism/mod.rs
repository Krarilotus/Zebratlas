//! Mechanism sources: HGNC genes, Reactome pathways, Gene2Phenotype mechanisms, ClinGen dosage
//! sensitivity and GO biological process annotations.
//!
//! Every file is a `prov:Entity` (URL, version, retrieval time, sha256) in [`MechanismData::provenance`];
//! every parse step is a `prov:Activity` counting what it read, kept and skipped. G2P disease ids
//! are resolved to canonical atlas nodes through the identity layer of the [`Atlas`] passed in.
//! The result is cached as `data/cache/mechanism.snapshot` (bincode), keyed by the size and mtime of
//! these files and of the atlas sources (identity resolution depends on them).

pub mod clingen;
pub mod g2p;
pub mod go;
pub mod hgnc;
pub use atlas_core::mechanism::process;
pub mod reactome;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use atlas_core::provenance::{Activity, ActivityIdx, Agent, EntityIdx, Provenance};
use atlas_core::{Atlas, CoreError};
use serde::{Deserialize, Serialize};

// The data types live in atlas-core (analytics consumes them without depending on ingest).
pub use atlas_core::mechanism::{
    CLINGEN, ClinGenDosage, DosageScore, Effect, G2P, G2pRecord, GO_GAF, GO_OBO, HGNC, Hgnc, HgncGene, MechanismData,
    ProcessIdx, ProcessKind, ProcessOntology, REACTOME_GENES, REACTOME_PATHWAYS, REACTOME_RELATION,
};
pub use atlas_core::provenance::params;

use crate::Origin;
use crate::error::IngestError;
use crate::sources::{self, SourceFile};

/// Activity ids of the mechanism ingest.
pub mod activity {
    pub const INGEST_HGNC: &str = "activity:ingest-hgnc";
    pub const INGEST_REACTOME: &str = "activity:ingest-reactome";
    /// Parse G2P and resolve its MONDO/OMIM ids to canonical atlas diseases.
    pub const INGEST_G2P: &str = "activity:ingest-g2p";
    pub const INGEST_CLINGEN: &str = "activity:ingest-clingen-dosage";
    pub const INGEST_GO: &str = "activity:ingest-go-bp";
}

const CC0: &str = "CC0-1.0";
const CC_BY: &str = "CC-BY-4.0";

pub const SOURCES: [SourceFile; 8] = [
    SourceFile {
        file: HGNC,
        url: "https://storage.googleapis.com/public-download-files/hgnc/tsv/tsv/hgnc_complete_set.txt",
        licence: CC0,
    },
    SourceFile {
        file: REACTOME_PATHWAYS,
        url: "https://reactome.org/download/current/ReactomePathways.txt",
        licence: CC_BY,
    },
    SourceFile {
        file: REACTOME_RELATION,
        url: "https://reactome.org/download/current/ReactomePathwaysRelation.txt",
        licence: CC_BY,
    },
    SourceFile {
        file: REACTOME_GENES,
        url: "https://reactome.org/download/current/NCBI2Reactome.txt",
        licence: CC_BY,
    },
    SourceFile {
        file: G2P,
        url: "https://www.ebi.ac.uk/gene2phenotype/api/panel/all/download/",
        licence: "EMBL-EBI terms of use (G2P: free to use with attribution)",
    },
    SourceFile {
        file: CLINGEN,
        url: "https://ftp.clinicalgenome.org/ClinGen_gene_curation_list_GRCh38.tsv",
        licence: "ClinGen terms of use (free with attribution)",
    },
    SourceFile {
        file: GO_OBO,
        url: "https://purl.obolibrary.org/obo/go/go-basic.obo",
        licence: CC_BY,
    },
    SourceFile {
        file: GO_GAF,
        url: "https://current.geneontology.org/annotations/goa_human.gaf.gz",
        licence: CC_BY,
    },
];

pub fn snapshot_path(data: &Path) -> PathBuf {
    match std::env::var_os("RARE_ATLAS_MECHANISM_SNAPSHOT") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => crate::snapshot_dir(data).join("mechanism.snapshot"),
    }
}

/// Freshness key: mechanism/atlas sources and accepted identity policy (G2P resolves disease IDs).
pub fn signature(raw: &Path) -> Result<String, IngestError> {
    let mut out = sources::signature(raw)?;
    let projection = crate::identity_projection::load(raw.parent().unwrap_or(raw))?;
    out.push_str(&format!(
        "identity:{}:{}:{}:{}:{};",
        atlas_core::identity_policy::RULE,
        atlas_core::identity_policy::VERSION,
        atlas_core::identity_policy::code_sha256(),
        projection.accepted.manifest_sha256,
        crate::identity_projection::digest(include_str!("../build.rs").replace("\r\n", "\n").as_bytes())
    ));
    for src in &SOURCES {
        let path = raw.join(src.file);
        let meta = std::fs::metadata(&path).map_err(IngestError::io(&path))?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        out.push_str(&format!("{}:{}:{};", src.file, meta.len(), mtime));
    }
    Ok(out)
}

/// Load the snapshot if fresh, else parse the sources and save it.
pub fn load_or_build(data: &Path, atlas: &Atlas, rebuild: bool) -> Result<(MechanismData, Origin), IngestError> {
    let raw = crate::raw_dir(data);
    let path = snapshot_path(data);
    let sig = signature(&raw)?;
    if !rebuild
        && path.exists()
        && let Ok((m, s)) = load(&path)
        && s == sig
    {
        return Ok((m, Origin::Snapshot));
    }
    let m = build(&raw, atlas)?;
    save(&path, &m, &sig)?;
    Ok((m, Origin::Built))
}

fn now() -> Option<String> {
    Some(sources::rfc3339(SystemTime::now()))
}

struct Steps<'a> {
    agent: Agent,
    entities: &'a HashMap<&'static str, EntityIdx>,
}

impl Steps<'_> {
    fn start(&self, prov: &mut Provenance, id: &str, label: &str, used: &[&str]) -> ActivityIdx {
        prov.add_activity(Activity {
            id: id.to_owned(),
            label: label.to_owned(),
            started_at: now(),
            used: used.iter().map(|f| self.entities[f]).collect(),
            agent: self.agent.clone(),
            ..Activity::default()
        })
    }
}

fn finish(prov: &mut Provenance, act: ActivityIdx, counts: &[(&str, u64)], params: &[(&str, String)]) {
    let a = prov.activity_mut(act);
    for (k, n) in counts {
        a.count(k, *n);
    }
    for (k, v) in params {
        a.parameters.insert((*k).to_owned(), v.clone());
    }
    a.ended_at = now();
}

/// Parse every mechanism source under `raw`; G2P diseases resolve through `atlas.identity`.
pub fn build(raw: &Path, atlas: &Atlas) -> Result<MechanismData, IngestError> {
    let repo = raw.parent().and_then(Path::parent).unwrap_or(raw);
    let mut prov = Provenance::default();
    let mut entities = HashMap::new();
    for src in &SOURCES {
        entities.insert(src.file, prov.add_entity(sources::entity(raw, src)?));
    }
    let mut agent = sources::agent(repo);
    agent.name = "atlas-ingest::mechanism".into();
    let steps = Steps {
        agent,
        entities: &entities,
    };
    let p = |f: &str| raw.join(f);
    let set_version = |prov: &mut Provenance, file: &str, v: Option<String>| {
        prov.entities[usize::from(entities[file].0)].version = v;
    };

    std::thread::scope(|s| {
        let hashes = s.spawn(|| {
            SOURCES
                .iter()
                .map(|src| sources::sha256(&raw.join(src.file)))
                .collect::<Vec<_>>()
        });

        let act = steps.start(&mut prov, activity::INGEST_HGNC, "Parse HGNC approved genes", &[HGNC]);
        let hgnc = hgnc::read(&p(HGNC), entities[HGNC])?;
        finish(
            &mut prov,
            act,
            &[
                ("kept", hgnc.genes.len() as u64),
                ("skipped:not-approved", hgnc.skipped_not_approved),
            ],
            &[],
        );

        let act = steps.start(
            &mut prov,
            activity::INGEST_REACTOME,
            "Parse Reactome human pathways, hierarchy and lowest-level gene membership; IC over genes",
            &[REACTOME_PATHWAYS, REACTOME_RELATION, REACTOME_GENES],
        );
        let (reactome, n) = reactome::read(&p(REACTOME_PATHWAYS), &p(REACTOME_RELATION), &p(REACTOME_GENES))?;
        finish(
            &mut prov,
            act,
            &[
                ("pathways", n.pathways),
                ("skipped:other-species-pathways", n.other_species),
                ("relations", n.relations),
                ("memberships", n.memberships),
                ("skipped:other-species-memberships", n.memberships_other_species),
                ("annotated-genes", reactome.annotated_genes as u64),
            ],
            &[("ic", "-ln(genes under pathway / annotated genes)".into())],
        );

        let act = steps.start(
            &mut prov,
            activity::INGEST_G2P,
            "Parse Gene2Phenotype; resolve disease MONDO/OMIM ids to canonical atlas nodes",
            &[G2P],
        );
        let (g2p, n) = g2p::read(&p(G2P), entities[G2P], atlas)?;
        let md5 = std::fs::read_to_string(p(&format!("{G2P}.md5")))
            .ok()
            .and_then(|s| s.split_whitespace().next().map(str::to_owned));
        finish(
            &mut prov,
            act,
            &[
                ("read", n.read),
                ("resolved", n.resolved),
                ("unresolved:no-active-atlas-node", n.unresolved),
                ("filed-under-own-g2p-node", n.own_node),
            ],
            &[("published_md5", md5.unwrap_or_else(|| "missing".into()))],
        );
        set_version(&mut prov, G2P, Some("2026-09-28".into()));

        let act = steps.start(
            &mut prov,
            activity::INGEST_CLINGEN,
            "Parse ClinGen dosage sensitivity (HI/TS scores)",
            &[CLINGEN],
        );
        let (clingen, date) = clingen::read(&p(CLINGEN), entities[CLINGEN])?;
        finish(&mut prov, act, &[("kept", clingen.len() as u64)], &[]);
        set_version(&mut prov, CLINGEN, date);

        let act = steps.start(
            &mut prov,
            activity::INGEST_GO,
            "Parse GO biological process (is_a + part_of) and human GAF annotations; IC over genes",
            &[GO_OBO, GO_GAF, HGNC],
        );
        let mut n = go::GoCounts::default();
        let (mut go_bp, alt) = go::read_obo(&p(GO_OBO), &mut n)?;
        go::read_gaf(&p(GO_GAF), &mut go_bp, &alt, &hgnc, &mut n)?;
        finish(
            &mut prov,
            act,
            &[
                ("bp-terms", n.bp_terms),
                ("skipped:obsolete-terms", n.obsolete_terms),
                ("skipped:other-namespace-terms", n.other_namespace_terms),
                ("annotations-read", n.annotations_read),
                ("kept", n.kept),
                ("skipped:aspect-not-P", n.skipped_other_aspect),
                ("skipped:NOT-qualifier", n.skipped_not),
                ("skipped:evidence-IEA-or-ND", n.skipped_evidence),
                ("skipped:taxon", n.skipped_taxon),
                ("skipped:symbol-not-in-HGNC", n.skipped_unknown_gene),
                ("skipped:obsolete-or-unknown-term", n.skipped_obsolete_or_unknown_term),
                ("annotated-genes", go_bp.annotated_genes as u64),
            ],
            &[
                ("excluded_evidence", go::EXCLUDED_EVIDENCE.join(",")),
                ("relations", "is_a,part_of (within BP)".into()),
                ("ic", "-ln(genes under term / annotated genes)".into()),
            ],
        );
        set_version(&mut prov, GO_OBO, n.data_version.clone());
        set_version(&mut prov, GO_GAF, n.gaf_date.clone());

        for (src, hash) in SOURCES.iter().zip(hashes.join().expect("hash thread panicked")) {
            prov.entities[usize::from(entities[src.file].0)].sha256 = Some(hash?);
        }
        let mut m = MechanismData::new(prov, hgnc, reactome, go_bp, g2p, clingen);
        m.finish();
        Ok(m)
    })
}

const MAGIC: &[u8; 8] = b"MECHSNAP";
/// Bump on any change to a serialised type.
pub const FORMAT: u32 = 5;

#[derive(Serialize)]
struct Out<'a> {
    signature: &'a str,
    data: &'a MechanismData,
}

#[derive(Deserialize)]
struct In {
    signature: String,
    data: MechanismData,
}

pub fn save(path: &Path, m: &MechanismData, signature: &str) -> Result<(), IngestError> {
    let codec = |source| {
        IngestError::Core(CoreError::Codec {
            path: path.to_owned(),
            source,
        })
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(IngestError::io(dir))?;
    }
    let tmp = path.with_extension("snapshot.part");
    let mut w = BufWriter::new(File::create(&tmp).map_err(IngestError::io(&tmp))?);
    w.write_all(MAGIC).map_err(IngestError::io(&tmp))?;
    w.write_all(&FORMAT.to_le_bytes()).map_err(IngestError::io(&tmp))?;
    bincode::serialize_into(&mut w, &Out { signature, data: m }).map_err(codec)?;
    w.flush().map_err(IngestError::io(&tmp))?;
    drop(w);
    std::fs::rename(&tmp, path).map_err(IngestError::io(path))
}

pub fn load(path: &Path) -> Result<(MechanismData, String), IngestError> {
    let mut r = BufReader::with_capacity(1 << 20, File::open(path).map_err(IngestError::io(path))?);
    let mut header = [0u8; 12];
    r.read_exact(&mut header).map_err(IngestError::io(path))?;
    let found = u32::from_le_bytes(header[8..].try_into().expect("4 bytes"));
    if &header[..8] != MAGIC || found != FORMAT {
        return Err(IngestError::Core(CoreError::Format {
            path: path.to_owned(),
            found,
            expected: FORMAT,
        }));
    }
    let In { signature, mut data } = bincode::deserialize_from(r).map_err(|source| {
        IngestError::Core(CoreError::Codec {
            path: path.to_owned(),
            source,
        })
    })?;
    data.finish();
    Ok((data, signature))
}

// -- shared helpers for the tabular parsers --

pub(crate) fn tsv_reader(path: &Path) -> Result<csv::Reader<File>, IngestError> {
    let f = File::open(path).map_err(IngestError::io(path))?;
    Ok(csv::ReaderBuilder::new().delimiter(b'\t').flexible(true).from_reader(f))
}

pub(crate) fn column(headers: &csv::StringRecord, name: &'static str, path: &Path) -> Result<usize, IngestError> {
    headers
        .iter()
        .position(|h| h == name)
        .ok_or(IngestError::MissingColumn {
            path: path.to_owned(),
            column: name,
        })
}

/// 1-based physical line where the record starts.
pub(crate) fn line_of(rec: &csv::StringRecord) -> u32 {
    rec.position().map_or(0, |p| p.line() as u32)
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn revocation_invalidates_mechanism_snapshot_with_unchanged_source_files() {
        use crate::graph::fixtures;
        let d = fixtures::TempData::new();
        let raw = d.path().join("raw");
        std::fs::create_dir(&raw).unwrap();
        for source in sources::SOURCES.iter().chain(SOURCES.iter()) {
            std::fs::write(raw.join(source.file), b"").unwrap();
        }
        d.write("cache/intake/test.sssom.tsv", format!(
            "subject_id\tpredicate_id\tobject_id\tmapping_justification\trule_id\trule_version\tevidence_url\tevidence_sha256\tevidence_locator\nMONDO:1\tskos:exactMatch\tOMIM:1\tsemapv:BackgroundKnowledgeBasedMatching\tR-DIS-01\t1.0.0\thttps://example.invalid/fixture\t{}\tfixture\n",
            "a".repeat(64)
        ).as_bytes());
        let intake = d.path().join("cache/intake");
        let gate = d.path().join("cache/mappings");
        let manifest = atlas_discovery::align::safety::directory(&intake, &gate, None).unwrap();
        let before = signature(&raw).unwrap();
        save(&snapshot_path(d.path()), &MechanismData::default(), &before).unwrap();
        assert!(matches!(
            load_or_build(d.path(), &fixtures::atlas(), false).unwrap().1,
            Origin::Snapshot
        ));
        let ledger = std::fs::read_to_string(gate.join("identity-decisions.jsonl")).unwrap();
        let decision: serde_json::Value = serde_json::from_str(ledger.lines().next().unwrap()).unwrap();
        let assertion = decision["prov:wasDerivedFrom"]["@id"]
            .as_str()
            .unwrap()
            .rsplit(':')
            .next()
            .unwrap();
        let review = d.path().join("review.json");
        std::fs::write(
            &review,
            serde_json::to_vec(&serde_json::json!({
                "input_digest":manifest["input_digest"], "actor":"fixture", "reviewed_at":"2026-10-04T00:00:00Z",
                "reason":"fixture revocation", "revoke":[assertion]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::rename(&gate, d.path().join("previous-gate")).unwrap();
        atlas_discovery::align::safety::directory(&intake, &gate, Some(&review)).unwrap();
        assert_ne!(before, signature(&raw).unwrap());
        // Empty fixture sources cannot rebuild: success here would prove stale cache reuse.
        assert!(load_or_build(d.path(), &fixtures::atlas(), false).is_err());
    }
}
