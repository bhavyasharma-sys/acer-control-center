use super::*;

impl ControlCenter {
    pub(super) fn page_content(&mut self, ui: &mut egui::Ui) {
        match self.page {
            Page::Dashboard => {
                let telemetry = self.telemetry.clone();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("POWER MODE").small().strong().color(MUTED));
                    ui.add_space(10.0);
                    for (index, name) in [(0, "Performance"), (1, "Balanced"), (2, "Battery Saver")] {
                        let selected = self.power_profile_index == index;
                        let button = egui::Button::new(
                            RichText::new(name).color(if selected { ACCENT } else { TEXT }),
                        )
                        .selected(selected)
                        .fill(if selected { Color32::from_rgb(12, 48, 62) } else { SURFACE })
                        .stroke(Stroke::new(1.0, if selected { ACCENT } else { BORDER }));
                        if ui.add(button).clicked() {
                            self.power_profile_index = index;
                            if let Err(error) = super::power_profiles::save(self.power_profiles, index) {
                                self.set_notice(false, format!("Could not save power profile: {error}"));
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("Targets are selected here; apply them in Power Profiles").small().color(MUTED));
                    });
                });
                ui.add_space(12.0);
                ui.columns(3, |columns| {
                    egui::Frame::new().fill(Color32::from_rgb(8, 13, 19))
                        .stroke(Stroke::new(1.0, BORDER)).corner_radius(8.0).inner_margin(12)
                        .show(&mut columns[0], |ui| {
                            ui.label(RichText::new("SYSTEM").small().strong().color(ACCENT));
                            ui.add_space(4.0);
                            ui.label(RichText::new("ACER ASPIRE 7 · A715-79G").strong());
                            ui.label(RichText::new("Core 5 210H · RTX 3050 6 GB").small().color(MUTED));
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label("Average CPU clock");
                                    ui.heading(telemetry.cpu_clock_mhz.map(|v| format!("{v} MHz")).unwrap_or_else(|| "Unavailable".into()));
                                    ui.label(telemetry.cpu_temperature.map(|v| format!("{v} °C temperature")).unwrap_or_else(|| "Temperature unavailable".into()));
                                });
                            });
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label("GPU clock");
                                    ui.heading(telemetry.gpu_clock_mhz.map(|v| format!("{v} MHz")).unwrap_or_else(|| "Unavailable".into()));
                                    ui.label(telemetry.gpu_temperature.map(|v| format!("{v} °C temperature")).unwrap_or_else(|| "Temperature unavailable".into()));
                                });
                            });
                            ui.add_space(12.0);
                            Self::progress_meter(ui, "CPU usage", telemetry.cpu_usage_percent,
                                telemetry.cpu_usage_percent.map(|v| format!("{v} %")).unwrap_or_else(|| "Unavailable".into()));
                            ui.add_space(7.0);
                            Self::progress_meter(ui, "GPU usage", telemetry.gpu_usage_percent,
                                telemetry.gpu_usage_percent.map(|v| format!("{v} %")).unwrap_or_else(|| "Unavailable".into()));
                        });
                    Self::dashboard_dial(&mut columns[1], &telemetry);
                    egui::Frame::new().fill(Color32::from_rgb(8, 13, 19))
                        .stroke(Stroke::new(1.0, BORDER)).corner_radius(8.0).inner_margin(12)
                        .show(&mut columns[2], |ui| {
                            ui.label(RichText::new("FAN SPEED CONTROL").small().strong().color(ACCENT));
                            ui.add_space(6.0);
                            let mode = components::segmented_choice(
                                ui,
                                self.fan_mode,
                                &[(components::FanMode::Auto, "Automatic"), (components::FanMode::Max, "Maximum")],
                            );
                            if let Some(mode) = mode {
                                let result = match mode {
                                    components::FanMode::Auto => self.action("Automatic fan mode", || libacercontrol::dbus::AcerControlDBus::new()?.set_fan_auto()),
                                    components::FanMode::Max => self.action("Maximum fan mode", || libacercontrol::dbus::AcerControlDBus::new()?.set_fan_max()),
                                };
                                if result {
                                    self.fan_mode = mode;
                                    self.fan_mode_confirmed = true;
                                }
                            }
                            if !self.fan_mode_confirmed {
                                ui.label(RichText::new("Firmware does not report the current mode").small().weak());
                            }
                            ui.add_space(16.0);
                            ui.label(RichText::new("CPU FAN").small().color(MUTED));
                            ui.heading(telemetry.cpu_fan_rpm.map(|v| format!("{v} RPM")).unwrap_or_else(|| "Unavailable".into()));
                            ui.add_space(9.0);
                            ui.label(RichText::new("GPU FAN").small().color(MUTED));
                            ui.heading(telemetry.gpu_fan_rpm.map(|v| format!("{v} RPM")).unwrap_or_else(|| "Unavailable".into()));
                            ui.add_space(12.0);
                            if ui.button("Cooling details →").clicked() { self.page = Page::Cooling; }
                        });
                });
                ui.add_space(12.0);
                ui.columns(3, |columns| {
                    egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).corner_radius(8.0).inner_margin(12)
                        .show(&mut columns[0], |ui| {
                            ui.label(RichText::new("MEMORY & STORAGE").small().strong().color(MUTED));
                            ui.add_space(6.0);
                            let percent = |usage: (u64, u64)| if usage.1 == 0 { 0 } else { ((usage.0 * 100 / usage.1).min(100)) as u8 };
                            let mem = telemetry.system_memory_mib;
                            let mem_detail = mem.map(|(used, total)| format!("{:.1} / {:.1} GiB", used as f32 / 1024.0, total as f32 / 1024.0)).unwrap_or_else(|| "Unavailable".into());
                            Self::progress_meter(ui, "System memory", mem.map(percent), mem_detail);
                            ui.add_space(9.0);
                            let disk = telemetry.root_storage_mib;
                            let disk_detail = disk.map(|(used, total)| format!("{:.1} / {:.1} GiB (root)", used as f32 / 1024.0, total as f32 / 1024.0)).unwrap_or_else(|| "Unavailable".into());
                            Self::progress_meter(ui, "Storage", disk.map(percent), disk_detail);
                            ui.add_space(9.0);
                            let vram = telemetry.gpu_memory_mib;
                            let vram_detail = vram.map(|(used, total)| format!("{} / {} MiB", used, total)).unwrap_or_else(|| "Unavailable".into());
                            Self::progress_meter(ui, "GPU memory", vram.map(percent), vram_detail);
                        });
                    egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).corner_radius(8.0).inner_margin(12)
                        .show(&mut columns[1], |ui| {
                            ui.label(RichText::new("PREFERENCES").small().strong().color(MUTED));
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                for (page, icon, title) in [(Page::Keyboard, "⌨", "Keyboard"), (Page::Cooling, "⌁", "Cooling"), (Page::Settings, "⚙", "Settings")] {
                                    if ui.add(egui::Button::new(format!("{icon}  {title}")).min_size(egui::vec2(92.0, 52.0))).clicked() { self.page = page; }
                                }
                            });
                            ui.add_space(5.0);
                            ui.label(RichText::new("Only controls supported by this driver are shown.").small().weak());
                        });
                    egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).corner_radius(8.0).inner_margin(12)
                        .show(&mut columns[2], |ui| {
                            ui.label(RichText::new("SELECTED POWER TARGET").small().strong().color(MUTED));
                            let [cpu, gpu] = self.power_profiles[self.power_profile_index];
                            ui.add_space(5.0);
                            ui.heading(["Performance", "Balanced", "Battery Saver"][self.power_profile_index]);
                            ui.label(format!("CPU {cpu} W   ·   GPU {gpu} W"));
                            ui.label(RichText::new("Selection does not apply caps until you press Apply.").small().weak());
                            ui.add_space(5.0);
                            if ui.button("Power profile settings →").clicked() { self.page = Page::Performance; }
                        });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let freshness = telemetry.sampled_at
                        .map(|time| format!("Live telemetry · updated {} sec ago", time.elapsed().as_secs().min(99)))
                        .unwrap_or_else(|| "Waiting for telemetry".into());
                    ui.label(RichText::new(freshness).small().color(MUTED));
                });
            }
            Page::Cooling => {
                ui.label(RichText::new("FAN MODE").small().strong().color(MUTED));
                ui.add_space(8.0);
                ui.label("Fan mode");
                let selected = components::segmented_choice(
                    ui,
                    self.fan_mode,
                    &[(components::FanMode::Auto, "Auto"), (components::FanMode::Max, "Max")],
                );
                if let Some(mode) = selected {
                    let success = match mode {
                        components::FanMode::Auto => self.action("Auto fan", || libacercontrol::dbus::AcerControlDBus::new()?.set_fan_auto()),
                        components::FanMode::Max => self.action("Maximum fan", || libacercontrol::dbus::AcerControlDBus::new()?.set_fan_max()),
                    };
                    if success {
                        self.fan_mode = mode;
                        self.fan_mode_confirmed = true;
                    }
                }
                ui.add_space(8.0);
                if self.fan_mode_confirmed {
                    ui.label(format!("Last selected mode: {}", match self.fan_mode { components::FanMode::Auto => "Auto", components::FanMode::Max => "Max" }));
                } else {
                    ui.label("Auto is assumed at startup; this firmware interface does not report the current mode.");
                }
                ui.add_space(20.0);
                ui.label(RichText::new("LIVE COOLING TELEMETRY").small().strong().color(MUTED));
                ui.add_space(10.0);
                Self::telemetry_gauges(ui, &self.telemetry);
            }
            Page::Keyboard => {
                ui.label(RichText::new("BACKLIGHT").small().strong().color(MUTED));
                ui.add_space(10.0);
                let mut changed = false;
                ui.horizontal(|ui| { ui.label("Brightness"); changed |= ui.add(egui::Slider::new(&mut self.brightness, 0..=255)).changed(); ui.label(format!("{} / 255", self.brightness)); });
                ui.horizontal(|ui| { ui.label("Color"); changed |= egui::color_picker::color_edit_button_srgba(ui, &mut self.color, egui::color_picker:: Alpha::Opaque).changed(); });
                if changed {
                    let (r,g,b) = (self.color.r(), self.color.g(), self.color.b());
                    let level = self.brightness;
                    if self.action("Keyboard lighting", move || {
                        let bus = libacercontrol::dbus::AcerControlDBus::new()?;
                        bus.set_keyboard_brightness(level)?;
                        bus.set_keyboard_color(r,g,b)
                    }) {
                        if let Some(controller) = &self.idle_controller {
                            controller.set_brightness(level);
                        }
                        self.animation_mode = components::AnimationMode::Off;
                        self.animation_mode_confirmed = true;
                    }
                }
                ui.add_space(16.0);
                ui.label("Keyboard animation");
                let selected = components::segmented_choice(
                    ui,
                    self.animation_mode,
                    &[(components::AnimationMode::Off, "Off"), (components::AnimationMode::Breathing, "Breathing")],
                );
                if let Some(mode) = selected {
                    let success = match mode {
                        components::AnimationMode::Off => self.action("Keyboard animation", || libacercontrol::dbus::AcerControlDBus::new()?.stop_animation()),
                        components::AnimationMode::Breathing => self.action("Keyboard animation", || libacercontrol::dbus::AcerControlDBus::new()?.start_breathing()),
                    };
                    if success {
                        self.animation_mode = mode;
                        self.animation_mode_confirmed = true;
                    }
                }
                if self.animation_mode_confirmed {
                    ui.label(format!("Restored animation setting: {}", match self.animation_mode { components::AnimationMode::Off => "Off", components::AnimationMode::Breathing => "Breathing" }));
                } else {
                    ui.label("Off is assumed at startup; the daemon could not report the saved animation setting.");
                }
                ui.add_space(18.0);
                ui.label(RichText::new("FADE AFTER INACTIVITY").strong());
                let mut idle_timeout = self.idle_timeout_seconds;
                egui::ComboBox::from_id_salt("keyboard-idle-timeout")
                    .selected_text(if idle_timeout == 0 {
                        "Off".to_owned()
                    } else {
                        format!("{} seconds", idle_timeout)
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut idle_timeout, 0, "Off");
                        for seconds in (5..=30).step_by(5) {
                            ui.selectable_value(
                                &mut idle_timeout,
                                seconds,
                                format!("{seconds} seconds"),
                            );
                        }
                    });
                if idle_timeout != self.idle_timeout_seconds {
                    let previous = self.idle_timeout_seconds;
                    let result = keyboard_idle::save_timeout(idle_timeout)
                        .and_then(|()| {
                            keyboard_idle::set_login_autostart(
                                idle_timeout > 0 || self.minimize_to_tray,
                            )
                        });
                    match result {
                        Ok(()) => {
                            if idle_timeout > 0 && !self.tray_ready {
                                match instance::start_background_service() {
                                    Ok(()) => {
                                        self.tray_ready = true;
                                        self.idle_controller = None;
                                    }
                                    Err(error) => {
                                        self.idle_controller = Some(keyboard_idle::KeyboardIdleController::new(idle_timeout, self.brightness));
                                        self.set_notice(false, format!("Inactivity dimming is active only while this window stays open because the tray service could not start: {error}"));
                                    }
                                }
                            } else if idle_timeout == 0 && !self.minimize_to_tray {
                                instance::request_background_quit();
                                self.tray_ready = false;
                                self.idle_controller = None;
                            } else if let Some(controller) = &self.idle_controller {
                                controller.set_timeout(idle_timeout);
                            }
                            self.idle_timeout_seconds = idle_timeout;
                            if self.notice.as_ref().is_none_or(|(ok, _, _)| *ok) {
                                self.set_notice(true, if idle_timeout > 0 {
                                "Inactivity timeout saved. The tray service keeps it running after the window closes.".to_owned()
                            } else {
                                "Inactivity timeout disabled and login startup removed.".to_owned()
                                });
                            }
                        }
                        Err(error) => {
                            let _ = keyboard_idle::save_timeout(previous);
                            self.set_notice(false, format!("Could not save inactivity settings: {error}"));
                        }
                    }
                }
                ui.label("The keyboard fades out after inactivity and fades back in when you return.");
                let timeout_status = self.idle_controller.as_ref().map(|controller| controller.status())
                    .unwrap_or_else(|| if self.idle_timeout_seconds > 0 { "Inactivity dimming runs in the background service".to_owned() } else { "Idle dimming is off".to_owned() });
                ui.label(RichText::new(timeout_status).weak().small());
                Self::keyboard_preview(ui, self.color, self.brightness);
                ui.add_space(14.0);
                egui::Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(8.0)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.label(RichText::new("LINUX FIRMWARE-KEY EVENT MAPPINGS").small().strong().color(MUTED));
                        ui.add_space(6.0);
                        ui.label("Backlight brightness down / up → KEY_KBDILLUMDOWN / KEY_KBDILLUMUP");
                        ui.label("Backlight toggle → KEY_KBDILLUMTOGGLE · touchpad toggle → KEY_F21");
                        ui.label("Lighting-mode event → KEY_LIGHTS_TOGGLE + Meta + Alt + F6 compatibility chord");
                        ui.label(RichText::new("These are the driver’s event translations, not confirmed Fn-key combinations for every A715-79G firmware revision.").small().weak());
                    });
            }
            Page::Performance => {
                ui.label(RichText::new("POWER PROFILES").small().strong().color(MUTED));
                ui.add_space(8.0);
                let selected = components::segmented_choice(
                    ui,
                    self.power_profile_index,
                    &[(0, "Performance"), (1, "Balanced"), (2, "Battery Saver")],
                );
                if let Some(index) = selected {
                    self.power_profile_index = index;
                    if let Err(error) = super::power_profiles::save(self.power_profiles, index) {
                        self.set_notice(false, format!("Could not save power profile: {error}"));
                    }
                }

                let profile_index = self.power_profile_index;
                let mut changed = false;
                ui.add_space(14.0);
                ui.label(RichText::new("CAPS FOR THIS PROFILE").small().strong().color(MUTED));
                ui.horizontal(|ui| {
                    ui.label("CPU");
                    changed |= ui.add(
                        egui::DragValue::new(&mut self.power_profiles[profile_index][0])
                            .range(1..=500)
                            .suffix(" W"),
                    ).changed();
                    ui.add_space(18.0);
                    ui.label("GPU");
                    changed |= ui.add(
                        egui::DragValue::new(&mut self.power_profiles[profile_index][1])
                            .range(1..=500)
                            .suffix(" W"),
                    ).changed();
                });
                if ui.button("Reset profile defaults").clicked() {
                    self.power_profiles[profile_index] = super::power_profiles::DEFAULTS[profile_index];
                    changed = true;
                }
                if changed {
                    if let Err(error) = super::power_profiles::save(self.power_profiles, profile_index) {
                        self.set_notice(false, format!("Could not save power limits: {error}"));
                    }
                }

                ui.add_space(12.0);
                let apply_label = if self.power_cap_applying {
                    "Applying power caps…"
                } else {
                    "Apply power caps"
                };
                if ui
                    .add_enabled(!self.power_cap_applying, egui::Button::new(apply_label))
                    .clicked()
                {
                    let [cpu_watts, gpu_watts] = self.power_profiles[profile_index];
                    self.power_cap_applying = true;
                    self.set_notice(true, "Applying CPU and GPU power caps…".to_owned());
                    let sender = self.power_cap_sender.clone();
                    std::thread::spawn(move || {
                        let result = libacercontrol::dbus::AcerControlDBus::new()
                            .and_then(|bus| bus.set_power_caps(cpu_watts, gpu_watts));
                        let _ = sender.send(result);
                    });
                }
                ui.add_space(10.0);
                ui.label(RichText::new("Caps are saved per profile. CPU values are written to Intel RAPL and read back; this does not measure workload power or prove firmware enforcement. GPU success requires NVIDIA's software-requested limit to match. An enforced ceiling or live power draw is reported separately and is not proof the requested cap was set; firmware and Dynamic Boost can still constrain actual power.").color(MUTED));
            }
            Page::Battery => { ui.label(RichText::new("Battery charge management is planned for a future release.").color(MUTED)); }
            Page::Settings => {
                ui.add_space(12.0);
                let previous = self.minimize_to_tray;
                let mut minimize_to_tray = previous;
                ui.checkbox(&mut minimize_to_tray, "Minimize to system tray when closing the window");
                if !self.tray_ready {
                    ui.label(RichText::new("Enabling this starts the background tray service for this session.").weak());
                }
                if minimize_to_tray != previous {
                    let result = keyboard_idle::save_minimize_to_tray(minimize_to_tray)
                        .and_then(|()| {
                            keyboard_idle::set_login_autostart(
                                minimize_to_tray || self.idle_timeout_seconds > 0,
                            )
                        });
                    match result {
                        Ok(()) => {
                            self.minimize_to_tray = minimize_to_tray;
                            if minimize_to_tray {
                                match instance::start_background_service() {
                                    Ok(()) => {
                                        self.tray_ready = true;
                                        self.idle_controller = None;
                                    }
                                    Err(error) => {
                                        self.set_notice(false, format!("Minimize to tray is saved and will be retried at login, but the tray service could not start: {error}"));
                                    }
                                }
                            } else if self.idle_timeout_seconds == 0 {
                                instance::request_background_quit();
                                self.tray_ready = false;
                            }
                            if self.tray_ready || !minimize_to_tray {
                                self.set_notice(true, if minimize_to_tray {
                                    "Closing the window will close its window while the background service remains in the system tray.".to_owned()
                                } else {
                                    "Closing the window will exit Acer Control Center when no background timeout is active.".to_owned()
                                });
                            }
                        }
                        Err(error) => {
                            let _ = keyboard_idle::save_minimize_to_tray(previous);
                            let _ = keyboard_idle::set_login_autostart(
                                previous || self.idle_timeout_seconds > 0,
                            );
                            self.set_notice(false, format!("Could not save tray setting: {error}"));
                        }
                    }
                }
                ui.label(RichText::new("Login startup stays enabled while tray minimization or keyboard inactivity dimming is on. The background service starts hidden with a tray icon.").weak());
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.button("Refresh status").clicked() {
                        self.about_info = about::AboutInfo::collect();
                    }
                    ui.label(RichText::new("Version and hardware information").weak());
                });
                egui::Grid::new("system-status")
                    .num_columns(2)
                    .striped(true)
                    .spacing([24.0, 8.0])
                    .show(ui, |ui| {
                        for (label, value) in &self.about_info.rows {
                            ui.label(RichText::new(label).weak());
                            ui.label(value);
                            ui.end_row();
                        }
                    });
                ui.add_space(20.0);
                ui.separator();
                ui.heading("Updates");
                ui.label("Check GitHub for a newer version. Download and installation stay under your control.");
                if ui.add_enabled(!self.update_checking, egui::Button::new("Check for updates")).clicked() {
                    self.update_checking = true;
                    self.update_status = "Checking GitHub Releases…".to_owned();
                    self.latest_release = None;
                    let sender = self.update_sender.clone();
                    std::thread::spawn(move || {
                        let _ = sender.send(about::check_latest_release());
                    });
                }
                ui.label(&self.update_status);
                if let Some(release) = &self.latest_release {
                    ui.hyperlink_to("View release and downloads", &release.html_url);
                } else if !self.update_checking
                    && self.update_status != "Updates are checked only when you ask."
                    && self.update_status != "Checking GitHub Releases…"
                {
                    ui.hyperlink_to("Open project releases", about::RELEASES_PAGE_URL);
                }
            }
        }
    }
}
