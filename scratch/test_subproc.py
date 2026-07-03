import asyncio
import sys
from pathlib import Path
from textual.app import App

LOG = Path("subproc_test.log")

def log(msg):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(msg + "\n")

class SubprocApp(App):
    async def on_mount(self):
        log("on_mount called")
        try:
            log("Spawning subprocess...")
            proc = await asyncio.create_subprocess_exec(
                sys.executable, "-c", "print('hello')",
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout, stderr = await proc.communicate()
            log(f"Subprocess succeeded! stdout: {stdout.decode().strip()}")
        except Exception as e:
            log(f"Subprocess failed: {e}")
        self.exit()

if __name__ == "__main__":
    LOG.write_text("", encoding="utf-8")
    SubprocApp().run()
