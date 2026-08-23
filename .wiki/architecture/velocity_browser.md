# velocity-browser: Engine & Networking

The `velocity-browser` crate (171 source files) implements a complete pure-Rust browser engine: DOM tree, CSS layout, JavaScript VM, GPU compositor, networking stack, CAPTCHA solver, and session management — all without external C/C++ dependencies.

---

## DOM Tree & Mutation System

### Slab DOM Tree (`dom/`)

```
dom/
├── mod.rs              # Module root, re-exports
├── tree.rs             # SlabDomTree: arena-allocated DOM nodes
├── mutation_observer.rs # NativeMutationObserver: fine-grained change tracking
├── mutation_batcher.rs # MutationBatcher: atomic DOM change batching
├── custom_elements.rs  # CustomElementRegistry, CustomElementDefinition
├── intersection_observer.rs # Element visibility monitoring
├── form.rs             # FormDataSerializer
└── shadow_dom.rs       # SlotProjection, SlotProjectionEngine
```

**SlabDomTree** uses arena allocation with slab-based node storage:
- `RawSlabNode` — individual DOM node in the slab arena
- `UnmanagedSlabArena` — raw slab memory management
- Node dirty/visible flags: `SLAB_NODE_DIRTY`, `SLAB_NODE_VISIBLE`
- Parent/sibling indexing for O(1) traversal

### Mutation System

- **MutationBatcher**: Collects individual DOM mutations into atomic batches for AI reflection
- **NativeMutationObserver**: `MutationRecord` tracking with observer callbacks
- **SlotProjectionEngine**: Shadow DOM slot projection for Web Components

---

## Layout Engine (Flexbox & Grid)

### Module Structure (`layout/`)

```
layout/
├── mod.rs              # Module root, re-exports
├── engine.rs           # LayoutEngine2D: main layout computation
├── flexbox.rs          # FlexLayoutEngine, FlexAlignmentSolver
├── grid.rs             # GridTrackSolver, GridTrack
├── parallel.rs         # ParallelLayoutEngine: concurrent layout passes
├── box.rs              # LayoutBox: computed dimensions and positions
└── types.rs            # DisplayMode, FlexDirection, AlignItems, etc.
```

### Key Types

```rust
// Layout computation
pub struct LayoutEngine2D { ... }
pub struct ParallelLayoutEngine { ... }

// Flexbox
pub struct FlexLayoutEngine { ... }
pub struct FlexAlignmentSolver { ... }
pub enum FlexDirection { Row, RowReverse, Column, ColumnReverse }
pub enum AlignItems { FlexStart, FlexEnd, Center, Baseline, Stretch }
pub enum JustifyContent { FlexStart, FlexEnd, Center, SpaceBetween, SpaceAround, SpaceEvenly }

// Grid
pub struct GridTrackSolver { ... }
pub struct GridTrack { ... }

// Output
pub struct LayoutBox { ... }
pub enum DisplayMode { Block, Inline, Flex, Grid, None }
```

### Layout Pipeline

1. **Style resolution**: CSS cascade produces computed styles per element
2. **Box generation**: LayoutBox tree constructed from DOM + computed styles
3. **Flex/Grid solving**: FlexAlignmentSolver or GridTrackSolver compute track sizes
4. **Parallel pass**: ParallelLayoutEngine distributes independent subtrees across threads
5. **Final positioning**: Absolute coordinates computed for each LayoutBox

---

## JS Virtual Machine & WASM

### Module Structure (`js/`)

```
js/
├── mod.rs              # Module root, re-exports
├── vm.rs               # JsVirtualMachine: ES6+ interpreter
├── interpreter.rs      # Instruction dispatch, scope chains
├── event_loop.rs       # JsEventLoopScheduler: micro/macrotask queues
├── dom_api.rs          # DOM bindings (Document, Element, querySelector)
├── web_apis.rs         # Browser APIs (fetch, console, setTimeout)
├── wasm_simd.rs        # WasmInterpreter, WasmSimdPipeline
├── worker_pool.rs      # WebWorkerPool, WorkerThread
├── events.rs           # JsEventListener, SyntheticEventDispatcher
├── pointer.rs          # PointerEvent handling
├── values.rs           # JsValue, WasmValue, WasmV128Vector
├── tasks.rs            # ScheduledTask, TaskKind
└── messages.rs         # WorkerMessage types
```

### JsVirtualMachine

- ES6+ syntax support with scope chains and closure environments
- Object prototype chain
- `JsValue` enum for dynamic typing
- DOM API bindings for `document.*`, `element.*`
- Web API bindings: `fetch`, `console`, `setTimeout`, `setInterval`

### Event Loop

`JsEventLoopScheduler` manages:
- **Microtask queue**: Promise resolutions, queueMicrotask
- **Macrotask queue**: setTimeout, setInterval, I/O callbacks
- `ScheduledTask` with `TaskKind` classification

### WASM SIMD

`WasmInterpreter` with `WasmSimdPipeline`:
- `WasmV128Vector` — 128-bit SIMD vector type
- `WasmValue` — WASM value types (i32, i64, f32, f64, v128)
- Vectorized execution for compute-heavy workloads

### Web Workers

`WebWorkerPool` manages worker threads:
- `WorkerThread` — individual worker execution context
- `WorkerMessage` — inter-worker communication
- Isolated scope per worker

---

## Engine Capabilities (Canvas, WebGPU, GPU Compositor)

### Module Structure (`engine/`)

```
engine/
├── mod.rs              # Module root
├── gpu_compositor.rs   # Hardware-accelerated layer compositing
├── webgpu.rs           # WebGPU API surface
├── webgl.rs            # WebGL context
├── canvas.rs           # HTML5 Canvas 2D context
├── canvas_context.rs   # Canvas rendering state
├── rasterizer.rs       # Software rasterizer fallback
├── svg.rs              # SVG rendering
├── crypto.rs           # Web Crypto API
├── audio.rs            # Web Audio API
├── files.rs            # File API / FileReader
├── geolocation.rs      # Geolocation API
├── network.rs          # Network information API
├── payment.rs          # Payment Request API
├── pdf_extractor.rs    # PDF content extraction
├── profile.rs          # Browser profile management
├── push_notifications.rs # Push API
├── sandbox.rs          # iframe sandbox policy
├── service_worker.rs   # Service Worker lifecycle
├── shadow_dom.rs       # Shadow DOM rendering
├── stealth_human.rs    # Anti-bot-detection humanization
├── captcha_solver.rs   # CAPTCHA solving engine
├── interstitial.rs     # Interstitial/ad handling
├── trace.rs            # Performance tracing
├── webcodecs.rs        # WebCodecs API
└── types.rs            # Engine types (if present)
```

### GPU Compositor

`gpu_compositor.rs` implements hardware-accelerated compositing:
- Layer tree construction from layout output
- Compositing operations (opacity, transform, blend)
- Integration with `webgpu.rs` for modern GPU API access

### Canvas

- `canvas.rs`: HTML5 `<canvas>` 2D rendering context
- `canvas_context.rs`: Drawing state management (transforms, styles, paths)
- `rasterizer.rs`: Software fallback when GPU unavailable

### Platform APIs

Modern browser platform features implemented in pure Rust:
- **Service Workers** (`service_worker.rs`): Lifecycle management, fetch events
- **Push Notifications** (`push_notifications.rs`): Push API subscription
- **Web Crypto** (`crypto.rs`): SubtleCrypto operations
- **Geolocation** (`geolocation.rs`): Position API
- **Payment Request** (`payment.rs`): Payment flow API
- **WebCodecs** (`webcodecs.rs`): Media encoding/decoding

### Agent-Specific Capabilities

- **Stealth/Humanization** (`stealth_human.rs`): Anti-bot-detection measures
- **CAPTCHA Solver** (`captcha_solver.rs`): Automated CAPTCHA handling
- **PDF Extractor** (`pdf_extractor.rs`): Content extraction from PDF documents

---

## Networking Stack & TLS 1.3

### Module Structure (`net/`)

```
net/
├── mod.rs              # Module root, re-exports
├── http_client.rs      # HTTP/1.1 and HTTP/2 client
├── http2_ws.rs         # HTTP/2 frame parser + WebSocket RFC 6455
├── http3_quic.rs       # QUIC/HTTP3 connection
├── tls13.rs            # TLS 1.3 client handshake state machine
├── tls.rs              # TLS stream wrapper
├── tls_handshake.rs    # ClientHello/ServerHello/Finished flight messages
├── tls_record.rs       # TLS record layer encapsulation
├── tls_fingerprint.rs  # TlsFingerprintRotator, TlsJa3Profile
├── tls_sigverify.rs    # TLS signature verification
├── tls_trust.rs        # TLS trust anchor management
├── x25519.rs           # Curve25519 ECDH key exchange
├── x509.rs             # X.509 certificate parsing
├── aes_gcm.rs          # AES-128/256-GCM AEAD
├── chacha20poly1305.rs # ChaCha20-Poly1305 AEAD
├── inflate.rs          # DEFLATE/gzip decompression
├── webrtc.rs           # WebRTC data channel transport
├── bluetooth.rs        # Web Bluetooth API
├── proxy.rs            # ProxyResolver, ProxyType
├── inspector.rs        # InspectorServer (DevTools protocol)
└── websocket.rs        # NativeWsClient, WsFrame
```

### TLS 1.3 Implementation

Pure Rust TLS 1.3 state machine (from-scratch engineering artifact):
- `tls13.rs`: Client handshake state machine
- `tls_handshake.rs`: Full flight message construction and verification
- `tls_record.rs`: Record layer encryption/decryption
- Crypto primitives: X25519 key exchange, AES-GCM and ChaCha20-Poly1305 AEAD

**Important**: The production TLS trust boundary uses `rustls` (ring provider) via `velocity-browser/Cargo.toml`. The from-scratch TLS stack is an engineering artifact demonstrating zero-dependency capability.

### Key Network Types

```rust
pub struct HttpClient { ... }
pub struct HttpResponse { ... }
pub struct QuicConnection { ... }
pub struct QuicStream { ... }
pub struct NativeWsClient { ... }
pub struct NativeTlsStream { ... }
pub struct TlsFingerprintRotator { ... }
pub struct TlsJa3Profile { ... }
pub struct ProxyResolver { ... }
pub struct InspectorServer { ... }
pub struct WebRtcTransport { ... }
pub struct WebBluetoothTransport { ... }
pub struct BluetoothDevice { ... }
```

### WebRTC

`WebRtcTransport` manages peer-to-peer data channels:
- `SignalingState`, `ConnectionState`, `IceConnectionState`
- `IceCandidateState`, `SdpType`, `SessionDescription`
- `DataChannel`, `DataChannelState`
- `MediaStreamTrack`, `TrackKind`, `TrackState`
- `RtcConfiguration`, `BundlePolicy`, `IceServer`

---

## Session Management & Storage

### Browser Session (`session.rs`)

```rust
pub struct BrowserSession { ... }
pub struct Cookie { ... }
```

### Session Subsystems

| Module | Purpose |
|--------|---------|
| `session_auth.rs` | `AuthReseeder`, `AuthTokenState` — authentication lifecycle |
| `session_cookie_store.rs` | `CookieStore`, `CookieRecord`, `SameSitePolicy` |
| `session_history.rs` | `HistoryStack`, `HistoryItem` — back/forward navigation |
| `session_storage.rs` | `SessionStorageDisk` — persistent key-value storage |
| `session_storage_events.rs` | `StorageEventBroadcaster`, `StorageEventRecord` |
| `session_storage_quota.rs` | `StorageQuotaManager`, `StorageQuotaEstimate` |
| `session_indexeddb.rs` | `IndexedDbStorage`, `IndexedDbRecord` |
| `session_swarm.rs` | `SwarmSessionOrchestrator` — multi-session coordination |

### Storage Architecture

```
Session
├── CookieStore (cookie management, SameSite policies)
├── SessionStorageDisk (disk-backed key-value store)
├── IndexedDbStorage (structured object store)
├── HistoryStack (navigation history)
├── StorageQuotaManager (enforce storage limits)
├── StorageEventBroadcaster (cross-tab events)
└── SwarmSessionOrchestrator (multi-session coordination)
```

### Parser Subsystem (`parser/`)

```
parser/
├── mod.rs              # Module root
├── html.rs             # Html5Tokenizer, HtmlParser
├── css.rs              # FastCssParser, CssMatcher
├── stream_jit.rs       # StreamJitTokenizer, StreamJitToken
├── bitmask.rs          # FastCssRuleBitmask
└── types.rs            # Parser types
```

- **Html5Tokenizer**: HTML5-compliant tokenization with malformed markup handling
- **FastCssParser**: CSS selector parsing with bitmask-accelerated matching
- **StreamJitTokenizer**: Streaming JIT token compilation for incremental parsing

### Style Subsystem (`style/`)

```
style/
├── mod.rs              # Module root
├── cascade.rs          # StyleCascader, Specificity
├── transitions.rs      # CSS transitions and animations
├── font_shaper.rs      # FontShaperEngine, GlyphMetric
└── scoped_css.rs       # ScopedCssMatcher
```

- **StyleCascader**: CSS cascade rules, specificity calculation, property inheritance
- **CssAnimation**: Keyframe animations with `FillMode`, `AnimationDirection`, `PlayState`, `TimingFunction`
- **FontShaperEngine**: Glyph metric computation for text layout
- **ScopedCssMatcher**: Scoped CSS matching with `Specificity` calculation
