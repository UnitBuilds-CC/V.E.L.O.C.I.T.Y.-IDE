// commands/transparency.rs — Routing transparency: model choices, cost flow

use anyhow::Result;

pub fn run_transparency(json: bool) -> Result<()> {
    use velocity_ide::velocity_client::{fmt_currency, fmt_number, VelocityClient};

    let client = VelocityClient::from_env()?;
    let t = client.get_transparency()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&t)?);
        return Ok(());
    }

    println!("\n=== Velocity Routing Transparency ===\n");
    println!("  Summary:");
    println!("    Assignments:  {}", t.summary.total_assignments);
    println!("    Errors:       {}", t.summary.total_errors);
    println!(
        "    Models:       {} available\n",
        t.summary.models_available
    );
    let total_tok = t.cost_flow.input_tokens + t.cost_flow.output_tokens;
    println!("  Cost Flow:");
    println!(
        "    Input tokens:  {} ({:.1}%)",
        fmt_number(t.cost_flow.input_tokens),
        if total_tok > 0 {
            t.cost_flow.input_tokens as f64 / total_tok as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "    Output tokens: {} ({:.1}%)",
        fmt_number(t.cost_flow.output_tokens),
        if total_tok > 0 {
            t.cost_flow.output_tokens as f64 / total_tok as f64 * 100.0
        } else {
            0.0
        }
    );
    println!("    In/Out ratio:  {:.2}", t.cost_flow.input_output_ratio);
    println!(
        "    Total cost:    {}\n",
        fmt_currency(t.cost_flow.total_cost_usd)
    );

    if !t.recent_routing_decisions.is_empty() {
        println!(
            "  Recent Routing Decisions (last {}):",
            t.recent_routing_decisions.len()
        );
        println!(
            "  {:<24} {:<20} {:<14} Rationale",
            "Domain", "Model", "Tokens"
        );
        println!("  {}", "-".repeat(90));
        for d in t.recent_routing_decisions.iter().take(20) {
            let r = d.routing_rationale.as_deref().unwrap_or("-");
            let short = if r.len() > 40 {
                format!("{}...", &r[..37])
            } else {
                r.to_string()
            };
            println!(
                "  {:<24} {:<20} {:<14} {}",
                d.domain,
                d.model_id,
                fmt_number(d.total_tokens),
                short
            );
        }
        println!();
    }
    if !t.model_selection_stats.is_empty() {
        println!("  Model Selection Stats:");
        println!(
            "  {:<24} {:>8} {:>12} {:>10} {:>10}",
            "Model", "Reqs", "Tokens", "Cost", "Avg ms"
        );
        println!("  {}", "-".repeat(70));
        for m in &t.model_selection_stats {
            println!(
                "  {:<24} {:>8} {:>12} {:>10} {:>10}",
                m.model_id,
                m.total_requests,
                fmt_number(m.total_tokens),
                fmt_currency(m.total_cost_usd),
                m.avg_duration_ms
            );
        }
        println!();
    }
    if !t.domain_distribution.is_empty() {
        println!("  Domain Distribution:");
        println!(
            "  {:<24} {:>8} {:>12} {:>10}",
            "Domain", "Reqs", "Tokens", "Cost"
        );
        println!("  {}", "-".repeat(60));
        for d in &t.domain_distribution {
            println!(
                "  {:<24} {:>8} {:>12} {:>10}",
                d.domain,
                d.requests,
                fmt_number(d.tokens),
                fmt_currency(d.cost_usd)
            );
        }
        println!();
    }
    if !t.available_models.is_empty() {
        println!("  Available Models & Pricing:");
        println!(
            "  {:<24} {:<14} {:<10} {:>12} {:>12}",
            "Model", "Provider", "Tier", "In $/Mtok", "Out $/Mtok"
        );
        println!("  {}", "-".repeat(76));
        for m in &t.available_models {
            println!(
                "  {:<24} {:<14} {:<10} {:>12.2} {:>12.2}",
                m.id, m.provider, m.tier, m.cost_input_per_mtok, m.cost_output_per_mtok
            );
        }
        println!();
    }
    Ok(())
}
