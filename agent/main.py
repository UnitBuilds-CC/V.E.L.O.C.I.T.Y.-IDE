#!/usr/bin/env python3
# agent/main.py
import json
import os
import sys
from pathlib import Path
from api import call_kimi
from tools import run_tool
from schemas import TOOLS

WORKSPACE = Path(__file__).resolve().parent.parent
MEMORY = WORKSPACE / "memory"

def safe_json_loads(raw: str) -> dict:
    """Parse a JSON string that may contain literal (unescaped) newlines inside
    string values — a common quirk in Kimi's native tool call output."""
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        pass
    # Escape literal control characters that appear inside JSON string values.
    # Strategy: walk character by character tracking whether we are inside a
    # JSON string and escape bare newlines / carriage returns found there.
    import re
    sanitized = re.sub(
        r'(?<=[^\\])([\n\r\t])',   # unescaped newline/cr/tab
        lambda m: {"\n": "\\n", "\r": "\\r", "\t": "\\t"}[m.group(1)],
        raw
    )
    return json.loads(sanitized)

def load_memory():
    blocks = []
    for name in ["project.md", "scratchpad.md", "todos.md"]:
        p = MEMORY / name
        if p.exists():
            blocks.append(f"--- {name} ---\n{p.read_text(encoding='utf-8')}\n")
    return "\n".join(blocks)

def main():
    instruction = sys.argv[1] if len(sys.argv) > 1 else input("Task: ")

    messages = [
        {"role": "system", "content": f"""You are Kimi K2.7, an autonomous coding agent.
You operate inside a self-contained workspace. You have tools. Use them.
CRITICAL: Keep your internal reasoning extremely brief (under 2 sentences) and act immediately. Large thoughts cause Cloudflare API timeouts.

To call a tool, use the XML format inside a markdown block or raw text:
<tool>
{{"name": "tool_name", "args": {{"arg_name": "arg_value"}}}}
</tool>

Current memory:
{load_memory()}
"""},
        {"role": "user", "content": instruction},
    ]

    while True:
        try:
            print("\r[Kimi] Thinking...", end="", flush=True)
            response = call_kimi(messages, tools=TOOLS)
            print("\r\033[K", end="", flush=True)  # Clear the "Thinking..." line
        except Exception as e:
            print("\r\033[K", end="", flush=True)
            print(f"API Error: {e}", file=sys.stderr)
            break

        messages.append({"role": "assistant", "content": response})

        # Parse tool calls from simple XML format or native formatting
        has_tool = False
        name = ""
        args = {}
        
        # 1. Try parsing XML format first
        if "<tool>" in response:
            try:
                block = response.split("<tool>")[1].split("</tool>")[0].strip()
                call = safe_json_loads(block)
                name = call["name"]
                args = call.get("args", {})
                has_tool = True
            except Exception:
                # If XML parsing fails (e.g. Kimi was discussing '<tool>' in thought block),
                # we do not set has_tool and will try parsing native format below.
                pass
                
        # 2. Try parsing native format if XML was not found or failed to parse
        if not has_tool and "<|tool_call_argument_begin|>" in response:
            try:
                # Parse args block
                block = response.split("<|tool_call_argument_begin|>")[1].split("<|tool_call_end|>")[0].strip()
                payload = safe_json_loads(block)

                # Kimi may embed the tool name inside the JSON body:
                # {"name": "shell", "args": {...}}
                # In that case the prefix tag holds a random call-ID (e.g. toolu_01X5m4a7n2m1...)
                if "name" in payload:
                    name = payload["name"]
                    args = payload.get("args", {})
                else:
                    # Older format: name is in the prefix tag, JSON IS the args
                    args = payload
                    name_block = response.split("<|tool_call_begin|>")[1].split("<|tool_call_argument_begin|>")[0].strip()
                    if name_block.startswith("functions."):
                        name_block = name_block[10:]
                    name = name_block.split(":")[0].strip()

                has_tool = True
            except Exception as e:
                print(f"Failed to parse native tool call: {e}")

        # Normalise all known tool name aliases to actual tool function names
        TOOL_NAME_MAP = {
            # shell execution
            "shell":           "run_command",
            "bash":            "run_command",
            "execute":         "run_command",
            "execute_command":  "run_command",
            # reading
            "view_file":       "read_file",
            "open_file":       "read_file",
            "cat":             "read_file",
            # writing / creating  (write_file exists in tools.py)
            "create_file":     "write_file",
            "save_file":       "write_file",
            "insert_content":  "write_file",
            "append_file":     "write_file",
            # editing
            "patch_file":      "edit_file",
            "str_replace":     "edit_file",
            "str_replace_editor": "edit_file",
            "replace":         "edit_file",
        }

        if has_tool and name:
            name = TOOL_NAME_MAP.get(name, name)
            try:
                result = run_tool(name, args)
                
                # Format the response back to Kimi in its native format or standard format
                if "<|tool_call_argument_begin|>" in response:
                    content_resp = f"<|tool_response_begin|><|tool_response_content_begin|>{json.dumps(result)}<|tool_response_content_end|><|tool_response_end|>"
                else:
                    content_resp = f"<result>{json.dumps(result)}</result>"
                    
                messages.append({"role": "user", "content": content_resp})
                print(f"[{name}] {args}")
                if "error" in result:
                    print(f"-> Error: {result['error']}")
                else:
                    print("-> Success")
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
