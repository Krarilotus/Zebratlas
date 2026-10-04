//! One generic KGX reader (D37 §1): `data/cache/kgx/<source>/{nodes,edges}.jsonl[.gz]` in KGX
//! JSON Lines with Biolink 4.x categories and predicates, plus our provenance and licence fields.
//!
//! **Identity.** Ids are normalised (`NCT:NCT…` → `NCT…`), mapped through the SSSOM exact-match
//! table, and resolved to atlas conditions, genes and phenotypes or existing graph nodes; a protein
//! named by `biolink:has_gene_product` of an atlas gene attaches to that gene.
//! **Scope (D37 §4).** Seeds are the atlas's rare conditions and its genes; an edge is kept when
//! one end is a seed (hop 1) or, with `hops = 2`, adjacent to a seed (hop 2). Hop-1 edges are
//! always kept, hop-2 edges up to `max_edges` per source. Files are streamed (two passes over the
//! edges, one over the nodes); excluded records, out-of-scope edges and unmapped ends are counted.
//! **Mapping.** Biolink categories become atlas kinds (ARCHITECTURE.md table); predicates become
//! typed relations, checked against the integrity shape; anything else is `related_to` with the
//! predicate in the reason. Licence and licence class travel per record (one entity per
//! file and licence).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use atlas_core::graph::{
    Access, Asset, AssetKind, Coverage, LicenceClass, LinkLevel, OrgKind, Organisation, Paper, RecIdx, RecordHash,
    Relation, SourceRecord, activity,
};
use atlas_core::integrity::shape;
use atlas_core::node::{EdgeKind, NodeKind};
use atlas_core::provenance::{ActivityIdx, EntityIdx, Locator, SourceEntity};
use serde_json::Value;

use super::builder::{Builder, NewEdge};
use super::cache;
use super::research::{self, opt, s, strs};
use super::sssom::Identity;
use crate::error::IngestError;

pub const DIR: &str = "cache/kgx";

/// Scope knobs (`RARE_ATLAS_KGX_HOPS`, `RARE_ATLAS_KGX_MAX_EDGES`).
#[derive(Clone, Copy, Debug)]
pub struct Scope {
    pub hops: u8,
    /// Hop-2 edges kept per source at most.
    pub max_edges: usize,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            hops: 2,
            max_edges: 200_000,
        }
    }
}

impl Scope {
    pub fn from_env() -> Self {
        let d = Self::default();
        let num = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok());
        Self {
            hops: num("RARE_ATLAS_KGX_HOPS").map_or(d.hops, |h| h.clamp(1, 2) as u8),
            max_edges: num("RARE_ATLAS_KGX_MAX_EDGES").unwrap_or(d.max_edges),
        }
    }
}

/// Source directories with a nodes or edges file, sorted.
pub fn sources(data: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(data.join(DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| file(data, n, "nodes").is_some() || file(data, n, "edges").is_some())
        .collect();
    out.sort();
    out
}

/// `kgx/<source>/<stem>.jsonl` or `.jsonl.gz` (relative to the data dir), when present.
fn file(data: &Path, source: &str, stem: &str) -> Option<String> {
    ["jsonl", "jsonl.gz"]
        .iter()
        .map(|ext| format!("{DIR}/{source}/{stem}.{ext}"))
        .find(|f| data.join(f).exists())
}

/// Every KGX file the build reads (for the snapshot signature).
pub fn inputs(data: &Path) -> Vec<std::path::PathBuf> {
    sources(data)
        .iter()
        .flat_map(|src| ["nodes", "edges"].map(|stem| file(data, src, stem)))
        .flatten()
        .map(|f| data.join(f))
        .collect()
}

use super::research::norm_curie as norm;

struct Resolver<'i> {
    ident: &'i Identity,
    /// Protein id → atlas gene id (`biolink:has_gene_product`).
    products: HashMap<String, String>,
}

impl Resolver<'_> {
    /// Key of an id: the atlas/graph node it resolves to, else its canonical form.
    fn key(&self, b: &Builder<'_>, id: &str) -> (String, Option<NodeKind>) {
        let c = self.ident.canonical(norm(id));
        let c = self.products.get(c).map_or(c, String::as_str);
        if let Some(d) = b.atlas.disease(c) {
            return (d.id.clone(), Some(NodeKind::Disease));
        }
        if let Some(g) = b.gene_id(c) {
            return (g, Some(NodeKind::Gene));
        }
        if c.starts_with("HP:")
            && let Some(t) = b.atlas.hpo.canonical(c)
        {
            return (b.atlas.hpo.term(t).id.clone(), Some(NodeKind::Phenotype));
        }
        (c.to_owned(), b.node(c).map(|(k, _)| k))
    }

    fn seed(&self, b: &Builder<'_>, key: &str, kind: Option<NodeKind>) -> bool {
        match kind {
            Some(NodeKind::Gene) => true,
            Some(NodeKind::Disease) => b.atlas.disease(key).is_some_and(|d| d.rare),
            _ => false,
        }
    }
}

/// Edge kept for the final pass.
struct Kept {
    record_id: String,
    record_url: Option<String>,
    retrieved_at: Option<String>,
    line: u32,
    sha: atlas_core::graph::Sha256,
    subject: String,
    predicate: String,
    object: String,
    level: LinkLevel,
    kind: EdgeKind,
    source: String,
    licence: String,
    class: LicenceClass,
}

/// Entities per (file, licence): records with a different licence than the file's first get
/// their own entity (same file, so verification re-reads the same path).
struct Entities {
    file: String,
    sha256: Option<String>,
    bytes: u64,
    version: Option<String>,
    retrieved_at: Option<String>,
    by_licence: HashMap<String, EntityIdx>,
}

impl Entities {
    fn new(data: &Path, file: &str, manifest: &Value) -> Self {
        let name = file.rsplit('/').next().unwrap_or(file).trim_end_matches(".gz");
        Self {
            file: file.to_owned(),
            sha256: opt(&manifest["files"][name], "sha256"),
            bytes: std::fs::metadata(data.join(file)).map(|m| m.len()).unwrap_or(0),
            version: opt(manifest, "schema").map(|s| format!("{s} (KGX JSON Lines)")),
            retrieved_at: opt(manifest, "created_at"),
            by_licence: HashMap::new(),
        }
    }

    fn get(
        &mut self,
        b: &mut Builder<'_>,
        data: &Path,
        act: ActivityIdx,
        licence: &str,
        class: LicenceClass,
    ) -> EntityIdx {
        if let Some(&e) = self.by_licence.get(licence) {
            return e;
        }
        if self.sha256.is_none() {
            self.sha256 = super::sssom::file_sha256(&data.join(&self.file)).ok();
        }
        let id = if self.by_licence.is_empty() {
            format!("source:{}", self.file)
        } else {
            format!("source:{}#licence={licence}", self.file)
        };
        let e = b.entity(SourceEntity {
            id,
            url: format!("KGX cache {}", self.file),
            file: self.file.clone(),
            version: self.version.clone(),
            retrieved_at: self.retrieved_at.clone(),
            sha256: self.sha256.clone(),
            bytes: self.bytes,
            licence: Some(licence.to_owned()),
        });
        research::licence(b, e, licence, class);
        b.data.provenance.activity_mut(act).used.push(e);
        self.by_licence.insert(licence.to_owned(), e);
        e
    }
}

fn licence_of(v: &Value) -> (String, LicenceClass) {
    let l = opt(v, "license").unwrap_or_else(|| "not stated".into());
    let class = opt(v, "license_class")
        .and_then(|c| LicenceClass::parse(&c))
        .unwrap_or_else(|| LicenceClass::classify(&l));
    (l, class)
}

fn edge_kind(v: &Value) -> (EdgeKind, LinkLevel) {
    let kl = s(v, "knowledge_level");
    if s(v, "agent_type") == "text_mining_agent" {
        (EdgeKind::Extracted, LinkLevel::Text)
    } else if matches!(
        kl,
        "prediction" | "statistical_association" | "not_provided" | "observation"
    ) {
        (EdgeKind::Inferred, LinkLevel::Related)
    } else {
        (EdgeKind::Observed, LinkLevel::Curated)
    }
}

/// Counters of one source.
#[derive(Default)]
struct Counts {
    nodes: usize,
    edges: usize,
    excluded: usize,
    out_of_scope: usize,
    over_cap: usize,
    unmapped: usize,
    people: usize,
}

pub fn ingest(b: &mut Builder<'_>, data: &Path, ident: &Identity, scope: Scope) -> Result<(), IngestError> {
    let act = b.start(
        activity::INGEST_KGX,
        "KGX caches (Biolink), scoped to two hops around rare conditions and their genes",
        &[],
    );
    b.param(act, "hops", scope.hops.to_string());
    b.param(act, "max_hop2_edges_per_source", scope.max_edges.to_string());
    for src in sources(data) {
        source(b, data, &src, ident, scope, act)?;
    }
    b.finish(act, &[]);
    Ok(())
}

fn source(
    b: &mut Builder<'_>,
    data: &Path,
    src: &str,
    ident: &Identity,
    scope: Scope,
    act: ActivityIdx,
) -> Result<(), IngestError> {
    let manifest: Value = std::fs::read(data.join(DIR).join(src).join("manifest.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    let edges_file = file(data, src, "edges");
    let nodes_file = file(data, src, "nodes");
    let mut res = Resolver {
        ident,
        products: HashMap::new(),
    };
    let mut c = Counts::default();
    // pass 1: gene products and hop-1 neighbours
    let mut hop1: HashSet<String> = HashSet::new();
    if let Some(f) = &edges_file {
        for line in cache::gz_lines(&data.join(f))? {
            let (_, bytes) = line.map_err(IngestError::io(&data.join(f)))?;
            let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            let (sk, skind) = res.key(b, s(&v, "subject"));
            if s(&v, "predicate") == "biolink:has_gene_product" && skind == Some(NodeKind::Gene) {
                res.products.insert(norm(s(&v, "object")).to_owned(), sk.clone());
            }
            let (ok, okind) = res.key(b, s(&v, "object"));
            if res.seed(b, &sk, skind) {
                hop1.insert(ok.clone());
            }
            if res.seed(b, &ok, okind) {
                hop1.insert(sk);
            }
        }
    }
    // pass 2: keep edges in scope
    let mut kept: Vec<Kept> = Vec::new();
    let mut hop2 = 0usize;
    if let Some(f) = &edges_file {
        for line in cache::gz_lines(&data.join(f))? {
            let (n, bytes) = line.map_err(IngestError::io(&data.join(f)))?;
            let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
                c.excluded += 1;
                continue;
            };
            if v["excluded"].as_bool() == Some(true) {
                c.excluded += 1;
                continue;
            }
            let (sk, skind) = res.key(b, s(&v, "subject"));
            let (ok, okind) = res.key(b, s(&v, "object"));
            let near = res.seed(b, &sk, skind) || res.seed(b, &ok, okind);
            if !near {
                let second = scope.hops >= 2 && (hop1.contains(&sk) || hop1.contains(&ok));
                if !second {
                    c.out_of_scope += 1;
                    continue;
                }
                if hop2 >= scope.max_edges {
                    c.over_cap += 1;
                    continue;
                }
                hop2 += 1;
            }
            let (licence, class) = licence_of(&v);
            let (kind, level) = edge_kind(&v);
            kept.push(Kept {
                record_id: opt(&v, "id").unwrap_or_else(|| format!("{}|{}|{}", s(&v, "subject"), s(&v, "predicate"), s(&v, "object"))),
                record_url: opt(&v, "prov_source_url").or_else(|| opt(&v, "url")).or_else(|| opt(&v, "iri")),
                retrieved_at: opt(&v, "prov_retrieved_at"),
                line: n,
                sha: cache::sha256(&bytes),
                subject: sk,
                predicate: s(&v, "predicate").to_owned(),
                object: ok,
                level,
                kind,
                source: s(&v, "primary_knowledge_source").to_owned(),
                licence,
                class,
            });
        }
    }
    drop(hop1);
    let needed: HashSet<&str> = kept
        .iter()
        .flat_map(|k| [k.subject.as_str(), k.object.as_str()])
        .filter(|id| b.node(id).is_none())
        .collect();
    let needed: HashSet<String> = needed.into_iter().map(str::to_owned).collect();
    // nodes pass
    if let Some(f) = &nodes_file {
        let mut ents = Entities::new(data, f, &manifest);
        for line in cache::gz_lines(&data.join(f))? {
            let (n, bytes) = line.map_err(IngestError::io(&data.join(f)))?;
            let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            let (key, kind) = res.key(b, s(&v, "id"));
            let merged_source = kind.is_some() && res.ident.canonical(norm(s(&v, "id"))) != norm(s(&v, "id"));
            if !merged_source && (kind.is_some() || !needed.contains(&key)) {
                continue;
            }
            if v["excluded"].as_bool() == Some(true) {
                c.excluded += 1;
                continue;
            }
            let (licence, class) = licence_of(&v);
            let entity = ents.get(b, data, act, &licence, class);
            let rec = b.record(SourceRecord {
                entity,
                locator: Locator::Line(n),
                id: s(&v, "id").to_owned(),
                url: opt(&v, "iri").or_else(|| opt(&v, "access_url")),
                fetched_at: opt(&v, "prov_retrieved_at"),
                hash: RecordHash::JsonLine,
                sha256: cache::sha256(&bytes),
            });
            if merged_source {
                // Canonicalisation must not erase the original member's source-record lineage.
                continue;
            }
            match node(b, src, &key, &v, rec, class) {
                Added::Yes => c.nodes += 1,
                Added::Person => c.people += 1,
                Added::No => c.unmapped += 1,
            }
        }
    }
    // edges
    if let Some(f) = &edges_file {
        let mut ents = Entities::new(data, f, &manifest);
        for k in kept {
            let (Some(sk), Some(ok)) = (kind_of(b, &k.subject), kind_of(b, &k.object)) else {
                c.unmapped += 1;
                continue;
            };
            let entity = ents.get(b, data, act, &k.licence, k.class);
            let rec = b.record(SourceRecord {
                entity,
                locator: Locator::Line(k.line),
                id: k.record_id.clone(),
                url: k.record_url.clone(),
                fetched_at: k.retrieved_at.clone(),
                hash: RecordHash::JsonLine,
                sha256: k.sha,
            });
            let (from, relation, to) = relation(b, &k, sk, ok);
            let reason = format!(
                "{} ({})",
                k.predicate,
                if k.source.is_empty() { src } else { &k.source }
            );
            b.edge(
                NewEdge {
                    from,
                    relation,
                    to,
                    kind: k.kind,
                    level: k.level,
                    reason,
                    activity: act,
                },
                &[rec],
            );
            c.edges += 1;
        }
    }
    let class = LicenceClass::parse(s(&manifest, "license_class"))
        .or_else(|| opt(&manifest["license_verification"], "license_class").and_then(|c| LicenceClass::parse(&c)));
    b.data.coverage.push(Coverage {
        source: format!("kgx:{src}"),
        label: format!("KGX cache {src}"),
        status: if edges_file.is_some() || nodes_file.is_some() { "loaded" } else { "absent" }.into(),
        files: [nodes_file, edges_file].into_iter().flatten().collect(),
        retrieved_at: opt(&manifest, "created_at"),
        scope: format!(
            "two hops around rare conditions and their genes; out of scope {}, over cap {}, unmapped {}, people not ingested {}",
            c.out_of_scope, c.over_cap, c.unmapped, c.people
        ),
        records: (c.nodes + c.edges) as u64,
        nodes: c.nodes as u64,
        edges: c.edges as u64,
        excluded: (c.excluded + c.out_of_scope + c.over_cap + c.unmapped) as u64,
        licence_class: class,
        ..Coverage::default()
    });
    Ok(())
}

fn kind_of(b: &Builder<'_>, id: &str) -> Option<NodeKind> {
    if b.atlas.disease(id).is_some_and(|d| d.id == id) {
        return Some(NodeKind::Disease);
    }
    if b.gene_id(id).as_deref() == Some(id) {
        return Some(NodeKind::Gene);
    }
    if b.atlas.hpo.canonical(id).is_some() {
        return Some(NodeKind::Phenotype);
    }
    b.node(id).map(|(k, _)| k)
}

fn asset_kind(b: &Builder<'_>, id: &str) -> Option<AssetKind> {
    match b.node(id) {
        Some((NodeKind::Asset, i)) => Some(b.data.assets[i as usize].kind),
        _ => None,
    }
}

/// Typed relation for a Biolink predicate and the end kinds (oriented `from → to`), checked
/// against the integrity shape; `related_to` otherwise.
fn relation<'k>(b: &Builder<'_>, k: &'k Kept, sk: NodeKind, ok: NodeKind) -> (&'k str, Relation, &'k str) {
    use NodeKind::*;
    let (s, o) = (k.subject.as_str(), k.object.as_str());
    let sa = asset_kind(b, s);
    let oa = asset_kind(b, o);
    let therapy =
        |a: Option<AssetKind>| matches!(a, Some(AssetKind::Drug | AssetKind::Programme | AssetKind::Designation));
    let model = |a: Option<AssetKind>| matches!(a, Some(AssetKind::Model | AssetKind::CellLine));
    let by_kinds = |s: &'k str,
                    sk: NodeKind,
                    sa: Option<AssetKind>,
                    o: &'k str,
                    ok: NodeKind|
     -> Option<(&'k str, Relation, &'k str)> {
        let r = match (sk, ok) {
            (Gene, Disease) => Relation::GeneAssociatedWithCondition,
            (Asset, Disease) if therapy(sa) => Relation::StudiedFor,
            (Asset, Gene) if therapy(sa) => Relation::Targets,
            (Asset, Disease | Gene) if model(sa) => Relation::ModelOf,
            (Asset, Disease | Gene) if sa != Some(AssetKind::OrthologGene) => Relation::ResourceFor,
            (Study, Disease) => Relation::StudiedFor,
            (Paper, Gene) => Relation::AboutGene,
            (Paper, Disease) => Relation::AboutCondition,
            (Asset | Gene, Phenotype) | (Disease, Phenotype) => Relation::HasPhenotype,
            _ => return None,
        };
        Some((s, r, o))
    };
    let typed = match k.predicate.as_str() {
        "biolink:orthologous_to" => Some((s, Relation::OrthologousTo, o)),
        "biolink:model_of" | "biolink:is_model_of" => Some((s, Relation::ModelOf, o)),
        "biolink:has_phenotype" => Some((s, Relation::HasPhenotype, o)),
        "biolink:same_as" | "biolink:exact_match" | "biolink:close_match" => Some((s, Relation::CandidateSameAs, o)),
        "biolink:affects"
        | "biolink:interacts_with"
        | "biolink:directly_physically_interacts_with"
        | "biolink:has_target"
            if therapy(sa) =>
        {
            Some((s, Relation::Targets, o))
        }
        "biolink:treats"
        | "biolink:treats_or_applied_or_studied_to_treat"
        | "biolink:applied_to_treat"
        | "biolink:studied_to_treat"
        | "biolink:in_clinical_trials_for"
        | "biolink:ameliorates_condition" => Some((s, Relation::StudiedFor, o)),
        _ => by_kinds(s, sk, sa, o, ok).or_else(|| by_kinds(o, ok, oa, s, sk)),
    };
    if let Some((from, r, to)) = typed {
        let (fk, tk) = if from == s { (sk, ok) } else { (ok, sk) };
        let (fs, ts) = shape(r);
        if fs.contains(&fk) && ts.contains(&tk) && from != to {
            return (from, r, to);
        }
    }
    (s, Relation::RelatedTo, o)
}

enum Added {
    Yes,
    No,
    Person,
}

fn first_cat(v: &Value) -> String {
    match &v["category"] {
        Value::Array(a) => a.first().and_then(Value::as_str).unwrap_or("").to_owned(),
        Value::String(c) => c.clone(),
        _ => String::new(),
    }
}

/// Graph node for one in-scope KGX node that is not an atlas/graph node yet.
fn node(b: &mut Builder<'_>, src: &str, id: &str, v: &Value, rec: RecIdx, class: LicenceClass) -> Added {
    let cat = first_cat(v);
    let name = opt(v, "name").unwrap_or_else(|| id.to_owned());
    let human_gene = ["HGNC:", "NCBIGene:", "ENSEMBL:", "UniProtKB:"]
        .iter()
        .any(|p| id.starts_with(p));
    let kind = match cat.as_str() {
        "biolink:Person" => return Added::Person,
        "biolink:Disease" | "biolink:PhenotypicFeature" | "biolink:DiseaseOrPhenotypicFeature" => return Added::No,
        "biolink:Gene" | "biolink:Protein" if human_gene => return Added::No,
        "biolink:Publication" | "biolink:Article" | "biolink:JournalArticle" => {
            b.register(id, NodeKind::Paper, b.data.papers.len());
            b.data.papers.push(Paper {
                id: id.to_owned(),
                title: name,
                journal: opt(v, "published_in").unwrap_or_default(),
                year: v["publication_year"].as_u64().map(|y| y as u16),
                doi: opt(v, "doi"),
                review: false,
                records: vec![rec],
            });
            return Added::Yes;
        }
        "biolink:Organization" | "biolink:Agent" => {
            b.register(id, NodeKind::Organisation, b.data.orgs.len());
            b.data.orgs.push(Organisation {
                id: id.to_owned(),
                name,
                kind: OrgKind::Institution,
                url: opt(v, "iri").or_else(|| opt(v, "url")),
                contact_url: None,
                country: opt(v, "country"),
                country_basis: opt(v, "country").map(|_| "directory".to_owned()),
                description: opt(v, "organization_type"),
                languages: Vec::new(),
                verified_on: None,
                channels: Vec::new(),
                records: vec![rec],
            });
            return Added::Yes;
        }
        "biolink:CellLine" => AssetKind::CellLine,
        "biolink:Genotype"
        | "biolink:Organism"
        | "biolink:OrganismTaxon"
        | "biolink:Strain"
        | "biolink:IndividualOrganism" => AssetKind::Model,
        "biolink:MaterialSample" | "biolink:Biospecimen" | "biolink:Biobank" => AssetKind::Biobank,
        "biolink:Dataset" | "biolink:DatasetVersion" | "biolink:DataSet" => AssetKind::Dataset,
        "biolink:Drug"
        | "biolink:SmallMolecule"
        | "biolink:ChemicalEntity"
        | "biolink:MolecularMixture"
        | "biolink:MolecularEntity" => AssetKind::Drug,
        "biolink:InformationContentEntity" => match src {
            "ema" | "fda" => AssetKind::Designation,
            "pipelines" | "nlorem" => AssetKind::Programme,
            _ => AssetKind::Other,
        },
        "biolink:Gene" => AssetKind::OrthologGene,
        _ => AssetKind::Other,
    };
    let route0 = v["access_routes"].as_array().and_then(|a| a.first());
    let holder = opt(v, "holder")
        .or_else(|| route0.and_then(|r| opt(r, "holder")))
        .or_else(|| opt(v, "data_access_committee_id"));
    let access = if let Some(r) = route0.filter(|r| opt(r, "request_url").is_some()) {
        Access {
            route: match s(r, "route_type") {
                "supplier_catalogue" | "repository" => "repository_order",
                "access_committee" => "access_committee",
                _ => "request_form",
            }
            .into(),
            url: opt(r, "request_url"),
            note: opt(r, "permitted_use").filter(|p| p != "not_assessed"),
        }
    } else if let Some(url) = opt(v, "access_policy_url") {
        Access {
            route: "access_committee".into(),
            url: Some(url),
            note: opt(v, "data_access_committee_id"),
        }
    } else {
        Access {
            route: "official_page".into(),
            // last resort: the Bioregistry resolver of the CURIE (the upstream record page)
            url: opt(v, "access_url")
                .or_else(|| opt(v, "iri"))
                .or_else(|| opt(v, "prov_source_url"))
                .or_else(|| id.contains(':').then(|| format!("https://bioregistry.io/{id}"))),
            note: opt(v, "access_route"),
        }
    };
    let mut facts = vec![
        ("category".to_owned(), cat.clone()),
        ("source".to_owned(), s(v, "primary_knowledge_source").to_owned()),
    ];
    // Keep explicit unknowns and null site status: a recruiting study is not a recruiting site.
    // Structured source data also preserves every request route and its separate reuse limits.
    let mut context = serde_json::Map::new();
    for key in [
        "asset_type",
        "access_routes",
        "overall_status",
        "site_status",
        "site",
        "last_update",
        "access_scope",
        "central_contact_roles",
        "site_contact_data",
        "availability",
        "permitted_use",
        "holder_status",
        "intended_use",
        "components",
        "reusable_component",
        "population_difference",
        "unresolved_question",
        "expert_check",
        "reuse_status",
        "rights",
        "condition_mapping",
        "material_rights",
    ] {
        if let Some(value) = v.get(key) {
            context.insert(key.into(), value.clone());
        }
    }
    if !context.is_empty() {
        facts.push(("access_context".into(), serde_json::Value::Object(context).to_string()));
    }
    for key in [
        "asset_type",
        "availability",
        "permitted_use",
        "holder_status",
        "intended_use",
        "orphan_designation_status",
        "interpretation",
        "knowledge_level",
    ] {
        if let Some(x) = opt(v, key).filter(|x| x != "unknown" && x != "not_assessed") {
            facts.push((key.to_owned(), x));
        }
    }
    let phases: Vec<&str> = strs(v, "phases").collect();
    if !phases.is_empty() {
        facts.push(("phases".into(), phases.join(", ")));
    }
    let id = research::asset_id(b, id);
    let new = research::asset(
        b,
        Asset {
            id: id.clone(),
            label: name,
            kind,
            category: cat,
            holder: None,
            holder_name: holder,
            verify_url: access.url.clone(),
            access,
            facts,
            release: class == LicenceClass::Open
                && v.get("release")
                    .is_none_or(|flag| flag == &serde_json::Value::Bool(true)),
            records: vec![rec],
        },
    );
    if new { Added::Yes } else { Added::No }
}
