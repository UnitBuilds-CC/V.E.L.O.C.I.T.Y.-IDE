import sys, os, requests
sys.path.insert(0, "agent")
import api

# Load .env
from pathlib import Path
for line in Path(".env").read_text().splitlines():
    line = line.strip()
    if line and "=" in line and not line.startswith("#"):
        k, v = line.split("=", 1)
        os.environ.setdefault(k, v)

accounts = api._load_accounts()
state    = api._load_state()

print(f"Registered accounts ({len(accounts)} total):")
for acct in accounts:
    exhausted = str(acct["n"]) in state
    test_url = f"https://api.cloudflare.com/client/v4/accounts/{acct['id']}/ai/run/@cf/meta/llama-3.1-8b-instruct"
    r = requests.post(
        test_url,
        headers={"Authorization": f"Bearer {acct['token']}"},
        json={"messages": [{"role": "user", "content": "hi"}]},
    )
    ok = r.status_code == 200
    err = "" if ok else (r.json().get("errors") or [{}])[0].get("message", "")[:60]
    tag = "[EXHAUSTED]" if exhausted else ("[OK]" if ok else "[FAIL]")
    print(f"  #{acct['n']} {acct['label']:12} tier={acct['tier']:4}  {tag}  {err}")

active = api._pick_account(accounts, state)
print(f"\nAuto-selected: {active['label'] if active else 'NONE — all exhausted'}")
