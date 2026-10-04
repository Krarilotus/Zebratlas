//! Google Gemini CLI (`gemini -p --output-format json`), tool-free through a managed system
//! settings file (core tools, discovery, MCP, extensions, hooks, memory, telemetry off; GTA's
//! managed settings). Not installed on the dev machine: unit-tested only.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use super::{Launch, Spawn, exit_error, npm_bin_dir, prompt_parts, run, version, which_native};
use crate::error::{LlmError, Result, truncate};
use crate::provider::{Availability, Provider, ProviderKind};
use crate::request::{CompletionRequest, ProviderOutput, Usage};
use crate::secret::ApiKey;

#[derive(Debug)]
pub struct GeminiCli {
    launch: Option<Launch>,
    gate: Semaphore,
}

impl GeminiCli {
    pub fn new(executable: Option<PathBuf>, max_parallel: usize) -> Result<Self> {
        let launch = match executable {
            Some(p) if p.extension().is_some_and(|e| e == "js") => {
                let node = which_native("node").ok_or_else(|| LlmError::Unavailable("node not found".into()))?;
                Some(Launch::Node { node, script: p })
            }
            Some(p) => Some(Launch::from_path(&p)?),
            None => resolve(),
        };
        Ok(Self {
            launch,
            gate: Semaphore::new(max_parallel.max(1)),
        })
    }
}

/// `gemini` on PATH if it is a direct executable; on Windows `node <npm>/@google/gemini-cli/bundle/gemini.js`.
fn resolve() -> Option<Launch> {
    if let Some(p) = which_native("gemini") {
        return Some(Launch::Exe(p));
    }
    let script = npm_bin_dir("gemini")?
        .join("node_modules")
        .join("@google")
        .join("gemini-cli")
        .join("bundle")
        .join("gemini.js");
    let node = which_native("node")?;
    script.is_file().then_some(Launch::Node { node, script })
}

/// Settings that win over user/project settings (system scope).
pub(crate) fn managed_settings(max_tokens: Option<u32>, temperature: Option<f64>, model: &str) -> Value {
    let mut s = json!({
        "model": {"maxSessionTurns": 1, "skipNextSpeakerCheck": true},
        "general": {"maxAttempts": 1, "enableAutoUpdate": false},
        "tools": {"core": [], "exclude": ["run_shell_command", "write_file", "replace", "web_fetch", "google_web_search"],
                  "discoveryCommand": "", "callCommand": "", "useRipgrep": false},
        "mcpServers": {},
        "context": {"fileName": [], "includeDirectoryTree": false, "includeDirectories": []},
        "ide": {"enabled": false}, "skills": {"enabled": false}, "hooksConfig": {"enabled": false},
        "experimental": {"enableAgents": false, "autoMemory": false},
        "advanced": {"ignoreLocalEnv": true, "autoConfigureMemory": false},
        "telemetry": {"enabled": false}, "privacy": {"usageStatisticsEnabled": false},
    });
    let mut gen_cfg = serde_json::Map::new();
    if let Some(n) = max_tokens {
        gen_cfg.insert("maxOutputTokens".into(), n.into());
    }
    if let Some(t) = temperature {
        gen_cfg.insert("temperature".into(), t.into());
    }
    if !gen_cfg.is_empty() {
        s["modelConfigs"] =
            json!({"overrides": [{"match": {"model": model}, "modelConfig": {"generateContentConfig": gen_cfg}}]});
    }
    s
}

pub(crate) fn parse_output(stdout: &str) -> Result<(String, Option<String>, Usage)> {
    let start = stdout
        .find('{')
        .ok_or_else(|| LlmError::BadOutput(format!("gemini: not JSON: {}", truncate(stdout, 300))))?;
    let v: Value = serde_json::from_str(&stdout[start..])?;
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        let msg = e.get("message").and_then(Value::as_str).unwrap_or("unknown error");
        return Err(LlmError::Provider {
            status: None,
            message: format!("gemini: {}", truncate(msg, 500)),
        });
    }
    let text = v.get("response").and_then(Value::as_str).unwrap_or_default().to_owned();
    let models = v.pointer("/stats/models").and_then(Value::as_object);
    let (reported, usage) = models
        .and_then(|m| m.iter().next())
        .map(|(name, s)| {
            let tok = |k: &str| s.pointer(&format!("/tokens/{k}")).and_then(Value::as_u64);
            (
                Some(name.clone()),
                Usage {
                    input_tokens: tok("prompt"),
                    output_tokens: tok("candidates"),
                    cached_input_tokens: tok("cached"),
                    cache_creation_input_tokens: None,
                    cost_usd: None,
                },
            )
        })
        .unwrap_or_default();
    Ok((text, reported, usage))
}

#[async_trait]
impl Provider for GeminiCli {
    fn kind(&self) -> ProviderKind {
        ProviderKind::GeminiCli
    }

    async fn probe(&self) -> Availability {
        let Some(launch) = &self.launch else {
            return Availability {
                installed: Some(false),
                ..Availability::not_ready("cli-absent")
            };
        };
        let version = version(launch).await;
        // Login state: only whether the CLI's OAuth file exists; it is never opened.
        let home = std::env::var_os("GEMINI_CLI_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from));
        let logged_in = home
            .map(|h| h.join(".gemini").join("oauth_creds.json").is_file())
            .filter(|b| *b);
        Availability {
            available: version.is_some() && logged_in == Some(true),
            installed: Some(true),
            version,
            logged_in,
            auth_mode: logged_in.map(|_| "subscription".into()),
            models: vec![],
            reason: if logged_in == Some(true) {
                "ready"
            } else {
                "auth-unknown"
            }
            .into(),
        }
    }

    async fn complete(&self, req: &CompletionRequest, model: &str, key: Option<&ApiKey>) -> Result<ProviderOutput> {
        if key.is_some() {
            return Err(LlmError::InvalidRequest(
                "Gemini CLI uses its own login; no key is accepted".into(),
            ));
        }
        let launch = self
            .launch
            .as_ref()
            .ok_or_else(|| LlmError::Unavailable("gemini CLI not installed".into()))?;
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| LlmError::Unavailable("closed".into()))?;
        let cli_version = version(launch).await;
        let dir = tempfile::tempdir()?;
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace)?;
        let settings_path = dir.path().join("system-settings.json");
        let settings = managed_settings(req.settings.max_tokens, req.settings.temperature, model);
        std::fs::write(&settings_path, settings.to_string())?;
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
            "tools=none,mcp=none,extensions=none,hooks=off,turns=1".into(),
        );
        if let Some(t) = req.settings.temperature {
            sent.insert("temperature".into(), t.to_string());
        }
        if let Some(n) = req.settings.max_tokens {
            sent.insert("max_tokens".into(), n.to_string());
        }
        if req.schema.is_some() {
            sent.insert("response_format".into(), "schema-in-prompt".into());
        }
        let args: Vec<OsString> = [
            "-p",
            "Answer the request given above.",
            "--output-format",
            "json",
            "-e",
            "none",
            "--approval-mode",
            "default",
            "-m",
            model,
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let out = run(Spawn {
            launch,
            args,
            stdin: Some(&stdin),
            cwd: &workspace,
            extra_env: vec![
                (
                    "GEMINI_CLI_SYSTEM_SETTINGS_PATH".into(),
                    settings_path.display().to_string(),
                ),
                ("GEMINI_CLI_NO_RELAUNCH".into(), "1".into()),
                ("NO_BROWSER".into(), "true".into()),
            ],
            deadline: req.deadline,
            inspect_line: None,
        })
        .await?;
        let parsed = parse_output(&out.stdout);
        if out.exit_code != Some(0) && parsed.is_err() {
            return Err(exit_error("gemini", &out));
        }
        let (text, reported, usage) = parsed?;
        Ok(ProviderOutput {
            text,
            reported_model: reported,
            usage,
            stop_reason: None,
            agent_version: cli_version.map(|v| format!("gemini-cli {v}")),
            sent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_output() {
        let s = r#"{"response":"hi","stats":{"models":{"gemini-2.5-flash":{"tokens":{"prompt":5,"candidates":1,"cached":0}}}}}"#;
        let (t, m, u) = parse_output(s).unwrap();
        assert_eq!(
            (t.as_str(), m.as_deref(), u.input_tokens),
            ("hi", Some("gemini-2.5-flash"), Some(5))
        );
        assert!(parse_output(r#"{"error":{"message":"quota"}}"#).is_err());
    }

    #[test]
    fn settings_disable_tools() {
        let s = managed_settings(Some(10), None, "m");
        assert_eq!(s["tools"]["core"], json!([]));
        assert_eq!(
            s["modelConfigs"]["overrides"][0]["modelConfig"]["generateContentConfig"]["maxOutputTokens"],
            10
        );
    }
}
