pub mod audio;
pub mod canvas;
pub mod canvas_context;
pub mod captcha;
pub mod captcha_solver;
pub mod crypto;
pub mod files;
pub mod geolocation;
pub mod gpu_compositor;
pub mod interstitial;
pub mod network;
pub mod payment;
pub mod pdf_extractor;
pub mod profile;
pub mod push_notifications;
pub mod rasterizer;
pub mod sandbox;
pub mod service_worker;
pub mod shadow_dom;
pub mod stealth_human;
pub mod svg;
pub mod trace;
pub mod webcodecs;
pub mod webgl;
pub mod webgpu;

pub use audio::{AudioContextNode, WebAudioEngine};
pub use canvas::{CanvasElement, CanvasExtractor};
pub use canvas_context::{Canvas2DContext, DrawCommand};
pub use captcha::{
    CaptchaOrchestrator, ChallengeArchetype, ChallengeDescriptor, ChallengeObserver,
    ProviderFingerprinter, SolveResult, SolveTemplate, TemplateStore, VisualFingerprint,
    VisualFingerprinter,
};
pub use captcha_solver::{CaptchaSolverEngine, CaptchaType};
pub use crypto::WebCryptoEngine;
pub use files::{DownloadStreamArtifact, FileChooserEvent, FileManager};
pub use geolocation::{Geocoordinates, GeolocationProvider};
pub use gpu_compositor::{GpuLayer, GpuTileCompositor};
pub use interstitial::{InterstitialClassifier, InterstitialKind};
pub use network::{NetworkRequest, NetworkTracker};
pub use payment::{
    PaymentAddress, PaymentItem, PaymentMethodFilter, PaymentMethodType, PaymentRequestEngine,
    PaymentValidationErrors, ShippingOption,
};
pub use pdf_extractor::PdfMediaExtractor;
pub use profile::DeviceProfile;
pub use push_notifications::{
    NotificationRecord, PushEvent, PushNotificationManager, PushSubscription,
};
pub use rasterizer::{PixelBuffer, SoftwareRasterizer};
pub use sandbox::{SandboxCapabilities, SandboxViolation, TabSandbox, ViolationCategory};
pub use service_worker::{
    BackgroundSyncRegistration, CacheStorageEngine, CacheStrategy, CachedResponse,
    FetchInterceptResult, FetchInterceptRule, PushMessage, ServiceWorkerManager,
    ServiceWorkerState,
};
pub use shadow_dom::{FrameTarget, ShadowFrameExtractor, ShadowHost};
pub use stealth_human::{BezierPoint, StealthHumanBehavior};
pub use svg::{SvgPathBuilder, SvgPathCommand, SvgShape, SvgTransform, SvgVectorEngine};
pub use trace::{
    ConsoleTraceRecord, DomMutationTraceRecord, NetworkTraceRecord, PerformanceTraceRecord,
    TraceCollector,
};
pub use webcodecs::{
    AudioFrame, CodecKind, CodecStats, EncodedPacket, VelocityCodecsEngine,
    VelocityFrameRingBuffer, VelocityRemotePacketStreamer, VideoFrame,
};
pub use webgl::{
    Framebuffer, IndexBuffer, IndexType, Matrix4x4, ShaderProgram, ShaderUniform, Texture2D,
    TextureFilter, TextureFormat as WebGLTextureFormat, TextureWrap, Viewport, WebGLContext,
};
pub use webgpu::{
    BindGroup, BindGroupEntry, BindResource, CommandEncoder, GpuTexture, PrimitiveTopology,
    RenderPipeline, TextureFormat as GpuTextureFormat, TextureUsage, VertexFormat,
    WebGpuComputeBuffer, WebGpuComputeEngine,
};
