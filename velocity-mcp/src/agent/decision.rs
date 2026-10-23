//! A pluggable *decision layer* — the fast "System 1" reflex that turns a blob
//! of application state into a handful of small, typed, probabilistic signals.
//!
//! This is deliberately **not** a chat model. Generative LLMs write the code,
//! explain the plan, and draft the response; a decision model answers bounded
//! questions ("single agent or team? does this need a repo pulled? which
//! target language? how complex?") and hands back calibrated probabilities the
//! rest of the program can branch on with ordinary, auditable code.
//!
//! Two backends are specced here because they share one interface contract:
//!
//! * [`LayaModel`] — Convai Innovations' open-weight, non-autoregressive 421M
//!   model. Apache-2.0, self-hosted, ~33 ms per decision, zero per-call cost.
//!   It runs on-device (a local GPU, an Apple-Silicon box, a mini-PC, or a
//!   plain CPU) and is exposed as an internal HTTP microservice.
//! * [`JevModel`] — TypeSafe AI's hosted System 1 endpoint
//!   (`POST /v1/systemone`). Closed-source and per-call billed, but fully
//!   managed. Same "state + typed questions -> keyed answers" paradigm, so it
//!   drops in behind the exact same [`DecisionModel`] trait.
//!
//! The whole point of the trait is that **the brain is swappable**: the IDE can
//! start on Laya for privacy + no API fees and move to Jev for zero ops, with
//! no change to any caller. When no backend is configured (or a call fails, or
//! confidence is too low), the caller falls back to its own deterministic
//! scorer — the system never depends on the network being up to make a decision.
//!
//! Both models answer the same three primitives, which is what [`Question`] and
//! [`Answer`] model:
//!
//! * **Choice** — pick one label from an approved set (routing, tagging).
//! * **Score** — rate against an *ordered* rubric (urgency, complexity).
//! * **Noul** — judge whether a statement is true, returning a yes-probability.
//!
//! A *decision matrix* is simply a named bundle of these questions for one
//! task; new tasks (approval gates, model-tier selection, triage) reuse the
//! exact same machinery by declaring more questions — see
//! [`crate::agent::intent`] for the first one wired up.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// The smallest useful slice of context to hand a decision model. Kept tiny on
/// purpose: models like Laya/Jev get *less* confused by less, and callers are
/// responsible for stripping secrets and irrelevant retrieval noise first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DecisionState {
    /// The primary textual state (e.g. the user's request).
    pub text: String,
    /// Optional structured facts as a JSON object, merged into the wire state.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub fields: Map<String, Value>,
}

impl DecisionState {
    /// Build a state from plain text.
    pub fn text(text: impl Into<String>) -> Self {
        DecisionState {
            text: text.into(),
            fields: Map::new(),
        }
    }

    /// Attach one structured fact alongside the text.
    pub fn with_field(mut self, key: &str, value: Value) -> Self {
        self.fields.insert(key.to_string(), value);
        self
    }

    /// Render the wire `state` node: an object when fields are present, else a
    /// bare string (both Laya and Jev accept either).
    pub fn to_value(&self) -> Value {
        if self.fields.is_empty() {
            return Value::String(self.text.clone());
        }
        let mut obj = Value::Object(self.fields.clone());
        obj.as_object_mut()
            .unwrap()
            .insert("text".to_string(), Value::String(self.text.clone()));
        obj
    }
}

/// One typed question — the unit of a decision. `id` keys the answer so callers
/// can connect a result to a known branch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Question {
    /// Choose exactly one of `choices`. Optional `descriptions` (parallel to
    /// `choices`) annotate each label; llama.cpp/Jev turn those into richer
    /// `criteria` and answer far more confidently than from bare labels.
    Choice {
        id: String,
        prompt: String,
        choices: Vec<String>,
        descriptions: Vec<String>,
    },
    /// Rate against an ordered rubric. `levels` run low -> high.
    Score {
        id: String,
        prompt: String,
        levels: Vec<String>,
    },
    /// Judge whether `prompt` is true; the answer is a yes-probability.
    Noul { id: String, prompt: String },
}

impl Question {
    /// The question id, common to all three primitives.
    pub fn id(&self) -> &str {
        match self {
            Question::Choice { id, .. }
            | Question::Score { id, .. }
            | Question::Noul { id, .. } => id,
        }
    }

    /// Wire type tag shared by both backends.
    fn kind(&self) -> &'static str {
        match self {
            Question::Choice { .. } => "choice",
            Question::Score { .. } => "score",
            Question::Noul { .. } => "noul",
        }
    }

    /// Serialize to the request `questions` entry, in the llama.cpp / Jev
    /// `systemone` wire shape: an `instructions` string plus a `criteria` node
    /// — a `label -> description` object for **choice** (descriptions optional)
    /// and an ordered array of levels for **score**.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("type".to_string(), Value::String(self.kind().to_string()));
        match self {
            Question::Choice {
                prompt,
                choices,
                descriptions,
                ..
            } => {
                m.insert("instructions".to_string(), Value::String(prompt.clone()));
                let mut crit = Map::new();
                for (i, c) in choices.iter().enumerate() {
                    let desc = descriptions
                        .get(i)
                        .filter(|d| !d.is_empty())
                        .map(|d| Value::String(d.clone()))
                        .unwrap_or(Value::Null);
                    crit.insert(c.clone(), desc);
                }
                m.insert("criteria".to_string(), Value::Object(crit));
            }
            Question::Score { prompt, levels, .. } => {
                m.insert("instructions".to_string(), Value::String(prompt.clone()));
                m.insert(
                    "criteria".to_string(),
                    Value::Array(levels.iter().cloned().map(Value::String).collect()),
                );
            }
            Question::Noul { prompt, .. } => {
                m.insert("instructions".to_string(), Value::String(prompt.clone()));
            }
        }
        Value::Object(m)
    }
}

/// One typed answer, keyed by the question `id` that produced it. Probabilities
/// are exposed alongside the label so callers can gate on certainty, not just
/// the argmax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Choice {
        id: String,
        selected: String,
        probabilities: Vec<(String, f32)>,
        confidence: f32,
    },
    Score {
        id: String,
        /// Normalized 0.0..=1.0 position on the rubric.
        value: f32,
        level: Option<String>,
        confidence: f32,
    },
    Noul {
        id: String,
        yes_probability: f32,
    },
}

impl Answer {
    /// The question id this answer belongs to.
    pub fn id(&self) -> &str {
        match self {
            Answer::Choice { id, .. } | Answer::Score { id, .. } | Answer::Noul { id, .. } => id,
        }
    }

    /// Confidence signal in [0,1] regardless of primitive (for Noul this is
    /// `|yes - 0.5| * 2`, i.e. how far from a coin flip the call is).
    pub fn confidence(&self) -> f32 {
        match self {
            Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => *confidence,
            Answer::Noul {
                yes_probability, ..
            } => (yes_probability - 0.5).abs() * 2.0,
        }
    }
}

/// The outcome of one decision call: every answer keyed by its question id,
/// plus which backend produced it (for logging / calibration tracking).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionOutput {
    pub answers: HashMap<String, Answer>,
    pub backend: String,
}

impl DecisionOutput {
    /// Borrow the answer for a question id, if present.
    pub fn get(&self, id: &str) -> Option<&Answer> {
        self.answers.get(id)
    }
}

/// Why a decision call could not be completed. Callers treat every variant as
/// "fall back to the deterministic scorer" — none of them are fatal.
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionError {
    /// The request could not be sent or the connection failed.
    Transport(String),
    /// The endpoint replied but the body was not the expected shape.
    Malformed(String),
    /// A required question id had no matching answer.
    MissingAnswer(String),
}

impl std::fmt::Display for DecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecisionError::Transport(m) => write!(f, "decision transport error: {m}"),
            DecisionError::Malformed(m) => write!(f, "decision response malformed: {m}"),
            DecisionError::MissingAnswer(id) => write!(f, "no answer returned for question '{id}'"),
        }
    }
}

impl std::error::Error for DecisionError {}

/// A swappable System-1 brain. Implementations answer a bundle of typed
/// questions about a state in one shot and return keyed, probabilistic answers.
pub trait DecisionModel {
    /// Short stable name for logging ("laya", "jev").
    fn name(&self) -> &str;
    /// Answer `questions` about `state`.
    fn decide(
        &self,
        state: &DecisionState,
        questions: &[Question],
    ) -> Result<DecisionOutput, DecisionError>;
}

/// Build the shared request envelope both backends understand.
pub fn build_request(state: &DecisionState, questions: &[Question]) -> Value {
    let mut qmap = Map::new();
    for q in questions {
        qmap.insert(q.id().to_string(), q.to_value());
    }
    let mut req = Map::new();
    req.insert("state".to_string(), state.to_value());
    req.insert("questions".to_string(), Value::Object(qmap));
    Value::Object(req)
}

/// Leniently parse a response body into keyed [`Answer`]s. Accepts the several
/// field spellings the published schemas use (`answer`/`choice`/`selected`,
/// `yes`/`yes_probability`/`probability`) so a schema tweak does not break the
/// client. Answers are matched back to `questions` by id and by declared type.
pub fn parse_answers(
    body: &Value,
    questions: &[Question],
) -> Result<DecisionOutput, DecisionError> {
    // The answers node may be at the top level or nested under a wrapper key.
    let answers_node = ["answers", "result", "decisions", "choices"]
        .iter()
        .find_map(|k| body.get(k).filter(|v| v.is_object()))
        .unwrap_or(body);
    if !answers_node.is_object() {
        return Err(DecisionError::Malformed(
            "no answers object in response".to_string(),
        ));
    }

    let mut answers = HashMap::new();
    for q in questions {
        let raw = match answers_node.get(q.id()) {
            Some(v) => v,
            None => continue, // missing answers handled by the caller below
        };
        if let Some(ans) = parse_one_answer(q, raw) {
            answers.insert(q.id().to_string(), ans);
        }
    }

    // Every requested question must have a usable answer.
    for q in questions {
        if !answers.contains_key(q.id()) {
            return Err(DecisionError::MissingAnswer(q.id().to_string()));
        }
    }
    Ok(DecisionOutput {
        answers,
        backend: String::new(),
    })
}

fn f32_of(v: Option<&Value>) -> Option<f32> {
    v.and_then(|x| x.as_f64()).map(|x| x as f32)
}

fn first_str(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| obj.get(*k).and_then(|v| v.as_str()).map(|s| s.to_string()))
}

fn parse_one_answer(q: &Question, raw: &Value) -> Option<Answer> {
    // A bare string/number is acceptable shorthand for some schemas.
    match q {
        Question::Choice { id, choices, .. } => {
            let obj = raw.as_object()?;
            let selected = first_str(obj, &["selected", "choice", "answer", "label"])?;
            let probabilities = match obj.get("probabilities").or_else(|| obj.get("scores")) {
                Some(Value::Object(m)) => m
                    .iter()
                    .filter_map(|(k, v)| v.as_f64().map(|p| (k.clone(), p as f32)))
                    .collect(),
                _ => choices
                    .iter()
                    .map(|c| (c.clone(), if c == &selected { 1.0 } else { 0.0 }))
                    .collect(),
            };
            let confidence =
                f32_of(obj.get("confidence").or_else(|| obj.get("probability"))).unwrap_or(0.0);
            Some(Answer::Choice {
                id: id.clone(),
                selected,
                probabilities,
                confidence,
            })
        }
        Question::Score { id, levels, .. } => {
            let obj = raw.as_object()?;
            // `value`/`normalized` are already on the 0..1 scale; llama.cpp
            // returns `score` as the *expected level index* (0..levels-1), so
            // divide it back into 0..1 when that's the only field present.
            let already_norm = obj.contains_key("value") || obj.contains_key("normalized");
            let raw_val = f32_of(
                obj.get("value")
                    .or_else(|| obj.get("normalized"))
                    .or_else(|| obj.get("score")),
            )?;
            let value = if already_norm || levels.len() < 2 {
                raw_val.clamp(0.0, 1.0)
            } else {
                (raw_val / (levels.len() - 1) as f32).clamp(0.0, 1.0)
            };
            let level = first_str(obj, &["level", "label"]);
            let confidence = f32_of(obj.get("confidence")).unwrap_or(value);
            Some(Answer::Score {
                id: id.clone(),
                value,
                level,
                confidence,
            })
        }
        Question::Noul { id, .. } => {
            // Some schemas return a bare number here.
            if let Some(p) = raw.as_f64() {
                return Some(Answer::Noul {
                    id: id.clone(),
                    yes_probability: p as f32,
                });
            }
            let obj = raw.as_object()?;
            let yes = f32_of(
                obj.get("noul")
                    .or_else(|| obj.get("yes"))
                    .or_else(|| obj.get("yes_probability"))
                    .or_else(|| obj.get("probability"))
                    .or_else(|| obj.get("score")),
            )?;
            Some(Answer::Noul {
                id: id.clone(),
                yes_probability: yes,
            })
        }
    }
}

/// Self-hosted Laya: an open-weight decision model exposed as a local HTTP
/// microservice. The endpoint is the base URL of that service; the request is
/// POSTed to `{endpoint}` (path configurable via [`LayaModel::path`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LayaModel {
    /// Base URL of the on-device inference service, e.g. `http://127.0.0.1:8321`.
    pub endpoint: String,
    /// Route the service exposes the shared envelope on.
    pub path: String,
    /// Wall-clock budget for the reflex call; Laya answers in ~33 ms so this is
    /// deliberately short — a miss falls back to the deterministic scorer fast.
    pub timeout_ms: u64,
}

impl LayaModel {
    fn url(&self) -> String {
        let base = self.endpoint.trim_end_matches('/');
        let path = self.path.trim_start_matches('/');
        if path.is_empty() {
            base.to_string()
        } else {
            format!("{base}/{path}")
        }
    }
}

impl DecisionModel for LayaModel {
    fn name(&self) -> &str {
        "laya"
    }

    fn decide(
        &self,
        state: &DecisionState,
        questions: &[Question],
    ) -> Result<DecisionOutput, DecisionError> {
        let mut req = build_request(state, questions);
        req.as_object_mut()
            .unwrap()
            .insert("model".to_string(), Value::String("laya".to_string()));
        let body = post_json(&self.url(), None, self.timeout_ms, req)?;
        finalize(body, questions, "laya")
    }
}

/// Hosted Jev: a managed System-1 endpoint. Same envelope as Laya, reached over
/// the public API with a bearer key.
#[derive(Debug, Clone, PartialEq)]
pub struct JevModel {
    /// Bearer token (from `JEV_API_KEY`), never logged.
    pub api_key: String,
    /// Model id, e.g. "jev-latest".
    pub model: String,
    /// Base API URL, defaulting to the documented host.
    pub base_url: String,
    pub timeout_ms: u64,
}

impl Default for JevModel {
    fn default() -> Self {
        JevModel {
            api_key: String::new(),
            model: "jev-latest".to_string(),
            base_url: "https://thejevai.com".to_string(),
            timeout_ms: 2000,
        }
    }
}

impl DecisionModel for JevModel {
    fn name(&self) -> &str {
        "jev"
    }

    fn decide(
        &self,
        state: &DecisionState,
        questions: &[Question],
    ) -> Result<DecisionOutput, DecisionError> {
        if self.api_key.is_empty() {
            return Err(DecisionError::Transport("missing JEV_API_KEY".to_string()));
        }
        let mut req = build_request(state, questions);
        req.as_object_mut()
            .unwrap()
            .insert("model".to_string(), Value::String(self.model.clone()));
        let url = format!("{}/v1/systemone", self.base_url.trim_end_matches('/'));
        let body = post_json(&url, Some(&self.api_key), self.timeout_ms, req)?;
        finalize(body, questions, "jev")
    }
}

fn finalize(
    body: Value,
    questions: &[Question],
    backend: &str,
) -> Result<DecisionOutput, DecisionError> {
    let mut out = parse_answers(&body, questions)?;
    out.backend = backend.to_string();
    Ok(out)
}

/// Issue the HTTP POST and deserialize the JSON body. Isolated so the pure
/// request/response helpers above stay network-free and unit-testable.
fn post_json(
    url: &str,
    bearer: Option<&str>,
    timeout_ms: u64,
    body: Value,
) -> Result<Value, DecisionError> {
    let timeout = std::time::Duration::from_millis(timeout_ms.max(1));
    let mut call = ureq::post(url)
        .timeout(timeout)
        .set("Content-Type", "application/json");
    if let Some(token) = bearer.filter(|t| !t.is_empty()) {
        call = call.set("Authorization", &format!("Bearer {token}"));
    }
    match call.send_json(body) {
        Ok(resp) => resp
            .into_json::<Value>()
            .map_err(|e| DecisionError::Malformed(format!("{url}: {e}"))),
        Err(e) => Err(DecisionError::Transport(format!("{url}: {e}"))),
    }
}

/// Which brain to use, resolved once from the environment. The default is
/// [`DecisionBackend::Heuristic`] so the system behaves deterministically (and
/// makes no network calls) until an operator explicitly opts into Laya or Jev.
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionBackend {
    /// Deterministic, in-process scorer — always available, no I/O. Callers use
    /// their own task-specific heuristic; this module does not model it.
    Heuristic,
    /// On-device Laya microservice.
    Laya(LayaModel),
    /// Hosted Jev API.
    Jev(JevModel),
}

impl DecisionBackend {
    /// The model to consult, if a networked backend is configured. `None` means
    /// "use the caller's deterministic fallback".
    pub fn model(&self) -> Option<&dyn DecisionModel> {
        match self {
            DecisionBackend::Heuristic => None,
            DecisionBackend::Laya(m) => Some(m),
            DecisionBackend::Jev(m) => Some(m),
        }
    }

    /// Stable name for logging.
    pub fn name(&self) -> &'static str {
        match self {
            DecisionBackend::Heuristic => "heuristic",
            DecisionBackend::Laya(_) => "laya",
            DecisionBackend::Jev(_) => "jev",
        }
    }
}

/// Resolve the active backend from environment configuration:
///
/// * `VELOCITY_DECISION_BACKEND` = `laya` | `jev` | `heuristic` (default).
/// * `VELOCITY_LAYA_ENDPOINT`   = base URL of the local Laya service
///   (`http://127.0.0.1:8321/decide` style), required for `laya`.
/// * `JEV_API_KEY`              = bearer token, required for `jev`.
/// * `VELOCITY_JEV_MODEL`       = model id, default `jev-latest`.
///
/// An unknown/absent selection, or a selection missing its endpoint/key, safely
/// degrades to [`DecisionBackend::Heuristic`].
pub fn resolve_backend() -> DecisionBackend {
    let get = |k: &str| std::env::var(k).unwrap_or_default().trim().to_string();
    match get("VELOCITY_DECISION_BACKEND").to_lowercase().as_str() {
        "laya" => {
            let endpoint = get("VELOCITY_LAYA_ENDPOINT");
            if endpoint.is_empty() {
                DecisionBackend::Heuristic
            } else {
                let (base, mut path) = split_endpoint(&endpoint);
                // llama.cpp serves the shared envelope on `/v1/systemone`; if the
                // operator gave only a bare host:port, default to that route.
                if path.is_empty() {
                    path = "/v1/systemone".to_string();
                }
                DecisionBackend::Laya(LayaModel {
                    endpoint: base,
                    path,
                    timeout_ms: parse_timeout(&get("VELOCITY_LAYA_TIMEOUT_MS"), 500),
                })
            }
        }
        "jev" => {
            let api_key = get("JEV_API_KEY");
            if api_key.is_empty() {
                DecisionBackend::Heuristic
            } else {
                DecisionBackend::Jev(JevModel {
                    api_key,
                    model: {
                        let m = get("VELOCITY_JEV_MODEL");
                        if m.is_empty() {
                            "jev-latest".to_string()
                        } else {
                            m
                        }
                    },
                    base_url: {
                        let b = get("VELOCITY_JEV_BASE_URL");
                        if b.is_empty() {
                            "https://thejevai.com".to_string()
                        } else {
                            b
                        }
                    },
                    timeout_ms: parse_timeout(&get("VELOCITY_JEV_TIMEOUT_MS"), 2000),
                })
            }
        }
        _ => DecisionBackend::Heuristic,
    }
}

fn parse_timeout(raw: &str, default_ms: u64) -> u64 {
    raw.parse::<u64>().unwrap_or(default_ms)
}

/// Split `http://host:port/route/decide` into (`http://host:port`, `/decide`).
fn split_endpoint(raw: &str) -> (String, String) {
    let without_scheme = raw.split_once("://").map(|(_, rest)| rest).unwrap_or(raw);
    match without_scheme.find('/') {
        Some(i) => {
            let scheme_len = raw.len() - without_scheme.len();
            let base = raw[..scheme_len + i].to_string();
            let path = without_scheme[i..].to_string();
            (base, path)
        }
        None => (raw.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn choice(id: &str, choices: &[&str]) -> Question {
        Question::Choice {
            id: id.to_string(),
            prompt: format!("{id}?"),
            choices: choices.iter().map(|s| s.to_string()).collect(),
            descriptions: Vec::new(),
        }
    }

    #[test]
    fn request_envelope_has_state_and_named_questions() {
        let state = DecisionState::text("clone litellm and rewrite it in rust")
            .with_field("lang_hint", json!("rust"));
        let qs = vec![
            choice("scope", &["single_agent", "team"]),
            Question::Noul {
                id: "needs_repo".into(),
                prompt: "does this need a repository?".into(),
            },
        ];
        let req = build_request(&state, &qs);
        assert!(req["state"]["text"].is_string());
        assert_eq!(req["state"]["lang_hint"], json!("rust"));
        assert_eq!(req["questions"]["scope"]["type"], json!("choice"));
        assert!(req["questions"]["scope"]["instructions"].is_string());
        assert!(req["questions"]["scope"]["criteria"]
            .as_object()
            .unwrap()
            .contains_key("single_agent"));
        assert_eq!(req["questions"]["needs_repo"]["type"], json!("noul"));
    }

    #[test]
    fn plain_text_state_serializes_as_string() {
        let req = build_request(
            &DecisionState::text("just a string"),
            &[choice("x", &["a"])],
        );
        assert_eq!(req["state"], json!("just a string"));
    }

    #[test]
    fn parses_typed_answers_from_a_jev_shaped_body() {
        let qs = vec![
            choice("scope", &["single_agent", "team"]),
            Question::Score {
                id: "complexity".into(),
                prompt: "how complex?".into(),
                levels: vec!["low".into(), "high".into()],
            },
            Question::Noul {
                id: "needs_repo".into(),
                prompt: "needs a repo?".into(),
            },
        ];
        let body = json!({
            "answers": {
                "scope": {"selected": "team", "probabilities": {"single_agent": 0.1, "team": 0.9}, "confidence": 0.9},
                "complexity": {"value": 0.72, "level": "high", "confidence": 0.66},
                "needs_repo": {"yes": 0.83}
            }
        });
        let out = parse_answers(&body, &qs).unwrap();
        match out.get("scope").unwrap() {
            Answer::Choice {
                selected,
                confidence,
                probabilities,
                ..
            } => {
                assert_eq!(selected, "team");
                assert!((*confidence - 0.9).abs() < 1e-6);
                assert_eq!(probabilities.len(), 2);
            }
            other => panic!("expected choice, got {other:?}"),
        }
        match out.get("complexity").unwrap() {
            Answer::Score { value, level, .. } => {
                assert!((*value - 0.72).abs() < 1e-6);
                assert_eq!(level.as_deref(), Some("high"));
            }
            other => panic!("expected score, got {other:?}"),
        }
        match out.get("needs_repo").unwrap() {
            Answer::Noul {
                yes_probability, ..
            } => assert!((*yes_probability - 0.83).abs() < 1e-6),
            other => panic!("expected noul, got {other:?}"),
        }
    }

    #[test]
    fn missing_answer_is_reported() {
        let qs = vec![
            choice("scope", &["single_agent", "team"]),
            Question::Noul {
                id: "needs_repo".into(),
                prompt: "?".into(),
            },
        ];
        let body = json!({ "answers": { "scope": {"selected": "team"} } });
        let err = parse_answers(&body, &qs).unwrap_err();
        assert_eq!(err, DecisionError::MissingAnswer("needs_repo".to_string()));
    }

    #[test]
    fn lenient_field_names_and_bare_number_noul() {
        let qs = vec![
            choice("route", &["a", "b"]),
            Question::Noul {
                id: "hot".into(),
                prompt: "?".into(),
            },
        ];
        let body = json!({ "route": {"answer": "b", "probability": 0.55}, "hot": 0.9 });
        let out = parse_answers(&body, &qs).unwrap();
        match out.get("route").unwrap() {
            Answer::Choice {
                selected,
                confidence,
                ..
            } => {
                assert_eq!(selected, "b");
                assert!((*confidence - 0.55).abs() < 1e-6);
            }
            other => panic!("{other:?}"),
        }
        match out.get("hot").unwrap() {
            Answer::Noul {
                yes_probability, ..
            } => assert!((*yes_probability - 0.9).abs() < 1e-6),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn answer_confidence_for_noul_is_distance_from_coinflip() {
        assert!(
            (Answer::Noul {
                id: "x".into(),
                yes_probability: 0.5
            }
            .confidence()
                - 0.0)
                .abs()
                < 1e-6
        );
        assert!(
            (Answer::Noul {
                id: "x".into(),
                yes_probability: 1.0
            }
            .confidence()
                - 1.0)
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn split_endpoint_separates_base_and_path() {
        assert_eq!(
            split_endpoint("http://127.0.0.1:8321/decide"),
            ("http://127.0.0.1:8321".to_string(), "/decide".to_string())
        );
        assert_eq!(
            split_endpoint("http://127.0.0.1:8321"),
            ("http://127.0.0.1:8321".to_string(), "".to_string())
        );
    }

    #[test]
    fn laya_url_joins_base_and_path() {
        let m = LayaModel {
            endpoint: "http://127.0.0.1:8321".into(),
            path: "/decide".into(),
            timeout_ms: 500,
        };
        assert_eq!(m.url(), "http://127.0.0.1:8321/decide");
    }

    #[test]
    fn resolve_defaults_to_heuristic() {
        // In the test env the opt-in vars are unset, so we must never select a
        // networked backend (that keeps the suite offline + deterministic).
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
        assert_eq!(resolve_backend(), DecisionBackend::Heuristic);
        assert!(DecisionBackend::Heuristic.model().is_none());
    }

    #[test]
    fn laya_selection_without_endpoint_degrades_to_heuristic() {
        std::env::set_var("VELOCITY_DECISION_BACKEND", "laya");
        std::env::remove_var("VELOCITY_LAYA_ENDPOINT");
        assert_eq!(resolve_backend(), DecisionBackend::Heuristic);
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
    }

    #[test]
    fn ev_selection_without_key_degrades_to_heuristic() {
        std::env::set_var("VELOCITY_DECISION_BACKEND", "jev");
        std::env::remove_var("JEV_API_KEY");
        assert_eq!(resolve_backend(), DecisionBackend::Heuristic);
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
    }
}
