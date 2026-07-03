"""Headless smoke test for the IDE compose/layout."""
import asyncio
import sys
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent
sys.path.insert(0, str(WORKSPACE / "agent"))

from ide.app import VelocityIDE

async def run():
    app = VelocityIDE()
    async with app.run_test() as pilot:
        await pilot.pause()
        tree = app.query_one("#file-tree")
        print(f"tree root expanded={tree.root.is_expanded}, children={len(tree.root.children)}")
        editor = app.query_one("#editor")
        print(f"editor content length={len(editor.content)}")
        shell = app.query_one("#shell-pane")
        print(f"shell log lines ok")
        status = app.query_one("#status-bar")
        print(f"status={status.status}, branch={status.branch}")
        print("COMPOSE_OK")

asyncio.run(run())
