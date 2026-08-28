use crossbeam_channel::Receiver;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub enum UiToAgentMessage {
    SetWorkspace(PathBuf),
    RefreshModels,
    RefreshUsage,
    ReloadProviderConfig,
    ApplySessionState {
        provider: AiProvider,
        model: String,
        thinking: bool,
    },
    SetModel(String),
    SetThinking(bool),
    SetProvider(AiProvider),
    UserPrompt(String),
    ClearHistory,
    ApproveTool {
        id: String,
        arguments: Value,
    },
    RejectTool {
        id: String,
    },
    RunLocalBuild,
    RunLocalRun,
    CancelTask,
    ReloadTeams,
    /// Fetch panel data for MCP/fetch tools (panel slug, optional params)
    FetchPanelData {
        panel: String,
    },
}

#[derive(Debug, Clone)]
pub enum AgentToUiMessage {
    #[allow(dead_code)]
    ThoughtToken(String),
    OutputToken(String),
    RequestToolApproval {
        id: String,
        tool_name: String,
        arguments: Value,
    },
    ToolExecutionStarted {
        tool_name: String,
    },
    ToolExecutionFinished {
        tool_name: String,
        result: String,
    },
    StatusUpdate(String),
    AgentFinished,
    UpdateFileBuffer {
        path: PathBuf,
        content: String,
    },
    ModelCatalog {
        models: Vec<ModelInfo>,
        selected: String,
        thinking: bool,
    },
    AccountUsage {
        accounts: Vec<crate::usage::AccountUsageView>,
        date: String,
    },
    ChatHistoryRestored(Vec<(String, String)>),
    ProviderChanged(AiProvider),
    /// Panel data response for MCP/fetch tools
    PanelData {
        panel: String,
        data: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadlessSubAgentEventKind {
    Status,
    Transcript,
    FileChange,
    OperatorNote,
    ToolApproval,
    ToolStarted,
    ToolFinished,
}

#[derive(Debug, Clone)]
pub struct HeadlessSubAgentEvent {
    pub kind: HeadlessSubAgentEventKind,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct HeadlessSubAgentProgress {
    pub events: Vec<HeadlessSubAgentEvent>,
    pub status_updates: Vec<String>,
    pub transcript: String,
    pub changed_files: Vec<String>,
    pub operator_notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HeadlessSubAgentRequest {
    pub workspace_root: PathBuf,
    pub provider: AiProvider,
    pub model: String,
    pub thinking: bool,
    pub prompt: String,
    pub cancel_rx: Option<Receiver<UiToAgentMessage>>,
    pub progress: Option<Arc<Mutex<HeadlessSubAgentProgress>>>,
    /// Optional list of files to pre-index for speculative pre-computation.
    pub scoped_files: Option<Vec<PathBuf>>,
}

#[derive(Debug, Clone, Default)]
pub struct HeadlessSubAgentResult {
    pub status_updates: Vec<String>,
    pub transcript: String,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub api_style: ApiStyle,
    pub supports_tools: bool,
    pub supports_thinking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiStyle {
    OpenAiTools,
    OpenAiChat,
    PromptCompletion,
}

/// Which AI backend to use for inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AiProvider {
    CloudflareWorkersAi,
    OpenRouter,
    AzureOpenAi,
    LocalOllama,
    OpenAI,
    Anthropic,
    GoogleVertex,
    /// Deepseek API (OpenAI-compatible endpoint).
    Deepseek,
    /// Alibaba Cloud Qwen (DashScope API, OpenAI-compatible).
    AlibabaQwen,
    /// AWS Bedrock (via OpenAI-compatible proxy or native API).
    AwsBedrock,
    /// Groq (OpenAI-compatible endpoint for LPU inference).
    Groq,
    /// Mistral AI (La Plateforme, OpenAI-compatible endpoint).
    Mistral,
    /// Together AI (OpenAI-compatible endpoint).
    TogetherAi,
    /// Fireworks AI (OpenAI-compatible endpoint).
    FireworksAi,
    /// Perplexity API (OpenAI-compatible endpoint).
    Perplexity,
    /// Cerebras (OpenAI-compatible endpoint for wafer-scale inference).
    Cerebras,
}

impl AiProvider {
    pub fn label(self) -> &'static str {
        match self {
            AiProvider::CloudflareWorkersAi => "Cloudflare Workers AI",
            AiProvider::OpenRouter => "OpenRouter",
            AiProvider::AzureOpenAi => "Azure OpenAI",
            AiProvider::LocalOllama => "Local Ollama",
            AiProvider::OpenAI => "OpenAI Direct",
            AiProvider::Anthropic => "Anthropic Claude",
            AiProvider::GoogleVertex => "Google Vertex AI",
            AiProvider::Deepseek => "Deepseek",
            AiProvider::AlibabaQwen => "Alibaba Qwen",
            AiProvider::AwsBedrock => "AWS Bedrock",
            AiProvider::Groq => "Groq",
            AiProvider::Mistral => "Mistral AI",
            AiProvider::TogetherAi => "Together AI",
            AiProvider::FireworksAi => "Fireworks AI",
            AiProvider::Perplexity => "Perplexity",
            AiProvider::Cerebras => "Cerebras",
        }
    }

    /// Stable machine slug used for NDA serialization and tool arguments.
    pub fn slug(self) -> &'static str {
        match self {
            AiProvider::CloudflareWorkersAi => "cloudflare",
            AiProvider::OpenRouter => "openrouter",
            AiProvider::AzureOpenAi => "azure",
            AiProvider::LocalOllama => "ollama",
            AiProvider::OpenAI => "openai",
            AiProvider::Anthropic => "anthropic",
            AiProvider::GoogleVertex => "vertex",
            AiProvider::Deepseek => "deepseek",
            AiProvider::AlibabaQwen => "alibaba",
            AiProvider::AwsBedrock => "bedrock",
            AiProvider::Groq => "groq",
            AiProvider::Mistral => "mistral",
            AiProvider::TogetherAi => "together",
            AiProvider::FireworksAi => "fireworks",
            AiProvider::Perplexity => "perplexity",
            AiProvider::Cerebras => "cerebras",
        }
    }

    /// Parse a provider from a slug or common alias. Case-insensitive.
    pub fn from_slug(value: &str) -> Option<AiProvider> {
        match value.trim().to_lowercase().as_str() {
            "cloudflare" | "cloudflareworkersai" | "cf" | "workers-ai" => {
                Some(AiProvider::CloudflareWorkersAi)
            }
            "openrouter" | "or" => Some(AiProvider::OpenRouter),
            "azure" | "azure_openai" | "azureopenai" => Some(AiProvider::AzureOpenAi),
            "ollama" | "local" | "localollama" => Some(AiProvider::LocalOllama),
            "openai" | "openai_direct" => Some(AiProvider::OpenAI),
            "anthropic" | "claude" => Some(AiProvider::Anthropic),
            "vertex" | "googlevertex" | "google_vertex" | "google" => {
                Some(AiProvider::GoogleVertex)
            }
            "deepseek" => Some(AiProvider::Deepseek),
            "alibaba" | "qwen" | "dashscope" | "alibaba_qwen" => Some(AiProvider::AlibabaQwen),
            "bedrock" | "awsbedrock" | "aws_bedrock" | "aws" => Some(AiProvider::AwsBedrock),
            "groq" => Some(AiProvider::Groq),
            "mistral" | "mistralai" | "mistral_ai" | "laplateforme" => Some(AiProvider::Mistral),
            "together" | "togetherai" | "together_ai" => Some(AiProvider::TogetherAi),
            "fireworks" | "fireworksai" | "fireworks_ai" => Some(AiProvider::FireworksAi),
            "perplexity" | "pplx" => Some(AiProvider::Perplexity),
            "cerebras" => Some(AiProvider::Cerebras),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
}

pub struct ToolCallAccumulator {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_provider_labels() {
        assert_eq!(AiProvider::Deepseek.label(), "Deepseek");
        assert_eq!(AiProvider::AlibabaQwen.label(), "Alibaba Qwen");
        assert_eq!(AiProvider::AwsBedrock.label(), "AWS Bedrock");
        assert_eq!(AiProvider::Groq.label(), "Groq");
        assert_eq!(AiProvider::Mistral.label(), "Mistral AI");
    }

    #[test]
    fn new_provider_slugs() {
        assert_eq!(AiProvider::Deepseek.slug(), "deepseek");
        assert_eq!(AiProvider::AlibabaQwen.slug(), "alibaba");
        assert_eq!(AiProvider::AwsBedrock.slug(), "bedrock");
        assert_eq!(AiProvider::Groq.slug(), "groq");
        assert_eq!(AiProvider::Mistral.slug(), "mistral");
    }

    #[test]
    fn new_provider_from_slug_roundtrip() {
        assert_eq!(
            AiProvider::from_slug("deepseek"),
            Some(AiProvider::Deepseek)
        );
        assert_eq!(
            AiProvider::from_slug("alibaba"),
            Some(AiProvider::AlibabaQwen)
        );
        assert_eq!(AiProvider::from_slug("qwen"), Some(AiProvider::AlibabaQwen));
        assert_eq!(
            AiProvider::from_slug("dashscope"),
            Some(AiProvider::AlibabaQwen)
        );
        assert_eq!(
            AiProvider::from_slug("bedrock"),
            Some(AiProvider::AwsBedrock)
        );
        assert_eq!(AiProvider::from_slug("aws"), Some(AiProvider::AwsBedrock));
        assert_eq!(AiProvider::from_slug("groq"), Some(AiProvider::Groq));
        assert_eq!(AiProvider::from_slug("mistral"), Some(AiProvider::Mistral));
        assert_eq!(
            AiProvider::from_slug("mistralai"),
            Some(AiProvider::Mistral)
        );
    }

    #[test]
    fn new_provider_from_slug_case_insensitive() {
        assert_eq!(
            AiProvider::from_slug("DEEPSEEK"),
            Some(AiProvider::Deepseek)
        );
        assert_eq!(AiProvider::from_slug("Groq"), Some(AiProvider::Groq));
        assert_eq!(AiProvider::from_slug("MISTRAL"), Some(AiProvider::Mistral));
    }

    #[test]
    fn unknown_slug_returns_none() {
        assert_eq!(AiProvider::from_slug("nonexistent"), None);
        assert_eq!(AiProvider::from_slug(""), None);
    }

    #[test]
    fn all_providers_have_unique_slugs() {
        let providers = vec![
            AiProvider::CloudflareWorkersAi,
            AiProvider::OpenRouter,
            AiProvider::AzureOpenAi,
            AiProvider::LocalOllama,
            AiProvider::OpenAI,
            AiProvider::Anthropic,
            AiProvider::GoogleVertex,
            AiProvider::Deepseek,
            AiProvider::AlibabaQwen,
            AiProvider::AwsBedrock,
            AiProvider::Groq,
            AiProvider::Mistral,
            AiProvider::TogetherAi,
            AiProvider::FireworksAi,
            AiProvider::Perplexity,
            AiProvider::Cerebras,
        ];
        let mut slugs = std::collections::HashSet::new();
        for p in &providers {
            assert!(slugs.insert(p.slug()), "duplicate slug: {}", p.slug());
        }
        assert_eq!(slugs.len(), providers.len());
    }

    #[test]
    fn all_providers_have_unique_labels() {
        let providers = vec![
            AiProvider::CloudflareWorkersAi,
            AiProvider::OpenRouter,
            AiProvider::AzureOpenAi,
            AiProvider::LocalOllama,
            AiProvider::OpenAI,
            AiProvider::Anthropic,
            AiProvider::GoogleVertex,
            AiProvider::Deepseek,
            AiProvider::AlibabaQwen,
            AiProvider::AwsBedrock,
            AiProvider::Groq,
            AiProvider::Mistral,
            AiProvider::TogetherAi,
            AiProvider::FireworksAi,
            AiProvider::Perplexity,
            AiProvider::Cerebras,
        ];
        let mut labels = std::collections::HashSet::new();
        for p in &providers {
            assert!(labels.insert(p.label()), "duplicate label: {}", p.label());
        }
    }

    #[test]
    fn slug_from_slug_roundtrip_for_all_providers() {
        let providers = vec![
            AiProvider::CloudflareWorkersAi,
            AiProvider::OpenRouter,
            AiProvider::AzureOpenAi,
            AiProvider::LocalOllama,
            AiProvider::OpenAI,
            AiProvider::Anthropic,
            AiProvider::GoogleVertex,
            AiProvider::Deepseek,
            AiProvider::AlibabaQwen,
            AiProvider::AwsBedrock,
            AiProvider::Groq,
            AiProvider::Mistral,
            AiProvider::TogetherAi,
            AiProvider::FireworksAi,
            AiProvider::Perplexity,
            AiProvider::Cerebras,
        ];
        for p in &providers {
            let slug = p.slug();
            let parsed = AiProvider::from_slug(slug);
            assert_eq!(parsed, Some(*p), "from_slug('{}') should return {:?}", slug, p);
        }
    }

    #[test]
    fn provider_count_is_16() {
        let providers = vec![
            AiProvider::CloudflareWorkersAi,
            AiProvider::OpenRouter,
            AiProvider::AzureOpenAi,
            AiProvider::LocalOllama,
            AiProvider::OpenAI,
            AiProvider::Anthropic,
            AiProvider::GoogleVertex,
            AiProvider::Deepseek,
            AiProvider::AlibabaQwen,
            AiProvider::AwsBedrock,
            AiProvider::Groq,
            AiProvider::Mistral,
            AiProvider::TogetherAi,
            AiProvider::FireworksAi,
            AiProvider::Perplexity,
            AiProvider::Cerebras,
        ];
        assert_eq!(providers.len(), 16, "Expected 16 providers");
    }

    #[test]
    fn all_provider_aliases_resolve() {
        // Cloudflare aliases
        assert_eq!(AiProvider::from_slug("cf"), Some(AiProvider::CloudflareWorkersAi));
        assert_eq!(AiProvider::from_slug("workers-ai"), Some(AiProvider::CloudflareWorkersAi));
        // OpenRouter aliases
        assert_eq!(AiProvider::from_slug("or"), Some(AiProvider::OpenRouter));
        // Azure aliases
        assert_eq!(AiProvider::from_slug("azure_openai"), Some(AiProvider::AzureOpenAi));
        // Ollama aliases
        assert_eq!(AiProvider::from_slug("local"), Some(AiProvider::LocalOllama));
        // Anthropic aliases
        assert_eq!(AiProvider::from_slug("claude"), Some(AiProvider::Anthropic));
        // Vertex aliases
        assert_eq!(AiProvider::from_slug("google"), Some(AiProvider::GoogleVertex));
        assert_eq!(AiProvider::from_slug("google_vertex"), Some(AiProvider::GoogleVertex));
        // Alibaba aliases
        assert_eq!(AiProvider::from_slug("qwen"), Some(AiProvider::AlibabaQwen));
        assert_eq!(AiProvider::from_slug("dashscope"), Some(AiProvider::AlibabaQwen));
        // Bedrock aliases
        assert_eq!(AiProvider::from_slug("aws"), Some(AiProvider::AwsBedrock));
        // Perplexity alias
        assert_eq!(AiProvider::from_slug("pplx"), Some(AiProvider::Perplexity));
    }
}
