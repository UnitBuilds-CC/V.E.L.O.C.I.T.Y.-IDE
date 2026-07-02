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
        call = None
        
        if "<tool>" in response:
            try:
                block = response.split("<tool>")[1].split("</tool>")[0].strip()
                call = json.loads(block)
                has_tool = True
            except Exception as e:
                print(f"Failed to parse XML tool call: {e}")
        elif "<|tool_call_argument_begin|>" in response:
            try:
                block = response.split("<|tool_call_argument_begin|>")[1].split("<|tool_call_end|>")[0].strip()
                call = json.loads(block)
                has_tool = True
            except Exception as e:
                print(f"Failed to parse native tool call: {e}")

        if has_tool and call:
            try:
                name = call["name"]
                args = call.get("args", {})
                
                # Map native tool names to workspace tools
                if name == "shell":
                    name = "run_command"
                elif name == "view_file":
                    name = "read_file"
                    
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
