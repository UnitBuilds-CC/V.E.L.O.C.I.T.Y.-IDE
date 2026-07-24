# Windows Automation (WA) Architecture

The Windows Automation (WA) subsystem (`velocity-mcp/src/wa/`) provides native desktop UI automation capabilities for Velocity agents running on Windows OS.

---

## 🏛️ Platform Architecture

```
                      ┌─────────────────────────────────┐
                      │          velocity-mcp           │
                      └────────────────┬────────────────┘
                                       │
                                       ▼
                       ┌───────────────────────────────┐
                       │       wa (Win Automation)     │
                       └───────────────┬───────────────┘
                                       │
            ┌──────────────────────────┼──────────────────────────┐
            │                          │                          │
            ▼                          ▼                          ▼
   ┌─────────────────┐        ┌─────────────────┐        ┌─────────────────┐
   │    platform     │        │     runtime     │        │     storage     │
   │ (WinUI Capture  │        │ (Action Exec &  │        │ (Snapshot & NDA │
   │ & Tree Inspect) │        │ Script Runner)  │        │ Persistence)    │
   └─────────────────┘        └─────────────────┘        └─────────────────┘
```

---

## 🔧 Core Components

### 1. Platform & Accessibility Capture (`src/wa/platform.rs` & `src/wa/windows/`)
- Interfaces directly with Windows UI Automation (UIA) APIs.
- **`execution.rs`**: Synthesizes mouse clicks, double clicks, right clicks, keystrokes, and window drag operations without physical hardware locks.
- Constructs hierarchical desktop UI element trees (window title, control type, bounding rectangle, automation ID, accessible text).

### 2. Runtime Execution (`src/wa/runtime.rs`)
- Parses action requests from MCP tools (`wa_click`, `wa_type`, `wa_run_script`).
- Handles retry conditions, timeout management, and step verification.
- Halts workflow execution safely upon encountering verification failures.

### 3. Storage & Snapshots (`src/wa/storage.rs`)
- Persists window state captures and action execution logs into compact binary NDA snapshots (`.velocity/wa_snapshots/`).
- Enables post-run action auditing and playback visual verification.
