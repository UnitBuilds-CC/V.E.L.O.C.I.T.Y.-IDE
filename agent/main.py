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
Always reason briefly, then act. When done, reply exactly with: DONE.

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
            response = call_kimi(messages, tools=TOOLS)
        except Exception as e:
            print(f"API Error: {e}", file=sys.stderr)
            break

        messages.append({"role": "assistant", "content": response})

        # Parse tool calls from simple XML format
        if "<tool>" in response:
            try:
                block = response.split("<tool>")[1].split("</tool>")[0].strip()
                call = json.loads(block)
                result = run_tool(call["name"], call.get("args", {}))
                messages.append({"role": "user", "content": f"<result>{json.dumps(result)}</result>"})
                print(f"[{call['name']}] {call.get('args', {})}")
                # Print result status
                if "error" in result:
                    print(f"-> Error: {result['error']}")
                else:
                    print("-> Success")
                continue
            except Exception as e:
                error_msg = f"Failed to parse or execute tool call: {e}"
                messages.append({"role": "user", "content": f"<result>{json.dumps({'error': error_msg})}</result>"})
                print(f"-> {error_msg}")
                continue

        if response.strip().endswith("DONE"):
            print(response)
            break

        print(response)
        follow = input("Next (or 'done'): ")
        if follow.lower() == "done":
            break
        messages.append({"role": "user", "content": follow})

if __name__ == "__main__":
    main()
