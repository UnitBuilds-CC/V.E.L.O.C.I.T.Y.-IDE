pub mod canvas;
pub mod interstitial;
pub mod shadow_dom;

pub use canvas::{CanvasElement, CanvasExtractor};
pub use interstitial::{InterstitialClassifier, InterstitialKind};
pub use shadow_dom::{FrameTarget, ShadowFrameExtractor, ShadowHost};
