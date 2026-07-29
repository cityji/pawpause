use std::collections::BTreeMap;
use std::process::Command;

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime};

use crate::ansi::strip_ansi_codes;

/// One slice of machine uptime that falls entirely within a single calendar
/// date. A boot that spans midnight is split into one `BootSession` per date
/// it covers (see `split_at_midnight`) — attributing a 20-hour overnight boot
/// wholly to the day it *started* produced an absurd spike next to a false
/// zero, which is what made this metric untrustworthy.
///
/// Note this measures **uptime**, not attentive use: a suspended-but-not-shut-
/// down laptop still counts as running, which is why the UI labels it
/// "Uptime" rather than "Laptop usage".
pub struct BootSession {
    pub date: NaiveDate,
    pub minutes: u64,
}

/// Parses "system boot" records from `last reboot -n <n>`. The `reboot`
/// pseudo-user is required: plain `last -n <n>` caps *total* records, so a
/// busy login history can push every reboot line out of the window and yield
/// an empty chart. Falls back to an empty list if the `last` binary is
/// unavailable or every line fails to parse — never panics.
pub fn list_boot_sessions(n: u32) -> Vec<BootSession> {
    let Ok(output) = Command::new("last").arg("reboot").arg("-n").arg(n.to_string()).output() else {
        return Vec::new();
    };
    let text = strip_ansi_codes(&String::from_utf8_lossy(&output.stdout));
    parse_last_output(&text, Local::now().naive_local())
}

/// Sums BootSession minutes per calendar date, ascending. Capped at 1440 —
/// overlapping `last` records (which do occur after an unclean shutdown)
/// could otherwise report more than 24 hours of uptime in one day.
pub fn daily_usage_minutes(sessions: &[BootSession]) -> Vec<(NaiveDate, u64)> {
    let mut totals: BTreeMap<NaiveDate, u64> = BTreeMap::new();
    for session in sessions {
        let entry = totals.entry(session.date).or_insert(0);
        *entry = (*entry + session.minutes).min(24 * 60);
    }
    totals.into_iter().collect()
}

/// The earliest date any parsed boot record covers. The UI uses this to avoid
/// charting zeros for days that simply predate the `wtmp` log — rotation
/// routinely leaves less than two weeks of history, and drawing those as flat
/// zero reads as "laptop was off", which is a lie.
pub fn history_starts_on(sessions: &[BootSession]) -> Option<NaiveDate> {
    sessions.iter().map(|s| s.date).min()
}

fn parse_last_output(text: &str, now: NaiveDateTime) -> Vec<BootSession> {
    let today = now.date();
    text.lines()
        .filter_map(|line| parse_boot_line(line, today, now))
        .flat_map(|(start, minutes)| split_at_midnight(start, minutes))
        .collect()
}

/// Splits a `minutes`-long span starting at `start` into per-calendar-date
/// chunks, so an overnight boot credits each day the share it actually ran.
fn split_at_midnight(start: NaiveDateTime, minutes: u64) -> Vec<BootSession> {
    let mut out = Vec::new();
    let mut cursor = start;
    let mut left = minutes;

    while left > 0 {
        let next_midnight = (cursor.date() + chrono::Duration::days(1)).and_hms_opt(0, 0, 0).expect("midnight is always valid");
        let until_midnight = (next_midnight - cursor).num_minutes().max(0) as u64;
        let chunk = left.min(until_midnight);
        if chunk > 0 {
            out.push(BootSession {
                date: cursor.date(),
                minutes: chunk,
            });
        }
        left -= chunk;
        cursor = next_midnight;
    }

    out
}

/// Lines look like:
///   "reboot   system boot  <kernel> Mon Jul 27 04:28 - 07:44  (03:15)"
///   "reboot   system boot  <kernel> Mon Jul 27 07:44   still running"
/// A completed boot's duration is read from the trailing `(HH:MM)` or
/// `(D+HH:MM)` parenthetical rather than diffing start/end clock times,
/// since multi-day boots print only an end *time*, no end date, making
/// direct date arithmetic from the two clock times unreliable.
///
/// Returns the span as `(start instant, duration)`; the caller splits it
/// across calendar dates rather than assigning it all to the start date.
fn parse_boot_line(line: &str, today: NaiveDate, now: NaiveDateTime) -> Option<(NaiveDateTime, u64)> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.first() != Some(&"reboot") || tokens.get(1) != Some(&"system") || tokens.get(2) != Some(&"boot") {
        return None;
    }

    let month = month_number(tokens.get(5)?)?;
    let day: u32 = tokens.get(6)?.parse().ok()?;
    let start_time = tokens.get(7)?;
    let date = resolve_date(month, day, today)?;
    let start = parse_time_on(date, start_time)?;

    let minutes = if tokens.get(8) == Some(&"still") {
        (now - start).num_minutes().max(0) as u64
    } else {
        parse_duration_minutes(tokens.get(10)?)?
    };

    Some((start, minutes))
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
        // 4 reboot lines, but the Jul 24 one spans midnight and splits in two.
        assert_eq!(parse_last_output(SAMPLE, fixed_now()).len(), 5);
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
    fn overnight_boot_is_split_across_the_dates_it_actually_covers() {
        // "Fri Jul 24 20:27 - 17:18 (20:50)" = 1250 minutes from 20:27.
        // Jul 24 gets 20:27->midnight = 213; Jul 25 gets the remaining 1037.
        let sessions = parse_last_output(SAMPLE, fixed_now());
        let jul24 = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();
        let jul25 = NaiveDate::from_ymd_opt(2026, 7, 25).unwrap();
        assert!(sessions.iter().any(|s| s.date == jul24 && s.minutes == 213));
        assert!(sessions.iter().any(|s| s.date == jul25 && s.minutes == 1037));
    }

    #[test]
    fn multi_day_boot_fills_whole_intermediate_days() {
        let start = NaiveDate::from_ymd_opt(2026, 7, 20).unwrap().and_hms_opt(22, 0, 0).unwrap();
        // 22:00 Jul 20 + 2 days 3h: 120 to midnight, two full days, then a
        // 60-minute tail on Jul 23.
        let split = split_at_midnight(start, 120 + 1440 + 1440 + 60);
        assert_eq!(split.len(), 4);
        assert_eq!(split[0].minutes, 120); // 22:00 -> midnight
        assert_eq!(split[1].minutes, 1440); // full day
        assert_eq!(split[2].minutes, 1440); // full day
        assert_eq!(split[3].date, NaiveDate::from_ymd_opt(2026, 7, 23).unwrap());
        assert_eq!(split[3].minutes, 60);
    }

    #[test]
    fn daily_usage_never_reports_more_than_a_full_day() {
        let day = NaiveDate::from_ymd_opt(2026, 7, 27).unwrap();
        let sessions = vec![
            BootSession { date: day, minutes: 1000 },
            BootSession { date: day, minutes: 1000 },
        ];
        assert_eq!(daily_usage_minutes(&sessions), vec![(day, 1440)]);
    }

    #[test]
    fn history_starts_on_reports_the_earliest_covered_date() {
        let sessions = parse_last_output(SAMPLE, fixed_now());
        assert_eq!(history_starts_on(&sessions), NaiveDate::from_ymd_opt(2026, 7, 24));
        assert_eq!(history_starts_on(&[]), None);
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
