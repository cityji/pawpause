import threading
from enum import Enum


class Phase(Enum):
    WORK = "work"
    SHORT_BREAK = "short_break"
    LONG_BREAK = "long_break"


class RunState(Enum):
    IDLE = "idle"
    RUNNING = "running"
    PAUSED = "paused"


PHASE_LABELS = {
    Phase.WORK: "Work",
    Phase.SHORT_BREAK: "Short break",
    Phase.LONG_BREAK: "Long break",
}

BREAK_PHASES = (Phase.SHORT_BREAK, Phase.LONG_BREAK)


class Pomodoro:
    """A minimal Pomodoro state machine, driven by an external 1s ticker.

    on_transition(old_phase, new_phase) fires on every phase change,
    including start (None -> WORK) and stop (phase -> None).
    on_tick() fires once per call to tick(), for UI refresh.
    """

    def __init__(self, config, on_transition, on_tick):
        self.config = config
        self.on_transition = on_transition
        self.on_tick = on_tick

        self.state = RunState.IDLE
        self.phase = None
        self.remaining = 0
        self.session_count = 0
        self._lock = threading.Lock()

    def _duration(self, phase):
        minutes = {
            Phase.WORK: self.config.work_minutes,
            Phase.SHORT_BREAK: self.config.short_break_minutes,
            Phase.LONG_BREAK: self.config.long_break_minutes,
        }[phase]
        return int(minutes * 60)

    def _advance(self, phase, session_count):
        if phase == Phase.WORK:
            session_count += 1
            if session_count % self.config.sessions_before_long_break == 0:
                return Phase.LONG_BREAK, session_count
            return Phase.SHORT_BREAK, session_count
        return Phase.WORK, session_count

    def start(self):
        with self._lock:
            if self.state != RunState.IDLE:
                return
            self.phase = Phase.WORK
            self.remaining = self._duration(Phase.WORK)
            self.session_count = 0
            self.state = RunState.RUNNING
        self.on_transition(None, Phase.WORK)

    def pause(self):
        with self._lock:
            if self.state == RunState.RUNNING:
                self.state = RunState.PAUSED

    def resume(self):
        with self._lock:
            if self.state == RunState.PAUSED:
                self.state = RunState.RUNNING

    def skip(self):
        with self._lock:
            if self.phase is None:
                return
            old_phase = self.phase
            new_phase, self.session_count = self._advance(old_phase, self.session_count)
            self.phase = new_phase
            self.remaining = self._duration(new_phase)
        self.on_transition(old_phase, new_phase)

    def stop(self):
        with self._lock:
            old_phase = self.phase
            self.phase = None
            self.state = RunState.IDLE
            self.remaining = 0
            self.session_count = 0
        if old_phase is not None:
            self.on_transition(old_phase, None)

    def tick(self):
        """Call once per second from a background thread."""
        transition = None
        with self._lock:
            if self.state == RunState.RUNNING and self.phase is not None:
                self.remaining -= 1
                if self.remaining <= 0:
                    old_phase = self.phase
                    new_phase, self.session_count = self._advance(old_phase, self.session_count)
                    self.phase = new_phase
                    self.remaining = self._duration(new_phase)
                    transition = (old_phase, new_phase)
        if transition:
            self.on_transition(*transition)
        self.on_tick()

    def status_text(self):
        with self._lock:
            if self.phase is None:
                return "Idle"
            mm, ss = divmod(max(self.remaining, 0), 60)
            label = PHASE_LABELS[self.phase]
            suffix = " (paused)" if self.state == RunState.PAUSED else ""
            return f"{label} — {mm:02d}:{ss:02d} left{suffix}"
