//! The conversation loop against a scripted provider (no network): plan → run → answer,
//! validator rejections, regeneration, template fallback, rules fallback, edited chips, extra
//! call rounds, the HTTP routes (JSON + SSE), key handling and saving for signed-in users.

mod common;

use std::sync::Arc;

use atlas_ask::{AskRequest, AskState, Asker, ChipOrigin, ChipStatus, IntentKind};
use atlas_llm::tasks::Origin;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{call, failing_llm, fixture, mock_llm, quota_llm};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn plan(calls: Vec<Value>) -> Value {
    json!({"action": "call", "calls": calls, "sentences": []})
}

fn answer(sentences: Value) -> Value {
    json!({"action": "answer", "calls": [], "sentences": sentences})
}

fn req(q: &str) -> AskRequest {
    AskRequest {
        question: q.into(),
        lang: "en".into(),
        connection: Some("mock".into()),
        ..AskRequest::default()
    }
}

#[test]
fn client_cannot_choose_budget_visitor_identity() {
    let request: AskRequest = serde_json::from_value(json!({
        "question": "help", "lang": "en", "visitor": "rotate-this-to-evade-quota"
    }))
    .unwrap();
    assert!(request.visitor.is_none());
}

fn group_plan() -> Value {
    plan(vec![call(
        "connections",
        &[("condition", "STXBP1"), ("kind", "patient_group")],
    )])
}

/// E1 = the foundation's node fact (with its link), E2 = serves STXBP1, E3 = gene edge.
fn good_answer() -> Value {
    answer(json!([
        {"text": "The STXBP1 Foundation is a patient group for families affected by STXBP1.", "cites": ["E1", "E2"], "kind": "fact"},
        {"text": "Its website is https://www.stxbp1disorders.org.", "cites": ["E1"], "kind": "fact"},
        {"text": "This is not medical advice.", "cites": [], "kind": "caveat"}
    ]))
}

fn asker(llm: Arc<atlas_llm::Llm>) -> Asker {
    let (atlas, graph) = fixture();
    Asker::new(atlas, graph, llm)
}

#[tokio::test]
async fn plans_runs_and_answers_with_checked_citations() {
    let (llm, mock) = mock_llm(&[group_plan(), good_answer()]);
    let out = asker(llm).ask(&req("Is there a patient group for STXBP1?"), None).await;
    assert_eq!(out.origin, Origin::Llm, "{:?}", out.validation);
    assert!(out.validation.passed);
    assert_eq!(out.chips.len(), 1);
    let chip = &out.chips[0];
    assert_eq!(
        (chip.intent, chip.origin, chip.status),
        (IntentKind::Connections, ChipOrigin::Model, Some(ChipStatus::Found))
    );
    assert_eq!(chip.resolved["condition"].node.id, "MONDO:0012812");
    // Every cited key maps to an edge or node id.
    let e2 = out.facts.iter().find(|f| f.key == "E2").unwrap();
    assert!(
        e2.edge && e2.id == "org:stxbp1-foundation|serves_gene|HGNC:11444",
        "{e2:?}"
    );
    assert_eq!(out.facts[0].url.as_deref(), Some("https://www.stxbp1disorders.org"));
    assert_eq!(
        out.calls.iter().map(|c| c.purpose).collect::<Vec<_>>(),
        ["plan", "answer"]
    );
    assert_eq!(out.provenance.len(), 2);
    // D48: the client can name every producing model without reading the PROV record.
    assert_eq!(out.models, vec![out.calls[0].model_label.clone()]);
    assert!(out.calls.iter().all(|c| !c.model_label.fallback.is_empty()));
    for (call, provenance) in out.calls.iter().zip(&out.provenance) {
        assert_eq!(provenance.activity.parameters["model.label"], call.model_label.fallback);
    }
    // The answer prompt carried the facts and the plan prompt the intent catalogue.
    let reqs = mock.requests.lock().unwrap();
    assert!(reqs[0].messages[0].content.contains("shared_people("));
    assert!(
        reqs[1].messages[1]
            .content
            .contains("[E2] STXBP1 Foundation (org:stxbp1-foundation) serves people with changes in")
    );
}

#[tokio::test]
async fn invented_tokens_are_rejected_then_regenerated_once() {
    let bad = answer(json!([
        {"text": "The STXBP1 Foundation helps 600 families.", "cites": ["E1"], "kind": "fact"},
        {"text": "Join trial NCT01234567.", "cites": ["E2"], "kind": "fact"}
    ]));
    let (llm, mock) = mock_llm(&[group_plan(), bad, good_answer()]);
    let out = asker(llm).ask(&req("patient group STXBP1"), None).await;
    assert_eq!(out.origin, Origin::Llm);
    assert_eq!(out.validation.attempts.len(), 2);
    let first = out.validation.attempts[0].join("\n");
    assert!(first.contains("600") && first.contains("NCT01234567"), "{first}");
    assert_eq!(
        out.calls.iter().map(|c| c.purpose).collect::<Vec<_>>(),
        ["plan", "answer", "retry"]
    );
    let retry = &mock.requests.lock().unwrap()[2];
    assert!(retry.messages.last().unwrap().content.contains("broke these rules"));
}

#[tokio::test]
async fn two_invalid_answers_fall_back_to_the_template() {
    let uncited =
        answer(json!([{"text": "STXBP1 causes epilepsy in 1 in 30,000 children.", "cites": [], "kind": "fact"}]));
    let unknown_key = answer(json!([{"text": "The STXBP1 Foundation helps.", "cites": ["E9"], "kind": "fact"}]));
    let (llm, _) = mock_llm(&[group_plan(), uncited, unknown_key]);
    let out = asker(llm).ask(&req("patient group STXBP1"), None).await;
    assert_eq!(out.origin, Origin::Template);
    assert!(!out.validation.passed);
    let all = out.validation.attempts.concat().join("\n");
    // The validator reports numbers exactly as written since the M1 hardening (30,000 stays 30,000).
    assert!(all.contains("without citing") && all.contains("30,000"), "{all}");
    assert!(
        out.validation
            .fallback_reason
            .as_deref()
            .unwrap()
            .starts_with("validator:")
    );
    assert_eq!(out.lang, "en");
    // Template sentences are the facts themselves, each citing its key.
    assert!(out.answer.iter().all(|s| s.cites.len() == 1));
    assert!(out.answer[0].text.contains("STXBP1 Foundation"));
}

#[tokio::test]
async fn schema_violations_count_as_failed_attempts() {
    let (llm, _) = mock_llm(&[group_plan(), json!({"action": "dance"}), json!("not even an object")]);
    let out = asker(llm).ask(&req("patient group STXBP1"), None).await;
    assert_eq!(out.origin, Origin::Template);
    assert_eq!(out.validation.attempts.len(), 2);
    assert!(out.validation.attempts[0][0].contains("not a valid step"));
}

#[tokio::test]
async fn provider_failure_uses_rules_and_template() {
    let (llm, _) = failing_llm();
    let out = asker(llm)
        .ask(&req("Which patient group helps with STXBP1?"), None)
        .await;
    assert_eq!(out.origin, Origin::Template);
    assert_eq!(
        out.provider_error,
        Some(atlas_ask::converse::ProviderError::Unavailable)
    );
    assert!(out.models.is_empty(), "no model produced the template answer");
    assert!(out.notes[0].contains("plan was not usable"), "{:?}", out.notes);
    assert!(
        out.validation
            .fallback_reason
            .as_deref()
            .unwrap()
            .starts_with("provider:")
    );
    assert_eq!(out.chips[0].origin, ChipOrigin::Rules);
    assert_eq!(out.chips[0].intent, IntentKind::Connections);
    assert_eq!(out.chips[0].slot("kind"), Some("patient_group"));
    assert!(!out.facts.is_empty());
}

#[tokio::test]
async fn edited_chips_skip_planning_and_plan_only_skips_running() {
    let (llm, mock) = mock_llm(&[good_answer()]);
    let a = asker(llm);
    let mut r = req("patient group");
    r.chips = Some(vec![
        atlas_ask::Chip::new(
            IntentKind::Connections,
            &[("condition", "STXBP1 encephalopathy"), ("kind", "patient_group")],
        )
        .with_origin(ChipOrigin::User),
    ]);
    let out = a.ask(&r, None).await;
    assert_eq!(out.origin, Origin::Llm, "{:?}", out.validation);
    assert_eq!(out.chips[0].origin, ChipOrigin::User);
    assert_eq!(mock.requests.lock().unwrap().len(), 1, "no planning call");

    let (llm, _) = mock_llm(&[group_plan()]);
    let mut p = req("Is there a group for STXBP1?");
    p.plan_only = true;
    let out = asker(llm).ask(&p, None).await;
    assert!(out.planned_only && out.facts.is_empty() && out.answer.is_empty());
    assert_eq!(out.chips[0].status, None);
    assert_eq!(out.chips[0].id, "c1");
}

#[tokio::test]
async fn model_may_call_more_intents_once() {
    let more = json!({"action": "call", "calls": [call("condition_summary_facts", &[("condition", "STXBP1")])], "sentences": []});
    let final_answer = answer(json!([
        {"text": "The STXBP1 Foundation supports families affected by STXBP1.", "cites": ["E1", "E2"], "kind": "fact"},
        {"text": "Seizures, also called fits, are common in this condition.", "cites": ["E7"], "kind": "fact"}
    ]));
    let (llm, mock) = mock_llm(&[group_plan(), more, final_answer]);
    let out = asker(llm).ask(&req("group and symptoms for STXBP1"), None).await;
    assert_eq!(out.chips.len(), 2);
    assert_eq!(out.chips[1].intent, IntentKind::ConditionSummaryFacts);
    assert_eq!(out.chips[1].id, "c2");
    let reqs = mock.requests.lock().unwrap();
    assert!(reqs[2].messages.last().unwrap().content.contains("More atlas results"));
    // After the extra round the schema only allows an answer.
    let schema = &reqs[2].schema.as_ref().unwrap().schema;
    assert_eq!(schema["properties"]["action"]["enum"], json!(["answer"]));
    let seizure = out
        .facts
        .iter()
        .find(|f| f.id.ends_with("has_phenotype|HP:0001250"))
        .expect("seizure edge");
    assert_eq!(
        seizure.msg.as_ref().unwrap()["params"]["term"]["params"]["plain"],
        "Fits"
    );
    assert!(seizure.text.contains("Fits"), "{}", seizure.text);
    if out.origin != Origin::Llm {
        panic!("{:?}\n{:#?}", out.validation, out.facts);
    }
}

#[tokio::test]
async fn out_of_scope_questions_get_a_direct_uncited_answer() {
    let direct = answer(json!([
        {"text": "I can't give medical advice about doses.", "cites": [], "kind": "caveat"},
        {"text": "Do you want to see patient groups or studies instead?", "cites": [], "kind": "ask"}
    ]));
    let (llm, _) = mock_llm(&[direct]);
    let out = asker(llm)
        .ask(&req("How much medicine should my child take?"), None)
        .await;
    assert_eq!(out.origin, Origin::Llm);
    assert!(out.chips.is_empty() && out.facts.is_empty());
    // A "fact" without facts behind it is not accepted.
    let (llm, _) = mock_llm(&[answer(json!([{"text": "Give 5 mg.", "cites": [], "kind": "fact"}]))]);
    let out = asker(llm).ask(&req("dose?"), None).await;
    assert_eq!(out.origin, Origin::Template);
}

#[tokio::test]
async fn key_goes_to_the_provider_but_never_into_the_response() {
    let (llm, mock) = mock_llm(&[group_plan(), good_answer()]);
    let mut r = req("group STXBP1");
    r.key = Some("sk-secret-123".into());
    assert!(!format!("{r:?}").contains("sk-secret"));
    let out = asker(llm).ask(&r, None).await;
    assert_eq!(mock.keys.lock().unwrap()[0].as_deref(), Some("ApiKey(***)"));
    assert!(!serde_json::to_string(&out).unwrap().contains("sk-secret"));
}

fn state(llm: Arc<atlas_llm::Llm>) -> AskState {
    let (atlas, graph) = fixture();
    AskState::new(atlas, graph, llm)
}

async fn body(res: axum::response::Response) -> String {
    String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

fn post(uri: &str, v: &Value) -> Request<Body> {
    Request::post(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(v.to_string()))
        .unwrap()
}

#[tokio::test]
async fn routes_json_sse_and_intents() {
    let (llm, _) = mock_llm(&[group_plan(), good_answer()]);
    let app = atlas_ask::router(state(llm));
    let res = app
        .clone()
        .oneshot(Request::get("/api/ask/intents").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let v: Value = serde_json::from_str(&body(res).await).unwrap();
    assert_eq!(v["intents"].as_array().unwrap().len(), 10);
    assert_eq!(v["intents"][2]["name"], "connections");
    assert_eq!(
        v["intents"][2]["parameters"]["properties"]["kind"]["enum"][1],
        "patient_group"
    );

    let res = app
        .clone()
        .oneshot(post("/api/ask", &json!({"question": "  "})))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let q = json!({"question": "Is there a patient group for STXBP1?", "connection": "mock"});
    let res = app.clone().oneshot(post("/api/ask", &q)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: Value = serde_json::from_str(&body(res).await).unwrap();
    assert_eq!(v["origin"], "llm");
    assert_eq!(v["chips"][0]["intent"], "connections");
    assert_eq!(v["saved"], false);

    let (llm, _) = mock_llm(&[group_plan(), good_answer()]);
    let app = atlas_ask::router(state(llm));
    let res = app.oneshot(post("/api/ask?stream=1", &q)).await.unwrap();
    assert!(
        res.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let text = body(res).await;
    let order: Vec<usize> = ["event: status", "event: chips", "event: results", "event: answer"]
        .iter()
        .map(|e| text.find(e).unwrap_or_else(|| panic!("{e} missing in {text}")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
}

#[tokio::test]
async fn signed_in_conversations_are_saved_through_accounts() {
    use atlas_accounts::{Store, auth::token_hash};
    let store = Store::in_memory().unwrap();
    let user = store
        .create_user("devon@example.org", None, Some("en"), "phc", 1)
        .unwrap();
    let token = "t".repeat(43);
    store
        .create_session(&user.id, &token_hash(&token), "password", 1, i64::MAX)
        .unwrap();
    let accounts = Arc::new(atlas_ask::AccountsStore::from_store(store));
    let (llm, _) = mock_llm(&[group_plan(), good_answer()]);
    let app = atlas_ask::router(state(llm).with_store(accounts.clone()));
    let mut r = post(
        "/api/ask",
        &json!({"question": "Is there a patient group for STXBP1?", "connection": "mock"}),
    );
    r.headers_mut().insert("x-atlas-csrf", "1".parse().unwrap());
    r.headers_mut()
        .insert(header::COOKIE, format!("__Host-atlas_session={token}").parse().unwrap());
    let v: Value = serde_json::from_str(&body(app.clone().oneshot(r).await.unwrap()).await).unwrap();
    assert_eq!(v["saved"], true, "{v}");
    let id = v["conversation_id"].as_str().unwrap();
    let conv = accounts.store().conversation(&user.id, id).unwrap().unwrap();
    assert_eq!(conv.messages.len(), 2);
    assert_eq!(conv.messages[1].citations[0]["id"], "org:stxbp1-foundation");
    assert_eq!(
        conv.messages[1].meta.as_ref().unwrap()["chips"][0]["intent"],
        "connections"
    );
    // Anonymous: answered, not saved.
    let v: Value = serde_json::from_str(
        &body(
            app.oneshot(post(
                "/api/ask",
                &json!({"question": "group STXBP1", "connection": "mock"}),
            ))
            .await
            .unwrap(),
        )
        .await,
    )
    .unwrap();
    assert_eq!(v["saved"], false);
}

#[tokio::test]
async fn free_quota_reached_is_typed_and_still_answers_from_the_atlas() {
    let (llm, _) = quota_llm();
    let out = asker(llm).ask(&req("Is there a patient group for STXBP1?"), None).await;
    assert_eq!(out.provider_error, Some(atlas_ask::converse::ProviderError::Quota));
    assert_eq!(out.origin, Origin::Template);
    assert!(!out.facts.is_empty());
    let v = serde_json::to_value(&out).unwrap();
    assert_eq!(v["provider_error"], "quota");
}
