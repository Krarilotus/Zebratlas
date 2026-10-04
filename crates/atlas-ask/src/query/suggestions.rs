//! D57b: property-first suggestions from the connected graph's adjacency index, without a model.
use super::{digest, plan::Direction};
use atlas_core::{graph::RecordWithhold, node::NodeRef};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SCAN_LIMIT: usize = 5_000;
const PAGE_SIZE: usize = 5;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionRequest {
    pub focus: String,
    pub relation: Option<String>,
    pub direction: Option<Direction>,
    #[serde(default)]
    pub offset: usize,
}

#[derive(Serialize)]
pub struct Suggestions {
    pub focus: NodeRef,
    pub scope: &'static str,
    pub properties: Vec<Value>,
    pub counts_exact: bool,
    pub scanned_assertions: usize,
    pub next_offset: Option<usize>,
    pub activity: Value,
}

#[derive(Default)]
struct Property {
    targets: BTreeMap<String, NodeRef>,
    assertions: BTreeSet<String>,
    status_counts: BTreeMap<String, usize>,
    records: BTreeSet<u32>,
    samples: BTreeMap<String, Value>,
}

impl crate::Asker {
    pub fn query_suggestions(&self, req: &SuggestionRequest) -> Result<Suggestions, String> {
        if req.focus.len() > 256 || req.offset > 1_000 || req.relation.as_ref().is_some_and(|r| r.len() > 100) {
            return Err("suggestion input exceeds limits".into());
        }
        self.link_query_entities(std::slice::from_ref(&req.focus))?;
        if self.graph.edge_by_id(&req.focus).is_some() {
            return Err("select an assertion endpoint to suggest its relations".into());
        }
        let ctx = crate::exec::Ctx::new(&self.atlas, &self.graph);
        let (target, _) = ctx.resolve_any(&req.focus).ok_or("unknown focus")?;
        let focus = ctx.target_ref(target);
        let mut groups: BTreeMap<(String, bool), Property> = BTreeMap::new();
        let mut scanned = 0;
        let mut exact = true;
        for inc in self.graph.incident(&focus.id) {
            if scanned == SCAN_LIMIT {
                exact = false;
                break;
            }
            scanned += 1;
            if req.relation.as_deref().is_some_and(|r| r != inc.edge.relation.as_str())
                || req
                    .direction
                    .is_some_and(|d| inc.outgoing != (d == Direction::Outgoing))
                || self.graph.records_withheld(&inc.edge.records).is_some()
                || [&inc.edge.from, &inc.edge.to].into_iter().any(|id| {
                    self.graph
                        .node(id)
                        .is_some_and(|key| self.graph.node_withheld(key).is_some())
                })
            {
                continue;
            }
            let Some((target, _)) = ctx.resolve_any(inc.other) else {
                continue;
            };
            let other = ctx.target_ref(target);
            // Suggestions must carry actual IDs, never approximate lexical matches or aliases.
            if other.id != inc.other {
                continue;
            }
            let group = groups
                .entry((inc.edge.relation.as_str().into(), inc.outgoing))
                .or_default();
            group.targets.insert(other.id.clone(), other.clone());
            if group.assertions.insert(inc.edge.id()) {
                *group.status_counts.entry(inc.edge.kind.as_str().into()).or_default() += 1;
            }
            group.records.extend(&inc.edge.records);
            // Stable smallest-ID samples, bounded even for a high-degree node.
            if group.samples.len() < 3 || group.samples.keys().next_back().is_some_and(|id| &other.id < id) {
                let evidence = inc
                    .edge
                    .records
                    .iter()
                    .map(|r| {
                        let record = self.graph.record(*r);
                        json!({"source":self.graph.provenance().entity(record.entity),
                        "record_locator":record.locator.to_string(),"record_url":record.url,
                        "retrieved_at":record.fetched_at,"sha256":atlas_core::graph::hex(&record.sha256),
                        "hash_scope":record.hash})
                    })
                    .collect::<Vec<_>>();
                group.samples.insert(other.id.clone(), json!({"node":other,"edge":inc.edge.id(),
                    "status":inc.edge.kind,"evidence":evidence,
                    "activity":if inc.edge.records.is_empty() {None} else {Some(self.graph.provenance().activity(inc.edge.activity))}}));
                if group.samples.len() > 3 {
                    group.samples.pop_last();
                }
            }
        }
        let mut groups = groups.into_iter().collect::<Vec<_>>();
        groups.sort_by(|(ka, a), (kb, b)| b.targets.len().cmp(&a.targets.len()).then_with(|| ka.cmp(kb)));
        let next_offset = (groups.len() > req.offset + PAGE_SIZE).then_some(req.offset + PAGE_SIZE);
        let mut used: BTreeMap<String, Value> = BTreeMap::new();
        let properties = groups.into_iter().skip(req.offset).take(PAGE_SIZE).map(|((relation,outgoing),g)| {
            let mut classes = BTreeMap::new();
            for node in g.targets.values() { *classes.entry(node.kind).or_insert(0usize) += 1; }
            for r in &g.records {
                let record = self.graph.record(*r);
                let source = self.graph.provenance().entity(record.entity);
                used.insert(source.id.clone(), json!(source));
            }
            json!({"hop":{"relation":relation,"direction":if outgoing {Direction::Outgoing} else {Direction::Incoming}},
                "distinct_targets":g.targets.len(),"assertions":g.assertions.len(),"status_counts":g.status_counts,
                "outputs":classes.into_iter().map(|(kind,count)|json!({"kind":kind,"count":count})).collect::<Vec<_>>(),
                "samples":g.samples.into_values().collect::<Vec<_>>(),
                "support_sha256":digest(serde_json::to_vec(&g.assertions).unwrap().as_slice())})
        }).collect::<Vec<_>>();
        let result_hash = digest(serde_json::to_vec(&properties).unwrap().as_slice());
        let activity_hash = digest(
            json!([
                focus.id,
                req.relation,
                req.direction,
                req.offset,
                exact,
                scanned,
                result_hash
            ])
            .to_string()
            .as_bytes(),
        );
        let activity = json!({"@type":"prov:Activity","@id":format!("urn:atlas:query-suggestions:{activity_hash}"),
            "prov:used":used.into_values().collect::<Vec<_>>(),"result_sha256":result_hash,
            "prov:wasAssociatedWith":{"@type":"prov:SoftwareAgent","name":"atlas-ask/query-suggestions","version":env!("CARGO_PKG_VERSION")},
            "parameters":{"focus":focus.id,"relation":req.relation,"direction":req.direction,"offset":req.offset,
                "page_size":PAGE_SIZE,"scan_limit":SCAN_LIMIT,"counts_exact":exact,"scanned_assertions":scanned,
                "scope":"connected_graph_adjacency","inferred_relations_added":false}});
        Ok(Suggestions {
            focus,
            scope: "connected_graph_adjacency",
            properties,
            counts_exact: exact,
            scanned_assertions: scanned,
            next_offset,
            activity,
        })
    }
}
