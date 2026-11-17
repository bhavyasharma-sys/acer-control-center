
use eframe::egui;
use egui::{Color32, RichText, Stroke};
use std::ffi::CString;
use std::process::{Command, Stdio};
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

const APP_BG: Color32 = Color32::from_rgb(6, 9, 13);
const SIDEBAR_BG: Color32 = Color32::from_rgb(9, 13, 19);
const SURFACE: Color32 = Color32::from_rgb(14, 21, 29);
const SURFACE_HOVER: Color32 = Color32::from_rgb(21, 33, 43);
const CARD_BG: Color32 = Color32::from_rgb(8, 13, 19);
const BORDER: Color32 = Color32::from_rgb(31, 50, 61);
const TEXT: Color32 = Color32::from_rgb(231, 240, 245);
const MUTED: Color32 = Color32::from_rgb(133, 154, 164);
const ACCENT: Color32 = Color32::from_rgb(43, 204, 220);
const ACCENT_DIM: Color32 = Color32::from_rgb(18, 52, 60);
const COOL: Color32 = Color32::from_rgb(59, 224, 179);
const WARM: Color32 = Color32::from_rgb(255, 188, 75);
const HOT: Color32 = Color32::from_rgb(255, 79, 96);

/// Power-mode tab labels, in profile order. Index matches
/// `power_profiles::DEFAULTS`.
const POWER_MODE_NAMES: [&str; 3] = ["Performance", "Balanced", "Battery Saver"];

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

/// A hardware action for the background worker thread.
///
/// Every D-Bus / driver call runs on the worker, never on the GUI thread,
/// so a slow daemon, a busy GPU, or a hung `nvidia-smi` cannot freeze tab
/// switching or window interaction.
enum HardwareJob {
    /// Read the daemon-reported keyboard/fan/animation state and, when
    /// requested, (re)write the login autostart entry. Both used to happen
    /// synchronously on the GUI thread before the first frame was painted.
    LoadInitialState { enable_autostart: bool },
    LoadAboutInfo,
    SetFanMode(components::FanMode),
    SetKeyboardLighting { brightness: u8, color: Color32 },
    SetAnimation(components::AnimationMode),
    /// Persist power profiles (config file write) off the GUI thread.
    SaveProfiles { profiles: [[u32; 2]; 3], index: usize },
    /// Persist the inactivity timeout, refresh login autostart, and
    /// optionally start or stop the background tray service.
    SaveIdleTimeout {
        seconds: u32,
        minimize_to_tray: bool,
        start_service: bool,
        stop_service: bool,
    },
    /// Persist the minimize-to-tray preference with the same handling.
    SaveTrayPreference {
        enabled: bool,
        idle_timeout_seconds: u32,
        start_service: bool,
        stop_service: bool,
    },
}

/// The worker's reply to a [`HardwareJob`].
enum HardwareResult {
    InitialState {
        brightness: u8,
        color: Color32,
        fan_mode: components::FanMode,
        fan_mode_confirmed: bool,
        animation_mode: components::AnimationMode,
        animation_mode_confirmed: bool,
        autostart_error: Option<String>,
    },
    AboutInfo(about::AboutInfo),
    FanMode {
        mode: components::FanMode,
        result: Result<(), String>,
    },
    KeyboardLighting {
        brightness: u8,
        color: Color32,
        result: Result<(), String>,
    },
    Animation {
        mode: components::AnimationMode,
        result: Result<(), String>,
    },
    ProfilesSaved(Result<(), String>),
    IdleTimeoutSaved {
        seconds: u32,
        result: Result<(), String>,
        /// `Some` only when a service start was requested.
        service_started: Option<Result<(), String>>,
        /// Whether the background service was asked to quit.
        service_stopped: bool,
    },
    TrayPreferenceSaved {
        enabled: bool,
        result: Result<(), String>,
        service_started: Option<Result<(), String>>,
        service_stopped: bool,
    },
}

/// Persist a preference, then optionally start/stop the background service.
/// Returns (save result, service-start result if requested, whether a stop
/// was issued). A failed save never touches the service.
fn save_preference_and_sync_service(
    save: impl FnOnce() -> Result<(), String>,
    start_service: bool,
    stop_service: bool,
) -> (Result<(), String>, Option<Result<(), String>>, bool) {
    let result = save();
    if result.is_err() {
        return (result, None, false);
    }
    let started = start_service.then(instance::start_background_service);
    let stopped = stop_service && started.is_none();
    if stopped {
        instance::request_background_quit();
    }
    (result, started, stopped)
}

fn run_hardware_job(job: HardwareJob) -> HardwareResult {
    match job {
        HardwareJob::LoadInitialState { enable_autostart } => {
            let autostart_error = keyboard_idle::set_login_autostart(enable_autostart).err();
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
            HardwareResult::InitialState {
                brightness,
                color,
                fan_mode,
                fan_mode_confirmed,
                animation_mode,
                animation_mode_confirmed,
                autostart_error,
            }
        }
        HardwareJob::LoadAboutInfo => HardwareResult::AboutInfo(about::AboutInfo::collect()),
        HardwareJob::SetFanMode(mode) => {
            let result = match mode {
                components::FanMode::Auto => libacercontrol::dbus::AcerControlDBus::new()
                    .and_then(|bus| bus.set_fan_auto()),
                components::FanMode::Max => libacercontrol::dbus::AcerControlDBus::new()
                    .and_then(|bus| bus.set_fan_max()),
            };
            HardwareResult::FanMode { mode, result }
        }
        HardwareJob::SetKeyboardLighting { brightness, color } => {
            let result = libacercontrol::dbus::AcerControlDBus::new().and_then(|bus| {
                bus.set_keyboard_brightness(brightness)?;
                bus.set_keyboard_color(color.r(), color.g(), color.b())
            });
            HardwareResult::KeyboardLighting {
                brightness,
                color,
                result,
            }
        }
        HardwareJob::SetAnimation(mode) => {
            let result = match mode {
                components::AnimationMode::Off => libacercontrol::dbus::AcerControlDBus::new()
                    .and_then(|bus| bus.stop_animation()),
                components::AnimationMode::Breathing => libacercontrol::dbus::AcerControlDBus::new()
                    .and_then(|bus| bus.start_breathing()),
            };
            HardwareResult::Animation { mode, result }
        }
        HardwareJob::SaveProfiles { profiles, index } => {
            HardwareResult::ProfilesSaved(power_profiles::save(profiles, index))
        }
        HardwareJob::SaveIdleTimeout {
            seconds,
            minimize_to_tray,
            start_service,
            stop_service,
        } => {
            let (result, service_started, service_stopped) =
                save_preference_and_sync_service(
                    || {
                        keyboard_idle::save_timeout(seconds).and_then(|()| {
                            keyboard_idle::set_login_autostart(seconds > 0 || minimize_to_tray)
                        })
                    },
                    start_service,
                    stop_service,
                );
            HardwareResult::IdleTimeoutSaved {
                seconds,
                result,
                service_started,
                service_stopped,
            }
        }
        HardwareJob::SaveTrayPreference {
            enabled,
            idle_timeout_seconds,
            start_service,
            stop_service,
        } => {
            let (result, service_started, service_stopped) =
                save_preference_and_sync_service(
                    || {
                        keyboard_idle::save_minimize_to_tray(enabled).and_then(|()| {
                            keyboard_idle::set_login_autostart(enabled || idle_timeout_seconds > 0)
                        })
                    },
                    start_service,
                    stop_service,
                );
            HardwareResult::TrayPreferenceSaved {
                enabled,
                result,
                service_started,
                service_stopped,
            }
        }
    }
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

/// Parse the aggregate `cpu` line of `/proc/stat` into (total, idle) ticks.
///
/// Fields are user, nice, system, idle, iowait, irq, softirq, steal, ...
/// Idle ticks are idle + iowait. Returns `None` for malformed input so the
/// caller reports "Unavailable" instead of a fabricated percentage.
fn parse_cpu_usage_sample(line: &str) -> Option<(u64, u64)> {
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
    Some((total, idle))
}

/// Busy percentage between two samples. `None` when there is no previous
/// sample yet or the counters did not advance.
fn cpu_usage_percent(previous: (u64, u64), current: (u64, u64)) -> Option<u8> {
    let total_delta = current.0.saturating_sub(previous.0);
    if total_delta == 0 {
        return None;
    }
    let idle_delta = current.1.saturating_sub(previous.1);
    let busy_delta = total_delta.saturating_sub(idle_delta);
    Some(((busy_delta.saturating_mul(100) / total_delta).min(100)) as u8)
}

fn read_cpu_usage(previous: &mut Option<(u64, u64)>) -> Option<u8> {
    let contents = std::fs::read_to_string("/proc/stat").ok()?;
    let line = contents.lines().next()?;
    let current = parse_cpu_usage_sample(line)?;
    let last = previous.replace(current)?;
    cpu_usage_percent(last, current)
}

/// Parse `/proc/meminfo` into (used, total) MiB.
///
/// Lines without a colon or with an unparsable value are skipped rather
/// than aborting the whole read: one odd line must not hide the memory
/// totals that were parsed successfully.
fn parse_meminfo(contents: &str) -> Option<(u64, u64)> {
    let mut total_kib = None;
    let mut available_kib = None;
    for line in contents.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let Some(kib) = value
            .split_whitespace()
            .next()
            .and_then(|number| number.parse::<u64>().ok())
        else {
            continue;
        };
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

fn read_system_memory() -> Option<(u64, u64)> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_meminfo(&contents)
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

/// Parse one `nvidia-smi --query-gpu=clocks.current.graphics,utilization.gpu,
/// memory.used,memory.total --format=csv,noheader,nounits` line into
/// (clock MHz, usage %, (used, total) MiB). Each field is independently
/// optional: an `N/A` field yields `None` for that metric only.
fn parse_nvidia_telemetry_line(
    line: &str,
) -> Option<(Option<u32>, Option<u8>, Option<(u64, u64)>)> {
    let fields: Vec<_> = line.split(',').map(str::trim).collect();
    if fields.len() != 4 {
        return None;
    }
    let clock = parse_nvidia_metric(fields[0]).map(|value| value as u32);
    let usage = parse_nvidia_metric(fields[1]).map(|value| value.min(100) as u8);
    let memory = match (parse_nvidia_metric(fields[2]), parse_nvidia_metric(fields[3])) {
        (Some(used), Some(total)) => Some((used, total)),
        _ => None,
    };
    Some((clock, usage, memory))
}

/// Run `nvidia-smi` with a hard timeout.
///
/// `Command::output()` blocks indefinitely; under GPU load a hung driver
/// call would stall the telemetry thread forever. Killing the child after
/// the timeout keeps the thread (and therefore fresh telemetry) alive.
/// Returns `None` on spawn failure, timeout, or non-zero exit.
fn run_nvidia_smi(args: &[&str], timeout: Duration) -> Option<std::process::Output> {
    let mut child = Command::new("nvidia-smi")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(_) => {
                let output = child.wait_with_output().ok()?;
                return output.status.success().then_some(output);
            }
            None => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

fn read_nvidia_telemetry() -> (Option<u32>, Option<u8>, Option<(u64, u64)>) {
    let output = run_nvidia_smi(
        &[
            "--query-gpu=clocks.current.graphics,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
            "-i",
            "0",
        ],
        Duration::from_secs(3),
    );
    let Some(output) = output else {
        return (None, None, None);
    };
    let Some(line) = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_owned)
    else {
        return (None, None, None);
    };
    parse_nvidia_telemetry_line(&line).unwrap_or((None, None, None))
}

/// Accept only physically plausible temperatures from the driver.
///
/// The DCHU temperature encoding can yield values outside any real sensor
/// range when the firmware has no data; those are reported as
/// "Unavailable" rather than displayed as a bogus reading.
fn sanitize_temperature(value: i32) -> Option<i32> {
    (-20..=125).contains(&value).then_some(value)
}

/// Accept only physically plausible fan speeds (0 = stopped fan is a real
/// reading from a successful ioctl).
fn sanitize_fan_rpm(value: i32) -> Option<i32> {
    (0..=20_000).contains(&value).then_some(value)
}

/// Whether a daemon power-cap message reports a verified readback.
///
/// The daemon prefixes verified results with "CPU RAPL setting read back"
/// / "NVIDIA software-requested limit read back" and failures with
/// "CPU cap not applied" / "GPU cap not applied". Success requires the
/// readback prefix and the absence of the failure marker, so an enforced
/// ceiling or live draw can never be mistaken for an applied cap.
fn is_power_cap_success(message: &str) -> bool {
    message.contains("read back") && !message.contains("not applied")
}

/// Visible power-apply state. Selecting a profile is never "Applied".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PowerApplyState {
    Applying,
    Applied,
    NotApplied,
}

fn power_apply_state(applying: bool, applied_matches: bool) -> PowerApplyState {
    if applying {
        PowerApplyState::Applying
    } else if applied_matches {
        PowerApplyState::Applied
    } else {
        PowerApplyState::NotApplied
    }
}

fn power_apply_state_label(state: PowerApplyState) -> (&'static str, Color32) {
    match state {
        PowerApplyState::Applying => ("APPLYING", WARM),
        PowerApplyState::Applied => ("APPLIED", COOL),
        PowerApplyState::NotApplied => ("NOT APPLIED", WARM),
    }
}

/// Whether the recorded applied caps still match the selected profile.
///
/// Both the profile index and the exact wattages must match: editing the
/// caps of an already-applied profile means the live caps no longer
/// correspond to the selection.
fn applied_matches_selection(
    last_applied: Option<([u32; 2], usize)>,
    index: usize,
    watts: [u32; 2],
) -> bool {
    last_applied
        .map(|(applied_watts, applied_index)| applied_index == index && applied_watts == watts)
        .unwrap_or(false)
}

/// Human-readable selected-vs-applied summary. Pure so it can be unit tested
/// without constructing the eframe app.
fn power_status_text(
    profiles: &[[u32; 2]; 3],
    index: usize,
    last_applied: Option<([u32; 2], usize)>,
) -> String {
    let index = index.min(2);
    let selected = profiles[index];
    let name = POWER_MODE_NAMES[index];
    match last_applied {
        Some((applied, applied_index)) if applied_matches_selection(last_applied, index, selected) => {
            format!(
                "Selected and applied: {} (CPU {} W · GPU {} W)",
                name, selected[0], selected[1]
            )
        }
        Some((applied, applied_index)) => format!(
            "Selected: {} (CPU {} W · GPU {} W) — not applied. Last applied: {} (CPU {} W · GPU {} W)",
            name,
            selected[0],
            selected[1],
            POWER_MODE_NAMES[applied_index.min(2)],
            applied[0],
            applied[1]
        ),
        None => format!(
            "Selected: {} (CPU {} W · GPU {} W) — not applied yet",
            name, selected[0], selected[1]
        ),
    }
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
    /// Caps and profile index of the last fully verified Apply, if any.
    /// Selecting a profile never sets this; only a readback-verified
    /// power-cap application does.
    last_applied_power: Option<([u32; 2], usize)>,
    /// Persistent result of the last power-cap Apply (success, CPU, GPU).
    last_power_result: Option<(bool, String, String)>,
    animation_mode: components::AnimationMode,
    animation_mode_confirmed: bool,
    idle_timeout_seconds: u32,
    minimize_to_tray: bool,
    tray_ready: bool,
    idle_controller: Option<keyboard_idle::KeyboardIdleController>,
    about_info: about::AboutInfo,
    about_loading: bool,
    initial_state_loaded: bool,
    /// Interactive hardware actions (fan, keyboard, preferences).
    hardware_sender: mpsc::Sender<HardwareJob>,
    /// Read-only queries that may block for a long time.
    query_sender: mpsc::Sender<HardwareJob>,
    hardware_receiver: mpsc::Receiver<HardwareResult>,
    /// Debounced keyboard-lighting change waiting to be sent to the worker.
    pending_keyboard: Option<(u8, Color32, Instant)>,
    profiles_saving: bool,
    /// Debounced profile-config write waiting to reach the worker.
    pending_profiles: Option<Instant>,
    pending_fan_mode: Option<components::FanMode>,
    pending_animation: Option<components::AnimationMode>,
    idle_settings_pending: bool,
    tray_settings_pending: bool,
    update_sender: mpsc::Sender<Result<about::LatestRelease, String>>,
    update_receiver: mpsc::Receiver<Result<about::LatestRelease, String>>,
    power_cap_sender: mpsc::Sender<Result<(String, String), String>>,
    power_cap_receiver: mpsc::Receiver<Result<(String, String), String>>,
    power_cap_applying: bool,
    /// The profile (index + watts) currently being applied, captured at
    /// dispatch time so a mid-apply re-selection cannot be mislabelled as
    /// the applied state.
    applying_profile: Option<(usize, [u32; 2])>,
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
        // Start with neutral defaults and load the real state on the
        // hardware worker thread. Blocking D-Bus calls here used to stall
        // the GUI thread before the first frame ("Application Not
        // Responding"), especially when the daemon was busy.
        let brightness = 128;
        let color = Color32::from_rgb(50, 140, 255);
        let (power_profiles, power_profile_index) = power_profiles::load();
        let idle_timeout_seconds = keyboard_idle::load_timeout();
        let stored_minimize_to_tray = keyboard_idle::load_minimize_to_tray();
        // Keep the saved preference visible even if the tray host is slow to
        // appear during login. Availability is tracked separately by tray_ready.
        let minimize_to_tray = stored_minimize_to_tray;
        // The login-autostart entry is (re)written on the worker thread;
        // only the small local preference reads stay here so the very first
        // frame already shows the user's saved profile and timeout.
        let enable_autostart = idle_timeout_seconds > 0 || minimize_to_tray;
        let tray_ready = background_ready;
        let (update_sender, update_receiver) = mpsc::channel();
        let (power_cap_sender, power_cap_receiver) = mpsc::channel();
        let (telemetry_sender, telemetry_receiver) = mpsc::channel();
        let (hardware_sender, job_receiver) = mpsc::channel::<HardwareJob>();
        let (query_sender, query_receiver) = mpsc::channel::<HardwareJob>();
        let (result_sender, hardware_receiver) = mpsc::channel::<HardwareResult>();
        // Two workers: interactive hardware actions on one, potentially slow
        // read-only queries (daemon state, driver/system information) on the
        // other. A hung or busy query can then never delay a fan, keyboard,
        // or preference action, and neither can ever touch the GUI thread.
        let action_result_sender = result_sender.clone();
        std::thread::spawn(move || {
            while let Ok(job) = job_receiver.recv() {
                let result = run_hardware_job(job);
                // The receiver is dropped only when the GUI exits; a send
                // failure simply ends the worker.
                if action_result_sender.send(result).is_err() {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            while let Ok(job) = query_receiver.recv() {
                let result = run_hardware_job(job);
                if result_sender.send(result).is_err() {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let mut previous_cpu = None;
            loop {
                let (cpu_clock_mhz, cpu_clock_max_mhz) = read_cpu_clock();
                let (gpu_clock_mhz, gpu_usage_percent, gpu_memory_mib) = read_nvidia_telemetry();
                let snapshot = TelemetrySnapshot {
                    cpu_temperature: libacercontrol::telemetry::cpu_temperature()
                        .ok()
                        .and_then(sanitize_temperature),
                    gpu_temperature: libacercontrol::telemetry::gpu_temperature()
                        .ok()
                        .and_then(sanitize_temperature),
                    cpu_fan_rpm: libacercontrol::telemetry::cpu_fan_rpm()
                        .ok()
                        .and_then(sanitize_fan_rpm),
                    gpu_fan_rpm: libacercontrol::telemetry::gpu_fan_rpm()
                        .ok()
                        .and_then(sanitize_fan_rpm),
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
        // Kick off the slow initial reads (D-Bus state + system info) on the
        // worker so the first frame paints immediately.
        let _ = query_sender.send(HardwareJob::LoadInitialState { enable_autostart });
        let _ = query_sender.send(HardwareJob::LoadAboutInfo);
        Self {
            page: Page::Dashboard,
            brightness,
            color,
            notice: (!tray_ready && stored_minimize_to_tray).then(|| {
                (
                    false,
                    "Minimize to tray remains enabled, but the tray service is unavailable. The window may stay open until the desktop tray is ready.".to_owned(),
                    Instant::now(),
                )
            }),
            fan_mode: components::FanMode::Auto,
            fan_mode_confirmed: false,
            power_profiles,
            power_profile_index,
            last_applied_power: None,
            last_power_result: None,
            animation_mode: components::AnimationMode::Off,
            animation_mode_confirmed: false,
            idle_timeout_seconds,
            minimize_to_tray,
            tray_ready,
            idle_controller: (!background_ready && idle_timeout_seconds > 0).then(|| {
                keyboard_idle::KeyboardIdleController::new(idle_timeout_seconds, brightness)
            }),
            about_info: about::AboutInfo {
                rows: vec![(
                    "Status".to_owned(),
                    "Loading system information…".to_owned(),
                )],
            },
            about_loading: true,
            initial_state_loaded: false,
            hardware_sender,
            query_sender,
            hardware_receiver,
            pending_keyboard: None,
            profiles_saving: false,
            pending_profiles: None,
            pending_fan_mode: None,
            pending_animation: None,
            idle_settings_pending: false,
            tray_settings_pending: false,
            update_sender,
            update_receiver,
            power_cap_sender,
            power_cap_receiver,
            power_cap_applying: false,
            applying_profile: None,
            telemetry_receiver,
            telemetry: TelemetrySnapshot::default(),
            update_checking: false,
            update_status: "Updates are checked only when you ask.".to_owned(),
            latest_release: None,
        }
    }

    /// Queue an interactive hardware action. Never blocks the GUI thread.
    fn dispatch(&self, job: HardwareJob) {
        let _ = self.hardware_sender.send(job);
    }

    /// Queue a read-only query on the separate query worker, so a slow or
    /// hung query cannot delay pending interactive actions.
    fn dispatch_query(&self, job: HardwareJob) {
        let _ = self.query_sender.send(job);
    }

    /// Apply one worker result to the visible state.
    fn handle_hardware_result(&mut self, result: HardwareResult) {
        match result {
            HardwareResult::InitialState {
                brightness,
                color,
                fan_mode,
                fan_mode_confirmed,
                animation_mode,
                animation_mode_confirmed,
                autostart_error,
            } => {
                self.brightness = brightness;
                self.color = color;
                self.fan_mode = fan_mode;
                self.fan_mode_confirmed = fan_mode_confirmed;
                self.animation_mode = animation_mode;
                self.animation_mode_confirmed = animation_mode_confirmed;
                self.initial_state_loaded = true;
                if let Some(error) = autostart_error {
                    self.set_notice(
                        false,
                        format!("Could not configure login startup for keyboard inactivity: {error}"),
                    );
                }
            }
            HardwareResult::AboutInfo(info) => {
                self.about_info = info;
                self.about_loading = false;
            }
            HardwareResult::FanMode { mode, result } => {
                self.pending_fan_mode = None;
                match result {
                    Ok(()) => {
                        self.fan_mode = mode;
                        self.fan_mode_confirmed = true;
                        self.set_notice(true, "Fan mode applied".to_owned());
                    }
                    Err(error) => {
                        self.set_notice(false, format!("Fan mode failed: {error}"));
                    }
                }
            }
            HardwareResult::KeyboardLighting {
                brightness,
                color,
                result,
            } => match result {
                Ok(()) => {
                    self.brightness = brightness;
                    self.color = color;
                    if let Some(controller) = &self.idle_controller {
                        controller.set_brightness(brightness);
                    }
                    self.animation_mode = components::AnimationMode::Off;
                    self.animation_mode_confirmed = true;
                    self.set_notice(true, "Keyboard lighting applied".to_owned());
                }
                Err(error) => {
                    self.set_notice(false, format!("Keyboard lighting failed: {error}"));
                }
            },
            HardwareResult::Animation { mode, result } => {
                self.pending_animation = None;
                match result {
                    Ok(()) => {
                        self.animation_mode = mode;
                        self.animation_mode_confirmed = true;
                        self.set_notice(true, "Keyboard animation applied".to_owned());
                    }
                    Err(error) => {
                        self.set_notice(false, format!("Keyboard animation failed: {error}"));
                    }
                }
            }
            HardwareResult::ProfilesSaved(result) => {
                self.profiles_saving = false;
                if let Err(error) = result {
                    self.set_notice(false, format!("Could not save power profile: {error}"));
                }
            }
            HardwareResult::IdleTimeoutSaved {
                seconds,
                result,
                service_started,
                service_stopped,
            } => {
                self.idle_settings_pending = false;
                match result {
                    Ok(()) => {
                        self.idle_timeout_seconds = seconds;
                        if let Some(start) = service_started {
                            match start {
                                Ok(()) => {
                                    self.tray_ready = true;
                                    self.idle_controller = None;
                                }
                                Err(error) => {
                                    self.idle_controller = Some(
                                        keyboard_idle::KeyboardIdleController::new(
                                            seconds,
                                            self.brightness,
                                        ),
                                    );
                                    self.set_notice(false, format!(
                                        "Inactivity dimming is active only while this window stays open because the tray service could not start: {error}"
                                    ));
                                }
                            }
                        } else if service_stopped {
                            self.tray_ready = false;
                            self.idle_controller = None;
                        } else if let Some(controller) = &self.idle_controller {
                            controller.set_timeout(seconds);
                        }
                        if self.notice.as_ref().is_none_or(|(ok, _, _)| *ok) {
                            self.set_notice(
                                true,
                                if seconds > 0 {
                                    "Inactivity timeout saved. The tray service keeps it running after the window closes.".to_owned()
                                } else {
                                    "Inactivity timeout disabled and login startup removed.".to_owned()
                                },
                            );
                        }
                    }
                    Err(error) => {
                        self.set_notice(
                            false,
                            format!("Could not save inactivity settings: {error}"),
                        );
                    }
                }
            }
            HardwareResult::TrayPreferenceSaved {
                enabled,
                result,
                service_started,
                service_stopped,
            } => {
                self.tray_settings_pending = false;
                match result {
                    Ok(()) => {
                        self.minimize_to_tray = enabled;
                        if let Some(start) = service_started {
                            match start {
                                Ok(()) => {
                                    self.tray_ready = true;
                                    self.idle_controller = None;
                                }
                                Err(error) => self.set_notice(false, format!(
                                    "Minimize to tray is saved and will be retried at login, but the tray service could not start: {error}"
                                )),
                            }
                        } else if service_stopped {
                            self.tray_ready = false;
                        }
                        if self.tray_ready || !enabled {
                            self.set_notice(
                                true,
                                if enabled {
                                    "Closing the window will hide it while the background service remains in the system tray.".to_owned()
                                } else {
                                    "Closing the window will exit Acer Control Center when no background timeout is active.".to_owned()
                                },
                            );
                        }
                    }
                    Err(error) => {
                        self.set_notice(false, format!("Could not save tray setting: {error}"));
                    }
                }
            }
        }
    }

    /// Short status line describing selected vs applied power caps.
    fn power_status_line(&self) -> String {
        power_status_text(
            &self.power_profiles,
            self.power_profile_index,
            self.last_applied_power,
        )
    }

    /// Queue a power-cap apply on a worker thread and remember which profile
    /// the request belongs to. Never blocks the GUI thread: `set_power_caps`
    /// runs `nvidia-smi` and can take seconds under GPU load.
    fn request_power_apply(&mut self) {
        if self.power_cap_applying {
            return;
        }
        let index = self.power_profile_index;
        let [cpu_watts, gpu_watts] = self.power_profiles[index];
        self.applying_profile = Some((index, [cpu_watts, gpu_watts]));
        self.power_cap_applying = true;
        self.last_power_result = None;
        self.set_notice(
            true,
            format!(
                "Applying {} caps: CPU {} W · GPU {} W…",
                POWER_MODE_NAMES[index], cpu_watts, gpu_watts
            ),
        );
        let sender = self.power_cap_sender.clone();
        std::thread::spawn(move || {
            let result = libacercontrol::dbus::AcerControlDBus::new()
                .and_then(|bus| bus.set_power_caps(cpu_watts, gpu_watts));
            let _ = sender.send(result);
        });
    }

    /// Select a power profile. Selection only changes the visible target and
    /// queues a config write; it never applies hardware caps.
    fn select_power_profile(&mut self, index: usize) {
        if index > 2 || index == self.power_profile_index {
            return;
        }
        self.power_profile_index = index;
        self.queue_profile_save(false);
    }

    /// Queue (debounced) or immediately dispatch the profile config write on
    /// the worker thread, keeping filesystem access off the GUI thread.
    fn queue_profile_save(&mut self, immediate: bool) {
        if immediate {
            self.pending_profiles = None;
            self.profiles_saving = true;
            self.dispatch(HardwareJob::SaveProfiles {
                profiles: self.power_profiles,
                index: self.power_profile_index,
            });
        } else {
            self.pending_profiles = Some(Instant::now());
        }
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
                    egui::FontId::proportional(11.0), MUTED);
                painter.text(egui::pos2(center.x - radius - 2.0, center.y + 12.0), egui::Align2::LEFT_CENTER, "0",
                    egui::FontId::proportional(11.0), MUTED);
                painter.text(egui::pos2(center.x + radius + 2.0, center.y + 12.0), egui::Align2::RIGHT_CENTER,
                    max_value as i32, egui::FontId::proportional(11.0), MUTED);
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

    /// The clock-speed speedometer. Returns the total height of its card so the
    /// dashboard can align it with the cards beside it.
    fn dashboard_dial(
        ui: &mut egui::Ui,
        telemetry: &TelemetrySnapshot,
        height: f32,
        min_height: f32,
    ) -> f32 {
        // `height` is the dial's minimum canvas height, chosen from the centre
        // column width so the speedometer keeps its proportions. `min_height`
        // is the content height of the row, so the card ends on the same
        // baseline as its neighbours; extra space is absorbed by the canvas,
        // with the readouts pinned to the bottom of it.
        egui::Frame::new()
            .fill(CARD_BG)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(12.0)
            // Same margins as `card`, so both have the same chrome height.
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(min_height.max(0.0));
                let header = ui.label(
                    RichText::new("CPU CLOCK SPEED  ·  SCALE IN GHz")
                        .small()
                        .strong()
                        .color(MUTED),
                );
                // 24 pt covers the item spacing under the header plus a little
                // slack, so the `set_min_height` above decides the card height.
                let canvas = (min_height - header.rect.height() - 24.0).max(height);
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), canvas),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(rect);
                let radius = (rect.width().min(rect.height() * 1.25) * 0.36).clamp(62.0, 116.0);
                let center = egui::pos2(rect.center().x, rect.top() + radius + 40.0);
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
                        egui::FontId::proportional(10.0), MUTED);
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
                    egui::FontId::proportional(11.0),
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
                painter.text(
                    egui::pos2(rect.center().x, rect.bottom() - 24.0),
                    egui::Align2::CENTER_BOTTOM,
                    "Average of online cores · scale from the detected cpufreq maximum",
                    egui::FontId::proportional(10.5),
                    MUTED,
                );
            })
            .response
            .rect
            .height()
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
        // Drain completed hardware jobs. `try_recv` never blocks: if the
        // worker is still stuck on a slow ioctl, D-Bus call, or nvidia-smi
        // the UI keeps painting and stays interactive.
        while let Ok(result) = self.hardware_receiver.try_recv() {
            self.handle_hardware_result(result);
        }
        while let Ok(snapshot) = self.telemetry_receiver.try_recv() {
            self.telemetry = snapshot;
        }
        // Debounce slider drags: only the settled value reaches the daemon,
        // so dragging a brightness slider cannot queue hundreds of blocking
        // D-Bus calls.
        if let Some((brightness, color, queued_at)) = self.pending_keyboard {
            if queued_at.elapsed() >= Duration::from_millis(120) {
                self.pending_keyboard = None;
                self.dispatch(HardwareJob::SetKeyboardLighting { brightness, color });
            }
        }
        if let Some(queued_at) = self.pending_profiles {
            if queued_at.elapsed() >= Duration::from_millis(250) {
                self.pending_profiles = None;
                self.profiles_saving = true;
                self.dispatch(HardwareJob::SaveProfiles {
                    profiles: self.power_profiles,
                    index: self.power_profile_index,
                });
            }
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
            // The applied profile is the one captured when the request was
            // dispatched, not whatever tab/wattage the user selected while
            // the daemon was working.
            let applied = self.applying_profile.take();
            match result {
                Ok((cpu, gpu)) => {
                    let success = is_power_cap_success(&cpu) && is_power_cap_success(&gpu);
                    if success {
                        self.last_applied_power = applied.map(|(index, watts)| (watts, index));
                    }
                    self.last_power_result = Some((success, cpu.clone(), gpu.clone()));
                    self.set_notice(success, format!("{cpu}. {gpu}."));
                }
                Err(error) => {
                    self.last_power_result = Some((false, error.clone(), error.clone()));
                    self.set_notice(
                        false,
                        format!("Could not contact the power-control daemon: {error}"),
                    );
                }
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
        // Notices live in a bottom status bar so an apply result — including
        // an honest driver rejection — is visible no matter which page is
        // shown or how far the content is scrolled. Failures stay longer than
        // successes because they carry detail the user needs to read.
        if self.notice.as_ref().is_some_and(|(ok, _, created)| {
            created.elapsed() >= if *ok {
                Duration::from_secs(4)
            } else {
                Duration::from_secs(14)
            }
        }) {
            self.notice = None;
        }
        if let Some((ok, message, _)) = self.notice.clone() {
            let color = if ok { ACCENT } else { Color32::from_rgb(255, 122, 122) };
            let marker = if ok { "✓" } else { "⚠" };
            egui::TopBottomPanel::bottom("acer-control-notice")
                .frame(
                    egui::Frame::new()
                        .fill(SIDEBAR_BG)
                        .stroke(Stroke::NONE)
                        .inner_margin(egui::Margin::symmetric(18, 9)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(marker).size(13.0).strong().color(color));
                        ui.label(RichText::new(message).size(12.0).color(color));
                    });
                });
        }

        egui::TopBottomPanel::top("acer-control-toolbar")
            .frame(
                egui::Frame::new()
                    .fill(SIDEBAR_BG)
                    .stroke(Stroke::NONE)
                    .inner_margin(egui::Margin::symmetric(18, 10)),
            )
            .show(ctx, |ui| {
                // Title band. Painted directly so the "Control Center" title
                // stays optically centred no matter how wide the left brand
                // or right status text is; at narrow widths the side items
                // are dropped instead of overlapping the title.
                let band = {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 38.0),
                        egui::Sense::hover(),
                    );
                    rect
                };
                let painter = ui.painter_at(band);
                let wide = band.width() >= 620.0;
                if wide {
                    painter.text(
                        band.left_center(),
                        egui::Align2::LEFT_CENTER,
                        "◢ ACER",
                        egui::FontId::proportional(17.0),
                        ACCENT,
                    );
                }
                painter.text(
                    band.center(),
                    egui::Align2::CENTER_CENTER,
                    "Control Center",
                    egui::FontId::proportional(21.0),
                    TEXT,
                );
                // Restrained cyan underline glow beneath the title.
                let glow_half = 96.0_f32.min(band.width() * 0.22);
                painter.line_segment(
                    [
                        egui::pos2(band.center().x - glow_half, band.bottom() - 2.0),
                        egui::pos2(band.center().x + glow_half, band.bottom() - 2.0),
                    ],
                    Stroke::new(4.0, Color32::from_rgba_unmultiplied(43, 204, 220, 40)),
                );
                painter.line_segment(
                    [
                        egui::pos2(band.center().x - glow_half, band.bottom() - 2.0),
                        egui::pos2(band.center().x + glow_half, band.bottom() - 2.0),
                    ],
                    Stroke::new(1.5, ACCENT),
                );
                if wide {
                    let status = self
                        .telemetry
                        .sampled_at
                        .map(|time| {
                            format!(
                                "LIVE · {}s",
                                time.elapsed().as_secs().min(99)
                            )
                        })
                        .unwrap_or_else(|| "WAITING FOR SENSORS".to_owned());
                    painter.text(
                        band.right_center(),
                        egui::Align2::RIGHT_CENTER,
                        format!("{}  ·  v{}", status, env!("CARGO_PKG_VERSION")),
                        egui::FontId::proportional(11.0),
                        MUTED,
                    );
                }

                ui.add_space(6.0);
                // Horizontal icon navigation, centred as a group and
                // wrapping rather than clipping when the window is narrow.
                ui.vertical_centered(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for page in Page::ALL {
                            let selected = self.page == page;
                            let text_color = if selected {
                                Color32::from_rgb(196, 245, 248)
                            } else {
                                MUTED
                            };
                            let button = egui::Button::new(
                                RichText::new(format!("{}  {}", page.icon(), page.title()))
                                    .size(13.0)
                                    .color(text_color),
                            )
                            .selected(selected)
                            .fill(if selected { ACCENT_DIM } else { Color32::TRANSPARENT })
                            .stroke(Stroke::new(
                                1.0,
                                if selected { ACCENT } else { Color32::TRANSPARENT },
                            ))
                            .corner_radius(8.0)
                            .min_size(egui::vec2(86.0, 32.0));
                            let response = ui.add(button);
                            if selected {
                                // Accent bar under the active tab.
                                let rect = response.rect;
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(rect.center().x - 13.0, rect.bottom() - 2.0),
                                        egui::vec2(26.0, 2.0),
                                    ),
                                    1.0,
                                    ACCENT,
                                );
                            }
                            if response.clicked() {
                                self.page = page;
                            }
                            let _ = response.on_hover_text(page.description());
                        }
                    });
                });
                ui.add_space(2.0);
                // min_rect (the used area), not max_rect: inside a top panel
                // max_rect can extend past the content on the first frames.
                let separator = ui.min_rect();
                ui.painter().line_segment(
                    [
                        egui::pos2(separator.left() - 18.0, separator.bottom()),
                        egui::pos2(separator.right() + 18.0, separator.bottom()),
                    ],
                    Stroke::new(1.0, BORDER),
                );
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(APP_BG).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.page_content(ui);
                    });
            });
        // Poll quickly while a background job is outstanding so debounced
        // slider drags and async results land promptly, then relax to the
        // 1 s telemetry cadence. This only schedules a repaint; it never
        // blocks on hardware.
        let busy = self.power_cap_applying
            || self.about_loading
            || !self.initial_state_loaded
            || self.pending_keyboard.is_some()
            || self.pending_profiles.is_some()
            || self.pending_fan_mode.is_some()
            || self.pending_animation.is_some()
            || self.profiles_saving
            || self.idle_settings_pending
            || self.tray_settings_pending
            || self.update_checking;
        let delay = if busy {
            Duration::from_millis(50)
        } else {
            Duration::from_secs(1)
        };
        ctx.request_repaint_after(delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------- /proc/stat parser

    #[test]
    fn test_parse_cpu_usage_sample_valid() {
        // user nice system idle iowait irq softirq steal
        let sample = parse_cpu_usage_sample(
            "cpu  1000 20 300 5000 100 5 10 3 0 0",
        )
        .unwrap();
        assert_eq!(sample.0, 1000 + 20 + 300 + 5000 + 100 + 5 + 10 + 3);
        assert_eq!(sample.1, 5000 + 100);
    }

    #[test]
    fn test_parse_cpu_usage_sample_rejects_bad_input() {
        assert!(parse_cpu_usage_sample("").is_none());
        assert!(parse_cpu_usage_sample("cpu0 1 2 3 4 5").is_none());
        assert!(parse_cpu_usage_sample("cpu 1 2 3").is_none());
        assert!(parse_cpu_usage_sample("cpu a b c d e").is_none());
        assert!(parse_cpu_usage_sample("intr 1 2 3 4 5").is_none());
    }

    #[test]
    fn test_cpu_usage_percent_between_samples() {
        // total grew by 1000, idle grew by 250 -> 75% busy
        assert_eq!(cpu_usage_percent((10_000, 8_000), (11_000, 8_250)), Some(75));
        // fully idle
        assert_eq!(cpu_usage_percent((0, 0), (1_000, 1_000)), Some(0));
        // fully busy
        assert_eq!(cpu_usage_percent((0, 0), (1_000, 0)), Some(100));
    }

    #[test]
    fn test_cpu_usage_percent_never_fabricates_on_stall() {
        // Counters did not advance: report unavailable, not 0%.
        assert_eq!(cpu_usage_percent((1_000, 900), (1_000, 900)), None);
        // Counter went backwards (suspend/resume): the delta saturates to
        // zero, which is reported as unavailable rather than invented.
        assert_eq!(cpu_usage_percent((5_000, 4_000), (1_000, 900)), None);
        // Idle delta larger than total delta saturates to 0 rather than
        // underflowing.
        assert_eq!(cpu_usage_percent((1_000, 500), (1_100, 1_000)), Some(0));
    }

    #[test]
    fn test_first_cpu_sample_only_records_a_baseline() {
        // Mirrors `read_cpu_usage`: the first sample cannot produce a
        // percentage, so the UI shows "Unavailable" rather than 0 %.
        let mut previous: Option<(u64, u64)> = None;
        let first = parse_cpu_usage_sample("cpu  100 0 100 800 0 0 0 0").unwrap();
        assert!(previous.replace(first).is_none());
        let second = parse_cpu_usage_sample("cpu  200 0 200 1400 0 0 0 0").unwrap();
        let baseline = previous.replace(second).unwrap();
        // total delta 800, idle delta 600 -> 200/800 busy = 25 %
        assert_eq!(cpu_usage_percent(baseline, second), Some(25));
    }

    // ------------------------------------------------------ /proc/meminfo parser

    #[test]
    fn test_parse_meminfo_typical() {
        let contents = "MemTotal:       16132956 kB\n\
                        MemFree:         1234567 kB\n\
                        MemAvailable:    8123456 kB\n\
                        Buffers:          123456 kB\n";
        let (used, total) = parse_meminfo(contents).unwrap();
        assert_eq!(total, 16132956 / 1024);
        assert_eq!(used, total - 8123456 / 1024);
    }

    #[test]
    fn test_parse_meminfo_skips_malformed_lines() {
        // A colon-less line or unparsable value must not abort the whole read.
        let contents = "garbage line without colon\n\
                        HugePages_Total:       0\n\
                        MemTotal:       8388608 kB\n\
                        Broken: not-a-number kB\n\
                        MemAvailable:   4194304 kB\n";
        let (used, total) = parse_meminfo(contents).unwrap();
        assert_eq!(total, 8192);
        assert_eq!(used, 4096);
    }

    #[test]
    fn test_parse_meminfo_missing_field_is_unavailable() {
        assert!(parse_meminfo("MemTotal: 8388608 kB\n").is_none());
        assert!(parse_meminfo("MemAvailable: 4194304 kB\n").is_none());
        assert!(parse_meminfo("").is_none());
    }

    #[test]
    fn test_parse_meminfo_used_never_negative() {
        // MemAvailable larger than MemTotal (virtualised hosts) saturates at 0.
        let contents = "MemTotal: 1024 kB\nMemAvailable: 4096 kB\n";
        assert_eq!(parse_meminfo(contents), Some((0, 1)));
    }

    // ------------------------------------------------------- NVIDIA CSV parser

    #[test]
    fn test_parse_nvidia_metric_values() {
        assert_eq!(parse_nvidia_metric("1965"), Some(1965));
        assert_eq!(parse_nvidia_metric(" 42 "), Some(42));
        assert_eq!(parse_nvidia_metric("[75]"), Some(75));
        assert_eq!(parse_nvidia_metric("6144 MiB"), Some(6144));
    }

    #[test]
    fn test_parse_nvidia_metric_unavailable_forms() {
        for text in ["N/A", "[N/A]", "n/a", "", "   ", "[Not Supported]", "[Insufficient Permissions]"] {
            assert_eq!(parse_nvidia_metric(text), None, "input was {text:?}");
        }
    }

    #[test]
    fn test_parse_nvidia_telemetry_line_full() {
        let (clock, usage, memory) =
            parse_nvidia_telemetry_line("1450, 37, 1024, 6144").unwrap();
        assert_eq!(clock, Some(1450));
        assert_eq!(usage, Some(37));
        assert_eq!(memory, Some((1024, 6144)));
    }

    #[test]
    fn test_parse_nvidia_telemetry_line_partial_na() {
        // GPU clock and utilisation are unavailable when the GPU is asleep;
        // memory can still be reported. Nothing may become zero.
        let (clock, usage, memory) =
            parse_nvidia_telemetry_line("[N/A], [N/A], 512, 6144").unwrap();
        assert_eq!(clock, None);
        assert_eq!(usage, None);
        assert_eq!(memory, Some((512, 6144)));
    }

    #[test]
    fn test_parse_nvidia_telemetry_line_memory_requires_both_fields() {
        let (_, _, memory) = parse_nvidia_telemetry_line("1450, 10, [N/A], 6144").unwrap();
        assert_eq!(memory, None);
    }

    #[test]
    fn test_parse_nvidia_telemetry_line_usage_clamped() {
        let (_, usage, _) = parse_nvidia_telemetry_line("1450, 250, 0, 6144").unwrap();
        assert_eq!(usage, Some(100));
    }

    #[test]
    fn test_parse_nvidia_telemetry_line_wrong_field_count() {
        assert!(parse_nvidia_telemetry_line("1450, 37, 1024").is_none());
        assert!(parse_nvidia_telemetry_line("1450, 37, 1024, 6144, 5").is_none());
        assert!(parse_nvidia_telemetry_line("").is_none());
    }

    // ------------------------------------------------------- sensor sanity gates

    #[test]
    fn test_sanitize_temperature_rejects_implausible_values() {
        assert_eq!(sanitize_temperature(45), Some(45));
        assert_eq!(sanitize_temperature(0), Some(0));
        assert_eq!(sanitize_temperature(-5), Some(-5));
        assert_eq!(sanitize_temperature(125), Some(125));
        assert_eq!(sanitize_temperature(126), None);
        assert_eq!(sanitize_temperature(-21), None);
        assert_eq!(sanitize_temperature(255), None);
    }

    #[test]
    fn test_sanitize_fan_rpm_allows_stopped_fan() {
        assert_eq!(sanitize_fan_rpm(0), Some(0));
        assert_eq!(sanitize_fan_rpm(3200), Some(3200));
        assert_eq!(sanitize_fan_rpm(-1), None);
        assert_eq!(sanitize_fan_rpm(65535), None);
    }

    // --------------------------------------------------- power-cap result checks

    #[test]
    fn test_is_power_cap_success_accepts_verified_readback() {
        assert!(is_power_cap_success(
            "CPU RAPL setting read back: 40 W (actual workload enforcement is not measured)"
        ));
        assert!(is_power_cap_success(
            "NVIDIA software-requested limit read back: 60 W; base limit 60.00 W, enforced ceiling 70.00 W"
        ));
    }

    #[test]
    fn test_is_power_cap_success_rejects_daemon_failures() {
        assert!(!is_power_cap_success("CPU cap not applied: RAPL write failed"));
        assert!(!is_power_cap_success(
            "GPU cap not applied: NVIDIA driver rejected the requested power limit: Changing power management limit is not supported in current scope for GPU."
        ));
        // The readback-mismatch failure embeds the phrase "read back"; the
        // "not applied" marker must still win.
        assert!(!is_power_cap_success(
            "GPU cap not applied: NVIDIA software-requested limit read back as 70.00 W, not 75 W"
        ));
    }

    #[test]
    fn test_is_power_cap_success_rejects_unverified_evidence() {
        // An enforced ceiling, a reported maximum, or live draw is not proof.
        assert!(!is_power_cap_success(
            "enforced ceiling 70.00 W, maximum limit 75.00 W, live draw 6.51 W"
        ));
        assert!(!is_power_cap_success(""));
    }

    // -------------------------------------------------------- UI state logic

    #[test]
    fn test_power_apply_state_precedence() {
        assert_eq!(
            power_apply_state(true, true),
            PowerApplyState::Applying
        );
        assert_eq!(
            power_apply_state(true, false),
            PowerApplyState::Applying
        );
        assert_eq!(
            power_apply_state(false, true),
            PowerApplyState::Applied
        );
        assert_eq!(
            power_apply_state(false, false),
            PowerApplyState::NotApplied
        );
    }

    #[test]
    fn test_power_apply_state_labels() {
        assert_eq!(
            power_apply_state_label(PowerApplyState::Applied).0,
            "APPLIED"
        );
        assert_eq!(
            power_apply_state_label(PowerApplyState::NotApplied).0,
            "NOT APPLIED"
        );
        assert_eq!(
            power_apply_state_label(PowerApplyState::Applying).0,
            "APPLYING"
        );
    }

    #[test]
    fn test_applied_matches_selection_requires_index_and_watts() {
        let applied = Some(([40, 75], 0usize));
        assert!(applied_matches_selection(applied, 0, [40, 75]));
        // Same profile, edited caps: no longer applied.
        assert!(!applied_matches_selection(applied, 0, [35, 75]));
        // Same caps, different profile index: no longer applied.
        assert!(!applied_matches_selection(applied, 1, [40, 75]));
        // Nothing applied yet.
        assert!(!applied_matches_selection(None, 0, [40, 75]));
    }

    #[test]
    fn test_power_status_text_selection_is_not_application() {
        let profiles = power_profiles::DEFAULTS;
        let text = power_status_text(&profiles, 0, None);
        assert!(text.contains("Performance"));
        assert!(text.contains("CPU 40 W"));
        assert!(text.contains("GPU 75 W"));
        assert!(text.contains("not applied yet"));
        assert!(!text.contains("Selected and applied"));
    }

    #[test]
    fn test_power_status_text_applied() {
        let profiles = power_profiles::DEFAULTS;
        let text = power_status_text(&profiles, 1, Some(([30, 60], 1)));
        assert!(text.contains("Selected and applied: Balanced"));
        assert!(text.contains("CPU 30 W"));
        assert!(text.contains("GPU 60 W"));
    }

    #[test]
    fn test_power_status_text_stale_application() {
        let profiles = power_profiles::DEFAULTS;
        let text = power_status_text(&profiles, 0, Some(([30, 60], 1)));
        assert!(text.contains("not applied"));
        assert!(text.contains("Last applied: Balanced"));
        assert!(text.contains("CPU 30 W"));
        assert!(!text.contains("Selected and applied"));
    }

    #[test]
    fn test_power_status_text_clamps_out_of_range_index() {
        let profiles = power_profiles::DEFAULTS;
        let text = power_status_text(&profiles, 9, None);
        assert!(text.contains("Battery Saver"));
    }

    #[test]
    fn test_profile_defaults_preserved() {
        assert_eq!(
            power_profiles::DEFAULTS,
            [[40, 75], [30, 60], [15, 30]]
        );
    }

    // ------------------------------------------------------------- navigation

    #[test]
    fn test_page_metadata_is_complete_and_unique() {
        assert_eq!(Page::ALL.len(), 6);
        let mut titles = Vec::new();
        for page in Page::ALL {
            assert!(!page.title().is_empty());
            assert!(!page.description().is_empty());
            assert!(!page.icon().is_empty());
            titles.push(page.title());
        }
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), Page::ALL.len());
    }

    #[test]
    fn test_power_mode_names_match_profile_defaults() {
        assert_eq!(POWER_MODE_NAMES.len(), power_profiles::DEFAULTS.len());
        assert_eq!(POWER_MODE_NAMES[0], "Performance");
        assert_eq!(POWER_MODE_NAMES[1], "Balanced");
        assert_eq!(POWER_MODE_NAMES[2], "Battery Saver");
    }
}
