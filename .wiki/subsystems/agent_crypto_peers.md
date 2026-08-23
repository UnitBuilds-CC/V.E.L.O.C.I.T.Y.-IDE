# Agent Crypto & Peer Networking

_At-rest encryption for NDA artifacts, cross-device peer messaging, and coordination bus bridging._

---

## Overview

The `velocity-mcp/src/agent/` module contains 7 files dedicated to agent security and cross-device collaboration: crypto (DPAPI-backed AES-256-GCM encryption), peer_bridge (coordination bus ↔ peer messaging), peer_link (peer connection management), peer_robust (resilient peer communication), and peer_server (incoming peer connections).

---

## Agent Crypto (`agent/crypto.rs`, 366 lines)

### Encryption Model

At-rest encryption for `.velocity/*.nda` artifacts using a hierarchical key model:

```
OS Keyring (DPAPI on Windows)
    │
    ▼ CryptUnprotectData (per-user, per-machine)
Master Key (32 bytes, cached per workspace)
    │
    ▼ HKDF domain separation
Per-Artifact Subkeys (sitemap, chat, transcripts, etc.)
    │
    ▼ AES-256-GCM (hardware AES-NI)
Sealed NDA Artifacts + SHA-256 AEAD tag
```

### Key Architecture

```rust
const MASTER_KEY_LEN: usize = 32;
const KEY_FILE: &str = "nda.key";

// Cache of decrypted per-workspace master keys
static KEY_CACHE: Lazy<Mutex<HashMap<PathBuf, [u8; MASTER_KEY_LEN]>>> = ...;
```

- **Master key**: One 32-byte key per workspace, generated once, sealed by OS keyring
- **HKDF derivation**: Per-artifact subkeys via `velocity_browser::nda::derive_nda_key` — ensures sitemap and chat transcripts never share a key
- **AES-256-GCM**: Payloads sealed with the `NDA1` envelope from velocity-browser, binding SHA-256 integrity tag + header as AEAD additional data
- **Key cache**: `HashMap<PathBuf, [u8; 32]>` — hits OS keyring at most once per workspace per process

### Windows OS Primitives

```rust
extern "system" {
    fn CryptProtectData(...);    // Seal key with DPAPI
    fn CryptUnprotectData(...);  // Unseal key from DPAPI
    fn BCryptGenRandom(...);     // Cryptographic RNG
}
```

- `ENTROPY = b"velocity-nda-keyring-v1"` — ties sealed blob to this application
- `CRYPTPROTECT_UI_FORBIDDEN` — no UI prompts during headless operation

---

## Peer-to-Peer System

### PeerBridge (`agent/peer_bridge.rs`, 478 lines)

Bridge between the cross-device peer system and the local coordination bus:

```rust
pub struct PeerBridge {
    bus: CoordinationBus,
    peer_mgr: PeerManager,
}
```

**Inbound**: Remote peer messages → `AgentBroadcast` on local bus
- `PeerMessageKind::Chat` → `AgentBroadcast::HelpRequested` (surfaces in orchestration feed)
- `PeerMessageKind::TaskRequest` → `AgentBroadcast::TaskAssigned` (remote agent needs work done)

**Outbound**: Local broadcasts → forwarded to interested remote peers

### PeerLink (`agent/peer_link.rs`)

Peer connection management:
- `PeerManager`: maintains list of connected peers with inbox/outbox
- `PeerMessage`: `{ from, kind, payload }` — simple message envelope
- `PeerMessageKind`: Chat, TaskRequest, TaskResult, FileTransfer

### PeerRobust (`agent/peer_robust.rs`)

Resilient peer communication:
- Retry logic for transient failures
- Message acknowledgment tracking
- Connection health monitoring

### PeerServer (`agent/peer_server.rs`)

Incoming peer connection handler:
- Listens for new peer connections
- Authenticates peer identity
- Registers with PeerManager

---

## Key Design Decisions

- **DPAPI over custom keyring**: Leverages Windows built-in key management — no password prompts, bound to user account
- **HKDF domain separation**: Each artifact class gets its own subkey — compromising one doesn't compromise others
- **NDA1 envelope**: Reuses the browser's vetted encryption wrapper — no custom crypto in the agent layer
- **Peer bridge pattern**: Clean separation between transport (peer_link) and coordination (coordination_bus) — peers appear as local agents
- **Lazy key loading**: Master key loaded from OS keyring on first access, cached for process lifetime

---

## See Also

- [NDA Format & Security Model](nda_security.md) — NDA binary format spec and integrity chain
- [Connectors & Security](connectors_security.md) — External service connectors and encrypted secrets
- [Multi-Agent Task Orchestrator](multi_agent_orchestrator.md) — Coordination bus consumer
- [Drone Subsystem](drone_subsystem.md) — Portable agent endpoint for cross-device collaboration
