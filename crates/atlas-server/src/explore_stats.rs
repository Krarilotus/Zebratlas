//! Snapshot metadata counts, not integrity validation or runtime reasoning claims.
use atlas_core::graph::RecordWithhold;
use atlas_core::node::{NodeKey, NodeKind};
use atlas_core::provenance::SourceEntity;
use atlas_core::{Atlas, Graph};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

type ScanResult<T> = Result<Arc<T>, String>;
type ScanReceiver<T> = watch::Receiver<Option<ScanResult<T>>>;
enum ScanState<T> {
    Empty,
    Running(ScanReceiver<T>),
    Ready(Arc<T>),
    Failed { error: String, retry_at: Instant },
}

/// The worker owns initialization independently of any HTTP request. Dropping
/// every subscriber cannot restart an already running blocking scan. Failures
/// have a cooldown, after which one caller may start one coordinated retry.
pub struct DurableScanCache<T> {
    state: Arc<Mutex<ScanState<T>>>,
    retry_after: Duration,
}
impl<T> Clone for DurableScanCache<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            retry_after: self.retry_after,
        }
    }
}
impl<T: Send + Sync + 'static> DurableScanCache<T> {
    pub fn new(retry_after: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(ScanState::Empty)),
            retry_after,
        }
    }
    pub async fn get_or_try_init<F>(&self, initializer: F) -> ScanResult<T>
    where
        F: FnOnce() -> Result<T, String> + Send + 'static,
    {
        let mut receiver = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "Source statistics cache unavailable".to_owned())?;
            match &*state {
                ScanState::Ready(value) => return Ok(value.clone()),
                ScanState::Running(receiver) => receiver.clone(),
                ScanState::Failed { error, retry_at } if Instant::now() < *retry_at => return Err(error.clone()),
                _ => {
                    let (sender, receiver) = watch::channel(None);
                    *state = ScanState::Running(receiver.clone());
                    let worker_state = self.state.clone();
                    let retry_after = self.retry_after;
                    // This task is deliberately detached from the subscriber.
                    // The blocking worker itself never holds the cache mutex.
                    tokio::spawn(async move {
                        let result = match tokio::task::spawn_blocking(initializer).await {
                            Ok(result) => result.map(Arc::new),
                            Err(_) => Err("Source statistics worker failed".to_owned()),
                        };
                        if let Ok(mut state) = worker_state.lock() {
                            *state = match &result {
                                Ok(value) => ScanState::Ready(value.clone()),
                                Err(error) => ScanState::Failed {
                                    error: error.clone(),
                                    retry_at: Instant::now() + retry_after,
                                },
                            };
                        }
                        sender.send_replace(Some(result));
                    });
                    receiver
                }
            }
        };
        loop {
            let result = { receiver.borrow_and_update().clone() };
            if let Some(result) = result {
                return result;
            }
            receiver
                .changed()
                .await
                .map_err(|_| "Source statistics worker unavailable".to_owned())?;
        }
    }
}

#[derive(Default, Serialize)]
struct SourceCounts {
    total: usize,
    missing_url: usize,
    missing_sha256: usize,
    missing_retrieved_at: usize,
}
impl SourceCounts {
    fn add(&mut self, e: &SourceEntity) {
        self.total += 1;
        self.missing_url += usize::from(e.url.trim().is_empty());
        self.missing_sha256 += usize::from(e.sha256.as_deref().is_none_or(|s| s.trim().is_empty()));
        self.missing_retrieved_at += usize::from(e.retrieved_at.as_deref().is_none_or(|s| s.trim().is_empty()));
    }
}
#[derive(Default, Serialize)]
struct RecordCounts {
    total: usize,
    missing_effective_url: usize,
    zero_sha256: usize,
    missing_effective_retrieved_at: usize,
    invalid_source_reference: usize,
}
impl RecordCounts {
    fn add(&mut self, r: &atlas_core::graph::SourceRecord, source: Option<&SourceEntity>) {
        self.total += 1;
        self.zero_sha256 += usize::from(r.sha256 == [0; 32]);
        self.invalid_source_reference += usize::from(source.is_none());
        self.missing_effective_url += usize::from(
            !r.url.as_deref().is_some_and(|s| !s.trim().is_empty())
                && !source.is_some_and(|e| !e.url.trim().is_empty()),
        );
        self.missing_effective_retrieved_at += usize::from(
            !r.fetched_at.as_deref().is_some_and(|s| !s.trim().is_empty())
                && !source
                    .and_then(|e| e.retrieved_at.as_deref())
                    .is_some_and(|s| !s.trim().is_empty()),
        );
    }
}
fn kinds() -> BTreeMap<&'static str, usize> {
    ["observed", "inferred", "extracted", "hypothesis"]
        .into_iter()
        .map(|s| (s, 0))
        .collect()
}

/// Caller caches this value for the immutable loaded snapshot pair.
/// Missing means absent/blank metadata; neither URLs nor hashes are verified here.
pub fn summarize(atlas: &Atlas, graph: &Graph) -> Value {
    let data = graph.data();
    let mut nodes = BTreeMap::<&str, usize>::new();
    nodes.insert(
        "disease",
        atlas
            .diseases()
            .iter()
            .map(|d| d.id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
    );
    nodes.insert(
        "gene",
        atlas.genes().iter().map(|g| g.id()).collect::<BTreeSet<_>>().len(),
    );
    nodes.insert(
        "phenotype",
        atlas
            .hpo
            .terms()
            .iter()
            .map(|t| t.id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
    );
    let mut visible_nodes = BTreeMap::<&str, usize>::new();
    let mut referenced_records = vec![false; data.records.len()];
    let mut blocked_ids = BTreeSet::new();
    let mut withheld_nodes = 0;
    for kind in [
        NodeKind::Study,
        NodeKind::Grant,
        NodeKind::Paper,
        NodeKind::Person,
        NodeKind::Organisation,
        NodeKind::Asset,
    ] {
        let mut seen = BTreeSet::new();
        let mut visible = 0;
        for idx in 0..graph.node_count(kind) as u32 {
            let key = NodeKey { kind, idx };
            let node = graph.node_ref(key);
            if !seen.insert(node.id.clone()) {
                continue;
            }
            if graph.node_withheld(key).is_some() {
                withheld_nodes += 1;
                blocked_ids.insert(node.id);
            } else {
                visible += 1;
                for &r in graph.node_records(key) {
                    if let Some(flag) = referenced_records.get_mut(r as usize) {
                        *flag = true;
                    }
                }
            }
        }
        nodes.insert(kind.as_str(), seen.len());
        visible_nodes.insert(kind.as_str(), visible);
    }
    let mut edge_kinds = kinds();
    let mut visible_edge_kinds = kinds();
    let mut relations = BTreeMap::new();
    let mut withheld_edges = 0;
    for edge in graph.edges() {
        *edge_kinds.entry(edge.kind.as_str()).or_default() += 1;
        *relations.entry(edge.relation.as_str()).or_insert(0usize) += 1;
        if blocked_ids.contains(&edge.from)
            || blocked_ids.contains(&edge.to)
            || graph.records_withheld(&edge.records).is_some()
        {
            withheld_edges += 1;
        } else {
            *visible_edge_kinds.entry(edge.kind.as_str()).or_default() += 1;
            for &r in &edge.records {
                if let Some(flag) = referenced_records.get_mut(r as usize) {
                    *flag = true;
                }
            }
        }
    }
    let mut atlas_gene_edges = 0;
    let mut atlas_phenotype_edges = 0;
    let mut atlas_absent_phenotype_edges = 0;
    for disease in atlas.diseases() {
        atlas_gene_edges += disease
            .genes
            .iter()
            .map(|l| atlas_journeys::gene_id(atlas, l))
            .collect::<BTreeSet<_>>()
            .len();
        atlas_phenotype_edges += disease.phenotypes.len();
        atlas_absent_phenotype_edges += disease.excluded.len();
    }
    let mut all_records = RecordCounts::default();
    let mut visible_records = RecordCounts::default();
    let mut visible_source_flags = vec![false; data.provenance.entities.len()];
    let mut withheld_records = 0;
    for (index, record) in data.records.iter().enumerate() {
        let source = data.provenance.entities.get(record.entity.0 as usize);
        all_records.add(record, source);
        withheld_records += usize::from(graph.records_withheld(&[index as u32]).is_some());
        if referenced_records[index] {
            visible_records.add(record, source);
            if let Some(flag) = visible_source_flags.get_mut(record.entity.0 as usize) {
                *flag = true;
            }
        }
    }
    let mut atlas_sources = SourceCounts::default();
    for source in &atlas.provenance.entities {
        atlas_sources.add(source);
    }
    let mut graph_sources = SourceCounts::default();
    let mut visible_sources = SourceCounts::default();
    for (index, source) in data.provenance.entities.iter().enumerate() {
        graph_sources.add(source);
        if visible_source_flags[index] {
            visible_sources.add(source);
        }
    }
    let node_total: usize = nodes.values().sum();
    json!({
        "scope":"loaded_snapshots",
        "counts":{
            "total_nodes":node_total,"nodes_by_kind":nodes,
            "connected_edges":graph.edges().len(),"connected_edge_kinds":edge_kinds,"connected_edges_by_relation":relations,
            "atlas_gene_edges":atlas_gene_edges,"atlas_phenotype_edges":atlas_phenotype_edges,"atlas_absent_phenotype_edges":atlas_absent_phenotype_edges,
            "initiatives":data.initiatives.len(),"identity_merges":data.identity_merges.len()
        },
        "provenance":{"atlas_sources":atlas_sources,"connected_sources":graph_sources,"connected_records":all_records},
        "serving_visible":{
            "scope":"connected_nodes_and_edges_passing_loaded_withholding",
            "connected_nodes_by_kind":visible_nodes,"connected_edges":graph.edges().len()-withheld_edges,
            "connected_edge_kinds":visible_edge_kinds,"referenced_records":visible_records,"referenced_sources":visible_sources
        },
        "withheld":{"connected_nodes":withheld_nodes,"connected_edges":withheld_edges,"connected_records":withheld_records},
        "semantics":{
            "missing_counts_denominator":"Each object total; referenced metadata counts cover records used by eligible connected nodes or edges.",
            "effective_record_metadata":"Nonblank record URL/retrieval time, otherwise nonblank source entity URL/retrieval time.",
            "atlas_scope":"All loaded ontology nodes and associations, including retired conditions; withholding counts apply only to connected graph.",
            "node_count_scope":"Distinct canonical IDs per node kind; initiatives and identity merges are separate records, not node kinds.",
            "edge_count_scope":"Stored connected assertions and separate atlas associations; these scopes can overlap and are not summed.",
            "checks":"Metadata presence only. No hash verification, URL validation, source reliability score, completeness guarantee, or runtime reasoning."
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::graph::{GraphData, GraphEdge, LinkLevel, Paper, Quarantine, RecordHash, Relation, SourceRecord};
    use atlas_core::node::EdgeKind;
    use atlas_core::provenance::{ActivityIdx, EntityIdx, Locator, Provenance};
    #[tokio::test]
    async fn cancelled_subscribers_do_not_restart_a_running_scan() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let cache = DurableScanCache::new(Duration::from_secs(5));
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first_cache = cache.clone();
        let first_calls = calls.clone();
        let first = tokio::spawn(async move {
            first_cache
                .get_or_try_init(move || {
                    first_calls.fetch_add(1, Ordering::SeqCst);
                    started_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                    Ok(42usize)
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .unwrap()
            .unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        // Model repeated disconnected HTTP requests while the same blocking
        // source scan still owns its immutable snapshot references.
        for _ in 0..5 {
            let cache = cache.clone();
            let calls = calls.clone();
            let waiter = tokio::spawn(async move {
                cache
                    .get_or_try_init(move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(99)
                    })
                    .await
            });
            tokio::task::yield_now().await;
            waiter.abort();
            assert!(waiter.await.unwrap_err().is_cancelled());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release_tx.send(()).unwrap();
        let next_calls = calls.clone();
        let value = tokio::time::timeout(
            Duration::from_secs(1),
            cache.get_or_try_init(move || {
                next_calls.fetch_add(1, Ordering::SeqCst);
                Ok(99)
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(*value, 42);
        assert_eq!(
            *cache
                .get_or_try_init(|| panic!("a completed scan must remain cached"))
                .await
                .unwrap(),
            42
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_scan_has_a_cooldown_and_one_coordinated_retry() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let cache = DurableScanCache::<usize>::new(Duration::from_millis(30));
        let calls = Arc::new(AtomicUsize::new(0));
        let first_calls = calls.clone();
        assert_eq!(
            cache
                .get_or_try_init(move || {
                    first_calls.fetch_add(1, Ordering::SeqCst);
                    Err("synthetic scan failure".into())
                })
                .await
                .unwrap_err(),
            "synthetic scan failure"
        );
        let second_calls = calls.clone();
        assert!(
            cache
                .get_or_try_init(move || {
                    second_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(42)
                })
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(40)).await;
        let mut waiters = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let calls = calls.clone();
            waiters.push(tokio::spawn(async move {
                cache
                    .get_or_try_init(move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(42)
                    })
                    .await
            }));
        }
        for waiter in waiters {
            assert_eq!(*waiter.await.unwrap().unwrap(), 42);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn worker_panic_notifies_waiters_without_leaking_panic_text_or_sticking_running() {
        let cache = DurableScanCache::<usize>::new(Duration::from_millis(30));
        let error = cache
            .get_or_try_init(|| panic!("synthetic private panic text"))
            .await
            .unwrap_err();
        assert_eq!(error, "Source statistics worker failed");
        assert!(!error.contains("private"));
        assert!(cache.get_or_try_init(|| Ok(42)).await.is_err());
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(*cache.get_or_try_init(|| Ok(42)).await.unwrap(), 42);
    }

    #[test]
    fn counts_snapshot_and_visible_provenance_separately() {
        let atlas = Atlas::new(vec![], Default::default(), Provenance::default(), vec![]);
        let source = SourceEntity {
            url: "https://example.org/source".into(),
            retrieved_at: Some("2026-10-04T00:00:00Z".into()),
            ..Default::default()
        };
        let record = |id: &str, hash| SourceRecord {
            entity: EntityIdx(0),
            locator: Locator::Line(1),
            id: id.into(),
            url: None,
            fetched_at: None,
            hash: RecordHash::JsonLine,
            sha256: hash,
        };
        let paper = |id: &str, r| Paper {
            id: id.into(),
            title: id.into(),
            journal: String::new(),
            year: None,
            doi: None,
            review: false,
            records: vec![r],
        };
        let edge = |from: &str, kind, r| GraphEdge {
            from: from.into(),
            to: "HGNC:1".into(),
            relation: Relation::AboutGene,
            kind,
            level: LinkLevel::Text,
            reason: String::new(),
            activity: ActivityIdx(0),
            records: vec![r],
        };
        let graph = Graph::new(GraphData {
            provenance: Provenance {
                entities: vec![source],
                ..Default::default()
            },
            records: vec![record("PMID:1", [1; 32]), record("PMID:2", [0; 32])],
            papers: vec![paper("PMID:1", 0), paper("PMID:2", 1)],
            edges: vec![
                edge("PMID:1", EdgeKind::Inferred, 0),
                edge("PMID:2", EdgeKind::Observed, 1),
            ],
            quarantine: vec![Quarantine {
                record: 1,
                reason: "fixture".into(),
            }],
            ..Default::default()
        });
        let stats = summarize(&atlas, &graph);
        assert_eq!(stats["counts"]["nodes_by_kind"]["paper"], 2);
        assert_eq!(stats["counts"]["connected_edge_kinds"]["inferred"], 1);
        assert_eq!(stats["serving_visible"]["connected_edges"], 1);
        assert_eq!(stats["serving_visible"]["referenced_records"]["total"], 1);
        assert_eq!(stats["withheld"]["connected_nodes"], 1);
        assert_eq!(stats["withheld"]["connected_records"], 1);
        assert_eq!(stats["provenance"]["connected_records"]["zero_sha256"], 1);
        assert_eq!(stats["provenance"]["connected_records"]["missing_effective_url"], 0);
        assert_eq!(stats["serving_visible"]["referenced_sources"]["missing_sha256"], 1);
        assert_eq!(
            stats["serving_visible"]["referenced_records"]["missing_effective_retrieved_at"],
            0
        );
    }
    #[test]
    fn empty_snapshots_have_zero_denominators() {
        let atlas = Atlas::new(vec![], Default::default(), Provenance::default(), vec![]);
        let result = summarize(&atlas, &Graph::default());
        assert_eq!(result["counts"]["total_nodes"], 0);
        assert_eq!(result["provenance"]["connected_records"]["total"], 0);
        assert_eq!(result["counts"]["connected_edge_kinds"]["hypothesis"], 0);
    }
}
