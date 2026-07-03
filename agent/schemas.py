# agent/schemas.py
"""OpenAI-style tool schemas exposed to the model."""

TOOLS = [
    {
        "name": "read_file",
        "description": "Read file contents from the workspace. Supports pagination and optional line numbers.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to the workspace root."},
                "offset": {"type": "integer", "description": "Line index to start reading from (0-indexed). Defaults to 0."},
                "limit": {"type": "integer", "description": "Maximum number of lines to read. Defaults to 500."},
                "line_numbers": {"type": "boolean", "description": "Prefix each line with its 1-based line number. Defaults to false."}
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
        "name": "insert_file",
        "description": "Insert text immediately before or after the first occurrence of an anchor string.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to the workspace root."},
                "anchor": {"type": "string", "description": "Anchor text to locate."},
                "new": {"type": "string", "description": "Text to insert."},
                "after": {"type": "boolean", "description": "If true, insert after the anchor; otherwise before. Defaults to true."}
            },
            "required": ["path", "anchor", "new"]
        }
    },
    {
        "name": "delete_lines",
        "description": "Delete a 1-based inclusive range of lines from a file.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to the workspace root."},
                "start": {"type": "integer", "description": "First line to delete (1-based)."},
                "end": {"type": "integer", "description": "Last line to delete (1-based). If omitted, only start is deleted."}
            },
            "required": ["path", "start"]
        }
    },
    {
        "name": "apply_patch",
        "description": "Apply a unified diff or +/- block patch to a file.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to the workspace root."},
                "patch": {"type": "string", "description": "Patch text in unified diff or simple +/- format."}
            },
            "required": ["path", "patch"]
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
        "description": "Search for lines containing a literal substring in files matching a glob pattern.",
        "parameters": {
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Literal substring to search for."},
                "path": {"type": "string", "description": "Directory relative to workspace root to start the search. Defaults to '.'."},
                "glob": {"type": "string", "description": "Glob pattern for matching filenames. Defaults to '*'."}
            },
            "required": ["pattern"]
        }
    },
    {
        "name": "grep",
        "description": "Fast regex search across files. Uses ripgrep when available, otherwise Python regex.",
        "parameters": {
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regular expression pattern to search for."},
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
        "name": "file_tree",
        "description": "Return a recursive directory tree as a formatted string.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory path relative to the workspace root. Defaults to '.'."},
                "max_depth": {"type": "integer", "description": "Maximum recursion depth. Defaults to 5."}
            }
        }
    },
    {
        "name": "git_status",
        "description": "Retrieve the current git status of the workspace.",
        "parameters": {"type": "object", "properties": {}}
    },
    {
        "name": "git_diff",
        "description": "Retrieve the current uncommitted git diff of the workspace.",
        "parameters": {"type": "object", "properties": {}}
    },
    {
        "name": "git_branch",
        "description": "List local and remote git branches.",
        "parameters": {"type": "object", "properties": {}}
    },
    {
        "name": "git_checkout",
        "description": "Switch to a git branch, optionally creating it.",
        "parameters": {
            "type": "object",
            "properties": {
                "branch": {"type": "string", "description": "Branch name to switch to."},
                "create": {"type": "boolean", "description": "Create the branch if it does not exist. Defaults to false."}
            },
            "required": ["branch"]
        }
    },
    {
        "name": "git_log",
        "description": "Show recent git commits in one-line format.",
        "parameters": {
            "type": "object",
            "properties": {
                "n": {"type": "integer", "description": "Number of commits to show. Defaults to 10."}
            }
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
        "description": "Save or overwrite a markdown block in the memory directory under a given key.",
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
                "key": {"type": "string", "description": "Name/key of the memory block."}
            },
            "required": ["key"]
        }
    },
    {
        "name": "memory_append",
        "description": "Append content to an existing memory block (creates it if missing).",
        "parameters": {
            "type": "object",
            "properties": {
                "key": {"type": "string", "description": "Name/key of the memory block."},
                "content": {"type": "string", "description": "Markdown content to append."}
            },
            "required": ["key", "content"]
        }
    },
    {
        "name": "scratchpad_append",
        "description": "Append a timestamped entry to the scratchpad.",
        "parameters": {
            "type": "object",
            "properties": {
                "entry": {"type": "string", "description": "Text entry to append."}
            },
            "required": ["entry"]
        }
    },
    {
        "name": "todo_add",
        "description": "Add a new todo item to the backlog.",
        "parameters": {
            "type": "object",
            "properties": {
                "text": {"type": "string", "description": "Todo text."}
            },
            "required": ["text"]
        }
    },
    {
        "name": "todo_complete",
        "description": "Mark a todo item as complete by its 0-based index.",
        "parameters": {
            "type": "object",
            "properties": {
                "index": {"type": "integer", "description": "0-based index of the todo to complete."}
            },
            "required": ["index"]
        }
    },
    {
        "name": "todo_list",
        "description": "Return the current todo list as structured data.",
        "parameters": {"type": "object", "properties": {}}
    },
    {
        "name": "ask_user",
        "description": "Prompt the user for input and return their answer.",
        "parameters": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string", "description": "Question or prompt to show the user."}
            },
            "required": ["prompt"]
        }
    }
]
