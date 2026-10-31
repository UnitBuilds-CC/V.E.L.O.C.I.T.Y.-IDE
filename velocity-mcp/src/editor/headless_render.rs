//! A test-only seam for proving what a renderer actually paints.
//!
//! Nothing in this crate drives an egui frame in production tests, so a panel's
//! behaviour used to be provable only by reading its source and hoping the
//! strings matched the claim. This closes that gap for one renderer at a time:
//! it runs a head-less frame and returns every character that frame put in the
//! paint list, which is what the user reads. A panel that builds the right
//! `String` but never labels it with it fails here and passes a source grep.

use egui;

/// Run `draw` for one frame at sidebar width and return the painted text.
///
/// Snippets are joined with newlines in paint order, so a test can check both
/// presence and ordering without knowing anything about galleys.
pub(crate) fn painted_text(draw: impl FnMut(&mut egui::Ui)) -> String {
    let ctx = egui::Context::default();
    let mut input = egui::RawInput::default();
    input.screen_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(300.0, 3000.0),
    ));
    let out = ctx.run_ui(input, draw);
    out.shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The seam itself is a claim, so it is tested: a string the renderer never
/// labels with must not appear, and text written straight to the painter must.
#[test]
fn the_probe_reports_only_what_reached_the_paint_list() {
    let text = painted_text(|ui| {
        ui.label("drawn by a label");
        let hidden = if ui.available_width() < 0.0 {
            "never drawn"
        } else {
            "also drawn"
        };
        ui.label(hidden);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Z",
            egui::FontId::proportional(9.0),
            egui::Color32::WHITE,
        );
    });
    assert!(text.contains("drawn by a label"), "{text}");
    assert!(text.contains("also drawn"), "{text}");
    assert!(!text.contains("never drawn"), "{text}");
    assert!(
        text.contains('Z'),
        "direct painter text is not collected: {text}"
    );
}

#[test]
fn an_empty_frame_paints_no_text() {
    assert_eq!(painted_text(|_ui| {}), "");
}
