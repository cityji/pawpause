use std::process::Command;

use crate::ansi::strip_ansi_codes;

/// Lists connected Wayland output names via `cosmic-randr list`, e.g. ["eDP-1", "HDMI-A-1"].
/// Falls back to an empty list if the command is unavailable or unparsable.
pub fn list_outputs() -> Vec<String> {
    let Ok(output) = Command::new("cosmic-randr").arg("list").output() else {
        return Vec::new();
    };
    let text = strip_ansi_codes(&String::from_utf8_lossy(&output.stdout));

    text.lines()
        .filter_map(|line| {
            let trimmed_start = line.trim_start();
            if trimmed_start != line || trimmed_start.is_empty() {
                // Indented lines are properties of the current output block, not names.
                return None;
            }
            let name = trimmed_start.split_whitespace().next()?;
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

fn parse_current_resolution(text: &str, name: &str) -> Option<(u32, u32)> {
    let mut in_target_block = false;
    for line in text.lines() {
        let trimmed_start = line.trim_start();
        let is_block_header = trimmed_start == line && !trimmed_start.is_empty();
        if is_block_header {
            in_target_block = trimmed_start.split_whitespace().next() == Some(name);
            continue;
        }
        if !in_target_block || !trimmed_start.contains("(current)") {
            continue;
        }
        let dims = trimmed_start.split_whitespace().next()?;
        let (w, h) = dims.split_once('x')?;
        return Some((w.parse().ok()?, h.parse().ok()?));
    }
    None
}

/// Resolves the current mode (width, height) of the named output, e.g.
/// `(1920, 1200)`, by parsing the `Modes:` block of `cosmic-randr list` for
/// the line marked `(current)`. `None` if the tool or output isn't available.
pub fn output_resolution(name: &str) -> Option<(u32, u32)> {
    let output = Command::new("cosmic-randr").arg("list").output().ok()?;
    let text = strip_ansi_codes(&String::from_utf8_lossy(&output.stdout));
    parse_current_resolution(&text, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "eDP-1 (enabled)\n  Make: BOE\n  Model: 0x0B8F\n  Position: 0,0\n\n  Modes:\n    1920x1200 @  60.001 Hz (current) (preferred)\n    1920x1200 @  48.001 Hz\nHDMI-A-1 (enabled)\n  Modes:\n    1920x1080 @  60.000 Hz (current) (preferred)\n";

    #[test]
    fn parses_the_mode_marked_current_for_the_named_output() {
        assert_eq!(parse_current_resolution(SAMPLE, "eDP-1"), Some((1920, 1200)));
        assert_eq!(parse_current_resolution(SAMPLE, "HDMI-A-1"), Some((1920, 1080)));
        assert_eq!(parse_current_resolution(SAMPLE, "DP-3"), None);
    }
}
