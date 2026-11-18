# Acer Control Center

Modern Linux control center for supported Acer gaming laptops.

## Features

- Native desktop app (pygame) with three skins: PredatorSense replica
  (default), dark left icon rail, and AcerSense-style light mint
- Six tabs: dashboard with 270-degree gauges, cooling, keyboard RGB,
  power profiles, battery, settings
- Per-profile CPU (RAPL PL1 + PL2) and GPU (NVIDIA power limit) watt
  targets, written only through the D-Bus daemon and verified by
  hardware read-back; CPU and GPU report success independently
- Keyboard brightness, colour, breathing animation, idle timeout
- Fan automatic/maximum control
- Live telemetry from /proc, /sys, hwmon and nvidia-smi: temperatures,
  clocks, utilisation, fans (RPM or NVIDIA percent), VRAM, memory,
  storage, battery
- System D-Bus daemon (root service; every hardware write goes through it)
- DKMS kernel driver for the Acer keyboard/EC interface
- Browser frontend over the same Python backend (no compilation needed)
- Automatic installation: Rust daemon, C library, DKMS module, udev and
  D-Bus policy, systemd service, desktop entry

## Supported Hardware

Hardware reported/tested by users:

- Acer ALG AL15G-53
- Acer Aspire 7 A715-79G (Core 5 210H, GeForce RTX 3050 6 GB; keyboard
  backlight driver reported working; CPU/GPU power-limit controls have not yet
  been validated on hardware)

The daemon detects Acer systems exposing the `acer_acpi` interface; individual
controls still depend on the laptop firmware and Linux driver support. CPU
power caps use Intel RAPL, while NVIDIA GPU caps use `nvidia-smi`. Either control
may be unavailable or reject a requested value on a particular laptop; the
Performance page reports this instead of claiming an unsupported cap was set.

The graphical application does not require GTK or Libadwaita. It uses the
system's X11 or Wayland session and communicates with the existing system
daemon over D-Bus. Linux graphics libraries (including X11/Wayland and
xkbcommon) are still supplied by the distribution. The source installer
supports Debian/Ubuntu, Arch, Fedora/RHEL, and openSUSE package managers. It
builds the application from Rust sources and requires Rust 1.88 or newer; when
the distribution Rust package is older, the installer sets up the stable
toolchain under `/opt/acer-control-center`.

Prebuilt packages are produced for Debian (`.deb`), RPM (`.rpm`), and
Arch (`.pkg.tar.zst`). Packages include the application, system daemon, DKMS
driver sources, desktop entry, and service integration. Kernel headers matching
the running kernel are needed to build the DKMS driver. If the package hook
reports missing headers, install them and run `sudo dkms autoinstall`.

These packages cover the listed packaging families, not every Linux
distribution. Other distributions need a compatible package manager and
dependencies or a distribution-specific recipe.

## Installation

Clone the repository

```bash
git clone https://github.com/stanlysilas/acer-control-center.git
cd acer-control-center
sudo ./install.sh
```

or

```bash
wget https://raw.githubusercontent.com/stanlysilas/acer-control-center/master/install.sh
chmod +x install.sh
sudo ./install.sh
```

or

**Download the package for your distribution from the [Releases](https://github.com/stanlysilas/acer-control-center/releases/latest) page. (Recommended)**

After installation, run

```bash
sudo usermod -aG acercontrol yourusername
```

And reboot the laptop when convenient.

The application version and device status are available in **Settings**. Use
**Check for updates** to query the latest published GitHub release; the app
provides a release link and leaves downloading and installation to you.

Keyboard color, brightness, breathing animation, and the selected fan mode are
restored by the system daemon after it starts. The keyboard inactivity timeout
is saved per user. When enabled, a small background process starts in the
system tray at desktop login; choosing **Off** removes that login startup
entry. The **Minimize to system tray when closing the window** option is
available in Settings. When enabled, closing the window exits the UI process
while leaving the tray and inactivity controller running. The tray menu can
reopen the window or quit the background process. This separate process model
also supports Wayland, where native windows cannot be hidden after mapping. If
the desktop session has no StatusNotifier tray host, the app opens visibly so
it stays accessible.

If Secure Boot is enabled, or if you plan to enable it later, complete the MOK enrollment described in the Additional Setup section.

The installer does not reboot the system automatically.

## Additional Setup

### Secure Boot and DKMS Module Signing

Acer Control Center uses DKMS to build its kernel modules. The modules are signed with a local DKMS Machine Owner Key (MOK) so they can be loaded when UEFI Secure Boot is enabled.

The source installer creates the DKMS signing key when necessary. Package
installs also create it when Secure Boot is enabled. MOK enrollment is
intentionally left to the user.

The installer does not change Secure Boot settings and does not reboot the system automatically.

If Secure Boot is enabled, or if you plan to enable it later, enroll the DKMS certificate:

```bash
sudo mokutil --import /var/lib/dkms/mok.pub
```

You will be asked to create a temporary enrollment password.

Reboot when convenient and complete the enrollment through the UEFI MOK Manager:

1. Select `Enroll MOK`.
2. Select `Continue`.
3. Select `Yes`.
4. Enter the password created with `mokutil --import`.
5. Reboot.

After the system starts, verify that the certificate is enrolled:

```bash
sudo mokutil --test-key /var/lib/dkms/mok.pub
```

You can also verify that the Acer module is signed:

```bash
modinfo acer_keyboard | grep -E '^(signer|sig_key|sig_hashalgo):'
```

A correctly signed module should show:

```text
signer:         DKMS module signing key
sig_key:        <your enrolled MOK fingerprint>
sig_hashalgo:   sha512
```

If Secure Boot is enabled and the MOK has not been enrolled, the kernel will reject the Acer modules with an error such as:

```text
Key was rejected by service
```

No reinstallation is required after enrolling the MOK. The existing signed modules can be loaded normally.

### Kernel Taint and Device Security

Because Acer Control Center's DKMS modules are built outside the upstream Linux kernel tree, Linux marks the running kernel as tainted when an Acer module is loaded.

Therefore, even when:

- Secure Boot is enabled
- the Acer modules are cryptographically signed
- the MOK is enrolled
- the modules load successfully

Fedora's device security report may still show:

```text
Linux Kernel Verification: ! Fail (Tainted)
```

This is expected for an out-of-tree DKMS driver.

The kernel's taint value distinguishes an out-of-tree module from an unsigned module. An out-of-tree module sets the `4096` taint bit, while an unsigned module sets the `8192` taint bit.

You can inspect the current taint state with:

```bash
cat /proc/sys/kernel/tainted
```

You can verify Secure Boot with:

```bash
mokutil --sb-state
```

And verify the Acer module signature with:

```bash
modinfo acer_keyboard | grep -E '^(signer|sig_key|sig_hashalgo):'
```

A kernel taint caused by a correctly signed out-of-tree module does not mean that Secure Boot rejected the module or that the module is unsigned.

Removing this particular taint would require the driver to become an upstream/in-tree Linux kernel driver. The taint should not be manually cleared because it is a diagnostic state maintained by the kernel.

## Desktop app (recommended)

A native window built with pygame over the same Python backend as the browser
frontend: one pip package, no compilation, no system libraries beyond what
pygame wheels bundle.

```bash
pip install pygame
python3 -m desktopgui                 # opens a 1280x800 window
python3 -m desktopgui --size 1000x700
python3 -m desktopgui --theme dark    # or: light
```

The default skin replicates Acer's PredatorSense look (charcoal panels, cyan
accent, notched left navigation, 270-degree gauges); `--theme dark` is the
left-icon-rail look and `--theme light` the AcerSense-style mint look.

Tabs switch with the mouse or keys 1-6; Esc or Q quits. Hardware calls go to
acer-control-daemon over the system D-Bus via busctl; telemetry comes from
/proc and /sys. When the daemon is absent the window still opens and shows a
banner.

The UI can render without a display, which is how it is tested in CI-less
sandboxes and how you can capture every tab yourself:

```bash
SDL_VIDEODRIVER=dummy python3 -m desktopgui --screenshot /tmp/shots
SDL_VIDEODRIVER=dummy python3 -m unittest discover -s desktopgui/tests -v
```

The test suite drives synthetic mouse and keyboard events through the real
event path against a fake busctl, and asserts that no card ever overlaps or
clips on any tab.

`install.sh` also installs a freedesktop `.desktop` entry so the app appears
in your launcher. A user copy in `~/.local/share/applications/` with the same
filename overrides the system one - handy to point the launcher at a
different wrapper (`which -a acer-control-center` lists every installed one).

## Power profiles and hardware limits (configuration)

Every user setting lives in one folder, `~/.config/acer-control-center/`:

| File | Contents |
| --- | --- |
| `power-profiles` | one line, seven integers: `p0cpu p0gpu p1cpu p1gpu p2cpu p2gpu selected` |
| `keyboard-idle-timeout` | seconds of inactivity before the backlight fades; the daemon restores it on activity |
| `minimize-to-tray` | tray behaviour flag |

`power-profiles` stores the CPU and GPU watt targets of the three profiles
(0 = Performance, 1 = Balanced, 2 = Battery Saver) plus which one is selected
(0-2). Defaults: `40 75 30 60 15 30 0`. Each watt field accepts 1-500; a
missing or malformed file falls back to the defaults. Edit it with any text
editor, then select that profile in the app and press **Apply** - or use the
1-500 W steppers and *Save profiles* button on the Performance tab, which
write the same file.

What **Apply** does per device:

- **CPU** - writes the watts to both RAPL constraints (PL1 and PL2) under
  `/sys/class/powercap/intel-rapl:0/` and verifies by reading the value back.
  Firmware sets a hard ceiling; see it with
  `cat /sys/class/powercap/intel-rapl:0/constraint_0_max_power_uw`
  (45 W on many Acer laptops). Requests above the ceiling are refused by the
  kernel and reported as `FAIL: CPU cap not applied: ...`, so the file can
  hold any number but only firmware-permitted ones stick.
- **GPU** - sets the NVIDIA power limit via `nvidia-smi` and verifies the
  software-requested limit by reading it back. Mobile drivers expose an
  *enforced ceiling* (`enforced.power.limit`) that can sit below the reported
  maximum; when a request above it is refused with the driver's
  "not supported in current scope" rejection (which exits 0!), the daemon
  retries once at the ceiling and reports
  `requested 75 W, applied 70 W (driver-enforced ceiling)`. Exit-code-only
  success is never trusted: a table of rejection phrases catches drivers
  that refuse while exiting 0.

Each device answers with its own `OK:` / `FAIL:` line and its own colour in
both frontends, so a refused GPU no longer paints a successful CPU write red.

Worth knowing:

- Plug/unplug events can let firmware reprogram RAPL on their own; press
  Apply again after switching power source if a value looks stale.
- Nothing else on the system should write these registers. Custom cap
  scripts, `thermald` or DPTF stacks will fight the daemon - see
  Troubleshooting below.

## Browser frontend (no compile needed)

Alongside the native egui application there is a dependency-free browser
frontend under `webgui/`. It is plain Python (standard library only) plus
HTML/CSS/JS: a small HTTP server serves the page and proxies hardware calls to
`acer-control-daemon` over the system D-Bus using `busctl`, and reads telemetry
straight from `/proc` and `/sys`. Nothing is compiled, so it runs anywhere
Python 3.9+ runs.

```bash
python3 -m webgui.acercontrol            # http://127.0.0.1:8765
python3 -m webgui.acercontrol --port 9000
python3 -m webgui.acercontrol --host 0.0.0.0   # share on the LAN
```

Open the printed address in any browser. When the daemon is not on the bus the
page still renders and shows a banner; telemetry keeps working because it does
not depend on the daemon.

Run the backend tests (they stub `busctl`, so no hardware is needed):

```bash
python3 -m unittest discover -s webgui/tests -v
```

## Troubleshooting

- **Daemon offline pill / banner** - `systemctl status
  acer-control-daemon.service`; re-run `sudo ./install.sh` to restore the
  service, D-Bus and udev policy files.
- **Fans read "Not exposed by kernel"** - many Acer laptops expose no hwmon
  fan inputs; the GPU fan then falls back to the NVIDIA percentage and the
  CPU row says so honestly instead of guessing.
- **A power cap reverts a few seconds after Apply** - another writer owns the
  register (a custom loop service, thermald, DPTF). Name it with
  `pgrep -af 'cap|thermald|dptf'` and
  `systemctl list-units --type=service | grep -iE 'acer|thermald|dptf'`,
  then disable it, or fold its intent into `power-profiles` instead.
  Watch the register live with
  `watch -n 2 'grep . /sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw'`.
- **Launcher opens a different build than the terminal** - desktop entries in
  `~/.local/share/applications/` override system ones; set `Exec=` to the
  wrapper you want (`which -a acer-control-center` lists them in PATH order).

## Uninstall

```bash
sudo ./uninstall.sh
```

## Project Structure

```text
driver/
daemon/
gui/
lib/
packaging/
```

## Current Status

### Implemented

- Dashboard
- Keyboard RGB
- D-Bus daemon
- Driver communication
- Live fan monitoring

### Planned

- Battery management
- Firmware updates

## License

GPL-3.0-or-later

## Discord Community

The **[AcerCtl](https://discord.gg/XjnjN25QED)** Discord server is used for community discussion, support, bug reports, feature requests, and project announcements.