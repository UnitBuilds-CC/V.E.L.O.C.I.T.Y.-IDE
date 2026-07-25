# Custom TLS 1.3 & Networking Stack

`velocity-browser` features a pure Rust networking and security stack in `velocity-browser/src/net/` (17 files). It operates without external C/C++ dependencies like OpenSSL or BoringSSL.

> **Note**: The production TLS trust boundary uses `rustls` (ring provider) configured in `Cargo.toml`. The from-scratch TLS 1.3 stack documented here is an engineering artifact demonstrating zero-dependency capability.

---

## Cryptographic Primitives

Implemented from scratch in idiomatic Rust:

| Module | Algorithm | Purpose |
|--------|-----------|---------|
| `x25519.rs` | Curve25519 ECDH | Key exchange |
| `aes_gcm.rs` | AES-128/256-GCM | Authenticated encryption (AEAD) |
| `chacha20poly1305.rs` | ChaCha20-Poly1305 | High-speed AEAD alternative |
| `inflate.rs` | DEFLATE / gzip | Decompression |

---

## TLS 1.3 Protocol Implementation

| Module | Responsibility |
|--------|---------------|
| `tls13.rs` | Client handshake state machine |
| `tls_handshake.rs` | ClientHello, ServerHello, EncryptedExtensions, Certificate, CertificateVerify, Finished flight messages |
| `tls_record.rs` | TLS inner plaintext record encapsulation and decryption |

### Handshake Flow

```
Client                              Server
  │                                    │
  ├── ClientHello ──────────────────▶  │
  │   (supported_versions, key_share,  │
  │    cipher_suites, extensions)      │
  │                                    │
  │  ◀───────────── ServerHello ───────┤
  │   (selected cipher, key_share)     │
  │                                    │
  │  ◀──────── EncryptedExtensions ────┤
  │  ◀──────────── Certificate ────────┤
  │  ◀──────── CertificateVerify ──────┤
  │  ◀────────────── Finished ─────────┤
  │                                    │
  ├── Finished ─────────────────────▶  │
  │                                    │
  │  ◀═══════ Application Data ═══════▶│
```

---

## Advanced Web Network Protocols

| Module | Protocol | Description |
|--------|----------|-------------|
| `http_client.rs` | HTTP/1.1 + HTTP/2 | Async client with connection pooling, keep-alive, redirects, cookies |
| `http2_ws.rs` | HTTP/2 + WebSocket | Multiplexed frame parser (HEADERS, DATA, SETTINGS, PING, RST_STREAM) + RFC 6455 |
| `quic.rs` | QUIC / HTTP/3 | `QuicConnection`, `QuicStream` for low-latency transport |
| `webrtc.rs` | WebRTC | Peer-to-peer data channel transport with full SDP/ICE state machine |
| `bluetooth.rs` | Web Bluetooth | `WebBluetoothTransport`, `BluetoothDevice` API layer |
| `websocket.rs` | WebSocket | `NativeWsClient`, `WsFrame` for persistent connections |

---

## TLS Fingerprint & Proxy

| Module | Purpose |
|--------|---------|
| `tls_fingerprint.rs` | `TlsFingerprintRotator` rotates JA3/JA4 fingerprints to avoid bot detection |
| `proxy.rs` | `ProxyResolver` with `ProxyType` support (HTTP, SOCKS4, SOCKS5) |
| `inspector.rs` | `InspectorServer` for DevTools protocol integration |

### TLS Fingerprint Rotation

```rust
pub struct TlsFingerprintRotator { ... }
pub struct TlsJa3Profile { ... }
```

Rotates TLS client hello fingerprints to match common browser profiles, preventing server-side bot detection based on JA3/JA4 hash analysis.

---

## WebRTC State Machine

Full WebRTC data channel implementation:

```rust
pub enum SignalingState { Stable, HaveLocalOffer, HaveRemoteOffer, ... }
pub enum ConnectionState { New, Connecting, Connected, Disconnected, Failed, Closed }
pub enum IceConnectionState { New, Checking, Connected, Completed, Failed, Disconnected }
pub struct RtcConfiguration { ... }
pub struct DataChannel { ... }
pub struct MediaStreamTrack { ... }
```

---

## See Also

- [velocity-browser: Engine & Networking](../architecture/velocity_browser.md) — Full module inventory
- [Agentic Browser Subsystem](agentic_browser.md) — AOM, action prediction, reflection
