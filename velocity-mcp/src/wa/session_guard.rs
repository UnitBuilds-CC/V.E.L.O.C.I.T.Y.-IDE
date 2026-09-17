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
//! A second, milder family covers actions that leave the desktop alone but
//! still mutate state the operator shares with every other window: writing or
//! emptying the clipboard, and starting a program that appears on the desktop
//! and takes the foreground. Those need `VELOCITY_WA_ALLOW_SESSION_EFFECT=1`,
//! and the stronger switch implies it.
//!
//! Read-only tools (enumerate, capture, snapshot, probe, screenshot) are *not*
//! gated: they measure the desktop without changing it.

/// Opt-in switch. Deliberately an environment variable rather than a tool
/// argument: a tool argument could be flipped by the same call that wanted the
/// side effect, which would make the gate decorative.
pub const ALLOW_ENV: &str = "VELOCITY_WA_ALLOW_SESSION_CHANGE";

/// Opt-in switch for the milder family: mutations of session-wide shared state
/// that do not move the operator between desktops. Setting [`ALLOW_ENV`] also
/// satisfies it, since that is the broader grant.
pub const EFFECT_ENV: &str = "VELOCITY_WA_ALLOW_SESSION_EFFECT";

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
    refusal_with(
        action,
        effect,
        "It acts on the interactive session rather than on a target the caller named",
        ALLOW_ENV,
    )
}

fn refusal_with(action: &str, effect: &str, why: &str, env: &str) -> String {
    // Callers phrase the effect as a bare clause; normalise the sentence border
    // here so no gate site has to remember the punctuation.
    let effect = effect.trim_end_matches(['.', ';']);
    format!("{action} was refused: {effect}. {why}, so it is opt-in only. Set {env}=1 to allow it.")
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

/// Pure half of [`effect_consent_given`]: the broader switch implies the milder
/// one, and neither is implied by the other's absence.
fn effect_consent_from(effect_raw: Option<&str>, allow_raw: Option<&str>) -> bool {
    consent_from(effect_raw) || consent_from(allow_raw)
}

/// Whether this process may mutate session-wide shared state (clipboard,
/// launching programs into the operator's desktop).
///
/// Satisfied by either switch: [`ALLOW_ENV`] is the broader grant and implies
/// it, so an operator who has consented to session changes is not asked twice.
pub fn effect_consent_given() -> bool {
    effect_consent_from(
        std::env::var(EFFECT_ENV).ok().as_deref(),
        std::env::var(ALLOW_ENV).ok().as_deref(),
    )
}

/// Refusal for the [`EFFECT_ENV`] family.
pub fn effect_refusal(action: &str, effect: &str) -> String {
    refusal_with(
        action,
        effect,
        "It changes state you share with every other window rather than a target the caller named",
        EFFECT_ENV,
    )
}

/// The [`EFFECT_ENV`] gate: `Err(effect_refusal)` when consent is missing.
pub fn effect_guard(action: &str, effect: &str) -> Result<(), String> {
    if effect_consent_given() {
        Ok(())
    } else {
        Err(effect_refusal(action, effect))
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

    #[test]
    fn the_broader_switch_implies_the_milder_one() {
        let cases = [
            (None, None, false),
            (Some(""), None, false),
            (Some("0"), Some("0"), false),
            (Some("1"), None, true),
            (Some("true"), None, true),
            // Consenting to session changes covers session effects; asking twice
            // would only teach operators to set both and mean neither.
            (None, Some("1"), true),
            (Some("1"), Some("0"), true),
        ];
        for (effect, allow, expected) in cases {
            assert_eq!(
                effect_consent_from(effect, allow),
                expected,
                "EFFECT_ENV={effect:?} ALLOW_ENV={allow:?} should be {expected}"
            );
        }
        // The implication runs one way only. `consent_given()` reads ALLOW_ENV
        // and nothing else, so unlocking clipboard writes must never be enough
        // to move the operator to another desktop.
        assert!(
            effect_consent_from(Some("1"), Some("0")),
            "EFFECT_ENV=1 grants the milder family"
        );
        assert!(
            !consent_from(Some("0")),
            "EFFECT_ENV=1 with ALLOW_ENV=0 must leave the session-change gate closed"
        );
    }

    #[test]
    fn effect_refusal_names_its_own_switch() {
        let message = effect_refusal("clipboard write", "it replaces what you have copied");
        assert!(message.starts_with("clipboard write was refused"));
        assert!(message.contains(EFFECT_ENV), "{message}");
        assert!(!message.contains(ALLOW_ENV), "{message}");
        assert!(!message.contains(".."), "run-on refusal: {message}");
        // Distinct wording from the session-change family: the two switches
        // differ, so an operator must be able to tell which one a message asks
        // for just by reading it.
        assert_ne!(
            message,
            refusal("clipboard write", "it replaces what you have copied")
        );
    }

    #[test]
    fn effect_guard_blocks_without_consent() {
        if effect_consent_given() {
            return; // opted in for the whole process; nothing to assert here
        }
        let err = effect_guard("process launch", "it puts a window on your desktop")
            .expect_err("unconsented session effect must be blocked");
        assert!(err.contains(EFFECT_ENV), "{err}");
    }
}
