# 4-Provider Agent Reasoning Loop with Automatic Failover

## Classification
- **Category**: Agent Subsystem
- **Files**: velocity-mcp/src/agent/ (28 files)
- **Criticality**: Critical — core AI reasoning infrastructure

## Summary

The agent reasoning engine implements a 4-provider automatic failover chain: Cloudflare Workers AI → OpenRouter → Azure OpenAI → Local Ollama. Failover is circular (Ollama wraps back to Cloudflare). Each provider implements a common trait, and dispatch handles selection, retry, and error recovery.

## Provider Chain

| Priority | Provider | Model | Auth |
|----------|----------|-------|------|
| 1 | Cloudflare Workers AI | `@cf/moonshotai/kimi-k2.7-code` | API key |
| 2 | OpenRouter | `tencent/hy3:free` | API key |
| 3 | Azure OpenAI | `gpt-4o` | Deployment + API key |
| 4 | Local Ollama | `llama3.2` / `qwen2.5-coder` | None |

## Key Files

| File | Role |
|------|------|
| `agent/provider.rs` | Provider trait definition |
| `agent/executor/dispatch.rs` | Provider selection and failover |
| `agent/executor/loop_runner.rs` | Reasoning loop execution |
| `agent/executor/team_routing.rs` | Multi-agent task routing |
| `agent/models.rs` | Model definitions, UiToAgentMessage, AgentToUiMessage |
| `agent/memory_store.rs` | Compressed history |
| `agent/reasoning.rs` | Effort level routing |
| `agent/executor/thread.rs` | Agent thread entry, API key resolution, reasoning loop spawn. FetchPanelData delegates to shared `system_tools::fetch_panel_data_value()` (879 LOC) |

## Agent-UI Message Types

- `UiToAgentMessage`: UserMessage, RunLocalRun, CancelTask, ReloadTeams, FetchPanelData { panel }
- `AgentToUiMessage`: ChatHistoryRestored, ProviderChanged, PanelData { panel, data }
- FetchPanelData handler delegates to `crate::registry::system_tools::fetch_panel_data_value()` so MCP tools and agent channel share one serialisation path

## Multi-Agent Features

- Background agents, collaboration, conflict resolution
- Peer-to-peer bridge (peer_bridge, peer_link, peer_server)
- Planning and self-improvement loops
- Shared memory and checkpoint support
