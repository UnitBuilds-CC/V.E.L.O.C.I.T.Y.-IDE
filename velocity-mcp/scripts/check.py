#!/usr/bin/env python3
"""
Run cargo check/tests/clippy and emit a structured diagnostics file.
Used by the orchestrator validator.
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "target" / "diagnostics.json"


def run(cmd: list[str]) -> dict:
    result = subprocess.run(
        cmd,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    return {
        "cmd": " ".join(cmd),
        "ok": result.returncode == 0,
        "output": result.stdout,
    }


def main() -> int:
    report = {
        "check": run(["cargo", "check", "--all-targets", "--message-format=short"]),
        "test": run(["cargo", "test", "--message-format=short"]),
        "clippy": run(["cargo", "clippy", "--", "-D", "warnings"]),
    }
    OUT.parent.mkdir(exist_ok=True)
    OUT.write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
    return 0 if all(v["ok"] for v in report.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
