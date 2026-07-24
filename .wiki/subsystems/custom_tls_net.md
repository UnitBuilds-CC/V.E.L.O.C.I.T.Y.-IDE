# Custom TLS 1.3 & Networking Stack

`velocity-browser` features a pure Rust networking and security stack located in `velocity-browser/src/net/`. It operates completely without external native C/C++ dependencies like OpenSSL or BoringSSL.

---

## 🔒 Cryptographic Primitives

Implemented from scratch in idiomatic Rust:
- **`x25519.rs`**: Elliptic-curve Diffie-Hellman (ECDH) key exchange algorithm over Curve25519.
- **`aes_gcm.rs`**: Authenticated Encryption with Associated Data (AEAD) using AES-128 and AES-256 in Galois/Counter Mode.
- **`chacha20poly1305.rs`**: High-speed AEAD construction combining ChaCha20 stream cipher with Poly1305 authenticator.
- **`inflate.rs`**: DEFLATE / gzip decompression algorithm implementation.

---

## 🌐 TLS 1.3 Protocol Implementation

- **`tls13.rs`**: State machine for handling TLS 1.3 client handshakes.
- **`tls_handshake.rs`**: Constructs and verifies `ClientHello`, `ServerHello`, `EncryptedExtensions`, `Certificate`, `CertificateVerify`, and `Finished` flight messages.
- **`tls_record.rs`**: Encapsulates and decrypts TLS inner plaintext records.

---

## 📡 Advanced Web Network Protocols

- **`http2_ws.rs`**: Multiplexed HTTP/2 frame parser (HEADERS, DATA, SETTINGS, PING, RST_STREAM) and WebSocket RFC 6455 protocol handler.
- **`http_client.rs`**: Asynchronous HTTP/1.1 and HTTP/2 client with connection pooling, keep-alive, redirect tracking, and cookie management.
- **`webrtc.rs`**: Peer-to-peer WebRTC transport data channel connection manager.
- **`bluetooth.rs`**: Web Bluetooth API interface layer.
