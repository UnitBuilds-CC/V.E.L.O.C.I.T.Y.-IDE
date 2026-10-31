//! The three right-sidebar panels Access mode names.
//!
//! Until now the header strip drew the titles "Accessibility Tree", "Contrast
//! Checker" and "ARIA Inspector" as inert labels and the body beneath them was
//! the same generic smart-sidebar content every other mode draws. A panel that
//! names a thing and shows no such thing is a claimed feature that does not
//! exist, which is what this module closes:
//!
//! * `contrast_checker` measures the palette actually on screen, through
//!   [`crate::editor::theme::palette_contrast_checks`], and paints the real
//!   ratio for each pair it is given;
//! * `a11y_tree` and `aria_inspector` have no source in this application, so
//!   they say so in as many words and name the API that would give them one.
//!   Neither invents rows to fill the space.

use crate::editor::mode_config::RightPanel;
use crate::editor::theme::{ContrastCheck, IdePalette, MIN_LARGE_TEXT_CONTRAST};
use egui;

/// The bodies this module owns, and the only description of them. A panel id
/// the mode lists that is not one of these draws nothing here on purpose: the
/// other modes' headers are titles for the generic smart sidebar below, and
/// inventing content for them is the mistake this module exists to stop.
enum Body {
    ContrastChecker,
    AccessibilityTree,
    AriaInspector,
}

fn body_for(id: &str) -> Option<Body> {
    match id {
        "contrast_checker" => Some(Body::ContrastChecker),
        "a11y_tree" => Some(Body::AccessibilityTree),
        "aria_inspector" => Some(Body::AriaInspector),
        _ => None,
    }
}

/// Whether the frame has anything of this module's to draw, so the caller can
/// skip measuring a palette it would not show.
pub fn owns_any_panel(panels: &[RightPanel]) -> bool {
    panels.iter().any(|panel| body_for(panel.id).is_some())
}

/// Draw the bodies this module owns for the panels the active mode lists, in
/// the order the mode lists them, and nothing for the ids it does not own.
///
/// The mode configs are the only source of panel ids: an id that appears here
/// and not there, or the other way round, is caught by
/// `every_access_mode_panel_earns_a_body`.
pub fn render_right_panel_bodies(
    panels: &[RightPanel],
    ui: &mut egui::Ui,
    checks: &[ContrastCheck],
    palette: IdePalette,
) {
    let mut drew = false;
    for panel in panels {
        let Some(body) = body_for(panel.id) else {
            continue;
        };
        if drew {
            ui.add_space(6.0);
            ui.separator();
        }
        ui.vertical(|ui| match body {
            Body::ContrastChecker => render_contrast_checker(ui, panel.label, checks, palette),
            Body::AccessibilityTree => render_accessibility_tree(ui, panel.label, palette),
            Body::AriaInspector => render_aria_inspector(ui, panel.label, palette),
        });
        drew = true;
    }
    if drew {
        ui.add_space(6.0);
        ui.separator();
    }
}

/// One row per colour pair, each with the ratio the pair actually measures on
/// the fill it is painted on, and a swatch of the real combination so the
/// number can be checked with the eye it is meant to describe.
fn render_contrast_checker(
    ui: &mut egui::Ui,
    title: &str,
    checks: &[ContrastCheck],
    palette: IdePalette,
) {
    ui.label(
        egui::RichText::new(title)
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    let failing = checks.iter().filter(|check| !check.passes()).count();
    ui.label(
        egui::RichText::new(format!(
            "{} colour pair(s) measured on this palette, {} below {:.1}:1.",
            checks.len(),
            failing,
            crate::editor::theme::MIN_TEXT_CONTRAST
        ))
        .size(8.0)
        .color(if failing == 0 {
            palette.text_muted
        } else {
            palette.warning
        }),
    );
    ui.add_space(3.0);
    for check in checks {
        ui.horizontal(|ui| {
            paint_pair(ui, check.ink_color, check.surface_color, 15.0);
            // The pair name is the thing a fix has to be applied to, so it
            // leads; the number is only meaningful next to the floor.
            ui.label(
                egui::RichText::new(format!("{} on {}", check.ink, check.surface))
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let tone = if check.passes() {
                    palette.text
                } else if check.ratio < MIN_LARGE_TEXT_CONTRAST {
                    palette.error
                } else {
                    palette.warning
                };
                ui.label(
                    egui::RichText::new(format!("{:.2}:1", check.ratio))
                        .size(9.0)
                        .monospace()
                        .color(tone),
                );
            });
        });
        if !check.passes() {
            let remedy = check.remedy();
            ui.horizontal(|ui| {
                ui.add_space(19.0);
                paint_pair(ui, remedy, check.surface_color, 11.0);
                ui.label(
                    egui::RichText::new(format!(
                        "needs {:.1}:1 - paint it with #{:02X}{:02X}{:02X}",
                        check.required,
                        remedy.r(),
                        remedy.g(),
                        remedy.b()
                    ))
                    .size(8.0)
                    .color(palette.text_muted),
                );
            });
        }
    }
}

/// A chip of `ink` drawn on `surface`, which is the only honest way to show a
/// contrast number: the reader can look at it instead of trusting the maths.
fn paint_pair(ui: &mut egui::Ui, ink: egui::Color32, surface: egui::Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, surface);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "A",
        egui::FontId::proportional(size * 0.62),
        ink,
    );
}

/// What the Accessibility Tree panel has to say about itself. Kept as data so
/// a test can read the claim the user reads.
const A11Y_TREE_LINES: &[&str] = &[
    "No accessible tree exists for this app to read.",
    "egui does build AccessKit nodes, but only while AccessKit is enabled for the frame and only in order to push them out to the operating system's assistive technology. Nothing in this window reads that tree back, and egui offers no way to enumerate the nodes it built, so there is no role, name or state this panel could list.",
    "What would fill it: call egui::Context::enable_accesskit for the frame, then read back the node for each region the app names with egui::Context::accesskit_node_builder(id, ..) and list the role and label it carries. The shape to hold the result already ships - mode_config::AccessibilityHub and its ScreenReaderSim, which nothing has ever fed - so the missing half is the walk, not a model.",
];

/// What the ARIA Inspector panel has to say about itself.
const ARIA_INSPECTOR_LINES: &[&str] = &[
    "No document is bound to this panel.",
    "ARIA is a contract between markup and an assistive technology, and this workspace does render markup: the browser engine in this crate answers for roles, names and form state through NativeBrowserBridge, which is what the MCP browser tools read. Every one of those calls needs a browser session id, and a session id is not something this window holds - so an inspector here would have no document to inspect.",
    "What would fill it: bind the panel to a session - a browser tab, or the session the agent is driving - and list NativeBrowserBridge::current_view and find_elements for it. The engine already answers; only the binding is missing.",
];

fn render_accessibility_tree(ui: &mut egui::Ui, title: &str, palette: IdePalette) {
    draw_no_source_panel(ui, title, "No tree to show.", A11Y_TREE_LINES, palette);
}

fn render_aria_inspector(ui: &mut egui::Ui, title: &str, palette: IdePalette) {
    draw_no_source_panel(ui, title, "Nothing bound.", ARIA_INSPECTOR_LINES, palette);
}

/// The shape both sourceless panels share: a heading, the flat statement, then
/// what would turn the statement into a list. No rows, no skeleton rows, no
/// placeholder text that a later reader might mistake for real content.
fn draw_no_source_panel(
    ui: &mut egui::Ui,
    title: &str,
    headline: &str,
    lines: &[&str],
    palette: IdePalette,
) {
    ui.label(
        egui::RichText::new(title)
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.label(
        egui::RichText::new(headline)
            .size(9.0)
            .strong()
            .color(palette.warning),
    );
    ui.add_space(2.0);
    for line in lines {
        ui.label(
            egui::RichText::new(*line)
                .size(8.0)
                .color(palette.text_muted),
        );
        ui.add_space(2.0);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::headless_render::painted_text;

    /// The panels in this module are the claim, so the tests read what the user
    /// would read out of the paint list rather than a second description of the
    /// same code.
    fn midnight_checks() -> Vec<ContrastCheck> {
        crate::editor::theme::palette_contrast_checks(&midnight())
    }

    fn access_panels() -> &'static [RightPanel] {
        crate::editor::mode_config::mode_config_for(
            crate::editor::theme::WorkspaceProfile::Accessibility,
        )
        .right_panels()
    }

    /// The palette on screen in the mode these panels belong to. Midnight is
    /// chosen because it is the default and because it fails one pair, so the
    /// same rows exercise both branches.
    fn midnight() -> IdePalette {
        crate::editor::theme::ThemeVariant::Midnight.palette()
    }

    #[test]
    fn every_access_mode_panel_earns_a_body() {
        let listed: Vec<&str> = access_panels().iter().map(|panel| panel.id).collect();
        let mut owned: Vec<&str> = listed
            .iter()
            .copied()
            .filter(|id| body_for(id).is_some())
            .collect();
        owned.sort();
        // Every id the mode lists is one this module draws, and the module owns
        // exactly three - so a new panel id appearing in mode_config without a
        // body here fails this test instead of shipping as another title with
        // nothing under it.
        assert_eq!(
            owned,
            vec!["a11y_tree", "aria_inspector", "contrast_checker"]
        );
        assert_eq!(
            listed.len(),
            owned.len(),
            "mode lists {:?}, module owns {:?}",
            listed,
            owned
        );
        assert!(owns_any_panel(access_panels()));
    }

    #[test]
    fn the_contrast_checker_paints_one_measured_row_per_pair() {
        let checks = midnight_checks();
        let text =
            painted_text(|ui| render_contrast_checker(ui, "Contrast Checker", &checks, midnight()));
        for check in &checks {
            assert!(
                text.contains(&format!("{} on {}", check.ink, check.surface)),
                "no row for {} on {}",
                check.ink,
                check.surface
            );
            assert!(
                text.contains(&format!("{:.2}:1", check.ratio)),
                "row for {} on {} does not show its measured ratio",
                check.ink,
                check.surface
            );
        }
        assert!(text.contains("colour pair(s) measured on this palette"));
    }

    #[test]
    fn a_failing_pair_is_offered_the_colour_that_clears_the_floor() {
        let checks = midnight_checks();
        let failing = checks.iter().filter(|check| !check.passes()).count();
        let text =
            painted_text(|ui| render_contrast_checker(ui, "Contrast Checker", &checks, midnight()));
        assert!(
            failing > 0,
            "Midnight is expected to fail at least one pair"
        );
        assert_eq!(
            text.matches("paint it with #").count(),
            failing,
            "every failing row offers a remedy and no passing row does"
        );
        // The summary line has to agree with the rows, or the panel is a
        // counter and not a checker.
        assert!(text.contains(&format!("{} below", failing)));
    }

    #[test]
    fn a_palette_that_clears_every_pair_says_so_without_a_remedy() {
        let palette = crate::editor::theme::ThemeVariant::HighContrast.palette();
        let checks = crate::editor::theme::palette_contrast_checks(&palette);
        assert!(checks.iter().all(|check| check.passes()));
        let text =
            painted_text(|ui| render_contrast_checker(ui, "Contrast Checker", &checks, palette));
        assert!(!text.contains("paint it with #"), "{text}");
        assert!(text.contains("0 below"));
    }

    #[test]
    fn the_sourceless_panels_state_their_state_and_name_the_api_that_would_fix_it() {
        let palette = midnight();
        let tree = painted_text(|ui| render_accessibility_tree(ui, "Accessibility Tree", palette));
        assert!(tree.contains("No accessible tree exists for this app to read."));
        assert!(tree.contains("enable_accesskit"));
        assert!(tree.contains("accesskit_node_builder"));
        // The excuse is only honest if the scaffolding it points at really
        // exists and really is unused; the test below re-checks both halves.
        assert!(tree.contains("AccessibilityHub"), "{tree}");
        let aria = painted_text(|ui| render_aria_inspector(ui, "ARIA Inspector", palette));
        assert!(aria.contains("No document is bound to this panel."));
        assert!(aria.contains("NativeBrowserBridge::current_view"));
        // Each panel answers for itself: neither borrows the other's excuse.
        assert!(!tree.contains("No document is bound"));
        assert!(!aria.contains("No accessible tree exists"));
        // And neither shows a fake row.
        for text in [&tree, &aria] {
            for banned in ["button", "heading \"", "role=", "Focus order:"] {
                assert!(
                    !text.contains(banned),
                    "{banned} in a panel that has no rows: {text}"
                );
            }
        }
    }

    #[test]
    fn the_scaffolding_the_tree_panel_blames_ships_and_still_has_no_feeder() {
        // The tree panel's excuse is that the model it needs already exists but
        // nothing has ever put rows into it. Both halves are checkable, and the
        // second is the one that goes stale: the moment some code builds a real
        // tree, the panel must stop telling the person it has no source.
        let hub = crate::editor::mode_config::AccessibilityHub::new();
        assert!(
            hub.screen_reader.elements.is_empty(),
            "a brand-new hub already carries a tree, so the panel's excuse is false"
        );

        let mut feeders: Vec<String> = Vec::new();
        for (path, src) in editor_sources() {
            let file = path.rsplit(['/', '\\']).next().unwrap_or(&path);
            // The definition owns the type; this module owns the copy that names it.
            if matches!(file, "mode_config.rs" | "access_panels.rs") {
                continue;
            }
            for needle in [
                "AccessibilityHub",
                "ScreenReaderSim",
                "build_accessibility_tree",
            ] {
                for (n, line) in src.lines().enumerate() {
                    if line.contains(needle) {
                        feeders.push(format!("{file}:{}: {needle}", n + 1));
                    }
                }
            }
        }
        assert!(
            feeders.is_empty(),
            "the tree panel says nothing feeds AccessibilityHub, but these do: {feeders:?}"
        );
    }

    /// Every `.rs` file under `src/editor`, as `(path, contents)`. Only used by
    /// the source guards above, which judge what the app wires rather than what
    /// it draws.
    fn editor_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    let name = path.to_string_lossy().into_owned();
                    if let Ok(src) = std::fs::read_to_string(&path) {
                        out.push((name, src));
                    }
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("editor");
        let mut out = Vec::new();
        walk(&root, &mut out);
        assert!(!out.is_empty(), "no sources found under {}", root.display());
        out
    }

    #[test]
    fn the_other_modes_get_no_accessibility_content() {
        // The other three modes list panels whose body has always been the
        // generic smart sidebar below the header. Drawing accessibility rows
        // under those headers would be a fresh false feature of exactly the
        // kind this module removes.
        for profile in [
            crate::editor::theme::WorkspaceProfile::Coder,
            crate::editor::theme::WorkspaceProfile::AutomationOperator,
            crate::editor::theme::WorkspaceProfile::MissionControl,
        ] {
            let panels = crate::editor::mode_config::mode_config_for(profile).right_panels();
            let text = painted_text(|ui| {
                render_right_panel_bodies(panels, ui, &midnight_checks(), midnight())
            });
            assert!(
                text.is_empty(),
                "{profile:?} gained accessibility content: {text}"
            );
            assert!(
                !owns_any_panel(panels),
                "{profile:?} claims a body it has none for"
            );
        }
    }

    #[test]
    fn the_access_mode_header_gets_all_three_bodies_in_its_own_order() {
        let text = painted_text(|ui| {
            render_right_panel_bodies(access_panels(), ui, &midnight_checks(), midnight())
        });
        for needle in [
            "Accessibility Tree",
            "Contrast Checker",
            "ARIA Inspector",
            "on bg_primary",
        ] {
            assert!(text.contains(needle), "missing {needle}");
        }
        // Order follows the mode config, so the strip and the content agree.
        let tree_at = text.find("Accessibility Tree").unwrap();
        let checker_at = text.find("Contrast Checker").unwrap();
        let aria_at = text.find("ARIA Inspector").unwrap();
        assert!(
            tree_at < checker_at && checker_at < aria_at,
            "{tree_at} {checker_at} {aria_at}"
        );

        // And each title has its own body beneath it. Without this the
        // dispatcher could route all three panels to one renderer and still
        // pass: the labels come from `panel.label`, so three identical bodies
        // would print three different titles.
        let bodies = [
            ("Accessibility Tree", "No accessible tree exists"),
            ("Contrast Checker", "colour pair(s) measured"),
            ("ARIA Inspector", "No document is bound"),
        ];
        let mut drawn: Vec<usize> = Vec::new();
        for (title, body) in bodies {
            let title_at = text.find(title).unwrap();
            let body_at = text
                .find(body)
                .unwrap_or_else(|| panic!("{title} printed no body of its own: {text}"));
            assert!(body_at > title_at, "{body} sits above its own {title} header");
            drawn.push(body_at);
        }
        assert!(
            drawn.windows(2).all(|pair| pair[0] < pair[1]),
            "bodies out of order: {drawn:?}"
        );
    }
}
