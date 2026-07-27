use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Work,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Work => "Work",
            Phase::ShortBreak => "Short break",
            Phase::LongBreak => "Long break",
        }
    }

    pub fn is_break(self) -> bool {
        matches!(self, Phase::ShortBreak | Phase::LongBreak)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Idle,
    Running,
    Paused,
}

/// A minimal Pomodoro state machine, driven by an external 1s tick().
pub struct Pomodoro {
    pub state: RunState,
    pub phase: Option<Phase>,
    pub remaining: i64,
    pub session_count: u32,
}

impl Pomodoro {
    pub fn new() -> Self {
        Self {
            state: RunState::Idle,
            phase: None,
            remaining: 0,
            session_count: 0,
        }
    }

    fn duration(phase: Phase, config: &Config) -> i64 {
        let minutes = match phase {
            Phase::Work => config.work_minutes,
            Phase::ShortBreak => config.short_break_minutes,
            Phase::LongBreak => config.long_break_minutes,
        };
        (minutes * 60) as i64
    }

    fn advance(phase: Phase, session_count: u32, config: &Config) -> (Phase, u32) {
        match phase {
            Phase::Work => {
                let session_count = session_count + 1;
                if config.sessions_before_long_break > 0
                    && session_count % config.sessions_before_long_break == 0
                {
                    (Phase::LongBreak, session_count)
                } else {
                    (Phase::ShortBreak, session_count)
                }
            }
            Phase::ShortBreak | Phase::LongBreak => (Phase::Work, session_count),
        }
    }

    /// Returns Some((old_phase, new_phase)) if a transition occurred.
    pub fn start(&mut self, config: &Config) -> Option<(Option<Phase>, Option<Phase>)> {
        if self.state != RunState::Idle {
            return None;
        }
        self.phase = Some(Phase::Work);
        self.remaining = Self::duration(Phase::Work, config);
        self.session_count = 0;
        self.state = RunState::Running;
        Some((None, self.phase))
    }

    pub fn pause(&mut self) {
        if self.state == RunState::Running {
            self.state = RunState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == RunState::Paused {
            self.state = RunState::Running;
        }
    }

    pub fn skip(&mut self, config: &Config) -> Option<(Option<Phase>, Option<Phase>)> {
        let old_phase = self.phase?;
        let (new_phase, session_count) = Self::advance(old_phase, self.session_count, config);
        self.phase = Some(new_phase);
        self.session_count = session_count;
        self.remaining = Self::duration(new_phase, config);
        Some((Some(old_phase), Some(new_phase)))
    }

    pub fn stop(&mut self) -> Option<(Option<Phase>, Option<Phase>)> {
        let old_phase = self.phase;
        self.phase = None;
        self.state = RunState::Idle;
        self.remaining = 0;
        self.session_count = 0;
        old_phase.map(|p| (Some(p), None))
    }

    /// Call once per second. Returns Some((old_phase, new_phase)) if a phase
    /// auto-advanced this tick.
    pub fn tick(&mut self, config: &Config) -> Option<(Option<Phase>, Option<Phase>)> {
        if self.state != RunState::Running {
            return None;
        }
        let Some(phase) = self.phase else {
            return None;
        };
        self.remaining -= 1;
        if self.remaining > 0 {
            return None;
        }
        let (new_phase, session_count) = Self::advance(phase, self.session_count, config);
        self.phase = Some(new_phase);
        self.session_count = session_count;
        self.remaining = Self::duration(new_phase, config);
        Some((Some(phase), Some(new_phase)))
    }

    pub fn status_text(&self) -> String {
        match self.phase {
            None => "Idle".to_string(),
            Some(phase) => {
                let remaining = self.remaining.max(0);
                let mm = remaining / 60;
                let ss = remaining % 60;
                let suffix = if self.state == RunState::Paused {
                    " (paused)"
                } else {
                    ""
                };
                format!("{} — {mm:02}:{ss:02} left{suffix}", phase.label())
            }
        }
    }

    pub fn short_time_text(&self) -> String {
        match self.phase {
            None => "—".to_string(),
            Some(_) => {
                let remaining = self.remaining.max(0);
                format!("{:02}:{:02}", remaining / 60, remaining % 60)
            }
        }
    }
}
