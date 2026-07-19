# agent/state.py
"""Structured workspace state manager for V.E.L.O.C.I.T.Y.

Keeps project context, scratchpad, todos, plans, checkpoints and a session
event log in memory/*.md (and memory/*.jsonl) so the agent can reason about
its own progress across turns.
"""
from __future__ import annotations

import json
import re
import shutil
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

WORKSPACE = Path(__file__).resolve().parent.parent
MEMORY = WORKSPACE / "memory"

# Execution mode: "host" means we are running directly on the user's machine.
EXECUTION_MODE = "host"


def _utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%SZ")


def _md_path(key: str) -> Path:
    key = key if key.endswith(".md") else f"{key}.md"
    return MEMORY / key


def read_memory(key: str) -> str:
    p = _md_path(key)
    if not p.exists():
        return ""
    return p.read_text(encoding="utf-8")


def write_memory(key: str, content: str) -> dict:
    p = _md_path(key)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    return {"ok": True, "path": str(p), "bytes": len(content)}


def append_memory(key: str, content: str) -> dict:
    p = _md_path(key)
    p.parent.mkdir(parents=True, exist_ok=True)
    existing = p.read_text(encoding="utf-8") if p.exists() else ""
    new = existing.rstrip() + "\n\n" + content.strip() + "\n"
    p.write_text(new, encoding="utf-8")
    return {"ok": True, "path": str(p), "bytes": len(new)}


def list_memory() -> list[str]:
    """Return the names of all files currently stored in memory/."""
    if not MEMORY.exists():
        return []
    return [p.name for p in MEMORY.iterdir() if p.is_file()]


# ---------------------------------------------------------------------------
# Project
# ---------------------------------------------------------------------------
def load_project() -> str:
    return read_memory("project")


def save_project(content: str) -> dict:
    return write_memory("project", content)


# ---------------------------------------------------------------------------
# Scratchpad
# ---------------------------------------------------------------------------
def load_scratchpad() -> str:
    return read_memory("scratchpad")


def save_scratchpad(content: str) -> dict:
    return write_memory("scratchpad", content)


def append_scratchpad(entry: str) -> dict:
    stamp = _utc_now()
    return append_memory("scratchpad", f"## {stamp}\n{entry.strip()}")


# ---------------------------------------------------------------------------
# Plan
# ---------------------------------------------------------------------------
def load_plan() -> str:
    return read_memory("plan")


def save_plan(content: str) -> dict:
    return write_memory("plan", content)


# ---------------------------------------------------------------------------
# Todos
# ---------------------------------------------------------------------------
_TODO_RE = re.compile(r"^\s*-\s*\[([ xX])\]\s*(.*)$", re.MULTILINE)


def load_todos() -> list[dict]:
    text = read_memory("todos")
    todos = []
    for m in _TODO_RE.finditer(text):
        todos.append({
            "done": m.group(1).lower() == "x",
            "text": m.group(2).strip(),
        })
    return todos


def _todos_to_markdown(todos: list[dict]) -> str:
    lines = ["# Backlog", ""]
    for t in todos:
        mark = "x" if t.get("done") else " "
        lines.append(f"- [{mark}] {t['text']}")
    lines.append("")
    return "\n".join(lines)


def save_todos(todos: list[dict]) -> dict:
    return write_memory("todos", _todos_to_markdown(todos))


def add_todo(text: str) -> dict:
    todos = load_todos()
    todos.append({"done": False, "text": text.strip()})
    return save_todos(todos)


def complete_todo(index: int) -> dict:
    todos = load_todos()
    if 0 <= index < len(todos):
        todos[index]["done"] = True
    return save_todos(todos)


def toggle_todo(index: int) -> dict:
    todos = load_todos()
    if 0 <= index < len(todos):
        todos[index]["done"] = not todos[index]["done"]
    return save_todos(todos)


# ---------------------------------------------------------------------------
# Checkpoints / snapshots
# ---------------------------------------------------------------------------
def snapshot_memory(key: str) -> dict:
    """Copy the entire memory directory to memory/snapshots/<key>."""
    snap_dir = MEMORY / "snapshots" / key
    if snap_dir.exists():
        shutil.rmtree(snap_dir)
    # Ignore the snapshots directory itself to avoid recursive bloat.
    shutil.copytree(MEMORY, snap_dir, ignore=shutil.ignore_patterns("snapshots"))
    return {"ok": True, "path": str(snap_dir), "type": "snapshot"}


def list_snapshots() -> list[str]:
    snap_root = MEMORY / "snapshots"
    if not snap_root.exists():
        return []
    return [str(p.relative_to(MEMORY)) for p in snap_root.iterdir() if p.is_dir()]


# ---------------------------------------------------------------------------
# Session event log (append-only, machine-friendly)
# ---------------------------------------------------------------------------
def log_event(event_type: str, payload: dict[str, Any]) -> dict:
    p = MEMORY / "session.jsonl"
    p.parent.mkdir(parents=True, exist_ok=True)
    record = {"ts": _utc_now(), "type": event_type, "payload": payload}
    with p.open("a", encoding="utf-8") as f:
        f.write(json.dumps(record, ensure_ascii=False, default=str) + "\n")
    return {"ok": True}


def load_session_events(n: int = 50) -> list[dict]:
    p = MEMORY / "session.jsonl"
    if not p.exists():
        return []
    lines = p.read_text(encoding="utf-8").strip().splitlines()
    events = []
    for line in lines[-n:]:
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return events


# ---------------------------------------------------------------------------
# Aggregate memory block for the system prompt
# ---------------------------------------------------------------------------
def load_memory_block() -> str:
    blocks = []
    for name in ["project.md", "plan.md", "scratchpad.md", "todos.md"]:
        p = MEMORY / name
        if p.exists():
            blocks.append(f"--- {name} ---\n{p.read_text(encoding='utf-8')}\n")

    # Append a tiny digest of recent events so the model has execution context.
    events = load_session_events(10)
    if events:
        blocks.append("--- recent events ---\n")
        for ev in events:
            blocks.append(f"- [{ev.get('ts')}] {ev.get('type')}: {ev.get('payload')}")
        blocks.append("")
    return "\n".join(blocks)


# ---------------------------------------------------------------------------
# Workspace introspection
# ---------------------------------------------------------------------------
def workspace_info() -> dict:
    return {
        "workspace": str(WORKSPACE),
        "execution_mode": EXECUTION_MODE,
        "memory_files": list_memory(),
    }
