use std::path::PathBuf;
use std::process::Command;

use crate::overlay::notify;

const BG_CONFIG_DIR: &str = ".config/cosmic/com.system76.CosmicBackground/v1";
const SOURCE_MARKER: &str = "source: Path(\"";

/// What was changed, so it can be restored exactly once the break ends.
pub struct WallpaperBackup {
    config_file: PathBuf,
    original_source: String,
}

fn config_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(BG_CONFIG_DIR)
}

/// `cosmic-bg` keeps one config file per output, or a single `all` file when
/// "same wallpaper on every output" is enabled — that flag lives in its own
/// `same-on-all` file alongside them.
fn target_config_file(output_name: &str) -> PathBuf {
    let dir = config_dir();
    let same_on_all = std::fs::read_to_string(dir.join("same-on-all"))
        .map(|s| s.trim() == "true")
        .unwrap_or(true);

    if same_on_all {
        return dir.join("all");
    }
    let per_output = dir.join(output_name);
    if per_output.is_file() {
        per_output
    } else {
        dir.join("all")
    }
}

/// Extracts the current `source: Path("...")` value and returns it alongside
/// the byte ranges needed to splice in a replacement.
fn extract_source(content: &str) -> Option<(usize, usize, String)> {
    let start = content.find(SOURCE_MARKER)? + SOURCE_MARKER.len();
    let end = start + content[start..].find('"')?;
    Some((start, end, content[start..end].to_string()))
}

fn replace_source(content: &str, new_path: &str) -> Option<String> {
    let (start, end, _) = extract_source(content)?;
    Some(format!("{}{}{}", &content[..start], new_path, &content[end..]))
}

fn cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pawpause")
        .join("blurred-wallpaper.jpg")
}

/// Blurs the current wallpaper and points `cosmic-bg` at the blurred copy.
/// `blur` is 0-100 (same scale as the old video-blur setting); 0 is a no-op.
/// `cosmic-bg` watches its config directory and hot-reloads on change, so no
/// extra signal is needed after rewriting the file.
pub fn apply(output_name: &str, blur: u32) -> Option<WallpaperBackup> {
    if blur == 0 {
        return None;
    }

    let config_file = target_config_file(output_name);
    let content = std::fs::read_to_string(&config_file).ok()?;
    let (_, _, original_source) = extract_source(&content)?;

    let out_path = cache_path();
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let sigma = blur as f64 / 10.0;
    let status = Command::new("ffmpeg")
        .args(["-y", "-i", &original_source, "-vf"])
        .arg(format!("gblur=sigma={sigma:.1}"))
        .arg(&out_path)
        .output();

    match status {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            notify(
                "PawPause",
                &format!("Could not blur wallpaper: {}", String::from_utf8_lossy(&output.stderr)),
            );
            return None;
        }
        Err(err) => {
            notify("PawPause", &format!("Could not run ffmpeg to blur wallpaper: {err}"));
            return None;
        }
    }

    let new_content = replace_source(&content, &out_path.to_string_lossy())?;
    if let Err(err) = std::fs::write(&config_file, new_content) {
        notify("PawPause", &format!("Could not update wallpaper config: {err}"));
        return None;
    }

    Some(WallpaperBackup {
        config_file,
        original_source,
    })
}

/// Restores the wallpaper that was active before `apply`.
pub fn restore(backup: WallpaperBackup) {
    let Ok(content) = std::fs::read_to_string(&backup.config_file) else {
        return;
    };
    if let Some(new_content) = replace_source(&content, &backup.original_source) {
        if let Err(err) = std::fs::write(&backup.config_file, new_content) {
            notify("PawPause", &format!("Could not restore wallpaper config: {err}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RON: &str = "(\n    output: \"all\",\n    source: Path(\"/usr/share/backgrounds/cosmic/earth.jpg\"),\n    filter_by_theme: true,\n    rotation_frequency: 300,\n)";

    #[test]
    fn extracts_the_source_path() {
        let (_, _, path) = extract_source(SAMPLE_RON).unwrap();
        assert_eq!(path, "/usr/share/backgrounds/cosmic/earth.jpg");
    }

    #[test]
    fn replace_source_only_touches_the_source_line() {
        let replaced = replace_source(SAMPLE_RON, "/tmp/blurred.jpg").unwrap();
        assert!(replaced.contains("source: Path(\"/tmp/blurred.jpg\")"));
        assert!(replaced.contains("output: \"all\""));
        assert!(replaced.contains("rotation_frequency: 300"));
        // Round-trips back to the original when replaced again.
        let restored = replace_source(&replaced, "/usr/share/backgrounds/cosmic/earth.jpg").unwrap();
        assert_eq!(restored, SAMPLE_RON);
    }

    #[test]
    fn missing_source_marker_returns_none() {
        assert!(extract_source("(\n    output: \"all\",\n)").is_none());
    }
}
