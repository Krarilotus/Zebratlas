use crate::{
    mcp, now,
    protocol::{Capability, Failure, Frame, MAX_FRAME, VERSION},
    random_token,
};

struct AbortTask(tokio::task::JoinHandle<()>);
impl AbortTask {
    fn abort(&self) {
        self.0.abort();
    }
    fn is_finished(&self) -> bool {
        self.0.is_finished()
    }
}
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
use atlas_llm::{ApiKey, Llm, ProviderKind};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

pub fn loopback(host: &str) -> bool {
    host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()) || host == "[::1]"
}
pub fn origin(value: &str) -> anyhow::Result<url::Url> {
    let u = url::Url::parse(value)?;
    anyhow::ensure!(
        u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.path() == "/"
            && (u.scheme() == "https" || (u.scheme() == "http" && u.host_str().is_some_and(loopback))),
        "use an HTTPS origin (HTTP only on loopback)"
    );
    Ok(u)
}
fn credential(origin: &url::Url) -> anyhow::Result<keyring::Entry> {
    Ok(keyring::Entry::new(
        "org.zebratlas.connector",
        &origin.origin().ascii_serialization(),
    )?)
}
pub fn forget(origin: &url::Url) -> anyhow::Result<()> {
    credential(origin)?.delete_credential()?;
    Ok(())
}
pub async fn pair(origin: &url::Url, label: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let response = client
        .post(origin.join("api/connector/pair")?)
        .json(&serde_json::json!({"label":label}))
        .send()
        .await?;
    anyhow::ensure!(response.status().is_success(), "pairing refused");
    let value: Value = response.json().await?;
    let code = value["device_code"]
        .as_str()
        .filter(|c| c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow::anyhow!("invalid device code"))?;
    let short = value["user_code"]
        .as_str()
        .filter(|c| c.len() == 12 && c.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow::anyhow!("invalid user code"))?;
    // Verify credential storage BEFORE consuming the pairing; no plaintext fallback.
    let entry = credential(origin)?;
    entry.set_password(code)?;
    anyhow::ensure!(entry.get_password()? == code, "credential store verification failed");
    eprintln!("Approve code {short} in your Zebratlas account. Only approve a code from your own connector.");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let response = client
            .post(origin.join("api/connector/poll")?)
            .json(&serde_json::json!({"device_code":code}))
            .send()
            .await?;
        // A blocking HTTP status ends acquisition; never retry via another route.
        anyhow::ensure!(
            response.status().is_success(),
            "pairing expired or refused; start a new pairing"
        );
        let poll: Value = response.json().await?;
        if poll["status"] == "approved" {
            eprintln!("Paired.");
            return Ok(());
        }
        anyhow::ensure!(poll["status"] == "authorization_pending", "invalid pairing response");
    }
    anyhow::bail!("pairing expired")
}

/// Local permission is an explicit allow-list. Only localhost compatible servers and
/// known provider adapters; no hosted-free key or recursively connected backend.
pub fn capabilities(llm: &Llm, allow: &[String]) -> anyhow::Result<Vec<Capability>> {
    anyhow::ensure!(allow.len() <= 16, "too many enabled connections");
    let mut caps = Vec::new();
    for name in allow {
        let c = llm.registry().get(name)?;
        anyhow::ensure!(
            c.kind != ProviderKind::Connector && c.config.free_tier != Some(true) && c.config.demo_only != Some(true),
            "connection cannot be shared"
        );
        if c.kind == ProviderKind::OpenAiCompatible {
            let url = url::Url::parse(c.provider.base_url().unwrap_or(""))?;
            anyhow::ensure!(
                url.host_str().is_some_and(loopback)
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "compatible models must use a loopback server"
            );
        }
        caps.push(Capability {
            connection: name.clone(),
            kind: c.kind,
            default_model: c.config.default_model.clone(),
            models: c.config.models.clone(),
        });
    }
    Ok(caps)
}

/// Run one call directly through its Provider so the original provider output (reported
/// model, software version, actual parameters) reaches atlas-llm's hosted PROV recorder.
pub async fn execute(
    llm: &Llm,
    allow: &[String],
    connection: &str,
    mut request: atlas_llm::CompletionRequest,
    expires: i64,
) -> Result<atlas_llm::request::ProviderOutput, Failure> {
    if !allow.iter().any(|c| c == connection)
        || expires <= now()
        || request.deadline.is_zero()
        || request.deadline > Duration::from_secs(120)
    {
        return Err(Failure::Rejected);
    }
    if serde_json::to_vec(&request).map_or(true, |bytes| bytes.len() > 64 * 1024) {
        return Err(Failure::Rejected);
    }
    let remaining = expires.saturating_sub(now());
    if remaining <= 0 {
        return Err(Failure::Timeout);
    }
    request.deadline = request.deadline.min(Duration::from_secs(remaining as u64));
    let c = llm.registry().get(connection).map_err(|_| Failure::Rejected)?;
    let model = request
        .model
        .as_deref()
        .or(c.config.default_model.as_deref())
        .ok_or(Failure::Rejected)?;
    if model.is_empty() || model.len() > 160 || model.starts_with('-') || model.chars().any(char::is_control) {
        return Err(Failure::Rejected);
    }
    if !c.config.models.is_empty() && !c.config.models.iter().any(|m| m == model) {
        return Err(Failure::Rejected);
    }
    let key = if c.kind.is_cli() {
        None
    } else {
        c.config.key_env.as_deref().and_then(ApiKey::from_env)
    };
    if c.key_policy.needs_key() && key.is_none() {
        return Err(Failure::Unavailable);
    }
    let output = tokio::time::timeout(request.deadline, c.provider.complete(&request, model, key.as_ref()))
        .await
        .map_err(|_| Failure::Timeout)?
        .map_err(|_| Failure::ProviderFailed)?;
    let mut output = output;
    // URLs may embed local gateway credentials. Export the provider kind, never its local URL.
    output.sent.remove("base_url");
    output.sent.insert("local.provider".into(), c.kind.as_str().into());
    output.sent.insert("local.connection".into(), connection.into());
    output.sent.insert("connector.protocol".into(), VERSION.to_string());
    Ok(output)
}

pub async fn run(origin: &url::Url, llm: Arc<Llm>, allow: Vec<String>, stdio: bool) -> anyhow::Result<()> {
    let token = atlas_accounts::auth::Secret::new(credential(origin)?.get_password()?);
    let caps = capabilities(&llm, &allow)?;
    let (mcp_tx, mut mcp_rx) = mpsc::channel::<Vec<u8>>(8);
    let stdin_task = if stdio {
        Some(AbortTask(tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
            while let Ok(Some(line)) = mcp::line(&mut reader).await {
                if mcp_tx.send(line).await.is_err() {
                    break;
                }
            }
        })))
    } else {
        None
    };
    let mut initialized = false;
    let mut failures = 0u32;
    loop {
        let mut url = origin.join("api/connector/connect")?;
        url.set_scheme(if origin.scheme() == "https" { "wss" } else { "ws" })
            .map_err(|_| anyhow::anyhow!("invalid socket scheme"))?;
        let mut request = url.as_str().into_client_request()?;
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {}", token.expose()).parse()?);
        let mut config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
        config.max_message_size = Some(MAX_FRAME);
        config.max_frame_size = Some(MAX_FRAME);
        let connected = tokio::select! {
            _=tokio::signal::ctrl_c()=>break,
            result=tokio::time::timeout(Duration::from_secs(15),tokio_tungstenite::connect_async_with_config(request,Some(config),false))=>result
        };
        match connected {
            Ok(Ok((socket, _))) => {
                failures = 0;
                let (mut sink, mut stream) = socket.split();
                sink.send(Message::Text(
                    serde_json::to_string(&Frame::Hello {
                        version: VERSION,
                        capabilities: caps.clone(),
                    })?
                    .into(),
                ))
                .await?;
                let (tx, mut rx) = mpsc::channel::<Frame>(8);
                let mut job: Option<(String, AbortTask)> = None;
                let mut pending: HashMap<String, Value> = HashMap::new();
                let mut last_seen = tokio::time::Instant::now();
                let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
                let mut stopped = false;
                loop {
                    tokio::select! {
                        _=tokio::signal::ctrl_c()=>{stopped=true;break;}
                        _=heartbeat.tick()=>{
                            if last_seen.elapsed()>Duration::from_secs(45){break;}
                            if sink.send(Message::Ping(Vec::new().into())).await.is_err(){break;}
                        }
                        Some(frame)=rx.recv()=>{
                            let Ok(text)=serde_json::to_string(&frame) else {break;};
                            if text.len()>MAX_FRAME || sink.send(Message::Text(text.into())).await.is_err(){break;}
                        }
                        line=mcp_rx.recv(), if stdio=>{
                            let Some(line)=line else {stopped=true;break;};
                            let action=match serde_json::from_slice(&line) {Ok(v)=>mcp::parse(v,&mut initialized),Err(_)=>mcp::Action::Reply(mcp::error(Value::Null,-32700,"Parse error"))};
                            match action {
                                mcp::Action::Ignore=>{},
                                mcp::Action::Reply(v)=>write_mcp(v).await?,
                                mcp::Action::Graph {id,query}=>{
                                    if !pending.is_empty(){write_mcp(mcp::error(id,-32000,"Graph read busy")).await?;continue;}
                                    let wire_id=random_token()?;
                                    pending.insert(wire_id.clone(),id);
                                    if tx.try_send(Frame::Graph {id:wire_id,query}).is_err(){break;}
                                }
                            }
                        }
                        incoming=stream.next()=>{
                            last_seen=tokio::time::Instant::now();
                            match incoming {
                                Some(Ok(Message::Ping(b)))=>{if sink.send(Message::Pong(b)).await.is_err(){break;}},
                                Some(Ok(Message::Pong(_)))=>{},
                                Some(Ok(Message::Text(text)))=>{
                                    let Ok(frame)=serde_json::from_str::<Frame>(&text) else {break;};
                                    match frame {
                                        Frame::Complete {id,connection,request,expires_at}=>{
                                            if job.as_ref().is_some_and(|(_,j)|!j.is_finished()) {
                                                let _=tx.try_send(Frame::Failed {id,code:Failure::Busy});continue;
                                            }
                                            let local=llm.clone();let allowed=allow.clone();let replies=tx.clone();let call_id=id.clone();
                                            job=Some((id,AbortTask(tokio::spawn(async move {
                                                let frame=match execute(&local,&allowed,&connection,request,expires_at).await {
                                                    Ok(output)=>Frame::Completed {id:call_id,output},Err(code)=>Frame::Failed {id:call_id,code}
                                                };
                                                let _=replies.try_send(frame);
                                            }))));
                                        }
                                        Frame::Cancel {id}=>{if job.as_ref().is_some_and(|(jid,_)|jid==&id) && let Some((_,j))=job.take(){j.abort();}},
                                        Frame::GraphResult {id,result}=>{if let Some(mcp_id)=pending.remove(&id){write_mcp(mcp::tool_result(mcp_id,result)).await?;}},
                                        Frame::Failed {id,..}=>{if let Some(mcp_id)=pending.remove(&id){write_mcp(mcp::error(mcp_id,-32000,"Graph read unavailable")).await?;}},
                                        _=>break
                                    }
                                }
                                _=>break
                            }
                        }
                    }
                }
                if let Some((_, j)) = job {
                    j.abort();
                }
                for (_, id) in pending {
                    write_mcp(mcp::error(id, -32000, "Disconnected; request not replayed")).await?;
                }
                let _ = sink.close().await;
                if stopped {
                    break;
                }
            }
            Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) => {
                // Authentication, rate limits and bot guards are final. Do not log the response body.
                if let Some(task) = stdin_task.as_ref() {
                    task.abort();
                }
                anyhow::bail!(
                    "connector HTTP handshake refused ({}); stopped",
                    response.status().as_u16()
                );
            }
            _ => {}
        }
        failures = failures.saturating_add(1);
        let delay = Duration::from_secs(1u64 << failures.min(5));
        eprintln!("Connector offline; reconnecting. In-flight calls are never replayed.");
        tokio::select! {_=tokio::signal::ctrl_c()=>break,_=tokio::time::sleep(delay)=>{}}
    }
    if let Some(task) = stdin_task {
        task.abort();
    }
    Ok(())
}
async fn write_mcp(v: Value) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut stdout = tokio::io::stdout();
    stdout.write_all(format!("{v}\n").as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}
