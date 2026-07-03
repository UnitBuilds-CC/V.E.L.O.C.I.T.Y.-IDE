import sys, os
sys.path.insert(0, "agent")
import api

def test(profile):
    import requests
    _, token, url = api._resolve_profile(profile)
    # Use non-streaming run endpoint to keep test fast
    run_url = url.replace("/v1/chat/completions", "").rstrip("/")
    run_url = f"https://api.cloudflare.com/client/v4/accounts/{os.getenv(f'CF_ACCOUNT_ID_{profile.upper()}') or os.getenv('CF_ACCOUNT_ID')}/ai/run/@cf/meta/llama-3.1-8b-instruct"
    r = requests.post(run_url, headers={"Authorization": f"Bearer {token}"},
                      json={"messages": [{"role": "user", "content": "Say hi"}]})
    status = "OK" if r.status_code == 200 else f"FAIL {r.status_code}"
    errors = r.json().get("errors") or []
    msg = errors[0]["message"][:80] if errors else (r.json().get("result") or {}).get("response", "")[:60]
    print(f"  [{profile}] {status}  {msg}")

print("Testing profiles...")
test("primary")
test("secondary")
test("tertiary")
