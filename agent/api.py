# agent/api.py
import os
import json
import sys
import time
import datetime as _dt
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
# Context management — rolling summary with re-summarization
# ---------------------------------------------------------------------------
# One special system message (tagged _SUMMARY_TAG) acts as a rolling progress
# log.  When context gets long:
#   1. New events are extracted from dropped messages and appended to it.
#   2. If the summary itself grows past MAX_SUMMARY_CHARS, Kimi is called
#      (non-streaming, direct) to compress it — no context_trim recursion.

MAX_HISTORY       = 20    # recent messages to keep verbatim
MAX_SUMMARY_CHARS = 3000  # chars before we ask Kimi to re-compress the summary

# Workspace root — used to locate memory/summaries/
_WORKSPACE = Path(__file__).resolve().parent.parent


def _archive_summary(content: str, timestamp: "_dt.datetime") -> None:
    """Save a summary to memory/summaries/<ISO-timestamp>.md before it is replaced."""
    summaries_dir = _WORKSPACE / "memory" / "summaries"
    summaries_dir.mkdir(parents=True, exist_ok=True)
    safe_ts = timestamp.strftime("%Y%m%dT%H%M%SZ")
    artifact = summaries_dir / f"{safe_ts}.md"
    artifact.write_text(
        f"# Velocity Summary — archived {timestamp.isoformat()}Z\n"
        f"<!-- This snapshot was saved automatically before re-summarization -->\n\n"
        f"{content}\n",
        encoding="utf-8",
    )
    print(f"[api] Summary archived → memory/summaries/{safe_ts}.md", file=sys.stderr)

_SUMMARY_TAG = "<!-- velocity-summary -->"  # sentinel to find our summary message


import re as _re

def _extract_events(messages: list) -> str:
    """Build a compact event list from a batch of dropped messages."""
    lines = []
    for m in messages:
        role    = m.get("role", "")
        content = str(m.get("content", ""))

        if role == "assistant":
            # Native tool call format
            for match in _re.finditer(
                r"<\|tool_call_begin\|>(.*?)<\|tool_call_argument_begin\|>(.*?)<\|tool_call_end\|>",
                content, _re.DOTALL
            ):
                prefix, raw_args = match.group(1).strip(), match.group(2).strip()
                try:
                    pl = json.loads(raw_args)
                    tname = pl.get("name") or prefix.split(".")[-1].split(":")[0]
                    args  = pl.get("args", pl)
                except Exception:
                    tname, args = prefix, {}
                asum = ", ".join(f"{k}={repr(str(v)[:50])}" for k, v in (args.items() if isinstance(args, dict) else {}.items()))
                lines.append(f"- CALLED {tname}({asum})")

            # XML tool call format
            for match in _re.finditer(r"<tool>(.*?)</tool>", content, _re.DOTALL):
                try:
                    pl    = json.loads(match.group(1))
                    tname = pl.get("name", "?")
                    args  = pl.get("args", {})
                    asum  = ", ".join(f"{k}={repr(str(v)[:50])}" for k, v in args.items())
                    lines.append(f"- CALLED {tname}({asum})")
                except Exception:
                    pass

            # Plain-text conclusions (strip tags)
            plain = _re.sub(r"<\|.*?\|>.*?<\|.*?\|>", "", content, flags=_re.DOTALL)
            plain = _re.sub(r"<tool>.*?</tool>",       "", plain,   flags=_re.DOTALL).strip()
            if plain and len(plain) > 30:
                lines.append(f"  → {plain[:180].replace(chr(10), ' ')}")

        elif role == "user":
            # Tool result injected by harness
            if "<result>" in content or "<|tool_response_begin|>" in content:
                snippet = _re.sub(r"<[^>]+>", " ", content).strip()
                lines.append(f"  RESULT: {snippet[:120].replace(chr(10), ' ')}")

    return "\n".join(lines)


def _kimi_resummary(bloated: str, api_token: str, api_url: str) -> str:
    """Ask Kimi to compress an oversized summary. Direct non-streaming call."""
    payload = {
        "model": MODEL,
        "messages": [
            {
                "role": "system",
                "content": (
                    "You are a concise progress log compressor. "
                    "Compress the following agent progress log into tight bullet points. "
                    "Preserve: every tool called, every file created/modified, key decisions. "
                    "Omit: verbose reasoning, repeated info, raw file contents. "
                    "Output ONLY the compressed bullet list, no preamble."
                ),
            },
            {"role": "user", "content": bloated},
        ],
        "temperature": 0.1,
        "stream": False,
        "max_tokens": 600,
    }
    headers = {"Authorization": f"Bearer {api_token}", "Content-Type": "application/json"}
    try:
        r = requests.post(api_url, headers=headers, json=payload, timeout=45)
        if r.status_code == 200:
            data = r.json()
            choices = (
                data.get("choices")
                or (data.get("result") or {}).get("choices")
                or []
            )
            if choices:
                compressed = choices[0].get("message", {}).get("content", "").strip()
                if compressed:
                    print("[api] Summary re-compressed by Kimi.", file=sys.stderr)
                    return compressed
    except Exception as exc:
        print(f"[api] Re-summarization failed ({exc}), keeping original.", file=sys.stderr)
    return bloated  # fall back to uncompressed


def context_trim(messages: list) -> list:
    """Rolling summary: append new events; re-summarize via Kimi if too big."""
    if len(messages) <= MAX_HISTORY + 2:
        return messages

    system_msgs = [m for m in messages if m.get("role") == "system"]
    non_system  = [m for m in messages if m.get("role") != "system"]

    # Separate out our own summary message from real system messages
    real_system  = [m for m in system_msgs if _SUMMARY_TAG not in m.get("content", "")]
    summary_msgs = [m for m in system_msgs if _SUMMARY_TAG     in m.get("content", "")]
    existing_summary = summary_msgs[-1]["content"] if summary_msgs else f"{_SUMMARY_TAG}\n## Progress log\n"

    # Split non-system into (dropped | kept)
    first_user = next((m for m in non_system if m.get("role") == "user"), None)
    tail       = non_system[-(MAX_HISTORY):]
    keep_ids   = set(id(m) for m in tail)
    if first_user:
        keep_ids.add(id(first_user))
    dropped = [m for m in non_system if id(m) not in keep_ids]

    if not dropped:
        return messages  # nothing to compress

    # Append new events to rolling summary
    new_events    = _extract_events(dropped)
    updated_summary = existing_summary.rstrip() + "\n" + new_events

    # Re-summarize if the summary itself is getting bloated
    if len(updated_summary) > MAX_SUMMARY_CHARS:
        _, api_token, api_url = _resolve_profile(_active_profile)
        compress_start = _dt.datetime.utcnow()

        print(
            f"[api] Summary too long ({len(updated_summary)} chars), asking Kimi to re-compress...",
            file=sys.stderr,
        )

        # ── Archive the old summary as a timestamped artifact ──────────────
        _archive_summary(existing_summary, compress_start)

        compressed      = _kimi_resummary(updated_summary, api_token, api_url)
        compress_finish = _dt.datetime.utcnow()

        # Wrap compressed text with timestamps so the artifact is self-documenting
        updated_summary = (
            f"{_SUMMARY_TAG}\n"
            f"<!-- compressed {compress_finish.isoformat()}Z -->\n"
            f"{compressed}"
        )

    summary_msg = {"role": "system", "content": updated_summary}
    print(f"[api] Context compressed: summarised {len(dropped)} messages ({len(updated_summary)} summary chars).",
          file=sys.stderr)

    result = real_system[:]
    result.append(summary_msg)
    if first_user and id(first_user) not in set(id(m) for m in tail):
        result.append(first_user)
    result.extend(tail)
    return result



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
