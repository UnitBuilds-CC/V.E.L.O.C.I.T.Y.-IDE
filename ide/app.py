"""V.E.L.O.C.I.T.Y. Terminal IDE dashboard built with Textual."""
from __future__ import annotations

import asyncio
import os
import subprocess
import sys
from pathlib import Path

from textual.app import App, ComposeResult
from textual.containers import Horizontal, Vertical
from textual.worker import Worker, get_current_worker
from textual.widgets import Footer, Header, Static

# Ensure agent/ is importable when running the IDE directly
_AGENT_DIR = Path(__file__).resolve().parent.parent / "agent"
if str(_AGENT_DIR) not in sys.path:
    sys.path.insert(0, str(_AGENT_DIR))

from ide.widgets import Editor, FileTree, ShellPane, StatusBar, TodosPanel

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
        width: 30%;
        max-width: 50;
        border: solid $primary;
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
    """

    BINDINGS = [
        ("q", "quit", "Quit"),
        ("r", "refresh", "Refresh"),
        ("g", "git_status", "Git status"),
        ("s", "run_agent", "Run agent"),
    ]

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        with Horizontal(id="main-layout"):
            with Vertical(id="sidebar"):
                yield TodosPanel(id="todos-panel")
                yield FileTree(str(WORKSPACE), id="file-tree")
            with Vertical(id="editor-pane"):
                yield Editor(id="editor")
        yield ShellPane(id="shell-pane")
        yield StatusBar(id="status-bar")
        yield Footer()

    def on_mount(self) -> None:
        self.title = "V.E.L.O.C.I.T.Y. IDE"
        self.sub_title = str(WORKSPACE)
        self.query_one("#todos-panel", TodosPanel).refresh_todos()
        self.query_one("#status-bar", StatusBar).refresh_branch()
        self.query_one("#status-bar", StatusBar).status = "ready"

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

    def run_agent_instruction(self, instruction: str) -> None:
        """Launch the Kimi agent harness to process the instruction."""
        # Quote the instruction safely for command execution
        safe_instruction = instruction.replace("'", "'\"'\"'")
        cmd = f"{sys.executable} -m agent.main '{safe_instruction}'"
        self.run_shell_command(cmd, display_command=f"Chat to Kimi: {instruction}")

    def run_shell_command(self, command: str, display_command: str = None, timeout: int = 300) -> None:
        """Run a shell command in a background worker so the UI stays responsive."""
        shell = self.query_one("#shell-pane", ShellPane)
        status = self.query_one("#status-bar", StatusBar)
        status.status = "running shell"
        disp = display_command or f"$ {command}"
        shell.log_output(disp, "bold cyan")
        self.run_worker(self._execute_command(command, timeout), exclusive=False)

    async def _execute_command(self, command: str, timeout: int) -> None:
        worker = get_current_worker()
        shell = self.query_one("#shell-pane", ShellPane)
        status = self.query_one("#status-bar", StatusBar)
        env = os.environ.copy()
        env["PYTHONIOENCODING"] = "utf-8"
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
                    text = line.decode("utf-8", errors="ignore")
                    shell.log_output(text)

            async def read_stderr():
                while not worker.is_cancelled:
                    line = await proc.stderr.readline()
                    if not line:
                        break
                    text = line.decode("utf-8", errors="ignore")
                    shell.log_output(text, "bold red")

            # Read both streams concurrently in real-time
            await asyncio.wait_for(
                asyncio.gather(read_stdout(), read_stderr()),
                timeout=timeout,
            )
            await proc.wait()

            shell.log_output(f"[exit {proc.returncode}]", "dim")
            status.status = "ready"
        except asyncio.TimeoutError:
            try:
                proc.kill()
                await proc.wait()
            except Exception:
                pass
            shell.log_output("Command timed out", "bold red")
            status.status = "ready"
        except Exception as exc:
            shell.log_output(f"Error: {exc}", "bold red")
            status.status = "ready"

    def action_refresh(self) -> None:
        self.query_one("#todos-panel", TodosPanel).refresh_todos()
        self.query_one("#status-bar", StatusBar).refresh_branch()
        self.query_one("#status-bar", StatusBar).status = "refreshed"

    def action_git_status(self) -> None:
        self.run_shell_command("git status -sb")

    def action_run_agent(self) -> None:
        shell = self.query_one("#shell-pane", ShellPane)
        shell.log_output("Launching agent harness...", "bold yellow")
        self.run_shell_command(f"{sys.executable} -m agent.main 'Continue the current plan.'")


def main() -> None:
    app = VelocityIDE()
    app.run()


if __name__ == "__main__":
    main()
