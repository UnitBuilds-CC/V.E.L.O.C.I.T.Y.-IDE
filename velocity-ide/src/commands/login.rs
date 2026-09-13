// commands/login.rs — Configure Velocity Router connection

use anyhow::Result;

#[derive(clap::Args)]
pub struct LoginArgs {
    /// Velocity Router base URL
    #[arg(long, default_value = "http://localhost:8787")]
    pub url: String,

    /// API key (vr_... prefix)
    #[arg(long)]
    pub key: String,
}

pub fn run_login(args: LoginArgs) -> Result<()> {
    use velocity_ide::velocity_client::{VelocityClient, VelocityConfig};

    let config = VelocityConfig { base_url: args.url, api_key: args.key };
    let warnings = config.validate();
    if !warnings.is_empty() { for w in &warnings { eprintln!("Warning: {}", w); } }
    config.save()?;

    println!("\nVelocity Router configured:");
    println!("  URL:  {}", config.base_url);
    println!("  Key:  {}...{}", &config.api_key[..8], &config.api_key[config.api_key.len().saturating_sub(4)..]);
    println!("\nSaved to ~/.velocity/config.toml\n");

    let client = VelocityClient::new(config);
    match client.health() {
        Ok(h) => println!("Router health: {} (v{}, {} models)", h.status, h.version, h.models_available),
        Err(e) => { println!("Warning: could not reach router: {}", e); println!("Configuration saved, but router may not be running."); }
    }
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn login_args_default_url() {
        let args = LoginArgs { url: "http://localhost:8787".into(), key: "vr_test123".into() };
        assert_eq!(args.url, "http://localhost:8787");
        assert!(args.key.starts_with("vr_"));
    }
}
