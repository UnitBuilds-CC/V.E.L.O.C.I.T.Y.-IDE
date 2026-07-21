pub mod executor;
pub mod models;
pub mod nda;
pub mod provider;

#[cfg(test)]
mod tests;

pub use executor::{
    run_agent_reasoning_loop, run_agent_thread, run_compilation_check, run_headless_subagent,
};
pub use models::{
    AgentToUiMessage, ApiStyle, ChatMessage, HeadlessSubAgentEvent, HeadlessSubAgentEventKind,
    HeadlessSubAgentProgress, HeadlessSubAgentRequest, HeadlessSubAgentResult, ModelInfo,
    UiToAgentMessage, AiProvider,
};
pub use nda::{
    append_changelog_nda, convert_jsonl_to_nda, generate_sitemap_text, load_chatlogs_nda,
    parse_chatlogs_nda, save_chatlogs_nda, serialize_chatlogs_nda, write_handover_nda,
    write_sitemap_nda, write_workspace_transcript_nda,
};
pub use provider::{
    default_model_info, enrich_model_profile, fetch_model_catalog, fetch_openrouter_models,
};
