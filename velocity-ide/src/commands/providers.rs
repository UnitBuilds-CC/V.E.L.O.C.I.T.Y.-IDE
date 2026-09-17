// commands/providers.rs — Manage provider API keys and query usage

use anyhow::Result;

#[derive(clap::Args)]
pub struct ProvidersArgs {
    /// Subcommand: list, add, remove, refresh
    #[arg(value_name = "ACTION")]
    pub action: String,

    /// Provider name (for add/remove): openai, anthropic, google, mistral, cohere, xai, github
    #[arg(long)]
    pub provider: Option<String>,

    /// API key for the provider (for add)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Optional base URL override (for add, e.g. Azure/proxy endpoints)
    #[arg(long)]
    pub base_url: Option<String>,
}

pub fn run_providers(args: ProvidersArgs, json: bool) -> Result<()> {
    use velocity_ide::provider_usage::{load_credentials, save_credentials, ProviderCredential};

    match args.action.as_str() {
        "list" => {
            let creds = load_credentials()?;
            if json {
                let masked: Vec<serde_json::Value> = creds.iter().map(|c| {
                    let mk = if c.api_key.len() > 12 { format!("{}...{}", &c.api_key[..8], &c.api_key[c.api_key.len()-4..]) } else { "****".into() };
                    serde_json::json!({ "provider": c.provider, "api_key": mk, "base_url": c.base_url })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&masked)?);
                return Ok(());
            }
            if creds.is_empty() {
                println!("\nNo provider API keys configured.");
                println!(
                    "Add one with: velocity-ide providers add --provider openai --api-key sk-...\n"
                );
                return Ok(());
            }
            println!("\n=== Configured Provider API Keys ===\n");
            println!("  {:<16} {:<20} Base URL", "Provider", "API Key");
            println!("  {}", "-".repeat(60));
            for c in &creds {
                let masked = if c.api_key.len() > 12 {
                    format!(
                        "{}...{}",
                        &c.api_key[..8],
                        &c.api_key[c.api_key.len() - 4..]
                    )
                } else {
                    "****".into()
                };
                println!(
                    "  {:<16} {:<20} {}",
                    c.provider,
                    masked,
                    c.base_url.as_deref().unwrap_or("(default)")
                );
            }
            println!();
        }
        "add" => {
            let provider = args.provider.as_deref().ok_or_else(|| {
                anyhow::anyhow!("--provider is required (e.g. --provider openai)")
            })?;
            let api_key = args
                .api_key
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--api-key is required"))?;
            if velocity_ide::provider_usage::Provider::from_str_loose(provider).is_none() {
                println!(
                    "Warning: '{}' is not a recognized provider. Adding anyway.",
                    provider
                );
            }
            let mut creds = load_credentials()?;
            creds.retain(|c| c.provider.to_lowercase() != provider.to_lowercase());
            creds.push(ProviderCredential {
                provider: provider.to_lowercase(),
                api_key: api_key.to_string(),
                base_url: args.base_url.clone(),
                model: None,
            });
            save_credentials(&creds)?;
            println!("\nProvider API key saved:");
            println!("  Provider:  {}", provider);
            println!(
                "  Key:       {}...{}",
                &api_key[..4],
                &api_key[api_key.len().saturating_sub(4)..]
            );
            if let Some(ref url) = args.base_url {
                println!("  Base URL:  {}", url);
            }
            println!("  Stored in: ~/.velocity/providers.toml\n");
            println!("Run `velocity-ide providers refresh` to query usage.\n");
        }
        "remove" => {
            let provider = args
                .provider
                .as_deref()
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
                println!("\nNo provider API keys configured.\n");
                return Ok(());
            }
            let snapshot = velocity_ide::provider_usage::query_all_providers(&creds);
            if json {
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
                return Ok(());
            }
            println!("\nQuerying {} provider(s)...\n", creds.len());
            println!(
                "  {:<16} {:<8} {:>12} {:>10}  Status",
                "Provider", "Valid", "Tokens", "Cost"
            );
            println!("  {}", "-".repeat(75));
            for p in &snapshot.providers {
                println!(
                    "  {:<16} {:<8} {:>12} {:>10}  {}",
                    p.display_name,
                    if p.key_valid { "yes" } else { "NO" },
                    velocity_ide::velocity_client::fmt_number(p.tokens_used),
                    velocity_ide::velocity_client::fmt_currency(p.cost_usd),
                    p.status
                );
            }
            println!(
                "\n  Total: {} tokens, {} across {} requests\n",
                velocity_ide::velocity_client::fmt_number(snapshot.total_tokens),
                velocity_ide::velocity_client::fmt_currency(snapshot.total_cost_usd),
                snapshot.total_requests
            );
            velocity_ide::provider_usage::write_snapshot(&snapshot)?;
            println!("Snapshot written to ~/.velocity/usage_snapshot.json");
            println!("Open the dashboard to see your combined API usage.\n");
        }
        other => anyhow::bail!(
            "Unknown action '{}'. Use: list, add, remove, refresh",
            other
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn providers_args_optional_fields() {
        let a = ProvidersArgs {
            action: "list".into(),
            provider: None,
            api_key: None,
            base_url: None,
        };
        assert!(a.provider.is_none());
        assert!(a.api_key.is_none());
        assert!(a.base_url.is_none());
    }
    #[test]
    fn providers_args_all_fields() {
        let a = ProvidersArgs {
            action: "add".into(),
            provider: Some("openai".into()),
            api_key: Some("sk-test".into()),
            base_url: Some("https://proxy.example.com".into()),
        };
        assert_eq!(a.provider.as_deref(), Some("openai"));
        assert_eq!(a.api_key.as_deref(), Some("sk-test"));
    }
}
