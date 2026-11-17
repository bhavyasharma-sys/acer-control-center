use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MICROWATTS_PER_WATT: u64 = 1_000_000;

fn write_rapl_constraint(path: &Path, watts: u32) -> Result<(), String> {
    let requested = u64::from(watts)
        .checked_mul(MICROWATTS_PER_WATT)
        .ok_or_else(|| "CPU power limit is out of range".to_string())?;
    let min_path = path.with_file_name(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .replace("power_limit_uw", "min_power_uw"),
    );
    let max_path = path.with_file_name(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .replace("power_limit_uw", "max_power_uw"),
    );

    if let Ok(value) = fs::read_to_string(&min_path) {
        let min = value.trim().parse::<u64>().map_err(|e| e.to_string())?;
        if min > 0 && requested < min {
            return Err(format!("CPU cap is below this RAPL constraint minimum ({:.1} W)", min as f64 / MICROWATTS_PER_WATT as f64));
        }
    }
    if let Ok(value) = fs::read_to_string(&max_path) {
        let max = value.trim().parse::<u64>().map_err(|e| e.to_string())?;
        if max > 0 && requested > max {
            return Err(format!("CPU cap exceeds this RAPL constraint maximum ({:.1} W)", max as f64 / MICROWATTS_PER_WATT as f64));
        }
    }

    fs::write(path, requested.to_string())
        .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    let actual = fs::read_to_string(path)
        .map_err(|e| format!("Could not verify {}: {e}", path.display()))?
        .trim()
        .parse::<u64>()
        .map_err(|e| format!("Could not parse the value read back from {}: {e}", path.display()))?;
    if actual != requested {
        return Err(format!(
            "RAPL read back {:.1} W instead of the requested {watts} W",
            actual as f64 / MICROWATTS_PER_WATT as f64
        ));
    }
    Ok(())
}

fn rapl_package_dirs() -> Result<Vec<PathBuf>, String> {
    let root = Path::new("/sys/class/powercap");
    let entries = fs::read_dir(root)
        .map_err(|e| format!("Intel RAPL powercap is unavailable: {e}"))?;
    let mut packages = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("intel-rapl:") && !name[name.find(':').unwrap_or(name.len()) + 1..].contains(':') {
            packages.push(entry.path());
        }
    }
    packages.sort();
    if packages.is_empty() {
        return Err("No Intel RAPL package power controls were found".into());
    }
    Ok(packages)
}

pub fn set_cpu_power_cap(watts: u32) -> Result<(), String> {
    if !(1..=500).contains(&watts) {
        return Err("CPU cap must be between 1 and 500 W".into());
    }

    let packages = rapl_package_dirs()?;
    let mut limits = Vec::new();

    for package in packages {
        let entries = fs::read_dir(&package)
            .map_err(|e| format!("Could not inspect {}: {e}", package.display()))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(index) = name
                .strip_prefix("constraint_")
                .and_then(|rest| rest.strip_suffix("_name"))
            else {
                continue;
            };
            let constraint_name = fs::read_to_string(entry.path()).unwrap_or_default();
            if matches!(constraint_name.trim(), "long_term" | "short_term") {
                limits.push(package.join(format!("constraint_{index}_power_limit_uw")));
            }
        }
    }

    if limits.is_empty() {
        return Err("RAPL package has no writable long-term or short-term power limits".into());
    }

    let old_values: Vec<String> = limits
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .map_err(|e| format!("Could not read {}: {e}", path.display()))
        })
        .collect::<Result<_, _>>()?;

    for (index, path) in limits.iter().enumerate() {
        if let Err(error) = write_rapl_constraint(path, watts) {
            for rollback_index in (0..=index).rev() {
                let _ = fs::write(&limits[rollback_index], old_values[rollback_index].trim());
            }
            return Err(error);
        }
    }
    Ok(())
}

fn nvidia_power_range() -> Result<(f64, f64), String> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=power.min_limit,power.max_limit",
            "--format=csv,noheader,nounits",
            "-i",
            "0",
        ])
        .output()
        .map_err(|e| format!("Could not run nvidia-smi: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Could not query NVIDIA power limits: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(line) = text.lines().next() else {
        return Err("nvidia-smi returned no GPU power-limit information".into());
    };
    let values: Vec<_> = line.split(',').map(str::trim).collect();
    if values.len() != 2 {
        return Err("nvidia-smi returned an unexpected power-limit format".into());
    }
    let min = values[0]
        .parse::<f64>()
        .map_err(|_| "NVIDIA driver does not expose a configurable minimum power limit".to_string())?;
    let max = values[1]
        .parse::<f64>()
        .map_err(|_| "NVIDIA driver does not expose a configurable maximum power limit".to_string())?;
    Ok((min, max))
}

fn nvidia_current_power_limit() -> Result<f64, String> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=power.limit",
            "--format=csv,noheader,nounits",
            "-i",
            "0",
        ])
        .output()
        .map_err(|e| format!("Could not query the NVIDIA power limit: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Could not read back the NVIDIA power limit: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .ok_or_else(|| "nvidia-smi returned no current power limit".to_string())?
        .trim()
        .parse::<f64>()
        .map_err(|_| "NVIDIA driver does not report a numeric current power limit".into())
}

pub fn set_gpu_power_cap(watts: u32) -> Result<(), String> {
    if !(1..=500).contains(&watts) {
        return Err("GPU cap must be between 1 and 500 W".into());
    }
    let (min, max) = nvidia_power_range()?;
    if f64::from(watts) < min || f64::from(watts) > max {
        return Err(format!("GPU cap is outside the NVIDIA-supported range ({min:.0}–{max:.0} W)"));
    }

    let watts_arg = watts.to_string();
    let output = Command::new("nvidia-smi")
        .args(["-i", "0", "--power-limit"])
        .arg(watts_arg)
        .output()
        .map_err(|e| format!("Could not run nvidia-smi: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "NVIDIA GPU rejected the power limit: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let actual = nvidia_current_power_limit()?;
    if (actual - f64::from(watts)).abs() > 0.5 {
        return Err(format!(
            "NVIDIA accepted the command but reports a current limit of {actual:.0} W, not the requested {watts} W"
        ));
    }
    Ok(())
}
