//! The atlas-core adapter against the real snapshots (feature `core`). Skips when the snapshots
//! are absent (`$RARE_ATLAS_DATA/cache/{atlas,graph}.snapshot`, default: the checkout's `data/`).
#![cfg(feature = "core")]

use std::path::PathBuf;
use std::sync::Arc;

use atlas_contrib::model::{CheckStatus, ContributorInput, FoundVia, NodeInput, SubjectKind};
use atlas_contrib::{Contrib, ContribConfig, ContributionKind, CoreLookup, GraphLookup, Submission};
use atlas_core::{Atlas, Graph};

struct Loaded {
    atlas: Atlas,
    graph: Graph,
}

fn load() -> Option<Arc<Loaded>> {
    let data = std::env::var("RARE_ATLAS_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"));
    let atlas_path = data.join("cache").join("atlas.snapshot");
    let graph_path = data.join("cache").join("graph.snapshot");
    if !atlas_path.is_file() || !graph_path.is_file() {
        eprintln!("skipping: no snapshots under {}", data.display());
        return None;
    }
    let (atlas, _) = atlas_core::snapshot::load(&atlas_path).ok()?;
    let (graph, _) = atlas_core::snapshot::load_graph(&graph_path).ok()?;
    Some(Arc::new(Loaded { atlas, graph }))
}

#[tokio::test]
async fn resolves_and_detects_curated_duplicates_on_real_data() {
    let Some(src) = load() else { return };
    let lookup = CoreLookup::new(src.clone(), |s| &s.atlas, |s| Some(&s.graph));

    // A gene by symbol, exact.
    let genes = lookup.search("STXBP1", &["gene"], 3);
    assert!(
        genes.first().is_some_and(|h| h.exact && h.label == "STXBP1"),
        "{genes:?}"
    );

    // An organisation serving a condition, else a study of one (whichever the snapshot has).
    let pick = |relation: &str| src.graph.edges().iter().find(|e| e.relation.as_str() == relation);
    let (edge, kind, subject_kind) = match pick("serves_condition") {
        Some(e) => (e, "organisation", SubjectKind::PatientGroup),
        None => (
            pick("studies_condition").expect("curated condition links"),
            "study",
            SubjectKind::Study,
        ),
    };
    let org = lookup.node(&edge.from).expect("subject node");
    assert_eq!(org.kind, kind);
    let by_name = lookup.search(&org.label, &[kind], 5);
    assert!(by_name.iter().any(|h| h.id == org.id && h.exact), "{by_name:?}");
    assert!(lookup.edges_of(&org.id).iter().any(|e| e.to == edge.to));
    let target = lookup.node(&edge.to).expect("condition node");
    assert_eq!(target.kind, "disease");

    // Suggesting that same link again is a blocking duplicate of the curated edge.
    let s = Contrib::new(ContribConfig::for_tests(), Arc::new(lookup)).unwrap();
    let sub = Submission {
        kind: ContributionKind::NewLink,
        kind_other: None,
        subject_kind_other: None,
        relationship: None,
        relationship_other: None,
        data_source: None,
        subject_kind: Some(subject_kind),
        subject: NodeInput {
            id: None,
            label: Some(org.label.clone()),
        },
        target: Some(NodeInput {
            id: Some(target.id.clone()),
            label: None,
        }),
        edge: None,
        statement: "Test: the group serves this condition.".into(),
        evidence_url: None,
        quote: Some("placeholder quote".into()),
        contact_url: None,
        found_via: FoundVia::default(),
        lang: None,
        contributor: ContributorInput {
            contact: Some("parent@example.invalid".into()),
            ..Default::default()
        },
    };
    let c = s.submit(sub, None).unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    let report = c.checks.unwrap();
    assert_eq!(report.subject.as_ref().map(|h| h.id.as_str()), Some(org.id.as_str()));
    let dup = report.find("duplicate").unwrap();
    assert_eq!(
        (dup.status, dup.code.as_str(), dup.blocking),
        (CheckStatus::Fail, "duplicate_curated", true)
    );
}
