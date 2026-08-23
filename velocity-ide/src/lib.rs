// V.E.L.O.C.I.T.Y.-IDE — library facade
// Re-exports modules so that binaries in src/bin/ can use `velocity_ide::*`.

pub mod compiler;
pub mod errors;
pub mod model;
pub mod nda;
pub mod nda_int;
pub mod pipeline_bridge;
pub mod pipeline_nda;
pub mod safety;
pub mod sandbox;
pub mod site_map;
pub mod tokenizer;
pub mod velocity_client;
pub mod provider_usage;
pub mod wiki;
