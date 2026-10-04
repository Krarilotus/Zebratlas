//! Loopback fake-device helpers; no external provider calls.
use super::*;

pub(super) type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub(super) fn cookie_request(builder: RequestBuilder, cookie: &str) -> RequestBuilder {
    builder.header("cookie", cookie).header("x-atlas-csrf", "1")
}
pub(super) async fn signup(client: &Client, origin: &str, name: &str) -> (String, String) {
    let response = client
        .post(format!("{origin}/api/account/signup"))
        .json(&json!({"email":format!("{name}@example.invalid"),"password":"fixture-password-only-123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let value: Value = response.json().await.unwrap();
    (cookie, value["user"]["id"].as_str().unwrap().into())
}
pub(super) async fn frame(socket: &mut Socket) -> Frame {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Ws::Ping(p) => socket.send(Ws::Pong(p)).await.unwrap(),
                Ws::Text(t) => return serde_json::from_str(&t).unwrap(),
                other => panic!("unexpected socket message: {other:?}"),
            }
        }
    })
    .await
    .unwrap()
}
pub(super) async fn send(socket: &mut Socket, frame: Frame) {
    socket
        .send(Ws::Text(serde_json::to_string(&frame).unwrap().into()))
        .await
        .unwrap();
}
pub(super) async fn answer(socket: &mut Socket, value: Value, schema_name: Option<&str>, model: &str) {
    let Frame::Complete {
        id,
        connection,
        request,
        ..
    } = frame(socket).await
    else {
        panic!("expected completion")
    };
    assert_eq!(connection, "fixture-local");
    assert_eq!(request.model.as_deref(), Some(model));
    assert_eq!(request.schema.as_ref().map(|s| s.name.as_str()), schema_name);
    if schema_name == Some("atlas_understand") {
        let sent = serde_json::to_string(&request).unwrap();
        assert!(!sent.contains("Alice Example"));
        assert!(!sent.contains("12.03.2010"));
    }
    if let Some(schema) = &request.schema {
        atlas_llm::check_json(&schema.schema, &value.to_string())
            .expect("fake reply must satisfy the real task schema");
    }
    // Only the explicitly permitted adapter/model metadata is returned by this fake device.
    send(
        socket,
        Frame::Completed {
            id,
            output: ProviderOutput {
                text: value.to_string(),
                reported_model: Some(format!("{model}-reported")),
                usage: Usage::default(),
                stop_reason: None,
                agent_version: Some("fake-device-v1".into()),
                sent: BTreeMap::from([("local.provider".into(), "openai-compatible".into())]),
            },
        },
    )
    .await;
}

/// Loopback SELECT fixture for the real canonical guard/compiler/HTTP execution.
/// It is a synthetic release, never a claim about the operator's live nrese data.
pub(super) async fn canonical_query_fixture() -> tokio::task::AbortHandle {
    use atlas_ask::query::{
        QueryEngine, SchemaCard,
        schema::{Lineage, Predicate},
    };
    let ra = atlas_ask::query::RA;
    let predicates = [
        format!("{ra}{}", atlas_journeys::GENE_RELATION),
        "http://www.w3.org/2000/01/rdf-schema#label".into(),
    ]
    .into_iter()
    .map(|p| (p, Predicate::default()))
    .collect();
    let schema = SchemaCard {
        schema_version: 1,
        activity: json!({"fixture":true}),
        graph: Lineage {
            source_url: "urn:atlas:synthetic-connector-release".into(),
            retrieved_at: "".into(),
            version: "synthetic-fixture-v1".into(),
            sha256: "00".repeat(32),
            record_locator: "test-only".into(),
        },
        statements: 2,
        classes: Default::default(),
        predicates,
        known_absent: Default::default(),
        prefixes: Default::default(),
        semantic_units: Default::default(),
        provenance_pattern: "test-only".into(),
    };
    let fixture = Router::new().route(
        "/sparql",
        axum::routing::post(|body: String| async move {
            assert!(
                body.contains("HGNC%3A11444"),
                "query must bind the trusted fixture gene"
            );
            assert!(
                body.contains("has_associated_gene"),
                "query must use the schema-owned relation"
            );
            Json(json!({"head":{"vars":["result","label"]},"results":{"bindings":[{
                "result":{"type":"uri","value":"https://w3id.org/rare-disease-atlas/id/MONDO%3A9999999"},
                "label":{"type":"literal","value":"Synthetic STXBP1 condition"}
            }]}}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/sparql", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });
    let engine = QueryEngine::new(schema, endpoint, None).unwrap();
    assert!(crate::explore_query::TEST_ENGINE.set(Arc::new(engine)).is_ok());
    task.abort_handle()
}
