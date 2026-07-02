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

# We support Cloudflare's OpenAI-compatible Workers AI endpoint
CF_ACCOUNT_ID = os.getenv("CF_ACCOUNT_ID")
CF_API_TOKEN = os.getenv("CF_API_TOKEN")

# Fallback to general environment variables if custom ones are needed
API_URL = os.getenv("CF_API_URL")
if not API_URL and CF_ACCOUNT_ID:
    API_URL = f"https://api.cloudflare.com/client/v4/accounts/{CF_ACCOUNT_ID}/ai/v1/chat/completions"

# Default model, customizable via CF_MODEL or CLOUDFLARE_MODEL env variables
MODEL = os.getenv("CF_MODEL") or os.getenv("CLOUDFLARE_MODEL") or "@cf/meta/llama-3.1-70b-instruct"

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
    payload = {
        "model": MODEL,
        "messages": messages,
        "temperature": 0.2,
    }
    if tools:
        payload["tools"] = tools
        payload["tool_choice"] = "auto"

    r = requests.post(API_URL, headers=headers, json=payload)
    r.raise_for_status()
    
    # Cloudflare OpenAI-compatible endpoint returns standard OpenAI payload
    return r.json()["choices"][0]["message"]["content"]
