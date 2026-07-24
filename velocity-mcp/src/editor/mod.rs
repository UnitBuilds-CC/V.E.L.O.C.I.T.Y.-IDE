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
pub mod expert_team;
pub mod skill_file;
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
pub mod code_folding;
pub mod completion;
pub mod debugger;
pub mod diagnostics;
pub mod extensions;
pub mod find_replace;
pub mod git_ui;
pub mod keybindings;
pub mod lsp_client;
pub mod minimap;
pub mod snippets;
pub mod terminal;
