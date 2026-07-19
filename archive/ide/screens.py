"""Modal screens for the V.E.L.O.C.I.T.Y. IDE."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.containers import Vertical
from textual.screen import Screen
from textual.widgets import Button, Input, Label, OptionList


class CommandPalette(Screen):
    """Quick-command launcher with fuzzy filtering."""

    DEFAULT_CSS = """
    CommandPalette {
        align: center middle;
    }
    #palette-container {
        width: 60;
        height: auto;
        max-height: 20;
        border: thick $background 80%;
        padding: 1 2;
        background: $surface;
    }
    #palette-input {
        height: 3;
    }
    OptionList {
        height: auto;
        max-height: 12;
        border: none;
    }
    """

    def __init__(self, commands: list[tuple[str, str]]) -> None:
        """commands: list of (key, label) tuples."""
        self.commands = commands
        self.filtered = list(commands)
        super().__init__()

    def compose(self) -> ComposeResult:
        with Vertical(id="palette-container"):
            yield Input(placeholder="Type a command...", id="palette-input")
            yield OptionList(*[label for _, label in self.filtered], id="palette-list")

    def on_mount(self) -> None:
        self.query_one("#palette-input", Input).focus()

    def _update_list(self) -> None:
        query = self.query_one("#palette-input", Input).value.lower()
        self.filtered = [
            (key, label) for key, label in self.commands
            if query in label.lower() or query in key.lower()
        ]
        option_list = self.query_one("#palette-list", OptionList)
        option_list.clear_options()
        for _, label in self.filtered:
            option_list.add_option(label)
        if option_list.option_count:
            option_list.highlighted = 0

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "palette-input":
            self._update_list()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "palette-input":
            self._select_highlighted()

    def on_option_list_option_selected(self, event: OptionList.OptionSelected) -> None:
        index = event.option_index
        if 0 <= index < len(self.filtered):
            self.dismiss(self.filtered[index][0])

    def _select_highlighted(self) -> None:
        option_list = self.query_one("#palette-list", OptionList)
        highlighted = option_list.highlighted
        if highlighted is not None and 0 <= highlighted < len(self.filtered):
            self.dismiss(self.filtered[highlighted][0])


class PromptScreen(Screen):
    """Simple one-line prompt that returns the user's input."""

    DEFAULT_CSS = """
    PromptScreen {
        align: center middle;
    }
    #prompt-container {
        width: 60;
        height: auto;
        border: thick $background 80%;
        padding: 1 2;
        background: $surface;
    }
    #prompt-label {
        height: auto;
        margin-bottom: 1;
    }
    #prompt-input {
        height: 3;
    }
    """

    def __init__(self, question: str, default: str = "") -> None:
        self.question = question
        self.default = default
        super().__init__()

    def compose(self) -> ComposeResult:
        with Vertical(id="prompt-container"):
            yield Label(self.question, id="prompt-label")
            yield Input(value=self.default, id="prompt-input")

    def on_mount(self) -> None:
        self.query_one("#prompt-input", Input).focus()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "prompt-input":
            self.dismiss(event.value)

    def on_key(self, event) -> None:
        # Escape cancels the prompt
        if event.key == "escape":
            self.dismiss(None)
