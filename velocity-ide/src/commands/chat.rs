// commands/chat.rs — Interactive chat subcommand

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use super::generate::{call_kimi, load_accounts, Message};

#[derive(clap::Args)]
pub struct ChatArgs {
    /// Directory containing NDA weight files (.nda / .bin)
    #[arg(long, value_name = "DIR")]
    pub model: Option<PathBuf>,

    /// Path to tokenizer.json (defaults to <model-dir>/../tokenizer.json)
    #[arg(long, value_name = "FILE")]
    pub tokenizer: Option<PathBuf>,

    /// Maximum number of new tokens to generate per response
    #[arg(long, default_value = "512", value_name = "N")]
    pub max_tokens: usize,

    /// Sampling temperature (0 = greedy, 0.7 = default, >1 = creative)
    #[arg(long, default_value = "0.7", value_name = "T")]
    pub temperature: f32,

    /// Top-p nucleus sampling threshold
    #[arg(long, default_value = "0.9", value_name = "P")]
    pub top_p: f32,

    /// Model architecture preset: 'bitnet3b' or 'qwen05'
    #[arg(long, default_value = "bitnet3b", value_name = "ARCH")]
    pub arch: String,
}

/// Validate chat arguments.
pub fn validate_chat_args(args: &ChatArgs) -> Vec<String> {
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
        other => issues.push(format!(
            "Unknown --arch '{}'. Use 'bitnet3b' or 'qwen05'.",
            other
        )),
    }
    issues
}

pub fn run_chat(_args: ChatArgs) -> Result<()> {
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
                let token_estimate = (response.len() as u64) / 4;
                history.push(Message {
                    role: "assistant".to_string(),
                    content: response.clone(),
                });
                println!();
                match velocity_ide::velocity_client::VelocityClient::from_env() {
                    Ok(client) => match client.get_usage() {
                        Ok(u) => {
                            println!("-> Completed in {:.1}s | {} tokens est. | tier: {} | total: {} / {}",
                                    elapsed,
                                    token_estimate,
                                    u.tier,
                                    velocity_ide::velocity_client::fmt_number(u.tokens_used),
                                    velocity_ide::velocity_client::fmt_number(u.tokens_limit));
                        }
                        Err(_) => {
                            println!(
                                "-> Completed in {:.1}s | {} tokens est.",
                                elapsed, token_estimate
                            );
                        }
                    },
                    Err(_) => {
                        println!(
                            "-> Completed in {:.1}s | {} tokens est.",
                            elapsed, token_estimate
                        );
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn chat_valid_defaults() {
        assert!(validate_chat_args(&default_chat_args()).is_empty());
    }

    #[test]
    fn chat_zero_max_tokens() {
        let mut args = default_chat_args();
        args.max_tokens = 0;
        assert_eq!(validate_chat_args(&args).len(), 1);
    }

    #[test]
    fn chat_negative_temperature() {
        let mut args = default_chat_args();
        args.temperature = -0.5;
        let issues = validate_chat_args(&args);
        assert!(issues[0].contains("temperature"));
    }

    #[test]
    fn chat_temperature_zero_is_valid() {
        let mut args = default_chat_args();
        args.temperature = 0.0;
        assert!(validate_chat_args(&args).is_empty());
    }

    #[test]
    fn chat_top_p_out_of_range() {
        let mut args = default_chat_args();
        args.top_p = -0.1;
        assert_eq!(validate_chat_args(&args).len(), 1);
        args.top_p = 1.5;
        assert_eq!(validate_chat_args(&args).len(), 1);
    }

    #[test]
    fn chat_unknown_arch() {
        let mut args = default_chat_args();
        args.arch = "gpt4".into();
        let issues = validate_chat_args(&args);
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
        assert!(validate_chat_args(&args).len() >= 4);
    }

    #[test]
    fn chat_very_high_temperature_not_rejected() {
        let mut args = default_chat_args();
        args.temperature = 100.0;
        let temp_issues: Vec<_> = validate_chat_args(&args)
            .into_iter()
            .filter(|i| i.contains("temperature"))
            .collect();
        assert!(temp_issues.is_empty());
    }

    #[test]
    fn chat_infinity_temperature_not_rejected() {
        let mut args = default_chat_args();
        args.temperature = f32::INFINITY;
        let temp_issues: Vec<_> = validate_chat_args(&args)
            .into_iter()
            .filter(|i| i.contains("temperature"))
            .collect();
        assert!(temp_issues.is_empty());
    }

    #[test]
    fn chat_top_p_exactly_zero() {
        let mut args = default_chat_args();
        args.top_p = 0.0;
        assert!(validate_chat_args(&args).is_empty());
    }

    #[test]
    fn chat_top_p_exactly_one() {
        let mut args = default_chat_args();
        args.top_p = 1.0;
        assert!(validate_chat_args(&args).is_empty());
    }

    #[test]
    fn chat_nan_temperature_passes() {
        let mut args = default_chat_args();
        args.temperature = f32::NAN;
        let temp_issues: Vec<_> = validate_chat_args(&args)
            .into_iter()
            .filter(|i| i.contains("temperature"))
            .collect();
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
        assert!(validate_chat_args(&args).iter().any(|i| i.contains("temperature")));
    }

    #[test]
    fn chat_temperature_exactly_zero_valid() {
        let mut args = default_chat_args();
        args.temperature = 0.0;
        assert!(validate_chat_args(&args).iter().all(|i| !i.contains("temperature")));
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
        assert!(validate_chat_args(&args).len() >= 4);
    }
}
