"""Test DirectoryTree with explicit expand."""
from pathlib import Path
from textual.app import App
from textual.widgets import DirectoryTree

WORKSPACE = Path(__file__).resolve().parent
LOG = WORKSPACE / "tree_test.log"

def log(msg):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(msg + "\n")

class TreeApp(App):
    CSS = """
    DirectoryTree { height: 100%; }
    """

    def compose(self):
        log("compose")
        yield DirectoryTree(str(WORKSPACE), id="tree")

    def on_mount(self):
        log("on_mount")
        tree = self.query_one("#tree", DirectoryTree)
        log(f"before expand: children={len(tree.root.children)}")
        tree.root.expand()
        log(f"after expand: children={len(tree.root.children)}")
        self.set_timer(0.5, self._check)

    def _check(self):
        tree = self.query_one("#tree", DirectoryTree)
        log(f"CHECK: expanded={tree.root.is_expanded}, children={len(tree.root.children)}")
        for c in tree.root.children:
            log(f"  child: {c.label}")
        self.exit()

if __name__ == "__main__":
    LOG.write_text("", encoding="utf-8")
    TreeApp().run()
