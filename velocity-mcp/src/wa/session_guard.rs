//! Consent gate for Windows-Automation actions that drive the *interactive
//! session* instead of a target the caller names.
//!
//! Bug #40. Several `wa_*` tools are indistinguishable from the user acting:
//!
//! - `wa_virtual_desktop_switch` synthesises Ctrl+Win+Left/Right through
//!   `keybd_event`. Windows treats that exactly as a real keystroke, and at the
//!   rightmost desktop the same hotkey *creates* a new desktop and moves the
//!   user onto it.
//! - `wa_input_sequence` injects arbitrary virtual-key codes and mouse events
//!   with no target-window parameter at all, so they land in whatever the user
//!   happens to have focused.
//! - `wa_tray_click` / `wa_notifications_dismiss` invoke whichever tray icon or
//!   toast button matches a label, not a handle the caller resolved.
//! - `bring_to_front` asks the shell for the foreground, which also moves the
//!   user to whichever desktop owns that window.
//!
//! A test sweep or a confused agent can therefore hijack the keyboard of
//! whoever is sitting at the machine. Everything in this family is refused
//! unless the operator has opted in with `VELOCITY_WA_ALLOW_SESSION_CHANGE=1`.
//!
//! Read-only tools (enumerate, capture, snapshot, probe, screenshot) are *not*
//! gated: they measure the desktop without changing it.

/// Opt-in switch. Deliberately an environment variable rather than a tool
/// argument: a tool argument could be flipped by the same call that wanted the
/// side effect, which would make the gate decorative.
pub const ALLOW_ENV: &str = "VELOCITY_WA_ALLOW_SESSION_CHANGE";

/// Truthy values for [`ALLOW_ENV`]. Anything else - including an empty or
/// whitespace value - means "not consented", so `VAR=` does not silently enable
/// the family.
pub fn consent_from(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// Whether this process may drive the interactive session.
pub fn consent_given() -> bool {
    consent_from(std::env::var(ALLOW_ENV).ok().as_deref())
}

/// The refusal every gated path returns before it touches anything.
///
/// `action` names the operation, `effect` says concretely what it would have
/// done to the person at the keyboard. The message never claims success, and it
/// names the switch rather than leaving the caller to guess at a retry.
pub fn refusal(action: &str, effect: &str) -> String {
    // Callers phrase the effect as a bare clause; normalise the sentence border
    // here so no gate site has to remember the punctuation.
    let effect = effect.trim_end_matches(['.', ';']);
    format!(
        "{action} was refused: {effect}. It acts on the interactive session rather than on a target \
         the caller named, so it is opt-in only. Set {ALLOW_ENV}=1 to allow it."
    )
}

/// The gate itself: `Err(refusal)` when consent is missing.
///
/// Used by the paths that already return `Result`; the struct-returning
/// managers build their own failure value from [`refusal`] instead.
pub fn guard(action: &str, effect: &str) -> Result<(), String> {
    if consent_given() {
        Ok(())
    } else {
        Err(refusal(action, effect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_values_consent() {
        for allowed in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(consent_from(Some(allowed)), "{allowed} must enable");
        }
        for denied in [
            None,
            Some(""),
            Some(" "),
            Some("0"),
            Some("false"),
            Some("no"),
        ] {
            assert!(!consent_from(denied), "{denied:?} must not enable");
        }
    }

    #[test]
    fn default_is_deny_when_the_operator_has_said_nothing() {
        if std::env::var(ALLOW_ENV).is_ok() {
            return; // opted in for the whole process; nothing to assert here
        }
        assert!(!consent_given());
    }

    #[test]
    fn refusal_names_the_switch_and_never_claims_success() {
        let message = refusal(
            "virtual desktop switch",
            "it sends the Ctrl+Win+Arrow shell hotkey to the desktop you are looking at",
        );
        assert!(message.starts_with("virtual desktop switch was refused"));
        assert!(message.contains(ALLOW_ENV), "{message}");
        assert!(!message.contains("succeeded"), "{message}");
        assert!(!message.contains("\"success\":true"), "{message}");
        // The effect clause and the explanation are two sentences, whichever
        // way the caller punctuated its effect.
        assert!(
            message.contains("looking at. It acts"),
            "run-on refusal: {message}"
        );
        let repunctuated = refusal("input sequence", "it injects keystrokes.");
        assert!(!repunctuated.contains(".."), "doubled stop: {repunctuated}");
    }

    #[test]
    fn guard_blocks_without_consent() {
        if std::env::var(ALLOW_ENV).is_ok() {
            return;
        }
        let err = guard(
            "input sequence",
            "it injects keystrokes into the focused window",
        )
        .expect_err("unconsented session change must be blocked");
        assert!(err.contains(ALLOW_ENV), "{err}");
    }

    #[test]
    fn guard_opens_for_any_consented_value() {
        // The gate has to be openable, or "opt-in" would be a lie. Tested
        // through the pure half rather than mutating the process environment,
        // which would race with the other tests in this binary.
        assert!(consent_from(Some("1")));
        assert!(consent_from(Some("on")));
        assert!(!consent_from(Some("")), "an empty value is not consent");
    }
}
