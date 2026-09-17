// commands/status.rs — Quick health check and router status

use anyhow::Result;
use serde::Serialize;

use velocity_ide::velocity_client;

/// Snapshot of the CLI environment.
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

pub fn inspect_environment() -> CliEnvironment {
    let url_set = std::env::var("VELOCITY_BASE_URL").is_ok();
    let key_set = std::env::var("VELOCITY_API_KEY").is_ok();
    let config_file_exists = velocity_client::dirs_next()
        .map(|h| h.join(".velocity").join("config.toml").exists())
        .unwrap_or(false);
    let provider_count = velocity_ide::provider_usage::load_credentials()
        .map(|c| c.len())
        .unwrap_or(0);
    let velocity_configured = url_set && key_set || config_file_exists;
    let credential_boundary_active =
        std::env::var("VELOCITY_API_KEY").is_err() && config_file_exists;
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

/// CLI diagnostic snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct CliDiagnostics {
    pub environment: CliEnvironment,
    pub velocity_config: Option<velocity_client::ConnectionInfo>,
    pub available_subcommands: Vec<&'static str>,
}

pub fn cli_diagnostics() -> CliDiagnostics {
    let env = inspect_environment();
    let velocity_config = if env.velocity_configured {
        velocity_client::VelocityConfig::load()
            .ok()
            .map(|c| c.connection_info())
    } else {
        None
    };
    CliDiagnostics {
        environment: env,
        velocity_config,
        available_subcommands: vec![
            "generate",
            "benchmark",
            "seed",
            "chat",
            "usage",
            "login",
            "providers",
            "status",
            "transparency",
            "completions",
        ],
    }
}

pub fn run_status(json: bool, verbose: bool) -> Result<()> {
    use velocity_client::VelocityClient;

    if verbose {
        let diag = cli_diagnostics();
        if json {
            println!("{}", serde_json::to_string_pretty(&diag)?);
            return Ok(());
        }
        println!("\n=== CLI Environment ===\n");
        println!(
            "  Velocity configured:  {}",
            diag.environment.velocity_configured
        );
        println!(
            "  Config file exists:   {}",
            diag.environment.config_file_exists
        );
        println!(
            "  Provider keys:        {}",
            diag.environment.provider_count
        );
        println!(
            "  Credential boundary:  {}",
            diag.environment.credential_boundary_active
        );
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
            println!("\n  Environment Issues:");
            for issue in &diag.environment.validation_issues {
                println!("    - {}", issue);
            }
        }
        println!();
    }

    let client = VelocityClient::from_env()?;
    if json {
        let out = serde_json::json!({ "health": client.health().ok(), "usage": client.get_usage().ok(), "rate_limit": client.get_rate_limit().ok() });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    match client.health() {
        Ok(h) => {
            println!("\n=== Velocity Router Status ===\n");
            println!("  Status:    {}", h.status);
            println!("  Version:   {}", h.version);
            println!("  Models:    {} available", h.models_available);
        }
        Err(e) => {
            println!("\n=== Velocity Router Status ===\n");
            println!("  Status:    UNREACHABLE");
            println!("  Error:     {}", e);
            println!("\n  Check that the router is running and VELOCITY_BASE_URL is correct.\n");
            return Ok(());
        }
    }
    if let Ok(u) = client.get_usage() {
        let tp = if u.tokens_limit > 0 {
            (u.tokens_used as f64 / u.tokens_limit as f64) * 100.0
        } else {
            0.0
        };
        let cp = if u.cost_limit_usd > 0.0 {
            (u.cost_usd / u.cost_limit_usd) * 100.0
        } else {
            0.0
        };
        println!("\n  Tier:      {}", u.tier);
        println!(
            "  Tokens:    {} / {}  ({:.1}%)",
            velocity_client::fmt_number(u.tokens_used),
            velocity_client::fmt_number(u.tokens_limit),
            tp
        );
        println!(
            "  Cost:      {} / {}  ({:.1}%)",
            velocity_client::fmt_currency(u.cost_usd),
            velocity_client::fmt_currency(u.cost_limit_usd),
            cp
        );
        println!("  Assigns:   {}", u.assignments_count);
    } else {
        println!("  (Could not fetch usage — key may not be configured)");
    }
    if let Ok(rl) = client.get_rate_limit() {
        println!(
            "\n  Rate:      {} req/min (resets in {}s)",
            rl.rate_limit.max_requests_per_minute, rl.rate_limit.resets_in_secs
        );
        println!(
            "  Billing:   resets in {} days",
            rl.billing_period.resets_in_days
        );
    }
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let j = serde_json::to_string(&env).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(p["velocity_configured"], true);
        assert_eq!(p["provider_count"], 3);
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
            validation_issues: vec!["not configured".into()],
        };
        let j = serde_json::to_string(&env).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(p["validation_issues"].as_array().unwrap().len(), 1);
    }
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
        let c = env.clone();
        assert!(c.velocity_configured);
        assert_eq!(c.provider_count, 5);
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
        assert!(format!("{:?}", env).contains("CliEnvironment"));
    }
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
        let j = serde_json::to_string(&env).unwrap();
        let m: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(m.as_object().unwrap().len(), 7);
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
        let mut c = env.clone();
        c.provider_count = 99;
        c.validation_issues.push("b".into());
        assert_eq!(env.provider_count, 3);
        assert_eq!(env.validation_issues.len(), 1);
    }
    #[test]
    fn cli_environment_json_field_types() {
        let env = CliEnvironment {
            velocity_configured: true,
            velocity_url_set: false,
            velocity_key_set: true,
            config_file_exists: true,
            provider_count: 5,
            credential_boundary_active: false,
            validation_issues: vec!["i1".into(), "i2".into()],
        };
        let j = serde_json::to_string(&env).unwrap();
        let m: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(m["velocity_configured"], true);
        assert_eq!(m["provider_count"], 5);
        assert_eq!(m["validation_issues"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn inspect_environment_returns_struct() {
        let env = inspect_environment();
        let _ = env.velocity_configured;
    }
    #[test]
    fn inspect_environment_serializes() {
        let j = serde_json::to_string(&inspect_environment()).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert!(p["velocity_configured"].is_boolean());
    }

    #[test]
    fn cli_diagnostics_has_all_subcommands() {
        let d = cli_diagnostics();
        assert_eq!(d.available_subcommands.len(), 10);
        assert!(d.available_subcommands.contains(&"generate"));
    }
    #[test]
    fn cli_diagnostics_serializes() {
        let d = cli_diagnostics();
        let j = serde_json::to_string(&d).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert!(p["environment"].is_object());
        assert_eq!(p["available_subcommands"].as_array().unwrap().len(), 10);
    }
    #[test]
    fn cli_diagnostics_clone() {
        let d = cli_diagnostics();
        let c = d.clone();
        assert_eq!(c.available_subcommands.len(), d.available_subcommands.len());
    }
    #[test]
    fn cli_diagnostics_debug_format() {
        assert!(format!("{:?}", cli_diagnostics()).contains("CliDiagnostics"));
    }
    #[test]
    fn cli_diagnostics_subcommands_order() {
        let d = cli_diagnostics();
        assert_eq!(d.available_subcommands[0], "generate");
        assert_eq!(d.available_subcommands[9], "completions");
    }
    #[test]
    fn cli_diagnostics_json_has_3_keys() {
        let d = CliDiagnostics {
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
        let j = serde_json::to_string(&d).unwrap();
        let m: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(m.as_object().unwrap().len(), 3);
    }
    #[test]
    fn cli_diagnostics_with_config_none() {
        let d = CliDiagnostics {
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
            available_subcommands: vec![],
        };
        let j = serde_json::to_string(&d).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert!(p["velocity_config"].is_null());
    }
    #[test]
    fn cli_diagnostics_with_config_some() {
        let d = CliDiagnostics {
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
        let j = serde_json::to_string(&d).unwrap();
        let p: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert!(p["velocity_config"].is_object());
        assert_eq!(
            p["velocity_config"]["base_url"],
            "https://router.example.com"
        );
    }
}
