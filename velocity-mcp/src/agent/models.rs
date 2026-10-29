use crate::registry::ToolSurface;
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

/// What a run has paid for replies that streamed and never named a tool call.
///
/// The loop's cut allowance is a ceiling on prose, and prose is priced by the
/// route: the measured spans for one cut turn were 901, 907, 919, 926 and 928
/// seconds on the same model. A ceiling held per *attempt* is therefore re-paid
/// by every model a fallback chain tries — run #26 spent 5,500s of its 6,025s on
/// six cut turns across three copies of one model and wrote nothing. Carrying the
/// tally across the chain makes the allowance a property of the dispatch, and
/// keeping the seconds makes the cost readable by a caller that cannot guess it
/// from status prose.
///
/// A hop receives the ledger as it stood and returns it with its own turns added,
/// so the number a caller reads after a hop is the dispatch total, not the hop's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RambleLedger {
    /// Cut turns paid for so far.
    pub cuts: u32,
    /// Wall-clock seconds those turns streamed.
    pub secs: u64,
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
    /// Maximum reasoning turns. None = use default (15).
    pub max_turns: Option<usize>,
    /// Wall-clock budget for the whole run. The loop checks it at every turn
    /// boundary and stops as [`AgentLoopOutcome::TimedOut`] rather than running
    /// on forever. `None` keeps the old unbounded behaviour, which is right for
    /// an interactive session a human can cancel but wrong for a dispatch that
    /// nobody is watching: a LiteLLM porting attempt once ran 21 minutes past
    /// the caller's own 300 s budget and had to be killed by hand.
    pub budget: Option<std::time::Duration>,
    /// Whether this task's promised deliverable is files the agent must write
    /// (the `output_format_block("artifact")` contract: "a response with no
    /// writes is a failed task"). When true, the loop refuses to report a clean
    /// `Completed` final answer if it never produced a file — it steers the
    /// model back to the write tool, and stops as `Blocked` if that fails, so an
    /// empty deliverable is never a silent success. `false` leaves the text
    /// answer as a valid ending (reports, chat, read-only analysis).
    pub requires_artifact: bool,
    /// The tool groups this run advertises. The registry has 347 tools whose
    /// schemas render to ~68,700 tokens — three and a half times the window a
    /// local turn declares — so a dispatched worker ships only the families its
    /// use case needs ([`ToolSurface::for_task_kind`]) and gains the rest with one
    /// `request_tool_groups` call. A caller that has not decided gets the
    /// interactive surface rather than the whole catalog.
    pub tool_surface: ToolSurface,
    /// What the dispatch this hop belongs to has already spent on rambling
    /// replies. A single hop passes a fresh ledger; a fallback chain passes the
    /// one every hop before it left, which is what turns the loop's cut allowance
    /// from a per-model budget into a per-run one. See [`RambleLedger`].
    pub ramble: RambleLedger,
}

/// Why the agent reasoning loop stopped, as a structured signal.
///
/// The loop used to return `()`, so every caller had to guess success by
/// scanning the transcript for the word `Error:` — and a *successful* porting
/// run quotes cargo output, so it contains that word too. Guessing this way
/// threw away a completed, compiler-validated LiteLLM port: `team_dispatch`
/// scored the winning attempt failed and re-ran the whole task on two models
/// that 404, then reported "no model succeeded".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentLoopOutcome {
    /// The model delivered a final answer, or post-run validation passed.
    Completed,
    /// The loop stopped for a reason that means the mission is not done, but a
    /// different model on the same provider could still succeed (a rejected
    /// model name, an exhausted turn budget, unrepairable tool markup).
    Blocked { reason: String },
    /// No provider could be reached at all: missing credentials, an account
    /// list that is empty, or a transport failure that outlived its retries.
    /// Trying another *model* on the same provider cannot help, so callers
    /// skip straight to the next provider.
    ///
    /// This is a variant rather than a `Blocked` reason prefix because the
    /// dispatch failover branches on it, and branching on `reason().starts_with`
    /// is the same prose-guessing defect that made `is_completed` necessary.
    ProviderUnavailable { reason: String },
    /// The wall-clock budget ran out mid-mission. Partial work is on disk and
    /// the transcript is real, so callers should report what exists instead of
    /// re-running the whole task on a weaker model with no time left.
    TimedOut {
        elapsed_secs: u64,
        budget_secs: u64,
        turns: usize,
    },
    /// An operator — or an orchestrator watchdog acting for one — stopped the
    /// run. Not a model failure and not worth a failover: the next provider
    /// would be doing work a human just asked to stop. It gets its own variant
    /// because the orchestrator worker used to detect it by scanning status
    /// updates for the substring "cancelled by operator", which is the same
    /// prose-guessing defect `is_completed` was introduced to remove.
    Cancelled,
}

impl AgentLoopOutcome {
    pub fn is_completed(&self) -> bool {
        matches!(self, AgentLoopOutcome::Completed)
    }

    /// Whether another model on the *same* provider is worth an attempt.
    pub fn is_provider_unavailable(&self) -> bool {
        matches!(self, AgentLoopOutcome::ProviderUnavailable { .. })
    }

    /// Whether the run stopped because its wall-clock budget was spent. Such a
    /// run must never trigger a failover: the budget that ran out is shared by
    /// every remaining candidate, so a retry only burns time it does not have.
    pub fn is_timed_out(&self) -> bool {
        matches!(self, AgentLoopOutcome::TimedOut { .. })
    }

    /// Whether a human (or a watchdog acting for one) stopped the run. Such a
    /// run must never trigger a failover, and must never be reported as a
    /// model or provider failure.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, AgentLoopOutcome::Cancelled)
    }

    /// Whether the loop ended on a *restartable* dead end — a stream stall, a
    /// repeated refusal/error, or a run that only narrated without acting. The
    /// orchestrator worker re-issues such an attempt from its continuation
    /// ledger, because a fresh context sometimes breaks a loop a wall-clock
    /// timeout would merely have stopped. A `Cancelled` or `TimedOut` (backstop
    /// spent) run is deliberately not restartable.
    pub fn is_blocked(&self) -> bool {
        matches!(self, AgentLoopOutcome::Blocked { .. })
    }

    /// Why a run stopped, for logs and the dispatch result. Empty when the
    /// mission finished normally.
    pub fn reason(&self) -> String {
        match self {
            AgentLoopOutcome::Completed => String::new(),
            AgentLoopOutcome::Blocked { reason } => reason.clone(),
            AgentLoopOutcome::ProviderUnavailable { reason } => {
                format!("provider unavailable: {reason}")
            }
            AgentLoopOutcome::TimedOut {
                elapsed_secs,
                budget_secs,
                turns,
            } => format!(
                "wall-clock budget of {budget_secs}s spent after {turns} turn(s) ({elapsed_secs}s elapsed)"
            ),
            AgentLoopOutcome::Cancelled => "cancelled by operator".to_string(),
        }
    }
}

impl Default for AgentLoopOutcome {
    /// Deliberately not `Completed`: an outcome nobody reported must never be
    /// read as success by a caller that only looks at `is_completed`.
    fn default() -> Self {
        AgentLoopOutcome::Blocked {
            reason: "loop reported no outcome".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HeadlessSubAgentResult {
    pub status_updates: Vec<String>,
    pub transcript: String,
    /// Whether the sub-agent actually finished its mission. Read this instead
    /// of pattern-matching the transcript.
    pub outcome: AgentLoopOutcome,
    /// The [`HeadlessSubAgentRequest::ramble`] ledger with this hop's cut turns
    /// added — the dispatch total once a chain has threaded it through.
    pub ramble: RambleLedger,
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
/// Serialize keeps variant names; Deserialize is handwritten (below) so
/// persisted files, UI labels, and hand-written LLM team JSON all parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum AiProvider {
    CloudflareWorkersAi,
    OpenRouter,
    AzureOpenAi,
    LocalOllama,
    /// A locally-run llama.cpp `llama-server` (OpenAI-compatible endpoint, no
    /// key). Reached with 3-tier MoE offload so a model far larger than VRAM
    /// still runs, trading throughput for an effectively unbounded context.
    LocalLlama,
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
            AiProvider::LocalLlama => "Local llama.cpp",
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
            AiProvider::LocalLlama => "llamacpp",
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

    /// Parse a provider from the display label produced by [`AiProvider::label`]
    /// (e.g. when restoring workspace preferences). Falls back to slug parsing
    /// so older or machine-formatted values still resolve. Every `label()`
    /// arm must have a mirror here — the preferences round-trip used to cover
    /// only 4 of the 16 providers, silently reverting the rest on restart.
    pub fn from_label(value: &str) -> Option<AiProvider> {
        let v = value.trim();
        let exact = match v {
            "Cloudflare Workers AI" => Some(AiProvider::CloudflareWorkersAi),
            "OpenRouter" => Some(AiProvider::OpenRouter),
            "Azure OpenAI" => Some(AiProvider::AzureOpenAi),
            "Local Ollama" => Some(AiProvider::LocalOllama),
            "Local llama.cpp" => Some(AiProvider::LocalLlama),
            "OpenAI Direct" => Some(AiProvider::OpenAI),
            "Anthropic Claude" => Some(AiProvider::Anthropic),
            "Google Vertex AI" => Some(AiProvider::GoogleVertex),
            "Deepseek" => Some(AiProvider::Deepseek),
            "Alibaba Qwen" => Some(AiProvider::AlibabaQwen),
            "AWS Bedrock" => Some(AiProvider::AwsBedrock),
            "Groq" => Some(AiProvider::Groq),
            "Mistral AI" => Some(AiProvider::Mistral),
            "Together AI" => Some(AiProvider::TogetherAi),
            "Fireworks AI" => Some(AiProvider::FireworksAi),
            "Perplexity" => Some(AiProvider::Perplexity),
            "Cerebras" => Some(AiProvider::Cerebras),
            _ => None,
        };
        exact.or_else(|| Self::from_slug(v))
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
            "llamacpp" | "llama" | "localllama" | "llamaserver" => Some(AiProvider::LocalLlama),
            "openai" | "openai_direct" => Some(AiProvider::OpenAI),
            "anthropic" | "claude" => Some(AiProvider::Anthropic),
            "vertex" | "googlevertex" | "google_vertex" | "google" => {
                Some(AiProvider::GoogleVertex)
            }
            "deepseek" => Some(AiProvider::Deepseek),
            "alibaba" | "qwen" | "dashscope" | "alibaba_qwen" | "alibabaqwen" => {
                Some(AiProvider::AlibabaQwen)
            }
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

/// Lenient deserializer: accepts the serde variant name (`"AlibabaQwen"`),
/// the UI display label (`"Alibaba Qwen"`), or any slug/alias (`"alibaba"`,
/// `"qwen"`). The derived variant-name-only form made `import_expert_team`
/// reject exactly the JSON shape its sibling tools (`create_expert_team`,
/// `bulk_import_members`) produce and accept. Serialization still emits
/// variant names, so persisted formats are unchanged.
impl<'de> serde::Deserialize<'de> for AiProvider {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        let raw = <String as serde::Deserialize>::deserialize(deserializer)?;
        if let Some(p) = Self::from_label(&raw) {
            return Ok(p);
        }
        // "AWS Bedrock", "aws-bedrock", "AwsBedrock" → "awsbedrock"
        let compact: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect();
        if let Some(p) = Self::from_slug(&compact) {
            return Ok(p);
        }
        Err(D::Error::custom(format!(
            "unknown AI provider {raw:?}; use a variant name (e.g. \"AlibabaQwen\"), \
             a display label (\"Alibaba Qwen\"), or a slug (\"alibaba\")"
        )))
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

    /// An outcome nobody reported must never read as success: `team_dispatch`
    /// fails over on `is_completed`, so a default that claimed completion would
    /// quietly accept a run that never answered.
    #[test]
    fn unreported_loop_outcome_defaults_to_blocked() {
        let unreported = AgentLoopOutcome::default();
        assert!(!unreported.is_completed());
        assert!(!unreported.reason().is_empty());

        assert!(AgentLoopOutcome::Completed.is_completed());
        assert_eq!(AgentLoopOutcome::Completed.reason(), "");

        let blocked = AgentLoopOutcome::Blocked {
            reason: "turn budget exhausted".to_string(),
        };
        assert!(!blocked.is_completed());
        assert_eq!(blocked.reason(), "turn budget exhausted");
        // A blocked run is still worth another model on the same provider.
        assert!(!blocked.is_provider_unavailable());
        assert!(!blocked.is_timed_out());
        assert!(!blocked.is_cancelled());
        // `Blocked` is the restartable dead end the orchestrator worker re-issues
        // from the ledger — the stall/loop signal, distinct from a cancel or a
        // spent backstop.
        assert!(blocked.is_blocked());
    }

    /// The non-completed outcomes must be distinguishable *structurally*:
    /// `team_dispatch` skips the rest of a provider's models on
    /// `ProviderUnavailable` and abandons the whole chain on `TimedOut`, and the
    /// orchestrator worker reports `Cancelled` instead of a model failure. Those
    /// decisions used to be made by matching a reason string or scanning status
    /// updates, which is how a healthy run that happened to mention a dead
    /// provider got thrown away.
    #[test]
    fn failover_decisions_read_the_outcome_variant_not_its_prose() {
        let dead = AgentLoopOutcome::ProviderUnavailable {
            reason: "Alibaba Qwen API key not set".to_string(),
        };
        assert!(!dead.is_completed());
        assert!(dead.is_provider_unavailable());
        assert!(!dead.is_timed_out());
        assert!(!dead.is_cancelled());
        // A dead provider is a failover, not a restartable dead end.
        assert!(!dead.is_blocked());
        assert!(
            dead.reason().contains("API key not set"),
            "the operator-facing text keeps the real cause: {}",
            dead.reason()
        );

        let spent = AgentLoopOutcome::TimedOut {
            elapsed_secs: 305,
            budget_secs: 300,
            turns: 13,
        };
        assert!(!spent.is_completed());
        assert!(spent.is_timed_out());
        // A timeout is not a dead provider: the next provider would be tried
        // with a budget that is already gone.
        assert!(!spent.is_provider_unavailable());
        let text = spent.reason();
        assert!(text.contains("300s") && text.contains("13 turn"), "{text}");

        let stopped = AgentLoopOutcome::Cancelled;
        assert!(!stopped.is_completed());
        assert!(stopped.is_cancelled());
        // A cancellation is neither a dead provider nor a spent budget, so
        // neither failover branch may claim it.
        assert!(!stopped.is_provider_unavailable());
        assert!(!stopped.is_timed_out());
        assert_eq!(stopped.reason(), "cancelled by operator");
    }

    /// The lenient `Deserialize` impl must accept every spelling the tool
    /// surface produces: variant names (persisted format), display labels
    /// (workspace preferences), and slugs/aliases (hand-written team JSON).
    #[test]
    fn ai_provider_deserializes_from_variant_label_and_slug() {
        let ok: &[(&str, AiProvider)] = &[
            ("\"AlibabaQwen\"", AiProvider::AlibabaQwen),
            ("\"Alibaba Qwen\"", AiProvider::AlibabaQwen),
            ("\"alibaba\"", AiProvider::AlibabaQwen),
            ("\"qwen\"", AiProvider::AlibabaQwen),
            ("\"CloudflareWorkersAi\"", AiProvider::CloudflareWorkersAi),
            ("\"cf\"", AiProvider::CloudflareWorkersAi),
            ("\"AWS Bedrock\"", AiProvider::AwsBedrock),
            ("\"aws-bedrock\"", AiProvider::AwsBedrock),
            ("\"OpenAI\"", AiProvider::OpenAI),
            ("\"cerebras\"", AiProvider::Cerebras),
        ];
        for (raw, expected) in ok {
            let parsed: AiProvider =
                serde_json::from_str(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
            assert_eq!(&parsed, expected, "for {raw}");
        }
        // Unknown spellings still fail loudly with a helpful message.
        let err = serde_json::from_str::<AiProvider>("\"bogus\"").unwrap_err();
        assert!(err.to_string().contains("unknown AI provider"));
        // Serialize keeps emitting variant names (persisted format unchanged).
        assert_eq!(
            serde_json::to_string(&AiProvider::AlibabaQwen).unwrap(),
            "\"AlibabaQwen\""
        );
    }

    #[test]
    fn new_provider_labels() {
        assert_eq!(AiProvider::Deepseek.label(), "Deepseek");
        assert_eq!(AiProvider::AlibabaQwen.label(), "Alibaba Qwen");
        assert_eq!(AiProvider::AwsBedrock.label(), "AWS Bedrock");
        assert_eq!(AiProvider::Groq.label(), "Groq");
        assert_eq!(AiProvider::Mistral.label(), "Mistral AI");
    }

    #[test]
    fn every_provider_label_round_trips_through_from_label() {
        // Bug #9 regression: workspace preferences used to restore only 4 of
        // the 16 provider labels, silently reverting the rest on restart.
        let all = [
            AiProvider::CloudflareWorkersAi,
            AiProvider::OpenRouter,
            AiProvider::AzureOpenAi,
            AiProvider::LocalOllama,
            AiProvider::LocalLlama,
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
        for p in all {
            assert_eq!(
                AiProvider::from_label(p.label()),
                Some(p),
                "label {:?} failed to round-trip",
                p.label()
            );
        }
        // Slug fallback + whitespace tolerance for legacy files.
        assert_eq!(
            AiProvider::from_label("  alibaba  "),
            Some(AiProvider::AlibabaQwen)
        );
        assert_eq!(AiProvider::from_label("NoSuchProvider"), None);
        assert_eq!(AiProvider::from_label(""), None);
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
            AiProvider::LocalLlama,
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
            AiProvider::LocalLlama,
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
            AiProvider::LocalLlama,
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
            assert_eq!(
                parsed,
                Some(*p),
                "from_slug('{}') should return {:?}",
                slug,
                p
            );
        }
    }

    #[test]
    fn provider_count_is_17() {
        let providers = vec![
            AiProvider::CloudflareWorkersAi,
            AiProvider::OpenRouter,
            AiProvider::AzureOpenAi,
            AiProvider::LocalOllama,
            AiProvider::LocalLlama,
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
        assert_eq!(providers.len(), 17, "Expected 17 providers");
    }

    #[test]
    fn all_provider_aliases_resolve() {
        // Cloudflare aliases
        assert_eq!(
            AiProvider::from_slug("cf"),
            Some(AiProvider::CloudflareWorkersAi)
        );
        assert_eq!(
            AiProvider::from_slug("workers-ai"),
            Some(AiProvider::CloudflareWorkersAi)
        );
        // OpenRouter aliases
        assert_eq!(AiProvider::from_slug("or"), Some(AiProvider::OpenRouter));
        // Azure aliases
        assert_eq!(
            AiProvider::from_slug("azure_openai"),
            Some(AiProvider::AzureOpenAi)
        );
        // Ollama aliases
        assert_eq!(
            AiProvider::from_slug("local"),
            Some(AiProvider::LocalOllama)
        );
        // Anthropic aliases
        assert_eq!(AiProvider::from_slug("claude"), Some(AiProvider::Anthropic));
        // Vertex aliases
        assert_eq!(
            AiProvider::from_slug("google"),
            Some(AiProvider::GoogleVertex)
        );
        assert_eq!(
            AiProvider::from_slug("google_vertex"),
            Some(AiProvider::GoogleVertex)
        );
        // Alibaba aliases
        assert_eq!(AiProvider::from_slug("qwen"), Some(AiProvider::AlibabaQwen));
        assert_eq!(
            AiProvider::from_slug("dashscope"),
            Some(AiProvider::AlibabaQwen)
        );
        // Bedrock aliases
        assert_eq!(AiProvider::from_slug("aws"), Some(AiProvider::AwsBedrock));
        // Perplexity alias
        assert_eq!(AiProvider::from_slug("pplx"), Some(AiProvider::Perplexity));
    }
}
