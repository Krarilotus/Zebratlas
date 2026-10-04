//! Real account + production router + outbound fake device; no external model calls.
use super::*;
use atlas_connector::protocol::{Capability, Frame, VERSION};
use atlas_llm::{
    CompletionRequest, Message, ProviderKind,
    request::{ProviderOutput, Usage},
};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, RequestBuilder};
use std::{collections::BTreeMap, time::Duration};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message as Ws, client::IntoClientRequest},
};
#[path = "connector_test_support.rs"]
mod support;
use support::*;

#[tokio::test]
async fn production_pair_choose_answer_and_revoke() {
    let temp = tempfile::tempdir().unwrap();
    let cfg = AccountsConfig {
        database: atlas_accounts::Database::File(temp.path().join("accounts.sqlite")),
        secure_cookies: false,
        ..AccountsConfig::for_tests()
    };
    let devices_path = temp.path().join("devices.sqlite");
    let devices = Arc::new(Store::open(&devices_path).unwrap());
    let mut state = crate::test_support::state();
    state.data = Arc::new(temp.path().into());
    // Journey cards follow record activity indices; the search-only fixture omits that activity.
    let mut graph = state.graph.data().clone();
    graph.provenance.add_activity(atlas_core::provenance::Activity {
        id: "activity:synthetic-connector-graph".into(),
        used: vec![atlas_core::provenance::EntityIdx(0)],
        ..Default::default()
    });
    state.graph = Arc::new(atlas_core::Graph::new(graph));
    state.withhold = crate::privacy::WithholdState::new(temp.path().into(), state.graph.clone());
    let withhold = state.withhold.clone();
    // Keep the production gpt-oss preset. No server key or external completion is used.
    let hosted = Arc::new(Llm::new(
        Registry::presets(false).unwrap(),
        Cache::new(temp.path().join("llm"), CacheMode::ReadWrite),
    ));
    let default = hosted.default_connection(false);
    let free_before = hosted.registry().get("hosted-free").unwrap().config.clone();
    assert_eq!(free_before.default_model.as_deref(), Some("openai/gpt-oss-120b"));
    state.llm.llm = Some(hosted.clone());
    let fact = crate::llm::condition_facts(&state.atlas, state.atlas.disease_idx("MONDO:9999999").unwrap())[0].clone();
    let accounts = atlas_accounts::try_router(cfg.clone()).unwrap();
    let app = crate::routes::assembled(
        state,
        accounts,
        Router::new(),
        Some(Config {
            accounts: cfg,
            devices: devices.clone(),
            conversations: None,
            public_origin: "https://example.invalid".into(),
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    // Abort on a failed assertion too: this task owns the listener and no external process.
    struct Stop(tokio::task::AbortHandle);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let stop = Stop(server.abort_handle());
    let _query_stop = Stop(canonical_query_fixture().await);
    let client = Client::builder().timeout(Duration::from_secs(8)).build().unwrap();
    let (alice, alice_id) = signup(&client, &origin, "alice").await;
    let (bob, _) = signup(&client, &origin, "bob").await;
    let pair: Value = client
        .post(format!("{origin}/api/connector/pair"))
        .json(&json!({"label":"Fixture computer"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let approval = format!("{origin}/api/account/connectors/approve");
    assert_eq!(
        client
            .post(&approval)
            .json(&json!({"user_code":pair["user_code"]}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .post(&approval)
            .header("cookie", &alice)
            .json(&json!({"user_code":pair["user_code"]}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        cookie_request(client.post(&approval), &alice)
            .json(&json!({"user_code":pair["user_code"]}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let ready: Value = client
        .post(format!("{origin}/api/connector/poll"))
        .json(&json!({"device_code":pair["device_code"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ready["status"], "approved");
    let device = ready["device_id"].as_str().unwrap();
    let name = format!("connector:{device}:fixture-local");
    let credential = pair["device_code"].as_str().unwrap();
    let mut handshake = format!("ws://{addr}/api/connector/connect")
        .into_client_request()
        .unwrap();
    handshake
        .headers_mut()
        .insert("authorization", format!("Bearer {credential}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(handshake.clone()).await.unwrap();
    send(
        &mut socket,
        Frame::Hello {
            version: VERSION,
            capabilities: vec![Capability {
                connection: "fixture-local".into(),
                kind: ProviderKind::OpenAiCompatible,
                default_model: Some("fixture-a".into()),
                models: vec!["fixture-a".into(), "fixture-b".into()],
            }],
        },
    )
    .await;
    // This graph request is also the Hello processing barrier.
    send(
        &mut socket,
        Frame::Graph {
            id: "read-1".into(),
            query: GraphQuery::Search {
                query: "STXBP1".into(),
                limit: 1,
            },
        },
    )
    .await;
    let Frame::GraphResult { result, .. } = frame(&mut socket).await else {
        panic!("expected graph result")
    };
    assert!(result["data"].to_string().contains("MONDO:9999999"));
    assert_eq!(
        result["provenance"]["source_url"],
        "https://example.invalid/api/search?q=STXBP1&limit=1"
    );
    assert_eq!(result["provenance"]["sha256"].as_str().unwrap().len(), 64);
    let http_bytes = client
        .get(format!("{origin}/api/search?q=STXBP1&limit=1"))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    // A second search has a new checked.at timestamp, but the same evidence/results.
    let direct: Value = serde_json::from_slice(&http_bytes).unwrap();
    assert_eq!(result["data"]["items"], direct["items"]);
    assert_eq!(result["data"]["checked"]["sources"], direct["checked"]["sources"]);
    assert_eq!(
        result["provenance"]["sha256"],
        atlas_connector::hash(&serde_json::to_vec(&result["data"]).unwrap())
    );

    let models = format!("{origin}/api/account/connectors/models");
    // Runtime erasure must reach MCP reads even though WebSocket frames skip HTTP filtering.
    for suppressed in [false, true] {
        if suppressed {
            use atlas_contrib::privacy::{PrivacyActions, RequestType, SuppressOrder, TraceParams};
            crate::privacy::ServerActions(withhold.clone())
                .suppress(&SuppressOrder {
                    reference: "connector-fixture-removal",
                    kind: RequestType::Remove,
                    trace: &TraceParams {
                        node: Some("NCTTEST0".into()),
                        ..Default::default()
                    },
                    nodes: &["NCTTEST0".into()],
                    reviewer: "fixture-only",
                })
                .unwrap();
        }
        send(
            &mut socket,
            Frame::Graph {
                id: "suppression-read".into(),
                query: GraphQuery::Search {
                    query: "STXBP1".into(),
                    limit: 20,
                },
            },
        )
        .await;
        let Frame::GraphResult { result, .. } = frame(&mut socket).await else {
            panic!("expected graph result")
        };
        assert_eq!(result["data"].to_string().contains("NCTTEST0"), !suppressed);
        assert!(result["data"].to_string().contains("NCTTEST1"));
        assert_eq!(
            result["provenance"]["sha256"],
            atlas_connector::hash(&serde_json::to_vec(&result["data"]).unwrap())
        );
    }
    let personal: Value = client
        .get(&models)
        .header("cookie", &alice)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(personal["default"], default);
    assert!(personal["selected"].is_null());
    assert!(
        personal["connections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == name && c["kind"] == "connector")
    );
    let other: Value = client
        .get(&models)
        .header("cookie", &bob)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!other.to_string().contains(device));
    // Guessing another account's connection never dispatches a prompt to its device.
    for cookie in [None, Some(&bob)] {
        let mut request = client
            .post(format!("{origin}/api/ask"))
            .json(&json!({"question":"fixture","connection":name,"save":false}));
        if let Some(cookie) = cookie {
            request = cookie_request(request, cookie);
        }
        let expected = if cookie.is_some() {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::UNAUTHORIZED
        };
        assert_eq!(request.send().await.unwrap().status(), expected);
    }
    for (field, value, expected) in [
        ("key", "fixture-key-must-stay-local", StatusCode::BAD_REQUEST),
        ("model", "unadvertised", StatusCode::BAD_REQUEST),
    ] {
        let mut body = json!({"question":"fixture","connection":name,"save":false});
        body[field] = json!(value);
        assert_eq!(
            cookie_request(client.post(format!("{origin}/api/ask")), &alice)
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            expected
        );
    }
    let model = format!("{origin}/api/account/connectors/model");
    let selected = json!({"connection":name,"model":"fixture-b"});
    assert_eq!(
        client
            .put(&model)
            .header("cookie", &alice)
            .json(&selected)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        cookie_request(client.put(&model), &bob)
            .json(&selected)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        cookie_request(client.put(&model), &alice)
            .json(&json!({"connection":name,"model":"unadvertised"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        cookie_request(client.put(&model), &alice)
            .json(&json!({"connection":name,"key":"must-not-store"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        cookie_request(client.put(&model), &alice)
            .json(&selected)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        Store::open(&devices_path)
            .unwrap()
            .choice(&alice_id)
            .unwrap()
            .unwrap()
            .model
            .as_deref(),
        Some("fixture-b")
    );
    // Existing model pickers are scoped; the immutable hosted preset is unchanged.
    for path in ["/api/llm/connections", "/api/ask/connections"] {
        let own: Value = client
            .get(format!("{origin}{path}"))
            .header("cookie", &alice)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(own.to_string().contains(&name));
        let anon: Value = client
            .get(format!("{origin}{path}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!anon.to_string().contains(&name));
    }
    assert_eq!(hosted.registry().get("hosted-free").unwrap().config, free_before);
    assert_eq!(hosted.default_connection(false), default);
    assert!(hosted.registry().get(&name).is_err());

    // Natural-language graph search must use the same account-selected device,
    // without injecting ask-only fields into ExploreRequest's closed schema.
    let pending = cookie_request(client.post(format!("{origin}/api/explore")), &alice)
        .json(&json!({"query":"STXBP1 conditions","limit":5}))
        .send();
    let canonical_plan = json!({"version":1,"pattern":"traverse","focus":["HGNC:11444"],
        "hops":[{"relation":atlas_journeys::GENE_RELATION,"direction":"incoming"}],
        "filters":{"country":null,"recruiting":null,"kind":null},"output":null,"limit":5,"reasoning":false});
    let reply = canonical_plan.clone();
    let (response, ()) = tokio::join!(
        pending,
        answer(&mut socket, reply, Some("atlas_query_plan"), "fixture-b")
    );
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let explored: Value = response.json().await.unwrap();
    assert_eq!(explored["interpretation"]["mode"], "agent", "{explored}");
    assert_eq!(explored["query_execution"]["answer"]["plan"], canonical_plan);
    assert_eq!(explored["execution"]["engine"], "nrese");
    assert_eq!(explored["results"][0]["id"], "MONDO:9999999");
    assert_eq!(explored["graph"]["nodes"].as_array().unwrap().len(), 2);

    let pending = cookie_request(client.post(format!("{origin}/api/explore")), &alice)
        .json(&json!({"query":"STXBP1 researchers unavailable"}))
        .send();
    let fail = async {
        let Frame::Complete { id, request, .. } = frame(&mut socket).await else {
            panic!("selected search must reach the paired device")
        };
        assert_eq!(request.model.as_deref(), Some("fixture-b"));
        send(
            &mut socket,
            Frame::Failed {
                id,
                code: atlas_connector::protocol::Failure::Unavailable,
            },
        )
        .await;
    };
    let (response, ()) = tokio::join!(pending, fail);
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let failed: Value = response.json().await.unwrap();
    assert_eq!(failed["retrieval"]["scope"], "name_candidates_only");
    assert!(failed["graph"]["nodes"].as_array().unwrap().is_empty());
    assert!(failed["results"].as_array().unwrap().is_empty());
    assert!(failed["execution"]["sparql"].is_null(), "selected failure cannot masquerade as executed results");
    assert_eq!(failed["interpretation"]["routing"]["attempted"].as_array().unwrap().len(), 1, "explicit device choice cannot move to another provider");

    // The real summary route uses the saved model without a connection header.
    let pending = cookie_request(
        client.get(format!("{origin}/api/condition/MONDO:9999999/summary")),
        &alice,
    )
    .send();
    // This synthetic atlas has one condition fact; repeat it to satisfy the two-sentence schema.
    let reply = json!({"sentences":[{"text":fact.text,"cites":[fact.key]},{"text":fact.text,"cites":[fact.key]}]});
    let (response, ()) = tokio::join!(pending, answer(&mut socket, reply, Some("plain_summary"), "fixture-b"));
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let summary: Value = response.json().await.unwrap();
    assert_eq!(summary["origin"], "llm", "{summary}");
    assert!(!summary["activities"].as_array().unwrap().is_empty());

    // Ask gets the preference in its JSON body, while its own explicit choice wins.
    let pending = cookie_request(client.post(format!("{origin}/api/ask")), &alice)
        .json(&json!({"question":"fixture greeting","plan_only":true,"save":false}))
        .send();
    let reply = json!({"action":"answer","calls":[],"sentences":[{"text":"Choose a condition to search.","cites":[],"kind":"ask"}]});
    let (response, ()) = tokio::join!(pending, answer(&mut socket, reply, Some("atlas_plan"), "fixture-b"));
    let asked: Value = response.unwrap().json().await.unwrap();
    assert_eq!(asked["calls"][0]["connection"], name, "{asked}");
    assert_eq!(asked["calls"][0]["model"], "fixture-b");
    assert_eq!(
        asked["provenance"][0]["activity"]["parameters"]["retention"],
        "private-no-cache"
    );

    let pending = cookie_request(client.post(format!("{origin}/api/ask")), &alice)
        .json(
            &json!({"question":"fixture greeting","connection":name,"model":"fixture-a","plan_only":true,"save":false}),
        )
        .send();
    let reply = json!({"action":"answer","calls":[],"sentences":[{"text":"Choose a condition to search.","cites":[],"kind":"ask"}]});
    let (response, ()) = tokio::join!(pending, answer(&mut socket, reply, Some("atlas_plan"), "fixture-a"));
    let explicit: Value = response.unwrap().json().await.unwrap();
    assert_eq!(explicit["calls"][0]["model"], "fixture-a");
    assert_eq!(
        devices.choice(&alice_id).unwrap().unwrap().model.as_deref(),
        Some("fixture-b")
    );

    let processor: Value = cookie_request(client.get(format!("{origin}/api/intake/processor")), &alice)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(processor["connection"], name);
    assert!(processor["processor"].as_str().unwrap().contains("fixture-b"));
    let pending = cookie_request(client.post(format!("{origin}/api/intake")), &alice)
        .json(&json!({"text":"Find studies for STXBP1"}))
        .send();
    let reply = json!({"references":[],"intent":"find_study","terms":[{"kind":"gene","text":"STXBP1","english":"","hgvs":"","gene":"","negated":false}]});
    let (response, ()) = tokio::join!(
        pending,
        answer(&mut socket, reply, Some("atlas_understand"), "fixture-b")
    );
    let intake: Value = response.unwrap().json().await.unwrap();
    assert_eq!(intake["model"]["origin"], "llm", "{intake}");
    assert!(
        intake["terms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["text"] == "STXBP1")
    );
    assert!(
        !temp.path().join("llm").exists(),
        "personal replies must not enter shared cache"
    );

    // The submitted one-box route uses the saved account model and the D56 schema.
    let pending = cookie_request(client.post(format!("{origin}/api/search")), &alice)
        .json(&json!({"text":"Patient: Alice Example\nDOB: 12.03.2010\nFind studies for STXBP1","lang":"de"}))
        .send();
    let reply = json!({"references":[],"intent":"find_study","terms":[{"kind":"gene","text":"STXBP1","english":"","hgvs":"","gene":"","negated":false}]});
    let (response, ()) = tokio::join!(
        pending,
        answer(&mut socket, reply, Some("atlas_understand"), "fixture-b")
    );
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let searched: Value = response.json().await.unwrap();
    assert_eq!(searched["understood"]["model"]["connection"], name);
    assert_eq!(searched["understood"]["intent"], "find_study");
    assert_eq!(searched["results"][0]["node"]["kind"], "study");
    assert!(!searched["understood"].to_string().contains("Alice"));

    let revoke = format!("{origin}/api/account/connectors/{device}");
    assert_eq!(
        cookie_request(client.delete(&revoke), &bob)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .delete(&revoke)
            .header("cookie", &alice)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        cookie_request(client.delete(&revoke), &alice)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(message) = socket.next().await {
            match message {
                Ok(Ws::Ping(p)) => {
                    let _ = socket.send(Ws::Pong(p)).await;
                }
                Ok(Ws::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(tokio_tungstenite::connect_async(handshake).await.is_err());
    assert!(
        Store::open(&devices_path)
            .unwrap()
            .authenticate(credential)
            .unwrap()
            .is_none()
    );
    // Retaining an offline/revoked choice fails closed, with no hosted completion or replay.
    let offline: Value = cookie_request(
        client.get(format!("{origin}/api/search?q=Find%20studies%20for%20STXBP1")),
        &alice,
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(offline["understood"]["model"]["status"], "unavailable");
    assert!(!offline["results"].as_array().unwrap().is_empty());
    for path in ["/api/condition/MONDO:9999999/summary", "/api/intake/processor"] {
        assert_eq!(
            cookie_request(client.get(format!("{origin}{path}")), &alice)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
    assert_eq!(
        cookie_request(client.post(format!("{origin}/api/ask")), &alice)
            .json(&json!({"question":"fixture"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        cookie_request(client.post(format!("{origin}/api/explore")), &alice)
            .json(&json!({"query":"STXBP1 conditions"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let edited_plan = json!({"focus":["HGNC:11444"],"intent":"conditions","filters":{}});
    let rerun = cookie_request(client.post(format!("{origin}/api/explore")), &alice)
        .json(&json!({"query":"STXBP1 conditions","plan":edited_plan}))
        .send()
        .await
        .unwrap();
    assert_eq!(rerun.status(), StatusCode::OK);
    let rerun: Value = rerun.json().await.unwrap();
    assert_eq!(rerun["plan"]["focus"], edited_plan["focus"]);
    assert_eq!(rerun["plan"]["intent"], "conditions");
    // Bypassing the model never bypasses canonical entity/plan validation.
    let invalid = cookie_request(client.post(format!("{origin}/api/explore")), &alice)
        .json(&json!({"plan":{"focus":["HGNC:invented"],"intent":"all","filters":{}}}))
        .send()
        .await
        .unwrap();
    assert!(invalid.status().is_client_error());
    let request = CompletionRequest::new(vec![Message::user("fixture")]).with_deadline(Duration::from_secs(1));
    assert_eq!(
        cookie_request(client.post(format!("{origin}/api/connector/complete")), &alice)
            .json(&json!({"connection":name,"request":request}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(
        client
            .get(format!("{origin}/api/condition/MONDO:9999999/summary"))
            .header("x-llm-connection", &name)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for path in [
        "/api/condition/MONDO:9999999/connections",
        "/api/resolve?q=MONDO:9999999&llm=0",
    ] {
        assert_eq!(
            cookie_request(client.get(format!("{origin}{path}")), &alice)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        cookie_request(client.post(format!("{origin}/api/ask")), &alice)
            .json(&json!({"question":"STXBP1","no_llm":true,"save":false}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        cookie_request(client.delete(&model), &alice)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert!(devices.choice(&alice_id).unwrap().is_none());
    let reset: Value = client
        .get(&models)
        .header("cookie", &alice)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reset["default"], default);
    assert!(reset["selected"].is_null());
    assert!(!reset.to_string().contains(&name));
    drop(stop);
    let _ = server.await;
}
