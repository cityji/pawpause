use std::collections::BTreeMap;
use std::process::Command;

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime};

use crate::ansi::strip_ansi_codes;

/// One boot-to-shutdown (or boot-to-still-running) span, bucketed under the
/// calendar date the boot *started* on (laptop-usage is naturally "day I
/// turned it on" — a different convention than stats.rs's end-date
/// bucketing, and that's fine, they're different metrics).
pub struct BootSession {
    pub date: NaiveDate,
    pub minutes: u64,
}

/// Parses up to `n` "system boot" records from `last -n <n>`. Falls back to
/// an empty list if the `last` binary is unavailable, produces no reboot
/// lines, or every line fails to parse — never panics on unexpected output.
pub fn list_boot_sessions(n: u32) -> Vec<BootSession> {
    let Ok(output) = Command::new("last").arg("-n").arg(n.to_string()).output() else {
        return Vec::new();
    };
    let text = strip_ansi_codes(&String::from_utf8_lossy(&output.stdout));
    parse_last_output(&text, Local::now().naive_local())
}

/// Sums BootSession minutes per calendar date, ascending.
pub fn daily_usage_minutes(sessions: &[BootSession]) -> Vec<(NaiveDate, u64)> {
    let mut totals: BTreeMap<NaiveDate, u64> = BTreeMap::new();
    for session in sessions {
        *totals.entry(session.date).or_insert(0) += session.minutes;
    }
    totals.into_iter().collect()
}

fn parse_last_output(text: &str, now: NaiveDateTime) -> Vec<BootSession> {
    let today = now.date();
    text.lines().filter_map(|line| parse_boot_line(line, today, now)).collect()
}

/// Lines look like:
///   "reboot   system boot  <kernel> Mon Jul 27 04:28 - 07:44  (03:15)"
///   "reboot   system boot  <kernel> Mon Jul 27 07:44   still running"
/// A completed boot's duration is read from the trailing `(HH:MM)` or
/// `(D+HH:MM)` parenthetical rather than diffing start/end clock times,
/// since multi-day boots print only an end *time*, no end date, making
/// direct date arithmetic from the two clock times unreliable.
fn parse_boot_line(line: &str, today: NaiveDate, now: NaiveDateTime) -> Option<BootSession> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.first() != Some(&"reboot") || tokens.get(1) != Some(&"system") || tokens.get(2) != Some(&"boot") {
        return None;
    }

    let month = month_number(tokens.get(5)?)?;
    let day: u32 = tokens.get(6)?.parse().ok()?;
    let start_time = tokens.get(7)?;
    let date = resolve_date(month, day, today)?;

    let minutes = if tokens.get(8) == Some(&"still") {
        let start = parse_time_on(date, start_time)?;
        (now - start).num_minutes().max(0) as u64
    } else {
        parse_duration_minutes(tokens.get(10)?)?
    };

    Some(BootSession { date, minutes })
}

fn month_number(abbrev: &str) -> Option<u32> {
    Some(match abbrev {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

/// `last` prints no year. Assume the current year unless that would put the
/// date in the future (relative to `today`), in which case it must be from
/// last year — handles the Dec/Jan wraparound cheaply.
fn resolve_date(month: u32, day: u32, today: NaiveDate) -> Option<NaiveDate> {
    let date = NaiveDate::from_ymd_opt(today.year(), month, day)?;
    if date > today {
        NaiveDate::from_ymd_opt(today.year() - 1, month, day)
    } else {
        Some(date)
    }
}

fn parse_time_on(date: NaiveDate, time_str: &str) -> Option<NaiveDateTime> {
    let (h, m) = time_str.split_once(':')?;
    let time = NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)?;
    Some(date.and_time(time))
}

/// `"(03:15)"` -> 195, `"(1+02:30)"` (multi-day) -> 1590.
fn parse_duration_minutes(raw: &str) -> Option<u64> {
    let trimmed = raw.trim_start_matches('(').trim_end_matches(')');
    let (days, hhmm) = match trimmed.split_once('+') {
        Some((d, rest)) => (d.parse::<u64>().ok()?, rest),
        None => (0, trimmed),
    };
    let (h, m) = hhmm.split_once(':')?;
    Some(days * 24 * 60 + h.parse::<u64>().ok()? * 60 + m.parse::<u64>().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "reboot   system boot  7.0.11-76070011- Mon Jul 27 07:44   still running\nreboot   system boot  7.0.11-76070011- Mon Jul 27 04:28 - 07:44  (03:15)\nreboot   system boot  7.0.11-76070011- Sun Jul 26 17:33 - 22:26  (04:52)\nreboot   system boot  7.0.11-76070011- Fri Jul 24 20:27 - 17:18  (20:50)\n\nwtmp begins Sat Jul 11 15:59:17 2026\n";

    fn fixed_now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 7, 27).unwrap().and_hms_opt(11, 51, 0).unwrap()
    }

    #[test]
    fn ignores_non_reboot_lines_like_the_wtmp_footer() {
        assert_eq!(parse_last_output(SAMPLE, fixed_now()).len(), 4);
    }

    #[test]
    fn still_running_boot_uses_elapsed_time_since_start() {
        let sessions = parse_last_output(SAMPLE, fixed_now());
        let today = NaiveDate::from_ymd_opt(2026, 7, 27).unwrap();
        // 07:44 -> 11:51 fixed "now" = 247 minutes.
        assert!(sessions.iter().any(|s| s.date == today && s.minutes == 247));
    }

    #[test]
    fn completed_boot_reads_duration_from_the_parenthetical() {
        let sessions = parse_last_output(SAMPLE, fixed_now());
        let jul26 = NaiveDate::from_ymd_opt(2026, 7, 26).unwrap();
        assert!(sessions.iter().any(|s| s.date == jul26 && s.minutes == 292));
    }

    #[test]
    fn daily_usage_sums_multiple_boots_on_the_same_date() {
        let jul27 = NaiveDate::from_ymd_opt(2026, 7, 27).unwrap();
        let jul26 = NaiveDate::from_ymd_opt(2026, 7, 26).unwrap();
        let sessions = vec![
            BootSession { date: jul27, minutes: 100 },
            BootSession { date: jul27, minutes: 50 },
            BootSession { date: jul26, minutes: 30 },
        ];
        assert_eq!(daily_usage_minutes(&sessions), vec![(jul26, 30), (jul27, 150)]);
    }

    #[test]
    fn parse_duration_minutes_handles_multi_day_format() {
        assert_eq!(parse_duration_minutes("(03:15)"), Some(195));
        assert_eq!(parse_duration_minutes("(1+02:30)"), Some(1590));
    }
}
