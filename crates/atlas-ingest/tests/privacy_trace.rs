//! D43 over the real graph snapshot (read-only): a synthetic person is added to the real graph
//! data, traced by ORCID and by name + affiliation, then suppressed; nothing else may change.
//! Uses `$RARE_ATLAS_GRAPH_SNAPSHOT` when set, otherwise `$RARE_ATLAS_DATA/cache/graph.snapshot`.
//! Skipped when that snapshot is missing. Prints counts only.

use std::path::PathBuf;

use atlas_core::graph::{GraphEdge, LinkLevel, Person, PersonSource, RecordHash, Relation, SourceRecord};
use atlas_core::node::EdgeKind;
use atlas_core::provenance::{ActivityIdx, Locator, SourceEntity};
use atlas_core::withhold::{Salt, Scope, SuppressionEntry, SuppressionFile, Withhold, apply_suppression};
use atlas_core::{Graph, snapshot};
use atlas_ingest::trace::{TraceOptions, TraceQuery, trace};

// Obvious placeholders: not a real ORCID holder, name or institute.
const ORCID: &str = "0000-0000-0000-0000";
const NAME: &str = "Synthetic Testperson Zz";
const AFF: &str = "Example Placeholder Institute";

fn snapshot_path() -> Option<PathBuf> {
    let data = std::env::var_os("RARE_ATLAS_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"));
    let p = atlas_ingest::graph::snapshot_path(&data);
    if !p.exists() {
        eprintln!("skipped: {} missing", p.display());
    }
    p.exists().then_some(p)
}

fn person(id: &str, rec: u32) -> Person {
    Person {
        id: id.into(),
        name: NAME.into(),
        name_variants: vec![],
        orcids: vec![ORCID.into()],
        affiliations: vec![format!("Dept. of Testing, {AFF}")],
        source: PersonSource::Resolved,
        genes: vec![],
        communities: vec![],
        cross_community: false,
        matched_by: vec!["orcid".into()],
        merge_basis: vec![],
        records: vec![rec],
    }
}

fn edge(from: &str, relation: Relation, to: &str, rec: u32) -> GraphEdge {
    GraphEdge {
        from: from.into(),
        relation,
        to: to.into(),
        kind: EdgeKind::Observed,
        level: LinkLevel::Curated,
        reason: "synthetic fixture".into(),
        activity: ActivityIdx(0),
        records: vec![rec],
    }
}

#[test]
fn trace_and_suppress_synthetic_person_on_real_graph() {
    let Some(snap) = snapshot_path() else { return };
    let (graph, _) = snapshot::load_graph(&snap).expect("graph snapshot");
    let mut data = graph.into_data();
    let (people0, edges0, papers0) = (data.people.len(), data.edges.len(), data.papers.len());
    assert!(people0 > 0 && papers0 > 0, "real graph has people and papers");

    // Synthetic cache + release under a temp data dir.
    let dir = std::env::temp_dir().join(format!("atlas-trace-{}", std::process::id()));
    let cache = dir.join("data/cache/people");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(dir.join("release/vtest")).unwrap();
    let rec_json = serde_json::json!({"schema": "people.synthetic", "version": 1, "header": {}, "records": [
        {"id": "unrelated", "name": "Nobody Placeholder"},
        {"id": format!("ORCID:{ORCID}"), "name": NAME, "orcid": ORCID}
    ]});
    std::fs::write(cache.join("synthetic.json"), rec_json.to_string()).unwrap();
    std::fs::write(
        dir.join("release/vtest/nodes.jsonl"),
        format!("{{\"id\":\"other\"}}\n{{\"id\":\"ORCID:{ORCID}\",\"kind\":\"person\"}}\n"),
    )
    .unwrap();

    let entity = data.provenance.add_entity(SourceEntity {
        id: "source:cache/people/synthetic.json".into(),
        url: "synthetic test fixture".into(),
        file: "cache/people/synthetic.json".into(),
        ..SourceEntity::default()
    });
    data.records.push(SourceRecord {
        entity,
        locator: Locator::Record("records[1]".into()),
        id: format!("ORCID:{ORCID}"),
        url: None,
        fetched_at: None,
        hash: RecordHash::CanonicalJson,
        sha256: [0; 32],
    });
    let rec = (data.records.len() - 1) as u32;
    let a = format!("ORCID:{ORCID}");
    let b = "person:synthetic-testperson-zz".to_string();
    let paper = data.papers[0].id.clone();
    data.people.push(person(&a, rec));
    let mut alias = person(&b, rec);
    alias.orcids.clear();
    data.people.push(alias);
    data.edges.push(edge(&a, Relation::AuthorOf, &paper, rec));
    data.edges.push(edge(&b, Relation::SameAs, &a, rec));
    let g = Graph::new(data);

    let salt = Salt::new("test-salt");
    let mut opts = TraceOptions::for_data(&dir.join("data"));
    opts.cache_dirs = vec!["cache/people".into()];
    let by_orcid = trace(
        &dir.join("data"),
        &g,
        &TraceQuery {
            orcid: Some(format!("https://orcid.org/{ORCID}")),
            ..TraceQuery::default()
        },
        &salt,
        &opts,
    );
    let c = by_orcid.counts();
    eprintln!("synthetic trace counts: {}", serde_json::to_string(&c).unwrap());
    assert_eq!(c.nodes, 2, "ORCID node + same_as alias");
    assert_eq!(c.edges, 2, "author_of + same_as");
    assert_eq!(c.graph_records, 1);
    assert_eq!(c.cache_records, 1, "only records[1] of the synthetic cache");
    assert_eq!(c.release_lines, 1);
    assert!(by_orcid.derivation.iter().any(|d| d.contains("synthetic test fixture")));

    let by_name = trace(
        &dir.join("data"),
        &g,
        &TraceQuery {
            name: Some("synthetic  TESTPERSON zz".into()),
            affiliation: Some("example placeholder institute".into()),
            ..TraceQuery::default()
        },
        &salt,
        &opts,
    );
    let names: Vec<_> = by_name.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(names, [a.as_str(), b.as_str()]);

    // Approve: suppression entry from the trace keys; nothing else changes.
    let file = SuppressionFile {
        entries: vec![SuppressionEntry {
            id: "sup_synthetic".into(),
            keys: by_orcid.keys.clone(),
            scope: Scope::All,
            reason: "gdpr_art17_erasure".into(),
            date: "2026-10-04T12:00:00Z".into(),
            reviewer: "agent:reviewer/tester".into(),
            request: Some("pr_synthetic".into()),
            salt_id: salt.id().into(),
        }],
        ..SuppressionFile::new()
    };
    let w = Withhold::from_lists(salt.clone(), Some(&serde_json::to_vec(&file).unwrap()), None).unwrap();
    assert!(w.node_id(&g, &a).is_some() && w.node_id(&g, &b).is_some());
    let e = g.incident(&paper).find(|i| i.other == a).unwrap().idx;
    assert!(w.edge(&g, e).is_some(), "edge to a suppressed person is withheld");
    let mut data = g.into_data();
    let applied = apply_suppression(&mut data, &w);
    eprintln!("applied: {}", serde_json::to_string(&applied).unwrap());
    assert_eq!(applied.people, 2);
    assert_eq!(applied.edges, 2);
    assert_eq!(data.people.len(), people0, "every real person kept");
    assert_eq!(data.edges.len(), edges0, "every real edge kept");
    assert_eq!(data.papers.len(), papers0);
    assert_eq!(
        data.records[rec as usize].id, "suppressed:sup_synthetic",
        "record id redacted"
    );
    // Re-applying (a re-ingest of the same caches) removes the same items again.
    let g2 = Graph::new(data);
    assert!(g2.node(&a).is_none() && g2.node(&b).is_none());
    std::fs::remove_dir_all(&dir).ok();
}
