//! The conversation loop: model → intent calls → executors → model → cited answer → validator →
//! one regeneration → deterministic template.
//!
//! Tool calling is a JSON protocol on top of atlas-llm's structured output, so it works the same on
//! every connection the user brings (HTTP APIs, local servers, subscription CLIs), and every call
//! goes through atlas-llm's cache and PROV-O records. Each model turn returns
//! `{"action": "call" | "answer", "calls": [...], "sentences": [...]}`; calls are [`Chip`]s.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use atlas_core::{Atlas, Graph};
use atlas_llm::LlmError;
use atlas_llm::tasks::validate::{Rules, check};
use atlas_llm::tasks::{Cited, Origin, SentenceKind, Validation, language_name};
use atlas_llm::{ApiKey, Call, Completion, CompletionRequest, JsonSchema, Llm, LlmCall, Message, check_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::exec::{self, Ctx};
use crate::facts::{self, AskFact, FactBook};
use crate::intent::{Chip, ChipOrigin, INTENTS, IntentKind, all_slot_names};
use crate::rules;

/// Chips run per answer, at most.
pub const MAX_CHIPS: usize = 8;
/// Extra call rounds the model may take after the first results.
pub const MAX_EXTRA_ROUNDS: usize = 1;
const MAX_TOKENS: u32 = 4000;
const DEADLINE: Duration = Duration::from_secs(120);
const MAX_NON_FACT: usize = 3;

/// One earlier turn, sent by the client for follow-up questions (plain text only).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    /// `user` or `assistant`.
    pub role: String,
    pub text: String,
}

/// `POST /api/ask`. `Debug` redacts the key.
#[derive(Clone, Default, Deserialize)]
pub struct AskRequest {
    /// Trusted caller identity, filled by the HTTP boundary; never accepted from JSON.
    #[serde(skip)]
    pub visitor: Option<String>,
    pub question: String,
    #[serde(default = "default_lang")]
    pub lang: String,
    /// Connection name from `/api/llm/connections` (default: `ATLAS_LLM_DEFAULT`, demo `kisski`).
    #[serde(default)]
    pub connection: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// BYO key for this request only: never stored, logged, cached or echoed.
    #[serde(default)]
    pub key: Option<String>,
    /// Edited chips: run exactly these, skip the model's parse.
    #[serde(default)]
    pub chips: Option<Vec<Chip>>,
    /// Only parse the question into chips; don't run them.
    #[serde(default)]
    pub plan_only: bool,
    /// Skip the model: rule-based chips and template answer (offline / no provider).
    #[serde(default)]
    pub no_llm: bool,
    #[serde(default)]
    pub history: Vec<Turn>,
    /// Append to this saved conversation (signed-in users).
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Save when signed in (default true).
    #[serde(default)]
    pub save: Option<bool>,
}

impl std::fmt::Debug for AskRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AskRequest")
            .field("question", &self.question)
            .field("lang", &self.lang)
            .field("connection", &self.connection)
            .field("model", &self.model)
            .field("key", &self.key.as_ref().map(|_| "***"))
            .field("chips", &self.chips)
            .field("plan_only", &self.plan_only)
            .field("no_llm", &self.no_llm)
            .finish_non_exhaustive()
    }
}

fn default_lang() -> String {
    "en".into()
}

/// Summary of one model call (the full PROV-O record is in `provenance`).
#[derive(Clone, Debug, Serialize)]
pub struct CallInfo {
    /// `plan`, `answer`, `retry`.
    pub purpose: &'static str,
    pub activity: String,
    pub connection: String,
    pub model: String,
    /// The model as the UI names it (D48): catalog message `{key, params, fallback}`.
    pub model_label: atlas_llm::ModelLabel,
    pub reported_model: Option<String>,
    pub cached: bool,
    pub latency_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AskResponse {
    pub question: String,
    /// Language of `answer` (template answers are in English).
    pub lang: String,
    pub requested_lang: String,
    /// The parsed question: editable intent + slots, with what each resolved to and found.
    pub chips: Vec<Chip>,
    pub answer: Vec<Cited>,
    /// Evidence bundle: every cited key → its edge or node id.
    pub facts: Vec<AskFact>,
    pub origin: Origin,
    pub validation: Validation,
    pub calls: Vec<CallInfo>,
    /// Distinct models that produced this answer, as the UI names them (D48).
    pub models: Vec<atlas_llm::ModelLabel>,
    pub provenance: Vec<LlmCall>,
    /// Plain notes about the run (fallbacks, limits).
    pub notes: Vec<String>,
    pub notes_msg: Vec<Value>,
    /// Why the user's provider could not be used, when it failed (the UI offers alternatives).
    pub provider_error: Option<ProviderError>,
    pub conversation_id: Option<String>,
    pub saved: bool,
    /// `true` when only the chips were parsed (`plan_only`).
    pub planned_only: bool,
}

/// Typed provider failure for the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderError {
    /// Free tier quota or provider rate limit reached: bring your own key, a local model, or later.
    Quota,
    /// A key is missing or was rejected.
    Key,
    /// Not installed, not reachable, timed out.
    Unavailable,
    Other,
}

impl ProviderError {
    pub fn of(e: &LlmError) -> Self {
        let msg = e.to_string().to_lowercase();
        match e {
            _ if msg.contains("quota") || msg.contains("daily cap") || msg.contains("free tier") => Self::Quota,
            LlmError::RateLimited(_) => Self::Quota,
            LlmError::MissingKey { .. } | LlmError::Auth(_) => Self::Key,
            LlmError::Unavailable(_) | LlmError::Timeout(_) | LlmError::UnknownConnection(_) => Self::Unavailable,
            _ => Self::Other,
        }
    }
}

/// Connection used when the request names none: atlas-llm's default for a visitor without a key
/// (D48: the hosted free tier on OpenRouter gpt-oss-120b when this server has its key, then
/// KISSKI, then local Ollama; `ATLAS_LLM_DEFAULT` overrides).
pub fn default_connection_for(llm: &Llm) -> String {
    llm.default_connection(false)
}

pub const HOSTED_FREE: &str = atlas_llm::HOSTED_FREE;

/// Progress events for the streaming endpoint.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Status { stage: &'static str },
    Chips { chips: Vec<Chip> },
    Results { chips: Vec<Chip>, facts: Vec<AskFact> },
    Answer { response: Box<AskResponse> },
    Error { message: String },
}

/// The model's turn.
#[derive(Debug, Deserialize)]
struct Step {
    action: String,
    #[serde(default)]
    calls: Vec<serde_json::Map<String, Value>>,
    #[serde(default)]
    sentences: Vec<Cited>,
}

/// Runs questions against the atlas with the user's model.
#[derive(Clone)]
pub struct Asker {
    pub atlas: Arc<Atlas>,
    pub graph: Arc<Graph>,
    pub llm: Arc<Llm>,
}

struct Session<'a> {
    asker: &'a Asker,
    req: &'a AskRequest,
    call: Call,
    events: Option<&'a mpsc::Sender<Event>>,
    calls: Vec<Completion>,
    purposes: Vec<&'static str>,
    notes: Vec<String>,
    provider_error: Option<ProviderError>,
}

impl Session<'_> {
    async fn emit(&self, e: Event) {
        if let Some(tx) = self.events {
            let _ = tx.send(e).await;
        }
    }

    async fn model(&mut self, purpose: &'static str, req: CompletionRequest) -> Result<String, String> {
        let req = match &self.req.model {
            Some(m) if !m.trim().is_empty() => req.with_model(m.trim()),
            _ => req,
        };
        match self.asker.llm.complete(&self.call, req).await {
            Ok(done) => {
                let text = done.response.text.clone();
                self.calls.push(done);
                self.purposes.push(purpose);
                Ok(text)
            }
            Err(e) => {
                self.provider_error.get_or_insert(ProviderError::of(&e));
                Err(format!("provider: {e}"))
            }
        }
    }
}

impl Asker {
    pub fn new(atlas: Arc<Atlas>, graph: Arc<Graph>, llm: Arc<Llm>) -> Self {
        Self { atlas, graph, llm }
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx::new(&self.atlas, &self.graph)
    }

    /// Answer one question. Never fails on provider problems: it falls back to rule-based chips
    /// and a template answer and says why in `validation.fallback_reason` / `notes`.
    pub async fn ask(&self, req: &AskRequest, events: Option<&mpsc::Sender<Event>>) -> AskResponse {
        let named = req.connection.clone().filter(|c| !c.trim().is_empty());
        let fallback = named.is_none() && req.key.as_deref().is_none_or(|k| k.trim().is_empty());
        let connection = named.unwrap_or_else(|| default_connection_for(&self.llm));
        let mut call = Call::new(connection).with_private().with_fallback(fallback);
        if let Some(visitor) = &req.visitor {
            call = call.with_visitor(visitor);
        }
        if let Some(k) = req.key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
            call = call.with_key(ApiKey::new(k));
        }
        let mut s = Session {
            asker: self,
            req,
            call,
            events,
            calls: Vec::new(),
            purposes: Vec::new(),
            notes: Vec::new(),
            provider_error: None,
        };
        let mut validation = Validation::default();
        let mut direct: Option<Vec<Cited>> = None;

        // 1. The parsed question.
        let mut chips: Vec<Chip> = match &req.chips {
            Some(c) if !c.is_empty() => c.clone(),
            _ if req.no_llm => rules::parse(&self.ctx(), &req.question),
            _ => {
                s.emit(Event::Status { stage: "planning" }).await;
                match self.plan(&mut s).await {
                    Ok(Plan::Calls(c)) => c,
                    Ok(Plan::Answer(sentences)) => {
                        direct = Some(sentences);
                        Vec::new()
                    }
                    Err(e) => {
                        s.notes.push(format!(
                            "the model's plan was not usable ({e}); the atlas parsed the question itself"
                        ));
                        rules::parse(&self.ctx(), &req.question)
                    }
                }
            }
        };
        chips.truncate(MAX_CHIPS);
        for (i, c) in chips.iter_mut().enumerate() {
            c.id = format!("c{}", i + 1);
        }
        s.emit(Event::Chips { chips: chips.clone() }).await;
        if req.plan_only {
            return self.finish(s, chips, FactBook::new(), vec![], Origin::Llm, validation, true);
        }

        // 2. Run the chips.
        let mut book = FactBook::new();
        self.run_chips(&mut chips, &mut book, 0);
        s.emit(Event::Results {
            chips: chips.clone(),
            facts: book.all().to_vec(),
        })
        .await;

        // 3. Answer (the model may first ask for more), validate, regenerate once, else template.
        if let Some(sentences) = direct {
            let issues = self.check_answer(&sentences, &book, &chips);
            validation.attempts.push(issues.clone());
            if issues.is_empty() {
                validation.passed = true;
                return self.finish(s, chips, book, sentences, Origin::Llm, validation, false);
            }
            validation.fallback_reason = Some(format!("validator: {}", issues.join("; ")));
            let t = rules::template(&chips, &book);
            return self.finish(s, chips, book, t, Origin::Template, validation, false);
        }
        if req.no_llm {
            validation.fallback_reason = Some("no model requested".into());
            let t = rules::template(&chips, &book);
            return self.finish(s, chips, book, t, Origin::Template, validation, false);
        }
        s.emit(Event::Status { stage: "answering" }).await;
        let mut messages = self.answer_messages(&s, &chips, &book);
        let mut extra_rounds = 0;
        let mut regenerated = false;
        loop {
            let may_call = extra_rounds < MAX_EXTRA_ROUNDS && chips.len() < MAX_CHIPS;
            let schema = step_schema(&book, may_call);
            let req = request(messages.clone(), schema.clone(), "atlas_answer");
            let purpose = if regenerated { "retry" } else { "answer" };
            let text = match s.model(purpose, req).await {
                Ok(t) => t,
                Err(e) => {
                    validation.fallback_reason = Some(e);
                    break;
                }
            };
            let step = match check_json(&schema, &text)
                .and_then(|v| serde_json::from_value::<Step>(v).map_err(|e| e.to_string()))
            {
                Ok(st) => st,
                Err(e) => {
                    validation
                        .attempts
                        .push(vec![format!("reply is not a valid step: {e}")]);
                    if regenerated {
                        validation.fallback_reason = Some(format!("validator: {e}"));
                        break;
                    }
                    regenerated = true;
                    messages.push(Message::assistant(text));
                    messages.push(Message::user(format!(
                        "Your reply did not match the JSON schema: {e}\nReply again with only the corrected JSON."
                    )));
                    continue;
                }
            };
            if step.action == "call" && may_call && !step.calls.is_empty() {
                extra_rounds += 1;
                let new = self.new_chips(&step.calls, &chips);
                if new.is_empty() {
                    messages.push(Message::assistant(text));
                    messages.push(Message::user(
                        "Those calls were already answered above. Now answer with action \"answer\".".to_owned(),
                    ));
                    continue;
                }
                let start = chips.len();
                let mut new = new;
                for (i, c) in new.iter_mut().enumerate() {
                    c.id = format!("c{}", start + i + 1);
                }
                chips.extend(new);
                chips.truncate(MAX_CHIPS);
                let before = book.len();
                self.run_chips(&mut chips, &mut book, start);
                s.emit(Event::Results {
                    chips: chips.clone(),
                    facts: book.all().to_vec(),
                })
                .await;
                messages.push(Message::assistant(text));
                messages.push(Message::user(format!(
                    "More atlas results:\n{}\n\nNow answer the question with action \"answer\".",
                    results_block(&chips[start..], &book.all()[before..])
                )));
                continue;
            }
            let issues = self.check_answer(&step.sentences, &book, &chips);
            validation.attempts.push(issues.clone());
            if issues.is_empty() {
                validation.passed = true;
                return self.finish(s, chips, book, step.sentences, Origin::Llm, validation, false);
            }
            if regenerated {
                validation.fallback_reason = Some(format!("validator: {}", issues.join("; ")));
                break;
            }
            regenerated = true;
            messages.push(Message::assistant(text));
            messages.push(Message::user(format!(
                "Your answer broke these rules:\n- {}\nWrite it again following all rules, with action \"answer\". Reply with JSON only.",
                issues.join("\n- ")
            )));
        }
        let t = rules::template(&chips, &book);
        self.finish(s, chips, book, t, Origin::Template, validation, false)
    }

    fn run_chips(&self, chips: &mut [Chip], book: &mut FactBook, from: usize) {
        let ctx = self.ctx();
        for c in chips.iter_mut().skip(from) {
            exec::execute(ctx, c, book);
        }
    }

    fn new_chips(&self, calls: &[serde_json::Map<String, Value>], have: &[Chip]) -> Vec<Chip> {
        let mut out: Vec<Chip> = Vec::new();
        for c in calls.iter().filter_map(chip_from_call) {
            if !have.iter().chain(&out).any(|h| h.same_query(&c)) {
                out.push(c);
            }
        }
        out
    }

    async fn plan(&self, s: &mut Session<'_>) -> Result<Plan, String> {
        let lang = language_name(&s.req.lang);
        let system = format!("{}\n\n{}", system_intro(), plan_rules(&lang));
        let user = format!("{}Question: {}", history_block(&s.req.history), s.req.question.trim());
        let schema = step_schema(&FactBook::new(), true);
        let mut messages = vec![Message::system(system), Message::user(user)];
        for attempt in 0..2 {
            let text = s
                .model(
                    if attempt == 0 { "plan" } else { "retry" },
                    request(messages.clone(), schema.clone(), "atlas_plan"),
                )
                .await?;
            let problem = match check_json(&schema, &text)
                .and_then(|v| serde_json::from_value::<Step>(v).map_err(|e| e.to_string()))
            {
                Err(e) => format!("the reply did not match the JSON schema: {e}"),
                Ok(step) if step.action == "answer" => {
                    if step.sentences.is_empty() {
                        "an answer needs at least one sentence".into()
                    } else {
                        return Ok(Plan::Answer(step.sentences));
                    }
                }
                Ok(step) => {
                    let chips = self.new_chips(&step.calls, &[]);
                    let bad: Vec<String> = chips.iter().flat_map(Chip::check).collect();
                    if chips.is_empty() {
                        "call at least one intent".into()
                    } else if !bad.is_empty() {
                        bad.join("; ")
                    } else {
                        return Ok(Plan::Calls(chips));
                    }
                }
            };
            if attempt == 0 {
                messages.push(Message::assistant(text));
                messages.push(Message::user(format!(
                    "{problem}\nReply again with only the corrected JSON."
                )));
            } else {
                return Err(problem);
            }
        }
        unreachable!("the loop returns")
    }

    fn answer_messages(&self, s: &Session<'_>, chips: &[Chip], book: &FactBook) -> Vec<Message> {
        let lang = language_name(&s.req.lang);
        let system = format!("{}\n\n{}", system_intro(), answer_rules(&lang));
        let calls: Vec<Value> = chips.iter().map(chip_to_call).collect();
        let user = format!(
            "{}Question: {}\n\nYou called: {}\n\nAtlas results:\n{}\n\nAnswer the question in {lang} from these facts{}",
            history_block(&s.req.history),
            s.req.question.trim(),
            Value::Array(calls),
            results_block(chips, book.all()),
            if chips.len() < MAX_CHIPS {
                ", or call more intents first with action \"call\" if something essential is missing."
            } else {
                "."
            }
        );
        vec![Message::system(system), Message::user(user)]
    }

    /// atlas-llm's validator plus the conversation's own limits.
    pub fn check_answer(&self, sentences: &[Cited], book: &FactBook, chips: &[Chip]) -> Vec<String> {
        let facts = book.for_validator();
        let mut verbatim: BTreeSet<String> = BTreeSet::new();
        for c in chips {
            verbatim.extend(c.slots.values().cloned());
            for r in c.resolved.values() {
                verbatim.insert(r.node.label.clone());
                verbatim.insert(r.node.id.clone());
            }
        }
        let rules = Rules {
            min_sentences: 1,
            max_sentences: 12,
            max_words: Some(40),
            min_fact_sentences: usize::from(!book.is_empty()),
            kinds: &[SentenceKind::Fact, SentenceKind::Caveat, SentenceKind::Ask],
            required_kinds: &[],
            verbatim: verbatim.into_iter().collect(),
        };
        let mut issues = check(sentences, &facts, &rules);
        let non_fact = sentences.iter().filter(|s| s.kind != SentenceKind::Fact).count();
        if non_fact > MAX_NON_FACT && !book.is_empty() {
            issues.push(format!(
                "use at most {MAX_NON_FACT} sentences that are not cited facts, not {non_fact}"
            ));
        }
        issues
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        &self,
        s: Session<'_>,
        chips: Vec<Chip>,
        book: FactBook,
        answer: Vec<Cited>,
        origin: Origin,
        validation: Validation,
        planned_only: bool,
    ) -> AskResponse {
        let calls = s
            .calls
            .iter()
            .zip(&s.purposes)
            .map(|(c, p)| CallInfo {
                purpose: p,
                activity: c.provenance.activity.id.clone(),
                connection: c.response.connection.clone(),
                model: c.response.requested_model.clone(),
                model_label: c.response.model_label.clone(),
                reported_model: c.response.reported_model.clone(),
                cached: c.response.cached,
                latency_ms: c.response.latency_ms,
            })
            .collect();
        let lang = if origin == Origin::Template {
            "en".to_owned()
        } else {
            s.req.lang.clone()
        };
        AskResponse {
            question: s.req.question.clone(),
            lang,
            requested_lang: s.req.lang.clone(),
            chips,
            answer,
            facts: book.into_vec(),
            origin,
            validation,
            calls,
            models: atlas_llm::model_labels(&s.calls),
            provenance: s.calls.into_iter().map(|c| c.provenance).collect(),
            notes_msg: s
                .notes
                .iter()
                .map(|_| crate::copy::msg("ask.response.planning_fallback", json!({})))
                .collect(),
            notes: s.notes,
            provider_error: s.provider_error,
            conversation_id: s.req.conversation_id.clone(),
            saved: false,
            planned_only,
        }
    }
}

enum Plan {
    Calls(Vec<Chip>),
    Answer(Vec<Cited>),
}

fn request(messages: Vec<Message>, schema: Value, name: &str) -> CompletionRequest {
    CompletionRequest::new(messages)
        .with_schema(JsonSchema::new(name, schema))
        .with_temperature(0.0)
        .with_max_tokens(MAX_TOKENS)
        .with_deadline(DEADLINE)
}

/// A model call object → chip (`intent` + non-null slots).
pub fn chip_from_call(call: &serde_json::Map<String, Value>) -> Option<Chip> {
    let intent = IntentKind::parse(call.get("intent")?.as_str()?)?;
    let mut chip = Chip::new(intent, &[]);
    for (k, v) in call {
        if k == "intent" {
            continue;
        }
        if let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty())
            && intent.spec().slots.iter().any(|sl| sl.name == k)
        {
            chip.slots.insert(k.clone(), s.to_owned());
        }
    }
    chip.origin = ChipOrigin::Model;
    Some(chip)
}

fn chip_to_call(c: &Chip) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("intent".into(), json!(c.intent.as_str()));
    for (k, v) in &c.slots {
        m.insert(k.clone(), json!(v));
    }
    Value::Object(m)
}

/// Schema of one model turn. `cites` is restricted to the known keys; `calls` uses one flat
/// object with every slot (null when unused) so strict structured-output modes accept it.
pub fn step_schema(book: &FactBook, may_call: bool) -> Value {
    let keys: Vec<&str> = book.all().iter().map(|f| f.key.as_str()).collect();
    let cites = if keys.is_empty() {
        json!({"type": "array", "items": {"type": "string"}, "maxItems": 0})
    } else {
        json!({"type": "array", "items": {"type": "string", "enum": keys}})
    };
    let mut call_props = serde_json::Map::new();
    let names: Vec<&str> = INTENTS.iter().map(|i| i.name).collect();
    call_props.insert("intent".into(), json!({"type": "string", "enum": names}));
    let slots = all_slot_names();
    for s in &slots {
        call_props.insert((*s).into(), json!({"type": ["string", "null"]}));
    }
    let mut required = vec!["intent"];
    required.extend(slots.iter().copied());
    let actions: Vec<&str> = if may_call {
        vec!["call", "answer"]
    } else {
        vec!["answer"]
    };
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["action", "calls", "sentences"],
        "properties": {
            "action": {"type": "string", "enum": actions},
            "calls": {
                "type": "array",
                "maxItems": if may_call { 4 } else { 0 },
                "items": {"type": "object", "additionalProperties": false, "required": required, "properties": call_props}
            },
            "sentences": {
                "type": "array",
                "maxItems": 12,
                "items": {
                    "type": "object", "additionalProperties": false,
                    "required": ["text", "cites", "kind"],
                    "properties": {
                        "text": {"type": "string"},
                        "cites": cites,
                        "kind": {"type": "string", "enum": ["fact", "caveat", "ask"]}
                    }
                }
            }
        }
    })
}

fn system_intro() -> String {
    let lines: Vec<String> = INTENTS.iter().map(|i| i.prompt_line()).collect();
    format!(
        "You help families, patient groups and researchers use Zebratlas, a sourced map of rare \
conditions, genes, symptoms, patient groups, studies, registries, researchers and research projects. \
You never answer from your own memory. Every fact comes from the atlas through these query intents \
(write names as the user wrote them; the atlas resolves names, aliases, typos and ids):\n{}",
        lines.join("\n")
    )
}

fn plan_rules(lang: &str) -> String {
    format!(
        "Now turn the user's question into 1 to 4 intent calls: reply {{\"action\":\"call\",\"calls\":[...],\"sentences\":[]}}. \
Every call object lists every slot key; use null for slots its intent does not have. Prefer intents that lead to people \
and studies the user can contact (connections, assets) and add condition_summary_facts when the user wants to understand \
the condition. If the atlas cannot help (a greeting, a request for personal medical advice, dosing, a diagnosis), reply \
{{\"action\":\"answer\",\"calls\":[],\"sentences\":[...]}} with sentences of kind \"caveat\" or \"ask\" in {lang}, \
without facts, and offer what the atlas can show instead. Never give medical advice."
    )
}

fn answer_rules(lang: &str) -> String {
    format!(
        "Answer rules:\n\
- Write in {lang}. Copy names, gene symbols, ids, numbers and links exactly as in the facts; never translate them.\n\
- Use only the facts below. Do not add knowledge of your own.\n\
- Every sentence of kind \"fact\" lists in `cites` the keys of the facts it uses; it may only contain ids, symbols, \
numbers and links that appear in those facts.\n\
- Kind \"caveat\": what the atlas does not show or was not searched, or that this is not medical advice. Kind \"ask\": \
one short follow-up question to the user. At most {MAX_NON_FACT} such sentences.\n\
- 2 to 10 short sentences, at most 30 words each, plain words a 12-year-old understands; explain a medical word in the same sentence.\n\
- Lead with what the user can do: name the group, study or person and give its official link from the facts.\n\
- If the atlas found nothing, say so plainly and say what was searched (cite the coverage facts).\n\
- No medical advice, no promises, no diagnosis.\n\
Reply with JSON only: {{\"action\":\"answer\",\"calls\":[],\"sentences\":[{{\"text\":...,\"cites\":[...],\"kind\":\"fact\"}}]}}."
    )
}

fn history_block(history: &[Turn]) -> String {
    let recent: Vec<String> = history
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|t| {
            let who = if t.role == "assistant" {
                "Atlas assistant"
            } else {
                "User"
            };
            let text: String = t.text.chars().take(600).collect();
            format!("{who}: {text}")
        })
        .collect();
    if recent.is_empty() {
        String::new()
    } else {
        format!(
            "Earlier in this conversation (context only, not facts):\n{}\n\n",
            recent.join("\n")
        )
    }
}

/// Per chip: what ran, what it resolved to, its notes and its facts.
pub fn results_block(chips: &[Chip], new_facts: &[AskFact]) -> String {
    let mut out = String::new();
    for c in chips {
        let slots: Vec<String> = c.slots.iter().map(|(k, v)| format!("{k}={v}")).collect();
        out.push_str(&format!("## {} {}({})", c.id, c.intent.as_str(), slots.join(", ")));
        for (slot, r) in &c.resolved {
            out.push_str(&format!("\n{slot} → {} ({})", r.node.label, r.node.id));
        }
        for n in &c.notes {
            out.push_str(&format!("\nnote: {n}"));
        }
        let mine: Vec<AskFact> = new_facts.iter().filter(|f| c.facts.contains(&f.key)).cloned().collect();
        if mine.is_empty() {
            out.push_str("\n(no facts)");
        } else {
            out.push('\n');
            out.push_str(&facts::block(&mine));
        }
        out.push_str("\n\n");
    }
    out.trim_end().to_owned()
}
