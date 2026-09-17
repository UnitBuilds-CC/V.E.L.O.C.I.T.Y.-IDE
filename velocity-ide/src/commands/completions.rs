// commands/completions.rs — Shell completion generation

use anyhow::Result;

#[derive(clap::Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_name = "SHELL")]
    pub shell: String,
}

pub fn run_completions(args: CompletionsArgs, cmd: &mut clap::Command) -> Result<()> {
    use clap_complete::{generate, Shell};

    let shell = match args.shell.to_lowercase().as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "powershell" | "pwsh" => Shell::PowerShell,
        other => anyhow::bail!(
            "Unknown shell: '{}'. Supported: bash, zsh, fish, powershell",
            other
        ),
    };

    generate(shell, cmd, "velocity_ide", &mut std::io::stdout());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cmd() -> clap::Command {
        // Minimal command for testing completions
        clap::Command::new("velocity_ide")
    }

    #[test]
    fn completions_unknown_shell_errors() {
        let args = CompletionsArgs {
            shell: "unknown_shell_xyz".into(),
        };
        let mut cmd = test_cmd();
        let result = run_completions(args, &mut cmd);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unknown_shell_xyz"));
    }

    #[test]
    fn completions_shell_name_case_insensitive() {
        let args = CompletionsArgs {
            shell: "Bash".into(),
        };
        let mut cmd = test_cmd();
        assert!(run_completions(args, &mut cmd).is_ok());
    }

    #[test]
    fn completions_pwsh_alias() {
        let args = CompletionsArgs {
            shell: "pwsh".into(),
        };
        let mut cmd = test_cmd();
        assert!(run_completions(args, &mut cmd).is_ok());
    }

    #[test]
    fn completions_powershell_full() {
        let args = CompletionsArgs {
            shell: "PowerShell".into(),
        };
        let mut cmd = test_cmd();
        assert!(run_completions(args, &mut cmd).is_ok());
    }
}
