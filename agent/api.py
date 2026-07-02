# agent/api.py
import os
import json
import sys
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

# Use Cloudflare's OpenAI-compatible completions endpoint which has solid streaming support
API_URL = os.getenv("CF_API_URL")
if not API_URL and CF_ACCOUNT_ID:
    API_URL = f"https://api.cloudflare.com/client/v4/accounts/{CF_ACCOUNT_ID}/ai/v1/chat/completions"

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
        "stream": True
    }

    r = requests.post(API_URL, headers=headers, json=payload, stream=True)
    if r.status_code != 200:
        print(f"Cloudflare API Error Response: {r.text}", file=sys.stderr)
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
