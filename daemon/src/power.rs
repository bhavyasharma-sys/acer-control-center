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

#[derive(Clone, Debug, Default, PartialEq)]
struct NvidiaPowerInfo {
    min_limit_w: Option<f64>,
    max_limit_w: Option<f64>,
    default_limit_w: Option<f64>,
    enforced_limit_w: Option<f64>,
    software_requested_limit_w: Option<f64>,
    power_draw_w: Option<f64>,
}

#[derive(Clone, Debug)]
struct CommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

trait CommandExecutor {
    fn execute(&self, args: &[String]) -> Result<CommandOutput, String>;
}

struct SystemCommandExecutor;

impl CommandExecutor for SystemCommandExecutor {
    fn execute(&self, args: &[String]) -> Result<CommandOutput, String> {
        let output = Command::new("nvidia-smi")
            .args(args)
            .output()
            .map_err(|error| format!("Could not run nvidia-smi: {error}"))?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn build_query_power_args() -> Vec<String> {
    [
        "--query-gpu=power.min_limit,power.max_limit,power.default_limit,enforced.power.limit,power.limit,power.draw",
        "--format=csv,noheader,nounits",
        "-i",
        "0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn build_detailed_power_args() -> Vec<String> {
    ["-q", "-d", "POWER", "-i", "0"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn build_set_power_limit_args(watts: u32) -> Vec<String> {
    vec![
        "-i".into(),
        "0".into(),
        "--power-limit".into(),
        watts.to_string(),
    ]
}

fn parse_optional_watts(value: &str) -> Option<f64> {
    let value = value.trim().trim_matches(|character| character == '[' || character == ']').trim();
    if value.is_empty()
        || value.eq_ignore_ascii_case("N/A")
        || value.eq_ignore_ascii_case("Not Supported")
        || value.eq_ignore_ascii_case("Not Available")
    {
        return None;
    }
    value
        .split_whitespace()
        .next()
        .and_then(|number| number.parse::<f64>().ok())
}

fn parse_csv_power_query(text: &str) -> Result<NvidiaPowerInfo, String> {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "nvidia-smi returned no GPU power information".to_string())?;
    let values: Vec<_> = line.split(',').map(str::trim).collect();
    if values.len() != 6 {
        return Err(format!(
            "nvidia-smi returned {} power fields; expected 6",
            values.len()
        ));
    }
    let parse = |index| parse_optional_watts(values[index]);
    Ok(NvidiaPowerInfo {
        min_limit_w: parse(0),
        max_limit_w: parse(1),
        default_limit_w: parse(2),
        enforced_limit_w: parse(3),
        software_requested_limit_w: parse(4),
        power_draw_w: parse(5),
    })
}

fn parse_detailed_power_query(text: &str) -> Result<NvidiaPowerInfo, String> {
    let mut info = NvidiaPowerInfo::default();
    let mut found_power_field = false;
    for line in text.lines() {
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        let watts = parse_optional_watts(value);
        match label.trim().to_ascii_lowercase().as_str() {
            "min power limit" => {
                info.min_limit_w = watts;
                found_power_field = true;
            }
            "max power limit" => {
                info.max_limit_w = watts;
                found_power_field = true;
            }
            "default power limit" => {
                info.default_limit_w = watts;
                found_power_field = true;
            }
            "enforced power limit" => {
                info.enforced_limit_w = watts;
                found_power_field = true;
            }
            "current power limit" => {
                // Older drivers expose only the current ceiling under this name.
                if info.enforced_limit_w.is_none() {
                    info.enforced_limit_w = watts;
                }
                found_power_field = true;
            }
            "requested power limit" | "software requested power limit" => {
                info.software_requested_limit_w = watts;
                found_power_field = true;
            }
            "power draw" | "average power draw" => {
                info.power_draw_w = watts;
                found_power_field = true;
            }
            _ => {}
        }
    }
    if !found_power_field {
        return Err("NVIDIA detailed query contained no recognizable power fields".into());
    }
    Ok(info)
}

fn query_nvidia_power_info(executor: &impl CommandExecutor) -> Result<NvidiaPowerInfo, String> {
    let csv_result = executor.execute(&build_query_power_args());
    if let Ok(output) = &csv_result {
        if output.success {
            if let Ok(info) = parse_csv_power_query(&output.stdout) {
                return Ok(info);
            }
        }
    }

    let details = executor.execute(&build_detailed_power_args())?;
    if !details.success {
        let csv_error = match csv_result {
            Ok(output) => {
                let detail = if output.stderr.trim().is_empty() {
                    output.stdout.trim()
                } else {
                    output.stderr.trim()
                };
                format!("CSV query failed: {detail}")
            }
            Err(error) => error,
        };
        return Err(format!(
            "Could not query NVIDIA power information ({csv_error}; detailed query: {})",
            if details.stderr.trim().is_empty() {
                details.stdout.trim()
            } else {
                details.stderr.trim()
            }
        ));
    }
    parse_detailed_power_query(&details.stdout)
}

fn power_info_summary(info: &NvidiaPowerInfo) -> String {
    fn show(value: Option<f64>) -> String {
        value
            .map(|watts| format!("{watts:.2} W"))
            .unwrap_or_else(|| "N/A".into())
    }
    format!(
        "base limit {}, enforced ceiling {}, software-requested limit {}, NVIDIA-reported maximum limit {}, live draw {}",
        show(info.default_limit_w),
        show(info.enforced_limit_w),
        show(info.software_requested_limit_w),
        show(info.max_limit_w),
        show(info.power_draw_w),
    )
}

/// Driver rejection phrases reported by `nvidia-smi --power-limit`.
///
/// The mobile driver on the A715-79G prints
/// "Changing power management limit is not supported in current scope..."
/// to stdout yet still exits with status 0, so the exit code alone must
/// never be treated as proof that a cap was accepted. Any of these
/// phrases in stdout or stderr means the write was rejected, even when
/// the process exits successfully. The readback check that follows is a
/// second line of defence: a missed phrase still fails verification.
const REJECTION_PHRASES: &[&str] = &[
    "not supported in current scope",
    "power management limit is not supported",
    "setting the power limit is not supported",
    "not supported",
    "failed to set",
    "failed to change",
    "insufficient permissions",
    "permission denied",
    "invalid power limit",
    "unknown gpu",
    "not available",
];

fn check_set_limit_output(output: &CommandOutput) -> Result<(), String> {
    let combined = format!("{}\n{}", output.stdout, output.stderr);
    let lower = combined.to_ascii_lowercase();
    if REJECTION_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        return Err(format!(
            "NVIDIA driver rejected the requested power limit: {}",
            combined.trim()
        ));
    }
    if !output.success {
        let detail = if output.stderr.trim().is_empty() {
            output.stdout.trim()
        } else {
            output.stderr.trim()
        };
        return Err(format!("NVIDIA GPU rejected the power limit: {detail}"));
    }
    Ok(())
}

fn validate_nvidia_watts(watts: u32, info: &NvidiaPowerInfo) -> Result<(), String> {
    if !(1..=500).contains(&watts) {
        return Err("GPU cap must be between 1 and 500 W".into());
    }
    if let Some(min) = info.min_limit_w {
        if f64::from(watts) < min {
            return Err(format!("GPU cap is below the NVIDIA-supported minimum ({min:.2} W)"));
        }
    }
    if let Some(max) = info.max_limit_w {
        if f64::from(watts) > max {
            return Err(format!("GPU cap exceeds the NVIDIA-supported maximum ({max:.2} W)"));
        }
    }
    Ok(())
}

fn verify_power_limit_readback(watts: u32, info: &NvidiaPowerInfo) -> Result<(), String> {
    let Some(requested) = info.software_requested_limit_w else {
        return Err(format!(
            "NVIDIA did not expose a numeric software-requested power limit; an enforced ceiling or live draw is not proof that the cap was set ({})",
            power_info_summary(info)
        ));
    };
    if (requested - f64::from(watts)).abs() > 0.5 {
        return Err(format!(
            "NVIDIA software-requested limit read back as {requested:.2} W, not {watts} W ({})",
            power_info_summary(info)
        ));
    }
    Ok(())
}

fn set_gpu_power_cap_with_executor(
    watts: u32,
    executor: &impl CommandExecutor,
) -> Result<String, String> {
    if !(1..=500).contains(&watts) {
        return Err("GPU cap must be between 1 and 500 W".into());
    }
    let before = query_nvidia_power_info(executor)?;
    validate_nvidia_watts(watts, &before)?;

    // Try the requested wattage verbatim first: when the driver accepts it,
    // the enforced ceiling is not a hard cap and the user's target must not
    // be silently lowered.
    let output = executor.execute(&build_set_power_limit_args(watts))?;
    let mut target = watts;
    let mut clamped = false;
    if let Err(error) = check_set_limit_output(&output) {
        // Mobile drivers refuse anything above the enforced ceiling with a
        // "not supported in current scope" rejection even though the request
        // is inside the min/max range. Retry once at that ceiling when the
        // request exceeds it; otherwise surface the original rejection.
        let ceiling = before
            .enforced_limit_w
            .map(|value| value as u32)
            .filter(|value| (1..watts).contains(value));
        let Some(ceiling) = ceiling else {
            return Err(format!("{error} Current NVIDIA state: {}.", power_info_summary(&before)));
        };
        let retry = executor.execute(&build_set_power_limit_args(ceiling))?;
        if let Err(retry_error) = check_set_limit_output(&retry) {
            return Err(format!(
                "{retry_error} Current NVIDIA state: {}.",
                power_info_summary(&before)
            ));
        }
        target = ceiling;
        clamped = true;
    }

    let after = query_nvidia_power_info(executor)?;
    // Verify what was actually written (the clamped target after a retry),
    // never the original request.
    verify_power_limit_readback(target, &after).map_err(|error| {
        format!("{error}. Current NVIDIA state: {}.", power_info_summary(&after))
    })?;
    let detail = power_info_summary(&after);
    Ok(if clamped {
        format!("requested {watts} W, applied {target} W (driver-enforced ceiling); {detail}")
    } else {
        detail
    })
}

pub fn set_gpu_power_cap(watts: u32) -> Result<String, String> {
    set_gpu_power_cap_with_executor(watts, &SystemCommandExecutor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct MockExecutor {
        outputs: Mutex<VecDeque<Result<CommandOutput, String>>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl MockExecutor {
        fn new(outputs: impl IntoIterator<Item = CommandOutput>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into_iter().map(Ok).collect()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CommandExecutor for MockExecutor {
        fn execute(&self, args: &[String]) -> Result<CommandOutput, String> {
            self.calls.lock().unwrap().push(args.to_vec());
            self.outputs.lock().unwrap().pop_front().unwrap_or_else(|| {
                Err("mock executor has no response left".into())
            })
        }
    }

    fn result(success: bool, stdout: &str, stderr: &str) -> CommandOutput {
        CommandOutput {
            success,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn mobile_info(requested: &str, draw: &str) -> String {
        format!("1.00, 75.00, 60.00, 70.00, {requested}, {draw}\n")
    }

    fn query_result(requested: &str, draw: &str) -> CommandOutput {
        result(true, &mobile_info(requested, draw), "")
    }

    #[test]
    fn test_build_query_power_args() {
        assert_eq!(build_query_power_args(), vec![
            "--query-gpu=power.min_limit,power.max_limit,power.default_limit,enforced.power.limit,power.limit,power.draw".to_owned(),
            "--format=csv,noheader,nounits".to_owned(), "-i".to_owned(), "0".to_owned()
        ]);
    }

    #[test]
    fn test_build_detailed_power_args() {
        assert_eq!(
            build_detailed_power_args(),
            vec!["-q".to_owned(), "-d".to_owned(), "POWER".to_owned(), "-i".to_owned(), "0".to_owned()]
        );
    }

    #[test]
    fn test_build_set_power_limit_args() {
        assert_eq!(
            build_set_power_limit_args(75),
            vec!["-i".to_owned(), "0".to_owned(), "--power-limit".to_owned(), "75".to_owned()]
        );
    }

    #[test]
    fn test_parse_csv_power_query_mobile() {
        let info = parse_csv_power_query(&mobile_info("[N/A]", "6.51")).unwrap();
        assert_eq!(info.min_limit_w, Some(1.0));
        assert_eq!(info.max_limit_w, Some(75.0));
        assert_eq!(info.default_limit_w, Some(60.0));
        assert_eq!(info.enforced_limit_w, Some(70.0));
        assert_eq!(info.software_requested_limit_w, None);
        assert_eq!(info.power_draw_w, Some(6.51));
    }

    #[test]
    fn test_parse_csv_power_query_desktop() {
        let info = parse_csv_power_query("50.00, 250.00, 200.00, 200.00, 180.00, 45.20\n").unwrap();
        assert_eq!(info.software_requested_limit_w, Some(180.0));
        assert_eq!(info.power_draw_w, Some(45.2));
    }

    #[test]
    fn test_parse_csv_power_query_malformed() {
        assert!(parse_csv_power_query("1, 75, 60\n").is_err());
        assert!(parse_csv_power_query("\n").is_err());
    }

    #[test]
    fn test_parse_detailed_power_query() {
        let text = "Power Readings\n    Power Draw : 6.51 W\n    Current Power Limit : 70.00 W\n    Requested Power Limit : [N/A]\n    Default Power Limit : 60.00 W\n    Min Power Limit : 1.00 W\n    Max Power Limit : 75.00 W\n    Enforced Power Limit : 70.00 W\n";
        let info = parse_detailed_power_query(text).unwrap();
        assert_eq!(info.power_draw_w, Some(6.51));
        assert_eq!(info.enforced_limit_w, Some(70.0));
        assert_eq!(info.software_requested_limit_w, None);
        assert_eq!(info.default_limit_w, Some(60.0));
        assert_eq!(info.max_limit_w, Some(75.0));
    }

    #[test]
    fn test_power_query_falls_back_to_detailed_format() {
        let detailed = "Power Readings\n  Default Power Limit : 60.00 W\n  Enforced Power Limit : 70.00 W\n  Requested Power Limit : [N/A]\n  Max Power Limit : 75.00 W\n  Power Draw : 6.51 W\n";
        let mock = MockExecutor::new([
            result(false, "", "Unknown field: enforced.power.limit"),
            result(true, detailed, ""),
        ]);
        let info = query_nvidia_power_info(&mock).unwrap();
        assert_eq!(info.enforced_limit_w, Some(70.0));
        assert_eq!(info.software_requested_limit_w, None);
        assert_eq!(mock.calls().len(), 2);
    }

    #[test]
    fn test_check_set_limit_output_mobile_rejected_exit_zero() {
        let output = result(true, "Changing power management limit is not supported in current scope for GPU: 00000000:01:00.0.", "");
        assert!(check_set_limit_output(&output).unwrap_err().contains("rejected"));
    }

    #[test]
    fn test_check_set_limit_output_success() {
        assert!(check_set_limit_output(&result(true, "", "")).is_ok());
    }

    #[test]
    fn test_check_set_limit_output_nonzero_exit() {
        assert!(check_set_limit_output(&result(false, "", "Insufficient Permissions")).is_err());
    }

    #[test]
    fn test_boundary_watts_input() {
        let info = NvidiaPowerInfo { min_limit_w: Some(1.0), max_limit_w: Some(500.0), ..Default::default() };
        assert!(validate_nvidia_watts(1, &info).is_ok());
        assert!(validate_nvidia_watts(500, &info).is_ok());
        assert!(validate_nvidia_watts(0, &info).is_err());
        assert!(validate_nvidia_watts(501, &info).is_err());
    }

    #[test]
    fn test_boundary_outside_nvidia_range() {
        let info = NvidiaPowerInfo { min_limit_w: Some(10.0), max_limit_w: Some(75.0), ..Default::default() };
        assert!(validate_nvidia_watts(9, &info).is_err());
        assert!(validate_nvidia_watts(76, &info).is_err());
    }

    #[test]
    fn test_mobile_rejected_does_not_claim_success() {
        // Both the requested target and the retry at the enforced ceiling are
        // refused: the daemon must report the rejection, never success.
        let mock = MockExecutor::new([
            query_result("[N/A]", "6.51"),
            result(true, "Changing power management limit is not supported in current scope for GPU.", ""),
            result(true, "Changing power management limit is not supported in current scope for GPU.", ""),
        ]);
        let error = set_gpu_power_cap_with_executor(75, &mock).unwrap_err();
        assert!(error.contains("driver rejected"));
        assert!(error.contains("enforced ceiling 70.00 W"));
        assert_eq!(mock.calls().len(), 3);
    }

    #[test]
    fn test_readback_mismatch_fails() {
        let mock = MockExecutor::new([
            query_result("[N/A]", "6.51"),
            result(true, "", ""),
            query_result("70.00", "6.51"),
        ]);
        let error = set_gpu_power_cap_with_executor(75, &mock).unwrap_err();
        assert!(error.contains("read back as 70.00 W"));
    }

    #[test]
    fn test_successful_limit_application() {
        let mock = MockExecutor::new([
            query_result("[N/A]", "6.51"),
            result(true, "", ""),
            query_result("75.00", "6.51"),
        ]);
        let detail = set_gpu_power_cap_with_executor(75, &mock).unwrap();
        assert!(detail.contains("software-requested limit 75.00 W"));
        assert!(detail.contains("enforced ceiling 70.00 W"));
        assert!(detail.contains("live draw 6.51 W"));
        assert_eq!(mock.calls().len(), 3);
    }

    #[test]
    fn test_power_draw_conflation_prevention() {
        let info = parse_csv_power_query(&mobile_info("[N/A]", "75.00")).unwrap();
        assert!(verify_power_limit_readback(75, &info).is_err());
    }

    #[test]
    fn test_check_set_limit_output_stderr_rejection_exit_zero() {
        let output = result(true, "", "Changing power management limit is not supported in current scope for GPU: 00000000:01:00.0.");
        let error = check_set_limit_output(&output).unwrap_err();
        assert!(error.contains("driver rejected"));
    }

    #[test]
    fn test_check_set_limit_output_generic_failure_phrases() {
        for text in [
            "Failed to set power limit for GPU 0.",
            "Failed to change power management limit.",
            "Insufficient Permissions to set power limit.",
            "Power limit is not supported on this GPU.",
        ] {
            let output = result(true, text, "");
            assert!(
                check_set_limit_output(&output).is_err(),
                "expected rejection for: {text}"
            );
        }
    }

    #[test]
    fn test_check_set_limit_output_empty_success() {
        assert!(check_set_limit_output(&result(true, "", "")).is_ok());
        assert!(check_set_limit_output(&result(true, "\n", "\n")).is_ok());
    }

    #[test]
    fn test_parse_optional_watts_variants() {
        assert_eq!(parse_optional_watts("70.00"), Some(70.0));
        assert_eq!(parse_optional_watts("70.00 W"), Some(70.0));
        assert_eq!(parse_optional_watts("[70.00]"), Some(70.0));
        assert_eq!(parse_optional_watts("N/A"), None);
        assert_eq!(parse_optional_watts("[N/A]"), None);
        assert_eq!(parse_optional_watts("Not Supported"), None);
        assert_eq!(parse_optional_watts("Not Available"), None);
        assert_eq!(parse_optional_watts(""), None);
        assert_eq!(parse_optional_watts("   "), None);
        assert_eq!(parse_optional_watts("garbage"), None);
    }

    #[test]
    fn test_power_info_summary_keeps_fields_separate() {
        let info = parse_csv_power_query(&mobile_info("[N/A]", "6.51")).unwrap();
        let summary = power_info_summary(&info);
        assert!(summary.contains("base limit 60.00 W"));
        assert!(summary.contains("enforced ceiling 70.00 W"));
        assert!(summary.contains("software-requested limit N/A"));
        assert!(summary.contains("NVIDIA-reported maximum limit 75.00 W"));
        assert!(summary.contains("live draw 6.51 W"));
        // The reported maximum must never be labelled a Dynamic Boost ceiling.
        assert!(!summary.to_ascii_lowercase().contains("dynamic boost"));
    }

    #[test]
    fn test_parse_detailed_software_requested_label() {
        let text = "Power Readings\n    Power Draw : 45.20 W\n    Enforced Power Limit : 200.00 W\n    Software Requested Power Limit : 180.00 W\n    Default Power Limit : 200.00 W\n";
        let info = parse_detailed_power_query(text).unwrap();
        assert_eq!(info.software_requested_limit_w, Some(180.0));
        assert_eq!(info.enforced_limit_w, Some(200.0));
    }

    #[test]
    fn test_parse_detailed_current_limit_does_not_override_enforced() {
        let text = "Power Readings\n    Current Power Limit : 70.00 W\n    Enforced Power Limit : 75.00 W\n    Power Draw : 6.51 W\n";
        let info = parse_detailed_power_query(text).unwrap();
        assert_eq!(info.enforced_limit_w, Some(75.0));
    }

    #[test]
    fn test_enforced_ceiling_is_not_proof_of_cap() {
        // Enforced ceiling happens to equal the request, but no numeric
        // software-requested limit was exposed: must still fail.
        let info = NvidiaPowerInfo {
            enforced_limit_w: Some(75.0),
            software_requested_limit_w: None,
            ..Default::default()
        };
        assert!(verify_power_limit_readback(75, &info).is_err());
    }

    #[test]
    fn test_max_limit_is_not_proof_of_cap() {
        let info = NvidiaPowerInfo {
            max_limit_w: Some(75.0),
            software_requested_limit_w: None,
            ..Default::default()
        };
        assert!(verify_power_limit_readback(75, &info).is_err());
    }

    #[test]
    fn test_readback_tolerance() {
        let info = NvidiaPowerInfo {
            software_requested_limit_w: Some(75.4),
            ..Default::default()
        };
        assert!(verify_power_limit_readback(75, &info).is_ok());
        let info = NvidiaPowerInfo {
            software_requested_limit_w: Some(75.6),
            ..Default::default()
        };
        assert!(verify_power_limit_readback(75, &info).is_err());
    }

    #[test]
    fn test_set_gpu_rejects_out_of_range_before_querying() {
        let mock = MockExecutor::new([]);
        assert!(set_gpu_power_cap_with_executor(0, &mock).is_err());
        assert!(set_gpu_power_cap_with_executor(501, &mock).is_err());
        assert_eq!(mock.calls().len(), 0);
    }

    #[test]
    fn test_set_gpu_rejects_above_nvidia_max_without_writing() {
        let mock = MockExecutor::new([query_result("[N/A]", "6.51")]);
        let error = set_gpu_power_cap_with_executor(100, &mock).unwrap_err();
        assert!(error.contains("exceeds the NVIDIA-supported maximum"));
        // Only the pre-query ran; no --power-limit write was attempted.
        assert_eq!(mock.calls().len(), 1);
    }

    #[test]
    fn test_executed_command_sequence_for_a_successful_apply() {
        // End-to-end argv check: pre-query, then exactly one --power-limit
        // write with the requested wattage, then the readback query.
        let mock = MockExecutor::new([
            query_result("[N/A]", "6.51"),
            result(true, "", ""),
            query_result("45.00", "30.10"),
        ]);
        let detail = set_gpu_power_cap_with_executor(45, &mock).unwrap();
        assert!(detail.contains("software-requested limit 45.00 W"));
        let calls = mock.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0], build_query_power_args());
        assert_eq!(calls[1], build_set_power_limit_args(45));
        assert_eq!(calls[2], build_query_power_args());
        // The write must carry the requested value, never the enforced
        // ceiling or the reported maximum.
        assert_eq!(calls[1].last().map(String::as_str), Some("45"));
    }

    #[test]
    fn test_rejected_apply_still_reports_the_requested_target() {
        // The 75 W Performance target stays in the error text so the user can
        // see what was asked for and why the driver refused it.
        let mock = MockExecutor::new([
            query_result("[N/A]", "6.51"),
            result(
                true,
                "Changing power management limit is not supported in current scope for GPU: 00000000:01:00.0.",
                "",
            ),
            result(
                true,
                "Changing power management limit is not supported in current scope for GPU: 00000000:01:00.0.",
                "",
            ),
        ]);
        let error = set_gpu_power_cap_with_executor(75, &mock).unwrap_err();
        assert!(error.contains("not supported in current scope"));
        assert!(error.contains("NVIDIA-reported maximum limit 75.00 W"));
        assert!(error.contains("enforced ceiling 70.00 W"));
        assert!(error.contains("base limit 60.00 W"));
        assert!(error.contains("live draw 6.51 W"));
        // One write at the requested target plus one retry at the enforced
        // ceiling; no readback query is performed after a rejected write.
        assert_eq!(mock.calls().len(), 3);
        assert_eq!(mock.calls()[2], build_set_power_limit_args(70));
    }

    #[test]
    fn test_scope_rejection_retries_at_enforced_ceiling() {
        // The mobile driver rejects 75 W (above its 70 W enforced ceiling) but
        // accepts the retry at the ceiling; the success message must say so.
        let mock = MockExecutor::new([
            query_result("[N/A]", "6.51"),
            result(
                true,
                "Changing power management limit is not supported in current scope for GPU: 00000000:01:00.0.",
                "",
            ),
            result(true, "All done.", ""),
            query_result("70.00", "6.51"),
        ]);
        let detail = set_gpu_power_cap_with_executor(75, &mock).unwrap();
        assert!(detail.contains("requested 75 W, applied 70 W"));
        let calls = mock.calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[1], build_set_power_limit_args(75));
        assert_eq!(calls[2], build_set_power_limit_args(70));
    }

    #[test]
    fn test_scope_rejection_without_ceiling_fails() {
        // Without an enforced ceiling there is nothing to clamp to, so the
        // original rejection is reported after a single write attempt.
        let mock = MockExecutor::new([
            result(true, "50.00, 250.00, 200.00, N/A, N/A, 45.20\n", ""),
            result(true, "Power limit is not supported on this GPU.", ""),
        ]);
        let error = set_gpu_power_cap_with_executor(220, &mock).unwrap_err();
        assert!(error.contains("not supported"));
        assert_eq!(mock.calls().len(), 2);
    }
}
