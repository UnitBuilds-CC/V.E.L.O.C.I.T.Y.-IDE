// The GPU driver and shader blobs live in velocity-ide (single source of
// truth); this crate only keeps its MCP-specific compiler helpers here.
pub mod jit;
pub mod parser_loader;
pub mod tokenizer;
