//! Claude Code (`claude -p`) with every tool, MCP server, hook, skill and setting source off.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Semaphore;

use super::{Finished, Launch, PROBE_DEADLINE, Spawn, exit_error, prompt_parts, run, version, which_native};
use crate::error::{LlmError, Result, truncate};
use crate::provider::{Availability, Provider, ProviderKind};
use crate::request::{CompletionRequest, ProviderOutput, Usage};
use crate::secret::ApiKey;

#[derive(Debug)]
pub struct ClaudeCode {
    launch: Option<Launch>,
    gate: Semaphore,
}

impl ClaudeCode {
    /// `executable`: explicit native path; default = `claude` on PATH (a real executable only).
    pub fn new(executable: Option<PathBuf>, max_parallel: usize) -> Result<Self> {
        let launch = match executable {
            Some(p) => Some(Launch::from_path(&p)?),
            None => which_native("claude").map(Launch::Exe),
        };
        Ok(Self {
            launch,
            gate: Semaphore::new(max_parallel.max(1)),
        })
    }

    /// Fixed restriction flags (argv, never a shell string).
    fn restricted_args(model: &str, system_file: Option<&std::path::Path>, schema_flag: Option<&str>) -> Vec<OsString> {
        let mut a: Vec<OsString> = [
            "-p",
            "--output-format",
            "json",
            "--tools",
            "",
            "--strict-mcp-config",
            "--setting-sources",
            "",
            "--settings",
            r#"{"disableAllHooks":true}"#,
            "--disable-slash-commands",
            "--no-session-persistence",
            "--permission-mode",
            "dontAsk",
            "--model",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        a.push(model.into());
        if let Some(f) = system_file {
            a.push("--system-prompt-file".into());
            a.push(f.as_os_str().to_owned());
        }
        if let Some(s) = schema_flag {
            a.push("--json-schema".into());
            a.push(s.into());
        }
        a
    }
}

/// Parse `claude -p --output-format json` (one result object).
pub(crate) fn parse_result(out: &Finished) -> Result<(String, Option<String>, Usage, Option<String>)> {
    let v: Value = serde_json::from_str(out.stdout.trim())
        .map_err(|_| LlmError::BadOutput(format!("claude: not JSON: {}", truncate(&out.stdout, 300))))?;
    if v.get("type").and_then(Value::as_str) != Some("result") {
        return Err(LlmError::BadOutput("claude: no result object".into()));
    }
    let text = v.get("result").and_then(Value::as_str).unwrap_or_default().to_owned();
    if v.get("is_error").and_then(Value::as_bool) == Some(true) {
        let lower = text.to_ascii_lowercase();
        if lower.contains("login") || lower.contains("auth") {
            return Err(LlmError::Auth(format!("claude: {}", truncate(&text, 300))));
        }
        return Err(LlmError::Provider {
            status: None,
            message: format!("claude: {}", truncate(&text, 500)),
        });
    }
    if let Some(d) = v.get("permission_denials").and_then(Value::as_array)
        && !d.is_empty()
    {
        return Err(LlmError::BadOutput("claude attempted a tool call (denied)".into()));
    }
    // modelUsage keys are the model ids actually used; take the one with most output tokens.
    let reported = v.get("modelUsage").and_then(Value::as_object).and_then(|m| {
        m.iter()
            .max_by_key(|(_, u)| u.get("outputTokens").and_then(Value::as_u64).unwrap_or(0))
            .map(|(k, _)| k.clone())
    });
    let u = v.get("usage");
    let num = |k: &str| u.and_then(|u| u.get(k)).and_then(Value::as_u64);
    let usage = Usage {
        input_tokens: num("input_tokens"),
        output_tokens: num("output_tokens"),
        cached_input_tokens: num("cache_read_input_tokens"),
        cache_creation_input_tokens: num("cache_creation_input_tokens"),
        cost_usd: None,
    };
    let stop = v.get("stop_reason").and_then(Value::as_str).map(str::to_owned);
    Ok((text, reported, usage, stop))
}

/// `claude auth status --json` → (logged_in, auth_mode). Only these two fields are read.
pub(crate) fn parse_auth(stdout: &str) -> (Option<bool>, Option<String>) {
    let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) else {
        return (None, None);
    };
    let logged_in = v.get("loggedIn").and_then(Value::as_bool);
    let mode = match v.get("authMethod").and_then(Value::as_str) {
        Some("claude.ai") => "subscription",
        Some("apiKey") => "api-key",
        Some(_) => "unknown",
        None => return (logged_in, None),
    };
    (logged_in, Some(mode.into()))
}

#[async_trait]
impl Provider for ClaudeCode {
    fn kind(&self) -> ProviderKind {
        ProviderKind::ClaudeCode
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
        let auth = run(Spawn {
            launch,
            args: ["auth", "status", "--json"].map(OsString::from).to_vec(),
            stdin: None,
            cwd: dir.path(),
            extra_env: vec![],
            deadline: PROBE_DEADLINE,
            inspect_line: None,
        })
        .await;
        let (logged_in, mode) = auth.map(|o| parse_auth(&o.stdout)).unwrap_or((None, None));
        a.logged_in = logged_in;
        a.auth_mode = mode;
        a.available = logged_in == Some(true);
        a.reason = match logged_in {
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
                "Claude Code uses its own login; no key is accepted".into(),
            ));
        }
        let launch = self
            .launch
            .as_ref()
            .ok_or_else(|| LlmError::Unavailable("claude CLI not installed".into()))?;
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| LlmError::Unavailable("closed".into()))?;
        let cli_version = version(launch).await;
        let dir = tempfile::tempdir()?;
        let (system, prompt) = prompt_parts(req, true);
        let system_file = if system.is_empty() {
            None
        } else {
            let f = dir.path().join("system.txt");
            std::fs::write(&f, &system)?;
            Some(f)
        };
        let mut sent = BTreeMap::new();
        sent.insert("model".into(), model.to_owned());
        sent.insert(
            "restrictions".into(),
            "tools=none,mcp=none,hooks=off,settings=none,session=ephemeral".into(),
        );
        let mut extra_env = vec![];
        if let Some(n) = req.settings.max_tokens {
            extra_env.push(("CLAUDE_CODE_MAX_OUTPUT_TOKENS".into(), n.to_string()));
            sent.insert("max_tokens".into(), n.to_string());
        }
        if req.settings.temperature.is_some() {
            sent.insert("temperature".into(), "not-supported-by-cli".into());
        }
        if req.schema.is_some() {
            sent.insert("response_format".into(), "schema-in-prompt".into());
        }
        let out = run(Spawn {
            launch,
            args: Self::restricted_args(model, system_file.as_deref(), None),
            stdin: Some(&prompt),
            cwd: dir.path(),
            extra_env,
            deadline: req.deadline,
            inspect_line: None,
        })
        .await?;
        if out.exit_code != Some(0) && !out.stdout.trim_start().starts_with('{') {
            return Err(exit_error("claude", &out));
        }
        let (text, reported, usage, stop) = parse_result(&out)?;
        Ok(ProviderOutput {
            text,
            reported_model: reported,
            usage,
            stop_reason: stop,
            agent_version: cli_version.map(|v| format!("claude-code {v}")),
            sent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(stdout: &str) -> Finished {
        Finished {
            exit_code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    #[test]
    fn parses_result() {
        let out = finished(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"hello",
            "usage":{"input_tokens":12,"output_tokens":3,"cache_read_input_tokens":5},
            "modelUsage":{"claude-haiku-x":{"outputTokens":3}},"permission_denials":[]}"#,
        );
        let (t, m, u, _) = parse_result(&out).unwrap();
        assert_eq!((t.as_str(), m.as_deref()), ("hello", Some("claude-haiku-x")));
        assert_eq!(u.input_tokens, Some(12));
    }

    #[test]
    fn denied_tool_is_an_error() {
        let out = finished(r#"{"type":"result","is_error":false,"result":"x","permission_denials":[{"tool":"Bash"}]}"#);
        assert!(matches!(parse_result(&out), Err(LlmError::BadOutput(_))));
    }

    #[test]
    fn auth_reads_only_two_fields() {
        assert_eq!(
            parse_auth(r#"{"loggedIn":true,"authMethod":"claude.ai","email":"x@y"}"#),
            (Some(true), Some("subscription".into()))
        );
        assert_eq!(parse_auth(r#"{"loggedIn":false}"#), (Some(false), None));
    }
}
