//! Persona jobs (D39): per condition or gene, the things a person can *get* — models & samples,
//! therapy programmes, free-to-read papers, funding, outcome measures — each with what it is, who
//! holds it, how to get it, its source record, licence and a verify link. No bare scores.
//! Quarantined records never appear (fail-closed). Shapes: docs/design/API.md "v2: jobs".

use std::collections::{BTreeMap, HashMap, HashSet};

use atlas_core::graph::{AssetKind, Job, RecIdx, RecordWithhold, Relation};
use atlas_core::node::{NodeKey, NodeKind};
use atlas_core::{Atlas, DiseaseIdx, Graph};
use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::copy_jobs;
use crate::nodes;
use crate::routes::{ApiError, ApiResult, AppState, not_found};

/// Coverage sources searched per job (prefix match on the coverage `source`).
fn searched_sources(job: Job) -> &'static [&'static str] {
    match job {
        Job::ModelsSamples => &[
            "org-assets",
            "erdri",
            "models",
            "kgx:alliance",
            "kgx:cellosaurus",
            "kgx:impc",
            "kgx:bbmri",
            "kgx:ega",
        ],
        Job::TherapyProgrammes => &[
            "pipelines",
            "regulatory",
            "kgx:chembl",
            "kgx:opentargets",
            "kgx:ema",
            "kgx:fda",
            "kgx:pipelines",
            "kgx:nlorem",
            "kgx:broad",
            "kgx:ctgov",
        ],
        Job::FreePapers => &["pubmed", "openaccess", "kgx:openalex"],
        Job::Funding => &[
            "funders",
            "reporter",
            "cordis",
            "gtr",
            "kaken",
            "europepmc_grants",
            "kgx:crossref_funders",
        ],
        Job::OutcomeMeasures => &["outcomes"],
    }
}

/// How the item reaches the subject: `exact` (linked to it), `gene` (to a causal gene),
/// `condition` (to a condition the gene causes), `ortholog` (to the gene's model-organism ortholog).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Reach {
    Exact,
    Gene,
    Condition,
    Ortholog,
}

impl Reach {
    fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Gene => "gene",
            Self::Condition => "condition",
            Self::Ortholog => "ortholog",
        }
    }
}

struct Found {
    key: NodeKey,
    reach: Reach,
    edge: u32,
}

fn job_of(graph: &Graph, key: NodeKey, relation: Relation) -> Option<Job> {
    match key.kind {
        NodeKind::Asset => graph.asset(key.idx).kind.job(),
        NodeKind::Grant if matches!(relation, Relation::AboutGene | Relation::AboutCondition) => Some(Job::Funding),
        NodeKind::Paper
            if matches!(
                relation,
                Relation::AboutGene | Relation::AboutCondition | Relation::ClaimsAbout
            ) && graph.open_access(&graph.paper(key.idx).id).next().is_some() =>
        {
            Some(Job::FreePapers)
        }
        _ => None,
    }
}

fn visible(graph: &Graph, key: NodeKey, edge: u32) -> bool {
    graph.node_withheld(key).is_none() && graph.records_withheld(&graph.edge(edge).records).is_none()
}

/// Items per job reachable from the targets, best reach first, deduplicated by node.
fn collect(graph: &Graph, targets: &[(String, Reach)], jobs: &[Job]) -> BTreeMap<Job, Vec<Found>> {
    let mut out: BTreeMap<Job, Vec<Found>> = BTreeMap::new();
    let mut seen: HashSet<NodeKey> = HashSet::new();
    let mut add = |key: NodeKey, reach: Reach, edge: u32, job: Job, out: &mut BTreeMap<Job, Vec<Found>>| {
        if jobs.contains(&job) && seen.insert(key) {
            out.entry(job).or_default().push(Found { key, reach, edge });
        }
    };
    let mut orthologs: Vec<String> = Vec::new();
    for (target, reach) in targets {
        for inc in graph.incident(target) {
            let r = inc.edge.relation;
            if r == Relation::CandidateSameAs {
                continue;
            }
            let Some(key) = graph.node(inc.other) else { continue };
            if r == Relation::OrthologousTo {
                orthologs.push(inc.other.to_owned());
                continue;
            }
            if !visible(graph, key, inc.idx) {
                continue;
            }
            if let Some(job) = job_of(graph, key, r) {
                add(key, *reach, inc.idx, job, &mut out);
            }
        }
    }
    // models of the gene's orthologs (one hop through `orthologous_to` only)
    if jobs.contains(&Job::ModelsSamples) {
        for o in orthologs {
            for inc in graph.incident(&o) {
                let Some(key) = graph.node(inc.other) else { continue };
                if key.kind == NodeKind::Asset
                    && matches!(graph.asset(key.idx).kind, AssetKind::Model | AssetKind::CellLine)
                    && visible(graph, key, inc.idx)
                {
                    add(key, Reach::Ortholog, inc.idx, Job::ModelsSamples, &mut out);
                }
            }
        }
    }
    for list in out.values_mut() {
        list.sort_by_key(|f| (f.reach, rank(graph, f.key)));
    }
    out
}

/// Within one reach: human cell lines and items with a holder and a request route first.
fn rank(graph: &Graph, key: NodeKey) -> (u8, u8, String) {
    match key.kind {
        NodeKind::Asset => {
            let a = graph.asset(key.idx);
            let kind = match a.kind {
                AssetKind::CellLine
                | AssetKind::Registry
                | AssetKind::Biobank
                | AssetKind::Programme
                | AssetKind::FundingCall => 0,
                AssetKind::Model | AssetKind::Dataset | AssetKind::OutcomeMeasure | AssetKind::Designation => 1,
                _ => 2,
            };
            let route = u8::from(a.holder_name.is_none()) + u8::from(a.access.route == "official_page");
            (kind, route, a.label.to_lowercase())
        }
        NodeKind::Paper => {
            let p = graph.paper(key.idx);
            (
                0,
                0,
                format!("{:05}:{}", u16::MAX - p.year.unwrap_or(0), p.title.to_lowercase()),
            )
        }
        _ => (1, 1, graph.node_ref(key).label.to_lowercase()),
    }
}

/// Source block of a record, with the coverage label of its file.
fn source(graph: &Graph, labels: &HashMap<&str, &str>, r: RecIdx) -> Value {
    let rec = graph.record(r);
    let ent = graph.provenance().entity(rec.entity);
    json!({
        "name": labels.get(ent.file.as_str()).copied().unwrap_or(ent.file.as_str()),
        "record": rec.id,
        "url": rec.url.clone().unwrap_or_else(|| ent.url.clone()),
        "retrieved_at": rec.fetched_at.clone().or_else(|| ent.retrieved_at.clone()),
        "sha256": atlas_core::graph::hex(&rec.sha256),
        "file": format!("{}#{}", ent.file, rec.locator),
        "version": ent.version,
        "record_locator": rec.locator.to_string(),
    })
}

fn licence(graph: &Graph, records: &[RecIdx]) -> Value {
    let class = graph.licence_class(records);
    let id = records
        .iter()
        .filter_map(|&r| graph.licence_of(r))
        .find(|l| l.class == class)
        .map(|l| l.licence.clone());
    json!({ "id": id, "class": class.as_str(), "release": class.release() })
}

fn facts(pairs: impl IntoIterator<Item = (String, String)>) -> Value {
    Value::Array(
        pairs
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| json!({ "key": k, "value": v }))
            .collect(),
    )
}

fn item(graph: &Graph, labels: &HashMap<&str, &str>, f: &Found) -> Value {
    let e = graph.edge(f.edge);
    let node_records = graph.node_records(f.key);
    let mut records: Vec<RecIdx> = node_records.to_vec();
    records.extend(&e.records);
    let (id, kind, label, holder, how, facts_v) = match f.key.kind {
        NodeKind::Asset => {
            let a = graph.asset(f.key.idx);
            let org = a
                .holder
                .as_deref()
                .and_then(|h| graph.node(h))
                .map(|k| graph.node_ref(k));
            let name = org.as_ref().map(|o| o.label.clone()).or_else(|| a.holder_name.clone());
            let holder = name
                .as_ref()
                .map(|n| json!({ "id": org.as_ref().map(|o| o.id.clone()), "name": n, "kind": "organisation" }));
            let mut qualifiers = a.access_context();
            let routes = qualifiers
                .as_object_mut()
                .and_then(|q| q.remove("access_routes"))
                .unwrap_or(json!([]));
            let how = json!({
                "route": a.access.route, "url": a.access.url, "note": a.access.note,
                "msg": copy_jobs::route(&a.access.route, name.as_deref()),
                "routes": routes, "qualifiers": qualifiers,
            });
            (
                a.id.clone(),
                a.kind.as_str().to_owned(),
                a.label.clone(),
                holder,
                how,
                facts(a.facts.iter().filter(|(k, _)| k != "access_context").cloned()),
            )
        }
        NodeKind::Grant => {
            let g = graph.grant(f.key.idx);
            let who = if g.organisation.is_empty() {
                g.agency.clone()
            } else {
                g.organisation.clone()
            };
            let years = match (g.fiscal_years.first(), g.fiscal_years.last()) {
                (Some(a), Some(z)) if a != z => format!("{a}–{z}"),
                (Some(a), _) => a.to_string(),
                _ => String::new(),
            };
            // RePORTER is a project record; it does not establish a contact channel.
            let how = json!({ "route": "official_page", "url": g.url, "note": null,
                              "msg": copy_jobs::route("official_page", Some(&who)) });
            let holder = Some(json!({ "id": null, "name": who, "kind": "organisation" }));
            let fs = facts([
                ("funder".to_owned(), g.agency.clone()),
                ("years".to_owned(), years),
                ("programme".to_owned(), g.activity_code.clone()),
            ]);
            (g.id.clone(), "grant".to_owned(), g.title.clone(), holder, how, fs)
        }
        _ => {
            let p = graph.paper(f.key.idx);
            let oa = graph.open_access(&p.id).next();
            if let Some(o) = oa {
                records.push(o.record);
            }
            let how = json!({ "route": "read_free", "url": oa.map(|o| o.url.clone()), "note": null,
                              "full_text_reuse_licence": oa.and_then(|o| o.licence.clone()),
                              "msg": copy_jobs::route("read_free", None) });
            let fs = facts([
                ("journal".to_owned(), p.journal.clone()),
                ("year".to_owned(), p.year.map(|y| y.to_string()).unwrap_or_default()),
                (
                    "free_to_read".to_owned(),
                    oa.map(|o| o.status.clone()).unwrap_or_default(),
                ),
                ("doi".to_owned(), p.doi.clone().unwrap_or_default()),
            ]);
            (p.id.clone(), "paper".to_owned(), p.title.clone(), None, how, fs)
        }
    };
    json!({
        "id": id,
        "what": { "kind": kind, "label": label, "explained": copy_jobs::kind(&kind) },
        "holder": holder,
        "how_to_get": how,
        "facts": facts_v,
        "match": f.reach.as_str(),
        "via": { "edge_id": e.id(), "relation": e.relation.as_str(), "reason": e.reason },
        "source": node_records.first().map(|&r| source(graph, labels, r)),
        "reading_source": (f.key.kind == NodeKind::Paper).then(|| {
            graph.open_access(&id).next().map(|o| source(graph, labels, o.record))
        }).flatten(),
        "evidence": id.starts_with("REPORTER:").then(|| format!("/api/funding/{}/evidence", nodes::encode_path(&id))),
        "licence": licence(graph, &records),
        "verify": format!("/api/verify/{}", nodes::encode_path(&id)),
    })
}

fn report(
    graph: &Graph,
    subject: Value,
    genes: Value,
    targets: &[(String, Reach)],
    jobs: &[Job],
    limit: usize,
) -> Value {
    let mut labels: HashMap<&str, &str> = HashMap::new();
    for c in graph.coverage() {
        for f in &c.files {
            labels.entry(f.as_str()).or_insert(c.label.as_str());
        }
    }
    let found = collect(graph, targets, jobs);
    let symbols: Vec<String> = genes
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g["label"].as_str().map(str::to_owned))
        .collect();
    let out: Vec<Value> = jobs
        .iter()
        .map(|&job| {
            let list = found.get(&job).map_or(&[][..], Vec::as_slice);
            let searched: Vec<&atlas_core::graph::Coverage> = graph
                .coverage()
                .iter()
                .filter(|c| {
                    searched_sources(job)
                        .iter()
                        .any(|s| c.source == *s || c.source.starts_with(&format!("{s}:")))
                })
                .collect();
            let mut names: Vec<String> = searched
                .iter()
                .filter(|c| crate::coverage::scoped(c, &symbols))
                .map(|c| c.label.clone())
                .collect();
            names.dedup();
            json!({
                "job": job.as_str(),
                "label": copy_jobs::label(job),
                "total": list.len(),
                "items": list.iter().take(limit).map(|f| item(graph, &labels, f)).collect::<Vec<_>>(),
                "none_found": list.is_empty().then(|| copy_jobs::none(job, &names)),
                "searched": searched.iter().map(|c| json!({
                    "source": c.source, "label": c.label, "status": c.status,
                    "retrieved_at": c.retrieved_at, "records": c.records,
                    "scope": c.scope, "genes_queried": c.genes,
                    "checked_for_subject": crate::coverage::scoped(c, &symbols),
                    "check_kind": "cached_index",
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "subject": subject, "genes": genes, "jobs": out })
}

#[derive(Deserialize)]
pub struct JobParams {
    job: Option<String>,
    limit: Option<usize>,
}

fn parse(p: &JobParams) -> Result<(Vec<Job>, usize), ApiError> {
    let jobs = match p.job.as_deref().filter(|j| !j.trim().is_empty()) {
        None => Job::ALL.to_vec(),
        Some(list) => list
            .split(',')
            .map(|j| {
                Job::parse(j.trim()).ok_or_else(|| {
                    ApiError(
                        axum::http::StatusCode::BAD_REQUEST,
                        crate::copy_extra::msg("api.error.job_unknown", json!({"job": j})),
                    )
                })
            })
            .collect::<Result<_, _>>()?,
    };
    Ok((jobs, p.limit.unwrap_or(10).clamp(1, 100)))
}

fn condition_targets(atlas: &Atlas, d: DiseaseIdx) -> (Vec<(String, Reach)>, Value) {
    let mut targets = vec![(atlas.disease_at(d).id.clone(), Reach::Exact)];
    let genes = nodes::causal_genes(atlas, d);
    let refs: Vec<Value> = genes
        .iter()
        .map(|(id, label, _)| json!({ "id": id, "kind": "gene", "label": label }))
        .collect();
    targets.extend(genes.into_iter().map(|(id, _, _)| (id, Reach::Gene)));
    (targets, Value::Array(refs))
}

/// `GET /api/condition/{id}/jobs`.
pub async fn condition(State(s): State<AppState>, Path(id): Path<String>, Query(p): Query<JobParams>) -> ApiResult {
    let (jobs, limit) = parse(&p)?;
    let atlas = s.atlas();
    // any source id, also one merged by an SSSOM exact match (DOID, MedGen, ...)
    let d = nodes::condition(atlas, &id)
        .or_else(|e| nodes::condition(atlas, s.graph.canonical_id(id.trim())).map_err(|_| e))
        .map_err(not_found)?;
    let (targets, genes) = condition_targets(atlas, d);
    let subject = serde_json::to_value(atlas.disease_ref(d)).expect("node ref");
    let mut body = report(&s.graph, subject, genes, &targets, &jobs, limit);
    body["coverage"] = crate::coverage::condition(atlas, &s.graph, d);
    body["already_working_on_this"] = json!(crate::initiatives::for_condition(atlas, &s.graph, d));
    Ok(Json(body))
}

/// `GET /api/gene/{id}/jobs`: the gene and the rare conditions it causes.
pub async fn gene(State(s): State<AppState>, Path(id): Path<String>, Query(p): Query<JobParams>) -> ApiResult {
    let (jobs, limit) = parse(&p)?;
    let atlas = s.atlas();
    let key = s.graph.canonical_id(id.trim()).to_owned();
    let g = atlas
        .gene(&key)
        .ok_or_else(|| not_found(crate::copy_extra::msg("api.error.gene_unknown", json!({"id": id}))))?;
    let gene = atlas.gene_at(g);
    let gid = gene.id().to_owned();
    let mut targets = vec![(gid.clone(), Reach::Exact)];
    for &d in &gene.diseases {
        let dis = atlas.disease_at(d);
        if dis.rare && dis.genes.iter().any(|l| l.symbol == gene.symbol && l.is_causal()) {
            targets.push((dis.id.clone(), Reach::Condition));
        }
    }
    let subject = json!({ "id": gid, "kind": "gene", "label": gene.symbol });
    let genes = json!([subject.clone()]);
    let mut body = report(&s.graph, subject, genes, &targets, &jobs, limit);
    body["already_working_on_this"] = json!(crate::initiatives::for_gene(atlas, &s.graph, g));
    Ok(Json(body))
}

#[cfg(test)]
mod tests {
    use atlas_core::Graph;
    use atlas_core::graph::{
        Access, Asset, AssetKind, EntityLicence, GraphData, GraphEdge, LicenceClass, LinkLevel, Quarantine, RecordHash,
        Relation, SourceRecord,
    };
    use atlas_core::node::EdgeKind;
    use atlas_core::provenance::{Activity, ActivityIdx, EntityIdx, Locator, SourceEntity};
    use serde_json::json;

    use super::{Job, Reach, report};

    fn asset(id: &str, kind: AssetKind, rec: u32) -> Asset {
        Asset {
            id: id.into(),
            label: format!("{id} label"),
            kind,
            category: "test".into(),
            holder: None,
            holder_name: Some("EBiSC".into()),
            access: Access {
                route: "repository_order".into(),
                url: Some("https://ebisc.example/1".into()),
                note: None,
            },
            facts: vec![("organism".into(), "Homo sapiens".into()), ("access_context".into(), json!({
                "access_routes":[{"request_url":"https://example.org/starr"}, {"request_url":"https://example.org/esco"}],
                "overall_status":"RECRUITING", "site_status":null, "rights":{"reuse":"not assessed"}
            }).to_string())],
            verify_url: None,
            release: true,
            records: vec![rec],
        }
    }

    fn edge(from: &str, to: &str, rec: u32) -> GraphEdge {
        GraphEdge {
            from: from.into(),
            relation: Relation::ModelOf,
            to: to.into(),
            kind: EdgeKind::Observed,
            level: LinkLevel::Gene,
            reason: "test".into(),
            activity: ActivityIdx(0),
            records: vec![rec],
        }
    }

    #[test]
    fn job_items_say_what_who_how_and_hide_quarantined() {
        let mut data = GraphData::default();
        data.provenance.add_entity(SourceEntity {
            id: "source:cache/kgx/x/nodes.jsonl".into(),
            file: "cache/kgx/x/nodes.jsonl".into(),
            sha256: Some("ab".into()),
            ..SourceEntity::default()
        });
        data.provenance.add_activity(Activity::default());
        data.licences.push(EntityLicence {
            entity: EntityIdx(0),
            licence: "CC-BY-4.0".into(),
            class: LicenceClass::Open,
        });
        for i in 0..2 {
            data.records.push(SourceRecord {
                entity: EntityIdx(0),
                locator: Locator::Line(i + 1),
                id: format!("r{i}"),
                url: None,
                fetched_at: Some("2026-10-04T00:00:00Z".into()),
                hash: RecordHash::JsonLine,
                sha256: [1; 32],
            });
        }
        data.assets = vec![
            asset("CVCL:1", AssetKind::CellLine, 0),
            asset("CVCL:2", AssetKind::CellLine, 1),
        ];
        data.edges = vec![edge("CVCL:1", "HGNC:11444", 0), edge("CVCL:2", "HGNC:11444", 1)];
        data.quarantine = vec![Quarantine {
            record: 1,
            reason: "fetched_after_block".into(),
        }];
        let g = Graph::new(data);
        let v = report(
            &g,
            json!({"id": "HGNC:11444"}),
            json!([]),
            &[("HGNC:11444".to_owned(), Reach::Exact)],
            &[Job::ModelsSamples, Job::Funding],
            10,
        );
        let models = &v["jobs"][0];
        assert_eq!(models["total"], 1, "the quarantined line is hidden");
        let it = &models["items"][0];
        assert_eq!(it["id"], "CVCL:1");
        assert_eq!(it["what"]["kind"], "cell_line");
        assert!(
            it["what"]["explained"]["fallback"]
                .as_str()
                .unwrap()
                .contains("cell line")
        );
        assert_eq!(it["holder"]["name"], "EBiSC");
        assert_eq!(it["how_to_get"]["route"], "repository_order");
        assert_eq!(it["how_to_get"]["url"], "https://ebisc.example/1");
        assert_eq!(it["how_to_get"]["routes"].as_array().unwrap().len(), 2);
        assert_eq!(it["how_to_get"]["qualifiers"]["overall_status"], "RECRUITING");
        assert!(
            it["how_to_get"]["qualifiers"]
                .as_object()
                .unwrap()
                .contains_key("site_status")
        );
        assert!(it["how_to_get"]["qualifiers"]["site_status"].is_null());
        assert_eq!(it["how_to_get"]["qualifiers"]["rights"]["reuse"], "not assessed");
        assert!(
            !it["facts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["key"] == "access_context")
        );
        assert_eq!(it["licence"]["class"], "open");
        assert_eq!(it["licence"]["release"], "full");
        assert_eq!(it["source"]["file"], "cache/kgx/x/nodes.jsonl#L1");
        assert_eq!(it["verify"], "/api/verify/CVCL%3A1");
        assert!(it.get("score").is_none(), "no bare scores (D39)");
        assert!(
            v["jobs"][1]["none_found"]["fallback"]
                .as_str()
                .unwrap()
                .contains("Searched")
        );
    }
}
