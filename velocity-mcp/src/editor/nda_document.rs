//! NDA Document editor tab — author, convert, view, and inspect portable NDA1
//! documents with self-contained provenance/history.
//!
//! A document is stored on disk either *portable* (the 48-byte-header NDA1
//! layout, openable in any browser) or *sealed* (that layout wrapped in the
//! workspace AES-256-GCM envelope). Origin and every author who has touched the
//! file ride inside the document as semantic triples, so history travels with
//! the file and is visible in any reference viewer.

use std::path::{Path, PathBuf};

use eframe::egui;
use velocity_browser::nda_portable::{
    CommandKind, DisplayCommand, NdaPortableDoc,
};

use crate::editor::theme::IdePalette;
use crate::editor::toast::{Toast, ToastQueue};

/// Which sub-view of the document is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NdaSubView {
    Canvas,
    Triples,
    History,
    Bytes,
}

/// What a `.nda` file on disk turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadedKind {
    /// Portable NDA1 document (Flags == 0).
    Portable,
    /// Sealed envelope that decrypted to a portable document.
    Sealed,
    /// An opaque NDA envelope / state file we don't render as a document.
    Opaque(String),
}

/// A resolved author identity for a revision commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    pub name: String,
    pub email: String,
    /// Which tier resolved the identity: `configured`, `git`, or `os`.
    pub source: String,
}

fn identity_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".velocity").join("nda_identity.json")
}

/// Resolve the author identity: configured workspace identity → git config →
/// OS username. The tier that produced the result is recorded in `source`.
pub fn resolve_author(workspace_root: &Path) -> Author {
    // Tier 1: configured workspace identity.
    if let Ok(raw) = std::fs::read_to_string(identity_path(workspace_root)) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("").trim().to_string();
            let email = v.get("email").and_then(|e| e.as_str()).unwrap_or("").trim().to_string();
            if !name.is_empty() {
                return Author { name, email, source: "configured".to_string() };
            }
        }
    }
    // Tier 2: git identity.
    let git_name = run_git(workspace_root, "user.name");
    let git_email = run_git(workspace_root, "user.email");
    if !git_name.is_empty() {
        return Author { name: git_name, email: git_email, source: "git".to_string() };
    }
    // Tier 3: OS username.
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "anonymous".to_string());
    Author { name: user, email: String::new(), source: "os".to_string() }
}

/// Persist the configured workspace identity (tier 1).
pub fn set_identity(workspace_root: &Path, name: &str, email: &str) -> std::io::Result<()> {
    let path = identity_path(workspace_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let v = serde_json::json!({ "name": name, "email": email });
    std::fs::write(path, serde_json::to_string_pretty(&v)?)
}

fn run_git(workspace_root: &Path, key: &str) -> String {
    std::process::Command::new("git")
        .arg("config")
        .arg("--get")
        .arg(key)
        .current_dir(workspace_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Current UTC time as an RFC3339 string (no external date crate).
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339_from_unix(secs)
}

fn rfc3339_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Stable per-document subkey label for sealing.
fn seal_label(path: Option<&Path>) -> Vec<u8> {
    let name = path
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled.nda".to_string());
    format!("nda-doc:{name}").into_bytes()
}

/// Read a `.nda` file, distinguishing portable / sealed / opaque.
pub fn load_from_disk(workspace_root: &Path, path: &Path) -> Result<(NdaPortableDoc, LoadedKind), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read failed: {e}"))?;
    if bytes.len() < 8 || &bytes[0..4] != b"NDA1" {
        return Err("not an NDA file (bad magic)".to_string());
    }
    let flags = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if flags == 0 {
        let doc = NdaPortableDoc::from_portable_bytes(&bytes).map_err(|e| e.to_string())?;
        return Ok((doc, LoadedKind::Portable));
    }
    if flags & (velocity_browser::nda::NDA_FLAG_ENCRYPTED | velocity_browser::nda::NDA_FLAG_RAW) != 0 {
        let plain = crate::agent::crypto::open(workspace_root, &seal_label(Some(path)), &bytes);
        if plain.is_empty() {
            return Err("sealed NDA could not be opened (wrong workspace key?)".to_string());
        }
        match NdaPortableDoc::from_portable_bytes(&plain) {
            Ok(doc) => return Ok((doc, LoadedKind::Sealed)),
            Err(_) => return Ok((NdaPortableDoc::new(), LoadedKind::Opaque("sealed envelope (not a document)".to_string()))),
        }
    }
    Ok((NdaPortableDoc::new(), LoadedKind::Opaque(format!("unknown NDA flags {flags:#x}"))))
}

/// Write a document to disk honoring the per-document seal toggle. Returns the
/// bytes written.
pub fn save_to_disk(workspace_root: &Path, path: &Path, doc: &NdaPortableDoc, sealed: bool) -> Result<Vec<u8>, String> {
    let portable = doc.to_portable_bytes();
    let out = if sealed {
        crate::agent::crypto::seal(workspace_root, &seal_label(Some(path)), &portable)
            .ok_or_else(|| "no key material available to seal (falling back impossible)".to_string())?
    } else {
        portable
    };
    std::fs::write(path, &out).map_err(|e| format!("write failed: {e}"))?;
    Ok(out)
}

/// The per-tab editor state for an NDA document.
pub struct NdaDocumentView {
    pub path: Option<PathBuf>,
    pub doc: NdaPortableDoc,
    pub kind: LoadedKind,
    pub sealed: bool,
    pub dirty: bool,
    pub sub: NdaSubView,
    // Authoring inputs.
    title_input: String,
    ts: String,
    tp: String,
    to: String,
    cmd_text: String,
    cmd_x: String,
    cmd_y: String,
    commit_msg: String,
    identity_name: String,
    identity_email: String,
    identity_loaded: bool,
    /// Decoded image textures for DrawImage commands, keyed by command index;
    /// the stored content string detects staleness when the doc edits.
    image_textures: std::collections::HashMap<usize, (String, egui::TextureId)>,
    last_error: Option<String>,
}

impl Default for NdaDocumentView {
    fn default() -> Self {
        Self::new()
    }
}

impl NdaDocumentView {
    pub fn new() -> Self {
        Self {
            path: None,
            doc: NdaPortableDoc::new(),
            kind: LoadedKind::Portable,
            sealed: false,
            dirty: false,
            sub: NdaSubView::Canvas,
            title_input: String::new(),
            ts: String::new(),
            tp: String::new(),
            to: String::new(),
            cmd_text: String::new(),
            cmd_x: "16".to_string(),
            cmd_y: "24".to_string(),
            commit_msg: String::new(),
            identity_name: String::new(),
            identity_email: String::new(),
            identity_loaded: false,
            image_textures: std::collections::HashMap::new(),
            last_error: None,
        }
    }

    /// Open an existing `.nda` file into this view.
    pub fn open(&mut self, workspace_root: &Path, path: &Path) {
        match load_from_disk(workspace_root, path) {
            Ok((doc, kind)) => {
                self.sealed = matches!(kind, LoadedKind::Sealed);
                self.title_input = doc.title().unwrap_or("").to_string();
                self.doc = doc;
                self.kind = kind;
                self.path = Some(path.to_path_buf());
                self.dirty = false;
                self.image_textures.clear();
                self.last_error = None;
            }
            Err(e) => {
                self.kind = LoadedKind::Opaque(e.clone());
                self.path = Some(path.to_path_buf());
                self.last_error = Some(e);
            }
        }
    }

    fn set_error(&mut self, msg: Option<String>) {
        self.last_error = msg;
    }

    /// The main UI. Returns an optional path the host should open in a browser
    /// (after an HTML export).
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        workspace_root: &Path,
        toasts: &mut ToastQueue,
        palette: IdePalette,
    ) -> Option<PathBuf> {
        let mut open_in_browser: Option<PathBuf> = None;

        if let LoadedKind::Opaque(reason) = &self.kind {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("◇").size(28.0).color(palette.accent.gamma_multiply(0.7)));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Opaque NDA envelope").size(13.0).strong().color(palette.text));
                ui.label(egui::RichText::new(reason.clone()).size(11.0).color(palette.text_muted));
                ui.label(egui::RichText::new("This is an encrypted state file, not a viewable document.").size(11.0).color(palette.text_muted));
            });
            return None;
        }

        ui.vertical(|ui| {
            // Header.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("NDA Document").size(13.0).strong().color(palette.accent));
                ui.add_space(6.0);
                let badge = match self.kind {
                    LoadedKind::Sealed => ("sealed", palette.warning),
                    _ => ("portable", palette.success),
                };
                ui.label(egui::RichText::new(badge.0).size(10.0).color(badge.1));
                if self.dirty {
                    ui.label(egui::RichText::new("● unsaved").size(10.0).color(palette.warning));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let revs = self.doc.revisions().len();
                    ui.label(
                        egui::RichText::new(format!("{} triples · {} commands · {} revisions", self.doc.triples.len(), self.doc.commands.len(), revs))
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                });
            });
            ui.separator();

            // Toolbar.
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new("Save").color(palette.text)).on_hover_text("Write the .nda to disk (honors seal toggle)").clicked() {
                    self.save(workspace_root, toasts);
                }
                if ui.button(egui::RichText::new("Commit Revision").color(palette.accent)).on_hover_text("Append an author-signed revision to the in-file history and save").clicked() {
                    self.commit(workspace_root, toasts);
                }
                if ui.button(egui::RichText::new("Export HTML").color(palette.success)).on_hover_text("Write a self-contained browser viewer next to the file").clicked() {
                    open_in_browser = self.export_html(workspace_root, toasts);
                }
                ui.checkbox(&mut self.sealed, "Seal at rest");
            });
            ui.separator();

            if let Some(err) = &self.last_error {
                ui.label(egui::RichText::new(format!("⚠ {err}")).size(11.0).color(palette.error));
            }

            // Sub-view tabs.
            ui.horizontal(|ui| {
                for (sv, label) in [
                    (NdaSubView::Canvas, "Canvas"),
                    (NdaSubView::Triples, "Triples"),
                    (NdaSubView::History, "History"),
                    (NdaSubView::Bytes, "Bytes"),
                ] {
                    let selected = self.sub == sv;
                    if ui.selectable_label(selected, label).clicked() {
                        self.sub = sv;
                    }
                }
            });
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| match self.sub {
                NdaSubView::Canvas => self.render_canvas(ui, palette),
                NdaSubView::Triples => self.render_triples(ui, palette),
                NdaSubView::History => self.render_history(ui, workspace_root, palette),
                NdaSubView::Bytes => self.render_bytes(ui, palette),
            });
        });

        open_in_browser
    }

    // --- Sub-views ---------------------------------------------------------

    fn render_canvas(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let size = egui::vec2(ui.available_width().max(320.0), 420.0);
        let (rect, _resp) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, egui::CornerRadius::same(4), egui::Color32::from_rgb(13, 17, 23));
        let ctx = ui.ctx().clone();
        // Precompute wrapped text galleys (layout needs &mut Fonts via the ctx closure).
        let galleys: std::collections::HashMap<usize, std::sync::Arc<egui::Galley>> = ui.ctx().fonts_mut(|f| {
            self.doc
                .commands
                .iter()
                .enumerate()
                .filter(|(_, c)| CommandKind::from_u8(c.kind) == Some(CommandKind::DrawText) && c.w > 0)
                .map(|(i, c)| {
                    let g = f.layout(c.content.clone(), egui::FontId::monospace(14.0), color_from_u32(c.color), c.w as f32);
                    (i, g)
                })
                .collect()
        });
        for (idx, c) in self.doc.commands.iter().enumerate() {
            let color = color_from_u32(c.color);
            let min = rect.min + egui::vec2(c.x as f32, c.y as f32);
            match CommandKind::from_u8(c.kind) {
                Some(CommandKind::DrawRect) => {
                    let r = egui::Rect::from_min_size(min, egui::vec2(c.w as f32, c.h as f32));
                    painter.rect_filled(r, egui::CornerRadius::ZERO, color);
                }
                Some(CommandKind::DrawText) => {
                    let font_id = egui::FontId::monospace(14.0);
                    if let Some(galley) = galleys.get(&idx) {
                        painter.galley(min, galley.clone(), color);
                    } else {
                        painter.text(min, egui::Align2::LEFT_TOP, &c.content, font_id, color);
                    }
                }
                Some(CommandKind::DrawImage) => {
                    let w = if c.w > 0 { c.w as f32 } else { 120.0 };
                    let h = if c.h > 0 { c.h as f32 } else { 80.0 };
                    let r = egui::Rect::from_min_size(min, egui::vec2(w, h));
                    // Resolve (and cache) the decoded texture for this command.
                    let cached = self
                        .image_textures
                        .get(&idx)
                        .filter(|(content, _)| content == &c.content)
                        .map(|(_, id)| *id);
                    let tex = match cached {
                        Some(id) => Some(id),
                        None => decode_data_url(&c.content).and_then(|bytes| {
                            image::load_from_memory(&bytes).ok().map(|img| {
                                let rgba = img.to_rgba8();
                                let dims = [rgba.width() as usize, rgba.height() as usize];
                                let image = egui::ColorImage::from_rgba_unmultiplied(dims, rgba.as_raw());
                                ctx.load_texture(format!("nda-img-{idx}"), image, egui::TextureOptions::LINEAR).id()
                            })
                        }),
                    };
                    if let Some(id) = tex {
                        self.image_textures.insert(idx, (c.content.clone(), id));
                        painter.image(id, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                    } else {
                        painter.rect_stroke(r, egui::CornerRadius::ZERO, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
                        painter.text(min + egui::vec2(4.0, 4.0), egui::Align2::LEFT_TOP, "[image]", egui::FontId::monospace(11.0), color);
                    }
                }
                Some(CommandKind::DrawVector) => {
                    let pts = velocity_browser::nda_portable::parse_vector_points(&c.content);
                    let points: Vec<egui::Pos2> = pts
                        .iter()
                        .map(|(dx, dy)| min + egui::vec2(*dx as f32, *dy as f32))
                        .collect();
                    let stroke_w = if c.h > 0 { c.h as f32 } else { 1.0 };
                    if points.len() >= 2 {
                        painter.add(egui::Shape::line(points, egui::Stroke::new(stroke_w, color)));
                    } else if points.len() == 1 {
                        painter.circle_filled(points[0], stroke_w.max(1.0), color);
                    }
                }
                None => {
                    let r = egui::Rect::from_min_size(min, egui::vec2(c.w as f32, c.h as f32));
                    painter.rect_stroke(r, egui::CornerRadius::ZERO, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
                }
            }
        }
        if self.doc.commands.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No display commands yet — add one under Triples.",
                egui::FontId::monospace(12.0),
                palette.text_muted,
            );
        }
    }

    fn render_triples(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        // Title.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Title").color(palette.text_muted));
            if ui.text_edit_singleline(&mut self.title_input).changed() {
                self.doc.set_title(&self.title_input);
                self.dirty = true;
            }
        });
        ui.separator();

        // Add triple.
        ui.label(egui::RichText::new("Add content triple").size(11.0).strong().color(palette.text));
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.ts);
            ui.label("→");
            ui.text_edit_singleline(&mut self.tp);
            ui.label("→");
            ui.text_edit_singleline(&mut self.to);
            if ui.button("Add").clicked() && !self.ts.is_empty() && !self.tp.is_empty() {
                self.doc.push_triple(self.ts.clone(), self.tp.clone(), self.to.clone());
                self.ts.clear();
                self.tp.clear();
                self.to.clear();
                self.dirty = true;
            }
        });

        // Add display command.
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Add text command").size(11.0).strong().color(palette.text));
        ui.horizontal(|ui| {
            ui.label("text");
            ui.text_edit_singleline(&mut self.cmd_text);
            ui.label("x");
            ui.add(egui::TextEdit::singleline(&mut self.cmd_x).desired_width(48.0));
            ui.label("y");
            ui.add(egui::TextEdit::singleline(&mut self.cmd_y).desired_width(48.0));
            if ui.button("Add").clicked() && !self.cmd_text.is_empty() {
                let x = self.cmd_x.parse().unwrap_or(16);
                let y = self.cmd_y.parse().unwrap_or(24);
                self.doc.push_command(DisplayCommand::text(self.cmd_text.clone(), x, y, 0xC9D1_D9FF));
                self.cmd_text.clear();
                self.dirty = true;
            }
        });
        ui.separator();

        // Content triple list.
        let mut remove: Option<usize> = None;
        ui.label(egui::RichText::new(format!("Triples ({})", self.doc.triples.len())).size(11.0).strong().color(palette.text));
        for (i, (s, p, o)) in self.doc.triples.clone().iter().enumerate() {
            let provenance = velocity_browser::nda_portable::is_provenance_predicate(p);
            ui.horizontal(|ui| {
                let color = if provenance { palette.text_muted } else { palette.text };
                ui.label(egui::RichText::new(format!("{s} → {p} → {o}")).size(11.0).color(color));
                if ui.small_button("✕").on_hover_text("Remove triple").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.doc.triples.remove(i);
            self.dirty = true;
        }

        // Command list.
        ui.add_space(6.0);
        ui.label(egui::RichText::new(format!("Display commands ({})", self.doc.commands.len())).size(11.0).strong().color(palette.text));
        let mut remove_cmd: Option<usize> = None;
        for (i, c) in self.doc.commands.clone().iter().enumerate() {
            ui.horizontal(|ui| {
                let label = match CommandKind::from_u8(c.kind) {
                    Some(CommandKind::DrawText) => format!("text @{},{} \"{}\"", c.x, c.y, c.content),
                    Some(CommandKind::DrawRect) => format!("rect @{},{} {}x{}", c.x, c.y, c.w, c.h),
                    Some(CommandKind::DrawImage) => format!("image @{},{} {}x{}", c.x, c.y, c.w, c.h),
                    _ => format!("cmd type {} @{},{}", c.kind, c.x, c.y),
                };
                ui.label(egui::RichText::new(label).size(11.0).color(palette.text));
                if ui.small_button("✕").clicked() {
                    remove_cmd = Some(i);
                }
            });
        }
        if let Some(i) = remove_cmd {
            self.doc.commands.remove(i);
            self.dirty = true;
        }
    }

    fn render_history(&mut self, ui: &mut egui::Ui, workspace_root: &Path, palette: IdePalette) {
        // Lazily load the configured identity once.
        if !self.identity_loaded {
            let a = resolve_author(workspace_root);
            if a.source == "configured" {
                self.identity_name = a.name;
                self.identity_email = a.email;
            }
            self.identity_loaded = true;
        }
        egui::Frame::new().fill(palette.bg_tertiary).corner_radius(4.0).inner_margin(8.0).show(ui, |ui| {
            ui.label(egui::RichText::new("Author identity (used for new revisions)").size(11.0).strong().color(palette.accent));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Name").size(11.0).color(palette.text_muted));
                ui.text_edit_singleline(&mut self.identity_name);
                ui.label(egui::RichText::new("Email").size(11.0).color(palette.text_muted));
                ui.text_edit_singleline(&mut self.identity_email);
                if ui.button("Save").on_hover_text("Persist as the configured workspace identity").clicked() {
                    match set_identity(workspace_root, self.identity_name.trim(), self.identity_email.trim()) {
                        Ok(_) => self.set_error(None),
                        Err(e) => self.set_error(Some(format!("failed to save identity: {e}"))),
                    }
                }
            });
            let resolved = resolve_author(workspace_root);
            ui.label(egui::RichText::new(format!("Resolved: {} <{}> [{}]", resolved.name, resolved.email, resolved.source)).size(10.0).color(palette.text_muted));
        });
        ui.add_space(8.0);

        let origin = self
            .doc
            .triples
            .iter()
            .find(|(_, p, _)| p == velocity_browser::nda_portable::NDA_ORIGIN)
            .map(|(_, _, o)| o.clone());
        let created = self
            .doc
            .triples
            .iter()
            .find(|(_, p, _)| p == velocity_browser::nda_portable::NDA_CREATED)
            .map(|(_, _, o)| o.clone());

        egui::Frame::new().fill(palette.bg_tertiary).corner_radius(4.0).inner_margin(8.0).show(ui, |ui| {
            ui.label(egui::RichText::new("Origin").size(11.0).strong().color(palette.accent));
            ui.label(egui::RichText::new(format!("Workspace: {}", origin.clone().unwrap_or_else(|| "unknown".into()))).size(11.0).color(palette.text));
            if let Some(c) = created {
                ui.label(egui::RichText::new(format!("Created: {c}")).size(11.0).color(palette.text_muted));
            }
        });
        ui.add_space(8.0);

        let revs = self.doc.revisions();
        if revs.is_empty() {
            ui.label(egui::RichText::new("No revisions recorded yet — use “Commit Revision”.").size(11.0).color(palette.text_muted));
            return;
        }
        let chain_ok = self.doc.verify_history().is_ok();
        ui.label(
            egui::RichText::new(if chain_ok { "✓ history chain verified" } else { "⚠ history chain broken" })
                .size(11.0)
                .color(if chain_ok { palette.success } else { palette.error }),
        );
        ui.add_space(4.0);

        for (i, r) in revs.iter().enumerate() {
            egui::Frame::new().fill(palette.bg_tertiary).corner_radius(4.0).inner_margin(8.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("#{i}")).size(11.0).strong().color(palette.accent));
                    ui.label(egui::RichText::new(format!("[{}]", r.author_source)).size(10.0).color(palette.text_muted));
                    ui.label(egui::RichText::new(&r.author_name).size(11.0).strong().color(palette.text));
                    if !r.author_email.is_empty() {
                        ui.label(egui::RichText::new(&r.author_email).size(10.0).color(palette.text_muted));
                    }
                });
                ui.label(egui::RichText::new(format!("{} {}", r.timestamp, r.message)).size(11.0).color(palette.text));
                ui.label(
                    egui::RichText::new(format!("content {}… ← parent {}", &r.content_hash[..r.content_hash.len().min(16)], if r.parent == velocity_browser::nda_portable::GENESIS { "genesis".to_string() } else { format!("{}…", &r.parent[..r.parent.len().min(16)]) }))
                        .monospace()
                        .size(10.0)
                        .color(palette.text_muted),
                );
            });
            ui.add_space(4.0);
        }
    }

    fn render_bytes(&self, ui: &mut egui::Ui, palette: IdePalette) {
        let bytes = self.doc.to_portable_bytes();
        let triple_count = self.doc.triples.len();
        let command_count = self.doc.commands.len();
        let triples_end = 48 + triple_count * 12;
        let commands_end = triples_end + command_count * 18;
        ui.label(
            egui::RichText::new(format!("{} bytes · header 0–47 · triples 48–{} · commands –{} · pool –{}", bytes.len(), triples_end.saturating_sub(1), commands_end.saturating_sub(1), bytes.len().saturating_sub(1)))
                .size(10.0)
                .color(palette.text_muted),
        );
        ui.add_space(4.0);
        egui::ScrollArea::horizontal().show(ui, |ui| {
            let mut text = String::new();
            for (l, chunk) in bytes.chunks(16).enumerate() {
                let off = l * 16;
                let region = if off < 48 { "HDR" } else if off < triples_end { "TRI" } else if off < commands_end { "CMD" } else { "POOL" };
                text.push_str(&format!("{off:08X} [{region:>4}] "));
                for b in chunk {
                    text.push_str(&format!("{b:02X} "));
                }
                text.push_str(" |");
                for b in chunk {
                    text.push(if (32..127).contains(b) { *b as char } else { '.' });
                }
                text.push_str("|\n");
            }
            ui.label(egui::RichText::new(text).monospace().size(11.0).color(palette.text));
        });
    }

    // --- Actions -----------------------------------------------------------

    fn default_save_path(&self, workspace_root: &Path) -> PathBuf {
        let stem = self
            .doc
            .title()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "untitled".to_string());
        let safe: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        workspace_root.join(format!("{safe}.nda"))
    }

    fn save(&mut self, workspace_root: &Path, toasts: &mut ToastQueue) {
        let path = match self.path.clone() {
            Some(p) => p,
            None => {
                let p = self.default_save_path(workspace_root);
                self.path = Some(p.clone());
                p
            }
        };
        match save_to_disk(workspace_root, &path, &self.doc, self.sealed) {
            Ok(_) => {
                self.dirty = false;
                self.kind = if self.sealed { LoadedKind::Sealed } else { LoadedKind::Portable };
                toasts.push(Toast::success(format!("Saved {}", path.display())));
            }
            Err(e) => {
                self.set_error(Some(e.clone()));
                toasts.push(Toast::error(format!("Save failed: {e}")));
            }
        }
    }

    fn commit(&mut self, workspace_root: &Path, toasts: &mut ToastQueue) {
        let author = resolve_author(workspace_root);
        let ts = now_rfc3339();
        let msg = self.commit_msg.trim().to_string();
        let origin = workspace_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string());
        self.doc.commit_revision(&author.name, &author.email, &author.source, &ts, &msg, &origin);
        self.commit_msg.clear();
        self.dirty = true;
        toasts.push(Toast::info(format!("Revision committed as {} ({})", author.name, author.source)));
        self.save(workspace_root, toasts);
    }

    fn export_html(&self, workspace_root: &Path, toasts: &mut ToastQueue) -> Option<PathBuf> {
        let nda_path = self.path.clone().unwrap_or_else(|| self.default_save_path(workspace_root));
        let bytes = self.doc.to_portable_bytes();
        let title = self.doc.title().unwrap_or("NDA Document").to_string();
        let html = crate::editor::nda_viewer::self_contained_html(&bytes, &title);
        let out = nda_path.with_extension("nda.html");
        match std::fs::write(&out, html) {
            Ok(_) => {
                toasts.push(Toast::success(format!("Exported {}", out.display())));
                Some(out)
            }
            Err(e) => {
                toasts.push(Toast::error(format!("Export failed: {e}")));
                None
            }
        }
    }
}

/// Convert an existing file into a renderable portable NDA document: text-like
/// files become wrapped DrawText lines plus content triples; images become a
/// single DrawImage command carrying a data-url.
pub fn convert_file_to_doc(path: &Path) -> Result<NdaPortableDoc, String> {
    let mut doc = NdaPortableDoc::new();
    let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".to_string());
    doc.set_title(&file_name);
    doc.push_triple("nda:doc", "nda:source_file", &file_name);

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp") {
        let raw = std::fs::read(path).map_err(|e| format!("read failed: {e}"))?;
        let mime = match ext.as_str() {
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        };
        let data_url = format!("data:{mime};base64,{}", crate::editor::nda_viewer::base64_encode(&raw));
        doc.push_command(DisplayCommand::image(data_url, 10, 10, 480, 320));
        doc.push_triple("nda:doc", "nda:kind", "image");
        return Ok(doc);
    }

    let text = crate::editor::knowledge_base::extract_text(path)
        .or_else(|| std::fs::read_to_string(path).ok())
        .ok_or_else(|| format!("cannot extract text from {}", file_name))?;

    doc.push_triple("nda:doc", "nda:kind", "text");
    let mut y: u16 = 20;
    for line in text.lines().take(200) {
        let trimmed: String = line.chars().take(120).collect();
        doc.push_command(DisplayCommand::text(trimmed, 12, y, 0xC9D1_D9FF));
        y = y.saturating_add(18);
    }
    let line_count = text.lines().count();
    doc.push_triple("nda:doc", "nda:line_count", line_count.to_string());
    Ok(doc)
}

/// Best-effort: open a path (e.g. an exported `.html`) in the default browser.
/// Failures are ignored — the file is still written for the user to open.
pub fn open_in_browser(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd").args(["/C", "start", "", &path.to_string_lossy()]).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

fn color_from_u32(c: u32) -> egui::Color32 {
    egui::Color32::from_rgba_premultiplied(
        ((c >> 24) & 0xFF) as u8,
        ((c >> 16) & 0xFF) as u8,
        ((c >> 8) & 0xFF) as u8,
        (c & 0xFF) as u8,
    )
}

/// Decode a `data:<mime>;base64,<payload>` URL into raw bytes. Returns `None`
/// for non-data URLs or invalid base64.
pub fn decode_data_url(url: &str) -> Option<Vec<u8>> {
    let rest = url.strip_prefix("data:")?;
    let b64 = rest.split_once(',')?.1;
    from_base64(b64)
}

/// Standard base64 decode (padding-tolerant); inverse of
/// [`crate::editor::nda_viewer::base64_encode`].
pub fn from_base64(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u32> = input
        .bytes()
        .filter(|&b| b != b'=' && !b.is_ascii_whitespace())
        .map(val)
        .collect::<Option<_>>()?;
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let n = chunk.len();
        let mut acc: u32 = 0;
        for &v in chunk {
            acc = (acc << 6) | v;
        }
        acc <<= (4 - n) * 6;
        if n >= 2 {
            out.push(((acc >> 16) & 0xFF) as u8);
        }
        if n >= 3 {
            out.push(((acc >> 8) & 0xFF) as u8);
        }
        if n >= 4 {
            out.push((acc & 0xFF) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_formats_epoch_and_known_date() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
        // 2021-01-01T00:00:00Z == 1609459200
        assert_eq!(rfc3339_from_unix(1_609_459_200), "2021-01-01T00:00:00Z");
    }

    #[test]
    fn resolve_author_falls_back_to_os() {
        // In a temp dir with no git and no configured identity, source is git or os.
        let tmp = std::env::temp_dir().join(format!("nda_author_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let author = resolve_author(&tmp);
        assert!(["git", "os", "configured"].contains(&author.source.as_str()));
        assert!(!author.name.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn configured_identity_wins() {
        let tmp = std::env::temp_dir().join(format!("nda_author_cfg_{}", std::process::id()));
        let _ = std::fs::create_dir_all(tmp.join(".velocity"));
        set_identity(&tmp, "Configured User", "cfg@example.com").unwrap();
        let author = resolve_author(&tmp);
        assert_eq!(author.source, "configured");
        assert_eq!(author.name, "Configured User");
        assert_eq!(author.email, "cfg@example.com");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn convert_text_file_produces_commands() {
        let tmp = std::env::temp_dir().join(format!("nda_convert_{}.txt", std::process::id()));
        std::fs::write(&tmp, "line one\nline two\nline three").unwrap();
        let doc = convert_file_to_doc(&tmp).unwrap();
        assert_eq!(doc.title().unwrap().ends_with(".txt"), true);
        assert_eq!(doc.commands.len(), 3);
        assert!(doc.commands.iter().all(|c| c.kind == CommandKind::DrawText as u8));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn from_base64_inverts_encoder() {
        for sample in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", b"\x00\x01\xff\xfe"] {
            let enc = crate::editor::nda_viewer::base64_encode(sample);
            assert_eq!(from_base64(&enc).unwrap(), sample.to_vec(), "sample {sample:?}");
        }
    }

    #[test]
    fn decode_data_url_extracts_payload() {
        let url = format!("data:text/plain;base64,{}", crate::editor::nda_viewer::base64_encode(b"hello nda"));
        assert_eq!(decode_data_url(&url).unwrap(), b"hello nda");
        assert!(decode_data_url("https://example.com/x.png").is_none());
        assert!(decode_data_url("data:image/png;base64,!!!not-base64!!!").is_none());
    }

    #[test]
    fn decode_data_url_decodes_real_png() {
        // Encode a 2x2 PNG via the image crate, wrap as a data-url, decode back.
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut png: Vec<u8> = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let url = format!("data:image/png;base64,{}", crate::editor::nda_viewer::base64_encode(&png));
        let decoded = decode_data_url(&url).unwrap();
        let reloaded = image::load_from_memory(&decoded).unwrap();
        assert_eq!((reloaded.width(), reloaded.height()), (2, 2));
    }
}
