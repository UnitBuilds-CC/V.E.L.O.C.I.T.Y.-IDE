# Editor IDE Feature Modules

## Classification
- **Category**: IDE Features
- **Files**: velocity-mcp/src/editor/ (83 modules total, 67+ feature modules)
- **Criticality**: High — full IDE capability beyond basic editing

## Summary

The editor module contains 83 sub-modules covering: workflow automation (5 files), plugin system (2), code intelligence via LSP (7), voice commands (1), multimodal input (1), editor core (15+), agent UI (4), governance & deploy (3), and miscellaneous features.

## Key Feature Clusters

### Workflow Automation
- `WorkflowStep`: AgentTask | Tool | Connector | Condition
- Persisted as JSON under `.velocity/workflows/`
- Produces `WorkflowRun` for governance audit

### Plugin System
- `PluginRegistry` — discovery, loading, lifecycle
- `PluginHandler` trait — tool dispatch
- User-granted `PluginPermission` per plugin

### Code Intelligence
- `LspServerConfig` — JSON-RPC over stdin/stdout to language servers
- `rust_analyzer()` factory for Rust workspace
- Completion, diagnostics, semantic search, inline suggestions

### Voice Commands
- `VoiceIntent`: OpenFile, Search, RunTests, Build, Deploy, FixError, Refactor, Navigate, Explain
- Windows Speech API continuous recognition
