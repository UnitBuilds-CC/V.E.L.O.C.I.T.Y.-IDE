"""Test DirectoryTree inside TabbedContent."""
from pathlib import Path
from textual.app import App
from textual.widgets import TabbedContent, TabPane, DirectoryTree, Static

WORKSPACE = Path(__file__).resolve().parent

class TabbedTreeApp(App):
    CSS = """
    TabbedContent { height: 100%; }
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
        self.call_after_refresh(self._check_tree, tree)

    def _check_tree(self, tree):
        print("TREE CHECK")
        print("root expanded:", tree.root.is_expanded)
        print("root children:", len(tree.root.children))
        for c in tree.root.children:
            print("  ", c.label)
        self.exit()

if __name__ == "__main__":
    TabbedTreeApp().run()
