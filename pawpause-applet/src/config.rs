use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::overlay::notify;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_work_minutes")]
    pub work_minutes: f64,
    #[serde(default = "default_short_break_minutes")]
    pub short_break_minutes: f64,
    #[serde(default = "default_long_break_minutes")]
    pub long_break_minutes: f64,
    #[serde(default = "default_sessions_before_long_break")]
    pub sessions_before_long_break: u32,
    #[serde(default = "default_video_path")]
    pub video_path: String,
    /// Optional looping "sleep" clip played after `video_path` finishes once
    /// (mirrors cat-gatekeeper's walk-in-then-sleep behavior). Empty or equal
    /// to `video_path` means just loop `video_path` on its own.
    #[serde(default)]
    pub video_sleep_path: String,
    #[serde(default = "default_wayland_output")]
    pub wayland_output: String,
    /// 0-100; blurs the desktop wallpaper (not the cat) during breaks, via
    /// cosmic-bg. 0 disables the effect entirely.
    #[serde(default)]
    pub blur: u32,
    /// Daily focused-minutes target shown on the Statistics page. 0 disables
    /// the goal indicator entirely (same "0 disables" convention as `blur`).
    #[serde(default)]
    pub daily_goal_minutes: u32,
}

fn default_work_minutes() -> f64 {
    25.0
}
fn default_short_break_minutes() -> f64 {
    5.0
}
fn default_long_break_minutes() -> f64 {
    20.0
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
            video_sleep_path: String::new(),
            wayland_output: default_wayland_output(),
            blur: 0,
            daily_goal_minutes: 0,
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
/// A config that fails to parse falls back to defaults but notifies the
/// user, rather than silently discarding their settings.
/// Returns (config, created).
pub fn load_or_create() -> (Config, bool) {
    let path = config_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let cfg = Config::default();
        save(&cfg);
        return (cfg, true);
    }

    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(cfg) => (cfg, false),
            Err(err) => {
                notify(
                    "PawPause",
                    &format!("Config at {} is invalid ({err}) — using defaults.", path.display()),
                );
                (Config::default(), false)
            }
        },
        Err(err) => {
            notify("PawPause", &format!("Could not read config: {err} — using defaults."));
            (Config::default(), false)
        }
    }
}

pub fn save(config: &Config) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if let Err(err) = std::fs::write(&path, format!("{json}\n")) {
                notify("PawPause", &format!("Could not save config: {err}"));
            }
        }
        Err(err) => notify("PawPause", &format!("Could not serialize config: {err}")),
    }
}
