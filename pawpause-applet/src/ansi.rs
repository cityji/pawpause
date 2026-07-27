/// Strips ANSI escape sequences (e.g. `\x1b[1m`) from CLI tool output so line
/// parsing doesn't have to account for color codes.
pub fn strip_ansi_codes(input: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_color_codes_and_keeps_plain_text() {
        assert_eq!(strip_ansi_codes("\u{1b}[1;32meDP-1\u{1b}[0m (enabled)"), "eDP-1 (enabled)");
    }

    #[test]
    fn leaves_plain_text_untouched() {
        assert_eq!(strip_ansi_codes("no codes here"), "no codes here");
    }
}
