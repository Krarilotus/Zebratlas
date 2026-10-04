//! Shared test helpers: a tiny synthetic atlas, the real atlas (read-only, skipped when the data
//! is missing), and a scripted in-process LLM provider (no network).

#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use atlas_core::disease::Disease;
use atlas_core::evidence::{GeneLink, PhenotypeAnnotation};
use atlas_core::graph::{GraphData, GraphEdge, LinkLevel, OrgKind, Organisation, Relation};
use atlas_core::node::EdgeKind;
use atlas_core::provenance::{ActivityIdx, EntityIdx, Provenance, RecordRef};
use atlas_core::term::{Scope, Synonym};
use atlas_core::{Atlas, DiseaseIdentity, Graph, Term};
use atlas_llm::provider::{Availability, KeyPolicy, Provider, ProviderKind};
use atlas_llm::registry::{Connection, ConnectionConfig};
use atlas_llm::request::{CompletionRequest, ProviderOutput, Usage};
use atlas_llm::{ApiKey, Cache, CacheMode, Llm, LlmError, Registry};

fn term(id: &str, name: &str, parent: Option<&str>) -> Term {
    Term {
        id: id.into(),
        name: name.into(),
        parents: parent.map(|p| vec![p.to_owned()]).unwrap_or_default(),
        ..Term::default()
    }
}

/// One condition (DEE4 / STXBP1), its gene, two symptoms, one patient group linked to the gene.
pub fn fixture() -> (Arc<Atlas>, Arc<Graph>) {
    let mut seizure = term("HP:0001250", "Seizure", Some("HP:0000118"));
    seizure.synonyms.push(Synonym {
        text: "Fits".into(),
        scope: Scope::Exact,
        kind: Some("layperson".into()),
    });
    let hpo = vec![
        term("HP:0000001", "All", None),
        term("HP:0000118", "Phenotypic abnormality", Some("HP:0000001")),
        seizure,
        term("HP:0001263", "Global developmental delay", Some("HP:0000118")),
    ];
    let rec = RecordRef::line(EntityIdx(0), 1);
    let mut d = Disease::new("MONDO:0012812", ActivityIdx(0));
    d.name = "developmental and epileptic encephalopathy 4".into();
    d.definition = "A rare brain condition with early seizures.".into();
    d.rare = true;
    d.add_name("STXBP1 encephalopathy", None);
    d.genes.push(GeneLink {
        symbol: "STXBP1".into(),
        association: "Disease-causing germline mutation(s) in".into(),
        source: "Orphanet".into(),
        source_disease: "ORPHA:178469".into(),
        pmids: vec![],
        assessed: Some(true),
        hgnc: Some("HGNC:11444".into()),
        ncbi_gene: None,
        record: rec.clone(),
    });
    for (i, hp) in ["HP:0001250", "HP:0001263"].iter().enumerate() {
        d.annotate(
            (i + 2) as u32,
            PhenotypeAnnotation {
                disease_id: "ORPHA:178469".into(),
                disease_name: d.name.clone(),
                hpo_id: (*hp).into(),
                negated: false,
                references: vec![],
                evidence: "TAS".into(),
                onset: None,
                frequency: None,
                sex: None,
                modifiers: vec![],
                aspect: "P".into(),
                biocuration: String::new(),
                record: rec.clone(),
            },
        );
    }
    let atlas = Atlas::new(hpo, DiseaseIdentity::default(), Provenance::default(), vec![d]);
    let data = GraphData {
        orgs: vec![Organisation {
            id: "org:stxbp1-foundation".into(),
            name: "STXBP1 Foundation".into(),
            kind: OrgKind::PatientGroup,
            url: Some("https://www.stxbp1disorders.org".into()),
            contact_url: None,
            country: Some("US".into()),
            country_basis: None,
            description: None,
            languages: vec!["en".into()],
            verified_on: Some("2026-10-03".into()),
            channels: vec![],
            records: vec![],
        }],
        edges: vec![GraphEdge {
            from: "org:stxbp1-foundation".into(),
            relation: Relation::ServesGene,
            to: "HGNC:11444".into(),
            kind: EdgeKind::Observed,
            level: LinkLevel::Curated,
            reason: String::new(),
            activity: ActivityIdx(0),
            records: vec![],
        }],
        ..GraphData::default()
    };
    (Arc::new(atlas), Arc::new(Graph::new(data)))
}

/// Data root of the main checkout (or `RARE_ATLAS_DATA`).
pub fn data_dir() -> PathBuf {
    atlas_ingest::data_dir()
}

/// The real atlas and graph, loaded once. Read-only: snapshots are used when current, else the
/// graph is built in memory (nothing is written). `None` when `data/raw` is missing.
pub fn real() -> Option<(Arc<Atlas>, Arc<Graph>)> {
    static REAL: OnceLock<Option<(Arc<Atlas>, Arc<Graph>)>> = OnceLock::new();
    REAL.get_or_init(|| {
        let data = data_dir();
        let raw = data.join("raw");
        if !raw.join("mondo.obo").exists() {
            eprintln!("skipping: no data at {}", raw.display());
            return None;
        }
        let snap = atlas_ingest::snapshot_path(&data);
        let sig = atlas_ingest::sources::signature(&raw).ok()?;
        let atlas = match atlas_core::snapshot::signature(&snap) {
            Ok(s) if s == sig => atlas_core::snapshot::load(&snap).ok()?.0,
            _ => atlas_ingest::build(&raw).expect("build atlas"),
        };
        let gsnap = atlas_ingest::graph::snapshot_path(&data);
        let gsig = atlas_ingest::graph::signature(&data).ok()?;
        let graph = match atlas_core::snapshot::graph_signature(&gsnap) {
            Ok(s) if s == gsig => atlas_core::snapshot::load_graph(&gsnap).ok()?.0,
            _ => Graph::new(atlas_ingest::graph::build(&data, &atlas).expect("build graph")),
        };
        Some((Arc::new(atlas), Arc::new(graph)))
    })
    .clone()
}

/// Scripted provider: replies in order (the last one repeats); records every request and key.
#[derive(Debug, Default)]
pub struct Mock {
    replies: Mutex<VecDeque<String>>,
    fail: bool,
    quota: bool,
    pub requests: Mutex<Vec<CompletionRequest>>,
    pub keys: Mutex<Vec<Option<String>>>,
}

#[async_trait]
impl Provider for Mock {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAiCompatible
    }

    fn base_url(&self) -> Option<&str> {
        Some("mock://")
    }

    async fn probe(&self) -> Availability {
        Availability::ready("ready")
    }

    async fn complete(
        &self,
        req: &CompletionRequest,
        _model: &str,
        key: Option<&ApiKey>,
    ) -> atlas_llm::Result<ProviderOutput> {
        self.requests.lock().unwrap().push(req.clone());
        self.keys.lock().unwrap().push(key.map(|k| format!("{k:?}")));
        if self.quota {
            return Err(LlmError::RateLimited("free quota reached for today".into()));
        }
        if self.fail {
            return Err(LlmError::Unavailable("mock is down".into()));
        }
        let mut q = self.replies.lock().unwrap();
        let text = if q.len() > 1 {
            q.pop_front().unwrap()
        } else {
            q.front().cloned().unwrap_or_default()
        };
        Ok(ProviderOutput {
            text,
            reported_model: Some("mock-model".into()),
            usage: Usage::default(),
            stop_reason: Some("stop".into()),
            agent_version: Some("mock 1".into()),
            sent: BTreeMap::new(),
        })
    }
}

/// An `Llm` whose only connection `mock` replies with `replies` in order (cache off).
pub fn mock_llm(replies: &[serde_json::Value]) -> (Arc<Llm>, Arc<Mock>) {
    mock_with(Mock {
        replies: Mutex::new(replies.iter().map(|v| v.to_string()).collect()),
        ..Mock::default()
    })
}

pub fn quota_llm() -> (Arc<Llm>, Arc<Mock>) {
    mock_with(Mock {
        quota: true,
        ..Mock::default()
    })
}

pub fn failing_llm() -> (Arc<Llm>, Arc<Mock>) {
    mock_with(Mock {
        fail: true,
        ..Mock::default()
    })
}

fn mock_with(m: Mock) -> (Arc<Llm>, Arc<Mock>) {
    let mock = Arc::new(m);
    let mut reg = Registry::default();
    reg.insert(Connection {
        config: ConnectionConfig {
            name: "mock".into(),
            default_model: Some("m".into()),
            ..ConnectionConfig::default()
        },
        kind: ProviderKind::OpenAiCompatible,
        key_policy: KeyPolicy::None,
        provider: mock.clone(),
    });
    let llm = Llm::new(reg, Cache::new(std::env::temp_dir(), CacheMode::Off));
    (Arc::new(llm), mock)
}

/// A model call object with every slot key (null when not given), as the schema demands.
pub fn call(intent: &str, slots: &[(&str, &str)]) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("intent".into(), intent.into());
    for s in atlas_ask::intent::all_slot_names() {
        m.insert(s.into(), serde_json::Value::Null);
    }
    for (k, v) in slots {
        m.insert((*k).into(), (*v).into());
    }
    serde_json::Value::Object(m)
}
