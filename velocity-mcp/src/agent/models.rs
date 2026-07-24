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
            "vertex" | "googlevertex" | "google_vertex" | "google" => Some(AiProvider::GoogleVertex),
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
