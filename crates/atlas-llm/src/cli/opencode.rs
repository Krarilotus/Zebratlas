//! Native OpenCode 1.x `run --format json`, with stdin prompts and a tool-free agent.
//!
//! Protocol checked against installed 1.2.27 and the matching official source:
//! https://github.com/anomalyco/opencode/tree/v1.2.27/packages/opencode/src/cli/cmd
//! Prices: https://opencode.ai/docs/zen/ (2026-10-04). Every completion refreshes
//! model metadata and refuses anything with nonzero/unknown input, output or cache cost.
//! Global config/skills/plugins are isolated, while OpenCode's own login stays in its
//! original data directory. The CLI creates a fresh private session; it has no ephemeral
//! flag, so its own session retention still applies. We never read/copy credentials.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use super::{Launch, PROBE_DEADLINE, Spawn, exit_error, npm_bin_dir, prompt_parts, run, version, which_native};
use crate::error::{LlmError, Result};
use crate::provider::{Availability, Provider, ProviderKind};
use crate::request::{CompletionRequest, ProviderOutput, Usage};
use crate::secret::ApiKey;

pub const DEFAULT_MODEL: &str = "opencode/big-pickle";
const AGENT: &str = "atlas-completion";

#[derive(Debug)]
pub struct OpenCodeCli {
    launch: Option<Launch>,
    gate: Semaphore,
}

impl OpenCodeCli {
    pub fn new(executable: Option<PathBuf>, max_parallel: usize) -> Result<Self> {
        Ok(Self {
            launch: match executable {
                Some(p) => Some(Launch::from_path(&p)?),
                None => resolve_native().map(Launch::Exe),
            },
            gate: Semaphore::new(max_parallel.max(1)),
        })
    }

    async fn complete_inner(&self, req: &CompletionRequest, model: &str) -> Result<ProviderOutput> {
        if !model.starts_with("opencode/") || model.trim() != model {
            return Err(LlmError::InvalidRequest(
                "OpenCode requires an explicit opencode/model ID".into(),
            ));
        }
        let launch = self
            .launch
            .as_ref()
            .ok_or_else(|| LlmError::Unavailable("opencode CLI not installed".into()))?;
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| LlmError::Unavailable("closed".into()))?;
        let cli_version = version(launch)
            .await
            .filter(|v| v.starts_with("1."))
            .ok_or_else(|| LlmError::Unavailable("OpenCode 1.x CLI is required".into()))?;
        let dir = tempfile::tempdir()?;
        let env = managed_env(dir.path(), req);
        let models = catalog(launch, dir.path(), &env, true).await?;
        if !models.iter().any(|m| m == model) {
            return Err(LlmError::Unavailable(format!(
                "OpenCode model {model} is not advertised with verified zero cost"
            )));
        }
        let (system, prompt) = prompt_parts(req, true);
        let stdin = if system.is_empty() {
            prompt
        } else {
            format!("{system}\n\n{prompt}")
        };
        let out = run(Spawn {
            launch,
            args: run_args(model),
            stdin: Some(&stdin),
            cwd: dir.path(),
            extra_env: env,
            deadline: req.deadline,
            inspect_line: Some(&inspect),
        })
        .await?;
        if out.exit_code != Some(0) {
            return Err(exit_error("opencode", &out));
        }
        let (text, usage, stop_reason) = parse_events(&out.stdout)?;
        let mut sent = BTreeMap::from([
            ("model".into(), model.into()),
            (
                "restrictions".into(),
                "tools=none,config=isolated,plugins=off,share=disabled,steps=1,paid=denied".into(),
            ),
            ("session".into(), "fresh; retained by OpenCode".into()),
            (
                "price_check".into(),
                "refreshed-cli-catalog:input=0,output=0,cache=0".into(),
            ),
        ]);
        if let Some(t) = req.settings.temperature {
            sent.insert("temperature".into(), t.to_string());
        }
        if let Some(n) = req.settings.max_tokens {
            sent.insert("max_tokens".into(), n.to_string());
        }
        if req.schema.is_some() {
            sent.insert("response_format".into(), "schema-in-prompt;validated-by-atlas".into());
        }
        Ok(ProviderOutput {
            text,
            reported_model: None, // CLI events do not report the served model.
            usage,
            stop_reason: Some(stop_reason),
            agent_version: Some(format!("opencode-cli {cli_version}")),
            sent,
        })
    }
}

fn resolve_native() -> Option<PathBuf> {
    for name in ["opencode", "opencode-cli"] {
        if let Some(p) = which_native(name) {
            return Some(p);
        }
    }
    // The Windows desktop installer does not add its CLI to PATH.
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let p = PathBuf::from(local).join("opencode").join("opencode-cli.exe");
            if p.is_file() {
                return Some(p);
            }
        }
        if let Some(npm) = npm_bin_dir("opencode") {
            for platform in ["opencode-windows-x64", "opencode-windows-arm64"] {
                for base in [
                    npm.join("node_modules"),
                    npm.join("node_modules/opencode-ai/node_modules"),
                ] {
                    let p = base.join(platform).join("bin/opencode.exe");
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

fn run_args(model: &str) -> Vec<OsString> {
    [
        "run",
        "--format",
        "json",
        "--model",
        model,
        "--agent",
        AGENT,
        "--title",
        "Atlas query",
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

fn managed_env(dir: &Path, req: &CompletionRequest) -> Vec<(String, String)> {
    let mut agent = json!({
        "mode": "primary", "description": "Return one text completion without tools.",
        "prompt": "Return only the requested answer. Do not use tools or inspect files.",
        "permission": {"*": "deny"}, "steps": 1,
    });
    if let Some(t) = req.settings.temperature {
        agent["temperature"] = json!(t);
    }
    let config = json!({
        "autoupdate": false, "share": "disabled", "snapshot": false,
        "permission": {"*": "deny"}, "plugin": [], "mcp": {}, "instructions": [],
        "lsp": false, "formatter": false, "compaction": {"auto": false, "prune": false},
        "default_agent": AGENT, "agent": {
            AGENT: agent,
            "title": {"disable": true},
            "summary": {"disable": true},
            "compaction": {"disable": true},
        },
    });
    let mut env = vec![
        ("XDG_CONFIG_HOME".into(), dir.join("config").display().to_string()),
        ("XDG_STATE_HOME".into(), dir.join("state").display().to_string()),
        ("OPENCODE_CONFIG_CONTENT".into(), config.to_string()),
        ("OPENCODE_DISABLE_PROJECT_CONFIG".into(), "true".into()),
        ("OPENCODE_DISABLE_AUTOUPDATE".into(), "true".into()),
        ("OPENCODE_DISABLE_DEFAULT_PLUGINS".into(), "true".into()),
        ("OPENCODE_DISABLE_EXTERNAL_SKILLS".into(), "true".into()),
        ("OPENCODE_DISABLE_CLAUDE_CODE".into(), "true".into()),
        ("OPENCODE_DISABLE_LSP_DOWNLOAD".into(), "true".into()),
        ("OPENCODE_EXPERIMENTAL_DISABLE_FILEWATCHER".into(), "true".into()),
        ("OPENCODE_PERMISSION".into(), json!({"*": "deny"}).to_string()),
    ];
    // OpenCode's transform.maxOutputTokens uses this cap for non-Codex Zen models.
    if let Some(n) = req.settings.max_tokens {
        env.push(("OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX".into(), n.to_string()));
    }
    env
}

async fn catalog(launch: &Launch, cwd: &Path, env: &[(String, String)], refresh: bool) -> Result<Vec<String>> {
    let mut args: Vec<OsString> = ["models", "opencode", "--verbose"].map(Into::into).to_vec();
    if refresh {
        args.push("--refresh".into());
    }
    let out = run(Spawn {
        launch,
        args,
        stdin: None,
        cwd,
        extra_env: env.to_vec(),
        deadline: PROBE_DEADLINE,
        inspect_line: None,
    })
    .await?;
    if out.exit_code != Some(0) {
        return Err(exit_error("opencode catalog", &out));
    }
    parse_catalog(&out.stdout)
}

fn parse_catalog(stdout: &str) -> Result<Vec<String>> {
    let mut models = Vec::new();
    let mut name = String::new();
    let mut block = String::new();
    for line in stdout.lines() {
        if line.starts_with("opencode/") {
            name = line.trim().to_owned();
            block.clear();
        } else if !name.is_empty() {
            block.push_str(line);
            block.push('\n');
            if line == "}" {
                let v: Value = serde_json::from_str(&block)?;
                let zero = |pointer| v.pointer(pointer).and_then(Value::as_f64) == Some(0.0);
                if v["providerID"] == "opencode"
                    && v["id"].as_str().is_some_and(|id| name == format!("opencode/{id}"))
                    && v.pointer("/api/url").and_then(Value::as_str) == Some("https://opencode.ai/zen/v1")
                    && zero("/cost/input")
                    && zero("/cost/output")
                    && zero("/cost/cache/read")
                    && zero("/cost/cache/write")
                {
                    models.push(name.clone());
                }
                name.clear();
            }
        }
    }
    if !name.is_empty() {
        return Err(LlmError::BadOutput("opencode: incomplete model metadata".into()));
    }
    Ok(models)
}

fn inspect(line: &str) -> std::result::Result<(), String> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Ok(());
    };
    if v["type"] == "tool_use" || v.pointer("/part/type").and_then(Value::as_str) == Some("tool") {
        return Err("OpenCode attempted a tool call; completion aborted".into());
    }
    Ok(())
}

fn zen_login_state(stdout: &str) -> Option<bool> {
    let plain = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap().replace_all(stdout, "");
    let lower = plain.to_ascii_lowercase();
    if lower
        .lines()
        .any(|line| line.contains("opencode zen") && (line.contains(" api") || line.contains(" oauth")))
    {
        return Some(true);
    }
    if lower.lines().any(|line| {
        let parts: Vec<_> = line.split_whitespace().collect();
        parts
            .windows(2)
            .any(|p| p[0].parse::<u64>().is_ok() && p[1] == "credentials")
    }) {
        return Some(false);
    }
    None
}

fn parse_events(stdout: &str) -> Result<(String, Usage, String)> {
    let mut text = String::new();
    let mut usage = Usage::default();
    let mut stop = None;
    for line in stdout.lines() {
        inspect(line).map_err(LlmError::BadOutput)?;
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v["type"].as_str() {
            Some("text") => text.push_str(v.pointer("/part/text").and_then(Value::as_str).unwrap_or_default()),
            Some("step_finish") => {
                stop = v.pointer("/part/reason").and_then(Value::as_str).map(str::to_owned);
                let n = |path| v.pointer(path).and_then(Value::as_u64);
                usage = Usage {
                    input_tokens: n("/part/tokens/input"),
                    output_tokens: n("/part/tokens/output"),
                    cached_input_tokens: n("/part/tokens/cache/read"),
                    cache_creation_input_tokens: n("/part/tokens/cache/write"),
                    cost_usd: v.pointer("/part/cost").and_then(Value::as_f64),
                };
            }
            Some("error") => {
                return Err(LlmError::Provider {
                    status: v
                        .pointer("/error/data/statusCode")
                        .and_then(Value::as_u64)
                        .and_then(|n| u16::try_from(n).ok()),
                    message: "OpenCode model request failed; check the CLI's own provider status".into(),
                });
            }
            _ => {}
        }
    }
    let stop = stop
        .filter(|s| matches!(s.as_str(), "stop" | "end-turn"))
        .ok_or_else(|| LlmError::BadOutput("opencode: turn did not finish successfully".into()))?;
    if text.trim().is_empty() {
        return Err(LlmError::BadOutput("opencode: empty response".into()));
    }
    Ok((text, usage, stop))
}

#[async_trait]
impl Provider for OpenCodeCli {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenCodeCli
    }

    async fn probe(&self) -> Availability {
        let Some(launch) = &self.launch else {
            return Availability {
                installed: Some(false),
                ..Availability::not_ready("cli-absent")
            };
        };
        let version = version(launch).await;
        let mut a = Availability {
            installed: Some(true),
            version: version.clone(),
            ..Availability::default()
        };
        if !version.is_some_and(|v| v.starts_with("1.")) {
            a.reason = "unsupported-cli-version".into();
            return a;
        }
        let Ok(dir) = tempfile::tempdir() else {
            return Availability::not_ready("no-temp-dir");
        };
        let req = CompletionRequest::new(vec![]);
        let env = managed_env(dir.path(), &req);
        let auth = run(Spawn {
            launch,
            args: ["auth", "list"].map(Into::into).to_vec(),
            stdin: None,
            cwd: dir.path(),
            extra_env: env.clone(),
            deadline: PROBE_DEADLINE,
            inspect_line: None,
        })
        .await;
        a.logged_in = auth
            .ok()
            .filter(|out| out.exit_code == Some(0))
            .and_then(|out| zen_login_state(&out.stdout));
        match catalog(launch, dir.path(), &env, false).await {
            Ok(models) if !models.is_empty() => {
                a.models = models;
                // A catalog is not authentication: the local 1.2.27 public free call
                // returned HTTP 403 despite advertising free models. Do not mark a
                // signed-out client ready merely because the catalog is present.
                a.available = a.logged_in == Some(true);
                a.auth_mode = a.logged_in.filter(|b| *b).map(|_| "api-key".into());
                a.reason = match a.logged_in {
                    Some(true) => "ready",
                    Some(false) => "signed-out",
                    None => "auth-unknown",
                }
                .into();
            }
            Ok(_) => a.reason = "no-verified-free-models".into(),
            Err(_) => a.reason = "model-catalog-unavailable".into(),
        }
        a
    }

    async fn complete(&self, req: &CompletionRequest, model: &str, key: Option<&ApiKey>) -> Result<ProviderOutput> {
        if key.is_some() {
            return Err(LlmError::InvalidRequest(
                "OpenCode uses its own login or public free models; no request key is accepted".into(),
            ));
        }
        tokio::time::timeout(req.deadline, self.complete_inner(req, model))
            .await
            .map_err(|_| LlmError::Timeout(req.deadline))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_entry(model: &str, cost: f64) -> String {
        format!(
            "opencode/{model}\n{}\n",
            serde_json::to_string_pretty(&json!({
                "id": model, "providerID": "opencode", "api": {"url": "https://opencode.ai/zen/v1"},
                "cost": {"input": cost, "output": cost, "cache": {"read": cost, "write": cost}},
            }))
            .unwrap()
        )
    }

    #[test]
    fn catalog_rejects_paid_unknown_and_changed_endpoints() {
        let catalog = catalog_entry("free", 0.0) + &catalog_entry("paid", 1.0);
        assert_eq!(parse_catalog(&catalog).unwrap(), ["opencode/free"]);
        assert!(
            parse_catalog(
                &catalog_entry("free", 0.0).replace("https://opencode.ai/zen/v1", "https://other.invalid/v1")
            )
            .unwrap()
            .is_empty()
        );
        assert!(parse_catalog("opencode/free\n{\n").is_err());
    }

    #[test]
    fn events_require_complete_text_and_reject_tools() {
        let events = "{\"type\":\"text\",\"part\":{\"text\":\"{\\\"gene\\\":\\\"STXBP1\\\"}\"}}\n{\"type\":\"step_finish\",\"part\":{\"reason\":\"stop\",\"cost\":0,\"tokens\":{\"input\":8,\"output\":4,\"cache\":{\"read\":0,\"write\":0}}}}";
        let (text, usage, reason) = parse_events(events).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["gene"], "STXBP1");
        assert_eq!(usage.output_tokens, Some(4));
        assert_eq!(usage.cost_usd, Some(0.0));
        assert_eq!(reason, "stop");
        assert!(parse_events("{\"type\":\"text\",\"part\":{\"text\":\"partial\"}}").is_err());
        assert!(inspect("{\"type\":\"tool_use\",\"part\":{\"type\":\"tool\"}}").is_err());
        assert!(
            parse_events("{\"type\":\"error\",\"error\":{\"data\":{\"message\":\"secret\"}}}")
                .unwrap_err()
                .to_string()
                .contains("provider status")
        );
        assert!(matches!(
            parse_events(
                r#"{"type":"error","error":{"name":"APIError","data":{"statusCode":403,"message":"secret"}}}"#
            ),
            Err(LlmError::Provider { status: Some(403), .. })
        ));
    }

    #[test]
    fn credential_listing_never_confuses_other_logins_with_zen() {
        assert_eq!(zen_login_state("— 0 credentials"), Some(false));
        assert_eq!(zen_login_state("│ Anthropic oauth\n— 1 credentials"), Some(false));
        assert_eq!(
            zen_login_state("│ OpenCode Zen \u{1b}[90mapi\n— 1 credentials"),
            Some(true)
        );
        assert_eq!(zen_login_state("failed to list providers"), None);
    }

    #[test]
    fn config_denies_tools_and_never_uses_prompt_as_argv() {
        let req = CompletionRequest::new(vec![])
            .with_max_tokens(100)
            .with_temperature(0.0);
        let env = managed_env(Path::new("/tmp/atlas-test"), &req);
        let config: Value =
            serde_json::from_str(&env.iter().find(|(k, _)| k == "OPENCODE_CONFIG_CONTENT").unwrap().1).unwrap();
        assert_eq!(config["permission"]["*"], "deny");
        assert_eq!(config["agent"][AGENT]["permission"]["*"], "deny");
        assert_eq!(config["share"], "disabled");
        assert!(
            env.iter()
                .any(|(k, v)| k == "OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX" && v == "100")
        );
        assert_eq!(run_args(DEFAULT_MODEL)[4], DEFAULT_MODEL);
    }

    #[tokio::test]
    async fn native_subprocess_contract_preserves_stdin_schema_and_selected_model() {
        let Some(node) = which_native("node") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fixture.js");
        std::fs::write(&script, r#"
            const args = process.argv.slice(2);
            if (args[0] === '--version') { console.log('1.2.27'); process.exit(0); }
            if (args[0] === 'models') {
                console.log('opencode/big-pickle');
                console.log(JSON.stringify({id:'big-pickle',providerID:'opencode',api:{url:'https://opencode.ai/zen/v1'},cost:{input:0,output:0,cache:{read:0,write:0}}}, null, 2));
                process.exit(0);
            }
            let stdin = '';
            process.stdin.on('data', chunk => stdin += chunk);
            process.stdin.on('end', () => {
                const config = JSON.parse(process.env.OPENCODE_CONFIG_CONTENT);
                if (args[4] !== 'opencode/big-pickle' || !stdin.includes('JSON Schema') || !stdin.includes('STXBP1') || config.permission['*'] !== 'deny') process.exit(9);
                console.log(JSON.stringify({type:'text',part:{text:JSON.stringify({gene:'STXBP1'})}}));
                console.log(JSON.stringify({type:'step_finish',part:{reason:'stop',cost:0,tokens:{input:10,output:4,cache:{read:0,write:0}}}}));
            });
        "#).unwrap();
        let provider = OpenCodeCli {
            launch: Some(Launch::Node { node, script }),
            gate: Semaphore::new(1),
        };
        let req =
            CompletionRequest::new(vec![crate::Message::user("Find STXBP1")]).with_schema(crate::JsonSchema::new(
                "gene",
                json!({"type":"object","properties":{"gene":{"type":"string"}},"required":["gene"]}),
            ));
        let out = provider.complete(&req, DEFAULT_MODEL, None).await.unwrap();
        assert_eq!(serde_json::from_str::<Value>(&out.text).unwrap()["gene"], "STXBP1");
        assert_eq!(out.sent["model"], DEFAULT_MODEL);
        assert_eq!(out.usage.cost_usd, Some(0.0));
        assert!(provider.complete(&req, "opencode/paid", None).await.is_err());
        assert!(
            provider
                .complete(&req, DEFAULT_MODEL, Some(&ApiKey::new("test-only")))
                .await
                .is_err()
        );
        let mut registry = crate::Registry::default();
        registry.insert(crate::registry::Connection {
            config: crate::ConnectionConfig {
                name: "fixture-opencode".into(),
                kind: Some(ProviderKind::OpenCodeCli),
                default_model: Some(DEFAULT_MODEL.into()),
                ..Default::default()
            },
            kind: ProviderKind::OpenCodeCli,
            key_policy: crate::KeyPolicy::None,
            provider: std::sync::Arc::new(provider),
        });
        let llm = crate::Llm::new(
            registry,
            crate::Cache::new(dir.path().join("cache"), crate::CacheMode::Off),
        );
        let done = llm
            .complete_json::<Value>(&crate::Call::new("fixture-opencode").with_private(), req)
            .await
            .unwrap();
        assert_eq!(done.json["gene"], "STXBP1");
        assert_eq!(done.calls.len(), 1);
        assert_eq!(done.calls[0].response.requested_model, DEFAULT_MODEL);
        assert_eq!(done.calls[0].response.json.as_ref().unwrap()["gene"], "STXBP1");
    }

    #[tokio::test]
    async fn deadline_includes_waiting_for_the_serialized_slot() {
        let Some(node) = which_native("node") else {
            return;
        };
        let provider = OpenCodeCli {
            launch: Some(Launch::Exe(node)),
            gate: Semaphore::new(0),
        };
        let req = CompletionRequest::new(vec![]).with_deadline(std::time::Duration::from_millis(10));
        assert!(matches!(
            provider.complete(&req, DEFAULT_MODEL, None).await,
            Err(LlmError::Timeout(_))
        ));
    }
}
