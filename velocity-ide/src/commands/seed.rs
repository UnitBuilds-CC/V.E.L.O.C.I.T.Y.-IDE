// commands/seed.rs — Seed subcommand: compile Rust sources into NDA programs

use std::path::PathBuf;

use anyhow::Result;

#[derive(clap::Args)]
pub struct SeedArgs {
    /// One or more Rust source files to compile into NDA programs.
    /// Glob patterns are supported (e.g. seeds/*.rs).
    #[arg(long, value_name = "FILE", num_args = 1..)]
    pub source: Vec<PathBuf>,

    /// Directory for the SiteMap that will receive the compiled programs.
    #[arg(long, value_name = "DIR")]
    pub site_map: PathBuf,

    /// Weight-root hash for the SiteMap (hex string).  Use 0 for a
    /// standalone seeding run not tied to specific model weights.
    #[arg(long, default_value = "0", value_name = "HEX")]
    pub weight_root: String,
}

/// Validate seed arguments.
pub fn validate_seed_args(args: &SeedArgs) -> Vec<String> {
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

pub fn run_seed(args: SeedArgs) -> Result<()> {
    use velocity_ide::compiler::rust_to_nda::seed_from_source;
    use velocity_ide::site_map::SiteMap;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn default_seed_args() -> SeedArgs {
        SeedArgs {
            source: vec![PathBuf::from("seeds/test.rs")],
            site_map: PathBuf::from("/tmp/sitemap"),
            weight_root: "0".into(),
        }
    }

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
        assert!(validate_seed_args(&args).is_empty());
    }

    #[test]
    fn seed_valid_hex_with_0x_prefix() {
        let mut args = default_seed_args();
        args.weight_root = "0xdeadbeef".into();
        assert!(validate_seed_args(&args).is_empty());
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
        assert!(validate_seed_args(&args).is_empty());
    }

    #[test]
    fn seed_multiple_source_files() {
        let mut args = default_seed_args();
        args.source = vec![
            PathBuf::from("seeds/a.rs"),
            PathBuf::from("seeds/b.rs"),
            PathBuf::from("seeds/c.rs"),
        ];
        assert!(validate_seed_args(&args).is_empty());
    }

    #[test]
    fn seed_empty_source_and_bad_hex() {
        let mut args = default_seed_args();
        args.source = vec![];
        args.weight_root = "nothex!".into();
        assert_eq!(validate_seed_args(&args).len(), 2);
    }

    #[test]
    fn seed_weight_root_0x_prefix_only() {
        let mut args = default_seed_args();
        args.weight_root = "0x".into();
        assert!(validate_seed_args(&args).is_empty());
    }

    #[test]
    fn seed_mixed_case_hex_weight_root() {
        let mut args = default_seed_args();
        args.weight_root = "DeAdBeEf".into();
        assert!(validate_seed_args(&args).is_empty());
    }

    #[test]
    fn seed_long_valid_hex() {
        let mut args = default_seed_args();
        args.weight_root = "0123456789abcdef".into();
        assert!(validate_seed_args(&args).is_empty());
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
        args.source = (0..100)
            .map(|i| PathBuf::from(format!("seeds/file_{}.rs", i)))
            .collect();
        assert!(validate_seed_args(&args).is_empty());
    }

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
        assert!(validate_seed_args(&args).is_empty());
    }

    #[test]
    fn seed_default_args_valid() {
        let args = default_seed_args();
        assert!(validate_seed_args(&args).is_empty());
    }
}
