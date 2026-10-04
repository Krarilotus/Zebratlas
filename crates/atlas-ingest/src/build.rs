//! Build the evidence graph: diseases (MONDO-keyed), phenotypes (HPO), genes, epidemiology.
//!
//! Port of `atlas.build()`. Every step is a `prov:Activity` that counts what it read, kept and
//! skipped. Retired Orphanet entries are kept as `status=retired` nodes (the Python drops them).
//! Identity decisions follow the Python except one deliberate change (D30.1): a name match only
//! nominates a `candidate_same_as` link and never merges; `build_with(.., LabelPolicy::Merge)`
//! reproduces the Python (pre-D30) identity for frozen historical evaluations.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;

use atlas_core::disease::{Classification, Disease, Status, add_attribute};
use atlas_core::evidence::GeneLink;
use atlas_core::identity::LabelPolicy;
use atlas_core::provenance::{Activity, ActivityIdx, Agent, EntityIdx, Provenance, RecordRef, activity};
use atlas_core::{Atlas, DiseaseIdentity, DiseaseIdx, Ontology, Term};

use crate::error::IngestError;
use crate::sources::{self, SOURCES};
use crate::{g2p, hpoa, obo, orphanet};

/// Retired Orphanet entries.
pub const RETIRED_PREFIXES: [&str; 2] = ["OBSOLETE:", "MOVED TO"];
pub const NON_RARE_PREFIX: &str = "NON RARE IN EUROPE:";

/// Parse every source under `raw` and assemble the atlas (checksums are computed alongside).
pub fn build(raw: &Path) -> Result<Atlas, IngestError> {
    build_with(raw, LabelPolicy::default())
}

/// [`build`] with an explicit label policy; `LabelPolicy::Merge` only reproduces pre-D30 runs.
pub fn build_with(raw: &Path, label_policy: LabelPolicy) -> Result<Atlas, IngestError> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut prov = Provenance::default();
    let mut entities = HashMap::new();
    for src in &SOURCES {
        entities.insert(src.file, prov.add_entity(sources::entity(raw, src)?));
    }
    let run = Run {
        raw,
        agent: sources::agent(&repo),
        entities,
        label_policy,
        accepted: crate::identity_projection::load(raw.parent().unwrap_or(raw))?.accepted,
    };
    std::thread::scope(|s| {
        // one background thread, sequential: checksums without saturating the disk
        let hashes = s.spawn(|| {
            SOURCES
                .iter()
                .map(|src| sources::sha256(&raw.join(src.file)))
                .collect::<Vec<_>>()
        });
        let mut atlas = run.graph(&mut prov)?;
        for (src, hash) in SOURCES.iter().zip(hashes.join().expect("hash thread panicked")) {
            let e = run.entities[src.file];
            atlas.provenance.entities[usize::from(e.0)].sha256 = Some(hash?);
        }
        Ok(atlas)
    })
}

struct Run<'a> {
    raw: &'a Path,
    agent: Agent,
    entities: HashMap<&'static str, EntityIdx>,
    label_policy: LabelPolicy,
    accepted: atlas_core::identity_policy::AcceptedIdentity,
}

fn now() -> Option<String> {
    Some(sources::rfc3339(SystemTime::now()))
}

impl Run<'_> {
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

    fn path(&self, file: &str) -> std::path::PathBuf {
        self.raw.join(file)
    }

    fn graph(&self, prov: &mut Provenance) -> Result<Atlas, IngestError> {
        let e = |f: &str| self.entities[f];

        let act = self.start(prov, activity::INGEST_HPO, "Parse HPO", &[sources::HP_OBO]);
        let hp = obo::read_obo(&self.path(sources::HP_OBO))?;
        prov.entities[usize::from(e(sources::HP_OBO).0)].version = hp.data_version().map(str::to_owned);
        let hpo = Ontology::new(hp.terms);
        finish(prov, act, &[("terms", hpo.len())]);

        let act = self.start(
            prov,
            activity::INGEST_MONDO,
            "Parse MONDO; create rare-subset disease nodes",
            &[sources::MONDO_OBO],
        );
        let mondo = obo::read_obo(&self.path(sources::MONDO_OBO))?;
        prov.entities[usize::from(e(sources::MONDO_OBO).0)].version = mondo.data_version().map(str::to_owned);
        let (disorders, version) = orphanet::read_disorders(&self.path(sources::PRODUCT1), e(sources::PRODUCT1))?;
        prov.entities[usize::from(e(sources::PRODUCT1).0)].version = version;

        let exact = self.start(
            prov,
            activity::IDENTITY_MONDO_EXACT,
            "Merge on MONDO:equivalentTo xrefs",
            &[sources::MONDO_OBO],
        );
        let orpha_exact = self.start(
            prov,
            activity::IDENTITY_ORPHANET_EXACT,
            "Merge on validated Orphanet E mappings to OMIM/MONDO",
            &[sources::PRODUCT1, sources::MONDO_OBO],
        );
        let identity = DiseaseIdentity::new_gated(
            &mondo.terms,
            disorders.iter().map(|(k, d)| (k.as_str(), d.mappings.as_slice())),
            &self.accepted,
        )
        .with_label_policy(self.label_policy);
        for act in [exact, orpha_exact] {
            prov.activity_mut(act).parameters.insert(
                "implementation_sha256".into(),
                crate::identity_projection::digest(include_str!("build.rs").replace("\r\n", "\n").as_bytes()),
            );
            prov.activity_mut(act).parameters.insert(
                "identity_gate_manifest_sha256".into(),
                self.accepted.manifest_sha256.clone(),
            );
            prov.activity_mut(act).parameters.insert(
                "identity_policy".into(),
                format!(
                    "{}@{}",
                    atlas_core::identity_policy::RULE,
                    atlas_core::identity_policy::VERSION
                ),
            );
        }
        let counts = identity.merge_counts();
        let conflicts = |by: &str| identity.conflicts().iter().filter(|c| c.asserted_by == by).count();
        finish(
            prov,
            exact,
            &[("merged", counts[0].1), ("conflicts", conflicts("MONDO"))],
        );
        finish(
            prov,
            orpha_exact,
            &[("merged", counts[1].1), ("conflicts", conflicts("Orphanet"))],
        );
        let label = self.start(
            prov,
            activity::IDENTITY_LABEL,
            match self.label_policy {
                LabelPolicy::Candidate => {
                    "Guarded nomination on equal names / exact synonyms (one id per source and MONDO term): \
                     candidate_same_as links, never merges (D30.1)"
                }
                LabelPolicy::Merge => {
                    "Guarded merge on equal names / exact synonyms (one id per source and MONDO term; pre-D30 \
                     reproduction)"
                }
            },
            &[sources::MONDO_OBO, sources::PRODUCT1, sources::HPOA],
        );
        prov.activity_mut(label).parameters.insert(
            "guard".into(),
            format!(
                "{}: {}",
                atlas_core::identity::LABEL_GUARD,
                atlas_core::identity::LABEL_GUARD_RULE
            ),
        );

        let mut g = Graph::new(identity, &mondo.terms, e(sources::MONDO_OBO));
        let rare: Vec<&Term> = mondo
            .terms
            .iter()
            .filter(|t| g.identity.is_live_mondo(&t.id) && t.in_subset("rare"))
            .collect();
        let mut n = 0;
        for t in rare {
            g.node(
                &t.id,
                &[] as &[&str],
                act,
                RecordRef::record(e(sources::MONDO_OBO), &t.id),
            );
            n += 1;
        }
        finish(prov, act, &[("terms", mondo.terms.len()), ("rare_nodes", n)]);

        let act = self.start(
            prov,
            activity::INGEST_ORPHANET_DISORDERS,
            "Orphanet disorders: names, synonyms, mappings",
            &[sources::PRODUCT1],
        );
        let classify = self.start(
            prov,
            activity::CLASSIFY_ORPHANET_STATUS,
            "Orphanet name prefixes: OBSOLETE:/MOVED TO -> retired; NON RARE IN EUROPE: -> not rare",
            &[sources::PRODUCT1],
        );
        let (mut retired_n, mut non_rare_n) = (0, 0);
        for (orpha, dis) in &disorders {
            if let Some(prefix) = RETIRED_PREFIXES.iter().find(|p| dis.name.starts_with(*p)) {
                g.retire(dis, act, classify, prefix);
                retired_n += 1;
                continue;
            }
            let (name, rare) = match dis.name.strip_prefix(NON_RARE_PREFIX) {
                Some(rest) => (rest.trim(), false),
                None => (dis.name.as_str(), true),
            };
            let links = g.identity.links(orpha).to_vec();
            let names: Vec<&str> = std::iter::once(name)
                .chain(dis.synonyms.iter().map(String::as_str))
                .collect();
            let d = g.node(orpha, &names, act, dis.record.clone());
            if d.name.is_empty() {
                d.name = name.to_owned();
            }
            for n in &names {
                d.add_name(n, None);
            }
            d.rare |= rare;
            d.related.extend(links);
            if !rare {
                non_rare_n += 1;
                d.classifications.push(Classification {
                    property: "rare".into(),
                    value: d.rare.to_string(),
                    reason: format!(
                        "Orphanet name of {orpha} starts with '{NON_RARE_PREFIX}' (original: '{}'); prefix removed; \
                         rare stays true only if MONDO marks the node rare",
                        dis.name
                    ),
                    activity: classify,
                    record: Some(dis.record.clone()),
                });
            }
        }
        finish(
            prov,
            act,
            &[("disorders", disorders.len()), ("retired_nodes", retired_n)],
        );
        finish(prov, classify, &[("retired", retired_n), ("non_rare", non_rare_n)]);

        let act = self.start(
            prov,
            activity::INGEST_HPOA,
            "HPO annotations: phenotypes, NOT phenotypes, inheritance, course",
            &[sources::HPOA, sources::HP_OBO],
        );
        let hpoa_path = self.path(sources::HPOA);
        prov.entities[usize::from(e(sources::HPOA).0)].version = hpoa::hpoa_version(&hpoa_path)?;
        let annotations = hpoa::read_hpoa(&hpoa_path, e(sources::HPOA))?;
        let mut c = Counts::default();
        c.add("rows", annotations.len());
        for a in annotations {
            let Some(hid) = hpo.canonical(&a.hpo_id) else {
                c.add("skipped:unknown-hpo-term", 1);
                continue;
            };
            if hpo.term(hid).id != a.hpo_id {
                c.add("remapped:alt-or-obsolete-hpo-id", 1);
            }
            let record = RecordRef::record(e(sources::HPOA), &a.disease_id);
            let d = match g.retired(&a.disease_id) {
                Some(i) => {
                    c.add("kept:on-retired-node", 1);
                    let d = &mut g.diseases[i as usize];
                    d.derive_from(record);
                    d
                }
                None => g.node(&a.disease_id, &[&a.disease_name], act, record),
            };
            if d.name.is_empty() {
                d.name = a.disease_name.clone();
            }
            d.add_name(&a.disease_name, None);
            match (a.aspect.as_str(), a.negated) {
                ("P", _) => d.annotate(hid, a),
                ("I", false) => add_attribute(&mut d.inheritance, &hpo.term(hid).name, a.record),
                ("C", false) => add_attribute(&mut d.clinical_course, &hpo.term(hid).name, a.record),
                (aspect, negated) => {
                    let why = if negated { "negated" } else { "aspect" };
                    c.add(&format!("skipped:{why}-{aspect}"), 1);
                }
            }
        }
        c.finish(prov, act);

        let act = self.start(
            prov,
            activity::INGEST_GENES_TO_DISEASE,
            "OMIM/MedGen gene-disease links",
            &[sources::GENES_TO_DISEASE],
        );
        let mut c = Counts::default();
        for gd in hpoa::read_genes_to_disease(&self.path(sources::GENES_TO_DISEASE), e(sources::GENES_TO_DISEASE))? {
            c.add("rows", 1);
            if gd.source.contains("orphadata") {
                // same records as en_product6, which also carries PMIDs and assessment status
                c.add("skipped:orphadata-row-read-from-en_product6", 1);
                continue;
            }
            let record = RecordRef::record(e(sources::GENES_TO_DISEASE), &gd.disease_id);
            g.node(&gd.disease_id, &[] as &[&str], act, record)
                .genes
                .push(GeneLink {
                    symbol: gd.symbol,
                    association: gd.association,
                    source: "OMIM/MedGen".into(),
                    source_disease: gd.disease_id,
                    pmids: Vec::new(),
                    assessed: None,
                    hgnc: None,
                    ncbi_gene: Some(gd.ncbi_gene),
                    record: gd.record,
                });
        }
        c.finish(prov, act);

        let act = self.start(
            prov,
            activity::INGEST_ORPHANET_GENES,
            "Orphanet gene-disease associations",
            &[sources::PRODUCT6],
        );
        let (assocs, version) = orphanet::read_gene_associations(&self.path(sources::PRODUCT6), e(sources::PRODUCT6))?;
        prov.entities[usize::from(e(sources::PRODUCT6).0)].version = version;
        let mut c = Counts::default();
        for ga in assocs {
            c.add("rows", 1);
            let d = g.orphanet_node(&ga.orpha, act, ga.record.clone(), &mut c);
            d.genes.push(GeneLink {
                symbol: ga.symbol,
                association: ga.association,
                source: "Orphanet".into(),
                source_disease: ga.orpha,
                pmids: ga.pmids,
                assessed: Some(ga.status == "Assessed"),
                hgnc: ga.hgnc,
                ncbi_gene: None,
                record: ga.record,
            });
        }
        c.finish(prov, act);

        let act = self.start(
            prov,
            activity::INGEST_ORPHANET_PREVALENCE,
            "Orphanet prevalence",
            &[sources::PRODUCT9_PREV],
        );
        let (prevalence, version) =
            orphanet::read_prevalence(&self.path(sources::PRODUCT9_PREV), e(sources::PRODUCT9_PREV))?;
        prov.entities[usize::from(e(sources::PRODUCT9_PREV).0)].version = version;
        let mut c = Counts::default();
        for (orpha, items) in prevalence {
            c.add("rows", items.len());
            let record = items[0].record.clone();
            g.orphanet_node(&orpha, act, record, &mut c).prevalence.extend(items);
        }
        c.finish(prov, act);

        let act = self.start(
            prov,
            activity::INGEST_ORPHANET_NATURAL_HISTORY,
            "Orphanet onset and inheritance",
            &[sources::PRODUCT9_AGES],
        );
        let (history, version) =
            orphanet::read_natural_history(&self.path(sources::PRODUCT9_AGES), e(sources::PRODUCT9_AGES))?;
        prov.entities[usize::from(e(sources::PRODUCT9_AGES).0)].version = version;
        let mut c = Counts::default();
        for (orpha, nh) in history {
            c.add("rows", 1);
            let d = g.orphanet_node(&orpha, act, nh.record.clone(), &mut c);
            for o in &nh.onset {
                add_attribute(&mut d.onset, o, nh.record.clone());
            }
            for i in &nh.inheritance {
                add_attribute(&mut d.inheritance, i, nh.record.clone());
            }
        }
        c.finish(prov, act);

        self.g2p_conditions(prov, &mut g)?;

        let labels = g.identity.merge_counts()[2].1;
        let candidates = g.identity.candidates().count();
        finish(prov, label, &[("merged", labels), ("candidates", candidates)]);

        let ic = self.start(
            prov,
            activity::COMPUTE_IC,
            "Information content over active annotated diseases",
            &[sources::HP_OBO, sources::HPOA],
        );
        prov.activity_mut(ic).parameters.insert(
            "definition".into(),
            "IC(t) = -ln(p(t)), p(t) = share of active diseases annotated with t or a descendant; \
             unannotated terms -ln(1/(n+1))"
                .into(),
        );
        let Graph { identity, diseases, .. } = g;
        let mut atlas = Atlas::new(hpo.into_terms(), identity, std::mem::take(prov), diseases);
        finish(&mut atlas.provenance, ic, &[]);
        Ok(atlas)
    }
}

impl Run<'_> {
    /// Gene-specific G2P conditions that no OMIM/Orphanet/MONDO node covers become their own
    /// condition nodes (`G2P:<id>`), marked newly described, linked `narrower` to the generic MONDO
    /// term G2P gives (a link, never a merge). Covered: G2P names an OMIM phenotype in the graph, or
    /// its MONDO term is a node that already has a causal link to the gene.
    fn g2p_conditions(&self, prov: &mut Provenance, g: &mut Graph<'_>) -> Result<(), IngestError> {
        let e = self.entities[sources::G2P];
        let act = self.start(
            prov,
            activity::INGEST_G2P_CONDITIONS,
            "Gene2Phenotype conditions without an OMIM/Orphanet/MONDO node (newly described)",
            &[sources::G2P],
        );
        let records = g2p::read_g2p(&self.path(sources::G2P), e)?;
        let mut c = Counts::default();
        c.add("rows", records.len());
        for r in records {
            if !r.is_supported() {
                c.add("skipped:disputed-or-refuted", 1);
                continue;
            }
            let covers = |id: &Option<String>, need_gene: bool| {
                id.as_ref()
                    .and_then(|id| g.index.get(&g.identity.resolve(id)))
                    .is_some_and(|&i| {
                        let d = &g.diseases[i as usize];
                        d.is_active() && (!need_gene || d.genes.iter().any(|l| l.symbol == r.symbol && l.is_causal()))
                    })
            };
            if covers(&r.disease_omim, false) {
                c.add("covered:omim-node", 1);
                continue;
            }
            if covers(&r.disease_mondo, true) {
                c.add("covered:mondo-node-with-causal-gene", 1);
                continue;
            }
            let id = format!("G2P:{}", r.id);
            if g.index.contains_key(&id) {
                c.add("skipped:duplicate-id", 1);
                continue;
            }
            let mut d = Disease::new(id.clone(), act);
            d.name.clone_from(&r.disease_name);
            d.add_name(&r.disease_name, None);
            d.rare = true;
            if let Some(m) = &r.disease_mondo {
                d.related.push(atlas_core::identity::IdLink {
                    target: m.clone(),
                    relation: "narrower".into(),
                    asserted_by: "G2P".into(),
                });
            }
            d.genes.push(GeneLink {
                symbol: r.symbol.clone(),
                association: format!("G2P {} ({})", r.confidence, r.allelic_requirement),
                source: "G2P".into(),
                source_disease: id.clone(),
                pmids: r.publications.clone(),
                assessed: None,
                hgnc: r.hgnc.clone(),
                ncbi_gene: None,
                record: r.record.clone(),
            });
            let generic = r.disease_mondo.as_deref().unwrap_or("none");
            d.classifications.push(Classification {
                property: atlas_core::disease::NEWLY_DESCRIBED.into(),
                value: "true".into(),
                reason: format!(
                    "G2P {} defines this gene-specific condition; no OMIM, Orphanet or MONDO disease node covers it.                      Linked as narrower than {generic} (not merged)",
                    r.id
                ),
                activity: act,
                record: Some(r.record.clone()),
            });
            d.classifications.push(Classification {
                property: "confidence".into(),
                value: r.confidence.clone(),
                reason: format!(
                    "G2P confidence of {} ({}, {})",
                    r.id, r.allelic_requirement, r.molecular_mechanism
                ),
                activity: act,
                record: Some(r.record.clone()),
            });
            d.derive_from(r.record);
            g.push(d);
            c.add("created:newly-described", 1);
        }
        c.finish(prov, act);
        Ok(())
    }
}

fn finish(prov: &mut Provenance, act: ActivityIdx, counts: &[(&str, usize)]) {
    let a = prov.activity_mut(act);
    a.ended_at = now();
    for (k, n) in counts {
        a.count(k, *n as u64);
    }
}

#[derive(Default)]
struct Counts(Vec<(String, usize)>);

impl Counts {
    fn add(&mut self, key: &str, n: usize) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v += n,
            None => self.0.push((key.to_owned(), n)),
        }
    }

    fn finish(self, prov: &mut Provenance, act: ActivityIdx) {
        let counts: Vec<(&str, usize)> = self.0.iter().map(|(k, n)| (k.as_str(), *n)).collect();
        finish(prov, act, &counts);
    }
}

/// Disease nodes under construction; `node` is the Python `node()` closure.
struct Graph<'a> {
    identity: DiseaseIdentity,
    mondo: HashMap<&'a str, &'a Term>,
    mondo_entity: EntityIdx,
    diseases: Vec<Disease>,
    index: HashMap<String, DiseaseIdx>,
    retired: HashSet<String>,
}

impl<'a> Graph<'a> {
    fn new(identity: DiseaseIdentity, mondo: &'a [Term], mondo_entity: EntityIdx) -> Self {
        let live = mondo
            .iter()
            .filter(|t| identity.is_live_mondo(&t.id))
            .map(|t| (t.id.as_str(), t))
            .collect();
        Self {
            identity,
            mondo: live,
            mondo_entity,
            diseases: Vec::new(),
            index: HashMap::new(),
            retired: HashSet::new(),
        }
    }

    /// Canonical node for a source id (label nomination when names are given), created on first use.
    fn node<S: AsRef<str>>(
        &mut self,
        source_id: &str,
        names: &[S],
        act: ActivityIdx,
        record: RecordRef,
    ) -> &mut Disease {
        let cid = if names.is_empty() {
            self.identity.resolve(source_id)
        } else {
            self.identity.nominate_by_label(source_id, names)
        };
        let idx = match self.index.get(&cid) {
            Some(&i) => i,
            None => {
                let mut d = Disease::new(cid.clone(), act);
                if let Some(term) = self.mondo.get(cid.as_str()) {
                    d.name.clone_from(&term.name);
                    d.definition.clone_from(&term.definition);
                    d.rare = term.in_subset("rare");
                    d.parents = term
                        .parents
                        .iter()
                        .filter(|p| self.identity.is_live_mondo(p))
                        .cloned()
                        .collect();
                    for s in term.synonyms.iter().filter(|s| s.scope == atlas_core::Scope::Exact) {
                        d.add_name(&s.text, s.kind.as_deref());
                    }
                    d.derive_from(RecordRef::record(self.mondo_entity, &term.id));
                }
                self.push(d)
            }
        };
        let d = &mut self.diseases[idx as usize];
        if source_id != cid {
            d.source_ids.insert(source_id.to_owned());
        }
        d.derive_from(record);
        d
    }

    fn push(&mut self, d: Disease) -> DiseaseIdx {
        let idx = self.diseases.len() as DiseaseIdx;
        self.index.insert(d.id.clone(), idx);
        self.diseases.push(d);
        idx
    }

    /// Keep a retired Orphanet entry as its own node, outside identity merging.
    fn retire(&mut self, dis: &orphanet::Disorder, act: ActivityIdx, classify: ActivityIdx, prefix: &str) {
        let mut d = Disease::new(dis.orpha.clone(), act);
        d.status = Status::Retired;
        d.name.clone_from(&dis.name);
        d.add_name(&dis.name, None);
        for s in &dis.synonyms {
            d.add_name(s, None);
        }
        d.related.extend(self.identity.links(&dis.orpha).iter().cloned());
        d.derive_from(dis.record.clone());
        d.classifications.push(Classification {
            property: "status".into(),
            value: "retired".into(),
            reason: format!("Orphanet name starts with '{prefix}'; kept out of identity merging, analytics and search"),
            activity: classify,
            record: Some(dis.record.clone()),
        });
        self.retired.insert(dis.orpha.clone());
        self.push(d);
    }

    fn retired(&self, id: &str) -> Option<DiseaseIdx> {
        self.retired.contains(id).then(|| self.index[id])
    }

    /// Node for an Orphanet product record: the retired node if the code is retired, else `node`.
    fn orphanet_node(&mut self, orpha: &str, act: ActivityIdx, record: RecordRef, c: &mut Counts) -> &mut Disease {
        match self.retired(orpha) {
            Some(i) => {
                c.add("kept:on-retired-node", 1);
                let d = &mut self.diseases[i as usize];
                d.derive_from(record);
                d
            }
            None => self.node(orpha, &[] as &[&str], act, record),
        }
    }
}
