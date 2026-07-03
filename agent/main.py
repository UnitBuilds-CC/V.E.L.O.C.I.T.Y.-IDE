#!/usr/bin/env python3
# agent/main.py
import json
import os
import sys
from pathlib import Path

# Ensure agent/ is on sys.path so `from state import ...` works in tools.py
_AGENT_DIR = Path(__file__).resolve().parent
if str(_AGENT_DIR) not in sys.path:
    sys.path.insert(0, str(_AGENT_DIR))

from api import call_kimi
from tools import run_tool
from state import load_memory_block
from schemas import TOOLS


def safe_json_loads(raw: str) -> dict:
    """Parse a JSON string that may contain literal (unescaped) newlines inside
    string values — a common quirk in Kimi's native tool call output."""
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        pass
    import re
    sanitized = re.sub(
        r'(?<=[^\\])([\n\r\t])',
        lambda m: {"\n": "\\n", "\r": "\\r", "\t": "\\t"}[m.group(1)],
        raw
    )
    return json.loads(sanitized)


# ---------------------------------------------------------------------------
# TOOL_NAME_MAP — kept in sync with tools._NAME_ALIASES
# ---------------------------------------------------------------------------
TOOL_NAME_MAP = {
    "shell":              "run_command",
    "bash":               "run_command",
    "execute":            "run_command",
    "execute_command":    "run_command",
    "view_file":          "read_file",
    "open_file":          "read_file",
    "cat":                "read_file",
    "create_file":        "write_file",
    "save_file":          "write_file",
    "insert_content":     "write_file",
    "append_file":        "write_file",
    "patch_file":         "apply_patch",
    "str_replace":        "edit_file",
    "str_replace_editor": "edit_file",
    "replace":            "edit_file",
    "insert":             "insert_file",
    "delete":             "delete_lines",
    "rg":                 "grep",
    "find":               "search",
    "ls":                 "list_dir",
    "dir":                "list_dir",
    "tree":               "file_tree",
}

SYSTEM_PROMPT = """\
You are Kimi K2.7, an autonomous coding agent.
You operate inside a self-contained workspace. You have tools. Use them immediately.
CRITICAL: Keep reasoning extremely brief (1-2 sentences max). Long thoughts cause API timeouts.

## Tool call format
<tool>
{{"name": "tool_name", "args": {{"arg": "value"}}}}
</tool>

## Available tools
### Files
- read_file(path, offset=0, limit=500, line_numbers=false)
  → {{content, total_lines, has_more}}  — paginate with offset+=500 when has_more=true
- write_file(path, content)       → create or overwrite a file
- edit_file(path, old, new)       → replace first occurrence of `old` with `new`
- insert_file(path, anchor, new, after=true) → insert text before/after anchor
- delete_lines(path, start, end)  → delete 1-based inclusive line range
- apply_patch(path, patch)        → apply unified diff patch

### Search
- grep(pattern, path=".", glob="*")   → regex search (uses ripgrep if available)
- search(pattern, path=".", glob="*") → literal substring search
- file_tree(path=".", max_depth=5)    → directory tree
- list_dir(path=".")                  → flat directory listing

### Shell
- run_command(command, cwd=".", timeout=60)
  → {{returncode, stdout[first 8000 chars], stderr}}

### Git
- git_status() / git_diff() / git_log(n=10)
- git_branch() / git_checkout(branch, create=false)
- git_commit(message)

### Memory & state
- memory_write(key, content) / memory_read(key) / memory_append(key, content)
- scratchpad_append(entry)
- todo_add(text) / todo_complete(index) / todo_list()

### Interaction
- ask_user(prompt) → {{answer}}
"""


def _parse_tool_call(response: str) -> tuple[str, dict, bool]:
    """Extract (name, args, found) from an assistant response."""
    # 1. XML <tool> format
    if "<tool>" in response:
        try:
            block = response.split("<tool>")[1].split("</tool>")[0].strip()
            call = safe_json_loads(block)
            return call["name"], call.get("args", {}), True
        except Exception:
            pass

    # 2. Native <|tool_call_...|> format
    if "<|tool_call_argument_begin|>" in response:
        try:
            block = response.split("<|tool_call_argument_begin|>")[1].split("<|tool_call_end|>")[0].strip()
            payload = safe_json_loads(block)
            if "name" in payload:
                return payload["name"], payload.get("args", {}), True
            # Older format: name in prefix tag
            args = payload
            name_block = response.split("<|tool_call_begin|>")[1].split("<|tool_call_argument_begin|>")[0].strip()
            if name_block.startswith("functions."):
                name_block = name_block[10:]
            name = name_block.split(":")[0].strip()
            return name, args, True
        except Exception as e:
            print(f"Failed to parse native tool call: {e}")

    return "", {}, False


def main():
    instruction = sys.argv[1] if len(sys.argv) > 1 else input("Task: ")

    messages = [
        {"role": "system", "content": SYSTEM_PROMPT + "\n## Current memory\n" + load_memory_block()},
        {"role": "user",   "content": instruction},
    ]

    while True:
        try:
            print("\r[Kimi] Thinking...", end="", flush=True)
            response = call_kimi(messages, tools=TOOLS)
            print("\r\033[K", end="", flush=True)
        except Exception as e:
            print("\r\033[K", end="", flush=True)
            print(f"API Error: {e}", file=sys.stderr)
            break

        messages.append({"role": "assistant", "content": response})

        name, args, has_tool = _parse_tool_call(response)

        if has_tool and name:
            name = TOOL_NAME_MAP.get(name, name)
            try:
                result = run_tool(name, args)
                # Mirror the response format Kimi used
                if "<|tool_call_argument_begin|>" in response:
                    content_resp = (
                        f"<|tool_response_begin|><|tool_response_content_begin|>"
                        f"{json.dumps(result)}"
                        f"<|tool_response_content_end|><|tool_response_end|>"
                    )
                else:
                    content_resp = f"<result>{json.dumps(result)}</result>"

                messages.append({"role": "user", "content": content_resp})
                print(f"[{name}] {args}")
                print(f"-> {'Error: ' + result['error'] if 'error' in result else 'Success'}")
                continue
            except Exception as e:
                error_msg = f"Failed to execute tool call: {e}"
                messages.append({"role": "user", "content": f"<result>{json.dumps({'error': error_msg})}</result>"})
                print(f"-> {error_msg}")
                continue

        if response.strip().endswith("DONE"):
            break

        follow = input("Next (or 'done'): ")
        if follow.lower() == "done":
            break
        messages.append({"role": "user", "content": follow})


if __name__ == "__main__":
    main()
