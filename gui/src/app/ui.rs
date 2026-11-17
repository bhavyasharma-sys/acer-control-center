use super::*;

/// Narrowest content width at which the three-part dashboard stays side by
/// side. 720 pt keeps the target composition at the app's 800x600 minimum
/// window; below it the panels stack vertically instead of clipping.
const DASHBOARD_MIN_ROW_WIDTH: f32 = 720.0;
/// Narrowest content width at which the bottom row stays side by side.
const BOTTOM_ROW_MIN_WIDTH: f32 = 660.0;
/// Narrowest content width at which the power-mode bar keeps Apply inline.
const POWER_BAR_MIN_WIDTH: f32 = 700.0;
/// Memory keys for the measured dashboard row heights.
const MAIN_ROW_ID: &str = "acer-control-dashboard-main-row";
const BOTTOM_ROW_ID: &str = "acer-control-dashboard-bottom-row";

/// Fan gauge full-scale, in RPM.
const FAN_FULL_SCALE: f32 = 6000.0;

/// Column widths for a weighted row, or `None` when the panels must stack.
fn panel_widths(ui: &egui::Ui, weights: &[f32], min_side_by_side: f32) -> Option<Vec<f32>> {
    if weights.is_empty() {
        return None;
    }
    let available = ui.available_width();
    if !available.is_finite() || available < min_side_by_side {
        return None;
    }
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let spacing = ui.spacing().item_spacing.x;
    let usable = (available - spacing * (weights.len() as f32 - 1.0)).max(1.0);
    Some(
        weights
            .iter()
            .map(|weight| (weight / total * usable).max(140.0))
            .collect(),
    )
}

/// Lay out `weights.len()` panels side by side when there is room, stacked
/// when there is not. `panel` receives the panel index and returns the height
/// of the card it drew; the tallest of them is returned so a row of cards can
/// be aligned on one baseline on the next frame.
fn layout_panels(
    ui: &mut egui::Ui,
    weights: &[f32],
    min_side_by_side: f32,
    mut panel: impl FnMut(&mut egui::Ui, usize) -> f32,
) -> f32 {
    let mut tallest = 0.0_f32;
    match panel_widths(ui, weights, min_side_by_side) {
        Some(shares) => {
            ui.horizontal_top(|ui| {
                for (index, share) in shares.iter().enumerate() {
                    ui.scope(|ui| {
                        // Fixed share so panels never squeeze or overlap.
                        ui.set_width(*share);
                        tallest = tallest.max(panel(ui, index));
                    });
                }
            });
        }
        None => {
            for index in 0..weights.len() {
                tallest = tallest.max(panel(ui, index));
                if index + 1 < weights.len() {
                    ui.add_space(10.0);
                }
            }
        }
    }
    tallest
}

/// Vertical chrome of a card: 14 pt of inner margin at the top and bottom plus
/// the 1 pt stroke on each side. Subtracting it from a row height gives the
/// content height the cards in that row need in order to end on one baseline.
pub(crate) const CARD_CHROME_HEIGHT: f32 = 30.0;

/// A rounded, near-black content card that fills its column. Returns the total
/// height of the card, chrome included.
fn card(ui: &mut egui::Ui, min_height: f32, content: impl FnOnce(&mut egui::Ui)) -> f32 {
    egui::Frame::new()
        .fill(CARD_BG)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if min_height > 0.0 {
                ui.set_min_height(min_height);
            }
            content(ui);
        })
        .response
        .rect
        .height()
}

/// Height a row of cards should have, taken from what the same row measured on
/// the previous frame. egui lays out in a single pass, so a row cannot know its
/// own final height while it is being drawn; remembering it converges after one
/// frame and keeps every card in the row the same height even when one of them
/// grows, for example when the last Apply result appears. `fallback` is used on
/// the first frame.
fn measured_row_height(ui: &egui::Ui, id: egui::Id, fallback: f32) -> f32 {
    // Dereference inside the closure: `data_mut` returns whatever the closure
    // returns, and a `&mut f32` would borrow the map, which does not outlive
    // the call.
    let stored = ui
        .ctx()
        .data_mut(|data| *data.get_temp_mut_or_default::<f32>(id));
    if stored.is_finite() && stored > 0.0 {
        stored.max(fallback)
    } else {
        fallback
    }
}

fn remember_row_height(ui: &egui::Ui, id: egui::Id, height: f32) {
    if height.is_finite() && height > 0.0 {
        ui.ctx().data_mut(|data| data.insert_temp(id, height));
    }
}

fn card_header(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 12.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 1.5, ACCENT);
        ui.label(
            RichText::new(title.to_uppercase())
                .size(11.0)
                .strong()
                .color(ACCENT),
        );
    });
    ui.add_space(8.0);
}

/// A label/value line with the value right-aligned.
fn stat_line(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(13.0).strong().color(color));
        });
    });
    ui.add_space(4.0);
}

/// A large readout with a small caption above it.
fn big_readout(ui: &mut egui::Ui, caption: &str, value: &str, available: bool) {
    ui.label(
        RichText::new(caption.to_uppercase())
            .size(11.0)
            .strong()
            .color(MUTED),
    );
    ui.label(
        RichText::new(value)
            .size(22.0)
            .strong()
            .color(if available { TEXT } else { MUTED }),
    );
}

fn temperature_color(value: i32) -> Color32 {
    if value >= 85 {
        HOT
    } else if value >= 70 {
        WARM
    } else {
        COOL
    }
}

/// Horizontal usage bar. When the source reports nothing the track is drawn
/// dashed so an absent sensor can never be read as zero.
fn usage_bar(ui: &mut egui::Ui, label: &str, percent: Option<u8>, detail: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(detail)
                    .size(12.0)
                    .strong()
                    .color(if percent.is_some() { TEXT } else { MUTED }),
            );
        });
    });
    ui.add_space(3.0);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 7.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.5, Color32::from_rgb(22, 31, 40));
    match percent {
        Some(percent) => {
            let fraction = (percent as f32 / 100.0).clamp(0.0, 1.0);
            let end = rect.left() + rect.width() * fraction;
            if end - rect.left() > 0.5 {
                painter.rect_filled(
                    egui::Rect::from_min_max(rect.left_top(), egui::pos2(end, rect.bottom())),
                    3.5,
                    color,
                );
            }
        }
        None => {
            let mut x = rect.left() + 2.0;
            while x < rect.right() - 4.0 {
                painter.line_segment(
                    [egui::pos2(x, rect.center().y), egui::pos2(x + 4.0, rect.center().y)],
                    Stroke::new(2.0, Color32::from_rgb(46, 60, 70)),
                );
                x += 9.0;
            }
        }
    }
    ui.add_space(7.0);
}

/// Rounded power-mode tab. The dot marks the profile whose caps are live.
fn power_pill(ui: &mut egui::Ui, label: &str, selected: bool, applied: bool) -> bool {
    let text_color = if selected {
        Color32::from_rgb(203, 248, 250)
    } else {
        MUTED
    };
    // `RichText::strong()` takes no argument, so weight is applied conditionally.
    let text = RichText::new(label).size(13.0).color(text_color);
    let text = if selected { text.strong() } else { text };
    let button = egui::Button::new(text)
    .selected(selected)
    .fill(if selected { ACCENT_DIM } else { SURFACE })
    .stroke(Stroke::new(1.0, if selected { ACCENT } else { BORDER }))
    .corner_radius(9.0)
    .min_size(egui::vec2(100.0, 34.0));
    let response = ui.add(button);
    if applied {
        let rect = response.rect;
        ui.painter().circle_filled(
            egui::pos2(rect.right() - 10.0, rect.top() + 10.0),
            3.0,
            ACCENT,
        );
    }
    response.clicked()
}

/// Small status chip: applied / not applied / applying.
fn state_chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    let font = egui::FontId::proportional(11.0);
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font, color));
    let padding = egui::vec2(9.0, 5.0);
    let size = galley.size() + padding * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 7.0, color.linear_multiply(0.16));
    painter.rect_stroke(
        rect,
        7.0,
        Stroke::new(1.0, color.linear_multiply(0.55)),
        egui::StrokeKind::Middle,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
}

impl ControlCenter {
    pub(super) fn page_content(&mut self, ui: &mut egui::Ui) {
        match self.page {
            Page::Dashboard => self.dashboard_page(ui),
            Page::Cooling => self.cooling_page(ui),
            Page::Keyboard => self.keyboard_page(ui),
            Page::Performance => self.performance_page(ui),
            Page::Battery => self.battery_page(ui),
            Page::Settings => self.settings_page(ui),
        }
    }

    // ---------------------------------------------------------------- dashboard

    fn dashboard_page(&mut self, ui: &mut egui::Ui) {
        let telemetry = self.telemetry.clone();
        let weights = [0.30_f32, 0.40, 0.30];
        // Height of the speedometer canvas follows the centre column width so
        // the gauge keeps its proportions at any window size.
        let centre_width = panel_widths(ui, &weights, DASHBOARD_MIN_ROW_WIDTH)
            .map(|shares| shares[1])
            .unwrap_or_else(|| ui.available_width());
        let dial_height = ((centre_width - 30.0) * 0.62).clamp(250.0, 288.0);
        // Row heights are measured rather than guessed, so the cards in a row
        // always end on one baseline. The memory key includes the width bucket,
        // so resizing the window starts from the fallbacks again instead of
        // keeping a height measured for a different layout. The fallbacks are
        // the first-frame budget: the dial plus its header, chrome and caption,
        // and a compact bottom row.
        let width_bucket = (ui.available_width() / 24.0) as i32;
        let main_id = egui::Id::new(MAIN_ROW_ID).with(width_bucket);
        let bottom_id = egui::Id::new(BOTTOM_ROW_ID).with(width_bucket);
        let main_row = measured_row_height(ui, main_id, dial_height + 86.0);
        let bottom_row = measured_row_height(ui, bottom_id, 226.0);
        let main_height = main_row - CARD_CHROME_HEIGHT;
        let bottom_height = bottom_row - CARD_CHROME_HEIGHT;

        self.power_mode_bar(ui);
        ui.add_space(12.0);

        let tallest = layout_panels(ui, &weights, DASHBOARD_MIN_ROW_WIDTH, |ui, index| {
            match index {
                0 => Self::dashboard_system_panel(ui, &telemetry, main_height),
                1 => Self::dashboard_dial_panel(ui, &telemetry, dial_height, main_height),
                _ => self.dashboard_fan_panel(ui, &telemetry, main_height),
            }
        });
        remember_row_height(ui, main_id, tallest.max(main_row));

        ui.add_space(12.0);

        let tallest = layout_panels(ui, &[0.36, 0.30, 0.34], BOTTOM_ROW_MIN_WIDTH, |ui, index| {
            match index {
                0 => Self::dashboard_memory_panel(ui, &telemetry, bottom_height),
                1 => self.dashboard_quick_panel(ui, bottom_height),
                _ => self.dashboard_target_panel(ui, bottom_height),
            }
        });
        remember_row_height(ui, bottom_id, tallest.max(bottom_row));
    }

    /// Power-mode tabs across the top of the dashboard, with the explicit
    /// Apply action. Selecting a tab never writes hardware caps.
    fn power_mode_bar(&mut self, ui: &mut egui::Ui) {
        let selected_index = self.power_profile_index;
        let selected = self.power_profiles[selected_index];
        let applied_matches =
            applied_matches_selection(self.last_applied_power, selected_index, selected);
        let applying = self.power_cap_applying;
        let mut clicked_profile: Option<usize> = None;
        let mut apply_requested = false;

        let inline_apply = ui.available_width() >= POWER_BAR_MIN_WIDTH;
        let apply_label = if applying { "Applying…" } else { "Apply" };
        let chip = power_apply_state_label(power_apply_state(applying, applied_matches));

        if inline_apply {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("POWER MODE")
                        .size(11.0)
                        .strong()
                        .color(MUTED),
                );
                ui.add_space(2.0);
                for (index, name) in POWER_MODE_NAMES.iter().enumerate() {
                    if power_pill(
                        ui,
                        name,
                        selected_index == index,
                        applied_matches && selected_index == index,
                    ) {
                        clicked_profile = Some(index);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !applying,
                            egui::Button::new(
                                RichText::new(apply_label)
                                    .size(13.0)
                                    .strong()
                                    .color(if applying { MUTED } else { Color32::from_rgb(4, 22, 26) }),
                            )
                            .fill(if applying { SURFACE } else { ACCENT })
                            .stroke(Stroke::new(
                                1.0,
                                if applying { BORDER } else { ACCENT },
                            ))
                            .corner_radius(9.0)
                            .min_size(egui::vec2(96.0, 34.0)),
                        )
                        .on_hover_text(format!(
                            "Write the {} caps (CPU {} W · GPU {} W) and verify by readback",
                            POWER_MODE_NAMES[selected_index], selected[0], selected[1]
                        ))
                        .clicked()
                    {
                        apply_requested = true;
                    }
                    ui.add_space(10.0);
                    state_chip(ui, chip.0, chip.1);
                });
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("POWER MODE")
                        .size(11.0)
                        .strong()
                        .color(MUTED),
                );
                for (index, name) in POWER_MODE_NAMES.iter().enumerate() {
                    if power_pill(
                        ui,
                        name,
                        selected_index == index,
                        applied_matches && selected_index == index,
                    ) {
                        clicked_profile = Some(index);
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                state_chip(ui, chip.0, chip.1);
                ui.add_space(8.0);
                if ui
                    .add_enabled(
                        !applying,
                        egui::Button::new(apply_label).min_size(egui::vec2(96.0, 32.0)),
                    )
                    .clicked()
                {
                    apply_requested = true;
                }
            });
        }

        if let Some(index) = clicked_profile {
            self.select_power_profile(index);
        }
        if apply_requested {
            self.request_power_apply();
        }
    }

    fn dashboard_system_panel(
        ui: &mut egui::Ui,
        telemetry: &TelemetrySnapshot,
        min_height: f32,
    ) -> f32 {
        card(ui, min_height, |ui| {
            card_header(ui, "System");
            ui.label(RichText::new("Acer Aspire 7 · A715-79G").size(14.0).strong());
            ui.label(
                RichText::new("Core 5 210H · RTX 3050 Laptop 6 GB")
                    .size(11.0)
                    .color(MUTED),
            );
            ui.add_space(10.0);

            let (cpu_clock, cpu_clock_ok) = match telemetry.cpu_clock_mhz {
                Some(mhz) => (format!("{mhz} MHz"), true),
                None => ("Unavailable".to_owned(), false),
            };
            stat_line(ui, "CPU clock (avg)", &cpu_clock, if cpu_clock_ok { TEXT } else { MUTED });

            let (cpu_temp, cpu_temp_color) = match telemetry.cpu_temperature {
                Some(value) => (format!("{value} °C"), temperature_color(value)),
                None => ("Unavailable".to_owned(), MUTED),
            };
            stat_line(ui, "CPU temperature", &cpu_temp, cpu_temp_color);

            let (gpu_clock, gpu_clock_ok) = match telemetry.gpu_clock_mhz {
                Some(mhz) => (format!("{mhz} MHz"), true),
                None => ("Unavailable".to_owned(), false),
            };
            stat_line(ui, "GPU clock", &gpu_clock, if gpu_clock_ok { TEXT } else { MUTED });

            let (gpu_temp, gpu_temp_color) = match telemetry.gpu_temperature {
                Some(value) => (format!("{value} °C"), temperature_color(value)),
                None => ("Unavailable".to_owned(), MUTED),
            };
            stat_line(ui, "GPU temperature", &gpu_temp, gpu_temp_color);

            ui.add_space(8.0);
            usage_bar(
                ui,
                "CPU use",
                telemetry.cpu_usage_percent,
                &telemetry
                    .cpu_usage_percent
                    .map(|value| format!("{value} %"))
                    .unwrap_or_else(|| "Unavailable".to_owned()),
                ACCENT,
            );
            usage_bar(
                ui,
                "GPU use",
                telemetry.gpu_usage_percent,
                &telemetry
                    .gpu_usage_percent
                    .map(|value| format!("{value} %"))
                    .unwrap_or_else(|| "Unavailable".to_owned()),
                COOL,
            );
        })
    }

    fn dashboard_dial_panel(
        ui: &mut egui::Ui,
        telemetry: &TelemetrySnapshot,
        dial_height: f32,
        min_height: f32,
    ) -> f32 {
        Self::dashboard_dial(ui, telemetry, dial_height, min_height)
    }

    fn dashboard_fan_panel(
        &mut self,
        ui: &mut egui::Ui,
        telemetry: &TelemetrySnapshot,
        min_height: f32,
    ) -> f32 {
        let mut fan_request: Option<components::FanMode> = None;
        let mut open_cooling = false;
        let fan_mode = self.pending_fan_mode.unwrap_or(self.fan_mode);
        let confirmed = self.fan_mode_confirmed;
        let pending = self.pending_fan_mode;

        let height = card(ui, min_height, |ui| {
            card_header(ui, "Fan speed control");
            if let Some(mode) = components::segmented_choice(
                ui,
                fan_mode,
                &[
                    (components::FanMode::Auto, "Automatic"),
                    (components::FanMode::Max, "Maximum"),
                ],
            ) {
                fan_request = Some(mode);
            }
            ui.add_space(4.0);
            if pending.is_some() {
                ui.label(RichText::new("Applying fan mode…").size(11.0).color(WARM));
            } else if !confirmed {
                ui.label(
                    RichText::new("This firmware interface does not report the active mode")
                        .size(11.0)
                        .color(MUTED),
                );
            } else {
                ui.label(
                    RichText::new(format!(
                        "Active mode: {}",
                        match fan_mode {
                            components::FanMode::Auto => "Automatic",
                            components::FanMode::Max => "Maximum",
                        }
                    ))
                    .size(11.0)
                    .color(MUTED),
                );
            }

            ui.add_space(12.0);
            let (cpu_fan_text, cpu_fan_ok) = match telemetry.cpu_fan_rpm {
                Some(rpm) => (format!("{rpm} RPM"), true),
                None => ("Unavailable".to_owned(), false),
            };
            big_readout(ui, "CPU fan", &cpu_fan_text, cpu_fan_ok);
            usage_bar(
                ui,
                "",
                telemetry
                    .cpu_fan_rpm
                    .map(|rpm| ((rpm as f32 / FAN_FULL_SCALE) * 100.0).clamp(0.0, 100.0) as u8),
                &format!("of {} RPM scale", FAN_FULL_SCALE as i32),
                ACCENT,
            );

            let (gpu_fan_text, gpu_fan_ok) = match telemetry.gpu_fan_rpm {
                Some(rpm) => (format!("{rpm} RPM"), true),
                None => ("Unavailable".to_owned(), false),
            };
            big_readout(ui, "GPU fan", &gpu_fan_text, gpu_fan_ok);
            usage_bar(
                ui,
                "",
                telemetry
                    .gpu_fan_rpm
                    .map(|rpm| ((rpm as f32 / FAN_FULL_SCALE) * 100.0).clamp(0.0, 100.0) as u8),
                &format!("of {} RPM scale", FAN_FULL_SCALE as i32),
                COOL,
            );

            ui.add_space(4.0);
            if ui
                .add(
                    egui::Button::new(RichText::new("Cooling details →").size(12.0))
                        .min_size(egui::vec2(0.0, 32.0)),
                )
                .clicked()
            {
                open_cooling = true;
            }
        });

        if let Some(mode) = fan_request {
            if self.pending_fan_mode.is_none() {
                self.pending_fan_mode = Some(mode);
                self.dispatch(HardwareJob::SetFanMode(mode));
            }
        }
        if open_cooling {
            self.page = Page::Cooling;
        }
        height
    }

    fn dashboard_memory_panel(
        ui: &mut egui::Ui,
        telemetry: &TelemetrySnapshot,
        min_height: f32,
    ) -> f32 {
        let percent = |usage: (u64, u64)| {
            (usage.1 > 0)
                .then(|| ((usage.0.saturating_mul(100) / usage.1).min(100)) as u8)
        };
        card(ui, min_height, |ui| {
            card_header(ui, "Memory & storage");
            let memory = telemetry.system_memory_mib;
            usage_bar(
                ui,
                "System memory",
                memory.and_then(percent),
                &memory
                    .map(|(used, total)| {
                        format!("{:.1} / {:.1} GiB", used as f32 / 1024.0, total as f32 / 1024.0)
                    })
                    .unwrap_or_else(|| "Unavailable".to_owned()),
                ACCENT,
            );
            let disk = telemetry.root_storage_mib;
            usage_bar(
                ui,
                "Storage (root)",
                disk.and_then(percent),
                &disk
                    .map(|(used, total)| {
                        format!("{:.1} / {:.1} GiB", used as f32 / 1024.0, total as f32 / 1024.0)
                    })
                    .unwrap_or_else(|| "Unavailable".to_owned()),
                COOL,
            );
            let vram = telemetry.gpu_memory_mib;
            usage_bar(
                ui,
                "GPU memory",
                vram.and_then(percent),
                &vram
                    .map(|(used, total)| format!("{used} / {total} MiB"))
                    .unwrap_or_else(|| "Unavailable".to_owned()),
                WARM,
            );
            ui.label(
                RichText::new("Memory from /proc/meminfo · storage from statvfs on / · VRAM from NVIDIA telemetry")
                    .size(11.0)
                    .color(MUTED),
            );
        })
    }

    fn dashboard_quick_panel(&mut self, ui: &mut egui::Ui, min_height: f32) -> f32 {
        let mut open: Option<Page> = None;
        let mut fan_request: Option<components::FanMode> = None;
        let fan_mode = self.pending_fan_mode.unwrap_or(self.fan_mode);
        let pending_fan = self.pending_fan_mode.is_some();

        let height = card(ui, min_height, |ui| {
            card_header(ui, "Quick controls");
            let next_fan = match fan_mode {
                components::FanMode::Auto => components::FanMode::Max,
                components::FanMode::Max => components::FanMode::Auto,
            };
            let label = match next_fan {
                components::FanMode::Max => "Set maximum fan",
                components::FanMode::Auto => "Set automatic fan",
            };
            if ui
                .add_enabled(
                    !pending_fan,
                    egui::Button::new(RichText::new(label).size(12.0))
                        .min_size(egui::vec2(0.0, 34.0)),
                )
                .clicked()
            {
                fan_request = Some(next_fan);
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (page, title) in [
                    (Page::Keyboard, "⌨ Keyboard"),
                    (Page::Cooling, "⌁ Cooling"),
                    (Page::Settings, "⚙ Settings"),
                ] {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(title).size(12.0))
                                .min_size(egui::vec2(96.0, 34.0)),
                        )
                        .clicked()
                    {
                        open = Some(page);
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new("Only controls supported by this driver are offered.")
                    .size(11.0)
                    .color(MUTED),
            );
        });

        if let Some(mode) = fan_request {
            if self.pending_fan_mode.is_none() {
                self.pending_fan_mode = Some(mode);
                self.dispatch(HardwareJob::SetFanMode(mode));
            }
        }
        if let Some(page) = open {
            self.page = page;
        }
        height
    }

    fn dashboard_target_panel(&mut self, ui: &mut egui::Ui, min_height: f32) -> f32 {
        let index = self.power_profile_index;
        let [cpu, gpu] = self.power_profiles[index];
        let name = POWER_MODE_NAMES[index];
        let applying = self.power_cap_applying;
        let applied_matches =
            applied_matches_selection(self.last_applied_power, index, [cpu, gpu]);
        let chip = power_apply_state_label(power_apply_state(applying, applied_matches));
        let last_result = self.last_power_result.clone();
        let mut apply_requested = false;
        let mut open_performance = false;

        let height = card(ui, min_height, |ui| {
            card_header(ui, "Selected power target");
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).size(18.0).strong());
                ui.add_space(6.0);
                state_chip(ui, chip.0, chip.1);
            });
            ui.add_space(4.0);
            stat_line(ui, "CPU limit (software request)", &format!("{cpu} W"), TEXT);
            stat_line(ui, "GPU limit (software request)", &format!("{gpu} W"), TEXT);
            ui.label(
                RichText::new("Selection only stores the target. Caps are written when you press Apply, and each write is verified by readback.")
                    .size(11.0)
                    .color(MUTED),
            );
            if let Some((success, cpu_message, gpu_message)) = &last_result {
                ui.add_space(6.0);
                let color = if *success { COOL } else { HOT };
                ui.label(RichText::new("Last apply result").size(11.0).strong().color(MUTED));
                ui.label(RichText::new(truncate(cpu_message, 150)).size(11.0).color(color));
                ui.label(RichText::new(truncate(gpu_message, 220)).size(11.0).color(color));
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !applying,
                        egui::Button::new(RichText::new(if applying { "Applying…" } else { "Apply caps" }).size(12.0).strong())
                            .min_size(egui::vec2(110.0, 34.0)),
                    )
                    .clicked()
                {
                    apply_requested = true;
                }
                if ui
                    .add(
                        egui::Button::new(RichText::new("Edit profiles →").size(12.0))
                            .min_size(egui::vec2(110.0, 34.0)),
                    )
                    .clicked()
                {
                    open_performance = true;
                }
            });
        });

        if apply_requested {
            self.request_power_apply();
        }
        if open_performance {
            self.page = Page::Performance;
        }
        height
    }

    // ------------------------------------------------------------------ cooling

    fn cooling_page(&mut self, ui: &mut egui::Ui) {
        let mut fan_request: Option<components::FanMode> = None;
        let fan_mode = self.pending_fan_mode.unwrap_or(self.fan_mode);
        let confirmed = self.fan_mode_confirmed;
        let pending = self.pending_fan_mode;

        card(ui, 0.0, |ui| {
            card_header(ui, "Fan mode");
            if let Some(mode) = components::segmented_choice(
                ui,
                fan_mode,
                &[
                    (components::FanMode::Auto, "Automatic"),
                    (components::FanMode::Max, "Maximum"),
                ],
            ) {
                fan_request = Some(mode);
            }
            ui.add_space(6.0);
            if pending.is_some() {
                ui.label(RichText::new("Applying fan mode…").size(11.0).color(WARM));
            } else if confirmed {
                ui.label(format!(
                    "Last confirmed mode: {}",
                    match fan_mode {
                        components::FanMode::Auto => "Automatic",
                        components::FanMode::Max => "Maximum",
                    }
                ));
            } else {
                ui.label(
                    "Automatic is assumed at startup; this firmware interface does not report the current mode.",
                );
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new("Fan control uses the supported driver ioctl only. No undocumented EC registers are written.")
                    .size(11.0)
                    .color(MUTED),
            );
        });

        if let Some(mode) = fan_request {
            if self.pending_fan_mode.is_none() {
                self.pending_fan_mode = Some(mode);
                self.dispatch(HardwareJob::SetFanMode(mode));
            }
        }

        ui.add_space(14.0);
        ui.label(
            RichText::new("LIVE COOLING TELEMETRY")
                .size(11.0)
                .strong()
                .color(MUTED),
        );
        ui.add_space(10.0);
        let telemetry = self.telemetry.clone();
        Self::telemetry_gauges(ui, &telemetry);
    }

    // ----------------------------------------------------------------- keyboard

    fn keyboard_page(&mut self, ui: &mut egui::Ui) {
        let mut animation_request: Option<components::AnimationMode> = None;
        let animation_mode = self.pending_animation.unwrap_or(self.animation_mode);
        let animation_confirmed = self.animation_mode_confirmed;
        let animation_pending = self.pending_animation;

        card(ui, 0.0, |ui| {
            card_header(ui, "Backlight");
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Brightness").size(12.0));
                changed |= ui
                    .add(egui::Slider::new(&mut self.brightness, 0..=255))
                    .changed();
                ui.label(
                    RichText::new(format!("{} / 255", self.brightness))
                        .size(12.0)
                        .color(MUTED),
                );
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Color").size(12.0));
                changed |= egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut self.color,
                    egui::color_picker::Alpha::Opaque,
                )
                .changed();
            });
            if changed {
                // Debounced: the worker receives the settled value only, so
                // dragging the slider cannot queue a flood of D-Bus writes.
                self.pending_keyboard = Some((self.brightness, self.color, Instant::now()));
            }
            if self.pending_keyboard.is_some() {
                ui.label(RichText::new("Applying keyboard lighting…").size(11.0).color(WARM));
            }
        });

        ui.add_space(12.0);
        card(ui, 0.0, |ui| {
            card_header(ui, "Keyboard animation");
            if let Some(mode) = components::segmented_choice(
                ui,
                animation_mode,
                &[
                    (components::AnimationMode::Off, "Off"),
                    (components::AnimationMode::Breathing, "Breathing"),
                ],
            ) {
                animation_request = Some(mode);
            }
            ui.add_space(6.0);
            if animation_pending.is_some() {
                ui.label(RichText::new("Applying animation…").size(11.0).color(WARM));
            } else if animation_confirmed {
                ui.label(format!(
                    "Restored animation setting: {}",
                    match animation_mode {
                        components::AnimationMode::Off => "Off",
                        components::AnimationMode::Breathing => "Breathing",
                    }
                ));
            } else {
                ui.label(
                    "Off is assumed at startup; the daemon could not report the saved animation setting.",
                );
            }
        });

        if let Some(mode) = animation_request {
            if self.pending_animation.is_none() {
                self.pending_animation = Some(mode);
                self.dispatch(HardwareJob::SetAnimation(mode));
            }
        }

        ui.add_space(12.0);
        self.keyboard_idle_card(ui);

        let color = self.color;
        let brightness = self.brightness;
        Self::keyboard_preview(ui, color, brightness);

        ui.add_space(14.0);
        card(ui, 0.0, |ui| {
            card_header(ui, "Linux firmware-key event mappings");
            ui.label("Backlight brightness down / up → KEY_KBDILLUMDOWN / KEY_KBDILLUMUP");
            ui.label("Backlight toggle → KEY_KBDILLUMTOGGLE · touchpad toggle → KEY_F21");
            ui.label("RF kill → KEY_RFKILL");
            ui.label("Lighting-mode event → KEY_LIGHTS_TOGGLE, plus a Meta + Alt + F6 compatibility chord");
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "These are the driver’s ACPI/WMI event translations for this machine, not confirmed Fn-key combinations for every A715-79G firmware revision.",
                )
                .size(11.0)
                .color(MUTED),
            );
        });
    }

    fn keyboard_idle_card(&mut self, ui: &mut egui::Ui) {
        let pending = self.idle_settings_pending;
        let current = self.idle_timeout_seconds;
        let mut requested: Option<u32> = None;

        card(ui, 0.0, |ui| {
            card_header(ui, "Fade after inactivity");
            ui.horizontal(|ui| {
                ui.label(RichText::new("Timeout").size(12.0));
                let mut selection = current;
                egui::ComboBox::from_id_salt("keyboard-idle-timeout")
                    .selected_text(if selection == 0 {
                        "Off".to_owned()
                    } else {
                        format!("{selection} seconds")
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selection, 0, "Off");
                        for seconds in (5..=30).step_by(5) {
                            ui.selectable_value(&mut selection, seconds, format!("{seconds} seconds"));
                        }
                    });
                if selection != current {
                    requested = Some(selection);
                }
                if pending {
                    ui.label(RichText::new("Saving…").size(11.0).color(WARM));
                }
            });
            ui.label("The keyboard fades out after inactivity and fades back in when you return.");
            let timeout_status = self
                .idle_controller
                .as_ref()
                .map(|controller| controller.status())
                .unwrap_or_else(|| {
                    if self.idle_timeout_seconds > 0 {
                        "Inactivity dimming runs in the background service".to_owned()
                    } else {
                        "Idle dimming is off".to_owned()
                    }
                });
            ui.label(RichText::new(timeout_status).size(11.0).color(MUTED));
        });

        if let Some(seconds) = requested {
            if !pending {
                self.idle_settings_pending = true;
                self.dispatch(HardwareJob::SaveIdleTimeout {
                    seconds,
                    minimize_to_tray: self.minimize_to_tray,
                    start_service: seconds > 0 && !self.tray_ready,
                    stop_service: seconds == 0 && !self.minimize_to_tray && self.tray_ready,
                });
            }
        }
    }

    // -------------------------------------------------------------- performance

    fn performance_page(&mut self, ui: &mut egui::Ui) {
        let profile_index = self.power_profile_index;
        let mut changed = false;
        let mut reset = false;
        let mut apply_requested = false;
        let applying = self.power_cap_applying;
        let selected = self.power_profiles[profile_index];
        let applied_matches =
            applied_matches_selection(self.last_applied_power, profile_index, selected);
        let chip = power_apply_state_label(power_apply_state(applying, applied_matches));
        let saving = self.profiles_saving || self.pending_profiles.is_some();
        let last_result = self.last_power_result.clone();

        card(ui, 0.0, |ui| {
            card_header(ui, "Power profiles");
            if let Some(index) = components::segmented_choice(
                ui,
                profile_index,
                &[
                    (0usize, POWER_MODE_NAMES[0]),
                    (1usize, POWER_MODE_NAMES[1]),
                    (2usize, POWER_MODE_NAMES[2]),
                ],
            ) {
                if index != profile_index {
                    self.select_power_profile(index);
                }
            }
            // Re-read after a possible selection change so the cap editors
            // below always target the profile that is now selected.
            let profile_index = self.power_profile_index;
            ui.add_space(4.0);
            if saving {
                ui.label(RichText::new("Saving profile…").size(11.0).color(WARM));
            } else {
                ui.label(
                    RichText::new(self.power_status_line())
                        .size(11.0)
                        .color(MUTED),
                );
            }

            ui.add_space(10.0);
            card_header(ui, "Caps for this profile");
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("CPU").size(12.0));
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.power_profiles[profile_index][0])
                            .range(1..=500)
                            .suffix(" W"),
                    )
                    .changed();
                ui.add_space(14.0);
                ui.label(RichText::new("GPU").size(12.0));
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.power_profiles[profile_index][1])
                            .range(1..=500)
                            .suffix(" W"),
                    )
                    .changed();
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Reset this profile to defaults")
                    .on_hover_text(format!(
                        "Restore CPU {} W · GPU {} W",
                        super::power_profiles::DEFAULTS[profile_index][0],
                        super::power_profiles::DEFAULTS[profile_index][1]
                    ))
                    .clicked()
                {
                    reset = true;
                }
                if ui.button("Reset all profiles to defaults").clicked() {
                    self.power_profiles = super::power_profiles::DEFAULTS;
                    self.queue_profile_save(true);
                }
            });
            if reset {
                self.power_profiles[profile_index] = super::power_profiles::DEFAULTS[profile_index];
                self.queue_profile_save(true);
            } else if changed {
                self.queue_profile_save(false);
            }

            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                let label = if applying { "Applying power caps…" } else { "Apply power caps" };
                if ui
                    .add_enabled(
                        !applying,
                        egui::Button::new(RichText::new(label).size(13.0).strong())
                            .min_size(egui::vec2(150.0, 36.0)),
                    )
                    .clicked()
                {
                    apply_requested = true;
                }
                ui.add_space(8.0);
                state_chip(ui, chip.0, chip.1);
            });
        });

        if apply_requested {
            self.request_power_apply();
        }

        ui.add_space(12.0);
        if let Some((success, cpu_message, gpu_message)) = &last_result {
            let color = if *success { COOL } else { HOT };
            card(ui, 0.0, |ui| {
                card_header(ui, if *success { "Last apply verified" } else { "Last apply rejected" });
                ui.label(RichText::new(truncate(cpu_message, 400)).size(12.0).color(color));
                ui.add_space(4.0);
                ui.label(RichText::new(truncate(gpu_message, 600)).size(12.0).color(color));
            });
            ui.add_space(12.0);
        }

        card(ui, 0.0, |ui| {
            card_header(ui, "How these limits are reported");
            for line in [
                "CPU values are written to Intel RAPL and read back; a readback proves the software request, not firmware enforcement or measured workload power.",
                "GPU success requires NVIDIA’s software-requested limit to read back at the requested value.",
                "Base/default limit, enforced ceiling, software-requested limit, driver-reported maximum, and live power draw are tracked and displayed separately.",
                "An enforced ceiling, the driver-reported maximum, or live draw is never treated as proof that a requested cap was accepted.",
                "This laptop’s NVIDIA driver reports “Changing power management limit is not supported in current scope” and exits 0; that is reported as a rejection, and the requested 75 W Performance target is kept.",
                "The driver-reported maximum is not labelled a Dynamic Boost ceiling: Dynamic Boost shares power dynamically and is not the same as a fixed software cap.",
            ] {
                ui.label(RichText::new(format!("· {line}")).size(11.0).color(MUTED));
                ui.add_space(3.0);
            }
        });
    }

    // ------------------------------------------------------------------ battery

    fn battery_page(&mut self, ui: &mut egui::Ui) {
        card(ui, 0.0, |ui| {
            card_header(ui, "Battery");
            ui.label(
                RichText::new(
                    "Battery charge management is planned for a future release. No charging thresholds are written today, so no values are shown here rather than invented ones.",
                )
                .color(MUTED),
            );
        });
    }

    // ----------------------------------------------------------------- settings

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        let mut tray_request: Option<bool> = None;
        let mut refresh = false;
        let tray_pending = self.tray_settings_pending;
        let minimize_to_tray = self.minimize_to_tray;
        let tray_ready = self.tray_ready;
        let idle_timeout_seconds = self.idle_timeout_seconds;
        let about_loading = self.about_loading;

        card(ui, 0.0, |ui| {
            card_header(ui, "Window behaviour");
            let mut enabled = minimize_to_tray;
            ui.add_enabled_ui(!tray_pending, |ui| {
                ui.checkbox(&mut enabled, "Minimize to system tray when closing the window");
            });
            if enabled != minimize_to_tray {
                tray_request = Some(enabled);
            }
            if tray_pending {
                ui.label(RichText::new("Saving…").size(11.0).color(WARM));
            } else if !tray_ready {
                ui.label(
                    RichText::new("Enabling this starts the background tray service for this session.")
                        .size(11.0)
                        .color(MUTED),
                );
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new("Login startup stays enabled while tray minimization or keyboard inactivity dimming is on. The background service starts hidden with a tray icon.")
                    .size(11.0)
                    .color(MUTED),
            );
        });

        if let Some(enabled) = tray_request {
            if !tray_pending {
                self.tray_settings_pending = true;
                self.dispatch(HardwareJob::SaveTrayPreference {
                    enabled,
                    idle_timeout_seconds,
                    start_service: enabled && !tray_ready,
                    stop_service: !enabled && idle_timeout_seconds == 0 && tray_ready,
                });
            }
        }

        ui.add_space(12.0);
        card(ui, 0.0, |ui| {
            card_header(ui, "System status");
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!about_loading, egui::Button::new(if about_loading { "Refreshing…" } else { "Refresh status" }))
                    .clicked()
                {
                    refresh = true;
                }
                ui.label(
                    RichText::new("Version and hardware information")
                        .size(11.0)
                        .color(MUTED),
                );
            });
            ui.add_space(8.0);
            egui::Grid::new("system-status")
                .num_columns(2)
                .striped(true)
                .spacing([24.0, 8.0])
                .show(ui, |ui| {
                    for (label, value) in &self.about_info.rows {
                        ui.label(RichText::new(label.as_str()).size(12.0).color(MUTED));
                        ui.label(RichText::new(value.as_str()).size(12.0));
                        ui.end_row();
                    }
                });
        });

        if refresh {
            self.about_loading = true;
            self.dispatch_query(HardwareJob::LoadAboutInfo);
        }

        ui.add_space(12.0);
        card(ui, 0.0, |ui| {
            card_header(ui, "Updates");
            ui.label("Check GitHub for a newer version. Download and installation stay under your control.");
            ui.add_space(6.0);
            if ui
                .add_enabled(
                    !self.update_checking,
                    egui::Button::new(if self.update_checking { "Checking…" } else { "Check for updates" }),
                )
                .clicked()
            {
                self.update_checking = true;
                self.update_status = "Checking GitHub Releases…".to_owned();
                self.latest_release = None;
                let sender = self.update_sender.clone();
                std::thread::spawn(move || {
                    let _ = sender.send(about::check_latest_release());
                });
            }
            ui.label(RichText::new(self.update_status.as_str()).size(12.0));
            if let Some(release) = &self.latest_release {
                ui.hyperlink_to("View release and downloads", &release.html_url);
            } else if !self.update_checking
                && self.update_status != "Updates are checked only when you ask."
                && self.update_status != "Checking GitHub Releases…"
            {
                ui.hyperlink_to("Open project releases", about::RELEASES_PAGE_URL);
            }
        });
    }
}

/// Keep long driver messages readable inside a card.
fn truncate(text: &str, maximum: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= maximum {
        trimmed.to_owned()
    } else {
        let cut: String = trimmed.chars().take(maximum.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
