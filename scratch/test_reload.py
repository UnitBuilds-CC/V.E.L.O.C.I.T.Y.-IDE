import asyncio
from pathlib import Path
from textual.app import App
from textual.widgets import DirectoryTree

class TestApp(App):
    def compose(self):
        yield DirectoryTree(r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace", id="tree")

    async def on_mount(self):
        tree = self.query_one("#tree", DirectoryTree)
        print("Calling reload...")
        try:
            tree.reload()
            print("Reload succeeded!")
        except Exception as e:
            print("Reload failed:", e)
        self.exit()

if __name__ == "__main__":
    app = TestApp()
    app.run()
