//! The Rust-only seam used by the single search/intake understand owner (D56/D57a).
//! It consumes `atlas_intake::Prepared.sent` and search-ir's selected IDs after redaction
//! and linking. It neither re-extracts entities nor accepts unredacted HTTP model prompts.
use super::{
    QueryEngine, QueryPlan,
    conversation::{QueryAnswer, QueryRequest},
    digest,
};
use atlas_llm::ApiKey;
use serde_json::{Value, json};

/// Provider choice resolved by the authenticated connector owner, including per-request keys.
/// No Serialize/Deserialize/Debug: keys and private text never enter HTTP payloads or logs.
#[derive(Default)]
pub struct QueryConnection {
    pub connection: Option<String>,
    pub model: Option<String>,
    pub key: Option<ApiKey>,
    pub visitor: Option<String>,
}

/// Trusted in-process input, built by search/intake after its shared privacy boundary.
/// `input_activity_id` references a trace with hashes, size/timing and counts, never raw text.
pub struct SearchQuery<'a> {
    pub prepared: &'a atlas_intake::Prepared,
    pub linked_ids: &'a [String],
    pub lang: &'a str,
    pub input_activity_id: &'a str,
    pub plan: Option<QueryPlan>,
    pub power_mode: bool,
}

#[derive(serde::Serialize)]
pub struct SearchAnswer {
    pub answer: QueryAnswer,
    pub linked: Vec<super::LinkedEntity>,
    /// Existing semantic-unit API accepts these focus IDs; this is not a fabricated unit ID.
    pub semantic_focus: Vec<String>,
    pub activity: Value,
}

impl crate::Asker {
    pub async fn understand_search_query(
        &self,
        engine: &QueryEngine,
        input: SearchQuery<'_>,
        connection: QueryConnection,
    ) -> Result<SearchAnswer, String> {
        // A hash-based input activity joins this execution back to the intake/search trace.
        if input.input_activity_id.is_empty() || input.input_activity_id.len() > 256 {
            return Err("a bounded search/intake activity ID is required".into());
        }
        let linked = self.link_query_entities(input.linked_ids)?;
        let req = QueryRequest {
            question: input.prepared.sent.clone(),
            linked: linked.clone(),
            lang: Some(input.lang.into()),
            plan: input.plan,
            power_mode: input.power_mode,
            connection: connection.connection,
            model: connection.model,
            visitor: connection.visitor,
        };
        let answer = engine.understand(&self.llm, &req, connection.key).await?;
        let focus = answer
            .plan
            .as_ref()
            .map(|p| p.focus.clone())
            .unwrap_or_else(|| input.linked_ids.to_vec());
        let semantic_focus = focus
            .into_iter()
            .flat_map(|id| match atlas_core::node::parse_edge_id(&id) {
                Some((from, _, to)) => vec![from.to_owned(), to.to_owned()],
                None => vec![id],
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let activity_key = digest(
            json!([
                input.input_activity_id,
                digest(input.prepared.sent.as_bytes()),
                input.linked_ids,
                answer.plan
            ])
            .to_string()
            .as_bytes(),
        );
        let activity = json!({
            "@type":"prov:Activity", "@id":format!("urn:atlas:search-query:{activity_key}"), "prov:used": input.input_activity_id,
            "input_sha256": input.prepared.sha256,
            "redacted_input_sha256": digest(input.prepared.sent.as_bytes()),
            "redaction_counts": input.prepared.redacted.counts,
            "input_bytes": input.prepared.bytes, "truncated": input.prepared.truncated,
            "linked_ids": input.linked_ids,
            "prov:generated": answer.results.iter().map(|r| &r.activity["@id"]).collect::<Vec<_>>(),
            "model_activities": answer.model_provenance.iter().map(|p| &p.activity.id).collect::<Vec<_>>(),
            "prov:wasAssociatedWith":{"@type":"prov:SoftwareAgent","name":"atlas-ask/search-query","version":env!("CARGO_PKG_VERSION")}
        });
        Ok(SearchAnswer {
            answer,
            linked,
            semantic_focus,
            activity,
        })
    }
}
