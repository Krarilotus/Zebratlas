//! Journey text tasks against a mock OpenAI-compatible server: validator, one regeneration,
//! template fallback, reconcile enum. No network.

use atlas_llm::tasks::{Origin, SentenceKind};
use atlas_llm::{
    Cache, CacheMode, Call, Candidate, DraftKind, Fact, Llm, MessageCard, Registry, UserRole, draft_message,
    plain_summary, reconcile, translate_snippet, why_sentence,
};
use serde_json::{Value, json};
use wiremock::matchers::path_regex;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn reply(content: &Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c", "object": "chat.completion", "created": 0, "model": "mock-model",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content.to_string()}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    }))
}

/// Serve `answers` in order (the last one repeats).
async fn server(answers: &[Value]) -> MockServer {
    let s = MockServer::start().await;
    for (i, a) in answers.iter().enumerate() {
        let m = Mock::given(path_regex(".*")).respond_with(reply(a));
        if i + 1 < answers.len() {
            m.up_to_n_times(1).mount(&s).await
        } else {
            m.mount(&s).await
        }
    }
    s
}

fn llm_for(uri: &str) -> Llm {
    let toml = format!(
        "replace_defaults = true\n[[connections]]\nname = \"mock\"\nkind = \"openai-compatible\"\n\
         base_url = \"{uri}/v1\"\ndefault_model = \"m\"\nkey_required = false\n"
    );
    Llm::new(
        Registry::from_toml(&toml, false).unwrap(),
        Cache::new(std::env::temp_dir(), CacheMode::Off),
    )
}

fn facts() -> Vec<Fact> {
    vec![
        Fact::new(
            "E1",
            "STXBP1 encephalopathy (ORPHA:178469) is a rare brain condition caused by changes in the STXBP1 gene.",
        ),
        Fact::new(
            "E2",
            "Most children have seizures that start in the first year of life.",
        ),
        Fact::new(
            "E3",
            "The STXBP1 Foundation (https://www.stxbp1disorders.org) supports more than 600 families.",
        ),
    ]
}

#[tokio::test]
async fn slow_provider_falls_back_within_the_task_budget_without_replay() {
    use std::time::{Duration, Instant};
    let s = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(reply(&json!({"sentences": []})).set_delay(Duration::from_secs(2)))
        .mount(&s)
        .await;
    let started = Instant::now();
    let out = plain_summary(
        &llm_for(&s.uri()),
        &Call::new("mock").with_deadline(Duration::from_millis(500)),
        "Synthetic condition",
        &facts(),
        "en",
    )
    .await;
    assert_eq!(out.origin, Origin::Template);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(
        out.validation
            .fallback_reason
            .as_deref()
            .is_some_and(|s| s.contains("deadline"))
    );
    assert!(
        out.sentences
            .iter()
            .all(|s| s.cites.iter().all(|c| facts().iter().any(|f| &f.key == c)))
    );
    assert!(
        s.received_requests().await.unwrap().len() <= 1,
        "timeouts are never replayed"
    );
}

#[tokio::test]
async fn provider_fallback_chain_shares_one_task_deadline() {
    use std::time::Duration;
    let first = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(ResponseTemplate::new(404).set_delay(Duration::from_millis(400)))
        .mount(&first)
        .await;
    let backup = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(
            reply(&json!({"sentences": [{"text": "Source fact.", "cites": ["F"]}]}))
                .set_delay(Duration::from_millis(800)),
        )
        .mount(&backup)
        .await;
    let config = format!(
        r#"replace_defaults = true
[[connections]]
name = "first"
kind = "openai-compatible"
base_url = "{}/v1"
default_model = "m"
key_required = false
[[connections]]
name = "backup"
kind = "openai-compatible"
base_url = "{}/v1"
default_model = "m"
key_required = false
"#,
        first.uri(),
        backup.uri()
    );
    let llm = Llm::new(
        Registry::from_toml(&config, false).unwrap(),
        Cache::new(std::env::temp_dir(), CacheMode::Off),
    )
    .with_fallbacks(["backup"]);
    let out = plain_summary(
        &llm,
        &Call::new("first")
            .with_fallback(true)
            .with_deadline(Duration::from_secs(1)),
        "Synthetic condition",
        &[Fact::new("F", "Source fact.")],
        "en",
    )
    .await;
    assert_eq!(
        out.origin,
        Origin::Template,
        "the backup cannot start a fresh task budget"
    );
    assert!(
        out.calls.is_empty(),
        "no unfinished provider is recorded as completed provenance"
    );
    assert_eq!(backup.received_requests().await.unwrap().len(), 1);
    assert!(
        out.validation.attempts.is_empty(),
        "a timeout never becomes a completed validation attempt"
    );
}

#[tokio::test]
async fn validation_retry_and_provider_fallback_share_the_remaining_budget() {
    use std::time::Duration;
    let first = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(
            reply(&json!({"sentences": [{"text": "Invented figure 90000000.", "cites": ["F"]}]}))
                .set_delay(Duration::from_millis(300)),
        )
        .up_to_n_times(1)
        .mount(&first)
        .await;
    Mock::given(path_regex(".*"))
        .respond_with(ResponseTemplate::new(404).set_delay(Duration::from_millis(300)))
        .mount(&first)
        .await;
    let backup = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(
            reply(&json!({"sentences": [{"text": "Source fact.", "cites": ["F"]}]}))
                .set_delay(Duration::from_millis(500)),
        )
        .mount(&backup)
        .await;
    let config = format!(
        r#"replace_defaults = true
[[connections]]
name = "first"
kind = "openai-compatible"
base_url = "{}/v1"
default_model = "m"
key_required = false
[[connections]]
name = "backup"
kind = "openai-compatible"
base_url = "{}/v1"
default_model = "m"
key_required = false
"#,
        first.uri(),
        backup.uri()
    );
    let llm = Llm::new(
        Registry::from_toml(&config, false).unwrap(),
        Cache::new(std::env::temp_dir(), CacheMode::Off),
    )
    .with_fallbacks(["backup"]);
    let out = plain_summary(
        &llm,
        &Call::new("first")
            .with_fallback(true)
            .with_deadline(Duration::from_secs(1)),
        "Synthetic condition",
        &[Fact::new("F", "Source fact.")],
        "en",
    )
    .await;
    assert_eq!(
        out.origin,
        Origin::Template,
        "retry fallback cannot consume a fresh full budget"
    );
    assert_eq!(out.calls.len(), 1, "retain only the initial completed activity");
    assert_eq!(out.validation.attempts.len(), 1);
    assert_eq!(backup.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn validation_retry_shares_the_original_deadline_and_retains_completed_provenance() {
    use std::time::Duration;
    let s = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(
            reply(&json!({"sentences": [
                {"text": "Invented figure 90000000.", "cites": ["E1"]}
            ]}))
            .set_delay(Duration::from_secs(1)),
        )
        .up_to_n_times(1)
        .mount(&s)
        .await;
    Mock::given(path_regex(".*"))
        .respond_with(
            reply(&json!({"sentences": [
                {"text": "Invented figure 90000000.", "cites": ["E1"]}
            ]}))
            .set_delay(Duration::from_millis(1500)),
        )
        .mount(&s)
        .await;
    let out = plain_summary(
        &llm_for(&s.uri()),
        &Call::new("mock").with_deadline(Duration::from_secs(2)),
        "Synthetic condition",
        &facts(),
        "en",
    )
    .await;
    assert_eq!(out.origin, Origin::Template);
    assert_eq!(
        out.calls.len(),
        1,
        "the first completed call's PROV-O record survives the retry timeout"
    );
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
    assert!(
        out.validation
            .fallback_reason
            .as_deref()
            .is_some_and(|s| s.contains("deadline"))
    );
}

#[tokio::test]
async fn summary_regenerates_once_after_invented_number() {
    let bad = json!({"sentences": [
        {"text": "STXBP1 encephalopathy is a rare brain condition.", "cites": ["E1"]},
        {"text": "It affects 1 in 90,000 children.", "cites": ["E2"]}
    ]});
    let good = json!({"sentences": [
        {"text": "STXBP1 encephalopathy is a rare brain condition caused by a change in one gene.", "cites": ["E1"]},
        {"text": "Most children have seizures in their first year.", "cites": ["E2"]}
    ]});
    let s = server(&[bad, good]).await;
    let out = plain_summary(
        &llm_for(&s.uri()),
        &Call::new("mock"),
        "STXBP1 encephalopathy",
        &facts(),
        "en",
    )
    .await;
    assert_eq!(out.origin, Origin::Llm);
    assert_eq!(out.validation.attempts.len(), 2);
    assert!(
        out.validation.attempts[0].iter().any(|i| i.contains("90,000")),
        "{:?}",
        out.validation
    );
    assert!(out.validation.passed);
    assert_eq!(
        out.validation.assurance.as_deref(),
        Some("citation and token checks passed")
    );
    assert_eq!(out.calls.len(), 2);
    // The regeneration request carried the validator's issues.
    let reqs = s.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[1].body).unwrap();
    assert!(body["messages"].to_string().contains("broke these rules"));
    assert!(
        body["response_format"].to_string().contains("\"E3\""),
        "cites enum sent"
    );
}

#[tokio::test]
async fn why_falls_back_to_template_after_two_failures() {
    let uncited = json!({"sentences": [{"text": "They can cure your child.", "cites": []}]});
    let s = server(&[uncited]).await;
    let out = why_sentence(
        &llm_for(&s.uri()),
        &Call::new("mock"),
        "STXBP1 Foundation",
        &facts(),
        "de",
    )
    .await;
    assert_eq!(out.origin, Origin::Template);
    assert_eq!(out.lang, "en");
    assert_eq!(out.requested_lang, "de");
    assert_eq!(out.sentences[0].cites, vec!["E1"]);
    assert!(
        out.validation
            .fallback_reason
            .as_deref()
            .unwrap()
            .starts_with("validator")
    );
    assert_eq!(out.calls.len(), 2);
}

#[tokio::test]
async fn provider_down_uses_template_without_retry() {
    let llm = llm_for("http://127.0.0.1:9");
    let out = plain_summary(&llm, &Call::new("mock"), "STXBP1 encephalopathy", &facts(), "en").await;
    assert_eq!(out.origin, Origin::Template);
    assert_eq!(out.sentences.len(), 2);
    assert!(
        out.validation
            .fallback_reason
            .as_deref()
            .unwrap()
            .starts_with("provider")
    );
    assert!(out.calls.is_empty());
}

fn candidates() -> Vec<Candidate> {
    vec![
        Candidate {
            id: "ORPHA:178469".into(),
            label: "STXBP1 encephalopathy with epilepsy".into(),
            synonyms: vec!["STXBP1 encephalopathy".into()],
            kind: Some("disease".into()),
        },
        Candidate {
            id: "ORPHA:33069".into(),
            label: "Dravet syndrome".into(),
            synonyms: vec![],
            kind: None,
        },
    ]
}

#[tokio::test]
async fn reconcile_exact_name_needs_no_call_and_llm_picks_from_enum() {
    let s = server(&[
        json!({"match": "ORPHA:178469", "alternatives": [], "confidence": "high",
                            "mention": "STXBP1-Mutation"}),
    ])
    .await;
    let llm = llm_for(&s.uri());
    let exact = reconcile(&llm, &Call::new("mock"), "stxbp1 encephalopathy", "en", &candidates()).await;
    assert_eq!(
        (exact.choice.as_deref(), exact.origin),
        (Some("ORPHA:178469"), Origin::Lexical)
    );
    assert!(s.received_requests().await.unwrap().is_empty());

    let r = reconcile(
        &llm,
        &Call::new("mock"),
        "Meine Tochter hat eine STXBP1-Mutation",
        "de",
        &candidates(),
    )
    .await;
    assert_eq!(r.origin, Origin::Llm);
    assert_eq!(r.choice.as_deref(), Some("ORPHA:178469"));
    assert_eq!(r.mention.as_deref(), Some("STXBP1-Mutation"));
    let body: Value = serde_json::from_slice(&s.received_requests().await.unwrap()[0].body).unwrap();
    assert!(
        body["response_format"].to_string().contains("ORPHA:33069"),
        "candidate enum sent"
    );
}

#[tokio::test]
async fn reconcile_rejects_invented_mention_then_falls_back() {
    let s =
        server(&[json!({"match": "ORPHA:33069", "alternatives": [], "confidence": "high", "mention": "Dravet"})]).await;
    let r = reconcile(&llm_for(&s.uri()), &Call::new("mock"), "stxbp", "en", &candidates()).await;
    assert_eq!(r.origin, Origin::Template);
    assert_eq!(r.choice.as_deref(), Some("ORPHA:178469"), "{r:?}");
}

#[tokio::test]
async fn draft_message_validates_names_and_kinds() {
    let card = MessageCard {
        recipient: "STXBP1 Foundation".into(),
        recipient_kind: "patient group".into(),
        channel: Some("https://www.stxbp1disorders.org/contact".into()),
        condition: "STXBP1 encephalopathy".into(),
        facts: facts(),
    };
    let good = json!({"subject": "Frage zu STXBP1 encephalopathy", "sentences": [
        {"kind": "fact", "text": "STXBP1 Foundation unterstützt mehr als 600 Familien.", "cites": ["E3"]},
        {"kind": "ask", "text": "Wie können wir Kontakt zu anderen Familien aufnehmen?", "cites": []}
    ]});
    let s = server(&[good]).await;
    let d = draft_message(
        &llm_for(&s.uri()),
        &Call::new("mock"),
        &card,
        UserRole::Parent,
        DraftKind::Outreach,
        "de",
        None,
    )
    .await;
    assert_eq!(d.body.origin, Origin::Llm, "{:?}", d.body.validation);
    assert!(d.text().starts_with("STXBP1 Foundation"));
    assert_eq!(
        d.body.sentences.iter().filter(|s| s.kind == SentenceKind::Fact).count(),
        1
    );

    // A translated name ("STXBP1-Stiftung") never reaches the user: template instead.
    let renamed = json!({"subject": "Frage", "sentences": [
        {"kind": "fact", "text": "STXBP1-Stiftung unterstützt 600 Familien.", "cites": ["E3"]},
        {"kind": "ask", "text": "Können wir reden?", "cites": []}
    ]});
    let s = server(&[renamed]).await;
    let d = draft_message(
        &llm_for(&s.uri()),
        &Call::new("mock"),
        &card,
        UserRole::Parent,
        DraftKind::Outreach,
        "de",
        None,
    )
    .await;
    assert_eq!(d.body.origin, Origin::Template);
    assert_eq!(d.body.lang, "de");
    assert!(!d.text().contains("STXBP1-Stiftung"));
    assert_eq!(d.channel, card.channel);
}

#[tokio::test]
async fn translation_keeps_original_and_tokens() {
    let original = "Die Stiftung unterstützt 600 Familien mit STXBP1 (https://example.org).";
    let s = server(&[json!({"source_lang": "de",
        "translation": "The foundation supports 600 families with STXBP1 (https://example.org)."})])
    .await;
    let t = translate_snippet(&llm_for(&s.uri()), &Call::new("mock"), original, "en").await;
    assert_eq!(t.origin, Origin::Llm);
    assert_eq!(t.original, original);
    assert_eq!(t.source_lang.as_deref(), Some("de"));

    let s = server(&[json!({"source_lang": "es", "translation": "The foundation supports many families."})]).await;
    let t = translate_snippet(&llm_for(&s.uri()), &Call::new("mock"), original, "en").await;
    assert_eq!(t.text, None, "invalid translation is dropped; original stays");
    assert_eq!(t.original, original);
}

#[tokio::test]
async fn adversarial_claims_regenerate_then_use_source_template() {
    let fact = Fact::new(
        "F",
        "Trial NCT01234567 is not recruiting. The observed rate is 0.5 percent.",
    );
    for text in [
        "Trial NCT01234567 is recruiting.",
        "The observed rate is 5 percent.",
        "A cure is available.",
    ] {
        let s = server(&[json!({"sentences": [{"text": text, "cites": ["F"]}]})]).await;
        let out = why_sentence(
            &llm_for(&s.uri()),
            &Call::new("mock"),
            "Trial",
            std::slice::from_ref(&fact),
            "en",
        )
        .await;
        assert_eq!(out.origin, Origin::Template, "{text}");
        assert_eq!(out.text(), fact.text);
        assert_eq!(out.validation.attempts.len(), 2);
        assert!(!out.validation.passed);
    }
}

#[tokio::test]
async fn unchecked_translation_polarity_keeps_the_source() {
    for (source_lang, original, lang, translation) in [
        (
            "en",
            "Trial NCT01234567 is not recruiting.",
            "de",
            "Studie NCT01234567 rekrutiert.",
        ),
        (
            "en",
            "Trial NCT01234567 is not recruiting.",
            "fr",
            "L’essai NCT01234567 ne recrute pas.",
        ),
        (
            "es",
            "El ensayo NCT01234567 no recluta.",
            "en",
            "Trial NCT01234567 is recruiting.",
        ),
    ] {
        let s = server(&[json!({"source_lang": source_lang, "translation": translation})]).await;
        let out = translate_snippet(&llm_for(&s.uri()), &Call::new("mock"), original, lang).await;
        assert_eq!(out.origin, Origin::Template);
        assert_eq!(out.original, original);
        assert!(out.text.is_none());
    }
}
