# agent/tools.py
"""Core tool implementations for the V.E.L.O.C.I.T.Y. agent workspace.

Tools are registered in a central registry so the dispatcher and the model
schemas are always derived from the same source of truth.
"""
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
from typing import Any, Callable

from state import (
    WORKSPACE,
    EXECUTION_MODE,
    append_memory,
    append_scratchpad,
    complete_todo,
    add_todo,
    load_plan,
    load_session_events,
    load_todos,
    log_event,
    read_memory,
    save_plan,
    snapshot_memory,
    list_snapshots,
    list_memory,
    workspace_info,
    write_memory,
)

# Optional tree-sitter AST support (graceful fallback if not installed)
try:
    from tree_sitter import Language, Parser
    from tree_sitter_python import language as PYTHON_LANGUAGE
    _TS_AVAILABLE = True
except Exception:
    _TS_AVAILABLE = False


# ---------------------------------------------------------------------------
# Tool registry
# ---------------------------------------------------------------------------
class ToolRegistry:
    """Lightweight registry that maps canonical tool names (and aliases) to
    functions and can derive OpenAI-style JSON schemas from type hints."""

    def __init__(self) -> None:
        self._tools: dict[str, Callable] = {}
        self._aliases: dict[str, str] = {}

    def register(
        self,
        name: str | None = None,
        aliases: list[str] | None = None,
    ) -> Callable:
        """Decorator that registers a function as a tool."""
        def decorator(func: Callable) -> Callable:
            canonical = name or func.__name__
            self._tools[canonical] = func
            for alias in (aliases or []):
                self._aliases[alias] = canonical
            return func
        return decorator

    def get(self, name: str) -> Callable | None:
        canonical = self._aliases.get(name, name)
        return self._tools.get(canonical)

    def names(self) -> list[str]:
        return list(self._tools.keys())

    @property
    def aliases(self) -> dict[str, str]:
        return dict(self._aliases)

    def _python_type_to_json(self, value: Any) -> tuple[str, list[str] | None]:
        """Map a Python value/annotation to a JSON-schema type."""
        if isinstance(value, bool):
            return "boolean", None
        if isinstance(value, int):
            return "integer", None
        if isinstance(value, str):
            return "string", None
        if isinstance(value, list):
            return "array", None
        if isinstance(value, dict):
            return "object", None
        return "string", None

    def schema_for(self, name: str) -> dict | None:
        func = self._tools.get(name)
        if func is None:
            return None
        sig = inspect.signature(func)
        properties: dict[str, dict] = {}
        required: list[str] = []
        for param_name, param in sig.parameters.items():
            if param.kind in (inspect.Parameter.VAR_POSITIONAL, inspect.Parameter.VAR_KEYWORD):
                continue
            default = param.default
            if default is inspect.Parameter.empty:
                required.append(param_name)
                json_type, enum = self._python_type_to_json("")
            else:
                json_type, enum = self._python_type_to_json(default)
            prop: dict = {"type": json_type}
            if enum is not None:
                prop["enum"] = enum
            properties[param_name] = prop

        description = (func.__doc__ or "").split("\n")[0].strip()
        return {
            "name": name,
            "description": description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": required,
            },
        }

    def schemas(self) -> list[dict]:
        return [self.schema_for(name) for name in sorted(self._tools.keys())]


registry = ToolRegistry()


# ---------------------------------------------------------------------------
# Path helpers
# ---------------------------------------------------------------------------
def _resolve_path(path: str) -> Path:
    p = Path(path)
    return p if p.is_absolute() else WORKSPACE / p


# ---------------------------------------------------------------------------
# File operations
# ---------------------------------------------------------------------------
@registry.register(aliases=["view_file", "open_file", "cat"])
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


@registry.register(aliases=["create_file", "save_file", "insert_content", "append_file"])
def write_file(path: str, content: str) -> dict:
    """Write or overwrite a file, creating parent directories as needed."""
    p = _resolve_path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    return {"ok": True, "path": str(p), "bytes": len(content)}


@registry.register(aliases=["str_replace", "str_replace_editor", "replace"])
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


@registry.register(aliases=["insert"])
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


@registry.register(aliases=["delete"])
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


@registry.register(aliases=["patch_file"])
def apply_patch(path: str, patch: str) -> dict:
    """Apply a unified-diff style patch to a file."""
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
@registry.register(aliases=["rg"])
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


@registry.register(aliases=["find"])
def search(pattern: str, path: str = ".", glob: str = "*") -> dict:
    """Literal substring search across files."""
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


@registry.register(aliases=["tree"])
def file_tree(path: str = ".", max_depth: int = 5) -> dict:
    """Return a recursive directory tree as a formatted string."""
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


@registry.register(aliases=["ls", "dir"])
def list_dir(path: str = ".") -> dict:
    """List all files and folders in the given workspace directory."""
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
@registry.register(aliases=["shell", "bash", "execute", "execute_command"])
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
        "mode": EXECUTION_MODE,
    }


# ---------------------------------------------------------------------------
# Python execution
# ---------------------------------------------------------------------------
@registry.register(aliases=["py", "python"])
def run_python(code: str, timeout: int = 30) -> dict:
    """Execute a Python snippet in a fresh subprocess and return its output."""
    proc = subprocess.run(
        [sys.executable, "-c", code],
        cwd=WORKSPACE,
        capture_output=True,
        text=True,
        timeout=timeout,
        encoding="utf-8",
        errors="ignore",
    )
    return {
        "returncode": proc.returncode,
        "stdout": proc.stdout[:8000] + ("...truncated" if len(proc.stdout) > 8000 else ""),
        "stderr": proc.stderr[-2000:],
    }


# ---------------------------------------------------------------------------
# Git helpers
# ---------------------------------------------------------------------------
def _git(cmd: str, timeout: int = 15) -> dict:
    return run_command(f"git {cmd}", timeout=timeout)


@registry.register()
def git_status() -> dict:
    """Retrieve the current git status of the workspace."""
    r = _git("status", timeout=10)
    return {"status": r.get("stdout", "") + "\n" + r.get("stderr", "")}


@registry.register()
def git_diff() -> dict:
    """Retrieve the current uncommitted git diff of the workspace."""
    r = _git("diff", timeout=10)
    return {"diff": r.get("stdout", "") + "\n" + r.get("stderr", "")}


@registry.register()
def git_branch() -> dict:
    """List local and remote git branches."""
    r = _git("branch -a", timeout=10)
    return {"branches": r.get("stdout", "").splitlines()}


@registry.register()
def git_checkout(branch: str, create: bool = False) -> dict:
    """Switch to a git branch, optionally creating it."""
    flag = "-b" if create else ""
    r = _git(f"checkout {flag} {branch}".strip(), timeout=15)
    return {
        "ok": r.get("returncode") == 0,
        "stdout": r.get("stdout", ""),
        "stderr": r.get("stderr", ""),
    }


@registry.register()
def git_log(n: int = 10) -> dict:
    """Show recent git commits in one-line format."""
    r = _git(f'log --oneline -n {n}', timeout=10)
    return {"log": r.get("stdout", "").splitlines()}


@registry.register()
def git_commit(message: str) -> dict:
    """Stage all workspace changes and commit them with the given message."""
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
@registry.register()
def memory_write(key: str, content: str) -> dict:
    """Save or overwrite a markdown block in the memory directory."""
    return write_memory(key, content)


@registry.register()
def memory_read(key: str) -> dict:
    """Read a saved markdown block from the memory directory by its key."""
    return {"content": read_memory(key)}


@registry.register()
def memory_append(key: str, content: str) -> dict:
    """Append content to an existing memory block (creates it if missing)."""
    return append_memory(key, content)


@registry.register()
def memory_list() -> dict:
    """List all files currently stored in memory/."""
    return {"memory_files": list_memory()}


@registry.register()
def scratchpad_append(entry: str) -> dict:
    """Append a timestamped entry to the scratchpad."""
    return append_scratchpad(entry)


@registry.register()
def todo_add(text: str) -> dict:
    """Add a new todo item to the backlog."""
    return add_todo(text)


@registry.register()
def todo_complete(index: int) -> dict:
    """Mark a todo item as complete by its 0-based index."""
    return complete_todo(index)


@registry.register()
def todo_toggle(index: int) -> dict:
    """Toggle the completion state of a todo item by its 0-based index."""
    from state import toggle_todo
    return toggle_todo(index)


@registry.register()
def todo_list() -> dict:
    """Return the current todo list as structured data."""
    return {"todos": load_todos()}


@registry.register()
def plan_read() -> dict:
    """Read the current agent plan from memory/plan.md."""
    return {"content": load_plan()}


@registry.register()
def plan_write(content: str) -> dict:
    """Write or overwrite the agent plan in memory/plan.md."""
    return save_plan(content)


@registry.register(aliases=["snapshot"])
def checkpoint_save(name: str) -> dict:
    """Save a checkpoint. Uses a git tag if available, otherwise a memory snapshot."""
    try:
        git_dir = subprocess.run(
            ["git", "rev-parse", "--git-dir"],
            cwd=WORKSPACE,
            capture_output=True,
            text=True,
            timeout=5,
        )
        if git_dir.returncode == 0:
            tag = f"velocity-checkpoint-{name}"
            r = subprocess.run(
                ["git", "tag", "-a", tag, "-m", f"Velocity checkpoint {name}"],
                cwd=WORKSPACE,
                capture_output=True,
                text=True,
                timeout=10,
            )
            if r.returncode == 0:
                return {"ok": True, "checkpoint": tag, "type": "git-tag"}
    except Exception:
        pass
    return snapshot_memory(name)


@registry.register()
def checkpoint_list() -> dict:
    """List saved checkpoints (git tags and memory snapshots)."""
    tags: list[str] = []
    try:
        r = subprocess.run(
            ["git", "tag", "-l", "velocity-checkpoint-*"],
            cwd=WORKSPACE,
            capture_output=True,
            text=True,
            timeout=5,
        )
        if r.returncode == 0:
            tags = [l.strip() for l in r.stdout.splitlines() if l.strip()]
    except Exception:
        pass
    return {"git_tags": tags, "snapshots": list_snapshots()}


@registry.register()
def session_events(n: int = 50) -> dict:
    """Return the most recent session events from the event log."""
    return {"events": load_session_events(n)}


@registry.register()
def state_info() -> dict:
    """Return workspace and execution-mode metadata."""
    return workspace_info()


@registry.register()
def think(thought: str) -> dict:
    """Log a reasoning step to the session event log."""
    log_event("think", {"thought": thought})
    return {"ok": True, "logged": thought[:200]}


# ---------------------------------------------------------------------------
# User interaction
# ---------------------------------------------------------------------------
@registry.register()
def ask_user(prompt: str) -> dict:
    """Prompt the user for input and return their answer."""
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
    "py": "run_python",
    "python": "run_python",
    "snapshot": "checkpoint_save",
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
    return args


def _filter_args(func: Callable, args: dict) -> dict:
    """Pass only arguments that the function signature accepts."""
    sig = inspect.signature(func)
    filtered: dict[str, Any] = {}
    has_var_keyword = False
    for param in sig.parameters.values():
        if param.kind == inspect.Parameter.VAR_KEYWORD:
            has_var_keyword = True
            break
    if has_var_keyword:
        return args
    for param_name, param in sig.parameters.items():
        if param.kind in (inspect.Parameter.POSITIONAL_OR_KEYWORD, inspect.Parameter.KEYWORD_ONLY):
            if param_name in args:
                filtered[param_name] = args[param_name]
    return filtered


def run_tool(name: str, args: dict) -> dict:
    """Execute a tool by canonical name or alias and log the outcome."""
    name = _NAME_ALIASES.get(name, name)
    args = _normalise_args(args, name)

    func = registry.get(name)
    if func is None:
        return {"error": f"Tool '{name}' not found"}

    filtered_args = _filter_args(func, args)
    try:
        result = func(**filtered_args)
    except Exception as exc:
        return {"error": str(exc)}

    if not isinstance(result, dict):
        result = {"content": result}

    # Append a concise event to the session log.
    try:
        log_event(
            "tool",
            {
                "name": name,
                "args": {k: str(v)[:200] for k, v in args.items()},
                "result": {k: str(v)[:200] for k, v in result.items()},
            },
        )
    except Exception:
        pass

    return result


# ---------------------------------------------------------------------------
# AST / code intelligence
# ---------------------------------------------------------------------------
@registry.register(aliases=["ast", "code_outline"])
def parse_python_ast(path: str) -> dict:
    """Parse a Python file and return its top-level definitions (classes,
    functions, methods) with line numbers. Falls back to a regex outline if
    tree-sitter is unavailable."""
    p = _resolve_path(path)
    if not p.exists():
        return {"error": f"File not found: {p}"}
    try:
        source = p.read_text(encoding="utf-8", errors="replace")
    except Exception as exc:
        return {"error": f"Could not read {p}: {exc}"}

    if _TS_AVAILABLE:
        try:
            parser = Parser(Language(PYTHON_LANGUAGE))
            tree = parser.parse(bytes(source, "utf-8"))
            root = tree.root_node
            defs: list[dict] = []

            def _visit(node, depth=0):
                if node.type in ("function_definition", "class_definition"):
                    name_node = node.child_by_field_name("name")
                    name = name_node.text.decode("utf-8") if name_node else "<anonymous>"
                    kind = "class" if node.type == "class_definition" else "function"
                    defs.append({
                        "name": name,
                        "kind": kind,
                        "line": node.start_point[0] + 1,
                        "depth": depth,
                    })
                    # Visit children so methods are captured inside classes
                    for child in node.children:
                        _visit(child, depth + 1)
                else:
                    for child in node.children:
                        _visit(child, depth)

            _visit(root)
            return {"definitions": defs, "engine": "tree-sitter"}
        except Exception as exc:
            return {"error": f"tree-sitter parse failed: {exc}"}

    # Fallback regex outline
    defs = []
    for i, line in enumerate(source.splitlines(), 1):
        stripped = line.lstrip()
        if stripped.startswith("class ") or stripped.startswith("def "):
            kind = "class" if stripped.startswith("class ") else "function"
            name = stripped.split()[1].split("(")[0].split(":")[0]
            depth = (len(line) - len(stripped)) // 4
            defs.append({"name": name, "kind": kind, "line": i, "depth": max(depth, 0)})
    return {"definitions": defs, "engine": "regex"}


# ---------------------------------------------------------------------------
# Dashboard integration tools
# ---------------------------------------------------------------------------
@registry.register(aliases=["ide", "dashboard"])
def launch_ide(file: str | None = None) -> dict:
    """Launch the V.E.L.O.C.I.T.Y. terminal IDE dashboard. Optionally open a
    file on startup. Returns immediately; the IDE runs in a subprocess."""
    ide_main = WORKSPACE / "ide" / "__main__.py"
    if not ide_main.exists():
        return {"error": f"IDE entry point not found: {ide_main}"}
    cmd = [sys.executable, "-m", "ide"]
    if file:
        cmd.extend(["--file", str(_resolve_path(file))])
    try:
        proc = subprocess.Popen(
            cmd,
            cwd=WORKSPACE,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            stdin=subprocess.DEVNULL,
        )
        return {"ok": True, "pid": proc.pid, "command": " ".join(cmd)}
    except Exception as exc:
        return {"error": f"Failed to launch IDE: {exc}"}
