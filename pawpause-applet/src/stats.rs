use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::overlay::notify;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionRecord {
    /// Local calendar date the session ended on, "YYYY-MM-DD".
    pub date: String,
    /// Empty string means "no project".
    #[serde(default)]
    pub project: String,
    pub seconds: u64,
    pub ended_at_epoch: i64,
    /// `None` = logged before completion-tracking existed (unknown, not
    /// "skipped"). `Some(true)` = the work phase ended naturally.
    /// `Some(false)` = ended via skip or stop.
    #[serde(default)]
    pub completed: Option<bool>,
}

pub struct Summary {
    pub hours_focused: f64,
    pub days_accessed: u32,
    pub day_streak: u32,
}

fn sessions_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("pawpause")
        .join("sessions.json")
}

pub fn load_sessions() -> Vec<SessionRecord> {
    let path = sessions_path();
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(sessions) => sessions,
            Err(err) => {
                notify(
                    "PawPause",
                    &format!("Sessions file at {} is invalid ({err}) — stats reset.", path.display()),
                );
                Vec::new()
            }
        },
        Err(err) => {
            notify("PawPause", &format!("Could not read sessions: {err} — stats reset."));
            Vec::new()
        }
    }
}

fn save_sessions(sessions: &[SessionRecord]) {
    let path = sessions_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(sessions) {
        Ok(json) => {
            if let Err(err) = std::fs::write(&path, format!("{json}\n")) {
                notify("PawPause", &format!("Could not save sessions: {err}"));
            }
        }
        Err(err) => notify("PawPause", &format!("Could not serialize sessions: {err}")),
    }
}

/// Appends a finished work session. `seconds` is how long the work phase
/// actually ran for (natural completion, skip, or stop all count).
/// `completed` distinguishes a natural phase-end from a skip/stop.
pub fn log_session(project: &str, seconds: u64, completed: bool) {
    if seconds == 0 {
        return;
    }
    let mut sessions = load_sessions();
    let now = Local::now();
    sessions.push(SessionRecord {
        date: now.format("%Y-%m-%d").to_string(),
        project: project.to_string(),
        seconds,
        ended_at_epoch: now.timestamp(),
        completed: Some(completed),
    });
    save_sessions(&sessions);
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Hours/days/streak computed against "today" in local time.
pub fn summary(sessions: &[SessionRecord]) -> Summary {
    let total_seconds: u64 = sessions.iter().map(|s| s.seconds).sum();
    let hours_focused = total_seconds as f64 / 3600.0;

    let mut days: Vec<NaiveDate> = sessions.iter().filter_map(|s| parse_date(&s.date)).collect();
    days.sort();
    days.dedup();
    let days_accessed = days.len() as u32;

    let today = Local::now().date_naive();
    let day_streak = if days.is_empty() {
        0
    } else {
        // Walk backward from today; a missing "today" doesn't break a streak
        // that's otherwise unbroken through yesterday (user just hasn't
        // focused yet today).
        let start = if days.contains(&today) {
            today
        } else if days.contains(&(today - chrono::Duration::days(1))) {
            today - chrono::Duration::days(1)
        } else {
            return Summary {
                hours_focused,
                days_accessed,
                day_streak: 0,
            };
        };
        let mut streak = 0u32;
        let mut cursor = start;
        loop {
            if days.contains(&cursor) {
                streak += 1;
                cursor -= chrono::Duration::days(1);
            } else {
                break;
            }
        }
        streak
    };

    Summary {
        hours_focused,
        days_accessed,
        day_streak,
    }
}

/// Per-project focused seconds for the current local week (Monday-Sunday),
/// sorted by descending time. Empty project is labeled "No project".
pub fn week_breakdown(sessions: &[SessionRecord]) -> Vec<(String, u64)> {
    let today = Local::now().date_naive();
    let week_start = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);

    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    for session in sessions {
        let Some(date) = parse_date(&session.date) else {
            continue;
        };
        if date < week_start || date > today {
            continue;
        }
        let label = if session.project.trim().is_empty() {
            "No project".to_string()
        } else {
            session.project.clone()
        };
        *totals.entry(label).or_insert(0) += session.seconds;
    }

    let mut rows: Vec<(String, u64)> = totals.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    rows
}

/// Zero-filled, ascending-date focused-seconds totals for the last `days`
/// calendar days ending today (inclusive). Feeds both a trend/area chart and
/// a calendar heatmap — same shape, different renderers.
pub fn daily_breakdown(sessions: &[SessionRecord], days: u32) -> Vec<(NaiveDate, u64)> {
    let today = Local::now().date_naive();
    let mut totals: BTreeMap<NaiveDate, u64> = BTreeMap::new();
    for session in sessions {
        let Some(date) = parse_date(&session.date) else {
            continue;
        };
        *totals.entry(date).or_insert(0) += session.seconds;
    }

    (0..days)
        .rev()
        .map(|offset| {
            let date = today - chrono::Duration::days(offset as i64);
            (date, totals.get(&date).copied().unwrap_or(0))
        })
        .collect()
}

pub struct CompletionSummary {
    pub completed: u32,
    pub skipped_or_stopped: u32,
    /// Sessions logged before completion-tracking existed — surfaced as its
    /// own bucket rather than folded into either count above.
    pub unknown: u32,
}

/// Counts natural-completion vs skip/stop vs pre-tracking-unknown, over all
/// history (mirrors summary()'s all-time scope, not date-scoped).
pub fn completion_summary(sessions: &[SessionRecord]) -> CompletionSummary {
    let mut summary = CompletionSummary {
        completed: 0,
        skipped_or_stopped: 0,
        unknown: 0,
    };
    for session in sessions {
        match session.completed {
            Some(true) => summary.completed += 1,
            Some(false) => summary.skipped_or_stopped += 1,
            None => summary.unknown += 1,
        }
    }
    summary
}

pub fn format_hhmm(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 3600, (seconds % 3600) / 60)
}

/// Human-scaled duration for headline figures: "34m", "2h 15m", "3h".
/// Preferred over `format_hhmm` on stat tiles — rendering 34 minutes of real
/// work as "0.6" hours (or "00:34") reads like a rounding error rather than
/// an achievement, which actively demotivates on a nearly-empty history.
pub fn format_duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Focused seconds logged today (local time).
pub fn today_seconds(sessions: &[SessionRecord]) -> u64 {
    let today = Local::now().date_naive();
    sessions
        .iter()
        .filter(|s| parse_date(&s.date) == Some(today))
        .map(|s| s.seconds)
        .sum()
}

/// Focused seconds in the current local week (Monday-start), and in the
/// equivalent slice of the previous week — compared day-for-day (the same
/// weekday position) so a Tuesday isn't measured against a full 7-day week
/// and made to look like a collapse.
pub fn week_over_week(sessions: &[SessionRecord]) -> (u64, u64) {
    let today = Local::now().date_naive();
    let days_in = today.weekday().num_days_from_monday() as i64;
    let this_start = today - chrono::Duration::days(days_in);
    let last_start = this_start - chrono::Duration::days(7);
    let last_end = last_start + chrono::Duration::days(days_in);

    let mut this_week = 0;
    let mut last_week = 0;
    for session in sessions {
        let Some(date) = parse_date(&session.date) else {
            continue;
        };
        if date >= this_start && date <= today {
            this_week += session.seconds;
        } else if date >= last_start && date <= last_end {
            last_week += session.seconds;
        }
    }
    (this_week, last_week)
}

/// The single best focused day on record, as `(date, seconds)`. Used as the
/// "personal best" reference a bar chart can normalize against, so today's
/// bar means "compared to your best" rather than always filling the row.
pub fn best_day(sessions: &[SessionRecord]) -> Option<(NaiveDate, u64)> {
    let mut totals: BTreeMap<NaiveDate, u64> = BTreeMap::new();
    for session in sessions {
        if let Some(date) = parse_date(&session.date) {
            *totals.entry(date).or_insert(0) += session.seconds;
        }
    }
    totals.into_iter().max_by_key(|(_, seconds)| *seconds)
}

/// Per-weekday focused seconds over the last `weeks` weeks, indexed
/// Monday=0..Sunday=6. Answers "when am I actually productive?" — a question
/// the raw daily series can't, and one that stays meaningful even with a
/// short history.
pub fn weekday_profile(sessions: &[SessionRecord], weeks: u32) -> [u64; 7] {
    let today = Local::now().date_naive();
    let cutoff = today - chrono::Duration::days(weeks as i64 * 7);
    let mut profile = [0u64; 7];
    for session in sessions {
        let Some(date) = parse_date(&session.date) else {
            continue;
        };
        if date < cutoff || date > today {
            continue;
        }
        profile[date.weekday().num_days_from_monday() as usize] += session.seconds;
    }
    profile
}

/// A short, honest, second-person line about the current state — the one
/// piece of "cheering" the UI does. Deliberately never invents a milestone
/// that hasn't happened: with no data it invites a first session rather than
/// congratulating the user for nothing.
pub fn encouragement(today: u64, goal_minutes: u32, streak: u32) -> String {
    let goal_secs = goal_minutes as u64 * 60;

    if goal_minutes > 0 && today >= goal_secs {
        return match streak {
            0 | 1 => "Goal met today. That's the hard part done.".to_string(),
            n => format!("Goal met — {n} days running. Momentum is real."),
        };
    }
    if today == 0 {
        return match streak {
            0 => "No sessions yet. One 25-minute block is enough to start.".to_string(),
            n => format!("{n}-day streak on the line. One session keeps it alive."),
        };
    }
    if goal_minutes > 0 {
        let left = (goal_secs - today).div_ceil(60);
        return format!("{left} more minutes to hit today's goal.");
    }
    format!("{} focused so far today.", format_duration(today))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(date: &str, project: &str, seconds: u64) -> SessionRecord {
        SessionRecord {
            date: date.to_string(),
            project: project.to_string(),
            seconds,
            ended_at_epoch: 0,
            completed: None,
        }
    }

    fn rec_completed(date: &str, seconds: u64, completed: Option<bool>) -> SessionRecord {
        SessionRecord {
            completed,
            ..rec(date, "", seconds)
        }
    }

    #[test]
    fn streak_counts_consecutive_days_ending_today_or_yesterday() {
        let today = Local::now().date_naive();
        let y1 = today - chrono::Duration::days(1);
        let y2 = today - chrono::Duration::days(2);
        let sessions = vec![
            rec(&today.format("%Y-%m-%d").to_string(), "A", 60),
            rec(&y1.format("%Y-%m-%d").to_string(), "A", 60),
            rec(&y2.format("%Y-%m-%d").to_string(), "A", 60),
        ];
        let s = summary(&sessions);
        assert_eq!(s.day_streak, 3);
        assert_eq!(s.days_accessed, 3);
    }

    #[test]
    fn streak_survives_no_session_yet_today() {
        let today = Local::now().date_naive();
        let y1 = today - chrono::Duration::days(1);
        let sessions = vec![rec(&y1.format("%Y-%m-%d").to_string(), "A", 60)];
        let s = summary(&sessions);
        assert_eq!(s.day_streak, 1);
    }

    #[test]
    fn streak_breaks_on_gap() {
        let today = Local::now().date_naive();
        let gap = today - chrono::Duration::days(3);
        let sessions = vec![rec(&gap.format("%Y-%m-%d").to_string(), "A", 60)];
        let s = summary(&sessions);
        assert_eq!(s.day_streak, 0);
    }

    #[test]
    fn week_breakdown_groups_by_project_and_formats_hhmm() {
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let sessions = vec![
            rec(&today, "Alpha", 3600 + 1800),
            rec(&today, "Alpha", 600),
            rec(&today, "", 60),
        ];
        let rows = week_breakdown(&sessions);
        assert_eq!(rows[0].0, "Alpha");
        assert_eq!(format_hhmm(rows[0].1), "01:40");
        assert_eq!(rows[1].0, "No project");
    }

    #[test]
    fn daily_breakdown_is_zero_filled_and_ascending() {
        let today = Local::now().date_naive();
        let y1 = today - chrono::Duration::days(1);
        // y2 (2 days ago) deliberately has no session — should show as 0.
        let sessions = vec![
            rec_completed(&today.format("%Y-%m-%d").to_string(), 120, Some(true)),
            rec_completed(&y1.format("%Y-%m-%d").to_string(), 60, Some(true)),
        ];
        let rows = daily_breakdown(&sessions, 3);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, today - chrono::Duration::days(2));
        assert_eq!(rows[0].1, 0);
        assert_eq!(rows[1].0, y1);
        assert_eq!(rows[1].1, 60);
        assert_eq!(rows[2].0, today);
        assert_eq!(rows[2].1, 120);
    }

    #[test]
    fn format_duration_reads_naturally_at_every_scale() {
        assert_eq!(format_duration(34 * 60), "34m");
        assert_eq!(format_duration(2 * 3600 + 15 * 60), "2h 15m");
        assert_eq!(format_duration(3 * 3600), "3h");
        assert_eq!(format_duration(30), "0m");
    }

    #[test]
    fn week_over_week_compares_the_same_number_of_days() {
        let today = Local::now().date_naive();
        let days_in = today.weekday().num_days_from_monday() as i64;
        let this_start = today - chrono::Duration::days(days_in);
        // Same weekday position one week earlier — always inside the compared slice.
        let last_equivalent = this_start - chrono::Duration::days(7) + chrono::Duration::days(days_in);
        let sessions = vec![
            rec(&today.format("%Y-%m-%d").to_string(), "A", 600),
            rec(&last_equivalent.format("%Y-%m-%d").to_string(), "A", 300),
        ];
        assert_eq!(week_over_week(&sessions), (600, 300));
    }

    #[test]
    fn best_day_picks_the_highest_total_across_sessions() {
        let sessions = vec![
            rec("2026-01-01", "A", 600),
            rec("2026-01-02", "A", 400),
            rec("2026-01-02", "B", 500),
        ];
        let (date, seconds) = best_day(&sessions).expect("has data");
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 1, 2).unwrap());
        assert_eq!(seconds, 900);
        assert_eq!(best_day(&[]), None);
    }

    #[test]
    fn weekday_profile_buckets_by_day_of_week() {
        let today = Local::now().date_naive();
        let sessions = vec![rec(&today.format("%Y-%m-%d").to_string(), "A", 600)];
        let profile = weekday_profile(&sessions, 4);
        assert_eq!(profile[today.weekday().num_days_from_monday() as usize], 600);
        assert_eq!(profile.iter().sum::<u64>(), 600);
    }

    #[test]
    fn encouragement_never_congratulates_an_empty_day() {
        assert!(encouragement(0, 120, 0).contains("start"));
        assert!(encouragement(0, 120, 4).contains("4-day streak"));
        // Goal met.
        assert!(encouragement(3600, 30, 3).contains("3 days"));
        // Partway: reports minutes remaining, rounded up.
        assert!(encouragement(600, 30, 0).contains("20 more minutes"));
    }

    #[test]
    fn today_seconds_sums_only_todays_records() {
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let sessions = vec![rec(&today, "A", 120), rec("2020-01-01", "A", 999)];
        assert_eq!(today_seconds(&sessions), 120);
    }

    #[test]
    fn completion_summary_splits_completed_skipped_and_unknown() {
        let sessions = vec![
            rec_completed("2026-01-01", 60, Some(true)),
            rec_completed("2026-01-02", 60, Some(true)),
            rec_completed("2026-01-03", 60, Some(false)),
            rec_completed("2026-01-04", 60, None),
        ];
        let s = completion_summary(&sessions);
        assert_eq!(s.completed, 2);
        assert_eq!(s.skipped_or_stopped, 1);
        assert_eq!(s.unknown, 1);
    }
}
