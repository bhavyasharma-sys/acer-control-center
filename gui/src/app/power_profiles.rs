use std::path::PathBuf;

pub const DEFAULTS: [[u32; 2]; 3] = [[40, 75], [30, 60], [15, 30]];

fn state_path() -> Result<PathBuf, String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "Could not determine the user configuration directory".to_string())?;
    Ok(config.join("acer-control-center").join("power-profiles"))
}

fn parse_profile_contents(contents: &str) -> Option<([[u32; 2]; 3], usize)> {
    let values: Vec<u32> = contents
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if values.len() != 7
        || values[..6].iter().any(|value| *value == 0 || *value > 500)
        || values[6] > 2
    {
        return None;
    }
    Some((
        [
            [values[0], values[1]],
            [values[2], values[3]],
            [values[4], values[5]],
        ],
        values[6] as usize,
    ))
}

pub fn load() -> ([[u32; 2]; 3], usize) {
    let Ok(path) = state_path() else { return (DEFAULTS, 0) };
    let Ok(contents) = std::fs::read_to_string(path) else { return (DEFAULTS, 0) };
    parse_profile_contents(&contents).unwrap_or((DEFAULTS, 0))
}

pub fn save(caps: [[u32; 2]; 3], selected: usize) -> Result<(), String> {
    let path = state_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| "Invalid power-profile configuration path".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let contents = format!(
        "{} {} {} {} {} {} {}\n",
        caps[0][0], caps[0][1], caps[1][0], caps[1][1], caps[2][0], caps[2][1], selected.min(2)
    );
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, contents).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_defaults() {
        assert_eq!(DEFAULTS, [[40, 75], [30, 60], [15, 30]]);
    }

    #[test]
    fn test_parse_profile_contents() {
        assert_eq!(
            parse_profile_contents("40 75 30 60 15 30 2"),
            Some(([[40, 75], [30, 60], [15, 30]], 2))
        );
    }

    #[test]
    fn test_invalid_profile_contents_falls_back() {
        assert!(parse_profile_contents("40 75 30 60 15 30 4").is_none());
        assert!(parse_profile_contents("40 75 30 60 15 0 0").is_none());
        assert!(parse_profile_contents("not a profile").is_none());
    }

    #[test]
    fn test_parse_accepts_mixed_whitespace_and_trailing_newline() {
        assert_eq!(
            parse_profile_contents("40 75\t30 60 15 30\n1\n"),
            Some(([[40, 75], [30, 60], [15, 30]], 1))
        );
        assert_eq!(
            parse_profile_contents("\n\n  40   75   30   60   15   30   0  \n"),
            Some(([[40, 75], [30, 60], [15, 30]], 0))
        );
    }

    #[test]
    fn test_parse_rejects_wrong_value_count() {
        assert!(parse_profile_contents("").is_none());
        assert!(parse_profile_contents("40 75 30 60 15 30").is_none());
        assert!(parse_profile_contents("40 75 30 60 15 30 0 0").is_none());
    }

    #[test]
    fn test_parse_accepts_wattage_bounds() {
        assert_eq!(
            parse_profile_contents("1 1 500 500 1 500 2"),
            Some(([[1, 1], [500, 500], [1, 500]], 2))
        );
        // Anything above the documented cap is rejected, not clamped.
        assert!(parse_profile_contents("501 75 30 60 15 30 0").is_none());
        assert!(parse_profile_contents("40 75 30 60 15 501 0").is_none());
        // Zero watts is rejected: a missing value must not become a real cap.
        assert!(parse_profile_contents("0 75 30 60 15 30 0").is_none());
    }

    #[test]
    fn test_parse_rejects_negative_and_non_numeric() {
        assert!(parse_profile_contents("-40 75 30 60 15 30 0").is_none());
        assert!(parse_profile_contents("40 75 30 60 15 30 -1").is_none());
        assert!(parse_profile_contents("40.0 75 30 60 15 30 0").is_none());
    }

    #[test]
    fn test_defaults_match_documented_profiles() {
        assert_eq!(DEFAULTS[0], [40, 75], "Performance");
        assert_eq!(DEFAULTS[1], [30, 60], "Balanced");
        assert_eq!(DEFAULTS[2], [15, 30], "Battery Saver");
    }

    #[test]
    fn test_defaults_round_trip_through_the_saved_format() {
        // The exact string `save` writes must parse back to the same state.
        let contents = format!(
            "{} {} {} {} {} {} {}\n",
            DEFAULTS[0][0], DEFAULTS[0][1], DEFAULTS[1][0], DEFAULTS[1][1],
            DEFAULTS[2][0], DEFAULTS[2][1], 2
        );
        assert_eq!(parse_profile_contents(&contents), Some((DEFAULTS, 2)));
    }
}
