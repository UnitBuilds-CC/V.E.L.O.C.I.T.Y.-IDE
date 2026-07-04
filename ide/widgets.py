"""Reusable widgets for the V.E.L.O.C.I.T.Y. terminal IDE."""
from __future__ import annotations

import subprocess
from pathlib import Path

from textual.widgets import DirectoryTree, Input, RichLog, Static
from textual.reactive import reactive
from textual.message import Message

# Resolve workspace root relative to this file (workspace/ide/widgets.py)
WORKSPACE = Path(__file__).resolve().parent.parent


class FileTree(DirectoryTree):
    """Workspace file tree that posts FileSelected messages on selection."""

    DEFAULT_CSS = """
    FileTree {
        width: 100%;
        height: 100%;
        border: none;
    }
    """

    class FileSelected(Message):
        """Emitted when a file is selected in the tree."""

        def __init__(self, path: Path) -> None:
            self.path = path
            super().__init__()

    def on_mount(self) -> None:
        self.root.expand()

    def on_directory_tree_file_selected(self, event: DirectoryTree.FileSelected) -> None:
        self.post_message(self.FileSelected(event.path))

    def reload_tree(self) -> None:
        self.reload()
        self.root.expand()


class Editor(Static):
    """Simple read-only file viewer with line numbers and syntax highlighting."""

    path: reactive[Path | None] = reactive(None)
    content: reactive[str] = reactive("")

    def watch_content(self, content: str) -> None:
        if not content:
            self.update("# No file open\nSelect a file from the sidebar.")
            return
        
        # Guess language from path suffix
        lexer = "text"
        if self.path:
            ext = self.path.suffix.lower()
            if ext == ".py":
                lexer = "python"
            elif ext in (".js", ".ts", ".jsx", ".tsx"):
                lexer = "javascript"
            elif ext in (".json", ".jsonl"):
                lexer = "json"
            elif ext == ".md":
                lexer = "markdown"
            elif ext in (".htm", ".html"):
                lexer = "html"
            elif ext == ".css":
                lexer = "css"
            elif ext in (".sh", ".bash", ".zsh", ".bat", ".ps1"):
                lexer = "shell"
            elif ext in (".xml", ".svg"):
                lexer = "xml"
            elif ext == ".toml":
                lexer = "toml"
            elif ext in (".yaml", ".yml"):
                lexer = "yaml"
            elif self.path.name.lower() == "dockerfile":
                lexer = "dockerfile"

        from rich.syntax import Syntax
        syntax = Syntax(
            content,
            lexer,
            theme="monokai",
            line_numbers=True,
            word_wrap=True,
        )
        self.update(syntax)


    def open_file(self, path: Path) -> None:
        self.path = path
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except Exception as exc:
            text = f"# Error reading {path}\n{exc}"
        self.content = text


class ShellPane(Static):
    """Bottom panel with a scrollable output log and a command input."""

    DEFAULT_CSS = """
    ShellPane {
        layout: vertical;
        height: 100%;
    }
    ShellPane RichLog {
        height: 1fr;
        border: solid $primary;
    }
    ShellPane Input {
        height: 3;
        border: solid $primary-lighten-2;
    }
    """

    class CommandSubmitted(Message):
        """Emitted when the user hits Enter in the shell input."""

        def __init__(self, command: str) -> None:
            self.command = command
            super().__init__()

    def compose(self):
        yield RichLog(highlight=True, markup=True, wrap=True, id="shell-log")
        yield Input(placeholder="Ask Kimi something (or prefix with ! for shell command)...", id="shell-input")

    def on_mount(self) -> None:
        self.query_one("#shell-log", RichLog).write("[bold green]>[/bold green] Shell ready.")

    def log_output(self, text: str, style: str = "") -> None:
        log = self.query_one("#shell-log", RichLog)
        # Remove trailing newline from command output if it exists to avoid extra spaces
        text_strip = text.rstrip("\r\n")
        if style:
            log.write(f"[{style}]{text_strip}[/{style}]")
        else:
            log.write(text_strip)

    def focus_input(self) -> None:
        self.query_one("#shell-input", Input).focus()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id != "shell-input":
            return
        command = event.value.strip()
        if not command:
            return
        event.input.value = ""
        self.post_message(self.CommandSubmitted(command))


class TodosPanel(Static):
    """Displays the current todo backlog from memory/todos.md."""

    DEFAULT_CSS = """
    TodosPanel {
        height: 1fr;
        border: solid $primary;
        padding: 0 1;
    }
    """

    todos: reactive[list[dict]] = reactive([])

    def watch_todos(self, todos: list[dict]) -> None:
        lines = ["[bold underline]Backlog[/bold underline]"]
        for i, t in enumerate(todos):
            mark = "[green]x[/green]" if t.get("done") else "[red]o[/red]"
            lines.append(f"{mark} {i}. {t['text']}")
        self.update("\n".join(lines) if todos else "No todos yet.")

    def refresh_todos(self) -> None:
        todos_path = WORKSPACE / "memory" / "todos.md"
        if not todos_path.exists():
            self.todos = []
            return
        parsed: list[dict] = []
        for line in todos_path.read_text(encoding="utf-8").splitlines():
            stripped = line.strip()
            if stripped.startswith("- [") and "]" in stripped:
                done = stripped[3].lower() == "x"
                text = stripped.split("]", 1)[1].strip()
                parsed.append({"done": done, "text": text})
        self.todos = parsed


class PlanPanel(Static):
    """Displays the current plan from memory/plan.md."""

    DEFAULT_CSS = """
    PlanPanel {
        height: 1fr;
        border: solid $primary;
        padding: 0 1;
    }
    """

    content: reactive[str] = reactive("")

    def watch_content(self, content: str) -> None:
        if not content.strip():
            self.update("[dim]No plan loaded.[/dim]")
            return
        lines = content.splitlines()
        # Hide the title if present to save space
        if lines and lines[0].startswith("#"):
            lines = lines[1:]
        self.update("\n".join(lines).strip() or content)

    def refresh_plan(self) -> None:
        plan_path = WORKSPACE / "memory" / "plan.md"
        if not plan_path.exists():
            self.content = ""
            return
        self.content = plan_path.read_text(encoding="utf-8")


class StatusBar(Static):
    """Footer-style status bar showing git branch, mode, and active status."""

    status: reactive[str] = reactive("idle")
    branch: reactive[str] = reactive("unknown")
    mode: reactive[str] = reactive("host")

    DEFAULT_CSS = """
    StatusBar {
        height: 1;
        color: $text;
        background: $surface;
        content-align: center middle;
    }
    """

    def watch_status(self, status: str) -> None:
        self.update_text()

    def watch_branch(self, branch: str) -> None:
        self.update_text()

    def watch_mode(self, mode: str) -> None:
        self.update_text()

    def update_text(self) -> None:
        self.update(
            f" | [b]Mode:[/b] {self.mode} "
            f"| [b]Git:[/b] {self.branch} "
            f"| [b]Status:[/b] {self.status} "
        )

    def refresh_branch(self) -> None:
        try:
            result = subprocess.run(
                ["git", "rev-parse", "--abbrev-ref", "HEAD"],
                cwd=WORKSPACE,
                capture_output=True,
                text=True,
                timeout=5,
            )
            self.branch = result.stdout.strip() or "no branch"
        except Exception:
            self.branch = "no git"
