# agent/tools.py
import subprocess
import json
import os
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent

def read_file(path: str, offset: int = 0, limit: int = 200) -> str:
    p = WORKSPACE / path
    lines = p.read_text(encoding="utf-8").splitlines()
    return "\n".join(lines[offset:offset+limit])

def write_file(path: str, content: str) -> dict:
    p = WORKSPACE / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    return {"ok": True, "path": str(p), "bytes": len(content)}

def edit_file(path: str, old: str, new: str) -> dict:
    p = WORKSPACE / path
    text = p.read_text(encoding="utf-8")
    if old not in text:
        return {"error": "old string not found"}
    p.write_text(text.replace(old, new, 1), encoding="utf-8")
    return {"ok": True, "path": str(p)}

def run_command(command: str, cwd: str = ".", timeout: int = 60) -> dict:
    result = subprocess.run(
        command, shell=True, cwd=WORKSPACE / cwd,
        capture_output=True, text=True, timeout=timeout,
        encoding="utf-8", errors="ignore"
    )
    return {
        "returncode": result.returncode,
        "stdout": result.stdout[-4000:],
        "stderr": result.stderr[-4000:],
    }

def search(pattern: str, path: str = ".", glob: str = "*") -> dict:
    matches = []
    root = WORKSPACE / path
    for p in root.rglob(glob):
        if p.is_file():
            try:
                for i, line in enumerate(p.read_text(encoding="utf-8", errors="ignore").splitlines(), 1):
                    if pattern in line:
                        matches.append(f"{p.relative_to(WORKSPACE)}:{i}: {line.strip()}")
            except Exception:
                pass
    return {"matches": matches[:50]}

def list_dir(path: str = ".") -> dict:
    p = WORKSPACE / path
    return {
        "entries": [
            {"name": e.name, "type": "dir" if e.is_dir() else "file"}
            for e in sorted(p.iterdir())
        ]
    }

def git_status() -> str:
    result = run_command("git status", timeout=10)
    return result.get("stdout", "") + "\n" + result.get("stderr", "")

def git_diff() -> str:
    result = run_command("git diff", timeout=10)
    return result.get("stdout", "") + "\n" + result.get("stderr", "")

def git_commit(message: str) -> dict:
    add_result = run_command("git add .", timeout=10)
    if add_result.get("returncode", 0) != 0:
        return {"error": "git add failed", "details": add_result}
    commit_result = run_command(f'git commit -m "{message}"', timeout=15)
    return {
        "ok": commit_result.get("returncode") == 0,
        "stdout": commit_result.get("stdout", ""),
        "stderr": commit_result.get("stderr", "")
    }

def memory_write(key: str, content: str) -> dict:
    if not key.endswith(".md"):
        key = f"{key}.md"
    p = WORKSPACE / "memory" / key
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    return {"ok": True, "path": str(p), "bytes": len(content)}

def memory_read(key: str) -> str:
    if not key.endswith(".md"):
        key = f"{key}.md"
    p = WORKSPACE / "memory" / key
    if not p.exists():
        return f"Error: memory key '{key}' not found."
    return p.read_text(encoding="utf-8")

def run_tool(name: str, args: dict) -> dict:
    import inspect
    try:
        func = None
        if name == "read_file":
            func = read_file
        elif name == "write_file":
            func = write_file
        elif name == "edit_file":
            func = edit_file
        elif name == "run_command":
            func = run_command
        elif name == "search":
            func = search
        elif name == "list_dir":
            func = list_dir
        elif name == "git_status":
            return {"status": git_status()}
        elif name == "git_diff":
            return {"diff": git_diff()}
        elif name == "git_commit":
            func = git_commit
        elif name == "memory_write":
            func = memory_write
        elif name == "memory_read":
            func = memory_read
        
        if func is None:
            return {"error": f"Tool '{name}' not found"}
            
        # Inspect function parameters and filter args to match
        sig = inspect.signature(func)
        filtered_args = {}
        for param_name, param in sig.parameters.items():
            if param.kind in (inspect.Parameter.POSITIONAL_OR_KEYWORD, inspect.Parameter.KEYWORD_ONLY):
                if param_name in args:
                    filtered_args[param_name] = args[param_name]
            elif param.kind == inspect.Parameter.VAR_KEYWORD:
                filtered_args.update(args)
                break
                
        # Call the target function with matching arguments
        result = func(**filtered_args)
        
        # Ensure result is formatted as dict/string for the model
        if isinstance(result, str):
            return {"content": result}
        return result
    except Exception as e:
        return {"error": str(e)}
