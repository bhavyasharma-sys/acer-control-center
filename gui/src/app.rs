
use eframe::egui;
use egui::{Color32, RichText, Stroke};
use std::ffi::CString;
use std::process::Command;
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
    fn icon(self) -> &'static str { match self {
        Self::Dashboard => "◈", Self::Cooling => "⌁", Self::Keyboard => "⌨",
        Self::Performance => "◉", Self::Battery => "▱", Self::Settings => "⚙",
    }}
}

const APP_BG: Color32 = Color32::from_rgb(7, 10, 15);
const SIDEBAR_BG: Color32 = Color32::from_rgb(10, 14, 21);
const SURFACE: Color32 = Color32::from_rgb(15, 22, 30);
const SURFACE_HOVER: Color32 = Color32::from_rgb(22, 34, 44);
const BORDER: Color32 = Color32::from_rgb(32, 51, 62);
const TEXT: Color32 = Color32::from_rgb(231, 240, 245);
const MUTED: Color32 = Color32::from_rgb(133, 154, 164);
const ACCENT: Color32 = Color32::from_rgb(43, 204, 220);
const COOL: Color32 = Color32::from_rgb(59, 224, 179);
const WARM: Color32 = Color32::from_rgb(255, 188, 75);
const HOT: Color32 = Color32::from_rgb(255, 79, 96);

#[derive(Clone, Default)]
struct TelemetrySnapshot {
    cpu_temperature: Option<i32>,
    gpu_temperature: Option<i32>,
    cpu_fan_rpm: Option<i32>,
    gpu_fan_rpm: Option<i32>,
    cpu_clock_mhz: Option<u32>,
    cpu_clock_max_mhz: Option<u32>,
    cpu_usage_percent: Option<u8>,
    gpu_clock_mhz: Option<u32>,
    gpu_usage_percent: Option<u8>,
    gpu_memory_mib: Option<(u64, u64)>,
    system_memory_mib: Option<(u64, u64)>,
    root_storage_mib: Option<(u64, u64)>,
    sampled_at: Option<Instant>,
}

fn read_cpu_clock() -> (Option<u32>, Option<u32>) {
    let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") else {
        return (None, None);
    };
    let mut current_total = 0_u64;
    let mut current_count = 0_u64;
    let mut maximum = 0_u64;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.strip_prefix("cpu").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
        }) {
            continue;
        }
        let cpufreq = entry.path().join("cpufreq");
        let current = ["scaling_cur_freq", "cpuinfo_cur_freq"]
            .iter()
            .copied()
            .find_map(|file| std::fs::read_to_string(cpufreq.join(file)).ok()?.trim().parse::<u64>().ok());
        let max = ["scaling_max_freq", "cpuinfo_max_freq"]
            .iter()
            .copied()
            .find_map(|file| std::fs::read_to_string(cpufreq.join(file)).ok()?.trim().parse::<u64>().ok());
        if let Some(current) = current {
            current_total += current;
            current_count += 1;
        }
        if let Some(max) = max {
            maximum = maximum.max(max);
        }
    }
    let current_mhz = (current_count > 0).then(|| (current_total / current_count / 1000) as u32);
    let maximum_mhz = (maximum > 0).then(|| (maximum / 1000) as u32);
    (current_mhz, maximum_mhz)
}

fn read_cpu_usage(previous: &mut Option<(u64, u64)>) -> Option<u8> {
    let contents = std::fs::read_to_string("/proc/stat").ok()?;
    let line = contents.lines().next()?;
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields.take(8).map(str::parse).collect::<Result<_, _>>().ok()?;
    if values.len() < 5 {
        return None;
    }
    let total = values.iter().copied().sum::<u64>();
    let idle = values[3].saturating_add(values[4]);
    let current = (total, idle);
    let last = previous.replace(current)?;
    let total_delta = total.saturating_sub(last.0);
    if total_delta == 0 {
        return None;
    }
    let busy_delta = total_delta.saturating_sub(idle.saturating_sub(last.1));
    Some(((busy_delta.saturating_mul(100) / total_delta).min(100)) as u8)
}

fn read_system_memory() -> Option<(u64, u64)> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kib = None;
    let mut available_kib = None;
    for line in contents.lines() {
        let (name, value) = line.split_once(':')?;
        let kib = value.split_whitespace().next()?.parse::<u64>().ok()?;
        match name {
            "MemTotal" => total_kib = Some(kib),
            "MemAvailable" => available_kib = Some(kib),
            _ => {}
        }
    }
    let total = total_kib? / 1024;
    let available = available_kib? / 1024;
    Some((total.saturating_sub(available), total))
}

fn read_root_storage() -> Option<(u64, u64)> {
    let path = CString::new("/").ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    let stats = unsafe { stats.assume_init() };
    let block_size = if stats.f_frsize > 0 {
        stats.f_frsize as u64
    } else {
        stats.f_bsize as u64
    };
    let total_bytes = (stats.f_blocks as u64).saturating_mul(block_size);
    let available_bytes = (stats.f_bavail as u64)
        .saturating_mul(block_size)
        .min(total_bytes);
    let mib = 1024_u64.pow(2);
    Some((total_bytes.saturating_sub(available_bytes) / mib, total_bytes / mib))
}

fn parse_nvidia_metric(value: &str) -> Option<u64> {
    let value = value.trim().trim_matches(|character| character == '[' || character == ']');
    if value.eq_ignore_ascii_case("N/A") || value.is_empty() {
        return None;
    }
    value.split_whitespace().next()?.parse().ok()
}

fn read_nvidia_telemetry() -> (Option<u32>, Option<u8>, Option<(u64, u64)>) {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=clocks.current.graphics,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
            "-i",
            "0",
        ])
        .output()
        .ok();
    let Some(output) = output.filter(|output| output.status.success()) else {
        return (None, None, None);
    };
    let Some(line) = String::from_utf8_lossy(&output.stdout).lines().next().map(str::to_owned) else {
        return (None, None, None);
    };
    let fields: Vec<_> = line.split(',').map(str::trim).collect();
    if fields.len() != 4 {
        return (None, None, None);
    }
    let clock = parse_nvidia_metric(fields[0]).map(|value| value as u32);
    let usage = parse_nvidia_metric(fields[1]).map(|value| value.min(100) as u8);
    let memory = match (parse_nvidia_metric(fields[2]), parse_nvidia_metric(fields[3])) {
        (Some(used), Some(total)) => Some((used, total)),
        _ => None,
    };
    (clock, usage, memory)
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
        std::thread::spawn(move || {
            let mut previous_cpu = None;
            loop {
                let (cpu_clock_mhz, cpu_clock_max_mhz) = read_cpu_clock();
                let (gpu_clock_mhz, gpu_usage_percent, gpu_memory_mib) = read_nvidia_telemetry();
                let snapshot = TelemetrySnapshot {
                    cpu_temperature: libacercontrol::telemetry::cpu_temperature().ok(),
                    gpu_temperature: libacercontrol::telemetry::gpu_temperature().ok(),
                    cpu_fan_rpm: libacercontrol::telemetry::cpu_fan_rpm().ok(),
                    gpu_fan_rpm: libacercontrol::telemetry::gpu_fan_rpm().ok(),
                    cpu_clock_mhz,
                    cpu_clock_max_mhz,
                    cpu_usage_percent: read_cpu_usage(&mut previous_cpu),
                    gpu_clock_mhz,
                    gpu_usage_percent,
                    gpu_memory_mib,
                    system_memory_mib: read_system_memory(),
                    root_storage_mib: read_root_storage(),
                    sampled_at: Some(Instant::now()),
                };
                if telemetry_sender.send(snapshot).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
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
                let arc_points = |start: f32, end: f32| {
                    (0..=20)
                        .map(|step| {
                            let position = start + (end - start) * step as f32 / 20.0;
                            let angle = std::f32::consts::PI * (1.0 - position);
                            egui::pos2(
                                center.x + radius * angle.cos(),
                                center.y - radius * angle.sin(),
                            )
                        })
                        .collect::<Vec<_>>()
                };
                painter.add(egui::Shape::line(
                    arc_points(0.0, 1.0),
                    Stroke::new(9.0, Color32::from_rgb(54, 62, 69)),
                ));
                for (start, end, color) in [(0.0, 0.68, COOL), (0.68, 0.86, WARM), (0.86, 1.0, HOT)] {
                    let visible_end = fraction.unwrap_or(0.0).min(end);
                    if visible_end > start {
                        painter.add(egui::Shape::line(
                            arc_points(start, visible_end),
                            Stroke::new(9.0, color),
                        ));
                    }
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

    fn dashboard_dial(ui: &mut egui::Ui, telemetry: &TelemetrySnapshot) {
        egui::Frame::new()
            .fill(Color32::from_rgb(8, 13, 19))
            .stroke(Stroke::new(1.0, Color32::from_rgb(40, 65, 75)))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.label(RichText::new("CPU CLOCK SPEED  ·  SCALE IN GHz").small().strong().color(MUTED));
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 254.0),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(rect);
                let center = egui::pos2(rect.center().x, rect.top() + 151.0);
                let radius = (rect.width() * 0.37).clamp(74.0, 116.0);
                let points = |fraction: f32| {
                    let angle = std::f32::consts::PI * (1.0 - fraction);
                    egui::pos2(
                        center.x + radius * angle.cos(),
                        center.y - radius * angle.sin(),
                    )
                };
                let arc = |end: f32| {
                    (0..=28)
                        .map(|step| points(end * step as f32 / 28.0))
                        .collect::<Vec<_>>()
                };
                let thermal_radius = radius + 9.0;
                let thermal_point = |fraction: f32| {
                    let angle = std::f32::consts::PI * (1.0 - fraction);
                    egui::pos2(
                        center.x + thermal_radius * angle.cos(),
                        center.y - thermal_radius * angle.sin(),
                    )
                };
                let thermal_arc = |start: f32, end: f32| {
                    (0..=24)
                        .map(|step| thermal_point(start + (end - start) * step as f32 / 24.0))
                        .collect::<Vec<_>>()
                };
                painter.add(egui::Shape::line(
                    thermal_arc(0.0, 1.0),
                    Stroke::new(3.0, Color32::from_rgb(36, 46, 55)),
                ));
                if let Some(temperature) = telemetry.cpu_temperature {
                    let fraction = (temperature as f32 / 100.0).clamp(0.0, 1.0);
                    for (start, end, color) in [(0.0, 0.68, COOL), (0.68, 0.86, WARM), (0.86, 1.0, HOT)] {
                        let visible_end = fraction.min(end);
                        if visible_end > start {
                            painter.add(egui::Shape::line(
                                thermal_arc(start, visible_end),
                                Stroke::new(4.0, color),
                            ));
                        }
                    }
                }
                painter.add(egui::Shape::line(
                    arc(1.0),
                    Stroke::new(8.0, Color32::from_rgb(35, 48, 59)),
                ));
                for mark in 0..=4 {
                    let fraction = mark as f32 / 4.0;
                    let point = points(fraction);
                    let direction = egui::vec2(point.x - center.x, point.y - center.y);
                    let inner = egui::pos2(center.x + direction.x * 0.88, center.y + direction.y * 0.88);
                    let outer = egui::pos2(center.x + direction.x * 1.02, center.y + direction.y * 1.02);
                    painter.line_segment([inner, outer], Stroke::new(1.5, MUTED));
                    let label = telemetry.cpu_clock_max_mhz
                        .map(|max| format!("{:.1}", max as f32 * fraction / 1000.0))
                        .unwrap_or_else(|| "—".to_owned());
                    let label_pos = egui::pos2(center.x + direction.x * 1.17, center.y + direction.y * 1.17);
                    painter.text(label_pos, egui::Align2::CENTER_CENTER, label,
                        egui::FontId::proportional(9.0), MUTED);
                }
                let clock_fraction = match (telemetry.cpu_clock_mhz, telemetry.cpu_clock_max_mhz) {
                    (Some(current), Some(maximum)) if maximum > 0 => {
                        Some((current as f32 / maximum as f32).clamp(0.0, 1.0))
                    }
                    _ => None,
                };
                if let Some(fraction) = clock_fraction {
                    let active_arc = arc(fraction);
                    painter.add(egui::Shape::line(
                        active_arc.clone(),
                        Stroke::new(17.0, Color32::from_rgba_unmultiplied(43, 204, 220, 42)),
                    ));
                    painter.add(egui::Shape::line(
                        active_arc,
                        Stroke::new(7.0, ACCENT),
                    ));
                    let needle = points(fraction);
                    let needle_tip = egui::pos2(
                        center.x + (needle.x - center.x) * 0.82,
                        center.y + (needle.y - center.y) * 0.82,
                    );
                    painter.line_segment([center, needle_tip], Stroke::new(5.0, Color32::from_rgb(19, 69, 81)));
                    painter.line_segment([center, needle_tip], Stroke::new(1.8, Color32::from_rgb(218, 250, 250)));
                }
                painter.circle_filled(center, 8.0, Color32::from_rgb(12, 24, 31));
                painter.circle_stroke(center, 8.0, Stroke::new(2.0, ACCENT));
                painter.circle_filled(center, 2.5, TEXT);
                let clock_text = telemetry
                    .cpu_clock_mhz
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".to_owned());
                painter.text(
                    egui::pos2(center.x, center.y - 24.0),
                    egui::Align2::CENTER_CENTER,
                    "CPU",
                    egui::FontId::proportional(14.0),
                    MUTED,
                );
                painter.text(
                    egui::pos2(center.x, center.y + 7.0),
                    egui::Align2::CENTER_CENTER,
                    clock_text,
                    egui::FontId::proportional(27.0),
                    TEXT,
                );
                painter.text(
                    egui::pos2(center.x, center.y + 31.0),
                    egui::Align2::CENTER_CENTER,
                    "MHz average",
                    egui::FontId::proportional(10.0),
                    MUTED,
                );
                let temperature = telemetry.cpu_temperature
                    .map(|value| format!("CPU temperature  {value} °C"))
                    .unwrap_or_else(|| "CPU temperature  Unavailable".to_owned());
                painter.text(
                    egui::pos2(rect.center().x, rect.bottom() - 8.0),
                    egui::Align2::CENTER_BOTTOM,
                    temperature,
                    egui::FontId::proportional(12.0),
                    telemetry.cpu_temperature.map(|value| {
                        if value >= 85 { HOT } else if value >= 70 { WARM } else { COOL }
                    }).unwrap_or(MUTED),
                );
            });
    }

    fn progress_meter(ui: &mut egui::Ui, label: &str, value: Option<u8>, detail: String) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).small().color(MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(detail).small().strong().color(TEXT));
            });
        });
        if let Some(value) = value {
            ui.add(
                egui::ProgressBar::new(value as f32 / 100.0)
                    .fill(ACCENT)
                    .desired_height(8.0),
            );
        } else {
            ui.label(RichText::new("Telemetry unavailable").weak().small());
        }
    }

    pub(crate) fn configure_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.dark_mode = true;
        visuals.override_text_color = Some(TEXT);
        visuals.window_fill = SURFACE;
        visuals.panel_fill = APP_BG;
        visuals.extreme_bg_color = Color32::from_rgb(5, 7, 10);
        visuals.faint_bg_color = Color32::from_rgb(11, 16, 21);
        visuals.selection.bg_fill = Color32::from_rgb(16, 57, 66);
        visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.noninteractive.bg_fill = SURFACE;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(20, 28, 36);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.active.bg_fill = Color32::from_rgb(19, 67, 76);
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
                        && gpu.starts_with("NVIDIA software-requested limit read back");
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
        egui::TopBottomPanel::top("acer-control-toolbar")
            .frame(egui::Frame::new().fill(SIDEBAR_BG).stroke(Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(18, 8)))
            .show(ctx, |ui| {
                ui.columns(3, |columns| {
                    columns[0].horizontal(|ui| {
                        ui.label(RichText::new("◢ ACER").size(16.0).strong().color(ACCENT));
                        ui.label(RichText::new("DEVICE CONTROL").small().color(MUTED));
                    });
                    columns[1].vertical_centered(|ui| {
                        ui.heading(RichText::new("Control Center").size(20.0).color(TEXT));
                        ui.label(RichText::new(self.page.description()).small().color(MUTED));
                    });
                    columns[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{}  ·  v{}", self.page.title(), env!("CARGO_PKG_VERSION"))).small().color(MUTED));
                    });
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for page in Page::ALL {
                        let selected = self.page == page;
                        let button = egui::Button::new(
                            RichText::new(format!("{}  {}", page.icon(), page.title()))
                                .color(if selected { Color32::from_rgb(178, 237, 230) } else { TEXT }),
                        )
                        .selected(selected)
                        .fill(if selected { Color32::from_rgb(21, 55, 61) } else { Color32::TRANSPARENT })
                        .stroke(Stroke::new(1.0, if selected { ACCENT } else { BORDER }))
                        .min_size(egui::vec2(94.0, 36.0));
                        if ui.add(button).clicked() {
                            self.page = page;
                        }
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(APP_BG).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}
