pub mod background_agents;
pub mod checkpoint;
pub mod collaboration;
pub mod conflict_resolution;
pub mod coordination;
pub mod crypto;
pub mod executor;
pub mod memory_store;
pub mod models;
pub mod nda;
pub mod peer_link;
pub mod peer_server;
pub mod planning;
pub mod provider;
pub mod reasoning;
pub mod self_improve;
pub mod shared_memory;

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
