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
pub mod wiki_view;

// Agentic UI Phase 1 - Zero-allocation components
pub mod agent_ui_render;
pub mod agent_ui_state;

// Agentic UI Phase 2 - Task timeline
pub mod task_timeline;

// Agentic UI Phase 2 - Smart sidebar
pub mod smart_sidebar;

// Expert Teams Studio & Task Routing
pub mod agent_memory;
pub mod expert_team;
pub mod skill_file;
pub mod team_builder_chat;
pub mod team_router;

// Mode-Specialized UI Workflows
pub mod bottom_panel;
pub mod mode_config;
pub mod sidebar_tabs;
pub mod toolbar_actions;

// IDE Core Editor Capabilities
pub mod auto_indent;
pub mod bracket_match;
pub mod breadcrumbs;
pub mod browse_panel;
pub mod checkpoint;
pub mod code_folding;
pub mod completion;
pub mod continuation_ledger;
pub mod debugger;
pub mod deploy_pipeline;
pub mod diagnostics;
pub mod extensions;
pub mod find_replace;
pub mod git_ui;
pub mod inline_suggestions;
pub mod keybindings;
pub mod knowledge_base;
pub mod live_orchestration;
pub mod lsp_client;
pub mod minimap;
pub mod regex_engine;
pub mod semantic_search;
pub mod snippets;
pub mod speculative_precomp;
pub mod terminal;
pub mod test_generator;
pub mod triggers;
pub mod voice_commands;
pub mod workflow;
