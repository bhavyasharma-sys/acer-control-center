/* Acer Control Center frontend. Vanilla JS, no build step.
   Polls /api/telemetry and /api/state; every missing reading renders as
   "Unavailable" instead of a fake zero. */
"use strict";

const el = (id) => document.getElementById(id);
const FAN_FULL_SCALE = 6000;
const GAUGE_ARC_LENGTH = Math.PI * 80; // matches the SVG path radius

let state = null;
let toastTimer = null;

/* ------------------------------------------------------------- formatting */

function fmt(value, unit, decimals = 0) {
  if (value === null || value === undefined) return "Unavailable";
  const number = decimals ? Number(value).toFixed(decimals) : String(value);
  return unit ? `${number} ${unit}` : number;
}

function gib(mib) {
  return (mib / 1024).toFixed(1);
}

function toast(message, bad = false) {
  const node = el("toast");
  node.textContent = message;
  node.classList.toggle("bad", bad);
  node.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => node.classList.add("hidden"), 4000);
}

/* ------------------------------------------------------------------ bars */

function setBar(barId, valueId, fraction, text, missing) {
  const bar = el(barId);
  const label = el(valueId);
  if (missing || fraction === null || fraction === undefined) {
    bar.style.width = "100%";
    bar.classList.add("missing");
  } else {
    bar.classList.remove("missing");
    bar.style.width = `${Math.max(0, Math.min(100, fraction * 100))}%`;
  }
  label.textContent = text;
}

/* ----------------------------------------------------------------- gauge */

function setGauge(fraction, readout, maxLabel) {
  const arc = el("gauge-arc");
  const needle = el("gauge-needle");
  arc.style.strokeDasharray = String(GAUGE_ARC_LENGTH);
  if (fraction === null || fraction === undefined) {
    arc.style.strokeDashoffset = String(GAUGE_ARC_LENGTH);
    needle.setAttribute("transform", "rotate(-90 100 110)");
  } else {
    const clamped = Math.max(0, Math.min(1, fraction));
    arc.style.strokeDashoffset = String(GAUGE_ARC_LENGTH * (1 - clamped));
    needle.setAttribute("transform", `rotate(${-90 + 180 * clamped} 100 110)`);
  }
  el("gauge-readout").textContent = readout;
  el("gauge-max").textContent = maxLabel;
}

/* ------------------------------------------------------------- segmented */

function buildSegmented(containerId, options, selected, onPick) {
  const container = el(containerId);
  container.textContent = "";
  options.forEach((option) => {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = option.label;
    button.classList.toggle("selected", option.value === selected);
    button.addEventListener("click", () => onPick(option.value));
    container.appendChild(button);
  });
}

/* ------------------------------------------------------------------- tabs */

document.querySelectorAll(".tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((other) =>
      other.classList.toggle("active", other === tab)
    );
    document.querySelectorAll(".tabpanel").forEach((panel) =>
      panel.classList.toggle("hidden", panel.id !== `tab-${tab.dataset.tab}`)
    );
  });
});

/* ------------------------------------------------------------------ fetch */

async function getJson(path) {
  const response = await fetch(path, { cache: "no-store" });
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  return response.json();
}

async function postJson(path, body) {
  const response = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return response.json();
}

/* --------------------------------------------------------------- renderers */

function renderTelemetry(t) {
  el("v-cpu-clock").textContent = fmt(t.cpu_clock_mhz, "MHz");
  el("v-cpu-temp").textContent = fmt(t.cpu_temperature_c, "°C");
  el("v-gpu-clock").textContent = fmt(t.gpu_clock_mhz, "MHz");
  el("v-gpu-temp").textContent = fmt(t.gpu_temperature_c, "°C");

  setBar("bar-cpu", "v-cpu-use",
    t.cpu_usage_percent === null ? null : t.cpu_usage_percent / 100,
    fmt(t.cpu_usage_percent, "%"), t.cpu_usage_percent === null);
  setBar("bar-gpu", "v-gpu-use",
    t.gpu_usage_percent === null ? null : t.gpu_usage_percent / 100,
    fmt(t.gpu_usage_percent, "%"), t.gpu_usage_percent === null);

  setBar("bar-cpu-fan", "v-cpu-fan",
    t.cpu_fan_rpm === null ? null : t.cpu_fan_rpm / FAN_FULL_SCALE,
    fmt(t.cpu_fan_rpm, "RPM"), t.cpu_fan_rpm === null);
  setBar("bar-gpu-fan", "v-gpu-fan",
    t.gpu_fan_rpm === null ? null : t.gpu_fan_rpm / FAN_FULL_SCALE,
    fmt(t.gpu_fan_rpm, "RPM"), t.gpu_fan_rpm === null);

  const mem = t.memory_mib;
  setBar("bar-mem", "v-mem", mem ? mem[0] / mem[1] : null,
    mem ? `${gib(mem[0])} / ${gib(mem[1])} GiB` : "Unavailable", !mem);
  const disk = t.storage_mib;
  setBar("bar-disk", "v-disk", disk ? disk[0] / disk[1] : null,
    disk ? `${gib(disk[0])} / ${gib(disk[1])} GiB` : "Unavailable", !disk);
  const vram = t.gpu_memory_mib;
  setBar("bar-vram", "v-vram", vram ? vram[0] / vram[1] : null,
    vram ? `${vram[0]} / ${vram[1]} MiB` : "Unavailable", !vram);

  const fraction = (t.cpu_clock_mhz !== null && t.cpu_clock_max_mhz)
    ? t.cpu_clock_mhz / t.cpu_clock_max_mhz : null;
  setGauge(fraction, fmt(t.cpu_clock_mhz, ""), fmt(t.cpu_clock_max_mhz, ""));

  el("c-cpu-fan").textContent = fmt(t.cpu_fan_rpm, "RPM");
  el("c-gpu-fan").textContent = fmt(t.gpu_fan_rpm, "RPM");
  el("c-cpu-temp").textContent = fmt(t.cpu_temperature_c, "°C");
  el("c-gpu-temp").textContent = fmt(t.gpu_temperature_c, "°C");
  el("c-gpu-power").textContent = fmt(t.gpu_power_watts, "W", 1);

  const batt = t.battery;
  el("b-percent").textContent = batt ? fmt(batt.percent, "%") : "Unavailable";
  el("b-status").textContent = batt && batt.status ? batt.status : "Unavailable";
  el("b-gpu-power").textContent = fmt(t.gpu_power_watts, "W", 1);
  setBar("bar-batt", "v-batt", batt && batt.percent !== null ? batt.percent / 100 : null,
    batt ? fmt(batt.percent, "%") : "Unavailable", !batt || batt.percent === null);
}

function applyChipText() {
  if (!state) return ["—", ""];
  const last = state.last_apply;
  if (!last) return ["not applied", "warn"];
  const matches = last.ok && last.index === state.selected_profile;
  if (!last.ok) return ["not applied", "bad"];
  return matches ? ["applied", "ok"] : ["applied (other mode)", "warn"];
}

function renderState(s) {
  state = s;

  const pill = el("conn-pill");
  pill.textContent = s.daemon_available ? "daemon online" : "daemon offline";
  pill.classList.toggle("ok", !!s.daemon_available);
  pill.classList.toggle("bad", !s.daemon_available);
  el("version").textContent = `v${s.frontend_version}`;

  const banner = el("banner");
  if (s.daemon_available) {
    banner.classList.add("hidden");
  } else {
    banner.classList.remove("hidden");
    banner.textContent =
      `Hardware control unavailable: ${s.daemon_error || "the daemon is not on the system bus"}. ` +
      "Telemetry still works; start acer-control-daemon to control fans, keyboard and power caps.";
  }

  const modes = s.profile_names.map((name, index) => ({ label: name, value: index }));
  buildSegmented("mode-segments", modes, s.selected_profile, selectProfile);

  const fanOptions = [
    { label: "Automatic", value: "auto" },
    { label: "Maximum", value: "max" },
  ];
  const fanSelected = s.fan_max === true ? "max" : s.fan_max === false ? "auto" : null;
  buildSegmented("fan-segments", fanOptions, fanSelected, setFan);
  buildSegmented("cooling-fan-segments", fanOptions, fanSelected, setFan);

  const [chipText, chipClass] = applyChipText();
  const chip = el("apply-state");
  chip.textContent = chipText;
  chip.className = `chip ${chipClass}`.trim();

  const pair = s.profiles[s.selected_profile] || [null, null];
  el("target-name").textContent = s.profile_names[s.selected_profile] || "—";
  el("v-target-cpu").textContent = fmt(pair[0], "W");
  el("v-target-gpu").textContent = fmt(pair[1], "W");

  const last = s.last_apply;
  const resultBox = el("apply-result");
  if (last) {
    resultBox.classList.remove("hidden");
    resultBox.classList.toggle("bad", !last.ok);
    resultBox.textContent = last.ok
      ? `CPU: ${last.cpu_message}\nGPU: ${last.gpu_message}`
      : (last.error || "apply failed");
  } else {
    resultBox.classList.add("hidden");
  }

  // keyboard
  if (s.keyboard_brightness !== null && s.keyboard_brightness !== undefined) {
    el("kb-brightness").value = String(s.keyboard_brightness);
    el("kb-brightness-value").textContent = fmt(s.keyboard_brightness, "%");
  } else {
    el("kb-brightness-value").textContent = "Unavailable";
  }
  el("kb-breathing").textContent =
    s.breathing === null ? "Breathing: unknown" : `Breathing: ${s.breathing ? "on" : "off"}`;

  // profiles table
  renderProfileTable(s);

  // settings
  el("s-frontend").textContent = s.frontend_version;
  el("s-daemon").textContent = s.daemon_version || "Unavailable";
  el("s-reachable").textContent = s.daemon_available ? "yes" : "no";

  getJson("/api/about").then((about) => {
    el("s-busctl").textContent = about.busctl ? "yes" : "no";
  }).catch(() => {});
}

function renderProfileTable(s) {
  const body = el("profile-rows");
  body.textContent = "";
  s.profiles.forEach((pair, index) => {
    const row = document.createElement("tr");
    const nameCell = document.createElement("td");
    nameCell.textContent = s.profile_names[index];
    row.appendChild(nameCell);
    // Each profile is a [cpu, gpu] wattage pair.
    ["cpu", "gpu"].forEach((slot, column) => {
      const cell = document.createElement("td");
      const input = document.createElement("input");
      input.type = "number";
      input.min = "1";
      input.max = "500";
      input.dataset.profile = String(index);
      input.dataset.slot = slot;
      input.value = String(pair[column]);
      cell.appendChild(input);
      row.appendChild(cell);
    });
    const radioCell = document.createElement("td");
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "profile-selected";
    radio.checked = index === s.selected_profile;
    radio.addEventListener("change", () => selectProfile(index));
    radioCell.appendChild(radio);
    row.appendChild(radioCell);
    body.appendChild(row);
  });
}

/* ----------------------------------------------------------------- actions */

async function refreshState() {
  try {
    renderState(await getJson("/api/state"));
  } catch (error) {
    el("conn-pill").textContent = "server error";
    el("conn-pill").classList.add("bad");
  }
}

async function selectProfile(index) {
  const current = state ? state.profiles : [[40, 75], [30, 60], [15, 30]];
  const result = await postJson("/api/profiles", { profiles: current, selected: index });
  if (!result.ok) toast(result.error || "could not store the selection", true);
  await refreshState();
}

async function setFan(mode) {
  const result = await postJson("/api/fan", { mode });
  const box = el("cooling-status");
  box.classList.remove("hidden");
  box.classList.toggle("bad", !result.ok);
  box.textContent = result.ok
    ? `Fan set to ${mode}.`
    : (result.error || "the daemon refused the request");
  if (!result.ok) toast(result.error || "fan change failed", true);
  await refreshState();
}

async function applyPower() {
  const button = el("apply-btn");
  button.disabled = true;
  button.textContent = "Applying…";
  try {
    const result = await postJson("/api/power/apply", { index: state.selected_profile });
    if (result.ok) toast("Power caps applied and verified by readback.");
    else toast(result.error || "power caps were not applied", true);
  } finally {
    button.disabled = false;
    button.textContent = "Apply";
    await refreshState();
  }
}

/* ------------------------------------------------------------- wiring once */

el("apply-btn").addEventListener("click", applyPower);

const SWATCHES = ["#00e5ff", "#ff2d55", "#34e07a", "#ffb300", "#8a5cff", "#ffffff"];
const swatchBox = el("kb-swatches");
SWATCHES.forEach((hex) => {
  const button = document.createElement("button");
  button.type = "button";
  button.style.background = hex;
  button.title = hex;
  button.addEventListener("click", async () => {
    const rgb = hexToRgb(hex);
    const result = await postJson("/api/keyboard/color", rgb);
    if (!result.ok) toast(result.error || "colour change failed", true);
    else toast(`Keyboard colour set to ${hex}.`);
    await refreshState();
  });
  swatchBox.appendChild(button);
});

function hexToRgb(hex) {
  const value = hex.replace("#", "");
  return {
    r: parseInt(value.slice(0, 2), 16),
    g: parseInt(value.slice(2, 4), 16),
    b: parseInt(value.slice(4, 6), 16),
  };
}

el("kb-color-apply").addEventListener("click", async () => {
  const result = await postJson("/api/keyboard/color", hexToRgb(el("kb-color").value));
  if (!result.ok) toast(result.error || "colour change failed", true);
  await refreshState();
});

let brightnessTimer = null;
el("kb-brightness").addEventListener("input", (event) => {
  el("kb-brightness-value").textContent = `${event.target.value} %`;
  clearTimeout(brightnessTimer);
  brightnessTimer = setTimeout(async () => {
    const result = await postJson("/api/keyboard/brightness",
      { value: Number(event.target.value) });
    if (!result.ok) toast(result.error || "brightness change failed", true);
  }, 200);
});

el("kb-breathing").addEventListener("click", async () => {
  const turningOn = !(state && state.breathing);
  const result = await postJson("/api/keyboard/animation",
    { name: turningOn ? "breathing" : "off" });
  if (!result.ok) toast(result.error || "animation change failed", true);
  await refreshState();
});

const idleSelect = el("kb-idle");
[0, 5, 10, 15, 20, 25, 30].forEach((seconds) => {
  const option = document.createElement("option");
  option.value = String(seconds);
  option.textContent = seconds === 0 ? "Off" : `${seconds} seconds`;
  idleSelect.appendChild(option);
});
idleSelect.addEventListener("change", async () => {
  const result = await postJson("/api/keyboard/idle",
    { seconds: Number(idleSelect.value) });
  if (!result.ok) toast(result.error || "could not store the idle timeout", true);
  else toast("Idle timeout stored.");
  await refreshState();
});

el("profile-save").addEventListener("click", async () => {
  const gathered = [[null, null], [null, null], [null, null]];
  document.querySelectorAll("#profile-rows input[type=number]").forEach((input) => {
    const index = Number(input.dataset.profile);
    const value = Number(input.value);
    if (input.dataset.slot === "cpu") gathered[index][0] = value;
    else if (input.dataset.slot === "gpu") gathered[index][1] = value;
  });
  const selected = state ? state.selected_profile : 2;
  const result = await postJson("/api/profiles", { profiles: gathered, selected });
  const status = el("profile-status");
  status.textContent = result.ok ? "Saved." : (result.error || "Save failed.");
  if (!result.ok) toast(result.error || "Save failed.", true);
  await refreshState();
});

/* ------------------------------------------------------------------- loops */

async function pollTelemetry() {
  try {
    renderTelemetry(await getJson("/api/telemetry"));
  } catch (error) {
    /* the server answers on the next tick */
  }
}

pollTelemetry();
refreshState();
setInterval(pollTelemetry, 1500);
setInterval(refreshState, 5000);
