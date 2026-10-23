use crate::agent::decision::{self, Answer, DecisionModel, DecisionState, Question};
use crate::agent::{AiProvider, ApiStyle, ModelInfo};
use crate::automation::instruction_registry::AgentTaskKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRequirements {
    pub needs_tools: bool,
    pub needs_reasoning: bool,
    pub prefers_long_context: bool,
}

impl TaskRequirements {
    pub fn for_kind(kind: AgentTaskKind) -> Self {
        match kind {
            AgentTaskKind::Refactor => Self {
                needs_tools: true,
                needs_reasoning: true,
                prefers_long_context: true,
            },
            AgentTaskKind::BugFix => Self {
                needs_tools: true,
                needs_reasoning: true,
                prefers_long_context: false,
            },
            AgentTaskKind::Test => Self {
                needs_tools: true,
                needs_reasoning: false,
                prefers_long_context: false,
            },
            AgentTaskKind::Documentation => Self {
                needs_tools: false,
                needs_reasoning: false,
                prefers_long_context: true,
            },
            AgentTaskKind::Analysis => Self {
                needs_tools: false,
                needs_reasoning: true,
                prefers_long_context: true,
            },
            AgentTaskKind::Planning => Self {
                needs_tools: false,
                needs_reasoning: true,
                prefers_long_context: true,
            },
            AgentTaskKind::Merge => Self {
                needs_tools: true,
                needs_reasoning: true,
                prefers_long_context: true,
            },
            AgentTaskKind::DesktopAutomation => Self {
                needs_tools: true,
                needs_reasoning: true,
                prefers_long_context: true,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelCandidate {
    pub provider: AiProvider,
    pub model_id: String,
    pub label: String,
    pub score: i32,
    pub supports_tools: bool,
    pub supports_thinking: bool,
    pub api_style: ApiStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapability {
    pub provider: AiProvider,
    pub native_tools_reliable: bool,
    pub good_for_parallelism: bool,
}

/// Outcome of a model-selection decision: the chosen candidate plus provenance
/// (which backend produced it and how sure it was). `backend` is `"heuristic"`
/// whenever the deterministic ranking was used instead of a System-1 model.
#[derive(Debug, Clone)]
pub struct ModelSelection {
    pub candidate: ModelCandidate,
    pub backend: String,
    pub confidence: f32,
    pub rationale: String,
}

/// Minimum probability a networked model must place on its chosen (and
/// usable-set-valid) option before we adopt the model's pick over the
/// deterministic ranking. We read the *probability of the selected option*
/// rather than the model's self-reported `confidence`, because small reflex
/// models like Laya report very conservative confidence even when their
/// probability ranking is a clear, usable argmax. The hard safety guarantee is
/// membership in the usable set, enforced separately.
pub const MODEL_CHOICE_MIN_PROBABILITY: f32 = 0.35;

/// How far ahead of the runner-up the chosen option must sit to count as a
/// *clear* argmax. Reflex distributions flatten as the candidate pool grows
/// (a 10-model token plan spreads ~0.10/0.19 across look-alike ids), so a flat
/// floor tuned for 3-option pools would silence the model on every big catalog.
/// Tuned on live Laya output: real clear-picks lead by ~3.3-6pp; coin-flip
/// photo-finishes (synthetic 0.18-vs-0.17, and Laya's own unstable near-ties)
/// stay below the bar.
pub const MODEL_CHOICE_DOMINANCE_MARGIN: f32 = 0.03;

/// Adoption bar for a pool of `n` options: ~1.8x the uniform baseline (1/n) —
/// the winner must be dramatically likelier than a random draw — clamped to
/// never exceed the flat floor (small pools are judged by the floor itself,
/// exactly as before) and never drop below 0.15 (tiny/absurd maps still need a
/// real signal). Result: n ≤ 5 behaves as the constant 0.35; a 10-model token
/// plan is judged at 0.18 instead of an unreachable flat 0.35.
pub fn model_choice_probability_floor(n: usize) -> f32 {
    let n = n.max(1) as f32;
    (1.8 / n).clamp(0.15, MODEL_CHOICE_MIN_PROBABILITY)
}

pub struct ModelQualityIndex;

impl ModelQualityIndex {
    pub fn provider_capabilities() -> [ProviderCapability; 16] {
        [
            ProviderCapability {
                provider: AiProvider::CloudflareWorkersAi,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::OpenRouter,
                native_tools_reliable: false,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::OpenAI,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::Anthropic,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::GoogleVertex,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::AzureOpenAi,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::LocalOllama,
                native_tools_reliable: true,
                good_for_parallelism: false,
            },
            ProviderCapability {
                provider: AiProvider::Deepseek,
                native_tools_reliable: true,
                good_for_parallelism: false,
            },
            ProviderCapability {
                provider: AiProvider::AlibabaQwen,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::Groq,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::Mistral,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::TogetherAi,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::FireworksAi,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::Perplexity,
                native_tools_reliable: false,
                good_for_parallelism: false,
            },
            ProviderCapability {
                provider: AiProvider::Cerebras,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
            ProviderCapability {
                provider: AiProvider::AwsBedrock,
                native_tools_reliable: true,
                good_for_parallelism: true,
            },
        ]
    }

    pub fn rank_models(
        kind: AgentTaskKind,
        provider: AiProvider,
        models: &[ModelInfo],
    ) -> Vec<ModelCandidate> {
        let requirements = TaskRequirements::for_kind(kind);
        let provider_caps = Self::provider_capabilities()
            .into_iter()
            .find(|caps| caps.provider == provider)
            .unwrap_or(ProviderCapability {
                provider,
                native_tools_reliable: false,
                good_for_parallelism: false,
            });

        let mut ranked: Vec<_> = models
            .iter()
            .map(|model| ModelCandidate {
                provider,
                model_id: model.id.clone(),
                label: model.label.clone(),
                score: Self::score_model(model, requirements, provider_caps),
                supports_tools: model.supports_tools,
                supports_thinking: model.supports_thinking,
                api_style: model.api_style,
            })
            .collect();
        ranked.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.label.cmp(&b.label)));
        ranked
    }

    fn score_model(
        model: &ModelInfo,
        requirements: TaskRequirements,
        caps: ProviderCapability,
    ) -> i32 {
        let mut score = 0;

        if requirements.needs_tools {
            score += if model.supports_tools { 35 } else { -25 };
            if model.supports_tools && caps.native_tools_reliable {
                score += 15;
            }
        } else if model.supports_tools {
            score += 5;
        }

        if requirements.needs_reasoning {
            score += if model.supports_thinking { 25 } else { -10 };
        } else if model.supports_thinking {
            score += 5;
        }

        if requirements.prefers_long_context {
            let lower = model.id.to_lowercase();
            if lower.contains("32b")
                || lower.contains("70b")
                || lower.contains("72b")
                || lower.contains("large")
                || lower.contains("long")
                || lower.contains("sonnet")
                || lower.contains("opus")
                || lower.contains("kimi")
                || lower.contains("qwen3")
                || lower.contains("deepseek")
            {
                score += 18;
            }
        }

        score += match model.api_style {
            ApiStyle::OpenAiTools => 12,
            ApiStyle::OpenAiChat => 6,
            ApiStyle::PromptCompletion => -8,
        };

        let lower = model.id.to_lowercase();
        if lower.contains("kimi")
            || lower.contains("claude")
            || lower.contains("deepseek")
            || lower.contains("qwen")
        {
            score += 8;
        }
        if lower.contains("free") {
            score -= 4;
        }
        if caps.good_for_parallelism {
            score += 4;
        }

        score
    }

    /// Recommend the best model for a given task from the available models.
    /// Returns None if no models are available.
    pub fn recommend_model_for_task(
        kind: AgentTaskKind,
        provider: AiProvider,
        models: &[ModelInfo],
    ) -> Option<ModelCandidate> {
        Self::rank_models(kind, provider, models)
            .into_iter()
            .max_by_key(|c| c.score)
    }

    /// Suggest whether the current provider is optimal for the task,
    /// or if switching to another provider would be beneficial.
    pub fn should_switch_provider(
        kind: AgentTaskKind,
        current_provider: AiProvider,
    ) -> Option<AiProvider> {
        let requirements = TaskRequirements::for_kind(kind);
        let caps = Self::provider_capabilities()
            .into_iter()
            .find(|c| c.provider == current_provider)?;

        // If task needs reliable tools but current provider doesn't have them
        if requirements.needs_tools && !caps.native_tools_reliable {
            // Suggest switching to a provider with reliable tool support
            return Some(AiProvider::Anthropic); // Best tool reliability
        }

        // If task needs parallelism but current provider isn't good for it
        if kind == AgentTaskKind::DesktopAutomation && !caps.good_for_parallelism {
            return Some(AiProvider::Groq); // Fast, good for parallel tasks
        }

        None // Current provider is fine
    }

    /// Pick the best model for a task from ONLY the models the caller says are
    /// usable, consulting a System-1 reflex brain (`Some(model)`) but guarding
    /// it hard: whatever the brain answers must be a member of `usable_models`,
    /// or we discard it and fall back to the deterministic [`rank_models`]
    /// ordering. This is what lets Laya/Jev steer model choice *without ever*
    /// selecting a model that is not actually enabled (a disabled provider API,
    /// a stale token-plan name, a hallucinated id).
    pub fn decide_model_with(
        kind: AgentTaskKind,
        provider: AiProvider,
        usable_models: &[ModelInfo],
        task_text: &str,
        model: Option<&dyn DecisionModel>,
    ) -> ModelSelection {
        let ranked = Self::rank_models(kind, provider, usable_models);
        let heuristic = || {
            match ranked.first() {
            Some(c) => ModelSelection {
                candidate: c.clone(),
                backend: "heuristic".to_string(),
                confidence: 1.0,
                rationale: format!(
                    "Deterministic ranking chose '{}' (score {}) as the best usable model for a {} task.",
                    c.model_id, c.score, kind.as_str()
                ),
            },
            None => ModelSelection {
                candidate: ModelCandidate {
                    provider,
                    model_id: String::new(),
                    label: String::new(),
                    score: 0,
                    supports_tools: false,
                    supports_thinking: false,
                    api_style: ApiStyle::OpenAiChat,
                },
                backend: "heuristic".to_string(),
                confidence: 0.0,
                rationale: "No usable models were provided for selection.".to_string(),
            },
        }
        };

        let Some(model) = model else {
            return heuristic();
        };
        if usable_models.is_empty() {
            return heuristic();
        }

        let ids: Vec<String> = usable_models.iter().map(|m| m.id.clone()).collect();
        // Describe each candidate by capability so the reflex model can score
        // them meaningfully instead of guessing among bare, look-alike ids.
        let req = TaskRequirements::for_kind(kind);
        let descriptions: Vec<String> = usable_models
            .iter()
            .map(|m| {
                let mut bits: Vec<&str> = Vec::new();
                bits.push(if m.supports_tools {
                    "supports tool calling"
                } else {
                    "no native tools"
                });
                if m.supports_thinking {
                    bits.push("extended reasoning");
                }
                let mut d = format!("{} ({})", m.label, bits.join(", "));
                if req.needs_tools && m.supports_tools {
                    d.push_str("; fits this tool-heavy task");
                }
                if req.needs_reasoning && m.supports_thinking {
                    d.push_str("; fits this reasoning task");
                }
                d
            })
            .collect();
        let questions = vec![Question::Choice {
            id: "model".to_string(),
            prompt: format!(
                "For a {} task, which of these available models is the best fit?",
                kind.as_str()
            ),
            choices: ids.clone(),
            descriptions,
        }];
        let state = DecisionState::text(task_text).with_field(
            "task_kind",
            serde_json::Value::String(kind.as_str().to_string()),
        );

        let Ok(out) = model.decide(&state, &questions) else {
            return heuristic();
        };
        // Accept the model's pick only if it (a) answered the `model` question
        // with a Choice, (b) chose an id we actually offered, and (c) is at or
        // above the confidence floor. Otherwise the deterministic ranking wins.
        if let Some(Answer::Choice {
            selected,
            confidence,
            probabilities,
            ..
        }) = out.get("model")
        {
            // Trust the probability on the chosen option, falling back to the
            // self-reported confidence when no probability map came back.
            let sel_prob = probabilities
                .iter()
                .find(|(label, _)| label == selected)
                .map(|(_, p)| *p)
                .unwrap_or(*confidence);
            // The runner-up is the strongest *other* option; with no map we
            // cannot measure dominance, so treat it as non-binding and let the
            // (fallback) confidence floor decide.
            let runner_up = probabilities
                .iter()
                .filter(|(label, _)| label != selected)
                .map(|(_, p)| *p)
                .fold(0.0f32, f32::max);
            // Milliprobability space: raw f32 comparisons must not flip a
            // near-tie either way — 0.192f32 actually stores 0.19200001, and
            // 0.37f32 + 0.05f32 sums to 0.41999996, so naive `<`/`+` on these
            // literals misjudge exact-looking gaps. Integer millis are exact.
            let dominance_required = !probabilities.is_empty()
                && (sel_prob * 1000.0).round()
                    < (runner_up * 1000.0).round()
                        + (MODEL_CHOICE_DOMINANCE_MARGIN * 1000.0).round();
            let floor = model_choice_probability_floor(ids.len());
            let in_set = ids.iter().any(|id| id == selected);
            if in_set && sel_prob >= floor && !dominance_required {
                if let Some(cand) = ranked.iter().find(|c| c.model_id == *selected).cloned() {
                    return ModelSelection {
                        candidate: cand,
                        backend: out.backend.clone(),
                        confidence: *confidence,
                        rationale: format!(
                            "{} chose '{}' for a {} task (option probability {sel_prob:.2}, bar {floor:.2}).",
                            out.backend, selected, kind.as_str()
                        ),
                    };
                }
            }
        }
        heuristic()
    }

    /// Convenience wrapper: resolve the configured backend (Laya/Jev/heuristic)
    /// from the environment and run [`decide_model_with`]. With no model wired
    /// up it is exactly the deterministic ranking — never a network surprise.
    pub fn decide_model_for_task(
        kind: AgentTaskKind,
        provider: AiProvider,
        usable_models: &[ModelInfo],
        task_text: &str,
    ) -> ModelSelection {
        let backend = decision::resolve_backend();
        Self::decide_model_with(kind, provider, usable_models, task_text, backend.model())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_tool_capable_models_for_refactor() {
        let models = vec![
            ModelInfo {
                id: "provider/basic-chat".to_string(),
                label: "basic-chat".to_string(),
                api_style: ApiStyle::OpenAiChat,
                supports_tools: false,
                supports_thinking: false,
            },
            ModelInfo {
                id: "provider/kimi-k2".to_string(),
                label: "kimi-k2".to_string(),
                api_style: ApiStyle::OpenAiTools,
                supports_tools: true,
                supports_thinking: true,
            },
        ];

        let ranked = ModelQualityIndex::rank_models(
            AgentTaskKind::Refactor,
            AiProvider::CloudflareWorkersAi,
            &models,
        );
        assert_eq!(
            ranked.first().map(|candidate| candidate.label.as_str()),
            Some("kimi-k2")
        );
    }

    #[test]
    fn task_requirements_for_all_kinds() {
        let kinds = vec![
            (AgentTaskKind::Refactor, true, true, true),
            (AgentTaskKind::BugFix, true, true, false),
            (AgentTaskKind::Test, true, false, false),
            (AgentTaskKind::Documentation, false, false, true),
            (AgentTaskKind::Analysis, false, true, true),
            (AgentTaskKind::Planning, false, true, true),
            (AgentTaskKind::Merge, true, true, true),
            (AgentTaskKind::DesktopAutomation, true, true, true),
        ];
        for (kind, needs_tools, needs_reasoning, prefers_long) in kinds {
            let req = TaskRequirements::for_kind(kind);
            assert_eq!(req.needs_tools, needs_tools, "{:?} needs_tools", kind);
            assert_eq!(
                req.needs_reasoning, needs_reasoning,
                "{:?} needs_reasoning",
                kind
            );
            assert_eq!(
                req.prefers_long_context, prefers_long,
                "{:?} prefers_long",
                kind
            );
        }
    }

    #[test]
    fn rank_models_empty_list() {
        let ranked = ModelQualityIndex::rank_models(
            AgentTaskKind::Test,
            AiProvider::CloudflareWorkersAi,
            &[],
        );
        assert!(ranked.is_empty());
    }

    #[test]
    fn recommend_model_for_task_empty_returns_none() {
        let result = ModelQualityIndex::recommend_model_for_task(
            AgentTaskKind::Test,
            AiProvider::CloudflareWorkersAi,
            &[],
        );
        assert!(result.is_none());
    }

    #[test]
    fn recommend_model_for_task_returns_best() {
        let models = vec![
            ModelInfo {
                id: "provider/basic".to_string(),
                label: "basic".to_string(),
                api_style: ApiStyle::OpenAiChat,
                supports_tools: false,
                supports_thinking: false,
            },
            ModelInfo {
                id: "provider/kimi-k2.7".to_string(),
                label: "kimi-k2.7".to_string(),
                api_style: ApiStyle::OpenAiTools,
                supports_tools: true,
                supports_thinking: true,
            },
        ];
        let best = ModelQualityIndex::recommend_model_for_task(
            AgentTaskKind::Refactor,
            AiProvider::CloudflareWorkersAi,
            &models,
        )
        .unwrap();
        assert_eq!(best.label, "kimi-k2.7");
    }

    #[test]
    fn should_switch_provider_for_unreliable_tools() {
        // Perplexity has native_tools_reliable = false
        let suggestion = ModelQualityIndex::should_switch_provider(
            AgentTaskKind::Refactor, // needs tools
            AiProvider::Perplexity,
        );
        assert!(suggestion.is_some());
    }

    #[test]
    fn should_switch_provider_none_for_good_provider() {
        // Cloudflare has reliable tools and good parallelism
        let suggestion = ModelQualityIndex::should_switch_provider(
            AgentTaskKind::Refactor,
            AiProvider::CloudflareWorkersAi,
        );
        assert!(suggestion.is_none());
    }

    #[test]
    fn provider_capabilities_count() {
        let caps = ModelQualityIndex::provider_capabilities();
        assert_eq!(caps.len(), 16);
    }

    #[test]
    fn score_model_penalizes_no_tools_for_refactor() {
        let req = TaskRequirements::for_kind(AgentTaskKind::Refactor);
        let caps = ProviderCapability {
            provider: AiProvider::CloudflareWorkersAi,
            native_tools_reliable: true,
            good_for_parallelism: true,
        };
        let no_tools = ModelInfo {
            id: "basic".to_string(),
            label: "basic".to_string(),
            api_style: ApiStyle::OpenAiChat,
            supports_tools: false,
            supports_thinking: false,
        };
        let with_tools = ModelInfo {
            id: "advanced".to_string(),
            label: "advanced".to_string(),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: true,
        };
        let score_no = ModelQualityIndex::score_model(&no_tools, req, caps);
        let score_yes = ModelQualityIndex::score_model(&with_tools, req, caps);
        assert!(score_yes > score_no);
    }

    // ---- model-selection decision matrix --------------------------------- //

    use crate::agent::decision::{DecisionError, DecisionOutput};
    use std::collections::HashMap;

    /// A controllable brain: answers the single `model` choice with a fixed
    /// label + confidence, or reports a transport failure. Lets us pin the
    /// safety guards without any network.
    struct FakeModel {
        answer: Option<(String, f32)>,
        backend_name: &'static str,
    }
    impl DecisionModel for FakeModel {
        fn name(&self) -> &str {
            self.backend_name
        }
        fn decide(
            &self,
            _state: &DecisionState,
            questions: &[Question],
        ) -> Result<DecisionOutput, DecisionError> {
            match &self.answer {
                None => Err(DecisionError::Transport("fake down".to_string())),
                Some((sel, conf)) => {
                    let id = questions[0].id().to_string();
                    let mut answers = HashMap::new();
                    answers.insert(
                        id.clone(),
                        Answer::Choice {
                            id,
                            selected: sel.clone(),
                            probabilities: vec![],
                            confidence: *conf,
                        },
                    );
                    Ok(DecisionOutput {
                        answers,
                        backend: self.backend_name.to_string(),
                    })
                }
            }
        }
    }

    fn fake(sel: &str, conf: f32) -> FakeModel {
        FakeModel {
            answer: Some((sel.to_string(), conf)),
            backend_name: "laya",
        }
    }

    fn usable3() -> Vec<ModelInfo> {
        let mk = |id: &str, think: bool| ModelInfo {
            id: id.to_string(),
            label: id.to_string(),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: think,
        };
        vec![
            mk("qwen3-coder", false),
            mk("deepseek-v3", true),
            mk("kimi-k2", true),
        ]
    }

    #[test]
    fn model_choice_is_adopted_when_confident_and_usable() {
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &usable3(),
            "refactor a coupled module",
            Some(&fake("kimi-k2", 0.85)),
        );
        assert_eq!(sel.backend, "laya");
        assert_eq!(sel.candidate.model_id, "kimi-k2");
        assert!((sel.confidence - 0.85).abs() < 1e-6);
    }

    #[test]
    fn a_hallucinated_disabled_model_is_rejected_and_we_fall_back() {
        // "gpt-9" was never offered (not an enabled/usable model): the guard
        // must discard it and use the deterministic top pick instead.
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &usable3(),
            "refactor a coupled module",
            Some(&fake("gpt-9-not-enabled", 0.99)),
        );
        assert_eq!(sel.backend, "heuristic");
        assert_eq!(sel.candidate.model_id, "deepseek-v3"); // deterministic top
    }

    #[test]
    fn a_low_confidence_choice_defers_to_the_ranking() {
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &usable3(),
            "refactor a coupled module",
            Some(&fake("kimi-k2", 0.20)), // below the 0.40 floor
        );
        assert_eq!(sel.backend, "heuristic");
        assert_eq!(sel.candidate.model_id, "deepseek-v3");
    }

    /// Same knob as FakeModel but also emits a per-option probability map, so
    /// we can pin that the *option probability* — not the model's very
    /// conservative self-reported confidence — drives adoption.
    struct ProbsModel {
        selected: &'static str,
        probabilities: Vec<(String, f32)>,
        confidence: f32,
    }
    impl DecisionModel for ProbsModel {
        fn name(&self) -> &str {
            "laya"
        }
        fn decide(
            &self,
            _state: &DecisionState,
            questions: &[Question],
        ) -> Result<DecisionOutput, DecisionError> {
            let id = questions[0].id().to_string();
            let mut answers = HashMap::new();
            answers.insert(
                id.clone(),
                Answer::Choice {
                    id,
                    selected: self.selected.to_string(),
                    probabilities: self.probabilities.clone(),
                    confidence: self.confidence,
                },
            );
            Ok(DecisionOutput {
                answers,
                backend: "laya".to_string(),
            })
        }
    }

    #[test]
    fn a_low_self_confidence_but_clear_option_probability_is_adopted() {
        // Reproduces the real Laya calibration: self-confidence 0.13 (far below
        // any floor) yet kimi-k2 is the argmax at 0.42 >= 0.35 — and usable.
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &usable3(),
            "refactor a coupled module",
            Some(&ProbsModel {
                selected: "kimi-k2",
                probabilities: vec![
                    ("qwen3-coder".into(), 0.37),
                    ("deepseek-v3".into(), 0.21),
                    ("kimi-k2".into(), 0.42),
                ],
                confidence: 0.13,
            }),
        );
        assert_eq!(sel.backend, "laya");
        assert_eq!(sel.candidate.model_id, "kimi-k2");
        assert!(sel.rationale.contains("option probability 0.42, bar 0.35"));
    }

    #[test]
    fn a_flat_option_probability_distribution_defers_to_the_ranking() {
        // No option stands out (all below the floor): deterministic ranking wins.
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &usable3(),
            "refactor a coupled module",
            Some(&ProbsModel {
                selected: "kimi-k2",
                probabilities: vec![
                    ("qwen3-coder".into(), 0.34),
                    ("deepseek-v3".into(), 0.33),
                    ("kimi-k2".into(), 0.33),
                ],
                confidence: 0.90,
            }),
        );
        assert_eq!(sel.backend, "heuristic");
        assert_eq!(sel.candidate.model_id, "deepseek-v3");
    }

    fn ten_catalog() -> Vec<ModelInfo> {
        [
            "deepseek-v4-flash-0731",
            "deepseek-v4-pro",
            "deepseek-v4.1-flash",
            "glm-5.2",
            "glm-5.3",
            "qwen3.6-flash",
            "qwen3.7-max",
            "qwen3.7-plus",
            "qwen3.8-flash",
            "qwen3.8-max",
        ]
        .iter()
        .map(|id| ModelInfo {
            id: id.to_string(),
            label: id.to_string(),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: true,
        })
        .collect()
    }

    #[test]
    fn a_clear_argmax_over_a_large_catalog_meets_the_scaled_floor() {
        // 10-model token plan, but this time the winner leads the runner-up by
        // more than the 5pp margin: the scaled bar (0.18 for n=10) admits a
        // pick a flat 0.35 floor would have silenced on every big catalog.
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &ten_catalog(),
            "refactor a tightly coupled module across services",
            Some(&ProbsModel {
                selected: "qwen3.7-max",
                probabilities: vec![
                    ("deepseek-v4-flash-0731".into(), 0.090),
                    ("deepseek-v4-pro".into(), 0.048),
                    ("deepseek-v4.1-flash".into(), 0.134),
                    ("glm-5.2".into(), 0.098),
                    ("glm-5.3".into(), 0.064),
                    ("qwen3.6-flash".into(), 0.120),
                    ("qwen3.7-max".into(), 0.192),
                    ("qwen3.7-plus".into(), 0.030),
                    ("qwen3.8-flash".into(), 0.135),
                    ("qwen3.8-max".into(), 0.050),
                ],
                confidence: 0.103,
            }),
        );
        assert!((model_choice_probability_floor(10) - 0.18).abs() < 1e-6);
        assert_eq!(sel.backend, "laya", "rationale={}", sel.rationale);
        assert_eq!(sel.candidate.model_id, "qwen3.7-max");
    }

    #[test]
    fn the_observed_real_token_plan_distribution_is_adopted() {
        // Verbatim 10-model Alibaba token-plan answer captured from the live
        // box: qwen3.7-max at 0.192 (self-confidence only 0.103) clears the
        // scaled 0.18 floor and leads the 0.159 runner-up by 3.3pp — over the
        // 3pp margin, so the guard adopts the reflex model's pick.
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &ten_catalog(),
            "refactor a tightly coupled module across services",
            Some(&ProbsModel {
                selected: "qwen3.7-max",
                probabilities: vec![
                    ("deepseek-v4-flash-0731".into(), 0.090),
                    ("deepseek-v4-pro".into(), 0.048),
                    ("deepseek-v4.1-flash".into(), 0.134),
                    ("glm-5.2".into(), 0.098),
                    ("glm-5.3".into(), 0.064),
                    ("qwen3.6-flash".into(), 0.159),
                    ("qwen3.7-max".into(), 0.192),
                    ("qwen3.7-plus".into(), 0.030),
                    ("qwen3.8-flash".into(), 0.135),
                    ("qwen3.8-max".into(), 0.050),
                ],
                confidence: 0.103,
            }),
        );
        assert_eq!(sel.backend, "laya");
        assert_eq!(sel.candidate.model_id, "qwen3.7-max");
    }

    #[test]
    fn a_photo_finish_in_a_large_catalog_defers_to_the_ranking() {
        // Same pool, but the argmax barely leads (0.18 vs 0.17): a photo-finish
        // between look-alikes is not enough to override the deterministic pick.
        let ten: Vec<ModelInfo> = (1..=10)
            .map(|i| ModelInfo {
                id: format!("model-{i}"),
                label: format!("model-{i}"),
                api_style: ApiStyle::OpenAiTools,
                supports_tools: true,
                supports_thinking: true,
            })
            .collect();
        let probs: Vec<(String, f32)> = (1..=10)
            .map(|i| (format!("model-{i}"), 0.17f32 + 0.001 * i as f32))
            .collect();
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &ten,
            "refactor a coupled module",
            Some(&ProbsModel {
                selected: "model-10",
                probabilities: probs,
                confidence: 0.30,
            }),
        );
        assert_eq!(sel.backend, "heuristic");
    }

    #[test]
    fn transport_failure_falls_back_to_heuristic() {
        let down = FakeModel {
            answer: None,
            backend_name: "laya",
        };
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &usable3(),
            "refactor a coupled module",
            Some(&down),
        );
        assert_eq!(sel.backend, "heuristic");
        assert_eq!(sel.candidate.model_id, "deepseek-v3");
    }

    #[test]
    fn no_model_wired_up_uses_pure_deterministic_ranking() {
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &usable3(),
            "refactor a coupled module",
            None,
        );
        assert_eq!(sel.backend, "heuristic");
        assert_eq!(sel.candidate.model_id, "deepseek-v3");
    }

    #[test]
    fn empty_usable_set_yields_an_empty_selection() {
        let sel = ModelQualityIndex::decide_model_with(
            AgentTaskKind::Refactor,
            AiProvider::AlibabaQwen,
            &[],
            "refactor a coupled module",
            Some(&fake("kimi-k2", 0.9)),
        );
        assert_eq!(sel.candidate.model_id, "");
        assert_eq!(sel.backend, "heuristic");
    }
}
