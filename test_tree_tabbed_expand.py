"""Test DirectoryTree inside TabbedContent with explicit expand."""
from pathlib import Path
from textual.app import App
from textual.widgets import TabbedContent, TabPane, DirectoryTree, Static

WORKSPACE = Path(__file__).resolve().parent

class TabbedTreeApp(App):
    CSS = """
    TabbedContent { height: 100%; }
    TabPane { height: 100%; }
    DirectoryTree { height: 100%; }
    """

    def compose(self):
        with TabbedContent():
            with TabPane("Files"):
                yield DirectoryTree(str(WORKSPACE), id="tree")
            with TabPane("Other"):
                yield Static("other")

    def on_mount(self):
        tree = self.query_one("#tree", DirectoryTree)
        tree.root.expand()
        self.set_timer(0.5, self._check)

    def _check(self):
        tree = self.query_one("#tree", DirectoryTree)
        with open(WORKSPACE / "tree_tabbed.log", "a", encoding="utf-8") as f:
            f.write(f"expanded={tree.root.is_expanded}, children={len(tree.root.children)}\n")
            for c in tree.root.children:
                f.write(f"  {c.label}\n")
        self.exit()

if __name__ == "__main__":
    log = WORKSPACE / "tree_tabbed.log"
    log.write_text("", encoding="utf-8")
    TabbedTreeApp().run()
