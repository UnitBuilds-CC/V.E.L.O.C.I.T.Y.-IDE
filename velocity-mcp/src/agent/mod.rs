pub mod background_agents;
pub mod checkpoint;
pub mod coordination;
pub mod crypto;
pub mod executor;
pub mod memory_store;
pub mod models;
pub mod nda;
pub mod planning;
pub mod provider;
pub mod reasoning;
pub mod self_improve;

#[cfg(test)]
mod tests;

pub use executor::{
    run_agent_thread, run_headless_subagent,
};
pub use models::{
    AgentToUiMessage, ApiStyle, HeadlessSubAgentEventKind,
    HeadlessSubAgentProgress, HeadlessSubAgentRequest, ModelInfo,
    UiToAgentMessage, AiProvider,
};
