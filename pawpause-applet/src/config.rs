use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_work_minutes")]
    pub work_minutes: u64,
    #[serde(default = "default_short_break_minutes")]
    pub short_break_minutes: u64,
    #[serde(default = "default_long_break_minutes")]
    pub long_break_minutes: u64,
    #[serde(default = "default_sessions_before_long_break")]
    pub sessions_before_long_break: u32,
    #[serde(default = "default_video_path")]
    pub video_path: String,
    #[serde(default = "default_wayland_output")]
    pub wayland_output: String,
}

fn default_work_minutes() -> u64 {
    25
}
fn default_short_break_minutes() -> u64 {
    5
}
fn default_long_break_minutes() -> u64 {
    20
}
fn default_sessions_before_long_break() -> u32 {
    4
}
fn default_video_path() -> String {
    dirs::home_dir()
        .unwrap_or_default()
        .join("Videos")
        .join("pawpause-break.mp4")
        .to_string_lossy()
        .into_owned()
}
fn default_wayland_output() -> String {
    "eDP-1".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            work_minutes: default_work_minutes(),
            short_break_minutes: default_short_break_minutes(),
            long_break_minutes: default_long_break_minutes(),
            sessions_before_long_break: default_sessions_before_long_break(),
            video_path: default_video_path(),
            wayland_output: default_wayland_output(),
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("pawpause")
        .join("config")
}

/// Loads ~/.config/pawpause/config, creating it with defaults if missing.
/// Returns (config, created).
pub fn load_or_create() -> (Config, bool) {
    let path = config_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let cfg = Config::default();
        if let Ok(json) = serde_json::to_string_pretty(&cfg) {
            let _ = std::fs::write(&path, format!("{json}\n"));
        }
        return (cfg, true);
    }

    let cfg = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    (cfg, false)
}
