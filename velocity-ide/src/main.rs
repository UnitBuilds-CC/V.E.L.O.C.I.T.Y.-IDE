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

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

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

// ─── Entry point ───────────────────────────────────────────────────────────

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    match cli.command {
        Command::Chat(args) => run_chat(args),
        Command::Seed(args) => run_seed(args),
        Command::Generate(args) => {
            if args.zero_float {
                let mode = crate::pipeline_nda::PipelineMode::from_str(&args.mode);
                if mode != crate::pipeline_nda::PipelineMode::Text {
                    run_generate_zero_nda(args, mode)
                } else {
                    run_generate_zero(args)
                }
            } else if args.arch != "bitnet3b" && args.arch != "bitnet" {
                // Non-BitNet architectures use local FP32 transformer inference
                run_generate_local(args)
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

fn run_generate_zero(args: GenerateArgs) -> Result<()> {
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
    println!();
    eprintln!(
        "\n--- Zero-Float Generation ---\nTokens : {}\nTime   : {:.2}s\nTok/s  : {:.2}",
        generated.len(),
        elapsed_s,
        generated.len() as f32 / elapsed_s.max(1e-6),
    );

    Ok(())
}

// ─── FP32 Local Generate (GPU float path for FP4/FP2 weights) ────────────────

fn run_generate_local(args: GenerateArgs) -> Result<()> {
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
    println!();
    eprintln!(
        "\n--- Local FP32 Generation ---\nTokens : {}\nTime   : {:.2}s\nTok/s  : {:.2}",
        generated.len(),
        elapsed_s,
        generated.len() as f32 / elapsed_s.max(1e-6),
    );

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
                history.push(Message {
                    role: "assistant".to_string(),
                    content: response,
                });
                let elapsed = t_gen.elapsed().as_secs_f32();
                println!("--- Response time: {:.2}s ---\n", elapsed);
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }

    Ok(())
}
