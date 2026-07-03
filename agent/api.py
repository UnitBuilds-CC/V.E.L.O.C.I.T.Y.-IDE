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
# Dynamic account registry
# ---------------------------------------------------------------------------
# Add accounts in .env as:
#   CF_ACCOUNT_1_ID=...
#   CF_ACCOUNT_1_TOKEN=...
#   CF_ACCOUNT_1_TIER=free        # free | paid  (default: free)
#   CF_ACCOUNT_1_LABEL=MyAcc      # optional human label
#
# Add as many as needed (2, 3, 4, ...).  CF_PROFILE is no longer needed.
# Selection order: all free accounts first (in numbered order), then paid.
# When an account hits daily quota (4006) it is marked exhausted in
# memory/.account_state.json and skipped for the rest of the UTC day.

_WORKSPACE_ROOT = Path(__file__).resolve().parent.parent
_STATE_FILE     = _WORKSPACE_ROOT / "memory" / ".account_state.json"


def _load_accounts() -> list[dict]:
    """Parse all CF_ACCOUNT_N_* entries from the environment, sorted free-first."""
    accounts = []
    n = 1
    while True:
        acct_id = os.getenv(f"CF_ACCOUNT_{n}_ID")
        token   = os.getenv(f"CF_ACCOUNT_{n}_TOKEN")
        if not acct_id or not token:
            break
        tier  = os.getenv(f"CF_ACCOUNT_{n}_TIER",  "free").lower()
        label = os.getenv(f"CF_ACCOUNT_{n}_LABEL", f"account-{n}")
        url   = (
            os.getenv(f"CF_ACCOUNT_{n}_URL")
            or f"https://api.cloudflare.com/client/v4/accounts/{acct_id}/ai/v1/chat/completions"
        )
        accounts.append({"n": n, "id": acct_id, "token": token,
                         "tier": tier, "label": label, "url": url})
        n += 1

    # Free accounts first, then paid; preserve numbered order within each tier
    accounts.sort(key=lambda a: (0 if a["tier"] == "free" else 1, a["n"]))
    return accounts


def _load_state() -> dict:
    """Load per-account exhaustion state, discarding stale entries from previous UTC days."""
    today = _dt.datetime.utcnow().strftime("%Y-%m-%d")
    if not _STATE_FILE.exists():
        return {}
    try:
        raw = json.loads(_STATE_FILE.read_text(encoding="utf-8"))
        # Keep only entries from today
        return {k: v for k, v in raw.items() if v.get("date") == today}
    except Exception:
        return {}


def _save_state(state: dict) -> None:
    _STATE_FILE.parent.mkdir(parents=True, exist_ok=True)
    _STATE_FILE.write_text(json.dumps(state, indent=2), encoding="utf-8")


def _mark_exhausted(n: int) -> None:
    """Persist that account N is quota-exhausted for today."""
    state = _load_state()
    state[str(n)] = {
        "date":         _dt.datetime.utcnow().strftime("%Y-%m-%d"),
        "exhausted_at": _dt.datetime.utcnow().isoformat() + "Z",
    }
    _save_state(state)
    print(f"[api] Account {n} marked exhausted for today.", file=sys.stderr)


def _pick_account(accounts: list[dict], state: dict) -> dict | None:
    """Return the first non-exhausted account, or None if all are spent."""
    exhausted_ids = set(state.keys())
    for acct in accounts:
        if str(acct["n"]) not in exhausted_ids:
            return acct
    return None


def current_profile() -> str:
    """Return a human-readable label for the currently active account."""
    accounts = _load_accounts()
    state    = _load_state()
    acct     = _pick_account(accounts, state)
    return acct["label"] if acct else "none"


# Legacy shim — kept so any code that calls set_profile() doesn't crash
def set_profile(profile: str) -> None:  # noqa: ARG001
    print("[api] set_profile() is deprecated; account selection is now automatic.",
          file=sys.stderr)


# ---------------------------------------------------------------------------
# Backwards-compat helper used by context_trim / _kimi_resummary
# ---------------------------------------------------------------------------
def _resolve_profile(profile=None):  # noqa: ARG001
    """Return (account_id, token, url) for the currently active account."""
    accounts = _load_accounts()
    state    = _load_state()
    acct     = _pick_account(accounts, state)
    if acct:
        return acct["id"], acct["token"], acct["url"]
    return None, None, None


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
                lines.append(f"  RESULT: {snippet[:800].replace(chr(10), ' ')}")

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
        _, api_token, api_url = _resolve_profile()
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
    accounts = _load_accounts()
    if not accounts:
        raise ValueError(
            "No Cloudflare accounts configured. "
            "Add CF_ACCOUNT_1_ID / CF_ACCOUNT_1_TOKEN to your .env file."
        )

    trimmed_messages = context_trim(messages)

    # Outer loop: try each non-exhausted account in priority order
    while True:
        state = _load_state()
        acct  = _pick_account(accounts, state)

        if acct is None:
            raise RuntimeError(
                "All Cloudflare accounts are quota-exhausted for today. "
                "Accounts will reset tomorrow (UTC midnight)."
            )

        api_url   = acct["url"]
        api_token = acct["token"]
        label     = acct["label"]

        headers = {
            "Authorization": f"Bearer {api_token}",
            "Content-Type":  "application/json",
        }
        payload = {
            "model":       MODEL,
            "messages":    trimmed_messages,
            "temperature": 0.2,
            "stream":      True,
        }

        # Inner loop: transient-error retry on the same account
        MAX_RETRIES = 5
        delay       = 5

        for attempt in range(MAX_RETRIES + 1):
            r = requests.post(api_url, headers=headers, json=payload, stream=True)

            if r.status_code == 200:
                break  # success — fall through to streaming

            # Parse Cloudflare error code
            try:
                err_body = r.json()
                err_code = (err_body.get("errors") or [{}])[0].get("code", 0)
            except Exception:
                err_code = 0

            if err_code == 4006:
                # Quota exhausted — mark and hot-swap to next account
                print(
                    f"\n[api] Account '{label}' quota exhausted — "
                    "hot-swapping to next account...",
                    file=sys.stderr,
                )
                _mark_exhausted(acct["n"])
                break  # break inner → outer loop picks next account

            # Transient errors (capacity / 5xx) — backoff + retry same account
            is_transient = (err_code == 3040) or (r.status_code >= 500)
            if is_transient and attempt < MAX_RETRIES:
                print(
                    f"\r[api] [{label}] Transient {r.status_code}/{err_code}, "
                    f"retry in {delay}s ({attempt+1}/{MAX_RETRIES})...",
                    end="", flush=True, file=sys.stderr,
                )
                time.sleep(delay)
                delay = min(delay * 2, 60)
                continue

            # Any other non-retryable error
            print(f"[api] [{label}] Error: {r.text}", file=sys.stderr)
            r.raise_for_status()

        else:
            # Inner loop exhausted all retries without a 200 or a quota event
            print(f"[api] [{label}] Max retries exceeded.", file=sys.stderr)
            _mark_exhausted(acct["n"])
            continue  # try next account

        # If we got a 200, r is ready to stream — exit the outer loop
        if r.status_code == 200:
            print(f"[api] Using account: {label} (tier={acct['tier']})", file=sys.stderr)
            break
        # Otherwise inner loop hit a 4006 break — continue outer to pick next account


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
