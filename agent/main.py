#!/usr/bin/env python3
# agent/main.py
import json
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
    """Parse a JSON string that may contain literal unescaped newlines."""
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        pass
    import re
    sanitized = re.sub(
        r'(?<=[^\\])([\n\r\t])',
        lambda m: {"\n": "\\n", "\r": "\\r", "\t": "\\t"}[m.group(1)],
        raw,
    )
    return json.loads(sanitized)


# ---------------------------------------------------------------------------
# TOOL_NAME_MAP — kept in sync with tools.registry._aliases
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
    "py":                 "run_python",
    "python":             "run_python",
    "snapshot":           "checkpoint_save",
}

SYSTEM_PROMPT = """\
You are Kimi K2.7, an autonomous coding agent.
You are running DIRECTLY ON THE HOST MACHINE (not containerized). Be careful with destructive commands.
CRITICAL: Keep reasoning extremely brief (1-2 sentences max). Long thoughts cause API timeouts.
You may emit MULTIPLE <tool> blocks in a single response — all will be executed in order.

IMPORTANT: If a previous action failed, was interrupted, or if you detect a connection reset/hot-swap from the recent events log, always use `read_file` or check `git_diff`/`git_status` to verify that your last edited files were written fully and are not truncated or corrupted.

## Tool call format
<tool>
{"name": "tool_name", "args": {"arg": "value"}}
</tool>

## Available tools
### Files
- read_file(path, offset=0, limit=500, line_numbers=false)
  → {content, total_lines, has_more}  paginate with offset+=500 when has_more=true
- write_file(path, content)       → create or overwrite a file
- edit_file(path, old, new)       → replace first occurrence of `old` with `new`
- insert_file(path, anchor, new, after=true)
- delete_lines(path, start, end)
- apply_patch(path, patch)        → apply unified diff patch

### Search
- grep(pattern, path=".", glob="*")   → regex search (ripgrep if available)
- search(pattern, path=".", glob="*") → literal substring search
- file_tree(path=".", max_depth=5)
- list_dir(path=".")

### Shell & Python
- run_command(command, cwd=".", timeout=60)
  → {returncode, stdout[first 8000 chars], stderr}
- run_python(code, timeout=30) → {returncode, stdout, stderr}

### Git
- git_status() / git_diff() / git_log(n=10)
- git_branch() / git_checkout(branch, create=false)
- git_commit(message)

### Plans & checkpoints
- plan_read() / plan_write(content)
- checkpoint_save(name) → git tag or memory snapshot
- checkpoint_list()

### Memory & state
- memory_write(key, content) / memory_read(key) / memory_append(key, content)
- memory_list()
- scratchpad_append(entry)
- todo_add(text) / todo_complete(index) / todo_list()
- session_events(n=50)
- state_info()

### Interaction
- ask_user(prompt) → {answer}
- think(thought)   → logs thought to session, returns ok
"""


# ---------------------------------------------------------------------------
# Multi-call parser — extracts ALL tool calls from one response
# ---------------------------------------------------------------------------
def _parse_all_tool_calls(response: str) -> list[tuple[str, dict]]:
    """Return [(name, args), ...] for every tool call in the response."""
    calls: list[tuple[str, dict]] = []

    # 1. XML <tool>...</tool> blocks
    parts = response.split("<tool>")
    for part in parts[1:]:
        if "</tool>" not in part:
            continue
        block = part.split("</tool>")[0].strip()
        try:
            call = safe_json_loads(block)
            calls.append((call["name"], call.get("args", {})))
        except Exception:
            pass

    if calls:
        return calls

    # 2. Native <|tool_call_begin|>...<|tool_call_end|> blocks
    segments = response.split("<|tool_call_begin|>")
    for seg in segments[1:]:
        if "<|tool_call_argument_begin|>" not in seg:
            continue
        try:
            prefix   = seg.split("<|tool_call_argument_begin|>")[0].strip()
            raw_args = seg.split("<|tool_call_argument_begin|>")[1].split("<|tool_call_end|>")[0].strip()
            payload  = safe_json_loads(raw_args)
            if "name" in payload:
                calls.append((payload["name"], payload.get("args", {})))
            else:
                name_block = prefix
                if name_block.startswith("functions."):
                    name_block = name_block[10:]
                name = name_block.split(":")[0].strip()
                calls.append((name, payload))
        except Exception as e:
            print(f"[harness] Failed to parse native tool call segment: {e}")

    return calls


def safe_print_call(name: str, args: dict):
    """Safely print a tool call, truncating long arguments and handling encoding issues."""
    parts = []
    for k, v in args.items():
        v_str = str(v)
        if len(v_str) > 120:
            v_str = v_str[:120] + "..."
        parts.append(f"{k}={repr(v_str)}")
    text = f"[{name}] {', '.join(parts)}"
    try:
        # Encode to terminal encoding with backslashreplace to prevent crashes on Windows CP1252
        enc = sys.stdout.encoding or "utf-8"
        sys.stdout.buffer.write((text + "\n").encode(enc, errors="backslashreplace"))
        sys.stdout.flush()
    except Exception:
        # Fallback if standard streams don't support buffer write (e.g. testing or redirected)
        print(f"[{name}] (arguments too long or contain special chars)")


def main():
    instruction = sys.argv[1] if len(sys.argv) > 1 else input("Task: ")

    messages = [
        {"role": "system", "content": SYSTEM_PROMPT + "\n## Current memory\n" + load_memory_block()},
        {"role": "user",   "content": instruction},
    ]

    while True:
        try:
            print("[Kimi] Thinking...", flush=True)
            response = call_kimi(messages, tools=TOOLS)
        except Exception as e:
            print(f"API Error: {e}", file=sys.stderr, flush=True)
            break

        messages.append({"role": "assistant", "content": response})

        tool_calls = _parse_all_tool_calls(response)

        if tool_calls:
            all_results = []
            for raw_name, args in tool_calls:
                name = TOOL_NAME_MAP.get(raw_name, raw_name)
                try:
                    # Print before calling so the user sees immediate action
                    print(f"[Kimi] Calling tool '{name}'...", flush=True)
                    result = run_tool(name, args)
                    safe_print_call(name, args)
                    print(f"-> {'Error: ' + result['error'] if 'error' in result else 'Success'}", flush=True)
                except Exception as e:
                    result = {"error": f"Failed to execute '{name}': {e}"}
                    print(f"-> {result['error']}")
                all_results.append({"tool": name, "result": result})

            if "<|tool_call_argument_begin|>" in response:
                content_resp = (
                    f"<|tool_response_begin|><|tool_response_content_begin|>"
                    f"{json.dumps(all_results)}"
                    f"<|tool_response_content_end|><|tool_response_end|>"
                )
            else:
                content_resp = f"<result>{json.dumps(all_results)}</result>"

            messages.append({"role": "user", "content": content_resp})
            continue

        if response.strip().endswith("DONE"):
            break

        follow = input("Next (or 'done'): ")
        if follow.lower() == "done":
            break
        messages.append({"role": "user", "content": follow})


if __name__ == "__main__":
    main()
