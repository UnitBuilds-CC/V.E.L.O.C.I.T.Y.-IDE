# agent/api.py
import os
import json
import sys
import time
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

# ---------------------------------------------------------------------------
# Multi-account profile resolution
# ---------------------------------------------------------------------------
# Set CF_PROFILE=primary or CF_PROFILE=secondary in .env (or environment).
# Falls back gracefully to bare CF_ACCOUNT_ID / CF_API_TOKEN for backwards compat.

def _resolve_profile(profile: str | None = None):
    """Return (account_id, api_token, api_url) for the requested profile."""
    p = (profile or os.getenv("CF_PROFILE", "primary")).lower()
    suffix = p.upper()
    account_id = (
        os.getenv(f"CF_ACCOUNT_ID_{suffix}")
        or os.getenv("CF_ACCOUNT_ID")
    )
    api_token = (
        os.getenv(f"CF_API_TOKEN_{suffix}")
        or os.getenv("CF_API_TOKEN")
    )
    api_url = os.getenv("CF_API_URL")
    if not api_url and account_id:
        api_url = (
            f"https://api.cloudflare.com/client/v4/accounts/"
            f"{account_id}/ai/v1/chat/completions"
        )
    return account_id, api_token, api_url

# Active profile — can be overridden at runtime via set_profile()
_active_profile: str | None = None

def set_profile(profile: str):
    """Switch the active Cloudflare account profile ('primary' or 'secondary')."""
    global _active_profile
    _active_profile = profile.lower()
    _, _, url = _resolve_profile(_active_profile)
    print(f"[api] Switched to profile '{_active_profile}' → {url}", file=sys.stderr)

def current_profile() -> str:
    return _active_profile or os.getenv("CF_PROFILE", "primary")

# Default model, customizable via CF_MODEL or CLOUDFLARE_MODEL env variables
MODEL = os.getenv("CF_MODEL") or os.getenv("CLOUDFLARE_MODEL") or "@cf/moonshotai/kimi-k2.7-code"


# ---------------------------------------------------------------------------
# Context management
# ---------------------------------------------------------------------------
# Kimi's reasoning chains are verbose. After many tool turns the conversation
# exceeds the model's practical context window, causing 500 errors.
# We keep: system message + original user request + last MAX_HISTORY messages.

MAX_HISTORY = 30  # tunable: number of recent messages to preserve


def context_trim(messages: list) -> list:
    """Return a pruned copy of messages safe to send to the API."""
    if len(messages) <= MAX_HISTORY + 2:
        return messages  # nothing to trim yet

    system_msgs = [m for m in messages if m.get("role") == "system"]
    # First user message is the original task — always keep it
    first_user = next((m for m in messages if m.get("role") == "user"), None)
    tail = messages[-(MAX_HISTORY):]

    pruned = system_msgs[:]
    if first_user and first_user not in tail:
        pruned.append(first_user)
    pruned.extend(tail)

    trimmed = len(messages) - len(pruned)
    if trimmed > 0:
        print(f"[api] Context trimmed: dropped {trimmed} old messages to stay within limits.",
              file=sys.stderr)
    return pruned


def call_kimi(messages, tools=None):
    account_id, api_token, api_url = _resolve_profile(_active_profile)

    if not api_url or not api_token:
        raise ValueError(
            "Missing Cloudflare API configuration. "
            "Please ensure CF_ACCOUNT_ID and CF_API_TOKEN environment variables are set."
        )

    headers = {
        "Authorization": f"Bearer {api_token}",
        "Content-Type": "application/json"
    }

    payload = {
        "model": MODEL,
        "messages": context_trim(messages),  # <-- trim before sending
        "temperature": 0.2,
        "stream": True
    }


    MAX_RETRIES = 5
    delay = 5  # seconds between retries

    for attempt in range(MAX_RETRIES + 1):
        r = requests.post(api_url, headers=headers, json=payload, stream=True)

        if r.status_code == 200:
            break  # success — proceed to stream

        # Parse the error code from the response body
        try:
            err_body = r.json()
            err_code = (err_body.get("errors") or [{}])[0].get("code", 0)
        except Exception:
            err_code = 0

        if err_code == 4006:
            # Quota exhausted — no point retrying
            print(f"[profile:{current_profile()}] Cloudflare API Error Response: {r.text}", file=sys.stderr)
            r.raise_for_status()

        # Transient errors: capacity (3040) or any 5xx — retry with backoff
        is_transient = (err_code == 3040) or (r.status_code >= 500)
        if is_transient and attempt < MAX_RETRIES:
            print(f"\r[api] Transient error {r.status_code}/{err_code}, retrying in {delay}s (attempt {attempt+1}/{MAX_RETRIES})...",
                  end="", flush=True, file=sys.stderr)
            time.sleep(delay)
            delay = min(delay * 2, 60)  # exponential backoff, cap at 60s
            continue

        # Any other error — fail immediately
        print(f"[profile:{current_profile()}] Cloudflare API Error Response: {r.text}", file=sys.stderr)
        r.raise_for_status()
    
    full_response = ""
    
    # Iterate over the server-sent events stream
    for line in r.iter_lines():
        if line:
            decoded_line = line.decode('utf-8').strip()
            if decoded_line.startswith("data:"):
                data_str = decoded_line[5:].strip()
                if data_str == "[DONE]":
                    break
                try:
                    data = json.loads(data_str)
                    
                    # Try parsing standard OpenAI choices/delta structure
                    choices = data.get("choices")
                    if not choices and "result" in data and isinstance(data["result"], dict):
                        choices = data["result"].get("choices")
                        
                    if choices:
                        delta = choices[0].get("delta", {})
                        
                        # Handle reasoning content if emitted by Kimi
                        reasoning = delta.get("reasoning_content", "")
                        if reasoning:
                            # Print reasoning content directly (e.g. thoughts)
                            sys.stdout.write(reasoning)
                            sys.stdout.flush()
                            full_response += reasoning
                            
                        # Handle normal content
                        content = delta.get("content", "")
                        if content:
                            sys.stdout.write(content)
                            sys.stdout.flush()
                            full_response += content
                            
                    # Fallback to standard Cloudflare response field
                    elif "response" in data:
                        content = data["response"]
                        sys.stdout.write(content)
                        sys.stdout.flush()
                        full_response += content
                    elif "result" in data and isinstance(data["result"], dict) and "response" in data["result"]:
                        content = data["result"]["response"]
                        sys.stdout.write(content)
                        sys.stdout.flush()
                        full_response += content
                        
                except Exception:
                    pass
                    
    sys.stdout.write("\n")
    sys.stdout.flush()
    return full_response
