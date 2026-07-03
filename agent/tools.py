# agent/tools.py
"""Core tool implementations for the V.E.L.O.C.I.T.Y. agent workspace."""
from __future__ import annotations

import inspect
import json
import os
import re
import shutil
import subprocess
import sys
from difflib import unified_diff
from pathlib import Path
from typing import Any

from state import (
    WORKSPACE,
    append_memory,
    append_scratchpad,
    load_memory_block,
    load_scratchpad,
    log_event,
    read_memory,
    write_memory,
)


# ---------------------------------------------------------------------------
# Path helpers
# ---------------------------------------------------------------------------
def _resolve_path(path: str) -> Path:
    p = Path(path)
    return p if p.is_absolute() else WORKSPACE / p


# ---------------------------------------------------------------------------
# File operations
# ---------------------------------------------------------------------------
def read_file(path: str, offset: int = 0, limit: int = 500, line_numbers: bool = False) -> dict:
    """Read a file with optional pagination and line numbers."""
    p = _resolve_path(path)
    if not p.exists():
        return {"error": f"File not found: {p}"}
    try:
        text = p.read_text(encoding="utf-8", errors="replace")
    except Exception as exc:
        return {"error": f"Could not read {p}: {exc}"}

    lines = text.splitlines()
    chunk = lines[offset:offset + limit]
    if line_numbers:
        chunk = [f"{offset + i + 1:4d} | {line}" for i, line in enumerate(chunk)]

    return {
        "content": "\n".join(chunk),
        "total_lines": len(lines),
        "offset": offset,
        "limit": limit,
        "has_more": (offset + limit) < len(lines),
    }


def write_file(path: str, content: str) -> dict:
    """Write or overwrite a file, creating parent directories as needed."""
    p = _resolve_path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    return {"ok": True, "path": str(p), "bytes": len(content)}


def edit_file(path: str, old: str, new: str) -> dict:
    """Replace the first occurrence of `old` with `new` in a file."""
    p = _resolve_path(path)
    if not p.exists():
        return {"error": f"File not found: {p}"}
    text = p.read_text(encoding="utf-8", errors="replace")
    if old not in text:
        return {"error": "old string not found"}
    p.write_text(text.replace(old, new, 1), encoding="utf-8")
    return {"ok": True, "path": str(p)}


def insert_file(path: str, anchor: str, new: str, after: bool = True) -> dict:
    """Insert `new` text immediately before or after the first `anchor`."""
    p = _resolve_path(path)
    if not p.exists():
        return {"error": f"File not found: {p}"}
    text = p.read_text(encoding="utf-8", errors="replace")
    if anchor not in text:
        return {"error": "anchor string not found"}
    idx = text.index(anchor)
    if after:
        idx += len(anchor)
    updated = text[:idx] + new + text[idx:]
    p.write_text(updated, encoding="utf-8")
    return {"ok": True, "path": str(p)}


def delete_lines(path: str, start: int, end: int | None = None) -> dict:
    """Delete 1-based inclusive line range [start, end]."""
    p = _resolve_path(path)
    if not p.exists():
        return {"error": f"File not found: {p}"}
    lines = p.read_text(encoding="utf-8", errors="replace").splitlines()
    end = end if end is not None else start
    if start < 1 or end > len(lines) or start > end:
        return {"error": f"Invalid range {start}-{end} for file with {len(lines)} lines"}
    updated = lines[: start - 1] + lines[end:]
    p.write_text("\n".join(updated) + ("\n" if updated and lines[-1] == "" else ""), encoding="utf-8")
    return {"ok": True, "path": str(p), "deleted": end - start + 1}


def apply_patch(path: str, patch: str) -> dict:
    """Apply a unified-diff style patch to a file.

    Supports patches produced by `diff -u` or the model's own block format:
        --- old
        +++ new
        @@ ... @@
         context
        -removed
        +added
    """
    p = _resolve_path(path)
    if not p.exists():
        return {"error": f"File not found: {p}"}

    original = p.read_text(encoding="utf-8", errors="replace")
    lines = original.splitlines()

    # Strip optional diff headers
    patch_lines = patch.splitlines()
    while patch_lines and (
        patch_lines[0].startswith("---")
        or patch_lines[0].startswith("+++")
        or patch_lines[0].startswith("@@")
        or patch_lines[0].strip() == ""
    ):
        if patch_lines[0].startswith("@@"):
            break
        patch_lines.pop(0)

    # If the patch still starts with a hunk header, parse it; otherwise treat
    # every line prefixed with +/- as a simple line-based replacement.
    if patch_lines and patch_lines[0].startswith("@@"):
        try:
            return _apply_unified_patch(p, original, patch)
        except Exception as exc:
            return {"error": f"Could not apply unified patch: {exc}"}

    # Simple +/- block replacement
    old_block: list[str] = []
    new_block: list[str] = []
    for line in patch_lines:
        if line.startswith("-"):
            old_block.append(line[1:])
        elif line.startswith("+"):
            new_block.append(line[1:])
        elif line.startswith(" "):
            old_block.append(line[1:])
            new_block.append(line[1:])

    old_text = "\n".join(old_block)
    if old_text not in original:
        return {"error": "Patch context not found in file"}
    p.write_text(original.replace(old_text, "\n".join(new_block), 1), encoding="utf-8")
    return {"ok": True, "path": str(p)}


def _apply_unified_patch(p: Path, original: str, patch: str) -> dict:
    """Minimal unified-diff applier."""
    lines = original.splitlines()
    patch_lines = patch.splitlines()
    result: list[str] = []
    i = 0
    hunk_idx = 0
    while hunk_idx < len(patch_lines):
        line = patch_lines[hunk_idx]
        if not line.startswith("@@"):
            hunk_idx += 1
            continue
        m = re.match(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@", line)
        if not m:
            raise ValueError(f"Bad hunk header: {line}")
        old_start = int(m.group(1)) - 1
        old_count = int(m.group(2)) if m.group(2) else 1
        hunk_idx += 1

        result.extend(lines[i:old_start])
        old_consumed = 0
        new_hunk: list[str] = []
        while hunk_idx < len(patch_lines) and not patch_lines[hunk_idx].startswith("@@"):
            pl = patch_lines[hunk_idx]
            if pl.startswith("-"):
                if old_start + old_consumed >= len(lines) or lines[old_start + old_consumed] != pl[1:]:
                    raise ValueError(f"Context mismatch at line {old_start + old_consumed + 1}")
                old_consumed += 1
            elif pl.startswith("+"):
                new_hunk.append(pl[1:])
            elif pl.startswith(" "):
                if old_start + old_consumed >= len(lines) or lines[old_start + old_consumed] != pl[1:]:
                    raise ValueError(f"Context mismatch at line {old_start + old_consumed + 1}")
                new_hunk.append(pl[1:])
                old_consumed += 1
            hunk_idx += 1

        if old_consumed != old_count:
            raise ValueError(f"Hunk expected {old_count} old lines, got {old_consumed}")
        result.extend(new_hunk)
        i = old_start + old_consumed

    result.extend(lines[i:])
    p.write_text("\n".join(result) + ("\n" if original.endswith("\n") else ""), encoding="utf-8")
    return {"ok": True, "path": str(p)}


# ---------------------------------------------------------------------------
# Search & navigation
# ---------------------------------------------------------------------------
def grep(pattern: str, path: str = ".", glob: str = "*") -> dict:
    """Fast regex search using ripgrep when available, otherwise Python."""
    root = _resolve_path(path)
    if shutil.which("rg"):
        cmd = ["rg", "--line-number", "--glob", glob, "-e", pattern, str(root)]
        try:
            proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="ignore", timeout=30)
            matches = [
                line.strip()
                for line in proc.stdout.splitlines()
                if line.strip()
            ][:100]
            return {"matches": matches, "engine": "ripgrep"}
        except Exception:
            pass

    matches = []
    prog = re.compile(pattern)
    for p in root.rglob(glob):
        if p.is_file():
            try:
                text = p.read_text(encoding="utf-8", errors="ignore")
                for i, line in enumerate(text.splitlines(), 1):
                    if prog.search(line):
                        rel = p.relative_to(WORKSPACE)
                        matches.append(f"{rel}:{i}: {line.strip()}")
                        if len(matches) >= 100:
                            return {"matches": matches, "engine": "python"}
            except Exception:
                pass
    return {"matches": matches, "engine": "python"}


def search(pattern: str, path: str = ".", glob: str = "*") -> dict:
    """Literal substring search (kept for backwards compatibility)."""
    root = _resolve_path(path)
    matches = []
    for p in root.rglob(glob):
        if p.is_file():
            try:
                text = p.read_text(encoding="utf-8", errors="ignore")
                for i, line in enumerate(text.splitlines(), 1):
                    if pattern in line:
                        rel = p.relative_to(WORKSPACE)
                        matches.append(f"{rel}:{i}: {line.strip()}")
                        if len(matches) >= 100:
                            return {"matches": matches}
            except Exception:
                pass
    return {"matches": matches}


def file_tree(path: str = ".", max_depth: int = 5) -> dict:
    """Return a recursive directory tree as a list of indented strings."""
    root = _resolve_path(path)
    lines: list[str] = [str(root.relative_to(WORKSPACE) if root != WORKSPACE else ".")]

    def walk(current: Path, prefix: str, depth: int) -> None:
        if depth > max_depth:
            return
        try:
            entries = sorted(current.iterdir(), key=lambda e: (not e.is_dir(), e.name.lower()))
        except PermissionError:
            return
        for idx, entry in enumerate(entries):
            if entry.name.startswith(".") and entry.name not in {".env", ".gitignore"}:
                continue
            is_last = idx == len(entries) - 1
            branch = "└── " if is_last else "├── "
            lines.append(f"{prefix}{branch}{entry.name}")
            if entry.is_dir():
                extension = "    " if is_last else "│   "
                walk(entry, prefix + extension, depth + 1)

    walk(root, "", 1)
    return {"tree": "\n".join(lines)}


def list_dir(path: str = ".") -> dict:
    p = _resolve_path(path)
    return {
        "entries": [
            {"name": e.name, "type": "dir" if e.is_dir() else "file"}
            for e in sorted(p.iterdir(), key=lambda e: (not e.is_dir(), e.name.lower()))
        ]
    }


# ---------------------------------------------------------------------------
# Shell execution
# ---------------------------------------------------------------------------
def run_command(command: str, cwd: str = ".", timeout: int = 60) -> dict:
    """Run a shell command inside the workspace."""
    result = subprocess.run(
        command,
        shell=True,
        cwd=_resolve_path(cwd),
        capture_output=True,
        text=True,
        timeout=timeout,
        encoding="utf-8",
        errors="ignore",
    )
    stdout = result.stdout
    stderr = result.stderr
    return {
        "returncode": result.returncode,
        "stdout": stdout[:8000] + ("...truncated" if len(stdout) > 8000 else ""),
        "stdout_bytes": len(stdout),
        "stderr": stderr[-2000:],
    }


# ---------------------------------------------------------------------------
# Git helpers
# ---------------------------------------------------------------------------
def _git(cmd: str, timeout: int = 15) -> dict:
    return run_command(f"git {cmd}", timeout=timeout)


def git_status() -> dict:
    r = _git("status", timeout=10)
    return {"status": r.get("stdout", "") + "\n" + r.get("stderr", "")}


def git_diff() -> dict:
    r = _git("diff", timeout=10)
    return {"diff": r.get("stdout", "") + "\n" + r.get("stderr", "")}


def git_branch() -> dict:
    r = _git("branch -a", timeout=10)
    return {"branches": r.get("stdout", "").splitlines()}


def git_checkout(branch: str, create: bool = False) -> dict:
    flag = "-b" if create else ""
    r = _git(f"checkout {flag} {branch}".strip(), timeout=15)
    return {
        "ok": r.get("returncode") == 0,
        "stdout": r.get("stdout", ""),
        "stderr": r.get("stderr", ""),
    }


def git_log(n: int = 10) -> dict:
    r = _git(f'log --oneline -n {n}', timeout=10)
    return {"log": r.get("stdout", "").splitlines()}


def git_commit(message: str) -> dict:
    add_result = _git("add .", timeout=10)
    if add_result.get("returncode", 0) != 0:
        return {"error": "git add failed", "details": add_result}
    commit_result = _git(f'commit -m "{message}"', timeout=15)
    return {
        "ok": commit_result.get("returncode") == 0,
        "stdout": commit_result.get("stdout", ""),
        "stderr": commit_result.get("stderr", ""),
    }


# ---------------------------------------------------------------------------
# Memory / state tools
# ---------------------------------------------------------------------------
def memory_write(key: str, content: str) -> dict:
    return write_memory(key, content)


def memory_read(key: str) -> dict:
    return {"content": read_memory(key)}


def memory_append(key: str, content: str) -> dict:
    return append_memory(key, content)


def scratchpad_append(entry: str) -> dict:
    return append_scratchpad(entry)


def todo_add(text: str) -> dict:
    from state import add_todo
    return add_todo(text)


def todo_complete(index: int) -> dict:
    from state import complete_todo
    return complete_todo(index)


def todo_list() -> dict:
    from state import load_todos
    return {"todos": load_todos()}


# ---------------------------------------------------------------------------
# User interaction
# ---------------------------------------------------------------------------
def ask_user(prompt: str) -> dict:
    try:
        answer = input(f"{prompt}\n> ")
    except EOFError:
        answer = ""
    return {"answer": answer}


# ---------------------------------------------------------------------------
# Tool dispatcher
# ---------------------------------------------------------------------------
_NAME_ALIASES = {
    "shell": "run_command",
    "bash": "run_command",
    "execute": "run_command",
    "execute_command": "run_command",
    "view_file": "read_file",
    "open_file": "read_file",
    "cat": "read_file",
    "create_file": "write_file",
    "save_file": "write_file",
    "insert_content": "write_file",
    "append_file": "write_file",
    "patch_file": "apply_patch",
    "str_replace": "edit_file",
    "str_replace_editor": "edit_file",
    "replace": "edit_file",
    "insert": "insert_file",
    "delete": "delete_lines",
    "rg": "grep",
    "find": "search",
    "ls": "list_dir",
    "dir": "list_dir",
    "tree": "file_tree",
}


def _normalise_args(args: dict, name: str) -> dict:
    """Map common alternative argument names to the ones our functions expect."""
    args = dict(args)
    if "file_path" in args:
        args["path"] = args.pop("file_path")
    elif "filepath" in args:
        args["path"] = args.pop("filepath")
    if "old_string" in args:
        args["old"] = args.pop("old_string")
    if "new_string" in args:
        args["new"] = args.pop("new_string")
    if "text" in args and name == "write_file":
        args["content"] = args.pop("text")
    if "content" in args and name == "memory_append":
        args["content"] = args["content"]
    return args


def run_tool(name: str, args: dict) -> dict:
    import inspect

    name = _NAME_ALIASES.get(name, name)
    args = _normalise_args(args, name)

    func: Any | None = None
    if name == "read_file":
        func = read_file
    elif name == "write_file":
        func = write_file
    elif name == "edit_file":
        func = edit_file
    elif name == "insert_file":
        func = insert_file
    elif name == "delete_lines":
        func = delete_lines
    elif name == "apply_patch":
        func = apply_patch
    elif name == "run_command":
        func = run_command
    elif name == "search":
        func = search
    elif name == "grep":
        func = grep
    elif name == "list_dir":
        func = list_dir
    elif name == "file_tree":
        func = file_tree
    elif name == "git_status":
        return git_status()
    elif name == "git_diff":
        return git_diff()
    elif name == "git_branch":
        return git_branch()
    elif name == "git_checkout":
        func = git_checkout
    elif name == "git_log":
        func = git_log
    elif name == "git_commit":
        func = git_commit
    elif name == "memory_write":
        func = memory_write
    elif name == "memory_read":
        func = memory_read
    elif name == "memory_append":
        func = memory_append
    elif name == "scratchpad_append":
        func = scratchpad_append
    elif name == "todo_add":
        func = todo_add
    elif name == "todo_complete":
        func = todo_complete
    elif name == "todo_list":
        func = todo_list
    elif name == "ask_user":
        func = ask_user

    if func is None:
        return {"error": f"Tool '{name}' not found"}

    sig = inspect.signature(func)
    filtered_args: dict[str, Any] = {}
    for param_name, param in sig.parameters.items():
        if param.kind in (inspect.Parameter.POSITIONAL_OR_KEYWORD, inspect.Parameter.KEYWORD_ONLY):
            if param_name in args:
                filtered_args[param_name] = args[param_name]
        elif param.kind == inspect.Parameter.VAR_KEYWORD:
            filtered_args.update(args)
            break

    try:
        result = func(**filtered_args)
    except Exception as exc:
        return {"error": str(exc)}

    if isinstance(result, str):
        return {"content": result}
    return result
