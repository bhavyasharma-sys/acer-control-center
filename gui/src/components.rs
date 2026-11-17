use eframe::egui::{self, Color32, RichText};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FanMode {
    Auto,
    Max,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AnimationMode {
    Off,
    Breathing,
}

/// Vivid cyan accent, matching the dashboard palette.
const ACCENT: Color32 = Color32::from_rgb(43, 204, 220);
const ACCENT_FILL: Color32 = Color32::from_rgb(18, 52, 60);
const ACCENT_BORDER: Color32 = Color32::from_rgb(36, 122, 138);
const SURFACE_FILL: Color32 = Color32::from_rgb(14, 21, 29);
const SURFACE_BORDER: Color32 = Color32::from_rgb(31, 50, 61);
const TEXT_DIM: Color32 = Color32::from_rgb(158, 176, 186);

pub fn segmented_choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: T,
    options: &[(T, &str)],
) -> Option<T> {
    let mut choice = None;
    // Adaptive width: fill what is available without overflowing narrow
    // dashboard columns, and never shrink below a touchable target.
    let spacing = 6.0_f32;
    let count = options.len().max(1) as f32;
    let width = ((ui.available_width() - spacing * (count - 1.0)) / count).clamp(56.0, 190.0);
    let height = 38.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = spacing;
        for (value, label) in options {
            let selected = current == *value;
            let text = if selected {
                RichText::new(*label).strong().color(ACCENT)
            } else {
                RichText::new(*label).color(TEXT_DIM)
            };
            let fill = if selected { ACCENT_FILL } else { SURFACE_FILL };
            let stroke = egui::Stroke::new(
                1.0_f32,
                if selected { ACCENT_BORDER } else { SURFACE_BORDER },
            );
            let button = egui::Button::new(text)
                .selected(selected)
                .fill(fill)
                .stroke(stroke)
                .corner_radius(8.0)
                .min_size(egui::vec2(width, height));
            if ui.add_sized([width, height], button).clicked() {
                choice = Some(*value);
            }
        }
    });
    choice
}
