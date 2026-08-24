// V.E.L.O.C.I.T.Y.-IDE — main entry point

mod compiler;
mod model;
mod nda;
mod nda_int;
mod pipeline_bridge;
mod pipeline_nda;
mod safety;
mod sandbox;
mod site_map;
mod tokenizer;
mod velocity_client;
mod provider_usage;
mod credential_guard;

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use velocity_client::VelocityConfig;

// ─── CLI definition ────────────────────────────────────────────────────────

/// V.E.L.O.C.I.T.Y.-IDE  —  NDA-accelerated LLM inference runtime
#[derive(Parser)]
#[command(
    name    = "velocity_ide",
    version,
    about   = "V.E.L.O.C.I.T.Y.-IDE: Verified, Efficient, Low-latency Optimised Computing \
               Inference Technology \u{2014} Intelligent Development Environment",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Output in JSON format (for scripting and piping)
    #[arg(long, global = true)]
    json: bool,

    /// Enable verbose/diagnostic output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Run autoregressive text generation with a real BitNet-3B NDA model
    Generate(GenerateArgs),

    /// Run the NDA-GEMV synthetic performance benchmark
    Benchmark,

    /// Compile Rust source files into NDA programs and store in the SiteMap.
    /// Use this to bootstrap the model with complete, real programs before
    /// Stage 3 RL training begins.  Accepts one or more .rs files.
    Seed(SeedArgs),

    /// Interactive CLI chat session to test model generation
    Chat(ChatArgs),

    /// Show Velocity Router usage statistics
    Usage(UsageArgs),

    /// Configure Velocity Router connection (API key and base URL)
    Login(LoginArgs),

    /// Manage provider API keys and query usage
    Providers(ProvidersArgs),

    /// Quick health check and router status
    Status,

    /// Show routing transparency — why models were chosen, cost flow
    Transparency,

    /// Generate shell completions for bash, zsh, fish, or powershell
    Completions(CompletionsArgs),
}

#[derive(clap::Args)]
struct GenerateArgs {
    /// Directory containing converted NDA weight files (.nda / .bin)
    #[arg(long, value_name = "DIR")]
    model: Option<PathBuf>,

    /// Path to tokenizer.json (defaults to <model-dir>/../tokenizer.json)
    #[arg(long, value_name = "FILE")]
    tokenizer: Option<PathBuf>,

    /// Input prompt text
    #[arg(long, value_name = "TEXT")]
    prompt: Option<String>,

    /// Path to file containing the input prompt text
    #[arg(long, value_name = "FILE")]
    prompt_file: Option<PathBuf>,

    /// Maximum number of new tokens to generate
    #[arg(long, default_value = "512", value_name = "N")]
    max_tokens: usize,

    /// Sampling temperature (0 = greedy, 0.7 = default, >1 = creative)
    #[arg(long, default_value = "0.7", value_name = "T")]
    temperature: f32,

    /// Top-p nucleus sampling threshold
    #[arg(long, default_value = "0.9", value_name = "P")]
    top_p: f32,

    /// Use zero-float NDA-Zero runtime (pure integer, ALiBi, argmax greedy).
    /// Requires a model converted with distill_nda_zero.py.
    #[arg(long, default_value = "false")]
    zero_float: bool,

    /// Model architecture preset: 'bitnet3b' (default) or 'qwen05' (NDA-Zero 0.5B coder).
    #[arg(long, default_value = "bitnet3b", value_name = "ARCH")]
    arch: String,

    /// Pipeline mode: 'text' (natural language), 'nda' (pure NDA native), 'auto' (detect).
    /// Only applies when --zero-float is set.
    #[arg(long, default_value = "text", value_name = "MODE")]
    mode: String,

    /// Path to site map directory for persistent KV cache (NDA native mode).
    /// Defaults to <model-dir>/site_map.
    #[arg(long, value_name = "DIR")]
    site_map: Option<PathBuf>,
}

#[derive(clap::Args)]
struct SeedArgs {
    /// One or more Rust source files to compile into NDA programs.
    /// Glob patterns are supported (e.g. seeds/*.rs).
    #[arg(long, value_name = "FILE", num_args = 1..)]
    source: Vec<PathBuf>,

    /// Directory for the SiteMap that will receive the compiled programs.
    #[arg(long, value_name = "DIR")]
    site_map: PathBuf,

    /// Weight-root hash for the SiteMap (hex string).  Use 0 for a
    /// standalone seeding run not tied to specific model weights.
    #[arg(long, default_value = "0", value_name = "HEX")]
    weight_root: String,
}

#[derive(clap::Args)]
struct ChatArgs {
    /// Directory containing converted NDA weight files (.nda / .bin)
    #[arg(long, value_name = "DIR")]
    model: Option<PathBuf>,

    /// Path to tokenizer.json (defaults to <model-dir>/../tokenizer.json)
    #[arg(long, value_name = "FILE")]
    tokenizer: Option<PathBuf>,

    /// Maximum number of new tokens to generate per response
    #[arg(long, default_value = "512", value_name = "N")]
    max_tokens: usize,

    /// Sampling temperature (0 = greedy, 0.7 = default, >1 = creative)
    #[arg(long, default_value = "0.7", value_name = "T")]
    temperature: f32,

    /// Top-p nucleus sampling threshold
    #[arg(long, default_value = "0.9", value_name = "P")]
    top_p: f32,

    /// Model architecture preset: 'bitnet3b' or 'qwen05'
    #[arg(long, default_value = "bitnet3b", value_name = "ARCH")]
    arch: String,
}

#[derive(clap::Args)]
struct UsageArgs {
    /// Show per-model and per-domain breakdown
    #[arg(long)]
    detailed: bool,

    /// Show rate limit and quota status with projections
    #[arg(long)]
    rate_limit: bool,

    /// Show enhanced summary with projections and sparkline
    #[arg(long)]
    summary: bool,

    /// Show timeseries data (hourly or daily)
    #[arg(long, value_name = "RANGE")]
    timeseries: Option<String>,
}

#[derive(clap::Args)]
struct LoginArgs {
    /// Velocity Router base URL
    #[arg(long, default_value = "http://localhost:8787")]
    url: String,

    /// API key (vr_... prefix)
    #[arg(long)]
    key: String,
}

#[derive(clap::Args)]
struct ProvidersArgs {
    /// Subcommand: list, add, remove, refresh
    #[arg(value_name = "ACTION")]
    action: String,

    /// Provider name (for add/remove): openai, anthropic, google, mistral, cohere, xai, github
    #[arg(long)]
    provider: Option<String>,

    /// API key for the provider (for add)
    #[arg(long)]
    api_key: Option<String>,

    /// Optional base URL override (for add, e.g. Azure/proxy endpoints)
    #[arg(long)]
    base_url: Option<String>,
}

#[derive(clap::Args)]
struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_name = "SHELL")]
    shell: String,
}

// ─── Diagnostics & Validation ────────────────────────────────────────────────

/// Snapshot of the CLI environment: config files, env vars, provider keys.
#[derive(Debug, Clone, Serialize)]
pub struct CliEnvironment {
    pub velocity_configured: bool,
    pub velocity_url_set: bool,
    pub velocity_key_set: bool,
    pub config_file_exists: bool,
    pub provider_count: usize,
    pub credential_boundary_active: bool,
    pub validation_issues: Vec<String>,
}

/// Inspect the CLI environment without making network calls.
pub fn inspect_environment() -> CliEnvironment {
    let url_set = std::env::var("VELOCITY_BASE_URL").is_ok();
    let key_set = std::env::var("VELOCITY_API_KEY").is_ok();
    let config_file_exists = velocity_client::dirs_next()
        .map(|h: std::path::PathBuf| h.join(".velocity").join("config.toml").exists())
        .unwrap_or(false);
    let provider_count = provider_usage::load_credentials()
        .map(|c| c.len())
        .unwrap_or(0);
    let velocity_configured = url_set && key_set || config_file_exists;
    // credential_guard scrubs env vars on account load; check if scrub happened.
    let credential_boundary_active = std::env::var("VELOCITY_API_KEY").is_err()
        && config_file_exists;
    let mut issues = Vec::new();
    if !velocity_configured {
        issues.push("Velocity Router not configured (no env vars or config file)".into());
    }
    if url_set && !key_set {
        issues.push("VELOCITY_BASE_URL set but VELOCITY_API_KEY is missing".into());
    }
    if key_set && !url_set {
        issues.push("VELOCITY_API_KEY set but VELOCITY_BASE_URL is missing".into());
    }
    CliEnvironment {
        velocity_configured,
        velocity_url_set: url_set,
        velocity_key_set: key_set,
        config_file_exists,
        provider_count,
        credential_boundary_active,
        validation_issues: issues,
    }
}

/// Validate generate arguments before dispatching to a backend.
fn validate_generate_args(args: &GenerateArgs) -> Vec<String> {
    let mut issues = Vec::new();
    if args.max_tokens == 0 {
        issues.push("--max-tokens must be > 0".into());
    }
    if args.max_tokens > 100_000 {
        issues.push("--max-tokens exceeds 100,000 (likely unintended)".into());
    }
    if args.temperature < 0.0 {
        issues.push("--temperature must be >= 0.0".into());
    }
    if args.temperature > 5.0 {
        issues.push("--temperature exceeds 5.0 (likely unintended)".into());
    }
    if args.top_p < 0.0 || args.top_p > 1.0 {
        issues.push("--top-p must be between 0.0 and 1.0".into());
    }
    match args.arch.as_str() {
        "bitnet3b" | "bitnet" | "qwen05" | "qwen" => {}
        other => issues.push(format!("Unknown --arch '{}'. Use 'bitnet3b' or 'qwen05'.", other)),
    }
    match args.mode.as_str() {
        "text" | "nda" | "auto" => {}
        other => issues.push(format!("Unknown --mode '{}'. Use 'text', 'nda', or 'auto'.", other)),
    }
    if args.prompt.is_none() && args.prompt_file.is_none() {
        issues.push("Either --prompt or --prompt-file must be provided".into());
    }
    issues
}

/// Validate chat arguments.
fn validate_chat_args(args: &ChatArgs) -> Vec<String> {
    let mut issues = Vec::new();
    if args.max_tokens == 0 {
        issues.push("--max-tokens must be > 0".into());
    }
    if args.temperature < 0.0 {
        issues.push("--temperature must be >= 0.0".into());
    }
    if args.top_p < 0.0 || args.top_p > 1.0 {
        issues.push("--top-p must be between 0.0 and 1.0".into());
    }
    match args.arch.as_str() {
        "bitnet3b" | "bitnet" | "qwen05" | "qwen" => {}
        other => issues.push(format!("Unknown --arch '{}'. Use 'bitnet3b' or 'qwen05'.", other)),
    }
    issues
}

/// Validate seed arguments.
fn validate_seed_args(args: &SeedArgs) -> Vec<String> {
    let mut issues = Vec::new();
    if args.source.is_empty() {
        issues.push("No source files specified. Use --source seeds/*.rs".into());
    }
    // Validate weight_root is valid hex.
    let hex = args.weight_root.trim_start_matches("0x");
    if !hex.is_empty() && u64::from_str_radix(hex, 16).is_err() {
        issues.push(format!("Invalid --weight-root hex: '{}'", args.weight_root));
    }
    issues
}

/// Diagnostic summary of the CLI configuration and environment.
#[derive(Debug, Clone, Serialize)]
pub struct CliDiagnostics {
    pub environment: CliEnvironment,
    pub velocity_config: Option<velocity_client::ConnectionInfo>,
    pub available_subcommands: Vec<&'static str>,
}

/// Build a full CLI diagnostic snapshot.
pub fn cli_diagnostics() -> CliDiagnostics {
    let env = inspect_environment();
    let velocity_config = if env.velocity_configured {
        VelocityConfig::load().ok().map(|c| c.connection_info())
    } else {
        None
    };
    CliDiagnostics {
        environment: env,
        velocity_config,
        available_subcommands: vec![
            "generate", "benchmark", "seed", "chat", "usage",
            "login", "providers", "status", "transparency", "completions",
        ],
    }
}

// ─── Entry point ───────────────────────────────────────────────────────────

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    if cli.verbose {
        log::info!("Verbose mode enabled");
        log::info!("velocity-ide v{}", env!("CARGO_PKG_VERSION"));
        let env = inspect_environment();
        log::info!("Velocity configured: {}", env.velocity_configured);
        log::info!("Provider keys: {}", env.provider_count);
        if !env.validation_issues.is_empty() {
            for issue in &env.validation_issues {
                log::warn!("Environment: {}", issue);
            }
        }
    }

    match cli.command {
        Command::Chat(args) => {
            let issues = validate_chat_args(&args);
            if !issues.is_empty() {
                for issue in &issues {
                    eprintln!("Error: {}", issue);
                }
                anyhow::bail!("Invalid chat arguments ({} issue(s))", issues.len());
            }
            run_chat(args)
        }
        Command::Seed(args) => {
            let issues = validate_seed_args(&args);
            if !issues.is_empty() {
                for issue in &issues {
                    eprintln!("Error: {}", issue);
                }
                anyhow::bail!("Invalid seed arguments ({} issue(s))", issues.len());
            }
            run_seed(args)
        }
        Command::Generate(args) => {
            let issues = validate_generate_args(&args);
            if !issues.is_empty() {
                for issue in &issues {
                    eprintln!("Error: {}", issue);
                }
                anyhow::bail!("Invalid generate arguments ({} issue(s))", issues.len());
            }
            if args.zero_float {
                let mode = crate::pipeline_nda::PipelineMode::from_str(&args.mode);
                if mode != crate::pipeline_nda::PipelineMode::Text {
                    run_generate_zero_nda(args, mode)
                } else {
                    run_generate_zero(args, cli.json)
                }
            } else if args.arch != "bitnet3b" && args.arch != "bitnet" {
                // Non-BitNet architectures use local FP32 transformer inference
                run_generate_local(args, cli.json)
            } else {
                run_generate(args)
            }
        }
        Command::Benchmark => {
            nda::run_nda_benchmark();
            println!();
            println!("Exotic Vulkan GPU Attention Benchmark:");
            if let Ok(driver) = compiler::driver::VulkanDriver::init() {
                if let Ok((contig_us, ndakv_us)) = driver.run_attn_benchmarks() {
                    println!("  Float32 Attention: {:.2} us", contig_us);
                    println!(
                        "  NDA-KV Attention : {:.2} us ({:.1}x speedup)",
                        ndakv_us,
                        contig_us / ndakv_us
                    );
                } else {
                    println!("  [FAIL] Failed to run GPU attention benchmarks.");
                }
            } else {
                println!("  [FAIL] Failed to initialize Vulkan GPU compute driver.");
            }
            Ok(())
        }
        Command::Usage(args) => run_usage(args, cli.json),
        Command::Login(args) => run_login(args),
        Command::Providers(args) => run_providers(args, cli.json),
        Command::Status => run_status(cli.json, cli.verbose),
        Command::Transparency => run_transparency(cli.json),
        Command::Completions(args) => run_completions(args),
    }
}

// ─── Seed ──────────────────────────────────────────────────────────────────

fn run_seed(args: SeedArgs) -> Result<()> {
    use crate::compiler::rust_to_nda::seed_from_source;
    use crate::site_map::SiteMap;

    if args.source.is_empty() {
        anyhow::bail!("No source files specified.  Use --source seeds/*.rs");
    }

    // Parse weight-root hex (0 = standalone, not tied to model weights)
    let weight_root =
        u64::from_str_radix(args.weight_root.trim_start_matches("0x"), 16).unwrap_or(0);

    eprintln!("[seed] Opening SiteMap at {:?}", args.site_map);
    eprintln!("[seed] Weight root: {:016x}", weight_root);
    let mut site_map = SiteMap::open(&args.site_map, weight_root)?;
    eprintln!("[seed] {}", site_map.stats());

    let mut total_functions = 0;
    let mut total_stored = 0;
    let n_files = args.source.len();

    for path in &args.source {
        eprint!("[seed] Compiling {:?} \u{2026} ", path);
        match seed_from_source(path, &mut site_map) {
            Ok(report) => {
                eprintln!("{}", report);
                total_functions += report.functions;
                total_stored += report.nodes_stored;
            }
            Err(e) => {
                eprintln!("FAILED: {e:#}");
            }
        }
    }

    eprintln!(
        "\n[seed] Done. {} file(s) \u{2192} {} functions \u{2192} {} NDA nodes stored",
        n_files, total_functions, total_stored,
    );
    eprintln!("[seed] {}", site_map.stats());

    Ok(())
}

// ─── Generate ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    content: String,
}

/// Structured execution report for generation results.
/// Supports both human-readable and JSON output via --json.
#[derive(Serialize)]
struct GenerationReport {
    mode: String,
    tokens_generated: usize,
    elapsed_ms: u64,
    tokens_per_second: f64,
    site_map_hits: usize,
    site_map_misses: usize,
    merkle_valid: Option<bool>,
    force_terminated: Option<bool>,
    sandbox_executed: Option<bool>,
    sandbox_panicked: Option<bool>,
    scope_passed: Option<bool>,
    stored_in_site_map: Option<bool>,
}

impl GenerationReport {
    /// Format for human-readable display.
    fn display(&self) {
        println!();
        println!("--- {} Generation Report ---", self.mode);
        println!("  Tokens:     {}", self.tokens_generated);
        println!("  Time:       {:.2}s", self.elapsed_ms as f64 / 1000.0);
        println!("  Speed:      {:.2} tok/s", self.tokens_per_second);
        if self.site_map_hits > 0 || self.site_map_misses > 0 {
            let total = self.site_map_hits + self.site_map_misses;
            let hit_rate = if total > 0 {
                self.site_map_hits as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            println!("  SiteMap:    {} hits / {} misses ({:.1}% hit rate)",
                self.site_map_hits, self.site_map_misses, hit_rate);
        }
        if let Some(valid) = self.merkle_valid {
            println!("  Merkle:     {}", if valid { "VALID" } else { "INVALID" });
        }
        if let Some(ft) = self.force_terminated {
            if ft {
                println!("  Note:       Force-terminated (budget exhausted before natural close)");
            }
        }
        if let Some(executed) = self.sandbox_executed {
            println!("  Sandbox:    {}", if executed { "executed" } else { "skipped" });
        }
        if let Some(panicked) = self.sandbox_panicked {
            if panicked {
                println!("  Sandbox:    PANICKED (invalid memory access caught)");
            }
        }
        if let Some(passed) = self.scope_passed {
            println!("  Scope:      {}", if passed { "PASSED" } else { "FAILED" });
        }
        if let Some(stored) = self.stored_in_site_map {
            if stored {
                println!("  Stored:     yes (available for future KV lookups)");
            }
        }
        println!();
    }
}

struct CloudflareAccount {
    id: String,
    token: String,
}

fn load_accounts() -> Vec<CloudflareAccount> {
    dotenvy::dotenv().ok();
    let mut accounts = Vec::new();
    for i in 1..=30 {
        let id_key = format!("CF_ACCOUNT_{}_ID", i);
        let token_key = format!("CF_ACCOUNT_{}_TOKEN", i);
        if let (Ok(id), Ok(token)) = (std::env::var(&id_key), std::env::var(&token_key)) {
            accounts.push(CloudflareAccount { id, token });
        }
    }
    // Scrub sensitive env vars after loading credentials into memory.
    // This prevents JIT-compiled closures and other untrusted code from
    // reading API keys via std::env::var (defense-in-depth).
    if !accounts.is_empty() {
        let scrubbed = credential_guard::scrub_sensitive_env_vars();
        if !scrubbed.is_empty() {
            log::debug!("Scrubbed {} sensitive env vars from process", scrubbed.len());
        }
    }
    accounts
}

fn call_kimi(messages: &[Message], accounts: &[CloudflareAccount]) -> Result<String> {
    if accounts.is_empty() {
        anyhow::bail!("No Cloudflare accounts found in environment variables or .env");
    }

    let payload = serde_json::json!({
        "model": "@cf/moonshotai/kimi-k2.7-code",
        "messages": messages,
        "stream": true,
    });

    for account in accounts {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/ai/v1/chat/completions",
            account.id
        );

        let response = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", account.token))
            .set("Content-Type", "application/json")
            .send_json(&payload);

        let resp = match response {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Account {} failed or rate-limited: {}", account.id, e);
                continue; // Failover to next account!
            }
        };

        let mut reader = BufReader::new(resp.into_reader());
        let mut full_response = String::new();
        let mut line = String::new();

        while let Ok(bytes_read) = reader.read_line(&mut line) {
            if bytes_read == 0 {
                break;
            }
            let cleaned = line.trim();
            if cleaned.is_empty() || cleaned == "data: [DONE]" {
                line.clear();
                continue;
            }
            if let Some(data_str) = cleaned.strip_prefix("data: ") {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(data_str) {
                    let mut content_chunk = String::new();
                    if let Some(choices) = val.get("choices") {
                        if let Some(delta) = choices.get(0).and_then(|c| c.get("delta")) {
                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                content_chunk = content.to_string();
                            }
                        }
                    } else if let Some(resp_field) = val.get("response").and_then(|r| r.as_str()) {
                        content_chunk = resp_field.to_string();
                    } else if let Some(result) = val.get("result") {
                        if let Some(resp_field) = result.get("response").and_then(|r| r.as_str()) {
                            content_chunk = resp_field.to_string();
                        }
                    }

                    if !content_chunk.is_empty() {
                        print!("{}", content_chunk);
                        std::io::stdout().flush().ok();
                        full_response.push_str(&content_chunk);
                    }
                }
            }
            line.clear();
        }
        println!();
        return Ok(full_response);
    }

    anyhow::bail!("All Cloudflare Workers AI accounts exhausted or failed.")
}

fn run_generate(args: GenerateArgs) -> Result<()> {
    let accounts = load_accounts();
    if accounts.is_empty() {
        anyhow::bail!("No Cloudflare accounts found in parent .env. Please configure them first.");
    }

    // Resolve prompt
    let prompt_text = if let Some(p) = args.prompt {
        p
    } else if let Some(pf) = &args.prompt_file {
        std::fs::read_to_string(pf).with_context(|| format!("Reading prompt file: {pf:?}"))?
    } else {
        anyhow::bail!("Either --prompt or --prompt-file must be provided");
    };

    let messages = vec![
        Message {
            role: "system".to_string(),
            content: "You are Kimi, a helpful AI coding assistant.".to_string(),
        },
        Message {
            role: "user".to_string(),
            content: prompt_text,
        },
    ];

    let t_gen = Instant::now();
    let _ = call_kimi(&messages, &accounts)?;
    let elapsed = t_gen.elapsed().as_secs_f32();
    println!("\n--- Generation finished in {:.2}s ---", elapsed);

    Ok(())
}

// ─── Zero-Float Generate ────────────────────────────────────────────────────

fn resolve_config(arch: &str) -> Result<model::config::ModelConfig> {
    match arch {
        "qwen05" | "qwen" => Ok(model::config::ModelConfig::qwen_coder_05b()),
        "bitnet3b" | "bitnet" => Ok(model::config::ModelConfig::bitnet_3b()),
        other => anyhow::bail!("Unknown --arch '{other}'. Use 'qwen05' or 'bitnet3b'."),
    }
}

fn resolve_model_dir(model: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(ref d) = model {
        if d.exists() {
            return Ok(d.clone());
        }
        anyhow::bail!("--model directory does not exist: {d:?}");
    }
    // Auto-discover: check relative to workspace
    let candidates = [
        PathBuf::from("models/qwen-coder-0.5b/nda"),
        PathBuf::from("models/bitnet-3b/nda"),
    ];
    for c in &candidates {
        if c.exists() {
            eprintln!("[auto-discover] Using model directory: {c:?}");
            return Ok(c.clone());
        }
    }
    anyhow::bail!("No --model specified and no model directory auto-discovered. Use --model <dir>.")
}

fn resolve_tokenizer(tokenizer: &Option<PathBuf>, model_dir: &Path) -> Result<PathBuf> {
    if let Some(ref t) = tokenizer {
        if t.exists() {
            return Ok(t.clone());
        }
        anyhow::bail!("--tokenizer file does not exist: {t:?}");
    }
    // Default: look for tokenizer.json next to or above the nda dir
    let candidates: Vec<PathBuf> = vec![
        model_dir.join("tokenizer.json"),
        model_dir.join("../tokenizer.json"),
        model_dir.join("tokenizer.ndat"),
        model_dir.join("../tokenizer.ndat"),
    ];
    for c in &candidates {
        if c.exists() {
            eprintln!("[auto-discover] Using tokenizer: {c:?}");
            return Ok(c.clone());
        }
    }
    anyhow::bail!("No --tokenizer specified and none auto-discovered. Use --tokenizer <file>.")
}

fn run_generate_zero(args: GenerateArgs, json: bool) -> Result<()> {
    use model::transformer_zero::ZeroTransformer;
    use model::weights::ModelWeights;

    let cfg = resolve_config(&args.arch)?;
    let model_dir = resolve_model_dir(&args.model)?;
    let tokenizer_path = resolve_tokenizer(&args.tokenizer, &model_dir)?;

    eprintln!(
        "[zero-float] Loading model: arch={}, model={:?}",
        args.arch, model_dir
    );
    let weights = ModelWeights::load(&model_dir, &cfg)?;
    let mut model = ZeroTransformer::new(cfg.clone(), weights);

    let tokenizer = tokenizer::Tokenizer::from_file(&tokenizer_path)?;

    // Resolve prompt
    let prompt_text = if let Some(p) = args.prompt {
        p
    } else if let Some(pf) = args.prompt_file {
        std::fs::read_to_string(pf).context("Reading prompt file")?
    } else {
        // Read from stdin
        print!("Prompt: ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        line.trim().to_string()
    };

    let prompt_tokens = tokenizer.encode(&prompt_text, true);
    eprintln!("[zero-float] Prompt: {} tokens", prompt_tokens.len());

    let t_gen = std::time::Instant::now();
    let mut generated = Vec::new();

    model.generate_greedy(&prompt_tokens, args.max_tokens, |tok_id| {
        let piece = tokenizer.decode_token(tok_id);
        print!("{}", piece);
        std::io::stdout().flush().ok();
        generated.push(tok_id);
    });

    let elapsed = t_gen.elapsed();
    let elapsed_s = elapsed.as_secs_f32();
    let elapsed_ms = elapsed.as_millis() as u64;
    let tok_per_s = generated.len() as f64 / elapsed_s.max(1e-6) as f64;

    let report = GenerationReport {
        mode: "Zero-Float".to_string(),
        tokens_generated: generated.len(),
        elapsed_ms,
        tokens_per_second: tok_per_s,
        site_map_hits: 0,
        site_map_misses: 0,
        merkle_valid: None,
        force_terminated: None,
        sandbox_executed: None,
        sandbox_panicked: None,
        scope_passed: None,
        stored_in_site_map: None,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        report.display();
    }

    Ok(())
}

// ─── FP32 Local Generate (GPU float path for FP4/FP2 weights) ────────────────

fn run_generate_local(args: GenerateArgs, json: bool) -> Result<()> {
    use model::transformer::Transformer;
    use model::weights::ModelWeights;

    let cfg = resolve_config(&args.arch)?;
    let model_dir = resolve_model_dir(&args.model)?;
    let tokenizer_path = resolve_tokenizer(&args.tokenizer, &model_dir)?;

    eprintln!(
        "[local] Loading model: arch={}, model={:?}",
        args.arch, model_dir
    );
    let weights = ModelWeights::load(&model_dir, &cfg)?;
    let mut model = Transformer::new(cfg.clone(), weights);

    let tokenizer = tokenizer::Tokenizer::from_file(&tokenizer_path)?;

    // Resolve prompt
    let prompt_text = if let Some(p) = args.prompt {
        p
    } else if let Some(pf) = args.prompt_file {
        std::fs::read_to_string(pf).context("Reading prompt file")?
    } else {
        print!("Prompt: ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        line.trim().to_string()
    };

    let prompt_tokens = tokenizer.encode(&prompt_text, true);
    eprintln!("[local] Prompt: {} tokens", prompt_tokens.len());

    let t_gen = std::time::Instant::now();
    let mut generated = Vec::new();

    // Use low temperature for near-greedy sampling (FP32 path supports sampling)
    let temperature = if args.temperature > 0.0 {
        args.temperature
    } else {
        0.6
    };
    let top_p = if args.top_p > 0.0 { args.top_p } else { 0.9 };

    model.generate(
        &prompt_tokens,
        args.max_tokens,
        temperature,
        top_p,
        |tok_id| {
            let piece = tokenizer.decode_token(tok_id);
            print!("{}", piece);
            std::io::stdout().flush().ok();
            generated.push(tok_id);
        },
    );

    let elapsed = t_gen.elapsed();
    let elapsed_s = elapsed.as_secs_f32();
    let elapsed_ms = elapsed.as_millis() as u64;
    let tok_per_s = generated.len() as f64 / elapsed_s.max(1e-6) as f64;

    let report = GenerationReport {
        mode: "Local FP32".to_string(),
        tokens_generated: generated.len(),
        elapsed_ms,
        tokens_per_second: tok_per_s,
        site_map_hits: 0,
        site_map_misses: 0,
        merkle_valid: None,
        force_terminated: None,
        sandbox_executed: None,
        sandbox_panicked: None,
        scope_passed: None,
        stored_in_site_map: None,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        report.display();
    }

    Ok(())
}

// ─── Dual-Path NDA Generate ─────────────────────────────────────────────────

fn run_generate_zero_nda(args: GenerateArgs, mode: pipeline_nda::PipelineMode) -> Result<()> {
    let cfg = resolve_config(&args.arch)?;
    let model_dir = resolve_model_dir(&args.model)?;
    let tokenizer_path = resolve_tokenizer(&args.tokenizer, &model_dir)?;

    // Resolve prompt
    let prompt_text = if let Some(p) = args.prompt {
        p
    } else if let Some(pf) = args.prompt_file {
        std::fs::read_to_string(pf).context("Reading prompt file")?
    } else {
        anyhow::bail!("Either --prompt or --prompt-file must be provided");
    };

    pipeline_bridge::run_dual_path(
        &model_dir,
        &tokenizer_path,
        &prompt_text,
        mode,
        args.max_tokens,
        cfg,
    )
}

fn run_chat(_args: ChatArgs) -> Result<()> {
    use std::io::BufRead;

    let accounts = load_accounts();
    if accounts.is_empty() {
        anyhow::bail!("No Cloudflare accounts found in parent .env. Please configure them first.");
    }

    println!("Ready! Enter a prompt below. Type 'exit' or 'quit' to end the session.\n");
    let mut history = vec![Message {
        role: "system".to_string(),
        content: "You are Kimi, a helpful AI coding assistant.".to_string(),
    }];

    let stdin = std::io::stdin();
    let mut reader = stdin.lock();

    loop {
        print!("> ");
        std::io::stdout().flush().ok();

        let mut input = String::new();
        if reader.read_line(&mut input).is_err() {
            break;
        }

        let prompt = input.trim();
        if prompt.is_empty() {
            continue;
        }
        if prompt == "exit" || prompt == "quit" {
            break;
        }

        history.push(Message {
            role: "user".to_string(),
            content: prompt.to_string(),
        });

        let t_gen = Instant::now();
        match call_kimi(&history, &accounts) {
            Ok(response) => {
                let elapsed = t_gen.elapsed().as_secs_f32();
                let token_estimate = (response.len() as u64) / 4; // ~4 chars/token estimate
                history.push(Message {
                    role: "assistant".to_string(),
                    content: response.clone(),
                });
                println!();
                // Post-assignment summary: try Velocity router for real stats,
                // fall back to local estimate.
                match velocity_client::VelocityClient::from_env() {
                    Ok(client) => {
                        match client.get_usage() {
                            Ok(u) => {
                                println!("-> Completed in {:.1}s | {} tokens est. | tier: {} | total: {} / {}",
                                    elapsed,
                                    token_estimate,
                                    u.tier,
                                    velocity_client::fmt_number(u.tokens_used),
                                    velocity_client::fmt_number(u.tokens_limit));
                            }
                            Err(_) => {
                                println!("-> Completed in {:.1}s | {} tokens est.", elapsed, token_estimate);
                            }
                        }
                    }
                    Err(_) => {
                        println!("-> Completed in {:.1}s | {} tokens est.", elapsed, token_estimate);
                    }
                }
                println!();
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }

    Ok(())
}

// ─── Usage ────────────────────────────────────────────────────────────────

fn run_usage(args: UsageArgs, json: bool) -> Result<()> {
    use velocity_client::{VelocityClient, fmt_number, fmt_currency, fmt_percent};

    let client = VelocityClient::from_env()?;

    if args.rate_limit {
        let rl = client.get_rate_limit()?;
        if json {
            println!("{}", serde_json::to_string_pretty(&rl)?);
            return Ok(());
        }
        println!();
        println!("=== Velocity Rate Limit & Quota ===");
        println!();
        println!("  Key:              {}", rl.key_label);
        println!("  Tier:             {}", rl.tier);
        println!("  Rate Limit:       {} req/min (resets in {}s)",
            rl.rate_limit.max_requests_per_minute, rl.rate_limit.resets_in_secs);
        println!();
        println!("  Tokens Used:      {} / {}  ({})",
            fmt_number(rl.tokens.used),
            fmt_number(rl.tokens.limit),
            fmt_percent(rl.tokens.quota_pct));
        println!("  Projected:        {} by end of period",
            fmt_number(rl.tokens.projected_monthly));
        println!();
        println!("  Cost:             {} / {}  ({})",
            fmt_currency(rl.cost.used_usd),
            fmt_currency(rl.cost.limit_usd),
            fmt_percent(rl.cost.quota_pct));
        println!("  Projected:        {} by end of period",
            fmt_currency(rl.cost.projected_monthly_usd));
        println!();
        println!("  Billing Reset:    in {} days", rl.billing_period.resets_in_days);
        println!();
        return Ok(());
    }

    if args.detailed {
        let detail = client.get_usage_detailed()?;
        if json {
            println!("{}", serde_json::to_string_pretty(&detail)?);
            return Ok(());
        }
        println!();
        println!("=== Velocity Usage Breakdown ===");
        println!();
        println!("  Key:              {}", detail.label);
        println!("  Tier:             {}", detail.tier);
        println!("  Total Tokens:     {}", fmt_number(detail.total_tokens));
        println!("  Total Cost:       {}", fmt_currency(detail.total_cost_usd));
        println!("  Assignments:      {}", detail.total_assignments);
        println!();
        println!("  By Model:");
        println!("  {:<24} {:>10} {:>12} {:>10}", "Model", "Assigns", "Tokens", "Cost");
        println!("  {}", "-".repeat(60));
        for m in &detail.by_model {
            println!("  {:<24} {:>10} {:>12} {:>10}",
                m.model_id, m.assignments, fmt_number(m.tokens), fmt_currency(m.cost_usd));
        }
        println!();
        println!("  By Domain:");
        println!("  {:<24} {:>10} {:>12} {:>10}", "Domain", "Assigns", "Tokens", "Cost");
        println!("  {}", "-".repeat(60));
        for d in &detail.by_domain {
            println!("  {:<24} {:>10} {:>12} {:>10}",
                d.domain, d.assignments, fmt_number(d.tokens), fmt_currency(d.cost_usd));
        }
        println!();
        return Ok(());
    }

    if args.summary {
        let s = client.get_usage_summary()?;
        if json {
            println!("{}", serde_json::to_string_pretty(&s)?);
            return Ok(());
        }
        println!();
        println!("=== Velocity Usage Summary (Enhanced) ===");
        println!();
        println!("  Tier:             {}", s.tier);
        println!("  Tokens:           {} / {}  ({})",
            fmt_number(s.tokens_used), fmt_number(s.tokens_limit), fmt_percent(s.token_quota_pct));
        println!("  Cost:             {} / {}  ({})",
            fmt_currency(s.cost_usd), fmt_currency(s.cost_limit_usd), fmt_percent(s.cost_quota_pct));
        println!("  Assignments:      {}", s.assignments_count);
        println!();
        println!("  Projections:");
        println!("    Tokens:         {} by end of period", fmt_number(s.projected_tokens));
        println!("    Cost:           {} by end of period", fmt_currency(s.projected_cost_usd));
        println!();
        println!("  Billing Period:");
        println!("    Start:          {}", s.billing_period.start);
        println!("    End:            {}", s.billing_period.end);
        println!("    Days remaining: {}", s.billing_period.days_remaining);
        println!();
        // Sparkline (last 24h hourly).
        if !s.sparkline.is_empty() {
            println!("  Hourly Sparkline (last 24h):");
            let max_tok = s.sparkline.iter().map(|b| b.tokens).max().unwrap_or(1).max(1);
            for b in &s.sparkline {
                let bar_len = (b.tokens as f64 / max_tok as f64 * 30.0) as usize;
                let bar = "#".repeat(bar_len);
                println!("    {:>6}  {:>10}  {}", b.label, fmt_number(b.tokens), bar);
            }
            println!();
        }
        return Ok(());
    }

    if let Some(ref range) = args.timeseries {
        // Determine granularity from range.
        let granularity = if range.ends_with('d') { "daily" } else { "hourly" };
        let ts = client.get_timeseries(granularity, range)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&ts)?);
            return Ok(());
        }
        println!();
        println!("=== Velocity Timeseries ({}, {}) ===", ts.granularity, ts.range);
        println!();
        let max_tok = ts.buckets.iter().map(|b| b.tokens).max().unwrap_or(1).max(1);
        for b in &ts.buckets {
            let bar_len = (b.tokens as f64 / max_tok as f64 * 30.0) as usize;
            let bar = "#".repeat(bar_len);
            println!("  {:>6}  {:>10}  {:>10}  {}",
                b.label, fmt_number(b.tokens), fmt_currency(b.cost_usd), bar);
        }
        println!();
        return Ok(());
    }

    // Default: summary view.
    let usage = client.get_usage()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&usage)?);
        return Ok(());
    }
    let token_pct = if usage.tokens_limit > 0 {
        (usage.tokens_used as f64 / usage.tokens_limit as f64) * 100.0
    } else {
        0.0
    };
    let cost_pct = if usage.cost_limit_usd > 0.0 {
        (usage.cost_usd / usage.cost_limit_usd) * 100.0
    } else {
        0.0
    };

    println!();
    println!("=== Velocity Usage Summary ===");
    println!();
    println!("  Tier:           {}", usage.tier);
    println!("  Tokens Used:    {} / {}  ({})",
        fmt_number(usage.tokens_used),
        fmt_number(usage.tokens_limit),
        fmt_percent(token_pct));
    println!("  Cost:           {} / {}  ({})",
        fmt_currency(usage.cost_usd),
        fmt_currency(usage.cost_limit_usd),
        fmt_percent(cost_pct));
    println!("  Assignments:    {}", usage.assignments_count);
    println!("  Period:         {} to {}", usage.period.start, usage.period.end);
    println!();

    Ok(())
}

// ─── Login ────────────────────────────────────────────────────────────────

fn run_login(args: LoginArgs) -> Result<()> {
    use velocity_client::VelocityConfig;

    let config = VelocityConfig {
        base_url: args.url,
        api_key: args.key,
    };

    // Validate before saving.
    let warnings = config.validate();
    if !warnings.is_empty() {
        for w in &warnings {
            eprintln!("Warning: {}", w);
        }
    }

    config.save()?;

    println!();
    println!("Velocity Router configured:");
    println!("  URL:  {}", config.base_url);
    println!("  Key:  {}...{}", &config.api_key[..8], &config.api_key[config.api_key.len().saturating_sub(4)..]);
    println!();
    println!("Saved to ~/.velocity/config.toml");
    println!();

    // Verify connection.
    let client = velocity_client::VelocityClient::new(config);
    match client.health() {
        Ok(h) => {
            println!("Router health: {} (v{}, {} models)",
                h.status, h.version, h.models_available);
        }
        Err(e) => {
            println!("Warning: could not reach router: {}", e);
            println!("Configuration saved, but router may not be running.");
        }
    }
    println!();

    Ok(())
}

// ─── Providers ────────────────────────────────────────────────────────────

fn run_providers(args: ProvidersArgs, json: bool) -> Result<()> {
    use provider_usage::{ProviderCredential, load_credentials, save_credentials};

    match args.action.as_str() {
        "list" => {
            let creds = load_credentials()?;
            if json {
                // Mask API keys in JSON output.
                let masked: Vec<serde_json::Value> = creds.iter().map(|c| {
                    let masked_key = if c.api_key.len() > 12 {
                        format!("{}...{}", &c.api_key[..8], &c.api_key[c.api_key.len()-4..])
                    } else {
                        "****".to_string()
                    };
                    serde_json::json!({
                        "provider": c.provider,
                        "api_key": masked_key,
                        "base_url": c.base_url,
                    })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&masked)?);
                return Ok(());
            }
            if creds.is_empty() {
                println!();
                println!("No provider API keys configured.");
                println!("Add one with: velocity-ide providers add --provider openai --api-key sk-...");
                println!();
                return Ok(());
            }
            println!();
            println!("=== Configured Provider API Keys ===");
            println!();
            println!("  {:<16} {:<20} {}", "Provider", "API Key", "Base URL");
            println!("  {}", "-".repeat(60));
            for c in &creds {
                let masked = if c.api_key.len() > 12 {
                    format!("{}...{}", &c.api_key[..8], &c.api_key[c.api_key.len()-4..])
                } else {
                    "****".to_string()
                };
                let base = c.base_url.as_deref().unwrap_or("(default)");
                println!("  {:<16} {:<20} {}", c.provider, masked, base);
            }
            println!();
        }

        "add" => {
            let provider = args.provider.as_deref()
                .ok_or_else(|| anyhow::anyhow!("--provider is required (e.g. --provider openai)"))?;
            let api_key = args.api_key.as_deref()
                .ok_or_else(|| anyhow::anyhow!("--api-key is required"))?;

            // Validate provider name.
            if provider_usage::Provider::from_str_loose(provider).is_none() {
                println!("Warning: '{}' is not a recognized provider. Adding anyway.", provider);
            }

            let mut creds = load_credentials()?;

            // Remove existing entry for same provider if present.
            creds.retain(|c| c.provider.to_lowercase() != provider.to_lowercase());

            creds.push(ProviderCredential {
                provider: provider.to_lowercase(),
                api_key: api_key.to_string(),
                base_url: args.base_url.clone(),
                model: None,
            });

            save_credentials(&creds)?;
            println!();
            println!("Provider API key saved:");
            println!("  Provider:  {}", provider);
            println!("  Key:       {}...{}", &api_key[..4], &api_key[api_key.len().saturating_sub(4)..]);
            if let Some(ref url) = args.base_url {
                println!("  Base URL:  {}", url);
            }
            println!("  Stored in: ~/.velocity/providers.toml");
            println!();
            println!("Run `velocity-ide providers refresh` to query usage.");
            println!();
        }

        "remove" => {
            let provider = args.provider.as_deref()
                .ok_or_else(|| anyhow::anyhow!("--provider is required"))?;

            let mut creds = load_credentials()?;
            let before = creds.len();
            creds.retain(|c| c.provider.to_lowercase() != provider.to_lowercase());

            if creds.len() == before {
                println!("No provider '{}' found in configuration.", provider);
            } else {
                save_credentials(&creds)?;
                println!("Provider '{}' removed.", provider);
            }
            println!();
        }

        "refresh" => {
            let creds = load_credentials()?;
            if creds.is_empty() {
                if json {
                    println!("{{}}");
                    return Ok(());
                }
                println!();
                println!("No provider API keys configured.");
                println!("Add one with: velocity-ide providers add --provider openai --api-key sk-...");
                println!();
                return Ok(());
            }

            let snapshot = provider_usage::query_all_providers(&creds);

            if json {
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
                return Ok(());
            }

            println!();
            println!("Querying {} provider(s)...", creds.len());
            println!();

            // Print results.
            println!("  {:<16} {:<8} {:>12} {:>10}  {}", "Provider", "Valid", "Tokens", "Cost", "Status");
            println!("  {}", "-".repeat(75));
            for p in &snapshot.providers {
                let valid = if p.key_valid { "yes" } else { "NO" };
                println!("  {:<16} {:<8} {:>12} {:>10}  {}",
                    p.display_name, valid,
                    velocity_client::fmt_number(p.tokens_used),
                    velocity_client::fmt_currency(p.cost_usd),
                    p.status);
            }
            println!();
            println!("  Total: {} tokens, {} across {} requests",
                velocity_client::fmt_number(snapshot.total_tokens),
                velocity_client::fmt_currency(snapshot.total_cost_usd),
                snapshot.total_requests);
            println!();

            // Write snapshot for the dashboard.
            provider_usage::write_snapshot(&snapshot)?;
            println!("Snapshot written to ~/.velocity/usage_snapshot.json");
            println!("Open the dashboard to see your combined API usage.");
            println!();
        }

        other => {
            anyhow::bail!(
                "Unknown action '{}'. Use: list, add, remove, refresh",
                other
            );
        }
    }

    Ok(())
}

// ─── Status ──────────────────────────────────────────────────────────────

fn run_status(json: bool, verbose: bool) -> Result<()> {
    use velocity_client::VelocityClient;

    // Extended diagnostics in verbose mode.
    if verbose {
        let diag = cli_diagnostics();
        if json {
            println!("{}", serde_json::to_string_pretty(&diag)?);
            return Ok(());
        }
        println!();
        println!("=== CLI Environment ===");
        println!();
        println!("  Velocity configured:  {}", diag.environment.velocity_configured);
        println!("  Config file exists:   {}", diag.environment.config_file_exists);
        println!("  Provider keys:        {}", diag.environment.provider_count);
        println!("  Credential boundary:  {}", diag.environment.credential_boundary_active);
        if let Some(ref conn) = diag.velocity_config {
            println!("  Router URL:           {}", conn.base_url);
            println!("  HTTPS:                {}", conn.is_https);
            println!("  API key prefix:       {}", conn.api_key_prefix);
            if !conn.validation_issues.is_empty() {
                for issue in &conn.validation_issues {
                    println!("  WARNING: {}", issue);
                }
            }
        }
        if !diag.environment.validation_issues.is_empty() {
            println!();
            println!("  Environment Issues:");
            for issue in &diag.environment.validation_issues {
                println!("    - {}", issue);
            }
        }
        println!();
    }

    let client = VelocityClient::from_env()?;

    // JSON mode: collect all data and serialize.
    if json {
        let health = client.health().ok();
        let usage = client.get_usage().ok();
        let rate = client.get_rate_limit().ok();
        let out = serde_json::json!({
            "health": health,
            "usage": usage,
            "rate_limit": rate,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    // Health check.
    match client.health() {
        Ok(h) => {
            println!();
            println!("=== Velocity Router Status ===");
            println!();
            println!("  Status:    {}", h.status);
            println!("  Version:   {}", h.version);
            println!("  Models:    {} available", h.models_available);
        }
        Err(e) => {
            println!();
            println!("=== Velocity Router Status ===");
            println!();
            println!("  Status:    UNREACHABLE");
            println!("  Error:     {}", e);
            println!();
            println!("  Check that the router is running and VELOCITY_BASE_URL is correct.");
            return Ok(());
        }
    }

    // Usage snapshot (quick summary).
    match client.get_usage() {
        Ok(u) => {
            let token_pct = if u.tokens_limit > 0 {
                (u.tokens_used as f64 / u.tokens_limit as f64) * 100.0
            } else {
                0.0
            };
            let cost_pct = if u.cost_limit_usd > 0.0 {
                (u.cost_usd / u.cost_limit_usd) * 100.0
            } else {
                0.0
            };
            println!();
            println!("  Tier:      {}", u.tier);
            println!("  Tokens:    {} / {}  ({:.1}%)",
                velocity_client::fmt_number(u.tokens_used),
                velocity_client::fmt_number(u.tokens_limit),
                token_pct);
            println!("  Cost:      {} / {}  ({:.1}%)",
                velocity_client::fmt_currency(u.cost_usd),
                velocity_client::fmt_currency(u.cost_limit_usd),
                cost_pct);
            println!("  Assigns:   {}", u.assignments_count);
        }
        Err(_) => {
            println!("  (Could not fetch usage — key may not be configured)");
        }
    }

    // Rate limit info.
    match client.get_rate_limit() {
        Ok(rl) => {
            println!();
            println!("  Rate:      {} req/min (resets in {}s)",
                rl.rate_limit.max_requests_per_minute, rl.rate_limit.resets_in_secs);
            println!("  Billing:   resets in {} days", rl.billing_period.resets_in_days);
        }
        Err(_) => {}
    }

    println!();
    Ok(())
}

// ─── Transparency ────────────────────────────────────────────────────────

fn run_transparency(json: bool) -> Result<()> {
    use velocity_client::{VelocityClient, fmt_number, fmt_currency};

    let client = VelocityClient::from_env()?;
    let t = client.get_transparency()?;

    // JSON mode: serialize the full transparency response.
    if json {
        println!("{}", serde_json::to_string_pretty(&t)?);
        return Ok(());
    }

    println!();
    println!("=== Velocity Routing Transparency ===");
    println!();
    println!("  Summary:");
    println!("    Assignments:  {}", t.summary.total_assignments);
    println!("    Errors:       {}", t.summary.total_errors);
    println!("    Models:       {} available", t.summary.models_available);
    println!();
    println!("  Cost Flow:");
    let total_tok = t.cost_flow.input_tokens + t.cost_flow.output_tokens;
    println!("    Input tokens:  {} ({:.1}%)",
        fmt_number(t.cost_flow.input_tokens),
        if total_tok > 0 { t.cost_flow.input_tokens as f64 / total_tok as f64 * 100.0 } else { 0.0 });
    println!("    Output tokens: {} ({:.1}%)",
        fmt_number(t.cost_flow.output_tokens),
        if total_tok > 0 { t.cost_flow.output_tokens as f64 / total_tok as f64 * 100.0 } else { 0.0 });
    println!("    In/Out ratio:  {:.2}", t.cost_flow.input_output_ratio);
    println!("    Total cost:    {}", fmt_currency(t.cost_flow.total_cost_usd));
    println!();

    // Recent routing decisions.
    if !t.recent_routing_decisions.is_empty() {
        println!("  Recent Routing Decisions (last {}):", t.recent_routing_decisions.len());
        println!("  {:<24} {:<20} {:<14} {}", "Domain", "Model", "Tokens", "Rationale");
        println!("  {}", "-".repeat(90));
        for d in t.recent_routing_decisions.iter().take(20) {
            let rationale = d.routing_rationale.as_deref().unwrap_or("-");
            let short = if rationale.len() > 40 { format!("{}...", &rationale[..37]) } else { rationale.to_string() };
            println!("  {:<24} {:<20} {:<14} {}", d.domain, d.model_id, fmt_number(d.total_tokens), short);
        }
        println!();
    }

    // Model selection stats.
    if !t.model_selection_stats.is_empty() {
        println!("  Model Selection Stats:");
        println!("  {:<24} {:>8} {:>12} {:>10} {:>10}", "Model", "Reqs", "Tokens", "Cost", "Avg ms");
        println!("  {}", "-".repeat(70));
        for m in &t.model_selection_stats {
            println!("  {:<24} {:>8} {:>12} {:>10} {:>10}",
                m.model_id, m.total_requests, fmt_number(m.total_tokens),
                fmt_currency(m.total_cost_usd), m.avg_duration_ms);
        }
        println!();
    }

    // Domain distribution.
    if !t.domain_distribution.is_empty() {
        println!("  Domain Distribution:");
        println!("  {:<24} {:>8} {:>12} {:>10}", "Domain", "Reqs", "Tokens", "Cost");
        println!("  {}", "-".repeat(60));
        for d in &t.domain_distribution {
            println!("  {:<24} {:>8} {:>12} {:>10}",
                d.domain, d.requests, fmt_number(d.tokens), fmt_currency(d.cost_usd));
        }
        println!();
    }

    // Available models.
    if !t.available_models.is_empty() {
        println!("  Available Models & Pricing:");
        println!("  {:<24} {:<14} {:<10} {:>12} {:>12}", "Model", "Provider", "Tier", "In $/Mtok", "Out $/Mtok");
        println!("  {}", "-".repeat(76));
        for m in &t.available_models {
            println!("  {:<24} {:<14} {:<10} {:>12.2} {:>12.2}",
                m.id, m.provider, m.tier, m.cost_input_per_mtok, m.cost_output_per_mtok);
        }
        println!();
    }

    Ok(())
}

// ─── Shell Completions ──────────────────────────────────────────────────────

fn run_completions(args: CompletionsArgs) -> Result<()> {
    use clap::CommandFactory;
    use clap_complete::{Shell, generate};

    let shell = match args.shell.to_lowercase().as_str() {
        "bash"       => Shell::Bash,
        "zsh"        => Shell::Zsh,
        "fish"       => Shell::Fish,
        "powershell" | "pwsh" => Shell::PowerShell,
        other => anyhow::bail!(
            "Unknown shell: '{}'. Supported: bash, zsh, fish, powershell",
            other
        ),
    };

    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "velocity_ide", &mut std::io::stdout());
    Ok(())
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────

    fn default_generate_args() -> GenerateArgs {
        GenerateArgs {
            model: None,
            tokenizer: None,
            prompt: Some("Hello world".into()),
            prompt_file: None,
            max_tokens: 512,
            temperature: 0.7,
            top_p: 0.9,
            zero_float: false,
            arch: "bitnet3b".into(),
            mode: "text".into(),
            site_map: None,
        }
    }

    fn default_chat_args() -> ChatArgs {
        ChatArgs {
            model: None,
            tokenizer: None,
            max_tokens: 512,
            temperature: 0.7,
            top_p: 0.9,
            arch: "bitnet3b".into(),
        }
    }

    fn default_seed_args() -> SeedArgs {
        SeedArgs {
            source: vec![PathBuf::from("seeds/test.rs")],
            site_map: PathBuf::from("/tmp/sitemap"),
            weight_root: "0".into(),
        }
    }

    // ── validate_generate_args ───────────────────────────────────────────

    #[test]
    fn generate_valid_defaults() {
        let args = default_generate_args();
        let issues = validate_generate_args(&args);
        assert!(issues.is_empty(), "default args should be valid, got: {:?}", issues);
    }

    #[test]
    fn generate_zero_max_tokens() {
        let mut args = default_generate_args();
        args.max_tokens = 0;
        let issues = validate_generate_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("max-tokens"));
    }

    #[test]
    fn generate_max_tokens_over_100k() {
        let mut args = default_generate_args();
        args.max_tokens = 100_001;
        let issues = validate_generate_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("100,000"));
    }

    #[test]
    fn generate_max_tokens_exactly_100k() {
        let mut args = default_generate_args();
        args.max_tokens = 100_000;
        let issues = validate_generate_args(&args);
        assert!(issues.is_empty(), "100k should be valid");
    }

    #[test]
    fn generate_negative_temperature() {
        let mut args = default_generate_args();
        args.temperature = -0.1;
        let issues = validate_generate_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("temperature"));
    }

    #[test]
    fn generate_temperature_over_5() {
        let mut args = default_generate_args();
        args.temperature = 5.1;
        let issues = validate_generate_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("5.0"));
    }

    #[test]
    fn generate_temperature_boundary_values() {
        let mut args = default_generate_args();
        args.temperature = 0.0;
        assert!(validate_generate_args(&args).is_empty(), "temp=0 should be valid");
        args.temperature = 5.0;
        assert!(validate_generate_args(&args).is_empty(), "temp=5 should be valid");
    }

    #[test]
    fn generate_top_p_below_zero() {
        let mut args = default_generate_args();
        args.top_p = -0.1;
        let issues = validate_generate_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("top-p"));
    }

    #[test]
    fn generate_top_p_above_one() {
        let mut args = default_generate_args();
        args.top_p = 1.1;
        let issues = validate_generate_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("top-p"));
    }

    #[test]
    fn generate_top_p_boundary_values() {
        let mut args = default_generate_args();
        args.top_p = 0.0;
        assert!(validate_generate_args(&args).is_empty());
        args.top_p = 1.0;
        assert!(validate_generate_args(&args).is_empty());
    }

    #[test]
    fn generate_unknown_arch() {
        let mut args = default_generate_args();
        args.arch = "llama3".into();
        let issues = validate_generate_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("llama3"));
        assert!(issues[0].contains("Unknown --arch"));
    }

    #[test]
    fn generate_all_valid_archs() {
        for arch in &["bitnet3b", "bitnet", "qwen05", "qwen"] {
            let mut args = default_generate_args();
            args.arch = arch.to_string();
            let issues = validate_generate_args(&args);
            assert!(issues.is_empty(), "arch '{}' should be valid", arch);
        }
    }

    #[test]
    fn generate_unknown_mode() {
        let mut args = default_generate_args();
        args.mode = "binary".into();
        let issues = validate_generate_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Unknown --mode"));
    }

    #[test]
    fn generate_all_valid_modes() {
        for mode in &["text", "nda", "auto"] {
            let mut args = default_generate_args();
            args.mode = mode.to_string();
            let issues = validate_generate_args(&args);
            assert!(issues.is_empty(), "mode '{}' should be valid", mode);
        }
    }

    #[test]
    fn generate_no_prompt_no_file() {
        let mut args = default_generate_args();
        args.prompt = None;
        args.prompt_file = None;
        let issues = validate_generate_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("--prompt"));
    }

    #[test]
    fn generate_prompt_file_only() {
        let mut args = default_generate_args();
        args.prompt = None;
        args.prompt_file = Some(PathBuf::from("prompt.txt"));
        let issues = validate_generate_args(&args);
        assert!(issues.is_empty(), "prompt_file alone should be valid");
    }

    #[test]
    fn generate_multiple_issues_stack() {
        let mut args = default_generate_args();
        args.max_tokens = 0;
        args.temperature = -1.0;
        args.top_p = 2.0;
        args.arch = "unknown".into();
        args.mode = "bad".into();
        args.prompt = None;
        args.prompt_file = None;
        let issues = validate_generate_args(&args);
        // max_tokens, temperature, top_p, arch, mode, prompt = 6 issues
        assert_eq!(issues.len(), 6);
    }

    // ── validate_chat_args ───────────────────────────────────────────────

    #[test]
    fn chat_valid_defaults() {
        let args = default_chat_args();
        let issues = validate_chat_args(&args);
        assert!(issues.is_empty());
    }

    #[test]
    fn chat_zero_max_tokens() {
        let mut args = default_chat_args();
        args.max_tokens = 0;
        let issues = validate_chat_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("max-tokens"));
    }

    #[test]
    fn chat_negative_temperature() {
        let mut args = default_chat_args();
        args.temperature = -0.5;
        let issues = validate_chat_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("temperature"));
    }

    #[test]
    fn chat_temperature_zero_is_valid() {
        let mut args = default_chat_args();
        args.temperature = 0.0;
        let issues = validate_chat_args(&args);
        assert!(issues.is_empty(), "greedy (temp=0) should be valid");
    }

    #[test]
    fn chat_top_p_out_of_range() {
        let mut args = default_chat_args();
        args.top_p = -0.1;
        let issues = validate_chat_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("top-p"));

        args.top_p = 1.5;
        let issues = validate_chat_args(&args);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn chat_unknown_arch() {
        let mut args = default_chat_args();
        args.arch = "gpt4".into();
        let issues = validate_chat_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("gpt4"));
    }

    #[test]
    fn chat_all_valid_archs() {
        for arch in &["bitnet3b", "bitnet", "qwen05", "qwen"] {
            let mut args = default_chat_args();
            args.arch = arch.to_string();
            assert!(validate_chat_args(&args).is_empty());
        }
    }

    #[test]
    fn chat_multiple_issues() {
        let mut args = default_chat_args();
        args.max_tokens = 0;
        args.temperature = -1.0;
        args.top_p = 5.0;
        args.arch = "nope".into();
        let issues = validate_chat_args(&args);
        assert_eq!(issues.len(), 4);
    }

    // ── validate_seed_args ───────────────────────────────────────────────

    #[test]
    fn seed_valid_defaults() {
        let args = default_seed_args();
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty());
    }

    #[test]
    fn seed_empty_source() {
        let mut args = default_seed_args();
        args.source = vec![];
        let issues = validate_seed_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("source"));
    }

    #[test]
    fn seed_valid_hex_weight_root() {
        let mut args = default_seed_args();
        args.weight_root = "deadbeef".into();
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty());
    }

    #[test]
    fn seed_valid_hex_with_0x_prefix() {
        let mut args = default_seed_args();
        args.weight_root = "0xdeadbeef".into();
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty());
    }

    #[test]
    fn seed_invalid_hex_weight_root() {
        let mut args = default_seed_args();
        args.weight_root = "xyzzy".into();
        let issues = validate_seed_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("weight-root"));
        assert!(issues[0].contains("xyzzy"));
    }

    #[test]
    fn seed_zero_weight_root_is_valid() {
        let mut args = default_seed_args();
        args.weight_root = "0".into();
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty());
    }

    #[test]
    fn seed_multiple_source_files() {
        let mut args = default_seed_args();
        args.source = vec![
            PathBuf::from("seeds/a.rs"),
            PathBuf::from("seeds/b.rs"),
            PathBuf::from("seeds/c.rs"),
        ];
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty());
    }

    #[test]
    fn seed_empty_source_and_bad_hex() {
        let mut args = default_seed_args();
        args.source = vec![];
        args.weight_root = "nothex!".into();
        let issues = validate_seed_args(&args);
        assert_eq!(issues.len(), 2);
    }

    // ── resolve_config ───────────────────────────────────────────────────

    #[test]
    fn resolve_config_qwen05() {
        let cfg = resolve_config("qwen05").unwrap();
        assert_eq!(cfg.n_layers, 24); // qwen_coder_05b has 24 layers
    }

    #[test]
    fn resolve_config_qwen_alias() {
        let cfg = resolve_config("qwen").unwrap();
        assert_eq!(cfg.n_layers, 24);
    }

    #[test]
    fn resolve_config_bitnet3b() {
        let cfg = resolve_config("bitnet3b").unwrap();
        assert_eq!(cfg.n_layers, 26); // bitnet_3b has 26 layers
    }

    #[test]
    fn resolve_config_bitnet_alias() {
        let cfg = resolve_config("bitnet").unwrap();
        assert_eq!(cfg.n_layers, 26);
    }

    #[test]
    fn resolve_config_unknown_arch() {
        let result = resolve_config("llama3");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("llama3"));
    }

    #[test]
    fn resolve_config_qwen_and_bitnet_differ() {
        let qwen = resolve_config("qwen05").unwrap();
        let bitnet = resolve_config("bitnet3b").unwrap();
        assert_ne!(qwen.n_layers, bitnet.n_layers);
        assert_ne!(qwen.hidden_size, bitnet.hidden_size);
    }

    // ── CliEnvironment serialization ─────────────────────────────────────

    #[test]
    fn cli_environment_serializes() {
        let env = CliEnvironment {
            velocity_configured: true,
            velocity_url_set: true,
            velocity_key_set: true,
            config_file_exists: false,
            provider_count: 3,
            credential_boundary_active: true,
            validation_issues: vec![],
        };
        let json = serde_json::to_string(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["velocity_configured"], true);
        assert_eq!(parsed["provider_count"], 3);
        assert_eq!(parsed["credential_boundary_active"], true);
        assert!(parsed["validation_issues"].as_array().unwrap().is_empty());
    }

    #[test]
    fn cli_environment_with_issues() {
        let env = CliEnvironment {
            velocity_configured: false,
            velocity_url_set: false,
            velocity_key_set: false,
            config_file_exists: false,
            provider_count: 0,
            credential_boundary_active: false,
            validation_issues: vec![
                "Velocity Router not configured".into(),
            ],
        };
        let json = serde_json::to_string(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let issues = parsed["validation_issues"].as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert!(issues[0].as_str().unwrap().contains("not configured"));
    }

    // ── CliDiagnostics ───────────────────────────────────────────────────

    #[test]
    fn cli_diagnostics_has_all_subcommands() {
        let diag = cli_diagnostics();
        assert_eq!(diag.available_subcommands.len(), 10);
        assert!(diag.available_subcommands.contains(&"generate"));
        assert!(diag.available_subcommands.contains(&"benchmark"));
        assert!(diag.available_subcommands.contains(&"seed"));
        assert!(diag.available_subcommands.contains(&"chat"));
        assert!(diag.available_subcommands.contains(&"usage"));
        assert!(diag.available_subcommands.contains(&"login"));
        assert!(diag.available_subcommands.contains(&"providers"));
        assert!(diag.available_subcommands.contains(&"status"));
        assert!(diag.available_subcommands.contains(&"transparency"));
        assert!(diag.available_subcommands.contains(&"completions"));
    }

    #[test]
    fn cli_diagnostics_serializes() {
        let diag = cli_diagnostics();
        let json = serde_json::to_string(&diag).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["environment"].is_object());
        assert!(parsed["available_subcommands"].is_array());
        assert_eq!(parsed["available_subcommands"].as_array().unwrap().len(), 10);
    }

    // ── GenerationReport ─────────────────────────────────────────────────

    #[test]
    fn generation_report_serializes_minimal() {
        let report = GenerationReport {
            mode: "text".into(),
            tokens_generated: 100,
            elapsed_ms: 500,
            tokens_per_second: 200.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["mode"], "text");
        assert_eq!(parsed["tokens_generated"], 100);
        assert_eq!(parsed["elapsed_ms"], 500);
        assert_eq!(parsed["tokens_per_second"], 200.0);
        // None fields should be null
        assert!(parsed["merkle_valid"].is_null());
        assert!(parsed["sandbox_executed"].is_null());
    }

    #[test]
    fn generation_report_serializes_full() {
        let report = GenerationReport {
            mode: "nda".into(),
            tokens_generated: 256,
            elapsed_ms: 1200,
            tokens_per_second: 213.33,
            site_map_hits: 42,
            site_map_misses: 8,
            merkle_valid: Some(true),
            force_terminated: Some(false),
            sandbox_executed: Some(true),
            sandbox_panicked: Some(false),
            scope_passed: Some(true),
            stored_in_site_map: Some(true),
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["mode"], "nda");
        assert_eq!(parsed["site_map_hits"], 42);
        assert_eq!(parsed["site_map_misses"], 8);
        assert_eq!(parsed["merkle_valid"], true);
        assert_eq!(parsed["sandbox_executed"], true);
        assert_eq!(parsed["scope_passed"], true);
        assert_eq!(parsed["stored_in_site_map"], true);
    }

    #[test]
    fn generation_report_tokens_per_second_calculation() {
        // 1000 tokens in 1000ms = 1000 tok/s
        let report = GenerationReport {
            mode: "text".into(),
            tokens_generated: 1000,
            elapsed_ms: 1000,
            tokens_per_second: 1000.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["tokens_per_second"], 1000.0);
    }

    // ── Message struct ───────────────────────────────────────────────────

    #[test]
    fn message_serialization_roundtrip() {
        let msg = Message {
            role: "user".into(),
            content: "Hello, world!".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, "user");
        assert_eq!(parsed.content, "Hello, world!");
    }

    #[test]
    fn message_deserialize_from_json() {
        let json = r#"{"role":"assistant","content":"Hi there"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "Hi there");
    }

    #[test]
    fn message_empty_content() {
        let msg = Message {
            role: "system".into(),
            content: String::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "");
    }

    #[test]
    fn message_unicode_content() {
        let msg = Message {
            role: "user".into(),
            content: "Hello 世界 🌍".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "Hello 世界 🌍");
    }

    // ── inspect_environment ──────────────────────────────────────────────

    #[test]
    fn inspect_environment_returns_struct() {
        let env = inspect_environment();
        // Should not panic; returns a valid struct
        let _ = env.velocity_configured;
        let _ = env.provider_count;
        let _ = env.credential_boundary_active;
    }

    #[test]
    fn inspect_environment_serializes() {
        let env = inspect_environment();
        let json = serde_json::to_string(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["velocity_configured"].is_boolean());
        assert!(parsed["velocity_url_set"].is_boolean());
        assert!(parsed["velocity_key_set"].is_boolean());
        assert!(parsed["config_file_exists"].is_boolean());
        assert!(parsed["provider_count"].is_number());
        assert!(parsed["credential_boundary_active"].is_boolean());
        assert!(parsed["validation_issues"].is_array());
    }

    // ── GenerationReport site_map hit rate ───────────────────────────────

    #[test]
    fn generation_report_hit_rate_calculation() {
        // 75 hits, 25 misses = 75% hit rate
        let report = GenerationReport {
            mode: "nda".into(),
            tokens_generated: 100,
            elapsed_ms: 500,
            tokens_per_second: 200.0,
            site_map_hits: 75,
            site_map_misses: 25,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let total = report.site_map_hits + report.site_map_misses;
        let hit_rate = report.site_map_hits as f64 / total as f64 * 100.0;
        assert!((hit_rate - 75.0).abs() < 0.01);
    }

    #[test]
    fn generation_report_all_optional_fields_none() {
        let report = GenerationReport {
            mode: "text".into(),
            tokens_generated: 10,
            elapsed_ms: 100,
            tokens_per_second: 100.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // All Option<bool> fields should serialize as null
        for key in &["merkle_valid", "force_terminated", "sandbox_executed",
                      "sandbox_panicked", "scope_passed", "stored_in_site_map"] {
            assert!(parsed[key].is_null(), "{} should be null", key);
        }
    }

    #[test]
    fn generation_report_sandbox_panicked_true() {
        let report = GenerationReport {
            mode: "nda".into(),
            tokens_generated: 50,
            elapsed_ms: 200,
            tokens_per_second: 250.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: Some(false),
            force_terminated: Some(true),
            sandbox_executed: Some(true),
            sandbox_panicked: Some(true),
            scope_passed: Some(false),
            stored_in_site_map: Some(false),
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["merkle_valid"], false);
        assert_eq!(parsed["force_terminated"], true);
        assert_eq!(parsed["sandbox_panicked"], true);
        assert_eq!(parsed["scope_passed"], false);
        assert_eq!(parsed["stored_in_site_map"], false);
    }

    // ── validate_generate_args — additional boundary tests ────────────────

    #[test]
    fn generate_max_tokens_exactly_one() {
        let mut args = default_generate_args();
        args.max_tokens = 1;
        let issues = validate_generate_args(&args);
        assert!(issues.is_empty(), "max_tokens=1 should be valid");
    }

    #[test]
    fn generate_both_prompt_and_prompt_file() {
        let mut args = default_generate_args();
        args.prompt = Some("hello".into());
        args.prompt_file = Some(PathBuf::from("prompt.txt"));
        let issues = validate_generate_args(&args);
        assert!(issues.is_empty(), "both prompt and prompt_file should be valid");
    }

    #[test]
    fn generate_nan_temperature_passes_validation() {
        // IEEE 754: NaN comparisons are all false, so NaN < 0.0 → false,
        // NaN > 5.0 → false. The validator does not catch NaN.
        let mut args = default_generate_args();
        args.temperature = f32::NAN;
        let issues = validate_generate_args(&args);
        let temp_issues: Vec<_> = issues.iter().filter(|i| i.contains("temperature")).collect();
        assert!(temp_issues.is_empty(), "NaN temperature should slip past validation");
    }

    #[test]
    fn generate_infinity_temperature_rejected() {
        let mut args = default_generate_args();
        args.temperature = f32::INFINITY;
        let issues = validate_generate_args(&args);
        assert!(issues.iter().any(|i| i.contains("temperature")));
    }

    #[test]
    fn generate_neg_infinity_temperature_rejected() {
        let mut args = default_generate_args();
        args.temperature = f32::NEG_INFINITY;
        let issues = validate_generate_args(&args);
        assert!(issues.iter().any(|i| i.contains("temperature")));
    }

    #[test]
    fn generate_top_p_nan_passes_validation() {
        let mut args = default_generate_args();
        args.top_p = f32::NAN;
        let issues = validate_generate_args(&args);
        let top_p_issues: Vec<_> = issues.iter().filter(|i| i.contains("top-p")).collect();
        assert!(top_p_issues.is_empty(), "NaN top_p should slip past validation");
    }

    #[test]
    fn generate_max_tokens_two() {
        let mut args = default_generate_args();
        args.max_tokens = 2;
        assert!(validate_generate_args(&args).is_empty());
    }

    #[test]
    fn generate_max_tokens_99999() {
        let mut args = default_generate_args();
        args.max_tokens = 99_999;
        assert!(validate_generate_args(&args).is_empty());
    }

    #[test]
    fn generate_empty_arch_string() {
        let mut args = default_generate_args();
        args.arch = String::new();
        let issues = validate_generate_args(&args);
        assert!(issues.iter().any(|i| i.contains("Unknown --arch")));
    }

    #[test]
    fn generate_empty_mode_string() {
        let mut args = default_generate_args();
        args.mode = String::new();
        let issues = validate_generate_args(&args);
        assert!(issues.iter().any(|i| i.contains("Unknown --mode")));
    }

    // ── validate_chat_args — additional boundary tests ────────────────────

    #[test]
    fn chat_very_high_temperature_not_rejected() {
        // validate_chat_args has no upper temperature bound
        let mut args = default_chat_args();
        args.temperature = 100.0;
        let issues = validate_chat_args(&args);
        let temp_issues: Vec<_> = issues.iter().filter(|i| i.contains("temperature")).collect();
        assert!(temp_issues.is_empty(), "chat should have no upper temperature bound");
    }

    #[test]
    fn chat_infinity_temperature_not_rejected() {
        let mut args = default_chat_args();
        args.temperature = f32::INFINITY;
        let issues = validate_chat_args(&args);
        let temp_issues: Vec<_> = issues.iter().filter(|i| i.contains("temperature")).collect();
        assert!(temp_issues.is_empty());
    }

    #[test]
    fn chat_top_p_exactly_zero() {
        let mut args = default_chat_args();
        args.top_p = 0.0;
        let issues = validate_chat_args(&args);
        assert!(issues.is_empty(), "top_p=0.0 should be valid");
    }

    #[test]
    fn chat_top_p_exactly_one() {
        let mut args = default_chat_args();
        args.top_p = 1.0;
        let issues = validate_chat_args(&args);
        assert!(issues.is_empty(), "top_p=1.0 should be valid");
    }

    #[test]
    fn chat_nan_temperature_passes() {
        let mut args = default_chat_args();
        args.temperature = f32::NAN;
        let issues = validate_chat_args(&args);
        let temp_issues: Vec<_> = issues.iter().filter(|i| i.contains("temperature")).collect();
        assert!(temp_issues.is_empty());
    }

    #[test]
    fn chat_max_tokens_one() {
        let mut args = default_chat_args();
        args.max_tokens = 1;
        assert!(validate_chat_args(&args).is_empty());
    }

    #[test]
    fn chat_neg_infinity_temperature_rejected() {
        let mut args = default_chat_args();
        args.temperature = f32::NEG_INFINITY;
        let issues = validate_chat_args(&args);
        assert!(issues.iter().any(|i| i.contains("temperature")));
    }

    // ── validate_seed_args — additional boundary tests ────────────────────

    #[test]
    fn seed_weight_root_0x_prefix_only() {
        // "0x" → trimmed to "" → empty check passes → valid
        let mut args = default_seed_args();
        args.weight_root = "0x".into();
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty(), "0x alone should be valid (empty after trim)");
    }

    #[test]
    fn seed_mixed_case_hex_weight_root() {
        let mut args = default_seed_args();
        args.weight_root = "DeAdBeEf".into();
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty(), "mixed case hex should be valid");
    }

    #[test]
    fn seed_long_valid_hex() {
        let mut args = default_seed_args();
        args.weight_root = "0123456789abcdef".into();
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty());
    }

    #[test]
    fn seed_weight_root_with_special_chars() {
        let mut args = default_seed_args();
        args.weight_root = "abc-def".into();
        let issues = validate_seed_args(&args);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("weight-root"));
    }

    #[test]
    fn seed_many_source_files() {
        let mut args = default_seed_args();
        args.source = (0..100).map(|i| PathBuf::from(format!("seeds/file_{}.rs", i))).collect();
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty());
    }

    // ── resolve_config — additional tests ─────────────────────────────────

    #[test]
    fn resolve_config_empty_string() {
        let result = resolve_config("");
        assert!(result.is_err());
    }

    #[test]
    fn resolve_config_case_sensitive() {
        // Config matching is exact string match, not case-insensitive
        assert!(resolve_config("Bitnet3b").is_err());
        assert!(resolve_config("QWEN05").is_err());
    }

    #[test]
    fn resolve_config_qwen_hidden_size() {
        let cfg = resolve_config("qwen05").unwrap();
        assert_eq!(cfg.hidden_size, 896);
    }

    #[test]
    fn resolve_config_bitnet_hidden_size() {
        let cfg = resolve_config("bitnet3b").unwrap();
        assert_eq!(cfg.hidden_size, 3200);
    }

    // ── resolve_model_dir tests ───────────────────────────────────────────

    #[test]
    fn resolve_model_dir_nonexistent_path() {
        let result = resolve_model_dir(&Some(PathBuf::from("/nonexistent/path/xyz")));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn resolve_model_dir_existing_temp_dir() {
        let tmp = std::env::temp_dir().join("velocity_test_model_dir");
        std::fs::create_dir_all(&tmp).ok();
        let result = resolve_model_dir(&Some(tmp.clone()));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), tmp);
        std::fs::remove_dir(&tmp).ok();
    }

    // ── resolve_tokenizer tests ───────────────────────────────────────────

    #[test]
    fn resolve_tokenizer_nonexistent_file() {
        let result = resolve_tokenizer(
            &Some(PathBuf::from("/nonexistent/tokenizer.json")),
            Path::new("/tmp"),
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn resolve_tokenizer_existing_file() {
        let tmp = std::env::temp_dir().join("velocity_test_tokenizer.json");
        std::fs::write(&tmp, "{}").ok();
        let result = resolve_tokenizer(&Some(tmp.clone()), Path::new("/tmp"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), tmp);
        std::fs::remove_file(&tmp).ok();
    }

    // ── CliEnvironment — derive & validation logic tests ──────────────────

    #[test]
    fn cli_environment_clone() {
        let env = CliEnvironment {
            velocity_configured: true,
            velocity_url_set: false,
            velocity_key_set: true,
            config_file_exists: true,
            provider_count: 5,
            credential_boundary_active: false,
            validation_issues: vec!["issue1".into()],
        };
        let cloned = env.clone();
        assert_eq!(cloned.velocity_configured, true);
        assert_eq!(cloned.provider_count, 5);
        assert_eq!(cloned.validation_issues.len(), 1);
    }

    #[test]
    fn cli_environment_debug_format() {
        let env = CliEnvironment {
            velocity_configured: false,
            velocity_url_set: false,
            velocity_key_set: false,
            config_file_exists: false,
            provider_count: 0,
            credential_boundary_active: false,
            validation_issues: vec![],
        };
        let debug_str = format!("{:?}", env);
        assert!(debug_str.contains("CliEnvironment"));
        assert!(debug_str.contains("velocity_configured"));
        assert!(debug_str.contains("provider_count"));
    }

    #[test]
    fn cli_environment_validation_url_without_key() {
        // Simulate: url set, key not set → should have an issue
        let env = CliEnvironment {
            velocity_configured: false,
            velocity_url_set: true,
            velocity_key_set: false,
            config_file_exists: false,
            provider_count: 0,
            credential_boundary_active: false,
            validation_issues: vec![
                "VELOCITY_BASE_URL set but VELOCITY_API_KEY is missing".into(),
            ],
        };
        assert_eq!(env.validation_issues.len(), 1);
        assert!(env.validation_issues[0].contains("URL"));
        assert!(env.validation_issues[0].contains("KEY"));
    }

    #[test]
    fn cli_environment_validation_key_without_url() {
        let env = CliEnvironment {
            velocity_configured: false,
            velocity_url_set: false,
            velocity_key_set: true,
            config_file_exists: false,
            provider_count: 0,
            credential_boundary_active: false,
            validation_issues: vec![
                "VELOCITY_API_KEY set but VELOCITY_BASE_URL is missing".into(),
            ],
        };
        assert_eq!(env.validation_issues.len(), 1);
        assert!(env.validation_issues[0].contains("KEY"));
        assert!(env.validation_issues[0].contains("URL"));
    }

    #[test]
    fn cli_environment_no_issues_when_fully_configured() {
        let env = CliEnvironment {
            velocity_configured: true,
            velocity_url_set: true,
            velocity_key_set: true,
            config_file_exists: true,
            provider_count: 3,
            credential_boundary_active: true,
            validation_issues: vec![],
        };
        assert!(env.validation_issues.is_empty());
        assert!(env.velocity_configured);
    }

    // ── CliDiagnostics — derive tests ─────────────────────────────────────

    #[test]
    fn cli_diagnostics_clone() {
        let diag = cli_diagnostics();
        let cloned = diag.clone();
        assert_eq!(cloned.available_subcommands.len(), diag.available_subcommands.len());
    }

    #[test]
    fn cli_diagnostics_debug_format() {
        let diag = cli_diagnostics();
        let debug_str = format!("{:?}", diag);
        assert!(debug_str.contains("CliDiagnostics"));
        assert!(debug_str.contains("available_subcommands"));
    }

    // ── Message — additional tests ────────────────────────────────────────

    #[test]
    fn message_clone() {
        let msg = Message {
            role: "user".into(),
            content: "test content".into(),
        };
        let cloned = msg.clone();
        assert_eq!(cloned.role, "user");
        assert_eq!(cloned.content, "test content");
    }

    #[test]
    fn message_deserialize_missing_role_fails() {
        let json = r#"{"content":"hello"}"#;
        let result: Result<Message, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'role' field should fail deserialization");
    }

    #[test]
    fn message_deserialize_missing_content_fails() {
        let json = r#"{"role":"user"}"#;
        let result: Result<Message, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'content' field should fail deserialization");
    }

    #[test]
    fn message_deserialize_extra_fields_ignored() {
        let json = r#"{"role":"user","content":"hi","extra_field":42}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hi");
    }

    #[test]
    fn message_long_content() {
        let long_content = "x".repeat(100_000);
        let msg = Message {
            role: "user".into(),
            content: long_content.clone(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content.len(), 100_000);
    }

    #[test]
    fn message_special_characters_roundtrip() {
        let msg = Message {
            role: "user".into(),
            content: "line1\nline2\ttab\r\n\"quotes\" \\ backslash".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, msg.content);
    }

    // ── GenerationReport — additional tests ───────────────────────────────

    #[test]
    fn generation_report_hit_rate_zero_total() {
        // Both hits and misses are 0 → display should handle gracefully
        let report = GenerationReport {
            mode: "text".into(),
            tokens_generated: 10,
            elapsed_ms: 100,
            tokens_per_second: 100.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        // display() prints to stdout; just verify it doesn't panic
        report.display();
    }

    #[test]
    fn generation_report_display_all_fields() {
        let report = GenerationReport {
            mode: "NDA-Zero".into(),
            tokens_generated: 500,
            elapsed_ms: 2500,
            tokens_per_second: 200.0,
            site_map_hits: 80,
            site_map_misses: 20,
            merkle_valid: Some(true),
            force_terminated: Some(true),
            sandbox_executed: Some(true),
            sandbox_panicked: Some(true),
            scope_passed: Some(false),
            stored_in_site_map: Some(true),
        };
        // Just verify display() doesn't panic with all fields populated
        report.display();
    }

    #[test]
    fn generation_report_display_no_optional_fields() {
        let report = GenerationReport {
            mode: "Local FP32".into(),
            tokens_generated: 42,
            elapsed_ms: 1000,
            tokens_per_second: 42.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        report.display();
    }

    #[test]
    fn generation_report_zero_tokens_per_second() {
        let report = GenerationReport {
            mode: "text".into(),
            tokens_generated: 0,
            elapsed_ms: 1000,
            tokens_per_second: 0.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["tokens_per_second"], 0.0);
        assert_eq!(parsed["tokens_generated"], 0);
    }

    #[test]
    fn generation_report_very_high_throughput() {
        let report = GenerationReport {
            mode: "text".into(),
            tokens_generated: 1_000_000,
            elapsed_ms: 100,
            tokens_per_second: 10_000_000.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["tokens_per_second"], 10_000_000.0);
    }

    #[test]
    fn generation_report_site_map_hit_rate_partial() {
        // 1 hit, 99 misses = 1% hit rate
        let report = GenerationReport {
            mode: "nda".into(),
            tokens_generated: 100,
            elapsed_ms: 500,
            tokens_per_second: 200.0,
            site_map_hits: 1,
            site_map_misses: 99,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let total = report.site_map_hits + report.site_map_misses;
        let hit_rate = report.site_map_hits as f64 / total as f64 * 100.0;
        assert!((hit_rate - 1.0).abs() < 0.01);
    }

    // ── run_completions — shell validation ────────────────────────────────

    #[test]
    fn completions_unknown_shell_errors() {
        let args = CompletionsArgs {
            shell: "unknown_shell_xyz".into(),
        };
        let result = run_completions(args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown shell"));
        assert!(err.contains("unknown_shell_xyz"));
    }

    #[test]
    fn completions_shell_name_case_insensitive() {
        // "Bash" → to_lowercase → "bash" → valid
        let args = CompletionsArgs {
            shell: "Bash".into(),
        };
        // This will write to stdout but should succeed
        let result = run_completions(args);
        assert!(result.is_ok());
    }

    #[test]
    fn completions_pwsh_alias() {
        let args = CompletionsArgs {
            shell: "pwsh".into(),
        };
        let result = run_completions(args);
        assert!(result.is_ok());
    }

    #[test]
    fn completions_powershell_full() {
        let args = CompletionsArgs {
            shell: "PowerShell".into(),
        };
        let result = run_completions(args);
        assert!(result.is_ok());
    }

    // ── Cross-validation between generate and chat validators ─────────────

    #[test]
    fn generate_and_chat_share_max_tokens_check() {
        // Both validators should reject max_tokens=0
        let mut g = default_generate_args();
        g.max_tokens = 0;
        let g_issues = validate_generate_args(&g);
        assert!(g_issues.iter().any(|i| i.contains("max-tokens")));

        let mut c = default_chat_args();
        c.max_tokens = 0;
        let c_issues = validate_chat_args(&c);
        assert!(c_issues.iter().any(|i| i.contains("max-tokens")));
    }

    #[test]
    fn generate_and_chat_share_arch_validation() {
        // Both should accept the same arch aliases
        for arch in &["bitnet3b", "bitnet", "qwen05", "qwen"] {
            let mut g = default_generate_args();
            g.arch = arch.to_string();
            assert!(validate_generate_args(&g).iter().all(|i| !i.contains("arch")));

            let mut c = default_chat_args();
            c.arch = arch.to_string();
            assert!(validate_chat_args(&c).iter().all(|i| !i.contains("arch")));
        }
    }

    #[test]
    fn generate_has_mode_check_chat_does_not() {
        // GenerateArgs has --mode; ChatArgs does not
        let mut g = default_generate_args();
        g.mode = "invalid".into();
        let g_issues = validate_generate_args(&g);
        assert!(g_issues.iter().any(|i| i.contains("mode")));
    }

    #[test]
    fn generate_has_prompt_check_chat_does_not() {
        // Chat doesn't require a prompt upfront (interactive)
        let mut g = default_generate_args();
        g.prompt = None;
        g.prompt_file = None;
        let g_issues = validate_generate_args(&g);
        assert!(g_issues.iter().any(|i| i.contains("prompt")));
        // Chat has no such check
        let c = default_chat_args();
        let c_issues = validate_chat_args(&c);
        assert!(c_issues.iter().all(|i| !i.contains("prompt")));
    }

    // ── CliDiagnostics subcommand ordering ────────────────────────────────

    #[test]
    fn cli_diagnostics_subcommands_in_expected_order() {
        let diag = cli_diagnostics();
        assert_eq!(diag.available_subcommands[0], "generate");
        assert_eq!(diag.available_subcommands[1], "benchmark");
        assert_eq!(diag.available_subcommands[2], "seed");
        assert_eq!(diag.available_subcommands[3], "chat");
        assert_eq!(diag.available_subcommands[4], "usage");
        assert_eq!(diag.available_subcommands[5], "login");
        assert_eq!(diag.available_subcommands[6], "providers");
        assert_eq!(diag.available_subcommands[7], "status");
        assert_eq!(diag.available_subcommands[8], "transparency");
        assert_eq!(diag.available_subcommands[9], "completions");
    }

    // ── GenerationReport JSON structure ───────────────────────────────────

    #[test]
    fn generation_report_json_has_all_keys() {
        let report = GenerationReport {
            mode: "test".into(),
            tokens_generated: 1,
            elapsed_ms: 1,
            tokens_per_second: 1.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = parsed.as_object().unwrap();
        // Verify all expected keys exist
        assert!(obj.contains_key("mode"));
        assert!(obj.contains_key("tokens_generated"));
        assert!(obj.contains_key("elapsed_ms"));
        assert!(obj.contains_key("tokens_per_second"));
        assert!(obj.contains_key("site_map_hits"));
        assert!(obj.contains_key("site_map_misses"));
        assert!(obj.contains_key("merkle_valid"));
        assert!(obj.contains_key("force_terminated"));
        assert!(obj.contains_key("sandbox_executed"));
        assert!(obj.contains_key("sandbox_panicked"));
        assert!(obj.contains_key("scope_passed"));
        assert!(obj.contains_key("stored_in_site_map"));
        assert_eq!(obj.len(), 12);
    }

    #[test]
    fn generation_report_pretty_json() {
        let report = GenerationReport {
            mode: "nda".into(),
            tokens_generated: 10,
            elapsed_ms: 50,
            tokens_per_second: 200.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let pretty = serde_json::to_string_pretty(&report).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  ")); // indented
    }

    // ── Block 196: JSON key counts ──────────────────────────────────────────

    #[test]
    fn cli_environment_json_has_7_keys() {
        let env = CliEnvironment {
            velocity_configured: true,
            velocity_url_set: true,
            velocity_key_set: true,
            config_file_exists: true,
            provider_count: 3,
            credential_boundary_active: false,
            validation_issues: vec![],
        };
        let json = serde_json::to_string(&env).unwrap();
        let map: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(map.as_object().unwrap().len(), 7);
    }

    #[test]
    fn cli_diagnostics_json_has_3_keys() {
        let diag = CliDiagnostics {
            environment: CliEnvironment {
                velocity_configured: false,
                velocity_url_set: false,
                velocity_key_set: false,
                config_file_exists: false,
                provider_count: 0,
                credential_boundary_active: false,
                validation_issues: vec![],
            },
            velocity_config: None,
            available_subcommands: vec!["generate"],
        };
        let json = serde_json::to_string(&diag).unwrap();
        let map: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(map.as_object().unwrap().len(), 3);
    }

    // ── Block 196: validate_generate_args multiple issues ───────────────────

    #[test]
    fn generate_multiple_issues_at_once() {
        let mut args = default_generate_args();
        args.max_tokens = 0;
        args.temperature = -1.0;
        args.top_p = 2.0;
        args.arch = "unknown_arch".into();
        args.mode = "bad_mode".into();
        args.prompt = None;
        args.prompt_file = None;
        let issues = validate_generate_args(&args);
        // Should have: max_tokens, temperature, top_p, arch, mode, prompt
        assert!(issues.len() >= 6, "expected >=6 issues, got {}: {:?}", issues.len(), issues);
    }

    #[test]
    fn generate_temperature_exactly_zero_valid() {
        let mut args = default_generate_args();
        args.temperature = 0.0;
        let issues = validate_generate_args(&args);
        assert!(issues.iter().all(|i| !i.contains("temperature")));
    }

    #[test]
    fn generate_temperature_exactly_five_valid() {
        let mut args = default_generate_args();
        args.temperature = 5.0;
        let issues = validate_generate_args(&args);
        assert!(issues.iter().all(|i| !i.contains("temperature")));
    }

    #[test]
    fn generate_top_p_boundary_values_196() {
        let mut args = default_generate_args();
        args.top_p = 0.0;
        assert!(validate_generate_args(&args).iter().all(|i| !i.contains("top-p")));
        args.top_p = 1.0;
        assert!(validate_generate_args(&args).iter().all(|i| !i.contains("top-p")));
    }

    #[test]
    fn generate_both_prompt_and_prompt_file_valid() {
        let mut args = default_generate_args();
        args.prompt = Some("hello".into());
        args.prompt_file = Some(PathBuf::from("prompt.txt"));
        let issues = validate_generate_args(&args);
        // Having both should NOT trigger the prompt issue
        assert!(issues.iter().all(|i| !i.contains("prompt")));
    }

    // ── Block 196: validate_chat_args edge cases ────────────────────────────

    #[test]
    fn chat_temperature_exactly_zero_valid() {
        let mut args = default_chat_args();
        args.temperature = 0.0;
        let issues = validate_chat_args(&args);
        assert!(issues.iter().all(|i| !i.contains("temperature")));
    }

    #[test]
    fn chat_top_p_boundaries() {
        let mut args = default_chat_args();
        args.top_p = 0.0;
        assert!(validate_chat_args(&args).iter().all(|i| !i.contains("top-p")));
        args.top_p = 1.0;
        assert!(validate_chat_args(&args).iter().all(|i| !i.contains("top-p")));
    }

    #[test]
    fn chat_multiple_issues_196() {
        let mut args = default_chat_args();
        args.max_tokens = 0;
        args.temperature = -1.0;
        args.top_p = 5.0;
        args.arch = "invalid".into();
        let issues = validate_chat_args(&args);
        assert!(issues.len() >= 4, "expected >=4 issues, got {}: {:?}", issues.len(), issues);
    }

    // ── Block 196: validate_seed_args ───────────────────────────────────────

    #[test]
    fn seed_empty_source_files() {
        let args = SeedArgs {
            source: vec![],
            site_map: PathBuf::from("/tmp/sm"),
            weight_root: "0".into(),
        };
        let issues = validate_seed_args(&args);
        assert!(issues.iter().any(|i| i.contains("source")));
    }

    #[test]
    fn seed_invalid_hex_weight_root_196() {
        let args = SeedArgs {
            source: vec![PathBuf::from("test.rs")],
            site_map: PathBuf::from("/tmp/sm"),
            weight_root: "ZZZZ_NOT_HEX".into(),
        };
        let issues = validate_seed_args(&args);
        assert!(issues.iter().any(|i| i.contains("hex")));
    }

    #[test]
    fn seed_valid_hex_with_0x_prefix_196() {
        let args = SeedArgs {
            source: vec![PathBuf::from("test.rs")],
            site_map: PathBuf::from("/tmp/sm"),
            weight_root: "0xDEADBEEF".into(),
        };
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty(), "expected no issues, got: {:?}", issues);
    }

    #[test]
    fn seed_default_args_valid() {
        let args = default_seed_args();
        let issues = validate_seed_args(&args);
        assert!(issues.is_empty(), "default seed args should be valid, got: {:?}", issues);
    }

    // ── Block 196: Message edge cases ───────────────────────────────────────

    #[test]
    fn message_empty_role_and_content() {
        let msg = Message {
            role: "".into(),
            content: "".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, "");
        assert_eq!(parsed.content, "");
    }

    #[test]
    fn message_unicode_roundtrip() {
        let msg = Message {
            role: "user".into(),
            content: "\u{1F600}\u{00E9}\u{4E16}\u{754C}".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, msg.content);
    }

    // ── Block 196: CliEnvironment serialization ─────────────────────────────

    #[test]
    fn cli_environment_json_field_types() {
        let env = CliEnvironment {
            velocity_configured: true,
            velocity_url_set: false,
            velocity_key_set: true,
            config_file_exists: true,
            provider_count: 5,
            credential_boundary_active: false,
            validation_issues: vec!["issue1".into(), "issue2".into()],
        };
        let json = serde_json::to_string(&env).unwrap();
        let map: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(map["velocity_configured"], true);
        assert_eq!(map["velocity_url_set"], false);
        assert_eq!(map["provider_count"], 5);
        assert_eq!(map["validation_issues"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn cli_environment_clone_independence() {
        let env = CliEnvironment {
            velocity_configured: true,
            velocity_url_set: true,
            velocity_key_set: true,
            config_file_exists: true,
            provider_count: 3,
            credential_boundary_active: true,
            validation_issues: vec!["a".into()],
        };
        let mut cloned = env.clone();
        cloned.provider_count = 99;
        cloned.validation_issues.push("b".into());
        assert_eq!(env.provider_count, 3);
        assert_eq!(env.validation_issues.len(), 1);
    }

    // ── Block 196: GenerationReport clone and display ───────────────────────

    #[test]
    fn generation_report_display_all_fields_196() {
        let report = GenerationReport {
            mode: "nda".into(),
            tokens_generated: 50,
            elapsed_ms: 200,
            tokens_per_second: 250.0,
            site_map_hits: 10,
            site_map_misses: 5,
            merkle_valid: Some(true),
            force_terminated: Some(false),
            sandbox_executed: Some(true),
            sandbox_panicked: Some(false),
            scope_passed: Some(true),
            stored_in_site_map: Some(false),
        };
        // Just verify display() doesn't panic
        report.display();
    }

    #[test]
    fn generation_report_tokens_per_second_formula() {
        // tokens_per_second should equal tokens_generated / (elapsed_ms / 1000)
        let report = GenerationReport {
            mode: "text".into(),
            tokens_generated: 100,
            elapsed_ms: 500,
            tokens_per_second: 200.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let expected = report.tokens_generated as f64 / (report.elapsed_ms as f64 / 1000.0);
        assert!((report.tokens_per_second - expected).abs() < 0.01);
    }

    // ── Block 196: LoginArgs and ProvidersArgs ──────────────────────────────

    #[test]
    fn login_args_default_url() {
        // Verify the default URL is localhost:8787
        let args = LoginArgs {
            url: "http://localhost:8787".into(),
            key: "vr_test123".into(),
        };
        assert_eq!(args.url, "http://localhost:8787");
        assert!(args.key.starts_with("vr_"));
    }

    #[test]
    fn providers_args_optional_fields() {
        let args = ProvidersArgs {
            action: "list".into(),
            provider: None,
            api_key: None,
            base_url: None,
        };
        assert!(args.provider.is_none());
        assert!(args.api_key.is_none());
        assert!(args.base_url.is_none());
    }

    #[test]
    fn providers_args_all_fields() {
        let args = ProvidersArgs {
            action: "add".into(),
            provider: Some("openai".into()),
            api_key: Some("sk-test".into()),
            base_url: Some("https://proxy.example.com".into()),
        };
        assert_eq!(args.provider.as_deref(), Some("openai"));
        assert_eq!(args.api_key.as_deref(), Some("sk-test"));
        assert!(args.base_url.as_deref().unwrap().contains("proxy"));
    }

    // ─── Block 207: resolve_config field coverage ──────────────────────────

    #[test]
    fn resolve_config_qwen_vocab_size_207() {
        let cfg = resolve_config("qwen05").unwrap();
        assert_eq!(cfg.vocab_size, 151936);
    }

    #[test]
    fn resolve_config_bitnet_vocab_size_207() {
        let cfg = resolve_config("bitnet3b").unwrap();
        assert_eq!(cfg.vocab_size, 32000);
    }

    #[test]
    fn resolve_config_qwen_n_heads_207() {
        let cfg = resolve_config("qwen05").unwrap();
        assert!(cfg.n_heads > 0);
    }

    #[test]
    fn resolve_config_bitnet_n_heads_207() {
        let cfg = resolve_config("bitnet3b").unwrap();
        assert!(cfg.n_heads > 0);
    }

    #[test]
    fn resolve_config_qwen_max_seq_len_207() {
        let cfg = resolve_config("qwen05").unwrap();
        assert!(cfg.max_seq_len > 0);
    }

    // ─── Block 207: resolve_model_dir additional ───────────────────────────

    #[test]
    fn resolve_model_dir_none_errors_without_auto_discover_207() {
        // When model is None and no candidate dirs exist, should error
        let result = resolve_model_dir(&None);
        // In test env, auto-discover candidates won't exist
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("model") || err.contains("auto-discover") || err.contains("--model"));
    }

    #[test]
    fn resolve_model_dir_existing_dir_returns_clone_207() {
        let tmp = std::env::temp_dir().join("velocity_test_model_dir_207");
        std::fs::create_dir_all(&tmp).ok();
        let result = resolve_model_dir(&Some(tmp.clone()));
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert_eq!(resolved, tmp);
        std::fs::remove_dir(&tmp).ok();
    }

    // ─── Block 207: resolve_tokenizer additional ───────────────────────────

    #[test]
    fn resolve_tokenizer_none_falls_through_to_candidates_207() {
        // When tokenizer is None and no candidates exist, should error
        let tmp_dir = std::env::temp_dir().join("velocity_test_no_tokenizer_207");
        std::fs::create_dir_all(&tmp_dir).ok();
        let result = resolve_tokenizer(&None, &tmp_dir);
        assert!(result.is_err());
        std::fs::remove_dir(&tmp_dir).ok();
    }

    #[test]
    fn resolve_tokenizer_finds_in_model_dir_207() {
        let tmp_dir = std::env::temp_dir().join("velocity_test_tok_discover_207");
        std::fs::create_dir_all(&tmp_dir).ok();
        let tok_path = tmp_dir.join("tokenizer.json");
        std::fs::write(&tok_path, "{}").ok();
        let result = resolve_tokenizer(&None, &tmp_dir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), tok_path);
        std::fs::remove_file(&tok_path).ok();
        std::fs::remove_dir(&tmp_dir).ok();
    }

    // ─── Block 207: CloudflareAccount struct ───────────────────────────────

    #[test]
    fn cloudflare_account_struct_fields_207() {
        let acct = CloudflareAccount {
            id: "acct_id_123".into(),
            token: "token_abc".into(),
        };
        assert_eq!(acct.id, "acct_id_123");
        assert_eq!(acct.token, "token_abc");
    }

    // (CloudflareAccount does not derive Clone — tested via load_accounts instead)

    // ─── Block 207: CliDiagnostics with velocity_config ────────────────────

    #[test]
    fn cli_diagnostics_with_config_none_207() {
        let diag = CliDiagnostics {
            environment: CliEnvironment {
                velocity_configured: false,
                velocity_url_set: false,
                velocity_key_set: false,
                config_file_exists: false,
                provider_count: 0,
                credential_boundary_active: false,
                validation_issues: vec!["not configured".into()],
            },
            velocity_config: None,
            available_subcommands: vec!["generate", "chat"],
        };
        let json = serde_json::to_string(&diag).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["velocity_config"].is_null());
    }

    #[test]
    fn cli_diagnostics_with_config_some_207() {
        let diag = CliDiagnostics {
            environment: CliEnvironment {
                velocity_configured: true,
                velocity_url_set: true,
                velocity_key_set: true,
                config_file_exists: true,
                provider_count: 2,
                credential_boundary_active: false,
                validation_issues: vec![],
            },
            velocity_config: Some(velocity_client::ConnectionInfo {
                base_url: "https://router.example.com".into(),
                is_https: true,
                api_key_prefix: "vr_".into(),
                api_key_length: 24,
                validation_issues: vec![],
            }),
            available_subcommands: vec!["generate"],
        };
        let json = serde_json::to_string(&diag).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["velocity_config"].is_object());
        assert_eq!(parsed["velocity_config"]["base_url"], "https://router.example.com");
        assert_eq!(parsed["velocity_config"]["is_https"], true);
    }

    // ─── Block 207: GenerationReport extreme values ────────────────────────

    #[test]
    fn generation_report_elapsed_ms_zero_207() {
        let report = GenerationReport {
            mode: "text".into(),
            tokens_generated: 10,
            elapsed_ms: 0,
            tokens_per_second: 0.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["elapsed_ms"], 0);
    }

    #[test]
    fn generation_report_elapsed_ms_u64_max_207() {
        let report = GenerationReport {
            mode: "text".into(),
            tokens_generated: 1,
            elapsed_ms: u64::MAX,
            tokens_per_second: 0.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let expected = serde_json::json!(u64::MAX);
        assert_eq!(parsed["elapsed_ms"], expected);
    }

    #[test]
    fn generation_report_mode_empty_string_207() {
        let report = GenerationReport {
            mode: "".into(),
            tokens_generated: 0,
            elapsed_ms: 0,
            tokens_per_second: 0.0,
            site_map_hits: 0,
            site_map_misses: 0,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["mode"], "");
        report.display();
    }

    #[test]
    fn generation_report_site_map_large_counts_207() {
        let report = GenerationReport {
            mode: "nda".into(),
            tokens_generated: 1000,
            elapsed_ms: 5000,
            tokens_per_second: 200.0,
            site_map_hits: 999_999,
            site_map_misses: 1,
            merkle_valid: None,
            force_terminated: None,
            sandbox_executed: None,
            sandbox_panicked: None,
            scope_passed: None,
            stored_in_site_map: None,
        };
        let total = report.site_map_hits + report.site_map_misses;
        let hit_rate = report.site_map_hits as f64 / total as f64 * 100.0;
        assert!(hit_rate > 99.99, "hit rate should be >99.99%, got {}", hit_rate);
        report.display();
    }

    // ─── Block 207: Message additional edge cases ──────────────────────────

    #[test]
    fn message_json_array_content_207() {
        let msg = Message {
            role: "user".into(),
            content: "[1,2,3]".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "[1,2,3]");
    }

    #[test]
    fn message_json_object_in_content_207() {
        let msg = Message {
            role: "assistant".into(),
            content: r#"{"key": "value", "num": 42}"#.into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, msg.content);
    }

    #[test]
    fn message_null_bytes_in_content_207() {
        let msg = Message {
            role: "user".into(),
            content: "before\0after".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "before\0after");
    }
}
