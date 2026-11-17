
use eframe::egui;
use egui::{Color32, RichText, Stroke};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::{about, components, instance, keyboard_idle, libacercontrol};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page { Dashboard, Cooling, Keyboard, Performance, Battery, Settings }

impl Page {
    const ALL: [Self; 6] = [Self::Dashboard, Self::Cooling, Self::Keyboard, Self::Performance, Self::Battery, Self::Settings];
    fn title(self) -> &'static str { match self {
        Self::Dashboard => "Dashboard", Self::Cooling => "Cooling", Self::Keyboard => "Keyboard RGB",
        Self::Performance => "Performance", Self::Battery => "Battery", Self::Settings => "Settings",
    }}
    fn description(self) -> &'static str { match self {
        Self::Dashboard => "Hardware status and device overview",
        Self::Cooling => "Fan control and thermal telemetry",
        Self::Keyboard => "Backlight color, effects, and inactivity behavior",
        Self::Performance => "System performance controls",
        Self::Battery => "Battery care and charging options",
        Self::Settings => "Application preferences and system information",
    }}
}

const APP_BG: Color32 = Color32::from_rgb(17, 20, 24);
const SIDEBAR_BG: Color32 = Color32::from_rgb(20, 24, 29);
const SURFACE: Color32 = Color32::from_rgb(26, 31, 37);
const SURFACE_HOVER: Color32 = Color32::from_rgb(34, 40, 47);
const BORDER: Color32 = Color32::from_rgb(46, 53, 61);
const TEXT: Color32 = Color32::from_rgb(229, 233, 237);
const MUTED: Color32 = Color32::from_rgb(151, 160, 169);
const ACCENT: Color32 = Color32::from_rgb(95, 174, 166);
const COOL: Color32 = Color32::from_rgb(89, 190, 170);
const WARM: Color32 = Color32::from_rgb(241, 177, 82);
const HOT: Color32 = Color32::from_rgb(226, 105, 92);

#[derive(Clone, Default)]
struct TelemetrySnapshot {
    cpu_temperature: Option<i32>,
    gpu_temperature: Option<i32>,
    cpu_fan_rpm: Option<i32>,
    gpu_fan_rpm: Option<i32>,
    sampled_at: Option<Instant>,
}

pub(crate) struct ControlCenter {
    page: Page,
    brightness: u8,
    color: Color32,
    notice: Option<(bool, String, Instant)>,
    fan_mode: components::FanMode,
    fan_mode_confirmed: bool,
    power_profiles: [[u32; 2]; 3],
    power_profile_index: usize,
    animation_mode: components::AnimationMode,
    animation_mode_confirmed: bool,
    idle_timeout_seconds: u32,
    minimize_to_tray: bool,
    tray_ready: bool,
    idle_controller: Option<keyboard_idle::KeyboardIdleController>,
    about_info: about::AboutInfo,
    update_sender: mpsc::Sender<Result<about::LatestRelease, String>>,
    update_receiver: mpsc::Receiver<Result<about::LatestRelease, String>>,
    power_cap_sender: mpsc::Sender<Result<(String, String), String>>,
    power_cap_receiver: mpsc::Receiver<Result<(String, String), String>>,
    power_cap_applying: bool,
    telemetry_receiver: mpsc::Receiver<TelemetrySnapshot>,
    telemetry: TelemetrySnapshot,
    update_checking: bool,
    update_status: String,
    latest_release: Option<about::LatestRelease>,
}

impl ControlCenter {
    fn set_notice(&mut self, succeeded: bool, message: String) {
        self.notice = Some((succeeded, message, Instant::now()));
    }

    pub(crate) fn new(background_ready: bool) -> Self {
        let bus = libacercontrol::dbus::AcerControlDBus::new().ok();
        let brightness = bus
            .as_ref()
            .and_then(|bus| bus.keyboard_brightness().ok())
            .unwrap_or(128);
        let color = bus
            .as_ref()
            .and_then(|bus| bus.keyboard_color().ok())
            .map(|(r, g, b)| Color32::from_rgb(r, g, b))
            .unwrap_or(Color32::from_rgb(50, 140, 255));
        let (fan_mode, fan_mode_confirmed) = bus
            .as_ref()
            .and_then(|bus| bus.fan_max_selected().ok())
            .map(|is_max| {
                (
                    if is_max { components::FanMode::Max } else { components::FanMode::Auto },
                    true,
                )
            })
            .unwrap_or((components::FanMode::Auto, false));
        let (animation_mode, animation_mode_confirmed) = bus
            .as_ref()
            .and_then(|bus| bus.breathing_selected().ok())
            .map(|is_breathing| {
                (
                    if is_breathing { components::AnimationMode::Breathing } else { components::AnimationMode::Off },
                    true,
                )
            })
            .unwrap_or((components::AnimationMode::Off, false));
        let (power_profiles, power_profile_index) = power_profiles::load();
        let idle_timeout_seconds = keyboard_idle::load_timeout();
        let stored_minimize_to_tray = keyboard_idle::load_minimize_to_tray();
        // Keep the saved preference visible even if the tray host is slow to
        // appear during login. Availability is tracked separately by tray_ready.
        let minimize_to_tray = stored_minimize_to_tray;
        let autostart_error = keyboard_idle::set_login_autostart(
            idle_timeout_seconds > 0 || minimize_to_tray,
        )
        .err();
        let tray_ready = background_ready;
        let (update_sender, update_receiver) = mpsc::channel();
        let (power_cap_sender, power_cap_receiver) = mpsc::channel();
        let (telemetry_sender, telemetry_receiver) = mpsc::channel();
        std::thread::spawn(move || loop {
            let snapshot = TelemetrySnapshot {
                cpu_temperature: libacercontrol::telemetry::cpu_temperature().ok(),
                gpu_temperature: libacercontrol::telemetry::gpu_temperature().ok(),
                cpu_fan_rpm: libacercontrol::telemetry::cpu_fan_rpm().ok(),
                gpu_fan_rpm: libacercontrol::telemetry::gpu_fan_rpm().ok(),
                sampled_at: Some(Instant::now()),
            };
            if telemetry_sender.send(snapshot).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        });
        Self {
            page: Page::Dashboard,
            brightness,
            color,
            notice: autostart_error.map(|error| {
                (false, format!("Could not configure login startup for keyboard inactivity: {error}"), Instant::now())
            }).or_else(|| (!tray_ready && stored_minimize_to_tray).then(|| (false, "Minimize to tray remains enabled, but the tray service is unavailable. The window may stay open until the desktop tray is ready.".to_owned(), Instant::now()))),
            fan_mode,
            fan_mode_confirmed,
            power_profiles,
            power_profile_index,
            animation_mode,
            animation_mode_confirmed,
            idle_timeout_seconds,
            minimize_to_tray,
            tray_ready,
            idle_controller: (!background_ready && idle_timeout_seconds > 0).then(|| {
                keyboard_idle::KeyboardIdleController::new(idle_timeout_seconds, brightness)
            }),
            about_info: about::AboutInfo::collect(),
            update_sender,
            update_receiver,
            power_cap_sender,
            power_cap_receiver,
            power_cap_applying: false,
            telemetry_receiver,
            telemetry: TelemetrySnapshot::default(),
            update_checking: false,
            update_status: "Updates are checked only when you ask.".to_owned(),
            latest_release: None,
        }
    }

    fn action(&mut self, label: &str, action: impl FnOnce() -> Result<(), String>) -> bool {
        let result = action();
        let succeeded = result.is_ok();
        self.notice = Some(match result { Ok(()) => (true, format!("{label} applied"), Instant::now()), Err(e) => (false, format!("{label} failed: {e}"), Instant::now()) });
        succeeded
    }

    fn gauge_card(ui: &mut egui::Ui, label: &str, value: Option<i32>, max_value: f32, unit: &str, suffix: &str) {
        egui::Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(204.0);
                ui.label(RichText::new(label.to_uppercase()).small().strong().color(MUTED));
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 169.0), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                let center = egui::pos2(rect.center().x, rect.top() + 100.0);
                let radius = (rect.width() * 0.31).clamp(48.0, 77.0);
                let fraction = value.map(|v| (v as f32 / max_value).clamp(0.0, 1.0));
                let segments = 48;
                for i in 0..segments {
                    let start = std::f32::consts::PI - std::f32::consts::PI * i as f32 / segments as f32;
                    let end = std::f32::consts::PI - std::f32::consts::PI * (i + 1) as f32 / segments as f32;
                    let active = fraction.is_some_and(|amount| (i as f32 + 0.5) / segments as f32 <= amount);
                    let position = i as f32 / segments as f32;
                    let band_color = if position < 0.68 { COOL } else if position < 0.86 { WARM } else { HOT };
                    let color = if active { band_color } else { Color32::from_rgb(54, 62, 69) };
                    painter.line_segment(
                        [egui::pos2(center.x + radius * start.cos(), center.y - radius * start.sin()),
                         egui::pos2(center.x + radius * end.cos(), center.y - radius * end.sin())],
                        Stroke::new(9.0, color),
                    );
                }
                let display = value.map(|v| format!("{v}{suffix}")).unwrap_or_else(|| "—".to_owned());
                painter.text(egui::pos2(center.x, center.y + 13.0), egui::Align2::CENTER_CENTER, display,
                    egui::FontId::proportional(27.0), TEXT);
                painter.text(egui::pos2(center.x, center.y + 37.0), egui::Align2::CENTER_CENTER,
                    if value.is_some() { unit } else { "SENSOR UNAVAILABLE" },
                    egui::FontId::proportional(10.0), MUTED);
                painter.text(egui::pos2(center.x - radius - 2.0, center.y + 12.0), egui::Align2::LEFT_CENTER, "0",
                    egui::FontId::proportional(10.0), MUTED);
                painter.text(egui::pos2(center.x + radius + 2.0, center.y + 12.0), egui::Align2::RIGHT_CENTER,
                    max_value as i32, egui::FontId::proportional(10.0), MUTED);
            });
    }

    fn telemetry_gauges(ui: &mut egui::Ui, telemetry: &TelemetrySnapshot) {
        ui.columns(2, |columns| {
            Self::gauge_card(&mut columns[0], "CPU temperature", telemetry.cpu_temperature, 100.0, "THERMAL · °C", "°C");
            Self::gauge_card(&mut columns[1], "GPU temperature", telemetry.gpu_temperature, 100.0, "THERMAL · °C", "°C");
        });
        ui.add_space(10.0);
        ui.columns(2, |columns| {
            Self::gauge_card(&mut columns[0], "CPU fan speed", telemetry.cpu_fan_rpm, 6000.0, "FAN SPEED · RPM", " RPM");
            Self::gauge_card(&mut columns[1], "GPU fan speed", telemetry.gpu_fan_rpm, 6000.0, "FAN SPEED · RPM", " RPM");
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let sampled = telemetry.sampled_at.map(|time| {
                format!("Updated {} sec ago", time.elapsed().as_secs().min(99))
            });
            ui.label(
                RichText::new(sampled.unwrap_or_else(|| "Waiting for sensor data".to_owned()))
                    .small()
                    .color(MUTED),
            );
        });
    }

    pub(crate) fn configure_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.dark_mode = true;
        visuals.override_text_color = Some(TEXT);
        visuals.window_fill = SURFACE;
        visuals.panel_fill = APP_BG;
        visuals.extreme_bg_color = Color32::from_rgb(13, 16, 19);
        visuals.faint_bg_color = Color32::from_rgb(23, 27, 32);
        visuals.selection.bg_fill = Color32::from_rgb(42, 76, 78);
        visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.noninteractive.bg_fill = SURFACE;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(36, 42, 49);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.active.bg_fill = Color32::from_rgb(42, 76, 78);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.open.bg_fill = SURFACE_HOVER;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, BORDER);
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(14.0, 9.0);
        style.spacing.interact_size = egui::vec2(40.0, 34.0);
        style.visuals.window_corner_radius = egui::CornerRadius::same(10);
        ctx.set_style(style);
    }

    fn keyboard_preview(ui: &mut egui::Ui, color: Color32, brightness: u8) {
        ui.add_space(16.0);
        ui.label(RichText::new("LIVE KEYBOARD PREVIEW").strong());
        let preview_width = ui.available_width().min(680.0);
        let scale = preview_width / 680.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(preview_width, 156.0 * scale), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let glow = |channel: u8| ((channel as u16 * brightness as u16) / 255) as u8;
        let key_color = Color32::from_rgb(glow(color.r()), glow(color.g()), glow(color.b()));
        let rows: [&[&str]; 4] = [
            &["Esc", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"],
            &["`", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "=", "<-"],
            &["Tab", "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P", "[", "]"],
            &["Caps", "A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'", "Enter"],
        ];
        for (row_index, row) in rows.iter().enumerate() {
            let gap = 5.0 * scale;
            let key_width = ((rect.width() - gap * (row.len() as f32 - 1.0)) / row.len() as f32).min(48.0 * scale);
            let row_width = row.len() as f32 * key_width + gap * (row.len() as f32 - 1.0);
            let start_x = rect.center().x - row_width / 2.0;
            let y = rect.top() + row_index as f32 * 34.0 * scale;
            for (column, label) in row.iter().enumerate() {
                let key = egui::Rect::from_min_size(
                    egui::pos2(start_x + column as f32 * (key_width + gap), y),
                    egui::vec2(key_width, 27.0 * scale),
                );
                painter.rect_filled(key, 5.0 * scale, Color32::from_gray(32));
                painter.rect_stroke(key, 5.0 * scale, egui::Stroke::new(scale, key_color), egui::StrokeKind::Inside);
                painter.text(key.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(10.0 * scale), key_color);
            }
        }
    }

}

mod power_profiles;
mod ui;

impl Drop for ControlCenter {
    fn drop(&mut self) {
        if !self.minimize_to_tray && self.idle_timeout_seconds == 0 {
            instance::request_background_quit();
        }
    }
}

impl eframe::App for ControlCenter {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if instance::take_activation_request() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        while let Ok(snapshot) = self.telemetry_receiver.try_recv() {
            self.telemetry = snapshot;
        }
        if instance::take_window_quit_request() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            if self.minimize_to_tray && self.tray_ready {
                log::info!("Window closed; background tray service and inactivity controller remain active.");
            } else if self.idle_timeout_seconds > 0 && self.tray_ready {
                log::info!("Window closed; inactivity controller remains active in the tray service.");
            }
        }
        if let Ok(result) = self.power_cap_receiver.try_recv() {
            self.power_cap_applying = false;
            match result {
                Ok((cpu, gpu)) => {
                    let success = cpu.starts_with("CPU RAPL setting read back")
                        && gpu.starts_with("NVIDIA current limit verified");
                    self.set_notice(success, format!("{cpu}. {gpu}."));
                }
                Err(error) => self.set_notice(
                    false,
                    format!("Could not contact the power-control daemon: {error}"),
                ),
            }
        }
        if let Ok(result) = self.update_receiver.try_recv() {
            self.update_checking = false;
            match result {
                Ok(release) => {
                    let current = env!("CARGO_PKG_VERSION");
                    self.update_status = match about::compare_versions(current, &release.tag_name) {
                        Some(std::cmp::Ordering::Less) => format!("Update available: {}", release.tag_name),
                        Some(std::cmp::Ordering::Equal) => format!("You are up to date ({current})."),
                        Some(std::cmp::Ordering::Greater) => format!("This build ({current}) is newer than the latest published release ({}).", release.tag_name),
                        None => format!("Latest published version: {}", release.tag_name),
                    };
                    self.latest_release = Some(release);
                }
                Err(error) => {
                    self.update_status = if error.starts_with("No published release") {
                        error
                    } else {
                        format!("Could not check for updates: {error}")
                    };
                }
            }
        }
        egui::SidePanel::left("navigation")
            .resizable(false)
            .default_width(230.0)
            .frame(egui::Frame::new().fill(SIDEBAR_BG).stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(egui::Margin::same(14)))
            .show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("ACER").size(11.0).strong().color(ACCENT));
                ui.add_space(3.0);
                ui.label(RichText::new("Control Center").size(18.0).strong().color(TEXT));
                ui.label(RichText::new("DEVICE MANAGEMENT").size(9.0).color(MUTED));
                ui.add_space(22.0);
                for page in Page::ALL {
                    if components::sidebar_button(ui, page.title(), self.page == page) {
                        self.page = page;
                    }
                    ui.add_space(2.0);
                }
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).small().color(MUTED));
            });
        });
        egui::TopBottomPanel::top("page-header")
            .frame(egui::Frame::new().fill(SIDEBAR_BG).stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(egui::Margin::symmetric(26, 16)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading(RichText::new(self.page.title()).size(22.0).color(TEXT));
                        ui.label(RichText::new(self.page.description()).small().color(MUTED));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("LINUX DEVICE UTILITY").small().color(MUTED));
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(APP_BG).inner_margin(egui::Margin::same(24)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(980.0);
                    self.page_content(ui);
                    if self.notice.as_ref().is_some_and(|(_, _, created)| created.elapsed() >= Duration::from_secs(3)) {
                        self.notice = None;
                    }
                    if let Some((ok, message, _)) = &self.notice {
                        ui.add_space(16.0);
                        let color = if *ok { ACCENT } else { Color32::from_rgb(220, 132, 122) };
                        egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0_f32, BORDER)).corner_radius(8.0).inner_margin(12).show(ui, |ui| {
                            ui.colored_label(color, message);
                        });
                    }
                });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}
