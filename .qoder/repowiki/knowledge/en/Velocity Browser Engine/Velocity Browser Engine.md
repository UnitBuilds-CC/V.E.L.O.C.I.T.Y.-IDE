# Velocity Browser Engine

## Classification
- **Category**: Primary Crate
- **Files**: ~171 source files
- **Criticality**: Critical — pure-Rust browser with no CDP

## Summary

`velocity-browser` implements a complete browser engine in pure Rust: slab-allocated DOM, flexbox/grid layout, JavaScript VM with WASM SIMD, HTTP/2-3 networking with custom TLS 1.3, and agentic features (AOM tree, OCR, action prediction).

## Module Breakdown

| Module | Files | Purpose |
|--------|-------|---------|
| `engine/` | 39 | Browser capabilities (auth, sessions, workflows) |
| `net/` | 19 | HTTP/2-3, TLS 1.3, WebSocket, WebRTC |
| `js/` | 56 | JS VM, WASM interpreter, event loop |
| `agentic/` | 10 | AOM tree, OCR, action predictor |
| `dom/` | 9 | Slab DOM tree, shadow slots, mutations |
| `layout/` | 7 | Flexbox, grid, parallel solvers |
| `parser/` | 6 | HTML parser |
| `style/` | 5 | CSS style resolution |

## Key Design Decisions

- No CDP/Chromium dependency
- Slab allocation for DOM memory efficiency
- Custom TLS 1.3 as engineering artifact
- Zero-allocation NDA writes for browser state
