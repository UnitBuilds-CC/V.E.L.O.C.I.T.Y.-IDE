# Agent Crypto & Peer Networking

## Classification
- **Category**: Security / Collaboration
- **Files**: velocity-mcp/src/agent/crypto.rs, peer_bridge.rs, peer_link.rs, peer_robust.rs, peer_server.rs (7 files total)
- **Criticality**: High — artifact encryption + cross-device collaboration

## Summary

DPAPI-backed AES-256-GCM encryption for NDA artifacts with HKDF domain separation, plus cross-device peer messaging via a coordination bus bridge pattern.

## Encryption Hierarchy

```
OS Keyring (DPAPI CryptProtectData)
    → Master Key (32 bytes, per workspace, cached)
        → HKDF per-artifact subkeys
            → AES-256-GCM sealed NDA artifacts
```

## Peer System

- `PeerBridge` — Translates PeerMessage ↔ AgentBroadcast
- `PeerManager` — Connected peer list with inbox/outbox
- `PeerMessage` — { from, kind: Chat|TaskRequest|TaskResult|FileTransfer, payload }
- Inbound: remote messages → local coordination bus
- Outbound: local broadcasts → remote peers
