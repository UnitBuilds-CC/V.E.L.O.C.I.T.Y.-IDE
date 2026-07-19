# agent/usage.py — Per-account Cloudflare Workers AI usage tracking
import json
import os
import datetime as _dt
from pathlib import Path

_WORKSPACE_ROOT = Path(__file__).resolve().parent.parent
_USAGE_FILE = _WORKSPACE_ROOT / "memory" / ".account_usage.json"
_LEGACY_STATE = _WORKSPACE_ROOT / "memory" / ".account_state.json"

_DEFAULT_LIMITS = {"free": 50, "paid": 500}


def _today() -> str:
    return _dt.datetime.utcnow().strftime("%Y-%m-%d")


def _default_limit(tier: str) -> int:
    return _DEFAULT_LIMITS.get(tier.lower(), 50)


def _load_raw() -> dict:
    """Load usage file, resetting if stale (previous UTC day)."""
    today = _today()
    if not _USAGE_FILE.exists():
        return {"date": today, "accounts": {}}
    try:
        raw = json.loads(_USAGE_FILE.read_text(encoding="utf-8"))
        if raw.get("date") != today:
            return {"date": today, "accounts": {}}
        raw.setdefault("accounts", {})
        return raw
    except Exception:
        return {"date": today, "accounts": {}}


def _save_raw(data: dict) -> None:
    _USAGE_FILE.parent.mkdir(parents=True, exist_ok=True)
    _USAGE_FILE.write_text(json.dumps(data, indent=2), encoding="utf-8")


def _migrate_legacy_exhausted(data: dict) -> None:
    """Import exhaustion flags from the legacy .account_state.json once per day."""
    if not _LEGACY_STATE.exists():
        return
    try:
        legacy = json.loads(_LEGACY_STATE.read_text(encoding="utf-8"))
        today = _today()
        for key, entry in legacy.items():
            if entry.get("date") != today:
                continue
            acct = data["accounts"].setdefault(key, {})
            acct["exhausted"] = True
            acct.setdefault("exhausted_at", entry.get("exhausted_at"))
    except Exception:
        pass


def _ensure_account(data: dict, n: int, label: str, tier: str) -> dict:
    key = str(n)
    acct = data["accounts"].setdefault(key, {})
    acct.setdefault("label", label)
    acct.setdefault("tier", tier)
    acct.setdefault("requests", 0)
    acct.setdefault("tokens_in", 0)
    acct.setdefault("tokens_out", 0)
    acct.setdefault("exhausted", False)
    acct.setdefault("exhausted_at", None)
    env_limit = os.getenv(f"CF_ACCOUNT_{n}_DAILY_LIMIT")
    if env_limit:
        acct["daily_limit"] = int(env_limit)
    else:
        acct.setdefault("daily_limit", _default_limit(tier))
    return acct


def _parse_accounts() -> list[dict]:
    """Parse CF_ACCOUNT_N_* entries from the environment."""
    accounts = []
    n = 1
    while True:
        acct_id = os.getenv(f"CF_ACCOUNT_{n}_ID")
        token = os.getenv(f"CF_ACCOUNT_{n}_TOKEN")
        if not acct_id or not token:
            break
        tier = os.getenv(f"CF_ACCOUNT_{n}_TIER", "free").lower()
        label = os.getenv(f"CF_ACCOUNT_{n}_LABEL", f"account-{n}")
        url = (
            os.getenv(f"CF_ACCOUNT_{n}_URL")
            or f"https://api.cloudflare.com/client/v4/accounts/{acct_id}/ai/v1/chat/completions"
        )
        accounts.append({"n": n, "id": acct_id, "token": token,
                         "tier": tier, "label": label, "url": url})
        n += 1
    accounts.sort(key=lambda a: (0 if a["tier"] == "free" else 1, a["n"]))
    return accounts


def load_accounts_with_usage() -> list[dict]:
    """Return account list enriched with current usage stats."""
    data = _load_raw()
    _migrate_legacy_exhausted(data)
    accounts = _parse_accounts()
    result = []
    for acct in accounts:
        stats = _ensure_account(data, acct["n"], acct["label"], acct["tier"])
        remaining = max(0, stats["daily_limit"] - stats["requests"])
        result.append({
            **acct,
            "requests": stats["requests"],
            "tokens_in": stats["tokens_in"],
            "tokens_out": stats["tokens_out"],
            "daily_limit": stats["daily_limit"],
            "remaining": 0 if stats.get("exhausted") else remaining,
            "exhausted": bool(stats.get("exhausted")),
            "exhausted_at": stats.get("exhausted_at"),
        })
    _save_raw(data)
    return result


def is_exhausted(n: int) -> bool:
    data = _load_raw()
    return bool(data["accounts"].get(str(n), {}).get("exhausted"))


def mark_exhausted(n: int, label: str = "", tier: str = "free") -> None:
    """Mark account as quota-exhausted for today."""
    data = _load_raw()
    acct = _ensure_account(data, n, label or f"account-{n}", tier)
    acct["exhausted"] = True
    acct["exhausted_at"] = _dt.datetime.utcnow().isoformat() + "Z"
    _save_raw(data)

    # Keep legacy file in sync for backwards compatibility
    legacy = {}
    if _LEGACY_STATE.exists():
        try:
            legacy = json.loads(_LEGACY_STATE.read_text(encoding="utf-8"))
        except Exception:
            legacy = {}
    legacy[str(n)] = {"date": _today(), "exhausted_at": acct["exhausted_at"]}
    _LEGACY_STATE.parent.mkdir(parents=True, exist_ok=True)
    _LEGACY_STATE.write_text(json.dumps(legacy, indent=2), encoding="utf-8")


def record_request(
    n: int,
    label: str,
    tier: str,
    tokens_in: int = 0,
    tokens_out: int = 0,
) -> dict:
    """Increment request counter and token totals; return updated stats."""
    data = _load_raw()
    acct = _ensure_account(data, n, label, tier)
    acct["requests"] = acct.get("requests", 0) + 1
    acct["tokens_in"] = acct.get("tokens_in", 0) + tokens_in
    acct["tokens_out"] = acct.get("tokens_out", 0) + tokens_out
    _save_raw(data)
    remaining = max(0, acct["daily_limit"] - acct["requests"])
    return {
        "requests": acct["requests"],
        "tokens_in": acct["tokens_in"],
        "tokens_out": acct["tokens_out"],
        "daily_limit": acct["daily_limit"],
        "remaining": 0 if acct.get("exhausted") else remaining,
        "exhausted": bool(acct.get("exhausted")),
    }


def get_usage_summary() -> dict:
    """Return full usage snapshot for display."""
    accounts = load_accounts_with_usage()
    total_requests = sum(a["requests"] for a in accounts)
    available = sum(1 for a in accounts if not a["exhausted"])
    return {
        "date": _today(),
        "accounts": accounts,
        "total_requests": total_requests,
        "available_count": available,
        "total_count": len(accounts),
    }


def pick_available_account(accounts: list[dict]) -> dict | None:
    """Pick a non-exhausted account with remaining quota, load-balanced."""
    import time

    available = [
        a for a in accounts
        if not is_exhausted(a["n"])
    ]
    if not available:
        return None
    idx = int(time.time() * 1000) % len(available)
    return available[idx]
