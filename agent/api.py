# agent/api.py
import os
from pathlib import Path
import requests

# Load .env if it exists
env_path = Path(__file__).resolve().parent.parent / ".env"
if env_path.exists():
    for line in env_path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            os.environ.setdefault(k.strip(), v.strip())

CF_ACCOUNT_ID = os.getenv("CF_ACCOUNT_ID")
CF_API_TOKEN = os.getenv("CF_API_TOKEN")

# Default model, customizable via CF_MODEL or CLOUDFLARE_MODEL env variables
MODEL = os.getenv("CF_MODEL") or os.getenv("CLOUDFLARE_MODEL") or "@cf/moonshotai/kimi-k2.7-code"

# Use the direct execution URL format for Workers AI
API_URL = os.getenv("CF_API_URL")
if not API_URL and CF_ACCOUNT_ID:
    API_URL = f"https://api.cloudflare.com/client/v4/accounts/{CF_ACCOUNT_ID}/ai/run/{MODEL}"

def call_kimi(messages, tools=None):
    if not API_URL or not CF_API_TOKEN:
        raise ValueError(
            "Missing Cloudflare API configuration. "
            "Please ensure CF_ACCOUNT_ID and CF_API_TOKEN environment variables are set."
        )

    headers = {
        "Authorization": f"Bearer {CF_API_TOKEN}",
        "Content-Type": "application/json"
    }
    # Direct execution endpoint does not use/need the "model" field or "tools" in payload
    payload = {
        "messages": messages,
        "temperature": 0.2,
    }

    r = requests.post(API_URL, headers=headers, json=payload)
    if r.status_code != 200:
        import sys
        print(f"Cloudflare API Error Response: {r.text}", file=sys.stderr)
    r.raise_for_status()
    
    # Returns standard OpenAI response wrapped under "result"
    return r.json()["result"]["choices"][0]["message"]["content"]
