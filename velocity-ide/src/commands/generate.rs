// commands/generate.rs — Text generation subcommands (Cloudflare, Zero-Float, Local FP32, NDA)

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use velocity_ide::{credential_guard, model, pipeline_bridge, pipeline_nda, tokenizer};

// ─── CLI Args ────────────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct GenerateArgs {
    /// Directory containing converted NDA weight files (.nda / .bin)
    #[arg(long, value_name = "DIR")]
    pub model: Option<PathBuf>,

    /// Path to tokenizer.json (defaults to <model-dir>/../tokenizer.json)
    #[arg(long, value_name = "FILE")]
    pub tokenizer: Option<PathBuf>,

    /// Input prompt text
    #[arg(long, value_name = "TEXT")]
    pub prompt: Option<String>,

    /// Path to file containing the input prompt text
    #[arg(long, value_name = "FILE")]
    pub prompt_file: Option<PathBuf>,

    /// Maximum number of new tokens to generate
    #[arg(long, default_value = "512", value_name = "N")]
    pub max_tokens: usize,

    /// Sampling temperature (0 = greedy, 0.7 = default, >1 = creative)
    #[arg(long, default_value = "0.7", value_name = "T")]
    pub temperature: f32,

    /// Top-p nucleus sampling threshold
    #[arg(long, default_value = "0.9", value_name = "P")]
    pub top_p: f32,

    /// Use zero-float NDA-Zero runtime (pure integer, ALiBi, argmax greedy).
    #[arg(long, default_value = "false")]
    pub zero_float: bool,

    /// Model architecture preset: 'bitnet3b' (default) or 'qwen05'
    #[arg(long, default_value = "bitnet3b", value_name = "ARCH")]
    pub arch: String,

    /// Pipeline mode: 'text', 'nda', 'auto'. Only applies when --zero-float is set.
    #[arg(long, default_value = "text", value_name = "MODE")]
    pub mode: String,

    /// Path to site map directory for persistent KV cache (NDA native mode).
    #[arg(long, value_name = "DIR")]
    pub site_map: Option<PathBuf>,
}

/// Validate generate arguments before dispatching to a backend.
pub fn validate_generate_args(args: &GenerateArgs) -> Vec<String> {
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
        other => issues.push(format!(
            "Unknown --arch '{}'. Use 'bitnet3b' or 'qwen05'.",
            other
        )),
    }
    match args.mode.as_str() {
        "text" | "nda" | "auto" => {}
        other => issues.push(format!(
            "Unknown --mode '{}'. Use 'text', 'nda', or 'auto'.",
            other
        )),
    }
    if args.prompt.is_none() && args.prompt_file.is_none() {
        issues.push("Either --prompt or --prompt-file must be provided".into());
    }
    issues
}

// ─── Shared Types ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Structured execution report for generation results.
#[derive(Serialize)]
pub struct GenerationReport {
    pub mode: String,
    pub tokens_generated: usize,
    pub elapsed_ms: u64,
    pub tokens_per_second: f64,
    pub site_map_hits: usize,
    pub site_map_misses: usize,
    pub merkle_valid: Option<bool>,
    pub force_terminated: Option<bool>,
    pub sandbox_executed: Option<bool>,
    pub sandbox_panicked: Option<bool>,
    pub scope_passed: Option<bool>,
    pub stored_in_site_map: Option<bool>,
}

impl GenerationReport {
    pub fn display(&self) {
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
            println!(
                "  SiteMap:    {} hits / {} misses ({:.1}% hit rate)",
                self.site_map_hits, self.site_map_misses, hit_rate
            );
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
            println!(
                "  Sandbox:    {}",
                if executed { "executed" } else { "skipped" }
            );
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

pub struct CloudflareAccount {
    pub id: String,
    pub token: String,
}

pub fn load_accounts() -> Vec<CloudflareAccount> {
    dotenvy::dotenv().ok();
    let mut accounts = Vec::new();
    for i in 1..=30 {
        let id_key = format!("CF_ACCOUNT_{}_ID", i);
        let token_key = format!("CF_ACCOUNT_{}_TOKEN", i);
        if let (Ok(id), Ok(token)) = (std::env::var(&id_key), std::env::var(&token_key)) {
            accounts.push(CloudflareAccount { id, token });
        }
    }
    if !accounts.is_empty() {
        let scrubbed = credential_guard::scrub_sensitive_env_vars();
        if !scrubbed.is_empty() {
            log::debug!(
                "Scrubbed {} sensitive env vars from process",
                scrubbed.len()
            );
        }
    }
    accounts
}

pub fn call_kimi(messages: &[Message], accounts: &[CloudflareAccount]) -> Result<String> {
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
                continue;
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

// ─── Resolve helpers ─────────────────────────────────────────────────────────

pub fn resolve_config(arch: &str) -> Result<model::config::ModelConfig> {
    match arch {
        "qwen05" | "qwen" => Ok(model::config::ModelConfig::qwen_coder_05b()),
        "bitnet3b" | "bitnet" => Ok(model::config::ModelConfig::bitnet_3b()),
        other => anyhow::bail!("Unknown --arch '{other}'. Use 'qwen05' or 'bitnet3b'."),
    }
}

pub fn resolve_model_dir(model: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(ref d) = model {
        if d.exists() {
            return Ok(d.clone());
        }
        anyhow::bail!("--model directory does not exist: {d:?}");
    }
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

pub fn resolve_tokenizer(tokenizer: &Option<PathBuf>, model_dir: &Path) -> Result<PathBuf> {
    if let Some(ref t) = tokenizer {
        if t.exists() {
            return Ok(t.clone());
        }
        anyhow::bail!("--tokenizer file does not exist: {t:?}");
    }
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

// ─── Run functions ───────────────────────────────────────────────────────────

pub fn run_generate(args: GenerateArgs) -> Result<()> {
    let accounts = load_accounts();
    if accounts.is_empty() {
        anyhow::bail!("No Cloudflare accounts found in parent .env. Please configure them first.");
    }
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

pub fn run_generate_zero(args: GenerateArgs, json: bool) -> Result<()> {
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
    let tok = tokenizer::Tokenizer::from_file(&tokenizer_path)?;

    let prompt_text = resolve_prompt(&args)?;
    let prompt_tokens = tok.encode(&prompt_text, true);
    eprintln!("[zero-float] Prompt: {} tokens", prompt_tokens.len());

    let t_gen = Instant::now();
    let mut generated = Vec::new();
    model.generate_greedy(&prompt_tokens, args.max_tokens, |tok_id| {
        let piece = tok.decode_token(tok_id);
        print!("{}", piece);
        std::io::stdout().flush().ok();
        generated.push(tok_id);
    });

    let elapsed = t_gen.elapsed();
    let elapsed_s = elapsed.as_secs_f32();
    let elapsed_ms = elapsed.as_millis() as u64;
    let tok_per_s = generated.len() as f64 / elapsed_s.max(1e-6) as f64;

    let report = GenerationReport {
        mode: "Zero-Float".into(),
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

pub fn run_generate_local(args: GenerateArgs, json: bool) -> Result<()> {
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
    let tok = tokenizer::Tokenizer::from_file(&tokenizer_path)?;

    let prompt_text = resolve_prompt(&args)?;
    let prompt_tokens = tok.encode(&prompt_text, true);
    eprintln!("[local] Prompt: {} tokens", prompt_tokens.len());

    let t_gen = Instant::now();
    let mut generated = Vec::new();
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
            let piece = tok.decode_token(tok_id);
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
        mode: "Local FP32".into(),
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

pub fn run_generate_zero_nda(args: GenerateArgs, mode: pipeline_nda::PipelineMode) -> Result<()> {
    let cfg = resolve_config(&args.arch)?;
    let model_dir = resolve_model_dir(&args.model)?;
    let tokenizer_path = resolve_tokenizer(&args.tokenizer, &model_dir)?;

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

/// Dispatch generate command based on args.
pub fn dispatch_generate(args: GenerateArgs, json: bool) -> Result<()> {
    if args.zero_float {
        let mode = pipeline_nda::PipelineMode::from_str(&args.mode);
        if mode != pipeline_nda::PipelineMode::Text {
            run_generate_zero_nda(args, mode)
        } else {
            run_generate_zero(args, json)
        }
    } else if args.arch != "bitnet3b" && args.arch != "bitnet" {
        run_generate_local(args, json)
    } else {
        run_generate(args)
    }
}

fn resolve_prompt(args: &GenerateArgs) -> Result<String> {
    if let Some(p) = &args.prompt {
        Ok(p.clone())
    } else if let Some(pf) = &args.prompt_file {
        std::fs::read_to_string(pf).context("Reading prompt file")
    } else {
        print!("Prompt: ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        Ok(line.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn generate_valid_defaults() {
        assert!(validate_generate_args(&default_generate_args()).is_empty());
    }
    #[test]
    fn generate_zero_max_tokens() {
        let mut a = default_generate_args();
        a.max_tokens = 0;
        assert_eq!(validate_generate_args(&a).len(), 1);
    }
    #[test]
    fn generate_max_tokens_over_100k() {
        let mut a = default_generate_args();
        a.max_tokens = 100_001;
        assert!(validate_generate_args(&a)[0].contains("100,000"));
    }
    #[test]
    fn generate_max_tokens_exactly_100k() {
        let mut a = default_generate_args();
        a.max_tokens = 100_000;
        assert!(validate_generate_args(&a).is_empty());
    }
    #[test]
    fn generate_negative_temperature() {
        let mut a = default_generate_args();
        a.temperature = -0.1;
        assert!(validate_generate_args(&a)[0].contains("temperature"));
    }
    #[test]
    fn generate_temperature_over_5() {
        let mut a = default_generate_args();
        a.temperature = 5.1;
        assert!(validate_generate_args(&a)[0].contains("5.0"));
    }
    #[test]
    fn generate_temperature_boundary_values() {
        let mut a = default_generate_args();
        a.temperature = 0.0;
        assert!(validate_generate_args(&a).is_empty());
        a.temperature = 5.0;
        assert!(validate_generate_args(&a).is_empty());
    }
    #[test]
    fn generate_top_p_below_zero() {
        let mut a = default_generate_args();
        a.top_p = -0.1;
        assert!(validate_generate_args(&a)[0].contains("top-p"));
    }
    #[test]
    fn generate_top_p_above_one() {
        let mut a = default_generate_args();
        a.top_p = 1.1;
        assert!(validate_generate_args(&a)[0].contains("top-p"));
    }
    #[test]
    fn generate_top_p_boundary_values() {
        let mut a = default_generate_args();
        a.top_p = 0.0;
        assert!(validate_generate_args(&a).is_empty());
        a.top_p = 1.0;
        assert!(validate_generate_args(&a).is_empty());
    }
    #[test]
    fn generate_unknown_arch() {
        let mut a = default_generate_args();
        a.arch = "llama3".into();
        assert!(validate_generate_args(&a)[0].contains("llama3"));
    }
    #[test]
    fn generate_all_valid_archs() {
        for arch in &["bitnet3b", "bitnet", "qwen05", "qwen"] {
            let mut a = default_generate_args();
            a.arch = arch.to_string();
            assert!(validate_generate_args(&a).is_empty());
        }
    }
    #[test]
    fn generate_unknown_mode() {
        let mut a = default_generate_args();
        a.mode = "binary".into();
        assert!(validate_generate_args(&a)[0].contains("Unknown --mode"));
    }
    #[test]
    fn generate_all_valid_modes() {
        for mode in &["text", "nda", "auto"] {
            let mut a = default_generate_args();
            a.mode = mode.to_string();
            assert!(validate_generate_args(&a).is_empty());
        }
    }
    #[test]
    fn generate_no_prompt_no_file() {
        let mut a = default_generate_args();
        a.prompt = None;
        a.prompt_file = None;
        assert!(validate_generate_args(&a)[0].contains("--prompt"));
    }
    #[test]
    fn generate_prompt_file_only() {
        let mut a = default_generate_args();
        a.prompt = None;
        a.prompt_file = Some(PathBuf::from("prompt.txt"));
        assert!(validate_generate_args(&a).is_empty());
    }
    #[test]
    fn generate_multiple_issues_stack() {
        let mut a = default_generate_args();
        a.max_tokens = 0;
        a.temperature = -1.0;
        a.top_p = 2.0;
        a.arch = "unknown".into();
        a.mode = "bad".into();
        a.prompt = None;
        a.prompt_file = None;
        assert_eq!(validate_generate_args(&a).len(), 6);
    }
    #[test]
    fn generate_max_tokens_exactly_one() {
        let mut a = default_generate_args();
        a.max_tokens = 1;
        assert!(validate_generate_args(&a).is_empty());
    }
    #[test]
    fn generate_both_prompt_and_prompt_file() {
        let mut a = default_generate_args();
        a.prompt = Some("hello".into());
        a.prompt_file = Some(PathBuf::from("p.txt"));
        assert!(validate_generate_args(&a).is_empty());
    }
    #[test]
    fn generate_nan_temperature_passes() {
        let mut a = default_generate_args();
        a.temperature = f32::NAN;
        assert!(validate_generate_args(&a)
            .iter()
            .all(|i| !i.contains("temperature")));
    }
    #[test]
    fn generate_infinity_temperature_rejected() {
        let mut a = default_generate_args();
        a.temperature = f32::INFINITY;
        assert!(validate_generate_args(&a)
            .iter()
            .any(|i| i.contains("temperature")));
    }
    #[test]
    fn generate_neg_infinity_temperature_rejected() {
        let mut a = default_generate_args();
        a.temperature = f32::NEG_INFINITY;
        assert!(validate_generate_args(&a)
            .iter()
            .any(|i| i.contains("temperature")));
    }
    #[test]
    fn generate_top_p_nan_passes() {
        let mut a = default_generate_args();
        a.top_p = f32::NAN;
        assert!(validate_generate_args(&a)
            .iter()
            .all(|i| !i.contains("top-p")));
    }
    #[test]
    fn generate_max_tokens_two() {
        let mut a = default_generate_args();
        a.max_tokens = 2;
        assert!(validate_generate_args(&a).is_empty());
    }
    #[test]
    fn generate_max_tokens_99999() {
        let mut a = default_generate_args();
        a.max_tokens = 99_999;
        assert!(validate_generate_args(&a).is_empty());
    }
    #[test]
    fn generate_empty_arch_string() {
        let mut a = default_generate_args();
        a.arch = String::new();
        assert!(validate_generate_args(&a)
            .iter()
            .any(|i| i.contains("Unknown --arch")));
    }
    #[test]
    fn generate_empty_mode_string() {
        let mut a = default_generate_args();
        a.mode = String::new();
        assert!(validate_generate_args(&a)
            .iter()
            .any(|i| i.contains("Unknown --mode")));
    }

    // resolve_config tests
    #[test]
    fn resolve_config_qwen05() {
        assert_eq!(resolve_config("qwen05").unwrap().n_layers, 24);
    }
    #[test]
    fn resolve_config_qwen_alias() {
        assert_eq!(resolve_config("qwen").unwrap().n_layers, 24);
    }
    #[test]
    fn resolve_config_bitnet3b() {
        assert_eq!(resolve_config("bitnet3b").unwrap().n_layers, 26);
    }
    #[test]
    fn resolve_config_bitnet_alias() {
        assert_eq!(resolve_config("bitnet").unwrap().n_layers, 26);
    }
    #[test]
    fn resolve_config_unknown_arch() {
        assert!(resolve_config("llama3").is_err());
    }
    #[test]
    fn resolve_config_qwen_and_bitnet_differ() {
        let q = resolve_config("qwen05").unwrap();
        let b = resolve_config("bitnet3b").unwrap();
        assert_ne!(q.n_layers, b.n_layers);
    }
    #[test]
    fn resolve_config_empty_string() {
        assert!(resolve_config("").is_err());
    }
    #[test]
    fn resolve_config_case_sensitive() {
        assert!(resolve_config("Bitnet3b").is_err());
    }
    #[test]
    fn resolve_config_qwen_hidden_size() {
        assert_eq!(resolve_config("qwen05").unwrap().hidden_size, 896);
    }
    #[test]
    fn resolve_config_bitnet_hidden_size() {
        assert_eq!(resolve_config("bitnet3b").unwrap().hidden_size, 3200);
    }
    #[test]
    fn resolve_config_qwen_vocab_size() {
        assert_eq!(resolve_config("qwen05").unwrap().vocab_size, 151936);
    }
    #[test]
    fn resolve_config_bitnet_vocab_size() {
        assert_eq!(resolve_config("bitnet3b").unwrap().vocab_size, 32000);
    }
    #[test]
    fn resolve_config_qwen_n_heads() {
        assert!(resolve_config("qwen05").unwrap().n_heads > 0);
    }
    #[test]
    fn resolve_config_bitnet_n_heads() {
        assert!(resolve_config("bitnet3b").unwrap().n_heads > 0);
    }
    #[test]
    fn resolve_config_qwen_max_seq_len() {
        assert!(resolve_config("qwen05").unwrap().max_seq_len > 0);
    }

    // resolve_model_dir tests
    #[test]
    fn resolve_model_dir_nonexistent() {
        assert!(resolve_model_dir(&Some(PathBuf::from("/nonexistent/path/xyz"))).is_err());
    }
    #[test]
    fn resolve_model_dir_existing_temp() {
        let tmp = std::env::temp_dir().join("velocity_test_model_dir");
        std::fs::create_dir_all(&tmp).ok();
        assert!(resolve_model_dir(&Some(tmp.clone())).is_ok());
        std::fs::remove_dir(&tmp).ok();
    }
    #[test]
    fn resolve_model_dir_none_errors() {
        assert!(resolve_model_dir(&None).is_err());
    }
    #[test]
    fn resolve_model_dir_clone() {
        let tmp = std::env::temp_dir().join("velocity_test_model_dir_207");
        std::fs::create_dir_all(&tmp).ok();
        let r = resolve_model_dir(&Some(tmp.clone())).unwrap();
        assert_eq!(r, tmp);
        std::fs::remove_dir(&tmp).ok();
    }

    // resolve_tokenizer tests
    #[test]
    fn resolve_tokenizer_nonexistent() {
        assert!(resolve_tokenizer(
            &Some(PathBuf::from("/nonexistent/tokenizer.json")),
            Path::new("/tmp")
        )
        .is_err());
    }
    #[test]
    fn resolve_tokenizer_existing() {
        let tmp = std::env::temp_dir().join("velocity_test_tokenizer.json");
        std::fs::write(&tmp, "{}").ok();
        assert!(resolve_tokenizer(&Some(tmp.clone()), Path::new("/tmp")).is_ok());
        std::fs::remove_file(&tmp).ok();
    }
    #[test]
    fn resolve_tokenizer_none_falls_through() {
        let tmp = std::env::temp_dir().join("velocity_test_no_tok_207");
        std::fs::create_dir_all(&tmp).ok();
        assert!(resolve_tokenizer(&None, &tmp).is_err());
        std::fs::remove_dir(&tmp).ok();
    }
    #[test]
    fn resolve_tokenizer_finds_in_model_dir() {
        let tmp = std::env::temp_dir().join("velocity_test_tok_discover_207");
        std::fs::create_dir_all(&tmp).ok();
        let tp = tmp.join("tokenizer.json");
        std::fs::write(&tp, "{}").ok();
        assert!(resolve_tokenizer(&None, &tmp).is_ok());
        std::fs::remove_file(&tp).ok();
        std::fs::remove_dir(&tmp).ok();
    }

    // Message tests
    #[test]
    fn message_roundtrip() {
        let m = Message {
            role: "user".into(),
            content: "Hello, world!".into(),
        };
        let j = serde_json::to_string(&m).unwrap();
        let p: Message = serde_json::from_str(&j).unwrap();
        assert_eq!(p.role, "user");
        assert_eq!(p.content, "Hello, world!");
    }
    #[test]
    fn message_from_json() {
        let m: Message = serde_json::from_str(r#"{"role":"assistant","content":"Hi"}"#).unwrap();
        assert_eq!(m.role, "assistant");
    }
    #[test]
    fn message_empty_content() {
        let m = Message {
            role: "system".into(),
            content: String::new(),
        };
        let j = serde_json::to_string(&m).unwrap();
        let p: Message = serde_json::from_str(&j).unwrap();
        assert_eq!(p.content, "");
    }
    #[test]
    fn message_unicode() {
        let m = Message {
            role: "user".into(),
            content: "Hello 世界 🌍".into(),
        };
        let j = serde_json::to_string(&m).unwrap();
        let p: Message = serde_json::from_str(&j).unwrap();
        assert_eq!(p.content, "Hello 世界 🌍");
    }
    #[test]
    fn message_missing_role_fails() {
        assert!(serde_json::from_str::<Message>(r#"{"content":"hello"}"#).is_err());
    }
    #[test]
    fn message_missing_content_fails() {
        assert!(serde_json::from_str::<Message>(r#"{"role":"user"}"#).is_err());
    }
    #[test]
    fn message_extra_fields_ignored() {
        let m: Message =
            serde_json::from_str(r#"{"role":"user","content":"hi","extra":42}"#).unwrap();
        assert_eq!(m.content, "hi");
    }
    #[test]
    fn message_long_content() {
        let lc = "x".repeat(100_000);
        let m = Message {
            role: "user".into(),
            content: lc.clone(),
        };
        let j = serde_json::to_string(&m).unwrap();
        let p: Message = serde_json::from_str(&j).unwrap();
        assert_eq!(p.content.len(), 100_000);
    }
    #[test]
    fn message_special_chars() {
        let m = Message {
            role: "user".into(),
            content: "line1\nline2\t\"quotes\" \\ backslash".into(),
        };
        let j = serde_json::to_string(&m).unwrap();
        let p: Message = serde_json::from_str(&j).unwrap();
        assert_eq!(p.content, m.content);
    }
    #[test]
    fn message_null_bytes() {
        let m = Message {
            role: "user".into(),
            content: "before\0after".into(),
        };
        let j = serde_json::to_string(&m).unwrap();
        let p: Message = serde_json::from_str(&j).unwrap();
        assert_eq!(p.content, "before\0after");
    }

    // GenerationReport tests
    #[test]
    fn report_serializes_minimal() {
        let r = GenerationReport {
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
        let j = serde_json::to_string(&r).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(p["mode"], "text");
        assert!(p["merkle_valid"].is_null());
    }
    #[test]
    fn report_serializes_full() {
        let r = GenerationReport {
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
        let j = serde_json::to_string(&r).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(p["site_map_hits"], 42);
    }
    #[test]
    fn report_hit_rate() {
        let r = GenerationReport {
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
        let t = r.site_map_hits + r.site_map_misses;
        assert!(((r.site_map_hits as f64 / t as f64 * 100.0) - 75.0).abs() < 0.01);
    }
    #[test]
    fn report_all_optional_null() {
        let r = GenerationReport {
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
        let j = serde_json::to_string(&r).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        for k in &[
            "merkle_valid",
            "force_terminated",
            "sandbox_executed",
            "sandbox_panicked",
            "scope_passed",
            "stored_in_site_map",
        ] {
            assert!(p[k].is_null());
        }
    }
    #[test]
    fn report_display_no_panic() {
        let r = GenerationReport {
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
        r.display();
    }
    #[test]
    fn report_display_all_fields() {
        let r = GenerationReport {
            mode: "nda".into(),
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
        r.display();
    }
    #[test]
    fn report_json_all_keys() {
        let r = GenerationReport {
            mode: "t".into(),
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
        let j = serde_json::to_string(&r).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(p.as_object().unwrap().len(), 12);
    }
    #[test]
    fn report_zero_tokens() {
        let r = GenerationReport {
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
        let j = serde_json::to_string(&r).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(p["tokens_per_second"], 0.0);
    }
    #[test]
    fn report_elapsed_zero() {
        let r = GenerationReport {
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
        let j = serde_json::to_string(&r).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(p["elapsed_ms"], 0);
    }
    #[test]
    fn report_mode_empty() {
        let r = GenerationReport {
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
        r.display();
    }

    // CloudflareAccount
    #[test]
    fn cloudflare_account_fields() {
        let a = CloudflareAccount {
            id: "id123".into(),
            token: "tok".into(),
        };
        assert_eq!(a.id, "id123");
    }

    // Cross-validation
    #[test]
    fn cross_max_tokens_check() {
        let mut g = default_generate_args();
        g.max_tokens = 0;
        assert!(validate_generate_args(&g)
            .iter()
            .any(|i| i.contains("max-tokens")));
    }
    #[test]
    fn cross_mode_check() {
        let mut g = default_generate_args();
        g.mode = "invalid".into();
        assert!(validate_generate_args(&g)
            .iter()
            .any(|i| i.contains("mode")));
    }
    #[test]
    fn cross_prompt_check() {
        let mut g = default_generate_args();
        g.prompt = None;
        g.prompt_file = None;
        assert!(validate_generate_args(&g)
            .iter()
            .any(|i| i.contains("prompt")));
    }
}
