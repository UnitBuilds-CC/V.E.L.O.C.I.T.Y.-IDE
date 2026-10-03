pub mod batch;
pub mod shmem;
pub mod telemetry;
pub mod telemetry_share;

/// HMAC key authenticating the GUI<->watcher telemetry shared-memory channel.
///
/// Both ends of the channel (the `TelemetryServer` spawned by
/// `velocity-ide-gui` and the `TelemetryClient` in `automation::watcher`) MUST
/// open the same `.velocity/telemetry_shmem.bin` with this one key. It was
/// previously hardcoded twice with divergent literals, so every AST write
/// failed HMAC, the watcher's circuit breaker tripped after three failures,
/// and the AST->SiteMap semantic index never received a triple in a real
/// session. Single-sourcing it here makes that class of drift impossible.
pub const IPC_TELEMETRY_HMAC_KEY: &[u8] = b"velocity_telemetry_v1";
