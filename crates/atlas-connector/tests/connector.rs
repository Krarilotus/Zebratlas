use atlas_connector::{
    broker::Broker,
    client,
    graph::{GraphReader, HttpGraph},
    mcp,
    protocol::{Capability, Frame, GraphQuery, VERSION},
    server::{Gateway, router},
    store::{Poll, Store},
};
use atlas_llm::request::{ProviderOutput, Usage};
use atlas_llm::{Cache, CacheMode, Call, CompletionRequest, Llm, Message, ProviderKind, Registry};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

fn cap() -> Capability {
    Capability {
        connection: "local".into(),
        kind: ProviderKind::OpenAiCompatible,
        default_model: Some("test-model".into()),
        models: vec![],
    }
}

#[cfg(target_os = "windows")]
#[test]
fn windows_credential_store_round_trip_no_plaintext_fallback() {
    let name = atlas_connector::random_token().unwrap();
    let entry = keyring::Entry::new("org.zebratlas.connector.tests", &name).unwrap();
    entry.set_password("fixture-only-not-a-real-credential").unwrap();
    assert_eq!(entry.get_password().unwrap(), "fixture-only-not-a-real-credential");
    entry.delete_credential().unwrap();
    assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
}
fn req() -> CompletionRequest {
    CompletionRequest::new(vec![Message::user("private test prompt")]).with_deadline(Duration::from_secs(2))
}
fn output() -> ProviderOutput {
    ProviderOutput {
        text: "test answer".into(),
        reported_model: Some("test-model-reported".into()),
        usage: Usage::default(),
        stop_reason: None,
        agent_version: Some("fixture adapter 1".into()),
        sent: std::collections::BTreeMap::from([("local.provider".into(), "openai-compatible".into())]),
    }
}
fn paired(store: &Store, user: &str) -> (String, String) {
    let p = store.pair("Test computer", atlas_connector::now()).unwrap();
    assert!(store.approve(&p.user_code, user, atlas_connector::now()).unwrap());
    let Poll::Ready { device_id } = store.poll(&p.device_code, atlas_connector::now()).unwrap() else {
        panic!()
    };
    (device_id, p.device_code)
}

#[test]
fn pairing_expiry_one_time_hash_only_and_durable_revocation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("devices.sqlite");
    let s = Store::open(&path).unwrap();
    let p = s.pair("Laptop", 100).unwrap();
    assert!(matches!(s.poll(&p.device_code, 100).unwrap(), Poll::Pending));
    assert!(s.poll(&p.device_code, 101).is_err());
    assert!(s.approve(&p.user_code, "alice", 101).unwrap());
    assert!(!s.approve(&p.user_code, "bob", 101).unwrap());
    let Poll::Ready { device_id } = s.poll(&p.device_code, 105).unwrap() else {
        panic!()
    };
    assert!(matches!(s.poll(&p.device_code, 110).unwrap(), Poll::Invalid));
    assert_eq!(s.authenticate(&p.device_code).unwrap().unwrap().user_id, "alice");
    assert!(!s.revoke(&device_id, "bob", 110).unwrap());
    drop(s);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let digest: String = conn
        .query_row("SELECT credential_hash FROM connector_devices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(digest, atlas_connector::hash(p.device_code.as_bytes()));
    assert_ne!(digest, p.device_code);
    let s = Store::open(&path).unwrap();
    assert!(s.active(&device_id, "alice").unwrap());
    assert!(s.revoke(&device_id, "alice", 111).unwrap());
    drop(s);
    assert!(
        Store::open(&path)
            .unwrap()
            .authenticate(&p.device_code)
            .unwrap()
            .is_none()
    );
    let s = Store::memory().unwrap();
    let p = s.pair("Expired", 100).unwrap();
    assert!(!s.approve(&p.user_code, "alice", 700).unwrap());
    assert!(matches!(s.poll(&p.device_code, 700).unwrap(), Poll::Invalid));
}

#[tokio::test]
async fn personal_failures_never_use_the_hosted_fallback_chain() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
    let hosted = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&hosted)
        .await;
    let store = Arc::new(Store::memory().unwrap());
    let (device, _) = paired(&store, "alice");
    let broker = Broker::new(store.clone());
    let (tx, _rx) = mpsc::channel(8);
    let session = broker.attach(&device, "alice".into(), vec![cap()], tx).unwrap();
    let mut registry = Registry::default();
    let connection = broker.connections("alice").remove(0);
    let name = connection.name().to_owned();
    registry.insert(connection);
    registry.insert(
        atlas_llm::registry::Connection::build(atlas_llm::ConnectionConfig {
            name: "fallback-fixture".into(),
            kind: Some(ProviderKind::OpenAiCompatible),
            base_url: Some(hosted.uri()),
            default_model: Some("fixture".into()),
            key_required: Some(false),
            ..Default::default()
        })
        .unwrap(),
    );
    let llm = Llm::new(registry, Cache::new("unused", CacheMode::Off)).with_fallbacks(["fallback-fixture"]);
    let scoped = llm.with_registry(llm.registry().clone());
    assert_eq!(scoped.default_chain(), vec!["fallback-fixture"]);
    broker.detach(&device, &session.generation);
    assert!(
        scoped
            .complete(&Call::new(&name).with_fallback(true), req())
            .await
            .is_err()
    );
    // Even a stale name missing from a new registry may never invoke a hosted provider.
    let empty = llm.with_registry(Registry::default());
    assert!(
        empty
            .complete(&Call::new(name).with_fallback(true), req())
            .await
            .is_err()
    );
    hosted.verify().await;
}

#[tokio::test]
async fn broker_isolation_provenance_and_personal_cache_bypass() {
    let store = Arc::new(Store::memory().unwrap());
    let (id, _) = paired(&store, "alice");
    let b = Broker::new(store);
    let (tx, mut rx) = mpsc::channel(8);
    let s = b.attach(&id, "alice".into(), vec![cap()], tx).unwrap();
    assert!(b.connections("bob").is_empty());
    let temp = tempfile::tempdir().unwrap();
    let mut registry = Registry::default();
    let conn = b.connections("alice").remove(0);
    let name = conn.name().to_owned();
    assert_eq!(conn.info(None).runs_on, "user-machine");
    registry.insert(conn);
    let llm = Llm::new(registry, Cache::new(temp.path(), CacheMode::ReadWrite));
    let run = tokio::spawn(async move { llm.complete(&Call::new(name), req()).await.unwrap() });
    let Frame::Complete { id: request, .. } = rx.recv().await.unwrap() else {
        panic!()
    };
    b.settle(&id, &s.generation, &request, Ok(output()));
    let result = run.await.unwrap();
    assert_eq!(result.response.reported_model.as_deref(), Some("test-model-reported"));
    assert_eq!(result.provenance.activity.parameters["retention"], "private-no-cache");
    assert_eq!(
        result.provenance.activity.parameters["sent.local.provider"],
        "openai-compatible"
    );
    assert_eq!(result.provenance.activity.agent.version, "fixture adapter 1");
    assert_eq!(
        result.provenance.response.sha256.as_deref(),
        Some(atlas_connector::hash(b"test answer").as_str())
    );
    assert!(result.provenance.prompt.file.is_empty());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn busy_timeout_cancel_disconnect_and_revoke_are_not_replayed() {
    let store = Arc::new(Store::memory().unwrap());
    let (id, _) = paired(&store, "alice");
    let b = Broker::new(store.clone());
    let (tx, mut rx) = mpsc::channel(8);
    let s = b.attach(&id, "alice".into(), vec![cap()], tx).unwrap();
    let p = b.connections("alice").remove(0).provider;
    let cp = p.clone();
    let job = tokio::spawn(async move { cp.complete(&req(), "test-model", None).await });
    let Frame::Complete { id: first, .. } = rx.recv().await.unwrap() else {
        panic!()
    };
    assert!(matches!(
        p.complete(&req(), "test-model", None).await,
        Err(atlas_llm::LlmError::RateLimited(_))
    ));
    job.abort();
    let _ = job.await;
    let Frame::Cancel { id: cancel } = rx.recv().await.unwrap() else {
        panic!()
    };
    assert_eq!(first, cancel);
    b.settle(&id, &s.generation, &first, Ok(output())); // Late reply cannot satisfy another request.
    let cp = p.clone();
    let job = tokio::spawn(async move {
        cp.complete(&req().with_deadline(Duration::from_millis(20)), "test-model", None)
            .await
    });
    assert!(matches!(rx.recv().await, Some(Frame::Complete { .. })));
    assert!(matches!(job.await.unwrap(), Err(atlas_llm::LlmError::Timeout(_))));
    assert!(matches!(rx.recv().await, Some(Frame::Cancel { .. })));
    let cp = p.clone();
    let job = tokio::spawn(async move { cp.complete(&req(), "test-model", None).await });
    assert!(matches!(rx.recv().await, Some(Frame::Complete { .. })));
    b.detach(&id, &s.generation);
    assert!(matches!(job.await.unwrap(), Err(atlas_llm::LlmError::Unavailable(_))));
    let (tx, mut rx2) = mpsc::channel(8);
    let s2 = b.attach(&id, "alice".into(), vec![cap()], tx).unwrap();
    b.detach(&id, &s.generation);
    assert!(!b.connections("alice").is_empty());
    assert!(store.revoke(&id, "alice", atlas_connector::now()).unwrap());
    b.revoke(&id);
    assert!(b.connections("alice").is_empty());
    assert!(p.complete(&req(), "test-model", None).await.is_err());
    assert!(rx2.try_recv().is_err());
    assert!(s2.closed.borrow().to_owned());
}

#[test]
fn native_origin_and_graph_url_boundaries() {
    for good in [
        "https://zebratlas.org",
        "http://127.0.0.1:1234",
        "http://localhost:1234",
        "http://[::1]:1234",
    ] {
        assert!(client::origin(good).is_ok(), "{good}");
    }
    for bad in [
        "http://zebratlas.org",
        "https://a:secret@zebratlas.org",
        "https://zebratlas.org/a",
        "https://zebratlas.org?x=1",
        "https://zebratlas.org#x",
        "http://localhost.evil:12",
    ] {
        assert!(client::origin(bad).is_err(), "{bad}");
    }
    let graph = HttpGraph::new("https://zebratlas.org").unwrap();
    let url = graph
        .url(&GraphQuery::Provenance {
            id: "../../account/connectors?x=#f".into(),
        })
        .unwrap();
    assert_eq!(url.host_str(), Some("zebratlas.org"));
    assert!(url.path().starts_with("/api/provenance/"));
    assert!(url.query().is_none());
    assert!(url.fragment().is_none());
    assert!(graph.url(&GraphQuery::Provenance { id: "..".into() }).is_err());
    assert!(
        graph
            .url(&GraphQuery::Search {
                query: "x".into(),
                limit: 0
            })
            .is_err()
    );
    assert!(serde_json::from_value::<GraphQuery>(json!({"op":"delete","id":"x"})).is_err());
    assert!(
        serde_json::from_value::<Frame>(json!({"type":"hello","version":1,"capabilities":[],"key":"secret"})).is_err()
    );
}

#[tokio::test]
async fn graph_preserves_evidence_response_hash_and_does_not_follow_redirects() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let server = MockServer::start().await;
    let evidence = json!({"records":[{"source":"fixture-only","locator":"row:7","sha256":"fixture-hash","excluded":true,"reason":"test"}]});
    Mock::given(method("GET"))
        .and(path("/api/provenance/edge:fixture"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&evidence))
        .mount(&server)
        .await;
    let graph = HttpGraph::new(&server.uri()).unwrap();
    let result = graph
        .read(GraphQuery::Provenance {
            id: "edge:fixture".into(),
        })
        .await
        .unwrap();
    assert_eq!(result["data"], evidence);
    assert_eq!(result["provenance"]["record_locator"], "$");
    assert_eq!(
        result["provenance"]["sha256"],
        atlas_connector::hash(&serde_json::to_vec(&evidence).unwrap())
    );
    Mock::given(path("/api/disease/redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/api/account/me"))
        .mount(&server)
        .await;
    assert!(graph.read(GraphQuery::Node { id: "redirect".into() }).await.is_err());
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn mcp_is_read_only_and_bounds_stdin() {
    let mut initialized = false;
    assert!(
        matches!(mcp::parse(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),&mut initialized),mcp::Action::Reply(v) if v["error"]["code"]==-32002)
    );
    assert!(
        matches!(mcp::parse(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),&mut initialized),mcp::Action::Reply(v) if v["result"]["protocolVersion"]=="2025-11-25")
    );
    assert!(matches!(
        mcp::parse(
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            &mut initialized
        ),
        mcp::Action::Ignore
    ));
    assert!(
        mcp::tools()["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["annotations"]["readOnlyHint"] == true)
    );
    for name in [
        "atlas_search",
        "atlas_condition",
        "atlas_provenance",
        "atlas_connections",
    ] {
        let args = if name == "atlas_search" {
            json!({"query":"fixture","limit":3})
        } else {
            json!({"id":"fixture"})
        };
        assert!(matches!(
            mcp::parse(
                json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":name,"arguments":args}}),
                &mut initialized
            ),
            mcp::Action::Graph { .. }
        ));
    }
    for args in [
        json!({"query":"x","limit":0}),
        json!({"query":"x","url":"https://evil"}),
        json!({"query":"x","limit":1.5}),
    ] {
        assert!(
            matches!(mcp::parse(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"atlas_search","arguments":args}}),&mut initialized),mcp::Action::Reply(v) if v["error"]["code"]==-32602)
        );
    }
    let mut too_long = tokio::io::BufReader::new(std::io::Cursor::new(vec![b'x'; 65537]));
    assert!(mcp::line(&mut too_long).await.is_err());
}

struct FixtureGraph;
#[async_trait::async_trait]
impl GraphReader for FixtureGraph {
    async fn read(&self, q: GraphQuery) -> anyhow::Result<Value> {
        Ok(json!({"fixture":true,"query":q,"provenance":{"record_locator":"fixture-only"}}))
    }
}

#[tokio::test]
async fn connection_check_only_contacts_selected_provider_and_respects_account_device_scope() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let metadata = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/selected/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"id":"configured-model"},{"id":"not-allowlisted"}]})),
        )
        .expect(1)
        .mount(&metadata)
        .await;
    Mock::given(method("GET"))
        .and(path("/untouched/models"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&metadata)
        .await;
    let accounts = Arc::new(atlas_accounts::Store::in_memory().unwrap());
    let alice_token = "a".repeat(43);
    let bob_token = "b".repeat(43);
    let alice = account(&accounts, "model-alice", &alice_token);
    account(&accounts, "model-bob", &bob_token);
    let store = Arc::new(Store::memory().unwrap());
    let (device, _) = paired(&store, &alice);
    let mut registry = Registry::from_toml(&format!(
        "replace_defaults=true\n[[connections]]\nname='selected'\nkind='openai-compatible'\nbase_url='{}/selected'\nkey_required=false\ndefault_model='configured-model'\nmodels=['configured-model','stale-model']\n[[connections]]\nname='untouched'\nkind='openai-compatible'\nbase_url='{}/untouched'\nkey_required=false\ndefault_model='other-model'\n", metadata.uri(),metadata.uri()), false).unwrap();
    #[derive(Debug)]
    struct NeverProbeServerCli;
    #[async_trait::async_trait]
    impl atlas_llm::Provider for NeverProbeServerCli {
        fn kind(&self) -> ProviderKind {
            ProviderKind::CodexCli
        }
        async fn probe(&self) -> atlas_llm::Availability {
            panic!("a hosted CLI must never be probed for a visitor")
        }
        async fn complete(
            &self,
            _: &CompletionRequest,
            _: &str,
            _: Option<&atlas_llm::ApiKey>,
        ) -> atlas_llm::Result<ProviderOutput> {
            panic!("metadata checks must never generate")
        }
    }
    registry.insert(atlas_llm::registry::Connection {
        config: atlas_llm::ConnectionConfig {
            name: "server-cli".into(),
            default_model: Some("server-subscription-model".into()),
            ..Default::default()
        },
        kind: ProviderKind::CodexCli,
        key_policy: atlas_llm::KeyPolicy::None,
        provider: Arc::new(NeverProbeServerCli),
    });
    let gateway = Gateway::new(store, accounts, false, Arc::new(FixtureGraph))
        .with_hosted(Arc::new(Llm::new(registry, Cache::new("unused", CacheMode::Off))));
    let (tx, mut rx) = mpsc::channel(8);
    gateway.broker().attach(&device, alice, vec![cap()], tx).unwrap();
    let api = router(gateway);
    let (_, listing) = http(
        api.clone(),
        "GET",
        "/api/account/connectors/models",
        Some(&alice_token),
        json!(null),
    )
    .await;
    assert!(
        listing["connections"]
            .as_array()
            .unwrap()
            .iter()
            .all(|connection| connection.get("availability").is_none_or(Value::is_null)),
        "ordinary listing performs no probes"
    );
    assert!(metadata.received_requests().await.unwrap().is_empty());
    let (_, server_cli) = http(
        api.clone(),
        "GET",
        "/api/account/connectors/models?connection=server-cli",
        Some(&alice_token),
        json!(null),
    )
    .await;
    assert_eq!(server_cli["connected"], false);
    assert_eq!(server_cli["models"], json!([]));
    assert_eq!(server_cli["availability"]["reason"], "native-connector-required");
    let (status, checked) = http(
        api.clone(),
        "GET",
        "/api/account/connectors/models?connection=selected",
        Some(&alice_token),
        json!(null),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(checked["connected"], true);
    assert_eq!(checked["models"], json!(["configured-model"]));
    assert!(
        checked["availability"].get("models").is_none(),
        "the full provider catalogue is not exposed"
    );
    let personal = format!("connector:{device}:local");
    let (_, checked) = http(
        api.clone(),
        "GET",
        &format!("/api/account/connectors/models?connection={personal}"),
        Some(&alice_token),
        json!(null),
    )
    .await;
    assert_eq!(checked["connected"], true);
    assert_eq!(checked["models"], json!(["test-model"]));
    assert!(
        rx.try_recv().is_err(),
        "liveness checks send no completion to the user's device"
    );
    assert_eq!(
        http(
            api.clone(),
            "GET",
            &format!("/api/account/connectors/models?connection={personal}"),
            Some(&bob_token),
            json!(null)
        )
        .await
        .0,
        404
    );
    assert_eq!(
        http(
            api,
            "GET",
            "/api/account/connectors/models?connection=selected",
            None,
            json!(null)
        )
        .await
        .0,
        401
    );
    assert!(
        metadata
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
}
fn account(accounts: &atlas_accounts::Store, name: &str, token: &str) -> String {
    let user = accounts
        .create_user(
            &format!("{name}@fixture.invalid"),
            None,
            None,
            "test-only",
            atlas_connector::now(),
        )
        .unwrap();
    accounts
        .create_session(
            &user.id,
            &atlas_accounts::auth::token_hash(token),
            "password",
            atlas_connector::now(),
            atlas_connector::now() + 600,
        )
        .unwrap();
    user.id
}
async fn http(router: axum::Router, method: &str, path: &str, token: Option<&str>, body: Value) -> (u16, Value) {
    use tower::ServiceExt;
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let response = router
        .oneshot(req.body(axum::body::Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn account_routes_require_auth_isolate_devices_and_enforce_csrf() {
    use tower::ServiceExt;
    let accounts = Arc::new(atlas_accounts::Store::in_memory().unwrap());
    let a = "a".repeat(43);
    let b = "b".repeat(43);
    let alice = account(&accounts, "alice", &a);
    account(&accounts, "bob", &b);
    let store = Arc::new(Store::memory().unwrap());
    let p = store.pair("fixture", atlas_connector::now()).unwrap();
    let gateway = Gateway::new(store.clone(), accounts, false, Arc::new(FixtureGraph));
    let r = router(gateway);
    assert_eq!(
        http(
            r.clone(),
            "POST",
            "/api/account/connectors/approve",
            None,
            json!({"user_code":p.user_code})
        )
        .await
        .0,
        401
    );
    assert_eq!(
        http(
            r.clone(),
            "POST",
            "/api/account/connectors/approve",
            Some(&a),
            json!({"user_code":p.user_code})
        )
        .await
        .0,
        200
    );
    let Poll::Ready { device_id } = store.poll(&p.device_code, atlas_connector::now()).unwrap() else {
        panic!()
    };
    assert!(store.active(&device_id, &alice).unwrap());
    assert_eq!(
        http(r.clone(), "GET", "/api/account/connectors", Some(&b), json!(null))
            .await
            .1,
        json!([])
    );
    assert_eq!(
        http(
            r.clone(),
            "DELETE",
            &format!("/api/account/connectors/{device_id}"),
            Some(&b),
            json!(null)
        )
        .await
        .0,
        404
    );
    let response = r
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("DELETE")
                .uri(format!("/api/account/connectors/{device_id}"))
                .header("cookie", format!("atlas_session={a}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert_eq!(
        http(
            r,
            "DELETE",
            &format!("/api/account/connectors/{device_id}"),
            Some(&a),
            json!(null)
        )
        .await
        .0,
        200
    );
    assert!(store.authenticate(&p.device_code).unwrap().is_none());
}

#[tokio::test]
async fn real_outbound_websocket_completion_graph_and_revocation() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message as Ws, client::IntoClientRequest};
    let accounts = Arc::new(atlas_accounts::Store::in_memory().unwrap());
    let token = "a".repeat(43);
    let alice = account(&accounts, "alice", &token);
    let store = Arc::new(Store::memory().unwrap());
    let (id, credential) = paired(&store, &alice);
    let gateway = Gateway::new(store.clone(), accounts, false, Arc::new(FixtureGraph));
    let r = router(gateway.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, r)
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await
            .unwrap()
    });
    let mut handshake = format!("ws://{addr}/api/connector/connect")
        .into_client_request()
        .unwrap();
    handshake
        .headers_mut()
        .insert("Authorization", format!("Bearer {credential}").parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(handshake).await.unwrap();
    ws.send(Ws::Text(
        serde_json::to_string(&Frame::Hello {
            version: VERSION,
            capabilities: vec![cap()],
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    // Graph request acts as handshake barrier and uses the same socket.
    ws.send(Ws::Text(
        serde_json::to_string(&Frame::Graph {
            id: "graph-1".into(),
            query: GraphQuery::Search {
                query: "fixture".into(),
                limit: 1,
            },
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    loop {
        match ws.next().await.unwrap().unwrap() {
            Ws::Ping(p) => ws.send(Ws::Pong(p)).await.unwrap(),
            Ws::Text(t) => {
                let Frame::GraphResult { id, result } = serde_json::from_str(&t).unwrap() else {
                    panic!()
                };
                assert_eq!(id, "graph-1");
                assert_eq!(result["fixture"], true);
                break;
            }
            _ => {}
        }
    }
    let name = format!("connector:{id}:local");
    let origin = format!("http://{addr}");
    let request = req();
    let response = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{origin}/api/connector/complete"))
            .bearer_auth(token)
            .json(&json!({"connection":name,"request":request,"inputs":["fixture:edge"]}))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()
    });
    loop {
        match ws.next().await.unwrap().unwrap() {
            Ws::Ping(p) => ws.send(Ws::Pong(p)).await.unwrap(),
            Ws::Text(t) => {
                let Frame::Complete { id, .. } = serde_json::from_str(&t).unwrap() else {
                    panic!()
                };
                ws.send(Ws::Text(
                    serde_json::to_string(&Frame::Completed { id, output: output() })
                        .unwrap()
                        .into(),
                ))
                .await
                .unwrap();
                break;
            }
            _ => {}
        }
    }
    let done = response.await.unwrap();
    assert_eq!(done["response"]["text"], "test answer");
    assert_eq!(done["provenance"]["inputs"], json!(["fixture:edge"]));
    assert!(store.revoke(&id, &alice, atlas_connector::now()).unwrap());
    gateway.broker().revoke(&id);
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(msg) = ws.next().await {
            if matches!(msg, Ok(Ws::Close(_))) {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(gateway.broker().connections(&alice).is_empty());
    let _ = stop_tx.send(());
    server.await.unwrap();
}

#[tokio::test]
async fn local_allowlist_and_actual_http_adapter() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/v1/chat/completions")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "id":"fixture-only","object":"chat.completion","model":"fixture-reported","choices":[{"index":0,"message":{"role":"assistant","content":"fixture answer"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}
    }))).mount(&server).await;
    let mut registry = Registry::default();
    registry.insert(
        atlas_llm::registry::Connection::build(atlas_llm::ConnectionConfig {
            name: "local".into(),
            kind: Some(ProviderKind::OpenAiCompatible),
            base_url: Some(format!("{}/v1", server.uri())),
            default_model: Some("fixture-requested".into()),
            key_required: Some(false),
            ..Default::default()
        })
        .unwrap(),
    );
    let llm = Llm::new(registry, Cache::new("unused", CacheMode::Off));
    let allow = vec!["local".into()];
    assert_eq!(client::capabilities(&llm, &allow).unwrap().len(), 1);
    assert!(
        client::execute(&llm, &[], "local", req(), atlas_connector::now() + 30)
            .await
            .is_err()
    );
    assert!(
        client::execute(&llm, &allow, "local", req(), atlas_connector::now() - 1)
            .await
            .is_err()
    );
    let out = client::execute(&llm, &allow, "local", req(), atlas_connector::now() + 30)
        .await
        .unwrap();
    assert_eq!(out.text, "fixture answer");
    assert_eq!(out.reported_model.as_deref(), Some("fixture-reported"));
    assert_eq!(out.sent["local.provider"], "openai-compatible");
    assert!(!out.sent.contains_key("base_url"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
