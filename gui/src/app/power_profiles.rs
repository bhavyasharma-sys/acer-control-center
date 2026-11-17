use std::path::PathBuf;

pub const DEFAULTS: [[u32; 2]; 3] = [[40, 75], [30, 60], [15, 30]];

fn state_path() -> Result<PathBuf, String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "Could not determine the user configuration directory".to_string())?;
    Ok(config.join("acer-control-center").join("power-profiles"))
}

pub fn load() -> ([[u32; 2]; 3], usize) {
    let Ok(path) = state_path() else { return (DEFAULTS, 0) };
    let Ok(contents) = std::fs::read_to_string(path) else { return (DEFAULTS, 0) };
    let values: Result<Vec<u32>, _> = contents.split_whitespace().map(str::parse).collect();
    let Ok(values) = values else { return (DEFAULTS, 0) };
    if values.len() != 7
        || values[..6].iter().any(|value| *value == 0 || *value > 500)
        || values[6] > 2
    {
        return (DEFAULTS, 0);
    }
    (
        [
            [values[0], values[1]],
            [values[2], values[3]],
            [values[4], values[5]],
        ],
        values[6] as usize,
    )
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
