//! Micro-benchmarks for the System-1 decision layer *around* the network hop:
//! request-envelope building, response parsing, and the deterministic
//! heuristic fallbacks. These are the paths every decision pays in-process,
//! so their cost — not the ~30 ms reflex round-trip — is what a cold
//! in-editor decision actually adds to a turn.
//!
//! Run: `cargo bench -p velocity_mcp --bench decision_micro`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use serde_json::json;
use std::collections::HashMap;
use velocity_mcp::agent::decision::{build_request, parse_answers, DecisionState, Question};
use velocity_mcp::agent::AiProvider;
use velocity_mcp::automation::instruction_registry::AgentTaskKind;
use velocity_mcp::automation::model_quality::ModelQualityIndex;

fn intent_matrix() -> Vec<Question> {
    vec![
        Question::Choice {
            id: "scope".to_string(),
            prompt: "Is this a single-agent task or a team task?".to_string(),
            choices: vec!["single_agent".to_string(), "team".to_string()],
            descriptions: vec![
                "a focused change on one module or surface; a single agent is enough".to_string(),
                "spans many modules, domains, or an entire repository".to_string(),
            ],
        },
        Question::Choice {
            id: "target_language".to_string(),
            prompt: "Which target language does the user ask for?".to_string(),
            choices: vec!["rust".to_string(), "python".to_string(), "none".to_string()],
            descriptions: vec![],
        },
        Question::Noul {
            id: "requires_repo_ingest".to_string(),
            prompt: "Must a repository be cloned and ingested first?".to_string(),
        },
    ]
}

fn model_choice_matrix(n: usize) -> Vec<Question> {
    let choices: Vec<String> = (0..n).map(|i| format!("model-{i}")).collect();
    let descriptions = choices
        .iter()
        .map(|c| format!("{c} (supports tool calling)"))
        .collect();
    vec![Question::Choice {
        id: "model".to_string(),
        prompt: "For a refactor task, which of these available models is the best fit?".to_string(),
        choices,
        descriptions,
    }]
}

fn sample_response_body(n_choices: usize) -> serde_json::Value {
    let probs: serde_json::Map<String, serde_json::Value> = (0..n_choices)
        .map(|i| {
            (
                format!("model-{i}"),
                json!(if i == 0 {
                    0.42
                } else {
                    0.58 / (n_choices as f64 - 1.0)
                }),
            )
        })
        .collect();
    json!({
        "model": "ggml-org/Laya-GGUF",
        "answers": {
            "model": {
                "type": "choice",
                "choice": "model-0",
                "probabilities": serde_json::Value::Object(probs),
                "confidence": 0.13
            }
        },
        "usage": { "input_tokens": 90, "output_tokens": 0 }
    })
}

fn bench_request_building(c: &mut Criterion) {
    let mut g = c.benchmark_group("request_building");
    for n in [3usize, 10, 24] {
        let questions = model_choice_matrix(n);
        let state = DecisionState::text("refactor a tightly coupled module across services")
            .with_field("task_kind", json!("refactor"));
        g.bench_with_input(BenchmarkId::new("envelope_json", n), &questions, |b, q| {
            b.iter(|| {
                let body = build_request(&state, std::hint::black_box(q));
                serde_json::to_string(&body).unwrap()
            })
        });
    }
    g.bench_with_input(
        BenchmarkId::new("intent_matrix_json", 0),
        &intent_matrix(),
        |b, q| {
            let state =
                DecisionState::text("clone liteLLM and rewrite the entire codebase in rust");
            b.iter(|| {
                let body = build_request(&state, std::hint::black_box(q));
                serde_json::to_string(&body).unwrap()
            })
        },
    );
    g.finish();
}

fn bench_response_parsing(c: &mut Criterion) {
    let mut g = c.benchmark_group("response_parsing");
    for n in [3usize, 10, 24] {
        let questions = model_choice_matrix(n);
        let body = sample_response_body(n);
        g.bench_with_input(BenchmarkId::new("choice_answers", n), &questions, |b, q| {
            b.iter(|| parse_answers(std::hint::black_box(&body), std::hint::black_box(q)).unwrap())
        });
    }
    g.finish();
}

fn bench_heuristic_fallback(c: &mut Criterion) {
    let mut g = c.benchmark_group("heuristic_fallback");
    for n in [3usize, 10, 24] {
        let models: Vec<_> = (0..n)
            .map(|i| velocity_mcp::agent::ModelInfo {
                id: format!("model-{i}"),
                label: format!("model-{i}"),
                api_style: velocity_mcp::agent::ApiStyle::OpenAiTools,
                supports_tools: true,
                supports_thinking: i % 2 == 0,
            })
            .collect();
        g.bench_with_input(
            BenchmarkId::new("decide_model_none", n),
            &models,
            |b, ms| {
                b.iter(|| {
                    ModelQualityIndex::decide_model_with(
                        AgentTaskKind::Refactor,
                        AiProvider::AlibabaQwen,
                        std::hint::black_box(ms),
                        "refactor a tightly coupled module",
                        None,
                    )
                })
            },
        );
    }
    // A dead endpoint must cost only the (sub-millisecond) connect refusal.
    let models: Vec<_> = (0..3)
        .map(|i| velocity_mcp::agent::ModelInfo {
            id: format!("model-{i}"),
            label: format!("model-{i}"),
            api_style: velocity_mcp::agent::ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: true,
        })
        .collect();
    struct Dead;
    impl velocity_mcp::agent::decision::DecisionModel for Dead {
        fn name(&self) -> &str {
            "laya"
        }
        fn decide(
            &self,
            _state: &DecisionState,
            _questions: &[Question],
        ) -> Result<
            velocity_mcp::agent::decision::DecisionOutput,
            velocity_mcp::agent::decision::DecisionError,
        > {
            Err(velocity_mcp::agent::decision::DecisionError::Transport(
                "refused".to_string(),
            ))
        }
    }
    g.bench_function("decide_model_transport_down", |b| {
        b.iter(|| {
            ModelQualityIndex::decide_model_with(
                AgentTaskKind::Refactor,
                AiProvider::AlibabaQwen,
                &models,
                "refactor a tightly coupled module",
                Some(&Dead),
            )
        })
    });
    g.finish();
    // Keep the map type import meaningful in scope without a wildcard use.
    let _ = HashMap::<String, String>::new();
}

criterion_group!(
    benches,
    bench_request_building,
    bench_response_parsing,
    bench_heuristic_fallback
);
criterion_main!(benches);
