//! Reader tests over small fixtures: each research cache becomes assets / grants / edges with
//! provenance, licence and an access route; excluded records are counted, never shown.

use atlas_core::graph::{AssetKind, Graph, LicenceClass, Relation};
use atlas_core::integrity;
use serde_json::json;

use super::fixtures::{self, CONDITION, GENE, TempData};
use super::{claims, funding, outcomes, programmes, samples};

fn edge<'g>(g: &'g Graph, from: &str, rel: Relation, to: &str) -> Option<&'g atlas_core::graph::GraphEdge> {
    g.edges()
        .iter()
        .find(|e| e.from == from && e.relation == rel && e.to == to)
}

#[test]
fn programmes_and_designations() {
    let atlas = fixtures::atlas();
    let d = TempData::new();
    d.envelope(
        programmes::PIPELINES,
        "pipelines.evidence",
        json!([
            {"id": "pipelines:X-1:2026", "program_id": "program:Acme:X-1", "record_type": "program", "sponsor": "Acme Bio",
             "genes": ["STXBP1"], "program_name": "X-1", "modality": "ASO", "stage": "Phase 1/2", "excluded": false,
             "url": "https://acme.example/x1", "fetched_at": "2026-10-03T00:00:00+00:00", "trial_ids": ["NCT00000001"]},
            {"id": "pipelines:fin", "record_type": "financing", "program_ids": ["program:Acme:X-1"], "round": "Series A",
             "amount": "50000000", "currency": "USD", "source_date": "2026-01-01", "excluded": false, "genes": []},
            {"id": "pipelines:gone", "record_type": "program", "excluded": true, "exclusion_reason": "site terms", "genes": ["STXBP1"]}
        ]),
    );
    d.envelope(
        programmes::REGULATORY,
        "regulatory.signals",
        json!([
            {"id": "EMA:od-1", "record_type": "orphan_designation", "source": "EMA", "url": "https://ema.example/od-1",
             "active_substance": "Drug Y", "intended_use": "Treatment of DEE4", "orphan_designation_status": "Positive",
             "sponsor": "Acme Bio",
             "mapping_candidates": [
                {"entity_type": "disease", "id": CONDITION, "basis": "exact_name_mention", "matched_name": "STXBP1 encephalopathy"},
                {"entity_type": "disease", "id": "MONDO:0005027", "basis": "exact_name_mention", "matched_name": "epilepsy"},
                {"entity_type": "disease", "id": CONDITION, "basis": "exact_synonym_mention", "matched_name": "DEE"},
                {"entity_type": "gene", "id": GENE, "basis": "explicit_case_sensitive_gene_token", "matched_name": "STXBP1"}]}
        ]),
    );
    let mut b = fixtures::builder(&atlas);
    programmes::ingest(&mut b, d.path()).unwrap();
    super::licences(&mut b);
    let g = Graph::new(b.data);
    let p = g.asset(g.node("program:Acme:X-1").unwrap().idx);
    assert_eq!(p.kind, AssetKind::Programme);
    assert_eq!(p.fact("modality"), Some("ASO"));
    assert!(p.fact("financing").unwrap().contains("Series A"));
    assert_eq!(p.holder.as_deref(), Some("org:acme-bio"));
    assert!(edge(&g, "program:Acme:X-1", Relation::Targets, GENE).is_some());
    assert!(g.node("pipelines:gone").is_none());
    assert_eq!(g.coverage_of("pipelines").unwrap().excluded, 1);
    // regulatory: only the rare condition, named with >= 6 characters
    assert!(edge(&g, "EMA:od-1", Relation::StudiedFor, CONDITION).is_some());
    assert!(edge(&g, "EMA:od-1", Relation::StudiedFor, "MONDO:0005027").is_none());
    assert!(edge(&g, "EMA:od-1", Relation::RelatedTo, GENE).is_some());
    // one sponsor node, held_by from both
    assert!(edge(&g, "EMA:od-1", Relation::HeldBy, "org:acme-bio").is_some());
    let report = integrity::check(&atlas, &g);
    for c in &report.contracts {
        assert!(c.passed, "{} failed: {:?}", c.id, report.violations);
    }
    assert_eq!(g.licence_class(&g.asset(0).records), LicenceClass::Unknown);
}

#[test]
fn samples_models_registries() {
    let atlas = fixtures::atlas();
    let d = TempData::new();
    d.envelope(
        samples::ORG_ASSETS,
        "org-assets.assets",
        json!([
            {"id": "orgasset:bio", "reporting_org_id": "atlasorg:x", "owner_org_id": null, "owner_name": "STXBP1 Foundation",
             "owner_role": "operator_or_resource_provider", "kind": "biobank", "name": "Biorepository", "genes": ["STXBP1"],
             "access": {"url": "https://found.example/res", "instructions": "See the official source."},
             "url": "https://found.example/res", "excluded": false, "ids": []},
            {"id": "orgasset:dup", "kind": "registry", "name": "Registry", "owner_name": "unknown", "genes": ["STXBP1"],
             "access": {"url": "https://found.example/reg"}, "url": "https://found.example/reg", "excluded": false},
            {"id": "orgasset:no", "excluded": true, "reason": "not an asset"}
        ]),
    );
    d.envelope(
        samples::ORG_OVERLAPS,
        "org-assets.overlaps",
        json!([{"id": "ov:1", "asset_ids": ["orgasset:bio", "orgasset:dup"], "relation": "same_published_id", "basis": "same NCT"}]),
    );
    d.envelope(
        samples::ERDRI,
        "directories.erdri",
        json!([
            {"id": "erdri:1", "name": "DEE registry", "homepage": "https://reg.example", "human_url": "https://erdri.example",
             "orpha_codes": [{"code": CONDITION}], "relevance": {"relevant": true}, "enabled": true, "registry_types": ["Patient driven"]},
            {"id": "erdri:2", "name": "Other", "relevance": {"relevant": false}, "orpha_codes": []}
        ]),
    );
    d.envelope(
        "cache/models/STXBP1.json",
        "models.assets",
        json!([
            {"id": "a1", "source": "Cellosaurus", "gene": "STXBP1", "url": "https://www.cellosaurus.org/CVCL_1", "asset_type": "cell_model",
             "model_id": "Cellosaurus:CVCL_1", "label": "iPSC-STXBP1", "organism": {"label": "Homo sapiens"}, "excluded": false,
             "registries": [{"category": "Cell line collections (Providers)", "database": "EBiSC", "accession": "X1", "url": "https://ebisc.example/X1"}],
             "gene_link": {"human_gene": GENE, "kind": "observed"}, "diseases": []},
            {"id": "a2", "source": "Alliance", "gene": "STXBP1", "url": "https://zfin.example/F1", "asset_type": "model_organism",
             "model_id": "ZFIN:F1", "label": "stxbp1b<sup>s1/s1</sup>", "excluded": false,
             "gene_link": {"human_gene": GENE, "kind": "inferred", "basis": "Alliance orthology prediction"}},
            {"id": "a3", "source": "Alliance", "gene": "STXBP1", "model_id": "ZFIN:F1", "label": "stxbp1b", "excluded": false, "url": "https://zfin.example/F1",
             "gene_link": {"human_gene": GENE, "kind": "inferred"}},
            {"id": "a4", "excluded": true, "model_id": "ZFIN:F9"}
        ]),
    );
    let mut b = fixtures::builder(&atlas);
    samples::ingest(&mut b, d.path()).unwrap();
    super::licences(&mut b);
    let g = Graph::new(b.data);
    let bio = g.asset(g.node("orgasset:bio").unwrap().idx);
    assert_eq!(bio.kind, AssetKind::Biobank);
    assert_eq!(bio.holder_name.as_deref(), Some("STXBP1 Foundation"));
    assert!(edge(&g, "orgasset:bio", Relation::ResourceFor, GENE).is_some());
    assert!(edge(&g, "orgasset:bio", Relation::CandidateSameAs, "orgasset:dup").is_some());
    assert!(g.node("orgasset:no").is_none());
    assert!(edge(&g, "erdri:1", Relation::ResourceFor, CONDITION).is_some());
    assert!(g.node("erdri:2").is_none());
    let cell = g.asset(g.node("CVCL:1").unwrap().idx);
    assert_eq!(cell.holder_name.as_deref(), Some("EBiSC"));
    assert_eq!(cell.kind, AssetKind::CellLine);
    assert_eq!(cell.access.route, "repository_order");
    assert_eq!(cell.access.url.as_deref(), Some("https://ebisc.example/X1"));
    let fish = g.asset(g.node("ZFIN:F1").unwrap().idx);
    assert_eq!(fish.label, "stxbp1b s1/s1");
    assert_eq!(
        fish.records.len(),
        2,
        "two annotations of one model merge into one asset"
    );
    let e = edge(&g, "ZFIN:F1", Relation::ModelOf, GENE).unwrap();
    assert_eq!(e.kind, atlas_core::node::EdgeKind::Inferred);
    assert_eq!(g.licence_class(&cell.records), LicenceClass::Open);
    let report = integrity::check(&atlas, &g);
    assert!(report.passed, "{:?}", report.violations);
}

#[test]
fn outcomes_funding_claims() {
    let atlas = fixtures::atlas();
    let d = TempData::new();
    d.envelope(
        outcomes::FILE,
        "outcomes.resources",
        json!([{"id": "outcomes:diary", "name": "Seizure diary", "kind": "outcome_measure", "source": "FDA COA Compendium",
                "url": "https://fda.example/coa", "facts": {"concept": "Seizure frequency"},
                "access": {"level": "public_metadata", "route": "https://fda.example/route"},
                "mappings": [{"basis": "explicit_gene_symbol", "id": GENE, "gene_symbol": "STXBP1"},
                             {"condition_text": "DEE4", "candidate_ids": [CONDITION, "MONDO:0005027"]}]}]),
    );
    d.envelope(
        funding::CALLS,
        "funders.calls",
        json!([
            {"id": "funders:c1", "funder": "STXBP1 Foundation", "title": "Grants Program", "status": "included",
             "application_status": "rolling", "url": "https://found.example/apply", "amount": {"currency": "USD", "maximum": 150000},
             "community_mappings": [{"gene": "STXBP1", "relation": "candidate_funding_fit", "basis": "source-explicit-gene"}]},
            {"id": "funders:x", "status": "excluded", "url": "https://x.example"}
        ]),
    );
    d.envelope(
        "cache/research_intl/gtr.json",
        "research_intl.gtr",
        json!([{"id": "100", "url": "https://gtr.example/100", "title": "STXBP1 project", "genes": ["STXBP1"], "matched_in": ["title"],
                "lead_funder": "MRC", "organisations": [{"role": "LEAD_ORG", "name": "University of Edinburgh"}], "start": "2023-06-01", "end": "2025-05-31"}]),
    );
    let mut b = fixtures::builder(&atlas);
    // a paper the claims point at
    b.register("PMID:1", atlas_core::node::NodeKind::Paper, 0);
    let entity = b.entity(atlas_core::provenance::SourceEntity {
        id: "source:pubmed".into(),
        sha256: Some("00".into()),
        licence: Some("NLM terms".into()),
        ..Default::default()
    });
    let rec = b.record(atlas_core::graph::SourceRecord {
        entity,
        locator: atlas_core::provenance::Locator::Record("records[0]".into()),
        id: "PMID:1".into(),
        url: None,
        fetched_at: None,
        hash: atlas_core::graph::RecordHash::CanonicalJson,
        sha256: [1; 32],
    });
    b.data.papers.push(atlas_core::graph::Paper {
        id: "PMID:1".into(),
        title: "A paper".into(),
        journal: String::new(),
        year: None,
        doi: None,
        review: false,
        records: vec![rec],
    });
    d.envelope(
        claims::FILE,
        "claims.extracted",
        json!([
            {"id": "claim:1", "status": "accepted", "quote_verified": true, "pmid": "PMID:1",
             "candidate": {"claim_type": "gene_mechanism", "quote": "STXBP1 haploinsufficiency"},
             "gene_node": {"id": GENE, "kind": "gene"}, "phenotype_node": null},
            {"id": "claim:2", "status": "excluded", "pmid": "PMID:1", "gene_node": {"id": GENE, "kind": "gene"}}
        ]),
    );
    outcomes::ingest(&mut b, d.path()).unwrap();
    funding::ingest(&mut b, d.path()).unwrap();
    claims::ingest(&mut b, d.path()).unwrap();
    super::licences(&mut b);
    let g = Graph::new(b.data);
    assert!(edge(&g, "outcomes:diary", Relation::ResourceFor, GENE).is_some());
    assert!(edge(&g, "outcomes:diary", Relation::ResourceFor, CONDITION).is_some());
    assert!(edge(&g, "outcomes:diary", Relation::ResourceFor, "MONDO:0005027").is_none());
    let call = g.asset(g.node("funders:c1").unwrap().idx);
    assert_eq!(call.kind, AssetKind::FundingCall);
    assert_eq!(call.access.route, "apply");
    assert_eq!(call.fact("amount"), Some("up to 150000 USD"));
    assert!(edge(&g, "funders:c1", Relation::Funds, GENE).is_some());
    assert!(g.node("funders:x").is_none());
    let grant = g.grant(g.node("GTR:100").unwrap().idx);
    assert_eq!(grant.agency, "MRC");
    assert_eq!(grant.fiscal_years, vec![2023, 2024, 2025]);
    assert!(edge(&g, "GTR:100", Relation::AboutGene, GENE).is_some());
    assert!(edge(&g, "GTR:100", Relation::AwardedTo, "org:university-of-edinburgh").is_some());
    let c = edge(&g, "PMID:1", Relation::ClaimsAbout, GENE).unwrap();
    assert_eq!(c.kind, atlas_core::node::EdgeKind::Extracted);
    assert_eq!(g.coverage_of("claims").unwrap().excluded, 1);
    assert_eq!(g.coverage_of("gtr").unwrap().licence_class, Some(LicenceClass::Open));
    let report = integrity::check(&atlas, &g);
    assert!(report.passed, "{:?}", report.violations);
}

#[test]
fn absent_caches_are_reported_not_fatal() {
    let atlas = fixtures::atlas();
    let d = TempData::new();
    let mut b = fixtures::builder(&atlas);
    programmes::ingest(&mut b, d.path()).unwrap();
    samples::ingest(&mut b, d.path()).unwrap();
    outcomes::ingest(&mut b, d.path()).unwrap();
    funding::ingest(&mut b, d.path()).unwrap();
    claims::ingest(&mut b, d.path()).unwrap();
    assert!(b.data.assets.is_empty());
    assert!(b.data.coverage.iter().all(|c| c.status == "absent"));
    // wrong schema → rejected row, no error
    d.envelope(outcomes::FILE, "something.else", json!([]));
    outcomes::ingest(&mut b, d.path()).unwrap();
    assert!(b.data.coverage.last().unwrap().status.starts_with("rejected"));
}
