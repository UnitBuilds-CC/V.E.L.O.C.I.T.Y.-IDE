use crate::registry::types::Tool;
use serde_json::json;

pub fn get_system_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "convert_to_nda".to_string(),
            description: "Convert any file (e.g. C# source code, PDF, CSV, Excel, Image, Zip archive) into a cryptographically signed NDA (.nda) binary document.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Absolute path to the input file to convert." },
                    "outputPath": { "type": "string", "description": "Optional absolute path to write the compiled .nda file. Defaults to input path with .nda extension." }
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
            description: "Write or overwrite a file with specific content in the workspace. Creates folders if they do not exist.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"scripts/bootstrap.sh\")" },
                    "content": { "type": "string", "description": "The text content to write to the file." }
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
            name: "delete_file".to_string(),
            description: "Delete a file in the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"temp.txt\")" }
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
    ]
}
