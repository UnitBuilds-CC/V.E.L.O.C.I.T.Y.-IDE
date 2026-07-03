"""Headless test for DirectoryTree rendering."""
import asyncio
from pathlib import Path
from textual.widgets import DirectoryTree
from textual.app import App

WORKSPACE = Path(__file__).resolve().parent

class TreeApp(App):
    def compose(self):
        yield DirectoryTree(str(WORKSPACE), id="tree")

    def on_mount(self):
        tree = self.query_one("#tree", DirectoryTree)
        print("MOUNTED tree")
        print("root expanded:", tree.root.is_expanded)
        print("root child count:", len(tree.root.children))
        print("root label:", tree.root.label)
        for child in tree.root.children:
            print("  child:", child.label)
        self.exit()

if __name__ == "__main__":
    app = TreeApp()
    app.run()
