# Browser Session Management

_Browser session lifecycle: authentication reseeding, cookie stores, history, IndexedDB, local storage, and vector memory._

---

## Overview

The `velocity-browser/src/session*.rs` files (8 modules) manage the full browser session lifecycle — from authentication state extraction and cookie persistence to IndexedDB storage and vector-based semantic memory.

---

## Module Map

| File | Lines | Purpose |
|------|-------|---------|
| `session.rs` | 2789 | Core `BrowserSession`: cookies, navigation, page state |
| `session_auth.rs` | 67 | `AuthTokenState` + `AuthReseeder`: extract & reseed auth |
| `session_cookie_store.rs` | — | Persistent cookie jar with domain/path matching |
| `session_history.rs` | — | Back/forward navigation history stack |
| `session_indexeddb.rs` | — | IndexedDB-compatible key-value object store |
| `session_storage.rs` | — | `localStorage` / `sessionStorage` implementation |
| `vector_memory.rs` | — | Semantic vector store for agent page memory |
| `agent_api.rs` | — | Agent-facing API for session inspection |

---

## BrowserSession (`session.rs`, 2789 lines)

The core session struct managing all page state:

- **Cookie management**: Parse, store, match by domain/path/expiry
- **Navigation state**: Current URL, back/forward stacks, loading state
- **Page state**: DOM tree reference, scroll position, viewport dimensions
- **Request pipeline**: Integrate with `net/` for HTTP requests with session cookies
- **Frame management**: Main frame + iframe tracking

---

## Auth Reseeder (`session_auth.rs`, 67 lines)

Extracts and reseeds authentication state across sessions:

```rust
pub struct AuthTokenState {
    pub bearer_token: Option<String>,
    pub cookies: HashMap<String, String>,
    pub storage_keys: HashMap<String, String>,
}

pub struct AuthReseeder;
```

- `extract_auth_state()`: Pulls cookies + storage keys + bearer tokens from a live session
- `reseed_into_session()`: Injects extracted auth state into a new session — enables session continuity across agent restarts
- Bearer token search order: `access_token` → `token` → `auth_token` in storage

---

## Cookie Store (`session_cookie_store.rs`)

Persistent cookie jar:
- Domain + path matching per RFC 6265
- Expiry tracking and automatic cleanup
- Secure/HttpOnly/SameSite flag enforcement
- Shared across all tabs within a session

---

## Session History (`session_history.rs`)

Navigation history:
- Back/forward stacks of visited URLs
- Session-scoped (not persisted across restarts)
- Supports `history.pushState()` and `history.replaceState()` from JS

---

## IndexedDB (`session_indexeddb.rs`)

In-process IndexedDB-compatible object store:
- Key-value storage with structured clone semantics
- Object store creation/deletion
- Transaction support (read-only, read-write)
- Used by page scripts for persistent client-side data

---

## Session Storage (`session_storage.rs`)

Web Storage API implementation:
- `localStorage`: Persists across sessions (file-backed)
- `sessionStorage`: Cleared on session end (in-memory)
- Key-value string storage matching the W3C spec
- Thread-local isolation per interpreter instance

---

## Vector Memory (`vector_memory.rs`)

Semantic vector store for agent page memory:
- Stores page content as embedding vectors
- Enables similarity search over previously visited pages
- Used by the agent empowerment layer for "remember this page" functionality
- Integrates with the AOM (Agent Object Model) for structured page data

---

## Key Design Decisions

- **Auth reseeding**: Enables agents to maintain authenticated sessions across restarts without re-login
- **Thread-local isolation**: Each interpreter gets its own storage — matches browser per-origin model
- **RFC 6265 cookies**: Standards-compliant cookie matching for real-world web compatibility
- **Vector memory**: First-class semantic memory for agents — not just page cache but searchable embeddings

---

## See Also

- [Agentic Browser Subsystem](agentic_browser.md) — AOM tree, action predictor
- [JS Interpreter & Runtime](js_interpreter_runtime.md) — Browser environment APIs (localStorage, timers)
- [Browser Engine & Networking](../architecture/velocity_browser.md) — Engine capabilities, TLS, HTTP
