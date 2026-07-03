"""Dashboard actions wired into the agent tool registry.

Each action takes the IDE app instance and performs a workspace operation
through the canonical tool layer in agent/tools.py.
"""
from __future__ import annotations

import sys
from pathlib import Path

# agent/ is made importable by app.py before this module is imported, but keep
# the fallback here so actions can be tested in isolation.
_AGENT_DIR = Path(__file__).resolve().parent.parent / "agent"
if str(_AGENT_DIR) not in sys.path:
    sys.path.insert(0, str(_AGENT_DIR))

from tools import run_tool


def _log(app, text: str, style: str = "") -> None:
    app.query_one("#shell-pane", object).log_output(text, style)


def _status(app, text: str) -> None:
    app.query_one("#status-bar", object).status = text


def _refresh_todos(app) -> None:
    app.query_one("#todos-panel", object).refresh_todos()


def _refresh_branch(app) -> None:
    app.query_one("#status-bar", object).refresh_branch()


def action_open_file(app, path: str) -> None:
    """Open a workspace file in the editor via the read_file tool."""
    p = Path(path)
    if not p.is_absolute():
        p = app.workspace / p
    if p.exists() and p.is_file():
        app.query_one("#editor", object).open_file(p)
        _status(app, f"opened {p.name}")
    else:
        result = run_tool("read_file", {"path": str(path)})
        if "error" in result:
            _log(app, f"open_file error: {result['error']}", "bold red")
            _status(app, "open failed")
        else:
            app.query_one("#editor", object).content = result.get("content", "")
            _status(app, f"opened {path}")


def action_search_files(app, pattern: str) -> None:
    """Search workspace files via the grep tool."""
    result = run_tool("grep", {"pattern": pattern})
    matches = result.get("matches", [])
    _log(app, f"[bold cyan]Search: {pattern} ({len(matches)} matches)[/bold cyan]")
    if not matches:
        _log(app, "No matches found.")
    for line in matches[:50]:
        _log(app, line)
    _status(app, f"search done ({len(matches)} matches)")


def action_git_status(app) -> None:
    """Show git status in the shell pane."""
    result = run_tool("git_status", {})
    _log(app, "[bold cyan]Git status[/bold cyan]")
    _log(app, result.get("status", "no output"))
    _refresh_branch(app)
    _status(app, "git status")


def action_git_diff(app) -> None:
    """Show git diff in the shell pane."""
    result = run_tool("git_diff", {})
    _log(app, "[bold cyan]Git diff[/bold cyan]")
    _log(app, result.get("diff", "no output"))
    _status(app, "git diff")


def action_git_log(app, n: int = 10) -> None:
    """Show recent commits in the shell pane."""
    result = run_tool("git_log", {"n": n})
    _log(app, f"[bold cyan]Git log (last {n})[/bold cyan]")
    for line in result.get("log", []):
        _log(app, line)
    _status(app, "git log")


def action_git_commit(app, message: str) -> None:
    """Stage and commit all changes."""
    result = run_tool("git_commit", {"message": message})
    if result.get("ok"):
        _log(app, f"[bold green]Committed:[/bold green] {message}")
    else:
        _log(app, f"[bold red]Commit failed:[/bold red] {result.get('stderr', result)}", "bold red")
    _refresh_branch(app)
    _status(app, "committed" if result.get("ok") else "commit failed")


def action_add_todo(app, text: str) -> None:
    """Add a new todo item."""
    result = run_tool("todo_add", {"text": text})
    if result.get("ok"):
        _log(app, f"[bold green]Added todo:[/bold green] {text}")
        _refresh_todos(app)
    else:
        _log(app, f"[bold red]Add todo failed:[/bold red] {result}", "bold red")
    _status(app, "todo added" if result.get("ok") else "todo add failed")


def action_complete_todo(app, index: int) -> None:
    """Mark a todo as complete by index."""
    result = run_tool("todo_complete", {"index": index})
    if result.get("ok"):
        _log(app, f"[bold green]Completed todo #{index}[/bold green]")
        _refresh_todos(app)
    else:
        _log(app, f"[bold red]Complete todo failed:[/bold red] {result}", "bold red")
    _status(app, "todo completed" if result.get("ok") else "todo complete failed")


def action_run_agent(app, instruction: str = "Continue the current plan.") -> None:
    """Run the agent harness with an instruction."""
    _log(app, f"[bold yellow]Running agent:[/bold yellow] {instruction}")
    app.run_agent_process(instruction)



# ---------------------------------------------------------------------------
# Command palette registry
# ---------------------------------------------------------------------------
DASHBOARD_COMMANDS: list[tuple[str, str]] = [
    ("open_file", "Open file..."),
    ("search_files", "Search in files..."),
    ("git_status", "Git status"),
    ("git_diff", "Git diff"),
    ("git_log", "Git log"),
    ("git_commit", "Git commit..."),
    ("add_todo", "Add todo..."),
    ("complete_todo", "Complete todo..."),
    ("run_agent", "Run agent..."),
    ("refresh", "Refresh"),
    ("quit", "Quit"),
]


def run_dashboard_action(app, key: str, arg: str | None = None) -> None:
    """Dispatch a dashboard action by its command-palette key."""
    if key == "open_file":
        action_open_file(app, arg or "")
    elif key == "search_files":
        action_search_files(app, arg or "")
    elif key == "git_status":
        action_git_status(app)
    elif key == "git_diff":
        action_git_diff(app)
    elif key == "git_log":
        action_git_log(app, int(arg) if arg and arg.isdigit() else 10)
    elif key == "git_commit":
        action_git_commit(app, arg or "checkpoint")
    elif key == "add_todo":
        action_add_todo(app, arg or "")
    elif key == "complete_todo":
        try:
            idx = int(arg) if arg is not None else -1
        except ValueError:
            idx = -1
        action_complete_todo(app, idx)
    elif key == "run_agent":
        action_run_agent(app, arg or "Continue the current plan.")
    elif key == "refresh":
        app.action_refresh()
    elif key == "quit":
        app.action_quit()
    else:
        _log(app, f"Unknown dashboard action: {key}", "bold red")
