// commands/mod.rs — Subcommand modules for velocity-ide CLI

pub mod chat;
pub mod completions;
pub mod generate;
pub mod login;
pub mod providers;
pub mod seed;
pub mod status;
pub mod transparency;
pub mod usage;

/// Redact a secret for terminal display without ever panicking: shows the
/// first 4 and last 4 characters separated by `...` for long values, or an
/// all-asterisk mask for short ones. Operates on `chars()` (not byte slices),
/// so a multi-byte UTF-8 key cannot hit a char-boundary panic, and no value
/// is sliced out of bounds.
pub(crate) fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 12 {
        "*".repeat(chars.len().max(1))
    } else {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}...{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::mask_secret;

    #[test]
    fn short_keys_never_panic() {
        // These used to slice `&key[..8]` / `&key[..4]` and crash the CLI.
        assert_eq!(mask_secret(""), "*");
        assert_eq!(mask_secret("ab"), "**");
        assert_eq!(mask_secret("abc"), "***");
    }

    #[test]
    fn multibyte_short_key_does_not_panic_on_boundary() {
        assert_eq!(mask_secret("héllo"), "*****");
    }

    #[test]
    fn long_keys_show_head_and_tail() {
        assert_eq!(mask_secret("sk-1234567890abcdef"), "sk-1...cdef");
    }
}
