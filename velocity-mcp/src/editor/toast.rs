use eframe::egui::{self, Align2, Color32, CornerRadius, Frame, Id, Margin, Order, ProgressBar, RichText, Vec2};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastLevel {
    fn color(&self) -> Color32 {
        match self {
            ToastLevel::Info => Color32::from_rgb(96, 165, 250),
            ToastLevel::Success => Color32::from_rgb(74, 222, 128),
            ToastLevel::Warning => Color32::from_rgb(250, 204, 21),
            ToastLevel::Error => Color32::from_rgb(248, 113, 113),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    pub created: Instant,
    pub ttl: Duration,
}

impl Toast {
    pub fn new(message: impl Into<String>, level: ToastLevel) -> Self {
        Self {
            message: message.into(),
            level,
            created: Instant::now(),
            ttl: Duration::from_secs(4),
        }
    }

    pub fn info(message: impl Into<String>) -> Self { Self::new(message, ToastLevel::Info) }
    pub fn success(message: impl Into<String>) -> Self { Self::new(message, ToastLevel::Success) }
    pub fn warn(message: impl Into<String>) -> Self { Self::new(message, ToastLevel::Warning) }
    pub fn error(message: impl Into<String>) -> Self { Self::new(message, ToastLevel::Error) }

    fn remaining(&self) -> f32 {
        let elapsed = self.created.elapsed().as_secs_f32();
        let ttl = self.ttl.as_secs_f32();
        (ttl - elapsed.min(ttl)) / ttl
    }
}

#[derive(Default)]
pub struct ToastQueue {
    toasts: Vec<Toast>,
}

impl ToastQueue {
    pub fn push(&mut self, toast: Toast) {
        self.toasts.push(toast);
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts.retain(|t| now.duration_since(t.created) < t.ttl);
        if self.toasts.is_empty() {
            return;
        }

        let area = egui::Area::new(Id::new("toast_area"))
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-20.0, -60.0))
            .order(Order::Foreground);

        area.show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.set_width(320.0);
                for (idx, toast) in self.toasts.iter_mut().enumerate() {
                    let remaining = toast.remaining();
                    let alpha = ((remaining * 2.0).min(1.0) * 255.0) as u8;
                    let mut base_color = Color32::from_rgb(18, 18, 24);
                    base_color[3] = alpha;
                    let stroke_color = toast.level.color();

                    Frame::new()
                        .fill(base_color)
                        .stroke(egui::Stroke::new(1.5, stroke_color))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::same(12))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(toast.message.clone()).color(stroke_color).size(13.0));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button(egui::RichText::new("✕").size(12.0)).clicked() {
                                        toast.created = Instant::now() - toast.ttl;
                                    }
                                });
                            });
                            if idx == 0 {
                                ui.add(
                                    ProgressBar::new(remaining)
                                        .desired_height(2.0)
                                        .fill(stroke_color),
                                );
                            }
                        });
                    ui.add_space(8.0);
                }
            });
        });
    }
}
