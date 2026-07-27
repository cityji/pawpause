use std::process::Command;

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Lists connected Wayland output names via `cosmic-randr list`, e.g. ["eDP-1", "HDMI-A-1"].
/// Falls back to an empty list if the command is unavailable or unparsable.
pub fn list_outputs() -> Vec<String> {
    let Ok(output) = Command::new("cosmic-randr").arg("list").output() else {
        return Vec::new();
    };
    let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));

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
