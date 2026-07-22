pub mod app;
pub mod browser;
pub mod buffer;
pub mod chat_panel;
pub mod code_editor;
pub mod graph_view;
pub mod history;
pub mod mission_control;
pub mod orchestrator;
pub use orchestrator as orchestrator_panel;
pub mod search;
pub mod status_bar;
pub mod theme;
pub mod toast;
pub mod usage_panel;

// Agentic UI Phase 1 - Zero-allocation components
pub mod agent_ui_render;
pub mod agent_ui_state;

// Agentic UI Phase 2 - Task timeline
pub mod task_timeline;

// Agentic UI Phase 2 - Smart sidebar
pub mod smart_sidebar;

// Expert Teams Studio & Task Routing
pub mod expert_team;
