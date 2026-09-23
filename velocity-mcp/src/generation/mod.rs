//! Multimodal generation framework: async job-submission APIs for image,
//! video, and audio generation models accessible through the provider's
//! native (non-OpenAI-compatible) endpoints.
//!
//! Models are described by [`registry::GenerationModelSpec`] entries stored
//! in `.velocity/generation_models.json`. The [`client`] module implements
//! the submit → poll → retrieve lifecycle shared by all DashScope-style
//! generation APIs.

pub mod client;
pub mod registry;

pub use registry::{GenerationModelSpec, OutputType};
