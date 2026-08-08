#![allow(dead_code)]

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

pub struct ModelQualityIndex;

impl ModelQualityIndex {
    pub fn provider_capabilities() -> [ProviderCapability; 13] {
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
                provider: AiProvider::Deepseek,
                native_tools_reliable: true,
                good_for_parallelism: false,
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
}
