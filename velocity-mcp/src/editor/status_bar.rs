use eframe::egui::{self, Color32, Panel, Ui};

pub struct StatusBar;

impl StatusBar {
    pub fn show(ui: &mut Ui, branch: Option<&str>, position: Option<(usize, usize)>, build_ok: bool, status: &str) {
        Panel::bottom("status_bar").show(ui, |ui: &mut egui::Ui| {
            ui.horizontal(|ui: &mut egui::Ui| {
                let (icon, color) = if build_ok {
                    ("✔", Color32::from_rgb(74, 222, 128))
                } else {
                    ("✖", Color32::from_rgb(248, 113, 113))
                };

                ui.label(egui::RichText::new(format!("{} build", icon)).color(color).size(12.0));
                ui.separator();

                if let Some(b) = branch {
                    ui.label(egui::RichText::new(format!("⎇ {}", b)).size(12.0));
                    ui.separator();
                }

                if let Some((line, col)) = position {
                    ui.label(egui::RichText::new(format!("Ln {}, Col {}", line + 1, col + 1)).size(12.0));
                    ui.separator();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                    ui.label(egui::RichText::new(status).size(12.0).color(Color32::GRAY));
                });
            });
        });
    }
}
