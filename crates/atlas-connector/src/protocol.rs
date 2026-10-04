use atlas_llm::request::ProviderOutput;
use atlas_llm::{CompletionRequest, ProviderKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 512 * 1024;
pub const MAX_DEADLINE_SECS: u64 = 120;

/// Public capability: never send local paths, env names, keys or credentials.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub connection: String,
    pub kind: ProviderKind,
    pub default_model: Option<String>,
    pub models: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Frame {
    Hello {
        version: u32,
        capabilities: Vec<Capability>,
    },
    Complete {
        id: String,
        connection: String,
        expires_at: i64,
        request: CompletionRequest,
    },
    Completed {
        id: String,
        output: ProviderOutput,
    },
    Failed {
        id: String,
        code: Failure,
    },
    Cancel {
        id: String,
    },
    Graph {
        id: String,
        query: GraphQuery,
    },
    GraphResult {
        id: String,
        result: Value,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    Busy,
    Unavailable,
    Timeout,
    Rejected,
    ProviderFailed,
}

/// Closed read-only surface. No arbitrary URLs, methods, files, SQL or mutations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraphQuery {
    Search { query: String, limit: u8 },
    Node { id: String },
    Edge { id: String },
    Provenance { id: String },
    Connections { id: String },
}
