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

/// A phase transition, with how long the phase being left had actually run
/// for (natural completion, skip, or stop all count) — used to credit focus
/// time to the right task/project.
pub struct Transition {
    pub old_phase: Option<Phase>,
    pub new_phase: Option<Phase>,
    pub old_phase_elapsed_secs: i64,
}

/// A minimal Pomodoro state machine, driven by an external 1s tick().
pub struct Pomodoro {
    pub state: RunState,
    pub phase: Option<Phase>,
    pub remaining: i64,
    pub session_count: u32,
    /// The full planned duration of the current phase, so elapsed time can be
    /// recovered as `current_phase_duration - remaining` when it ends.
    current_phase_duration: i64,
}

impl Pomodoro {
    pub fn new() -> Self {
        Self {
            state: RunState::Idle,
            phase: None,
            remaining: 0,
            session_count: 0,
            current_phase_duration: 0,
        }
    }

    fn elapsed_in_current_phase(&self) -> i64 {
        (self.current_phase_duration - self.remaining.max(0)).max(0)
    }

    fn duration(phase: Phase, config: &Config) -> i64 {
        let minutes = match phase {
            Phase::Work => config.work_minutes,
            Phase::ShortBreak => config.short_break_minutes,
            Phase::LongBreak => config.long_break_minutes,
        };
        (minutes * 60.0).round() as i64
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

    /// Returns Some(transition) if a transition occurred.
    pub fn start(&mut self, config: &Config) -> Option<Transition> {
        if self.state != RunState::Idle {
            return None;
        }
        self.phase = Some(Phase::Work);
        self.current_phase_duration = Self::duration(Phase::Work, config);
        self.remaining = self.current_phase_duration;
        self.session_count = 0;
        self.state = RunState::Running;
        Some(Transition {
            old_phase: None,
            new_phase: self.phase,
            old_phase_elapsed_secs: 0,
        })
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

    pub fn skip(&mut self, config: &Config) -> Option<Transition> {
        let old_phase = self.phase?;
        let old_phase_elapsed_secs = self.elapsed_in_current_phase();
        let (new_phase, session_count) = Self::advance(old_phase, self.session_count, config);
        self.phase = Some(new_phase);
        self.session_count = session_count;
        self.current_phase_duration = Self::duration(new_phase, config);
        self.remaining = self.current_phase_duration;
        Some(Transition {
            old_phase: Some(old_phase),
            new_phase: Some(new_phase),
            old_phase_elapsed_secs,
        })
    }

    pub fn stop(&mut self) -> Option<Transition> {
        let old_phase = self.phase;
        let old_phase_elapsed_secs = self.elapsed_in_current_phase();
        self.phase = None;
        self.state = RunState::Idle;
        self.remaining = 0;
        self.current_phase_duration = 0;
        self.session_count = 0;
        old_phase.map(|p| Transition {
            old_phase: Some(p),
            new_phase: None,
            old_phase_elapsed_secs,
        })
    }

    /// Call once per second. Returns Some(transition) if a phase auto-advanced
    /// this tick.
    pub fn tick(&mut self, config: &Config) -> Option<Transition> {
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
        let old_phase_elapsed_secs = self.current_phase_duration;
        let (new_phase, session_count) = Self::advance(phase, self.session_count, config);
        self.phase = Some(new_phase);
        self.session_count = session_count;
        self.current_phase_duration = Self::duration(new_phase, config);
        self.remaining = self.current_phase_duration;
        Some(Transition {
            old_phase: Some(phase),
            new_phase: Some(new_phase),
            old_phase_elapsed_secs,
        })
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
