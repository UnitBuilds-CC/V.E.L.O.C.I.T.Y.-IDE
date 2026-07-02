# agent/schemas.py

TOOLS = [
    {
        "name": "read_file",
        "description": "Read file contents from the workspace. Returns up to 'limit' lines starting from 'offset'.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to the workspace root."},
                "offset": {"type": "integer", "description": "Line index to start reading from (0-indexed). Defaults to 0."},
                "limit": {"type": "integer", "description": "Maximum number of lines to read. Defaults to 200."}
            },
            "required": ["path"]
        }
    },
    {
        "name": "write_file",
        "description": "Write or overwrite a file with the given content. Creates parent directories if they don't exist.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to the workspace root."},
                "content": {"type": "string", "description": "Complete text content to write to the file."}
            },
            "required": ["path", "content"]
        }
    },
    {
        "name": "edit_file",
        "description": "Replace a single unique occurrence of 'old' text with 'new' text in a file.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to the workspace root."},
                "old": {"type": "string", "description": "Exact text content to find and replace."},
                "new": {"type": "string", "description": "New text content to replace the old text with."}
            },
            "required": ["path", "old", "new"]
        }
    },
    {
        "name": "run_command",
        "description": "Run a shell command inside the workspace. Returns stdout, stderr, and returncode.",
        "parameters": {
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "The exact shell command to run."},
                "cwd": {"type": "string", "description": "Directory relative to workspace root to run the command in. Defaults to '.'."},
                "timeout": {"type": "integer", "description": "Command timeout in seconds. Defaults to 60."}
            },
            "required": ["command"]
        }
    },
    {
        "name": "search",
        "description": "Search for lines matching a pattern in files matching a glob pattern.",
        "parameters": {
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Substring pattern to search for."},
                "path": {"type": "string", "description": "Directory relative to workspace root to start the search. Defaults to '.'."},
                "glob": {"type": "string", "description": "Glob pattern for matching filenames. Defaults to '*'."}
            },
            "required": ["pattern"]
        }
    },
    {
        "name": "list_dir",
        "description": "List all files and folders in the given workspace directory.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory path relative to the workspace root. Defaults to '.'."}
            }
        }
    },
    {
        "name": "git_status",
        "description": "Retrieve the current git status of the workspace.",
        "parameters": {
            "type": "object",
            "properties": {}
        }
    },
    {
        "name": "git_diff",
        "description": "Retrieve the current uncommitted git diff of the workspace.",
        "parameters": {
            "type": "object",
            "properties": {}
        }
    },
    {
        "name": "git_commit",
        "description": "Stages all workspace changes (git add .) and commits them with the given commit message.",
        "parameters": {
            "type": "object",
            "properties": {
                "message": {"type": "string", "description": "Commit message describing the changes."}
            },
            "required": ["message"]
        }
    },
    {
        "name": "memory_write",
        "description": "Save or update a markdown block in the memory directory under a given key.",
        "parameters": {
            "type": "object",
            "properties": {
                "key": {"type": "string", "description": "Name/key of the memory block (e.g. 'scratchpad' or 'todos')."},
                "content": {"type": "string", "description": "Markdown content to persist."}
            },
            "required": ["key", "content"]
        }
    },
    {
        "name": "memory_read",
        "description": "Read a saved markdown block from the memory directory by its key.",
        "parameters": {
            "type": "object",
            "properties": {
                "key": {"type": "string", "description": "Name/key of the memory block (e.g. 'scratchpad' or 'todos')."}
            },
            "required": ["key"]
        }
    }
]
