// Structural patterns that are intentional in this codebase
#![allow(clippy::too_many_arguments)] // Complex engine functions need many params
#![allow(clippy::result_large_err)] // Error types carry diagnostic context
#![allow(clippy::needless_range_loop)] // Index-based loops are often clearer for parallel arrays
#![allow(clippy::while_let_loop)] // Explicit loop+match is sometimes more readable
#![allow(clippy::clone_on_copy)] // Explicit clone for clarity in numeric code
#![allow(clippy::redundant_clone)] // Sometimes clone is clearer than borrow gymnastics
#![allow(clippy::branches_sharing_code)] // Shared code in branches is sometimes intentional
#![allow(clippy::should_implement_trait)] // from_str methods don't always need FromStr trait
#![allow(clippy::vec_init_then_push)] // Sometimes push pattern is clearer
#![allow(clippy::manual_strip)] // Explicit strip for clarity
#![allow(clippy::doc_lazy_continuation)] // Doc formatting preference
#![allow(unused_assignments)] // Initialization pattern for clarity
#![allow(clippy::unnecessary_get_then_check)] // get().is_some() sometimes clearer
#![allow(clippy::same_item_push)] // Intentional repeated push
#![allow(clippy::manual_memcpy)] // Explicit copy for clarity
#![allow(clippy::cloned_ref_to_slice_refs)] // clone() sometimes clearer than from_ref
#![allow(clippy::collapsible_match)] // Nested match sometimes clearer
#![allow(clippy::explicit_counter_loop)] // Explicit counter for clarity

pub mod agent_api;
pub mod agentic;
pub mod aom;
pub mod dom;
pub mod engine;
pub mod js;
pub mod layout;
pub mod nda;
pub mod nda_portable;
pub mod net;
pub mod parser;
pub mod predicates;
pub mod screencast;
pub mod session;
pub mod session_auth;
pub mod session_cookie_store;
pub mod session_history;
pub mod session_indexeddb;
pub mod session_storage;
pub mod session_storage_events;
pub mod session_storage_quota;
pub mod session_swarm;
pub mod style;
pub mod vector_memory;

pub use agent_api::{diff, AgentActionResult, FactChange, NdaDelta};
pub use agentic::{
    ActionPredictorEngine, AgenticAomNode, AgenticAomTree, NdaEncoder, OcrTextBoundingBox,
    PredictedActionTarget, VelocityOcrEngine, ZeroAllocNdaWriter,
};
pub use dom::{
    CustomElementDefinition, CustomElementRegistry, DomTree, FormDataSerializer, MutationBatcher,
    MutationRecord, NativeMutationObserver, RawSlabNode, SlabDomTree, SlotProjection,
    SlotProjectionEngine, UnmanagedSlabArena, SLAB_NODE_DIRTY, SLAB_NODE_VISIBLE,
};
pub use engine::*;
pub use js::{
    JsEventListener, JsEventLoopScheduler, JsRtcPeerConnection, JsValue, JsVirtualMachine,
    PointerEvent, ScheduledTask, SyntheticEventDispatcher, TaskKind, WasmInterpreter,
    WasmSimdPipeline, WasmV128Vector, WasmValue, WebWorkerPool, WorkerMessage, WorkerThread,
};
pub use layout::{
    AlignItems, DisplayMode, FlexAlignmentSolver, FlexDirection, FlexLayoutEngine, GridTrack,
    GridTrackSolver, JustifyContent, LayoutBox, LayoutEngine2D, ParallelLayoutEngine,
};
pub use nda::{NdaDictionary, NdaDocument, NdaFact, NdaObject, NdaTriple};
pub use net::{
    BluetoothDevice, BundlePolicy, ConnectionState, DataChannel, DataChannelState, HttpClient,
    HttpResponse, IceCandidateState, IceConnectionState, IceServer, InspectorServer,
    MediaStreamTrack, NativeTlsStream, NativeWsClient, ProxyResolver, ProxyType, QuicConnection,
    QuicStream, RtcConfiguration, SdpType, SessionDescription, SignalingState,
    TlsFingerprintRotator, TlsJa3Profile, TlsState, TrackKind, TrackState, WebBluetoothTransport,
    WebRtcTransport, WsFrame,
};
pub use parser::{
    CssMatcher, FastCssParser, FastCssRuleBitmask, Html5Tokenizer, HtmlParser, StreamJitToken,
    StreamJitTokenizer,
};
pub use session::{BrowserSession, Cookie};
pub use session_auth::{AuthReseeder, AuthTokenState};
pub use session_cookie_store::{CookieRecord, CookieStore, SameSitePolicy};
pub use session_history::{HistoryItem, HistoryStack};
pub use session_indexeddb::{IndexedDbRecord, IndexedDbStorage};
pub use session_storage::SessionStorageDisk;
pub use session_storage_events::{StorageEventBroadcaster, StorageEventRecord};
pub use session_storage_quota::{StorageQuotaEstimate, StorageQuotaManager};
pub use session_swarm::SwarmSessionOrchestrator;
pub use style::{
    interpolate_value, parse_keyframes, AnimationDirection, AnimationInstance, AnimationManager,
    AnimationState, CssAnimation, CssRule, FillMode, FontShaperEngine, GlyphMetric, KeyframeStop,
    KeyframesRule, PlayState, ScopedCssMatcher, Specificity, StepPosition, StyleCascader,
    TimingFunction, TransitionInstance, TransitionManager, TransitionSpec, TransitionState,
};
