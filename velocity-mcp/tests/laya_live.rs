//! LIVE end-to-end tests against a real on-device Laya server.
//!
//! These are `#[ignore]`d so the default suite stays offline and deterministic.
//! To run them, start llama.cpp serving the Laya decision GGUF, point the env
//! vars at it, then:
//!
//! ```text
//! llama-server -hf ggml-org/Laya-GGUF:Q8_0 --host 127.0.0.1 --port 8321
//! $env:VELOCITY_DECISION_BACKEND = "laya"
//! $env:VELOCITY_LAYA_ENDPOINT     = "http://127.0.0.1:8321"
//! cargo test -p velocity_mcp --test laya_live -- --ignored --nocapture --test-threads=1
//! ```
//!
//! They exercise the *real* client path — `resolve_backend` building a
//! `LayaModel`, `ureq` posting to `/v1/systemone`, and `parse_answers` reading
//! the llama.cpp reply — which the mock round-trip test cannot cover.

use std::time::Instant;

use velocity_mcp::agent::decision::{self, Answer, DecisionModel, DecisionState, Question};
use velocity_mcp::agent::intent;

fn force_laya_backend() -> decision::LayaModel {
    // Default to the local llama.cpp port if the caller did not override it.
    if std::env::var("VELOCITY_LAYA_ENDPOINT").is_err() {
        std::env::set_var("VELOCITY_LAYA_ENDPOINT", "http://127.0.0.1:8321");
    }
    std::env::set_var("VELOCITY_DECISION_BACKEND", "laya");
    match decision::resolve_backend() {
        decision::DecisionBackend::Laya(m) => {
            // Path-defaulting to /v1/systemone is itself part of what we verify.
            assert_eq!(m.path, "/v1/systemone", "bare host must default route");
            m
        }
        other => panic!("expected a laya backend, got {}", other.name()),
    }
}

#[test]
#[ignore = "requires a live Laya server (see module docs)"]
fn live_laya_answers_the_intent_matrix_fast() {
    let model = force_laya_backend();
    let qs: Vec<Question> = intent::intent_questions();
    let state = DecisionState::text("clone liteLLM and rewrite the entire codebase in rust");

    let mut times_ms: Vec<f64> = Vec::new();
    let mut last: Option<decision::DecisionOutput> = None;
    for _ in 0..12 {
        let t = Instant::now();
        let out = model.decide(&state, &qs).expect("live Laya must answer");
        times_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        last = Some(out);
    }
    let out = last.unwrap();

    // Every declared question must come back with a usable, correctly-typed answer.
    for q in &qs {
        assert!(out.get(q.id()).is_some(), "missing answer for {}", q.id());
    }
    assert_eq!(out.backend, "laya");
    match out.get("scope") {
        Some(Answer::Choice { selected, .. }) => {
            assert!(
                selected == "team" || selected == "single_agent",
                "scope answer must be one of the offered labels, got {selected}"
            );
        }
        other => panic!("scope was not a Choice: {other:?}"),
    }
    match out.get("target_language") {
        Some(Answer::Choice { selected, .. }) => assert!(
            selected == "rust" || selected == "none" || !selected.is_empty(),
            "language must be a label"
        ),
        other => panic!("target_language was not a Choice: {other:?}"),
    }

    let n = times_ms.len() as f64;
    let avg = times_ms.iter().sum::<f64>() / n;
    let min = times_ms.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = times_ms.iter().cloned().fold(0.0_f64, f64::max);
    println!(
        "LIVE_LAYA_LATENCY n={n:.0} min_ms={min:.1} avg_ms={avg:.1} max_ms={max:.1} backend={}",
        out.backend
    );
    // A reflex brain must stay interactive: fail loudly if the round trip stalls.
    assert!(
        avg < 1000.0,
        "live decision too slow for chat: avg {avg:.1} ms"
    );
}

#[test]
#[ignore = "requires a live Laya server (see module docs)"]
fn live_decide_intent_routes_a_repo_port_to_a_team() {
    let _model = force_laya_backend();
    // decide_intent consults the live model, then honors its own confidence
    // floor / deterministic fallback. Either way a whole-repo rust port must
    // resolve to a team with specialists staffed.
    let d = intent::decide_intent("clone liteLLM and rewrite the entire codebase in rust");
    println!(
        "DECIDE_INTENT scope={:?} conf={:.2} roles={} target_lang={:?} signals={:?}",
        d.scope,
        d.confidence,
        d.roles.len(),
        d.target_language,
        d.signals
    );
    assert_eq!(d.scope, intent::TaskScope::Team);
    assert!(!d.roles.is_empty(), "team scope must staff roles");
}

#[test]
#[ignore = "requires a live Laya server (see module docs)"]
fn live_model_selection_is_constrained_to_usable_candidates() {
    use velocity_mcp::agent::{AiProvider, ApiStyle, ModelInfo};
    use velocity_mcp::automation::instruction_registry::AgentTaskKind;
    use velocity_mcp::automation::model_quality::ModelQualityIndex;

    let _model = force_laya_backend();
    // Only these three models are "usable" for the caller (enabled API, valid
    // token plan). The selection must land on one of these ids, never on a
    // plausible-but-disabled model the reflex hallucinates.
    let mk = |id: &str, tools: bool, think: bool| ModelInfo {
        id: id.to_string(),
        label: id.to_string(),
        api_style: ApiStyle::OpenAiTools,
        supports_tools: tools,
        supports_thinking: think,
    };
    let usable = vec![
        mk("qwen3-coder", true, false),
        mk("deepseek-v3", true, true),
        mk("kimi-k2", true, true),
    ];
    let sel = ModelQualityIndex::decide_model_for_task(
        AgentTaskKind::Refactor,
        AiProvider::AlibabaQwen,
        &usable,
        "refactor a large coupled module across several files, needs tool use and reasoning",
    );
    assert!(
        usable.iter().any(|m| m.id == sel.candidate.model_id),
        "selection escaped the usable set: {:?}",
        sel.candidate.model_id
    );
    println!(
        "MODEL_SELECTION backend={} id={} conf={:.2} rationale={}",
        sel.backend, sel.candidate.model_id, sel.confidence, sel.rationale
    );
}
