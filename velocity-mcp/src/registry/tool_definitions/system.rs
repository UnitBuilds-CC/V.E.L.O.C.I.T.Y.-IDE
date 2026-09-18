use crate::registry::types::Tool;
use serde_json::json;

pub fn get_system_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "convert_to_nda".to_string(),
            description: "Convert any file (text, code, CSV, image) into a portable, browser-viewable NDA1 (.nda) document with self-contained provenance. Text becomes wrapped draw-text commands plus content triples; images become a draw-image command. Optionally seal (AES-256-GCM) for confidentiality.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Absolute path to the input file to convert." },
                    "outputPath": { "type": "string", "description": "Optional absolute path to write the compiled .nda file. Defaults to input path with .nda extension." },
                    "seal": { "type": "boolean", "description": "Optional. If true, seal the document at rest with the workspace AES-256-GCM key (not browser-viewable until opened in velocity). Defaults to false (portable)." }
                },
                "required": ["filePath"]
            }),
        },
        Tool {
            name: "read_nda".to_string(),
            description: "Read and parse a compiled .nda binary file to view its semantic triples, visual display commands, and string pool contents.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ndaPath": { "type": "string", "description": "Absolute path to the .nda file to inspect." }
                },
                "required": ["ndaPath"]
            }),
        },
        Tool {
            name: "execute_nda".to_string(),
            description: "Execute a runnable .nda container. If it holds a compiled C# binary, it is run in-memory. If it contains a script (e.g., Python, Node.js, PowerShell, Bash), it executes via the corresponding shell process.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ndaPath": { "type": "string", "description": "Absolute path to the runnable .nda file." },
                    "arguments": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional command-line arguments to pass to the executable or script."
                    }
                },
                "required": ["ndaPath"]
            }),
        },
        Tool {
            name: "read_file".to_string(),
            description: "Read the contents of a file in the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"scripts/bootstrap.sh\")" }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "write_file".to_string(),
            description: "Write or overwrite a file with specific content in the workspace. Creates folders if they do not exist. Include 'context' to record *why* this change is being made — the reason is persisted in the codebase event log so future agents can understand the decision.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"scripts/bootstrap.sh\")" },
                    "content": { "type": "string", "description": "The text content to write to the file." },
                    "context": { "type": "string", "description": "Why this file is being written (requirement, design decision, bug fix). Recorded in the codebase event log for future reference." }
                },
                "required": ["relativeFilePath", "content"]
            }),
        },
        Tool {
            name: "list_dir".to_string(),
            description: "List the contents of a directory relative to the workspace root.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeDirPath": { "type": "string", "description": "Directory path relative to workspace root. Use \".\" for the workspace root." }
                },
                "required": ["relativeDirPath"]
            }),
        },
        Tool {
            name: "grep_search".to_string(),
            description: "Find lines containing a query string in the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The text to search for" }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "run_command".to_string(),
            description: "Run a shell command inside the current workspace directory and capture its combined stdout and stderr output.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The command line string to execute." }
                },
                "required": ["command"]
            }),
        },
        Tool {
            name: "fetch_panel_data".to_string(),
            description: "Read structured IDE panel data without navigating the GUI. Supported panels: teams (expert-team roster), wiki (workspace documentation summary), graph (symbol/file summary), bookmarks (saved workspace bookmarks), and files (a workspace directory listing). This is read-only and safe for agents to use for context gathering.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "panel": {
                        "type": "string",
                        "enum": ["teams", "wiki", "graph", "bookmarks", "files"],
                        "description": "The IDE panel data to fetch."
                    },
                    "relativePath": {
                        "type": "string",
                        "description": "Optional workspace-relative directory for the files panel. Defaults to the workspace root. Ignored by other panels."
                    }
                },
                "required": ["panel"]
            }),
        },
        Tool {
            name: "delete_file".to_string(),
            description: "Delete a file in the workspace. Include 'context' to record *why* this file is being removed — the reason is persisted in the codebase event log.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"temp.txt\")" },
                    "context": { "type": "string", "description": "Why this file is being deleted. Recorded in the codebase event log." }
                },
                "required": ["relativeFilePath"]
            }),
        },
        // ── Agent Checkpointing ─────────────────────────────────────────────
        Tool {
            name: "agent_checkpoint_create".to_string(),
            description: "Create a workspace checkpoint (git-based snapshot) before making changes. Allows restoring to this point later.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "label": { "type": "string", "description": "Human-readable label for this checkpoint (e.g. 'before refactor')." }
                },
                "required": ["label"]
            }),
        },
        Tool {
            name: "agent_checkpoint_restore".to_string(),
            description: "Restore the workspace to a previously created checkpoint. Reverts all file changes made after that checkpoint.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "checkpointId": { "type": "integer", "description": "The ID of the checkpoint to restore to. Use agent_checkpoint_list to see available IDs." }
                },
                "required": ["checkpointId"]
            }),
        },
        Tool {
            name: "agent_checkpoint_list".to_string(),
            description: "List all available workspace checkpoints with their IDs, labels, and creation times.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        // ── Agent Memory ────────────────────────────────────────────────────
        Tool {
            name: "agent_memory_remember".to_string(),
            description: "Store a persistent memory that the agent can recall in future sessions. Use for strategies, facts, and learned patterns.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Unique key for this memory (e.g. 'site:github:login_flow')." },
                    "content": { "type": "string", "description": "The content to remember." },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags for filtering (e.g. ['web', 'auth'])." },
                    "score": { "type": "number", "description": "Initial importance score 0.0-1.0. Default 0.5." }
                },
                "required": ["key", "content"]
            }),
        },
        Tool {
            name: "agent_memory_recall".to_string(),
            description: "Recall memories relevant to a query using semantic similarity. Returns the most relevant stored memories.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query to find relevant memories." },
                    "limit": { "type": "integer", "description": "Maximum number of results to return. Default 5." }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "agent_memory_forget".to_string(),
            description: "Remove a specific memory by its key. Use to clean up outdated or incorrect memories.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The key of the memory to remove." }
                },
                "required": ["key"]
            }),
        },
        // ── Test Generation ─────────────────────────────────────────────────
        Tool {
            name: "code_generate_tests".to_string(),
            description: "Generate test scaffolding for source code. Analyzes function signatures and produces test stubs with edge cases.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Source code to generate tests for." },
                    "language": { "type": "string", "description": "Programming language (rust, typescript, python)." }
                },
                "required": ["source", "language"]
            }),
        },
        Tool {
            name: "code_coverage_analyze".to_string(),
            description: "Analyze test coverage for the workspace (or a specific file/directory via 'path'). Discovers testable functions, reports the coverage percentage and untested functions, and scaffolds test skeletons for the gaps.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Optional workspace-relative path to a file or directory to scope the analysis. Omit to analyze the whole workspace." }
                },
                "required": []
            }),
        },
        // ── Knowledge / RAG ─────────────────────────────────────────────────
        Tool {
            name: "knowledge_ingest".to_string(),
            description: "Add content to the workspace knowledge base (a persistent, chunked RAG store). Provide either inline 'text' (with an optional 'source' name) or a workspace-relative 'path' to a file or directory.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Raw text to ingest. Use with 'source'." },
                    "source": { "type": "string", "description": "Name/label for the source when ingesting 'text' (default: inline)." },
                    "path": { "type": "string", "description": "Workspace-relative path to a file or directory to ingest." }
                },
                "required": []
            }),
        },
        Tool {
            name: "knowledge_search".to_string(),
            description: "Search the workspace knowledge base and return the most relevant passages, ranked by TF-IDF cosine similarity.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural-language or keyword query." },
                    "k": { "type": "integer", "description": "Maximum number of results to return (default 5)." }
                },
                "required": ["query"]
            }),
        },
        // ── Workspace Indexing ───────────────────────────────────────────────
        Tool {
            name: "index_workspace".to_string(),
            description: "Index the workspace by compiling all Rust source files into the site map (NDA triples). This populates the symbol graph, enables the wiki, and gives the IDE semantic understanding of the codebase. Must be run at least once before graph/wiki panels return data. Safe to re-run — updates incrementally. Include 'context' to record *why* indexing is being triggered.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Optional workspace-relative subdirectory to limit indexing. Omit to index the entire workspace." },
                    "context": { "type": "string", "description": "Why indexing is being triggered (e.g. 'after major refactor', 'initial workspace setup'). Recorded in the codebase event log." }
                },
                "required": []
            }),
        },
        // ── Custom Tool Management ───────────────────────────────────────────
        Tool {
            name: "tool_register".to_string(),
            description: "Register a new custom tool backed by a shell command. The tool becomes immediately available for invocation without restarting the MCP server. Use {{param}} placeholders in the command to substitute argument values at call time. Custom tools are persisted to .velocity/custom_tools.json and survive restarts.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name":        { "type": "string",  "description": "Unique tool name (must not collide with built-in tools)." },
                    "description": { "type": "string",  "description": "Human-readable description shown in tools/list." },
                    "command":     { "type": "string",  "description": "Shell command to execute. Use {{param}} for argument substitution." },
                    "parameters":  { "type": "object",  "description": "JSON Schema properties object for the tool's input parameters (optional)." }
                },
                "required": ["name", "description", "command"]
            }),
        },
        Tool {
            name: "tool_unregister".to_string(),
            description: "Remove a previously registered custom tool. The tool is removed from the active list and from disk persistence.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the custom tool to remove." }
                },
                "required": ["name"]
            }),
        },
        Tool {
            name: "tool_list_custom".to_string(),
            description: "List all dynamically registered custom tools with their names, descriptions, and input schemas.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        // ── Audit & Observability ──────────────────────────────────────────
        Tool {
            name: "audit_status".to_string(),
            description: "Show the tool execution audit log status: total entries recorded, active sessions, and the most recent tool calls with their outcomes and durations. Useful for verifying that all tool invocations are being tracked.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max recent entries to return (default 10, max 100)." }
                },
                "required": []
            }),
        },
        // ── Codebase Event Store ──────────────────────────────────────────
        Tool {
            name: "event_record".to_string(),
            description: "Record a codebase event linking an agent action to a site map state change. Captures *why* a change was made (context), what files were affected, and the Merkle root before/after. Events are persisted to .velocity/events/events.jsonl for long-term codebase history.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "What changed (e.g. 'Added OAuth2 token validation to auth module')." },
                    "context": { "type": "string", "description": "Why this change was made (agent reasoning, requirements, motivation)." },
                    "tool_name": { "type": "string", "description": "Tool that triggered the change (auto-filled if called from dispatch)." },
                    "affected_files": { "type": "array", "items": { "type": "string" }, "description": "Relative paths of files modified." },
                    "merkle_root_before": { "type": "string", "description": "Site map Merkle root before the change (hex, 16 chars)." },
                    "merkle_root_after": { "type": "string", "description": "Site map Merkle root after the change (hex, 16 chars)." }
                },
                "required": ["description"]
            }),
        },
        Tool {
            name: "event_history".to_string(),
            description: "Query the codebase event history. Returns events newest-first with optional filters for file path and outcome (success/failure/revert/pending). Shows the full decision trail including failed attempts and reverts.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Filter events to those affecting a specific file (substring match)." },
                    "outcome": { "type": "string", "description": "Filter by outcome: success, failure, revert, pending." },
                    "limit": { "type": "integer", "description": "Max events to return (default 20, max 200)." }
                },
                "required": []
            }),
        },
        Tool {
            name: "event_context".to_string(),
            description: "Get full detail for a single codebase event by sequence number. Includes agent context, Merkle roots, affected files, and failure reason.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sequence": { "type": "integer", "description": "Event sequence number." }
                },
                "required": ["sequence"]
            }),
        },
        Tool {
            name: "event_mark_outcome".to_string(),
            description: "Update the outcome of a previously recorded codebase event. Use this to mark a change as success, failure (with reason), or revert. This enables tracking which approaches worked and which didn't, so future agents don't repeat failed experiments.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sequence": { "type": "integer", "description": "Event sequence number to update." },
                    "outcome": { "type": "string", "description": "New outcome: success, failure, revert, or pending." },
                    "failure_reason": { "type": "string", "description": "If outcome is failure, explain why (e.g. 'Race condition in concurrent auth flows')." }
                },
                "required": ["sequence", "outcome"]
            }),
        },
        Tool {
            name: "event_timeline".to_string(),
            description: "Show a chronological timeline of all codebase events with state transitions (Merkle root changes). Shows the codebase's evolution over time — what changed, when, why, and with what outcome. Newest first.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max events to return (default 50, max 500)." }
                },
                "required": []
            }),
        },
        Tool {
            name: "event_attach_context".to_string(),
            description: "Attach agent reasoning (the *why*) to an existing codebase event. Use this after a change to record why it was made — the requirement, the motivation, the design decision. This context is what makes the event store valuable for future agents reviewing the codebase.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sequence": { "type": "integer", "description": "Event sequence number to attach context to." },
                    "context": { "type": "string", "description": "The reasoning behind the change (requirement, motivation, design decision)." }
                },
                "required": ["sequence", "context"]
            }),
        },
        // ── Workflows ───────────────────────────────────────────────────────
        Tool {
            name: "workflow_run".to_string(),
            description: "Execute a saved workflow by id. Runs its steps in order (tool calls, agent tasks, conditions) and returns the run record with per-step status and output.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Id of the workflow to run (see .velocity/workflows/)." }
                },
                "required": ["id"]
            }),
        },
        // ── Connectors ──────────────────────────────────────────────────────
        Tool {
            name: "connector_call".to_string(),
            description: "Invoke a configured external HTTP connector by id. Resolves the connector's credential from the encrypted secret store, assembles the request (base URL + path, headers, auth), and returns the response status and body.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Id of the connector to call (see .velocity/connectors.json)." },
                    "method": { "type": "string", "description": "HTTP method (GET, POST, ...). Defaults to GET." },
                    "path": { "type": "string", "description": "Path appended to the connector base URL (leading slash optional)." },
                    "headers": { "type": "object", "description": "Optional extra headers as a string map." },
                    "body": { "type": "string", "description": "Optional request body sent verbatim." }
                },
                "required": ["id"]
            }),
        },
        // ── Multimodal ──────────────────────────────────────────────────────
        Tool {
            name: "generate_image".to_string(),
            description: "Generate an image from a text prompt via Cloudflare Workers AI and save it into the workspace. Returns the saved path.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "Text prompt describing the desired image." },
                    "model": { "type": "string", "description": "Optional Workers AI text-to-image model id. Defaults to stable-diffusion-xl." },
                    "output": { "type": "string", "description": "Optional workspace-relative output path (e.g. generated/logo.png)." }
                },
                "required": ["prompt"]
            }),
        },
        Tool {
            name: "describe_image".to_string(),
            description: "Describe an image file. Vision-capable models can consume the image directly; returns an OCR text fallback for non-vision models plus mime and size.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path to the image file." }
                },
                "required": ["path"]
            }),
        },

        // ── GUI Control Bridge ─────────────────────────────────────────────
        Tool {
            name: "gui_open_file".to_string(),
            description: "Open a file in the running IDE's editor. Requires the GUI to be running. Sends a command via the GUI control named pipe.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path to the file to open." }
                },
                "required": ["path"]
            }),
        },
        Tool {
            name: "gui_get_state".to_string(),
            description: "Get the current state of the running IDE: open files, active file, active panel, focused central tab, whether the central area shows the dock or the welcome screen, sidebar visibility, chat message count, git branch. Requires the GUI to be running.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
            }),
        },
        Tool {
            name: "gui_navigate_panel".to_string(),
            description: "Navigate to a specific panel in the IDE's activity bar. Valid panels: files, search, git, chat, build, agents, knowledge, workspace. Requires the GUI to be running.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "panel": { "type": "string", "description": "Panel name: files, search, git, chat, build, agents, knowledge, or workspace." }
                },
                "required": ["panel"]
            }),
        },
        Tool {
            name: "gui_toggle_panel".to_string(),
            // Built from the same table the command resolves against, so the
            // advertised list cannot drift from the accepted one.
            description: format!(
                "Open a central panel tab in the running IDE (settings, wiki, graph, ...) -- the same entry point the activity-bar gear, the menu, Ctrl+, and the status-bar provider chip use -- or close it if it already has focus. Valid panels: {}. Requires the GUI to be running.",
                crate::editor::app::types::bridge_panel_names().join(", ")
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "panel": { "type": "string", "description": format!("One of: {}.", crate::editor::app::types::bridge_panel_names().join(", ")) }
                },
                "required": ["panel"]
            }),
        },
        Tool {
            name: "gui_quit".to_string(),
            description: "Quit the running IDE. Requires the GUI to be running.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
            }),
        },
    ]
}
