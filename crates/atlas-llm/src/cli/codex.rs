//! OpenAI Codex CLI (`codex exec --json`): read-only sandbox, ephemeral, user config, rules and
//! every optional tool feature off, empty temp workspace. The JSONL event stream is inspected
//! live: any tool item (command, file change, MCP, web search) kills the process.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Semaphore;

use super::{Launch, PROBE_DEADLINE, Spawn, exit_error, npm_bin_dir, prompt_parts, run, version, which_native};
use crate::error::{LlmError, Result, truncate};
use crate::provider::{Availability, Provider, ProviderKind};
use crate::request::{CompletionRequest, ProviderOutput, Usage};
use crate::secret::ApiKey;

/// Features switched off for every run (checked against codex-cli 0.160 `features list`).
const DISABLED_FEATURES: &[&str] = &[
    "shell_tool",
    "unified_exec",
    "view_image",
    "multi_agent",
    "multi_agent_v2",
    "goals",
    "apps",
    "plugins",
    "remote_plugin",
    "in_app_browser",
    "standalone_web_search",
    "hooks",
    "skill_mcp_dependency_install",
    "skill_search",
    "memories",
    "sleep_tool",
    "image_generation",
    "code_mode",
    "code_mode_host",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "tool_suggest",
    "js_repl",
    "shell_snapshot",
];
/// Item types a pure text answer may contain.
const SAFE_ITEMS: &[&str] = &["agent_message", "reasoning", "error"];
/// `model` value meaning "the CLI's own default model" (no `-m`).
pub const CLI_DEFAULT_MODEL: &str = "default";

#[derive(Debug)]
pub struct CodexCli {
    launch: Option<Launch>,
    gate: Semaphore,
}

impl CodexCli {
    pub fn new(executable: Option<PathBuf>, max_parallel: usize) -> Result<Self> {
        let launch = match executable {
            Some(p) => Some(Launch::from_path(&p)?),
            None => resolve_native().map(Launch::Exe),
        };
        Ok(Self {
            launch,
            gate: Semaphore::new(max_parallel.max(1)),
        })
    }

    fn args(model: &str, workspace: &Path) -> Vec<OsString> {
        let mut a: Vec<OsString> = vec![
            "exec".into(),
            "--json".into(),
            "--skip-git-repo-check".into(),
            "--ephemeral".into(),
            "--ignore-user-config".into(),
            "--ignore-rules".into(),
            "--sandbox".into(),
            "read-only".into(),
            "--color".into(),
            "never".into(),
            "-C".into(),
            workspace.as_os_str().to_owned(),
        ];
        for c in [
            r#"web_search="disabled""#,
            r#"history.persistence="none""#,
            r#"approval_policy="never""#,
            "project_doc_max_bytes=0",
        ] {
            a.push("-c".into());
            a.push(c.into());
        }
        for f in DISABLED_FEATURES {
            a.push("--disable".into());
            a.push((*f).into());
        }
        if model != CLI_DEFAULT_MODEL {
            a.push("-m".into());
            a.push(model.into());
        }
        a.push("-".into()); // prompt from stdin
        a
    }
}

/// On Windows the npm `codex.cmd` shim runs node through `cmd.exe`; start the package's native
/// binary instead (the GTA rule). Elsewhere `codex` on PATH is started directly.
fn resolve_native() -> Option<PathBuf> {
    if let Some(p) = which_native("codex") {
        return Some(p);
    }
    let npm = npm_bin_dir("codex")?;
    let root = npm.join("node_modules").join("@openai");
    let platforms = [
        ("codex-win32-x64", "x86_64-pc-windows-msvc"),
        ("codex-win32-arm64", "aarch64-pc-windows-msvc"),
    ];
    for (pkg, triple) in platforms {
        let rel = Path::new(pkg).join("vendor").join(triple).join("bin").join("codex.exe");
        for base in [root.join("codex").join("node_modules").join("@openai"), root.clone()] {
            let p = base.join(&rel);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Live check of one JSONL event: only text/reasoning items are allowed.
pub(crate) fn inspect(line: &str) -> std::result::Result<(), String> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Ok(());
    };
    let ty = v.get("type").and_then(Value::as_str).unwrap_or_default();
    if ty.starts_with("item.") {
        let item = v
            .get("item")
            .and_then(|i| i.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if !SAFE_ITEMS.contains(&item) {
            return Err(format!("codex attempted a tool item '{item}'; process killed"));
        }
    }
    Ok(())
}

/// Collect the final answer and usage from the JSONL stream.
pub(crate) fn parse_events(stdout: &str) -> Result<(String, Usage)> {
    let mut text = String::new();
    let mut usage = Usage::default();
    let mut completed = false;
    for line in stdout.lines().filter(|l| l.trim_start().starts_with('{')) {
        let v: Value = serde_json::from_str(line)?;
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "item.completed" => {
                let item = &v["item"];
                if item.get("type").and_then(Value::as_str) == Some("agent_message")
                    && let Some(t) = item.get("text").and_then(Value::as_str)
                {
                    text = t.to_owned(); // last message wins
                }
            }
            "turn.completed" => {
                completed = true;
                let u = &v["usage"];
                usage = Usage {
                    input_tokens: u.get("input_tokens").and_then(Value::as_u64),
                    output_tokens: u.get("output_tokens").and_then(Value::as_u64),
                    cached_input_tokens: u.get("cached_input_tokens").and_then(Value::as_u64),
                    cache_creation_input_tokens: None,
                    cost_usd: None,
                };
            }
            "turn.failed" | "error" => {
                let msg = v
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .or_else(|| v.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                let lower = msg.to_ascii_lowercase();
                if lower.contains("login") || lower.contains("auth") || lower.contains("401") {
                    return Err(LlmError::Auth(format!("codex: {}", truncate(msg, 300))));
                }
                return Err(LlmError::Provider {
                    status: None,
                    message: format!("codex: {}", truncate(msg, 500)),
                });
            }
            _ => {}
        }
    }
    if !completed {
        return Err(LlmError::BadOutput("codex: turn did not complete".into()));
    }
    Ok((text, usage))
}

#[async_trait]
impl Provider for CodexCli {
    fn kind(&self) -> ProviderKind {
        ProviderKind::CodexCli
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
        if version.is_none() {
            a.reason = "version-unavailable".into();
            return a;
        }
        let Ok(dir) = tempfile::tempdir() else {
            return Availability::not_ready("no-temp-dir");
        };
        let out = run(Spawn {
            launch,
            args: ["login", "status"].map(OsString::from).to_vec(),
            stdin: None,
            cwd: dir.path(),
            extra_env: vec![],
            deadline: PROBE_DEADLINE,
            inspect_line: None,
        })
        .await;
        if let Ok(out) = out {
            let text = format!("{}\n{}", out.stdout, out.stderr).to_ascii_lowercase();
            if out.exit_code == Some(0) && text.contains("logged in") {
                a.logged_in = Some(true);
                a.auth_mode = Some(
                    if text.contains("chatgpt") {
                        "subscription"
                    } else if text.contains("api key") {
                        "api-key"
                    } else {
                        "unknown"
                    }
                    .into(),
                );
            } else if text.contains("not logged in") {
                a.logged_in = Some(false);
            }
        }
        a.available = a.logged_in == Some(true);
        a.reason = match a.logged_in {
            Some(true) => "ready",
            Some(false) => "signed-out",
            None => "auth-unknown",
        }
        .into();
        a
    }

    async fn complete(&self, req: &CompletionRequest, model: &str, key: Option<&ApiKey>) -> Result<ProviderOutput> {
        if key.is_some() {
            return Err(LlmError::InvalidRequest(
                "Codex uses its own login; no key is accepted".into(),
            ));
        }
        let launch = self
            .launch
            .as_ref()
            .ok_or_else(|| LlmError::Unavailable("codex CLI not installed".into()))?;
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| LlmError::Unavailable("closed".into()))?;
        let cli_version = version(launch).await;
        let dir = tempfile::tempdir()?;
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace)?;
        // Codex exec has no separate system flag without user config; system text leads the prompt.
        let (system, prompt) = prompt_parts(req, true);
        let stdin = if system.is_empty() {
            prompt
        } else {
            format!("{system}\n\n{prompt}")
        };
        let mut sent = BTreeMap::new();
        sent.insert("model".into(), model.to_owned());
        sent.insert(
            "restrictions".into(),
            "sandbox=read-only,ephemeral,user-config=ignored,tool-features=off".into(),
        );
        if req.settings.temperature.is_some() {
            sent.insert("temperature".into(), "not-supported-by-cli".into());
        }
        if req.settings.max_tokens.is_some() {
            sent.insert("max_tokens".into(), "not-supported-by-cli".into());
        }
        if req.schema.is_some() {
            sent.insert("response_format".into(), "schema-in-prompt".into());
        }
        let check = inspect;
        let out = run(Spawn {
            launch,
            args: Self::args(model, &workspace),
            stdin: Some(&stdin),
            cwd: &workspace,
            extra_env: vec![],
            deadline: req.deadline,
            inspect_line: Some(&check),
        })
        .await?;
        let parsed = parse_events(&out.stdout);
        if out.exit_code != Some(0) && parsed.is_err() {
            return Err(parsed
                .err()
                .filter(|e| !matches!(e, LlmError::BadOutput(_)))
                .unwrap_or_else(|| exit_error("codex", &out)));
        }
        let (text, usage) = parsed?;
        Ok(ProviderOutput {
            text,
            // `codex exec --json` does not report the served model.
            reported_model: None,
            usage,
            stop_reason: Some("turn.completed".into()),
            agent_version: cli_version.map(|v| format!("codex-cli {v}")),
            sent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_items_are_rejected() {
        assert!(inspect(r#"{"type":"item.started","item":{"type":"command_execution","command":"ls"}}"#).is_err());
        assert!(inspect(r#"{"type":"item.completed","item":{"type":"agent_message","text":"hi"}}"#).is_ok());
        assert!(inspect("not json").is_ok());
    }

    #[test]
    fn parses_stream() {
        let s = "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n\
                 {\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"ok\"}}\n\
                 {\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":9,\"cached_input_tokens\":0,\"output_tokens\":2}}\n";
        let (t, u) = parse_events(s).unwrap();
        assert_eq!((t.as_str(), u.output_tokens), ("ok", Some(2)));
        assert!(parse_events("{\"type\":\"turn.failed\",\"error\":{\"message\":\"boom\"}}").is_err());
    }

    #[test]
    fn args_have_no_shell_and_read_stdin() {
        let a = CodexCli::args("gpt-x", Path::new("/tmp/w"));
        assert_eq!(a.last().unwrap(), "-");
        assert!(a.iter().any(|x| x == "read-only"));
        assert!(
            !CodexCli::args(CLI_DEFAULT_MODEL, Path::new("/tmp/w"))
                .iter()
                .any(|x| x == "-m")
        );
    }
}
