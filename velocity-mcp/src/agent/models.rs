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
        /// Why this call was refused, in the words the model should read. A bare
        /// "rejected" is not a steer: an unattended run that gets one retries the
        /// same call next turn, and a person who declined in the panel cannot tell
        /// the run what to do instead.
        reason: String,
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

/// Declares the provider catalog once: the enum's variants and every string a
/// person or a file can use to name them.
///
/// One row is one backend: `Variant, "display label", "slug", [extra
/// spellings]`. The macro emits the enum, [`AiProvider::ALL`], `label`, `slug`,
/// `aliases`, `variant_name`, `from_label` and `from_slug` from those rows, so
/// the lists cannot disagree. The variant name is a parse key too (compared
/// case-insensitively) because it is what persisted team JSON and NDA rows
/// already carry, which is why no row has to repeat it.
macro_rules! ai_provider_catalog {
    ($($(#[$attr:meta])* $variant:ident, $label:literal, $slug:literal, [$($alias:literal),* $(,)?];)*) => {
        #[doc = "Which AI backend to use for inference."]
        #[doc = ""]
        #[doc = "The variants, and every name each one answers to, come from the rows of the"]
        #[doc = "[`ai_provider_catalog`] invocation: [`AiProvider::ALL`], `label`, `slug`,"]
        #[doc = "`aliases`, `variant_name`, `from_label` and `from_slug` are all generated from"]
        #[doc = "that one table. Serialize keeps variant names; Deserialize is handwritten so"]
        #[doc = "persisted files, UI labels, and hand-written LLM team JSON all parse."]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
        pub enum AiProvider {
            $($(#[$attr])* $variant,)*
        }

        impl AiProvider {
            /// Every dispatchable backend, in the order the provider pickers,
            /// `list_providers` and the manual present them.
            ///
            /// This is the one list. Before it, `label`, `slug`, `from_label`,
            /// `from_slug`, the `list_providers` payload, both GUI combo boxes
            /// and the credential table each held their own enumeration, and
            /// they had already drifted: llama.cpp was dispatchable — it has
            /// match arms in the executor, a fallback and a default model — but
            /// appeared in none of the lists, so no person could select it and
            /// `list_providers` reported 16 backends for a 17-variant enum.
            pub const ALL: &[AiProvider] = &[$(AiProvider::$variant,)*];

            /// The name a person reads, in the picker and in workspace preferences.
            pub fn label(self) -> &'static str {
                match self {
                    $(AiProvider::$variant => $label,)*
                }
            }

            /// Stable machine slug used for NDA serialization and tool arguments.
            pub fn slug(self) -> &'static str {
                match self {
                    $(AiProvider::$variant => $slug,)*
                }
            }

            /// The extra spellings [`AiProvider::from_slug`] accepts besides its
            /// own slug and variant name — and exactly what `list_providers`
            /// advertises, so the tool can never offer an alias that the parser
            /// would then reject.
            pub fn aliases(self) -> &'static [&'static str] {
                match self {
                    $(AiProvider::$variant => &[$($alias),*],)*
                }
            }

            /// The serde variant name, which is the persisted form in team JSON.
            pub fn variant_name(self) -> &'static str {
                match self {
                    $(AiProvider::$variant => stringify!($variant),)*
                }
            }

            /// Parse a provider from the display label produced by
            /// [`AiProvider::label`] (e.g. when restoring workspace preferences),
            /// falling back to slug and alias parsing so older or
            /// machine-formatted values still resolve. Both halves read
            /// [`AiProvider::ALL`]. The label half used to be a hand-written
            /// match over all seventeen labels whose own doc comment warned
            /// "every `label()` arm must have a mirror here" — a correct list,
            /// kept correct by a person remembering to duplicate each row, which
            /// is the maintenance the macro now does instead.
            pub fn from_label(value: &str) -> Option<AiProvider> {
                let v = value.trim();
                Self::ALL
                    .iter()
                    .copied()
                    .find(|p| p.label() == v)
                    .or_else(|| Self::from_slug(v))
            }

            /// Parse a provider from a slug, a declared alias, or the variant
            /// name. Case-insensitive, and blank-safe: no key is empty, so an
            /// empty string resolves to nothing.
            pub fn from_slug(value: &str) -> Option<AiProvider> {
                let v = value.trim().to_lowercase();
                Self::ALL.iter().copied().find(|p| {
                    p.slug() == v
                        || p.variant_name().eq_ignore_ascii_case(&v)
                        || p.aliases().iter().any(|a| a.eq_ignore_ascii_case(&v))
                })
            }
        }
    };
}

// The catalog itself. A doc comment here would be dropped (rustdoc does not
// document macro invocations), so the enum's own docs are emitted by the macro.
ai_provider_catalog! {
    CloudflareWorkersAi, "Cloudflare Workers AI", "cloudflare", ["cf", "workers-ai"];
    OpenRouter, "OpenRouter", "openrouter", ["or"];
    AzureOpenAi, "Azure OpenAI", "azure", ["azure_openai"];
    LocalOllama, "Local Ollama", "ollama", ["local"];
    /// A locally-run llama.cpp `llama-server` (OpenAI-compatible endpoint, no
    /// key). Reached with 3-tier MoE offload so a model far larger than VRAM
    /// still runs, trading throughput for an effectively unbounded context.
    LocalLlama, "Local llama.cpp", "llamacpp", ["llama", "llamaserver"];
    OpenAI, "OpenAI Direct", "openai", ["openai_direct"];
    Anthropic, "Anthropic Claude", "anthropic", ["claude"];
    GoogleVertex, "Google Vertex AI", "vertex", ["google_vertex", "google"];
    /// Deepseek API (OpenAI-compatible endpoint).
    Deepseek, "Deepseek", "deepseek", [];
    /// Alibaba Cloud Qwen (DashScope API, OpenAI-compatible).
    AlibabaQwen, "Alibaba Qwen", "alibaba", ["qwen", "dashscope", "alibaba_qwen"];
    /// AWS Bedrock (via OpenAI-compatible proxy or native API).
    AwsBedrock, "AWS Bedrock", "bedrock", ["aws_bedrock", "aws"];
    /// Groq (OpenAI-compatible endpoint for LPU inference).
    Groq, "Groq", "groq", [];
    /// Mistral AI (La Plateforme, OpenAI-compatible endpoint).
    Mistral, "Mistral AI", "mistral", ["mistralai", "mistral_ai", "laplateforme"];
    /// Together AI (OpenAI-compatible endpoint).
    TogetherAi, "Together AI", "together", ["together_ai"];
    /// Fireworks AI (OpenAI-compatible endpoint).
    FireworksAi, "Fireworks AI", "fireworks", ["fireworks_ai"];
    /// Perplexity API (OpenAI-compatible endpoint).
    Perplexity, "Perplexity", "perplexity", ["pplx"];
    /// Cerebras (OpenAI-compatible endpoint for wafer-scale inference).
    Cerebras, "Cerebras", "cerebras", [];
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
    fn every_spelling_of_every_backend_resolves_back_to_it() {
        // The parsers are generated from the same rows as the variants, so this
        // is the check that no name a person, a preferences file or a hand-written
        // team JSON can write lands on the wrong backend — or on none. It reads
        // the names out of the catalog instead of restating them: the test it
        // replaces carried its own array of seventeen variants and compared
        // labels only, so an eighth spelling system never existed there and a
        // row added to the catalog would not have joined the list that claims to
        // cover every backend.
        for p in AiProvider::ALL.iter().copied() {
            assert_eq!(AiProvider::from_label(p.label()), Some(p), "label");
            assert_eq!(AiProvider::from_slug(p.slug()), Some(p), "slug");
            assert_eq!(
                AiProvider::from_slug(p.variant_name()),
                Some(p),
                "variant name (the persisted form)"
            );
            assert_eq!(
                AiProvider::from_label(p.variant_name()),
                Some(p),
                "variant name through the label parser"
            );
            for alias in p.aliases() {
                assert_eq!(
                    AiProvider::from_slug(alias),
                    Some(p),
                    "alias {alias:?} of {p:?}"
                );
                // Case-insensitive, because a model types "QWEN" as readily as "qwen".
                let upper = alias.to_uppercase();
                assert_eq!(
                    AiProvider::from_slug(&upper),
                    Some(p),
                    "alias {alias:?} upper-cased"
                );
                let raw = format!("\"{alias}\"");
                let parsed: AiProvider = serde_json::from_str(&raw)
                    .unwrap_or_else(|e| panic!("{raw} should parse: {e}"));
                assert_eq!(parsed, p, "alias {alias:?} through Deserialize");
            }
        }
        // Slug fallback + whitespace tolerance for legacy files.
        assert_eq!(
            AiProvider::from_label("  alibaba  "),
            Some(AiProvider::AlibabaQwen)
        );
        assert_eq!(AiProvider::from_label("NoSuchProvider"), None);
        assert_eq!(AiProvider::from_label(""), None);
        assert_eq!(AiProvider::from_slug(""), None);
    }

    #[test]
    fn no_two_backends_answer_to_the_same_name() {
        // `from_slug` returns the first catalog row whose keys match, so a
        // spelling claimed by two backends would make one of them unreachable by
        // name — and `list_providers` would advertise an alias that resolves to
        // someone else. Labels matter the same way: they are what the picker
        // draws and what a preferences file stores.
        let mut owner: std::collections::HashMap<String, AiProvider> =
            std::collections::HashMap::new();
        for p in AiProvider::ALL.iter().copied() {
            let mine: std::collections::HashSet<String> = [p.slug(), p.variant_name()]
                .into_iter()
                .chain(p.aliases().iter().copied())
                .map(|key| key.trim().to_lowercase())
                .collect();
            assert!(
                !mine.contains(""),
                "{p:?} carries a blank spelling from the catalog"
            );
            for folded in mine {
                if let Some(prev) = owner.insert(folded.clone(), p) {
                    panic!("{folded:?} names both {prev:?} and {p:?}");
                }
            }
        }
        let labels: std::collections::HashSet<&str> =
            AiProvider::ALL.iter().map(|p| p.label()).collect();
        assert_eq!(
            labels.len(),
            AiProvider::ALL.len(),
            "two backends share a picker label, so one cannot be chosen by name"
        );
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
    fn the_catalog_has_no_duplicate_rows() {
        // Every other guard in this module reads `ALL`, so a repeated row would
        // make the backend count wrong in every report at once while still
        // parsing fine. A 17-row pairwise scan needs no `Hash` derive, which
        // `AiProvider` has never carried and no production code needs.
        let all = AiProvider::ALL;
        for (index, provider) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(
                    provider, other,
                    "{provider:?} appears twice in the provider catalog"
                );
            }
        }
    }

    #[test]
    fn every_backend_is_dispatchable_and_has_a_fallback() {
        // The executor's `match` arms are exhaustive, so the compiler already
        // knows how to reach a new variant; what a stale hand-written list hid
        // was whether the rest of the system could name it. Two provider.rs
        // tests used to answer that over a 16-item list of their own — which is
        // how llama.cpp kept a fallback and a default model in source while no
        // list anywhere checked that llama.cpp had them.
        for p in AiProvider::ALL.iter().copied() {
            let model = crate::agent::provider::default_provider_model(p);
            assert!(!model.trim().is_empty(), "{p:?} offers no default model");
            let fb = crate::agent::provider::fallback_provider(p);
            assert_ne!(fb, p, "{p:?} falls back to itself, so failover spins");
            assert!(
                AiProvider::ALL.contains(&fb),
                "{p:?} falls back to {fb:?}, which the catalog does not carry"
            );
        }
    }

    #[test]
    fn the_provider_pickers_read_the_catalog() {
        // Both combo boxes carried their own arrays, in two different orders,
        // and both had lost llama.cpp — the backend was dispatchable but could
        // not be chosen by a person. A guard on the source is the honest form of
        // "the picker shows the catalog", because egui draws nothing in a unit
        // test: each file must loop over `AiProvider::ALL`, and must not name a
        // backend inside a loop header or an array literal.
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let pickers = ["src/editor/chat_panel.rs", "src/editor/app/render.rs"];
        for rel in pickers {
            let text = std::fs::read_to_string(crate_dir.join(rel))
                .unwrap_or_else(|e| panic!("{rel}: {e}"));
            assert!(
                text.lines()
                    .any(|line| line.contains("for ") && line.contains("AiProvider::ALL")),
                "{rel} no longer builds its provider picker from AiProvider::ALL"
            );
            for p in AiProvider::ALL {
                let element = format!("AiProvider::{p:?},");
                for line in text.lines() {
                    let is_a_list = line.contains("for ") || line.contains('[');
                    assert!(
                        !(is_a_list && line.contains(&element)),
                        "{rel} puts {element} back into a list of its own: {}",
                        line.trim()
                    );
                }
            }
        }
    }

    #[test]
    fn documented_backend_counts_match_the_catalog() {
        // The number of backends a person can pick is a fact about the catalog,
        // so the documents that state it are compared against it instead of
        // trusted: `USER_GUIDE.md` and three manual pages said "16 providers"
        // for a 17-variant enum.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("velocity-mcp sits at the repo root")
            .to_path_buf();
        let docs = [
            "USER_GUIDE.md",
            "README.md",
            "docs/manual/README.md",
            "docs/manual/getting-started.md",
            "docs/manual/panels-reference.md",
            "docs/manual/settings-providers.md",
            "docs/manual/mcp-tools.md",
        ];
        let mut checked = 0usize;
        for rel in docs {
            let text =
                std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
            for (n, sentence) in provider_count_claims(&text) {
                checked += 1;
                assert_eq!(
                    n,
                    AiProvider::ALL.len(),
                    "{rel} counts the backends as {n}: {sentence}"
                );
            }
        }
        assert!(
            checked >= 4,
            "the count guard read only {checked} provider claims from the manuals; a rewrite \
             that reworded all of them would otherwise pass this test by saying nothing"
        );
    }

    #[test]
    fn the_manual_lists_every_backend_the_picker_offers() {
        // `panels-reference.md` documents the chat pill's combo box by name, so a
        // backend missing from it is a backend a reader cannot discover even when
        // the picker does show it.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("velocity-mcp sits at the repo root")
            .to_path_buf();
        let text = std::fs::read_to_string(root.join("docs/manual/panels-reference.md"))
            .expect("panels-reference.md");
        for p in AiProvider::ALL {
            assert!(
                text.contains(p.variant_name()),
                "the manual's provider list never names {}",
                p.variant_name()
            );
        }
    }

    #[test]
    fn the_user_guide_table_matches_the_catalog() {
        // `USER_GUIDE.md` carries a Provider/Default Model table, and every one
        // of its columns had drifted: sixteen rows for a 17-variant enum, plus
        // `llama3.2` for Local Ollama and `qwen-plus` for Alibaba Qwen where the
        // code says `qwen2.5-coder:0.5b` and `qwen-max`. A count guard alone
        // would not have caught the two stale model names, so the rows are
        // compared cell by cell against `label()` and `default_provider_model`.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("velocity-mcp sits at the repo root")
            .to_path_buf();
        let text = std::fs::read_to_string(root.join("USER_GUIDE.md")).expect("USER_GUIDE.md");
        let lines: Vec<&str> = text.lines().collect();

        let expected: Vec<(String, String)> = AiProvider::ALL
            .iter()
            .map(|p| {
                (
                    p.label().to_string(),
                    crate::agent::provider::default_provider_model(*p),
                )
            })
            .collect();

        let mut tables = 0usize;
        let mut index = 0usize;
        while index < lines.len() {
            let header = lines[index].trim();
            if !header.starts_with("| Provider | Default Model") {
                index += 1;
                continue;
            }
            tables += 1;
            let mut rows: Vec<(String, String)> = Vec::new();
            index += 1;
            while index < lines.len() && lines[index].trim().starts_with('|') {
                let cells: Vec<&str> = lines[index]
                    .trim()
                    .trim_matches('|')
                    .split('|')
                    .map(str::trim)
                    .collect();
                index += 1;
                if cells.len() < 2 || cells[0].chars().all(|c| c == '-') {
                    continue; // separator row
                }
                rows.push((cells[0].to_string(), cells[1].trim_matches('`').to_string()));
            }
            assert_eq!(
                rows, expected,
                "the Provider/Default Model table in USER_GUIDE.md no longer matches \
                 AiProvider::ALL and default_provider_model (table #{tables})"
            );
        }
        assert!(
            tables >= 1,
            "USER_GUIDE.md has no Provider/Default Model table at all, so this guard reads nothing"
        );
    }

    /// Every "<number> … providers" claim in `text`, with the line that carries it.
    ///
    /// Prose that counts something other than the platform's catalog is skipped
    /// by wording, not by file: "at least 2 providers" is a setup instruction.
    /// The scan is backwards from the word so that "16 AI providers" and "the
    /// same 17 providers" both read as claims, and `providers_health (5)`, where
    /// the number follows the word, does not.
    fn provider_count_claims(text: &str) -> Vec<(usize, String)> {
        const WORD: &str = "providers";
        let mut claims = Vec::new();
        for line in text.lines() {
            let folded = line.to_lowercase();
            let mut search = 0usize;
            while let Some(offset) = folded[search..].find(WORD) {
                let at = search + offset;
                search = at + WORD.len();
                let prev = folded[..at].chars().next_back();
                let next = folded[at + WORD.len()..].chars().next();
                let joined = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
                if joined(prev) || joined(next) {
                    continue; // "provider", "providers_health", part of a path
                }
                let window: String = folded[..at].chars().rev().take(18).collect::<String>();
                let window: String = window.chars().rev().collect();
                if window.contains("at least") {
                    continue;
                }
                let end = match window.rfind(|c: char| c.is_ascii_digit()) {
                    Some(i) => i + 1,
                    None => continue,
                };
                let start = window[..end]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit())
                    .count();
                let gap = &window[end..];
                let gap_ok = gap
                    .chars()
                    .all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '\'');
                let standalone = window[..end - start]
                    .chars()
                    .next_back()
                    .map(|c| !c.is_alphanumeric())
                    .unwrap_or(true);
                if gap_ok && standalone {
                    let n: usize = window[end - start..end].parse().unwrap();
                    claims.push((n, line.trim().to_string()));
                }
            }
        }
        claims
    }
}
