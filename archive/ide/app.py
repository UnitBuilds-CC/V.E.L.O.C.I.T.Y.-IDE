"""V.E.L.O.C.I.T.Y. Terminal IDE dashboard built with Textual."""
from __future__ import annotations

import asyncio
import itertools
import os
import sys
from pathlib import Path

from textual.app import App, ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import ContentSwitcher, Footer, Header, Static, TabbedContent, TabPane
from textual.worker import get_current_worker

# Spinner frames for the status bar
_SPINNER = itertools.cycle(["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])


# Ensure agent/ is importable when running the IDE directly
_AGENT_DIR = Path(__file__).resolve().parent.parent / "agent"
if str(_AGENT_DIR) not in sys.path:
    sys.path.insert(0, str(_AGENT_DIR))

from ide.actions import DASHBOARD_COMMANDS, run_dashboard_action
from ide.screens import CommandPalette, PromptScreen
from ide.widgets import Editor, FileTree, PlanPanel, ShellPane, StatusBar, TodosPanel

WORKSPACE = Path(__file__).resolve().parent.parent


class VelocityIDE(App):
    """Main Textual application for the agentic coding IDE."""

    CSS = """
    Screen {
        layout: vertical;
    }
    #main-layout {
        height: 1fr;
    }
    #sidebar {
        width: 32;
        max-width: 40;
        border: solid $primary;
    }
    #sidebar-tabs {
        height: 1fr;
    }
    #editor-pane {
        width: 1fr;
        border: solid $primary;
    }
    #shell-pane {
        height: 35%;
        border: solid $primary;
    }
    #status-bar {
        dock: bottom;
    }
    /* Highlight the border of focused panes with neon accent color */
    #sidebar:focus-within {
        border: double $accent;
    }
    #editor-pane:focus-within {
        border: double $accent;
    }
    #shell-pane:focus-within {
        border: double $accent;
    }
    """



    BINDINGS = [
        ("q", "quit", "Quit"),
        ("r", "refresh", "Refresh"),
        ("g", "git_status", "Git status"),
        ("d", "git_diff", "Git diff"),
        ("l", "git_log", "Git log"),
        ("s", "run_agent", "Run agent"),
        ("ctrl+o", "command_open_file", "Open file"),
        ("ctrl+shift+f", "command_search_files", "Search"),
        ("ctrl+p", "command_palette", "Command palette"),
        ("ctrl+t", "command_run_agent", "Run agent"),
        ("ctrl+b", "focus_sidebar", "Focus sidebar"),
        ("ctrl+e", "focus_editor", "Focus editor"),
        ("ctrl+slash", "focus_shell", "Focus shell"),
        ("f5", "refresh", "Refresh"),
    ]

    def __init__(self, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self.workspace = WORKSPACE

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        with Horizontal(id="main-layout"):
            with Vertical(id="sidebar"):
                with TabbedContent(id="sidebar-tabs"):
                    with TabPane("Files", id="tab-files"):
                        yield FileTree(str(WORKSPACE), id="file-tree")
                    with TabPane("Todos", id="tab-todos"):
                        yield TodosPanel(id="todos-panel")
                    with TabPane("Plan", id="tab-plan"):
                        yield PlanPanel(id="plan-panel")
            with Vertical(id="editor-pane"):
                yield Editor(id="editor")
        yield ShellPane(id="shell-pane")
        yield StatusBar(id="status-bar")
        yield Footer()

    def on_mount(self) -> None:
        self.title = "V.E.L.O.C.I.T.Y. IDE"
        self.sub_title = str(WORKSPACE)
        self.query_one("#todos-panel", TodosPanel).refresh_todos()
        self.query_one("#plan-panel", PlanPanel).refresh_plan()
        self.query_one("#status-bar", StatusBar).refresh_branch()
        self.query_one("#status-bar", StatusBar).status = "ready"
        self.query_one("#file-tree", FileTree).focus()

    # ------------------------------------------------------------------
    # Widget events
    # ------------------------------------------------------------------
    def on_file_tree_file_selected(self, event: FileTree.FileSelected) -> None:
        if event.path.is_file():
            self.query_one("#editor", Editor).open_file(event.path)
            self.query_one("#status-bar", StatusBar).status = f"opened {event.path.name}"

    def on_shell_pane_command_submitted(self, event: ShellPane.CommandSubmitted) -> None:
        cmd = event.command.strip()
        if cmd.startswith("!"):
            # Run raw shell command
            self.run_shell_command(cmd[1:].strip())
        else:
            # Run as instruction to Kimi agent
            self.run_agent_instruction(cmd)

    # ------------------------------------------------------------------
    # Actions
    # ------------------------------------------------------------------
    def action_refresh(self) -> None:
        self.query_one("#todos-panel", TodosPanel).refresh_todos()
        self.query_one("#plan-panel", PlanPanel).refresh_plan()
        self.query_one("#file-tree", FileTree).reload_tree()
        self.query_one("#status-bar", StatusBar).refresh_branch()
        self.query_one("#status-bar", StatusBar).status = "refreshed"

    def action_git_diff(self) -> None:
        run_dashboard_action(self, "git_diff")

    def action_git_log(self) -> None:
        run_dashboard_action(self, "git_log")

    def action_focus_sidebar(self) -> None:
        self.query_one("#file-tree", FileTree).focus()

    def action_focus_editor(self) -> None:
        self.query_one("#editor", Editor).focus()

    def action_focus_shell(self) -> None:
        self.query_one("#shell-pane", ShellPane).focus_input()

    def action_git_status(self) -> None:
        run_dashboard_action(self, "git_status")

    def action_run_agent(self) -> None:
        run_dashboard_action(self, "run_agent")

    def action_command_palette(self) -> None:
        self.push_screen(CommandPalette(DASHBOARD_COMMANDS), self._on_palette_select)

    def action_command_open_file(self) -> None:
        self.push_screen(PromptScreen("Open file:"), self._on_open_file)

    def action_command_search_files(self) -> None:
        self.push_screen(PromptScreen("Search pattern:"), self._on_search_files)

    def action_command_run_agent(self) -> None:
        self.push_screen(PromptScreen("Agent instruction:", "Continue the current plan."), self._on_run_agent)

    def _on_palette_select(self, key: str | None) -> None:
        if key is None:
            return
        if key in {"open_file", "search_files", "git_commit", "add_todo", "complete_todo", "run_agent"}:
            # These need an argument; prompt for it.
            prompts = {
                "open_file": "Open file:",
                "search_files": "Search pattern:",
                "git_commit": "Commit message:",
                "add_todo": "Todo text:",
                "complete_todo": "Todo index:",
                "run_agent": "Agent instruction:",
            }
            defaults = {
                "run_agent": "Continue the current plan.",
                "git_commit": "checkpoint",
            }
            self.push_screen(
                PromptScreen(prompts[key], defaults.get(key, "")),
                lambda value: self._run_action_with_arg(key, value),
            )
        else:
            run_dashboard_action(self, key)

    def _run_action_with_arg(self, key: str, value: str | None) -> None:
        if value is None:
            return
        run_dashboard_action(self, key, value)

    def _on_open_file(self, value: str | None) -> None:
        if value:
            run_dashboard_action(self, "open_file", value)

    def _on_search_files(self, value: str | None) -> None:
        if value:
            run_dashboard_action(self, "search_files", value)

    def _on_run_agent(self, value: str | None) -> None:
        run_dashboard_action(self, "run_agent", value or "Continue the current plan.")

    # ------------------------------------------------------------------
    # Execution helpers
    # ------------------------------------------------------------------
    def run_agent_instruction(self, instruction: str) -> None:
        """Launch the Kimi agent harness to process the instruction."""
        run_dashboard_action(self, "run_agent", instruction)

    def handle_tui_action(self, action: str, arg: str) -> None:
        """Execute a remote action requested by the agent process."""
        if action == "open_file":
            p = Path(arg)
            if not p.is_absolute():
                p = self.workspace / p
            if p.exists() and p.is_file():
                self.query_one("#editor", object).open_file(p)
                self.query_one("#status-bar", object).status = f"opened {p.name}"
        elif action == "refresh":
            self.action_refresh()
        elif action == "show_tab":
            tabbed = self.query_one("#sidebar-tabs", object)
            if arg == "files":
                tabbed.active = "tab-files"
            elif arg == "todos":
                tabbed.active = "tab-todos"
            elif arg == "plan":
                tabbed.active = "tab-plan"

    def run_agent_process(self, instruction: str, timeout: int = 600) -> None:
        """Launch the Kimi agent by passing the instruction as a direct argv element.

        Uses create_subprocess_exec (not shell=True) so no quoting or escaping
        is needed — the instruction string is handed directly to the process.
        """
        shell = self.query_one("#shell-pane", ShellPane)
        status = self.query_one("#status-bar", StatusBar)
        status.status = "agent: thinking"
        shell.log_output(f"[Kimi] ▶ {instruction}", "bold yellow")
        self.run_worker(
            self._execute_argv(
                [sys.executable, "-m", "agent.main", instruction],
                timeout=timeout,
                is_agent=True,
            ),
            exclusive=False,
        )

    def run_shell_command(self, command: str, display_command: str = None, timeout: int = 300) -> None:
        """Run a shell command in a background worker so the UI stays responsive."""
        shell = self.query_one("#shell-pane", ShellPane)
        status = self.query_one("#status-bar", StatusBar)
        is_agent = "agent.main" in command
        status.status = "agent: thinking" if is_agent else "running shell"
        disp = display_command or f"$ {command}"
        shell.log_output(disp, "bold cyan")
        self.run_worker(self._execute_command(command, timeout, is_agent=is_agent), exclusive=False)

    async def _execute_argv(self, argv: list, timeout: int, is_agent: bool = True) -> None:
        """Like _execute_command but spawns via exec (no shell) using a proper argv list."""
        worker = get_current_worker()
        shell = self.query_one("#shell-pane", ShellPane)
        status = self.query_one("#status-bar", StatusBar)
        env = os.environ.copy()
        env["PYTHONIOENCODING"] = "utf-8"

        spinner_running = True
        async def _spin():
            while spinner_running:
                frame = next(_SPINNER)
                current = status.status
                label = current.lstrip("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ")
                status.status = f"{frame} {label}"
                await asyncio.sleep(0.12)

        spin_task = asyncio.ensure_future(_spin())

        try:
            proc = await asyncio.create_subprocess_exec(
                *argv,
                cwd=WORKSPACE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                env=env,
            )

            async def read_stdout():
                while not worker.is_cancelled:
                    line = await proc.stdout.readline()
                    if not line:
                        break
                    text = line.decode("utf-8", errors="ignore").rstrip("\r\n")
                    if not text:
                        continue
                    if text.startswith("[TUI_ACTION]"):
                        parts = text[12:].strip().split(":", 1)
                        act = parts[0].strip()
                        arg = parts[1].strip() if len(parts) > 1 else ""
                        self.handle_tui_action(act, arg)
                    elif text.startswith("[Kimi] Thought:"):
                        shell.log_output("💭 " + text[15:], "italic rgb(180,180,100)")
                    elif text.startswith("[Kimi] Chat:"):
                        shell.log_output("🤖 " + text[12:], "bold white")
                    elif text.startswith("[Kimi] Thinking"):
                        shell.log_output("⠿ Thinking...", "bold yellow")
                        status.status = "⠿ agent: thinking"
                    elif text.startswith("[Kimi] Calling tool"):
                        tool_name = text.split("'")[1] if "'" in text else "tool"
                        shell.log_output("🔧 " + text[7:], "bold cyan")
                        status.status = f"⠿ agent: {tool_name}"
                    elif text.startswith("->"):
                        ok = "Error" not in text
                        shell.log_output(("✔ " if ok else "✖ ") + text, "green" if ok else "bold red")
                    else:
                        shell.log_output(text)

            async def read_stderr():
                while not worker.is_cancelled:
                    line = await proc.stderr.readline()
                    if not line:
                        break
                    text = line.decode("utf-8", errors="ignore").rstrip("\r\n")
                    if text:
                        shell.log_output(text, "bold red")

            await asyncio.wait_for(
                asyncio.gather(read_stdout(), read_stderr()),
                timeout=timeout,
            )
            await proc.wait()

            rc = proc.returncode
            shell.log_output(f"[exit {rc}]", "dim green" if rc == 0 else "dim red")
            status.status = "agent: done ✓" if is_agent else "ready"
        except asyncio.TimeoutError:
            try:
                proc.kill()
                await proc.wait()
            except Exception:
                pass
            shell.log_output("Agent timed out", "bold red")
            status.status = "timed out"
        except Exception as exc:
            shell.log_output(f"Error: {exc}", "bold red")
            status.status = "error"
        finally:
            spinner_running = False
            spin_task.cancel()

    async def _execute_command(self, command: str, timeout: int, is_agent: bool = False) -> None:
        worker = get_current_worker()
        shell = self.query_one("#shell-pane", ShellPane)
        status = self.query_one("#status-bar", StatusBar)
        env = os.environ.copy()
        env["PYTHONIOENCODING"] = "utf-8"

        # Spinner task: pulses the status bar label while the command runs
        spinner_running = True
        async def _spin():
            while spinner_running:
                frame = next(_SPINNER)
                if is_agent:
                    current = status.status
                    # Keep the descriptive part, just prepend the spinner
                    label = current.lstrip("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ")
                    status.status = f"{frame} {label}"
                else:
                    status.status = f"{frame} running…"
                await asyncio.sleep(0.12)

        spin_task = asyncio.ensure_future(_spin())

        try:
            proc = await asyncio.create_subprocess_shell(
                command,
                cwd=WORKSPACE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                env=env,
            )

            async def read_stdout():
                while not worker.is_cancelled:
                    line = await proc.stdout.readline()
                    if not line:
                        break
                    text = line.decode("utf-8", errors="ignore").rstrip("\r\n")
                    if not text:
                        continue
                    if text.startswith("[TUI_ACTION]"):
                        parts = text[12:].strip().split(":", 1)
                        act = parts[0].strip()
                        arg = parts[1].strip() if len(parts) > 1 else ""
                        self.handle_tui_action(act, arg)
                    elif text.startswith("[Kimi] Thought:"):
                        shell.log_output("💭 " + text[15:], "italic rgb(180,180,100)")
                    elif text.startswith("[Kimi] Chat:"):
                        shell.log_output("🤖 " + text[12:], "bold white")
                    elif text.startswith("[Kimi] Thinking"):
                        shell.log_output("⠿ Thinking...", "bold yellow")
                        if is_agent:
                            status.status = "⠿ agent: thinking"
                    elif text.startswith("[Kimi] Calling tool"):
                        tool_name = text.split("'")[1] if "'" in text else "tool"
                        shell.log_output("🔧 " + text[7:], "bold cyan")
                        if is_agent:
                            status.status = f"⠿ agent: {tool_name}"
                    elif text.startswith("->"):
                        ok = "Error" not in text
                        shell.log_output(("✔ " if ok else "✖ ") + text, "green" if ok else "bold red")
                    else:
                        shell.log_output(text)

            async def read_stderr():
                while not worker.is_cancelled:
                    line = await proc.stderr.readline()
                    if not line:
                        break
                    text = line.decode("utf-8", errors="ignore").rstrip("\r\n")
                    if text:
                        shell.log_output(text, "bold red")

            # Read both streams concurrently in real-time
            await asyncio.wait_for(
                asyncio.gather(read_stdout(), read_stderr()),
                timeout=timeout,
            )
            await proc.wait()

            rc = proc.returncode
            shell.log_output(f"[exit {rc}]", "dim green" if rc == 0 else "dim red")
            status.status = "agent: done ✓" if is_agent else "ready"
        except asyncio.TimeoutError:
            try:
                proc.kill()
                await proc.wait()
            except Exception:
                pass
            shell.log_output("Command timed out", "bold red")
            status.status = "timed out"
        except Exception as exc:
            shell.log_output(f"Error: {exc}", "bold red")
            status.status = "error"
        finally:
            spinner_running = False
            spin_task.cancel()


def main() -> None:
    app = VelocityIDE()
    app.run()


if __name__ == "__main__":
    main()
