//! MCP stdio: stdout contains JSON-RPC only; graph tools are read-only.
use crate::protocol::GraphQuery;
use serde_json::{Value, json};

pub enum Action {
    Reply(Value),
    Graph { id: Value, query: GraphQuery },
    Ignore,
}
pub fn result(id: Value, value: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":value})
}
pub fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
pub fn tools() -> Value {
    let tool = |name: &str, description: &str, properties: Value, required: Value| {
        json!({"name":name,"description":description,
        "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})
    };
    json!({"tools":[
        tool("atlas_search","Find conditions, genes, symptoms and groups.",json!({"query":{"type":"string","minLength":1,"maxLength":512},"limit":{"type":"integer","minimum":1,"maximum":20}}),json!(["query"])),
        tool("atlas_condition","Read a condition and its recorded evidence.",json!({"id":{"type":"string","maxLength":256}}),json!(["id"])),
        tool("atlas_provenance","Read source records for a node or edge.",json!({"id":{"type":"string","maxLength":256}}),json!(["id"])),
        tool("atlas_connections","Read a condition's partners and supporting graph evidence.",json!({"id":{"type":"string","maxLength":256}}),json!(["id"]))
    ]})
}
pub fn parse(value: Value, initialized: &mut bool) -> Action {
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || !value.is_object() {
        return Action::Reply(error(id, -32600, "Invalid request"));
    }
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return Action::Reply(error(id, -32600, "Invalid request"));
    };
    if !value.as_object().unwrap().contains_key("id") {
        return Action::Ignore;
    }
    match method {
        "initialize" => {
            *initialized = true;
            let requested = value
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-11-25");
            let version = if ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"].contains(&requested) {
                requested
            } else {
                "2025-11-25"
            };
            Action::Reply(result(
                id,
                json!({"protocolVersion":version,"capabilities":{"tools":{}},
                "serverInfo":{"name":"zebratlas","version":env!("CARGO_PKG_VERSION")}}),
            ))
        }
        "ping" => Action::Reply(result(id, json!({}))),
        _ if !*initialized => Action::Reply(error(id, -32002, "Initialize first")),
        "tools/list" => Action::Reply(result(id, tools())),
        "tools/call" => {
            let name = value.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = value.pointer("/params/arguments").and_then(Value::as_object);
            let Some(args) = args else {
                return Action::Reply(error(id, -32602, "Invalid arguments"));
            };
            let query = if name == "atlas_search" {
                if args.keys().any(|k| k != "query" && k != "limit") {
                    None
                } else {
                    let q = args.get("query").and_then(Value::as_str);
                    let limit = match args.get("limit") {
                        Some(v) => v.as_u64(),
                        None => Some(10),
                    };
                    q.zip(limit)
                        .filter(|(q, l)| !q.is_empty() && q.len() <= 512 && (1..=20).contains(l))
                        .map(|(q, l)| GraphQuery::Search {
                            query: q.into(),
                            limit: l as u8,
                        })
                }
            } else if args.len() == 1 {
                args.get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty() && s.len() <= 256)
                    .and_then(|s| match name {
                        "atlas_condition" => Some(GraphQuery::Node { id: s.into() }),
                        "atlas_provenance" => Some(GraphQuery::Provenance { id: s.into() }),
                        "atlas_connections" => Some(GraphQuery::Connections { id: s.into() }),
                        _ => None,
                    })
            } else {
                None
            };
            match query {
                Some(query) => Action::Graph { id, query },
                None => Action::Reply(error(id, -32602, "Unknown tool or invalid arguments")),
            }
        }
        _ => Action::Reply(error(id, -32601, "Method not found")),
    }
}
pub fn tool_result(id: Value, data: Value) -> Value {
    result(
        id,
        json!({"content":[{"type":"text","text":data.to_string()}],"structuredContent":data,"isError":false}),
    )
}

/// Fixed-size incremental framing: a newline-free input cannot grow memory without bound.
pub async fn line<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> anyhow::Result<Option<Vec<u8>>> {
    use tokio::io::AsyncBufReadExt;
    let mut out = Vec::new();
    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            return if out.is_empty() { Ok(None) } else { Ok(Some(out)) };
        }
        let end = buf.iter().position(|&b| b == b'\n').map(|p| p + 1);
        let take = end.unwrap_or(buf.len());
        anyhow::ensure!(out.len() + take <= 64 * 1024, "MCP input too large");
        out.extend_from_slice(&buf[..take]);
        reader.consume(take);
        if end.is_some() {
            return Ok(Some(out));
        }
    }
}
