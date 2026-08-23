# Browser Session Management

## Classification
- **Category**: Browser Engine / Session
- **Files**: velocity-browser/src/session*.rs + vector_memory.rs + agent_api.rs (8 files)
- **Criticality**: High — session continuity for agent automation

## Summary

Full browser session lifecycle: core BrowserSession (2789 lines), auth reseeder for session continuity, cookie store (RFC 6265), history, IndexedDB, localStorage/sessionStorage, and vector-based semantic memory for agent page recall.

## Key Components

- `BrowserSession` (2789 LOC) — Cookies, navigation, page state, frame management
- `AuthReseeder` — Extract & reseed auth state across sessions (bearer + cookies + storage)
- Cookie store — Domain/path matching, expiry, Secure/HttpOnly/SameSite
- IndexedDB — In-process key-value object store
- Vector memory — Semantic embeddings for agent "remember this page"

## Design Decisions

- Auth reseeding enables session continuity without re-login
- Thread-local isolation per interpreter (browser per-origin model)
- Vector memory: searchable embeddings, not just page cache
