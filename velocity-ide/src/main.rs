// V.E.L.O.C.I.T.Y.-IDE — main entry point (thin dispatcher)

mod commands;

use velocity_ide::compiler;
use velocity_ide::nda;

use clap::{Parser, Subcommand};

use commands::chat::ChatArgs;
use commands::completions::CompletionsArgs;
use commands::generate::GenerateArgs;
use commands::login::LoginArgs;
use commands::providers::ProvidersArgs;
use commands::seed::SeedArgs;
use commands::usage::UsageArgs;

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

// ─── Entry point ───────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    if cli.verbose {
        log::info!("Verbose mode enabled");
        log::info!("velocity-ide v{}", env!("CARGO_PKG_VERSION"));
        let env = commands::status::inspect_environment();
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
            let issues = commands::chat::validate_chat_args(&args);
            if !issues.is_empty() {
                for issue in &issues {
                    eprintln!("Error: {}", issue);
                }
                anyhow::bail!("Invalid chat arguments ({} issue(s))", issues.len());
            }
            commands::chat::run_chat(args)
        }
        Command::Seed(args) => {
            let issues = commands::seed::validate_seed_args(&args);
            if !issues.is_empty() {
                for issue in &issues {
                    eprintln!("Error: {}", issue);
                }
                anyhow::bail!("Invalid seed arguments ({} issue(s))", issues.len());
            }
            commands::seed::run_seed(args)
        }
        Command::Generate(args) => {
            let issues = commands::generate::validate_generate_args(&args);
            if !issues.is_empty() {
                for issue in &issues {
                    eprintln!("Error: {}", issue);
                }
                anyhow::bail!("Invalid generate arguments ({} issue(s))", issues.len());
            }
            commands::generate::dispatch_generate(args, cli.json)
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
        Command::Usage(args) => commands::usage::run_usage(args, cli.json),
        Command::Login(args) => commands::login::run_login(args),
        Command::Providers(args) => commands::providers::run_providers(args, cli.json),
        Command::Status => commands::status::run_status(cli.json, cli.verbose),
        Command::Transparency => commands::transparency::run_transparency(cli.json),
        Command::Completions(args) => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            commands::completions::run_completions(args, &mut cmd)
        }
    }
}
