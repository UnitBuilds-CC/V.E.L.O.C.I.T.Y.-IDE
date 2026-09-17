// commands/usage.rs — Usage statistics subcommand

use anyhow::Result;

#[derive(clap::Args)]
pub struct UsageArgs {
    /// Show per-model and per-domain breakdown
    #[arg(long)]
    pub detailed: bool,

    /// Show rate limit and quota status with projections
    #[arg(long)]
    pub rate_limit: bool,

    /// Show enhanced summary with projections and sparkline
    #[arg(long)]
    pub summary: bool,

    /// Show timeseries data (hourly or daily)
    #[arg(long, value_name = "RANGE")]
    pub timeseries: Option<String>,
}

pub fn run_usage(args: UsageArgs, json: bool) -> Result<()> {
    use velocity_ide::velocity_client::{fmt_currency, fmt_number, fmt_percent, VelocityClient};

    let client = VelocityClient::from_env()?;

    if args.rate_limit {
        let rl = client.get_rate_limit()?;
        if json {
            println!("{}", serde_json::to_string_pretty(&rl)?);
            return Ok(());
        }
        println!("\n=== Velocity Rate Limit & Quota ===\n");
        println!("  Key:              {}", rl.key_label);
        println!("  Tier:             {}", rl.tier);
        println!(
            "  Rate Limit:       {} req/min (resets in {}s)",
            rl.rate_limit.max_requests_per_minute, rl.rate_limit.resets_in_secs
        );
        println!(
            "\n  Tokens Used:      {} / {}  ({})",
            fmt_number(rl.tokens.used),
            fmt_number(rl.tokens.limit),
            fmt_percent(rl.tokens.quota_pct)
        );
        println!(
            "  Projected:        {} by end of period",
            fmt_number(rl.tokens.projected_monthly)
        );
        println!(
            "\n  Cost:             {} / {}  ({})",
            fmt_currency(rl.cost.used_usd),
            fmt_currency(rl.cost.limit_usd),
            fmt_percent(rl.cost.quota_pct)
        );
        println!(
            "  Projected:        {} by end of period",
            fmt_currency(rl.cost.projected_monthly_usd)
        );
        println!(
            "\n  Billing Reset:    in {} days\n",
            rl.billing_period.resets_in_days
        );
        return Ok(());
    }

    if args.detailed {
        let detail = client.get_usage_detailed()?;
        if json {
            println!("{}", serde_json::to_string_pretty(&detail)?);
            return Ok(());
        }
        println!("\n=== Velocity Usage Breakdown ===\n");
        println!("  Key:              {}", detail.label);
        println!("  Tier:             {}", detail.tier);
        println!("  Total Tokens:     {}", fmt_number(detail.total_tokens));
        println!(
            "  Total Cost:       {}",
            fmt_currency(detail.total_cost_usd)
        );
        println!("  Assignments:      {}\n", detail.total_assignments);
        println!("  By Model:");
        println!(
            "  {:<24} {:>10} {:>12} {:>10}",
            "Model", "Assigns", "Tokens", "Cost"
        );
        println!("  {}", "-".repeat(60));
        for m in &detail.by_model {
            println!(
                "  {:<24} {:>10} {:>12} {:>10}",
                m.model_id,
                m.assignments,
                fmt_number(m.tokens),
                fmt_currency(m.cost_usd)
            );
        }
        println!("\n  By Domain:");
        println!(
            "  {:<24} {:>10} {:>12} {:>10}",
            "Domain", "Assigns", "Tokens", "Cost"
        );
        println!("  {}", "-".repeat(60));
        for d in &detail.by_domain {
            println!(
                "  {:<24} {:>10} {:>12} {:>10}",
                d.domain,
                d.assignments,
                fmt_number(d.tokens),
                fmt_currency(d.cost_usd)
            );
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
        println!("\n=== Velocity Usage Summary (Enhanced) ===\n");
        println!("  Tier:             {}", s.tier);
        println!(
            "  Tokens:           {} / {}  ({})",
            fmt_number(s.tokens_used),
            fmt_number(s.tokens_limit),
            fmt_percent(s.token_quota_pct)
        );
        println!(
            "  Cost:             {} / {}  ({})",
            fmt_currency(s.cost_usd),
            fmt_currency(s.cost_limit_usd),
            fmt_percent(s.cost_quota_pct)
        );
        println!("  Assignments:      {}\n", s.assignments_count);
        println!("  Projections:");
        println!(
            "    Tokens:         {} by end of period",
            fmt_number(s.projected_tokens)
        );
        println!(
            "    Cost:           {} by end of period\n",
            fmt_currency(s.projected_cost_usd)
        );
        println!("  Billing Period:");
        println!("    Start:          {}", s.billing_period.start);
        println!("    End:            {}", s.billing_period.end);
        println!("    Days remaining: {}\n", s.billing_period.days_remaining);
        if !s.sparkline.is_empty() {
            println!("  Hourly Sparkline (last 24h):");
            let max_tok = s
                .sparkline
                .iter()
                .map(|b| b.tokens)
                .max()
                .unwrap_or(1)
                .max(1);
            for b in &s.sparkline {
                let bar = "#".repeat((b.tokens as f64 / max_tok as f64 * 30.0) as usize);
                println!("    {:>6}  {:>10}  {}", b.label, fmt_number(b.tokens), bar);
            }
            println!();
        }
        return Ok(());
    }

    if let Some(ref range) = args.timeseries {
        let granularity = if range.ends_with('d') {
            "daily"
        } else {
            "hourly"
        };
        let ts = client.get_timeseries(granularity, range)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&ts)?);
            return Ok(());
        }
        println!(
            "\n=== Velocity Timeseries ({}, {}) ===\n",
            ts.granularity, ts.range
        );
        let max_tok = ts
            .buckets
            .iter()
            .map(|b| b.tokens)
            .max()
            .unwrap_or(1)
            .max(1);
        for b in &ts.buckets {
            let bar = "#".repeat((b.tokens as f64 / max_tok as f64 * 30.0) as usize);
            println!(
                "  {:>6}  {:>10}  {:>10}  {}",
                b.label,
                fmt_number(b.tokens),
                fmt_currency(b.cost_usd),
                bar
            );
        }
        println!();
        return Ok(());
    }

    // Default: summary view
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
    println!("\n=== Velocity Usage Summary ===\n");
    println!("  Tier:           {}", usage.tier);
    println!(
        "  Tokens Used:    {} / {}  ({})",
        fmt_number(usage.tokens_used),
        fmt_number(usage.tokens_limit),
        fmt_percent(token_pct)
    );
    println!(
        "  Cost:           {} / {}  ({})",
        fmt_currency(usage.cost_usd),
        fmt_currency(usage.cost_limit_usd),
        fmt_percent(cost_pct)
    );
    println!("  Assignments:    {}", usage.assignments_count);
    println!(
        "  Period:         {} to {}\n",
        usage.period.start, usage.period.end
    );
    Ok(())
}
