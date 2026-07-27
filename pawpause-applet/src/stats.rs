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

/// Appends a completed work session. `seconds` is how long the work phase
/// actually ran for (natural completion, skip, or stop all count).
pub fn log_session(project: &str, seconds: u64) {
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

pub fn format_hhmm(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 3600, (seconds % 3600) / 60)
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
}
