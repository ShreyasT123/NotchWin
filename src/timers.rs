use std::time::{Duration, Instant};

// ── types ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum TimerKind {
    Countdown,
    Stopwatch,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TimerState {
    Idle,
    Running,
    Paused,
    Finished,
}

#[derive(Clone, Debug)]
pub struct IslandTimer {
    pub id: String,
    pub kind: TimerKind,
    pub state: TimerState,
    pub label: String,
    /// Target duration for countdowns; `None` for stopwatches.
    pub duration: Option<Duration>,
    /// Accumulated elapsed time from all previous runs (not including the
    /// current in-progress run, which is tracked via `started_at`).
    pub elapsed: Duration,
    /// The `Instant` at which the timer was most recently resumed/started.
    pub started_at: Option<Instant>,
    /// Lap snapshots for stopwatches.
    pub laps: Vec<Duration>,
}

impl IslandTimer {
    fn new(id: String, kind: TimerKind, duration: Option<Duration>, label: &str) -> Self {
        Self {
            id,
            kind,
            state: TimerState::Idle,
            label: label.to_string(),
            duration,
            elapsed: Duration::ZERO,
            started_at: None,
            laps: Vec::new(),
        }
    }

    // ── queries ───────────────────────────────────────────────────────────────

    /// Total elapsed time at `now`, including any in-progress run.
    pub fn elapsed_at(&self, now: Instant) -> Duration {
        match self.started_at {
            Some(started) if self.state == TimerState::Running => {
                self.elapsed + now.saturating_duration_since(started)
            }
            _ => self.elapsed,
        }
    }

    /// For countdowns: how much time is left at `now`.
    /// Saturates at zero; returns zero for stopwatches.
    pub fn remaining_at(&self, now: Instant) -> Duration {
        match &self.duration {
            Some(target) => target.saturating_sub(self.elapsed_at(now)),
            None => Duration::ZERO,
        }
    }

    /// Progress fraction 0.0–1.0 for countdowns, `None` for stopwatches.
    pub fn progress_at(&self, now: Instant) -> Option<f32> {
        let target = self.duration?;
        if target.is_zero() {
            return Some(1.0);
        }
        let elapsed = self.elapsed_at(now);
        let fraction = elapsed.as_secs_f32() / target.as_secs_f32();
        Some(fraction.min(1.0))
    }

    /// `true` while the timer is actively counting.
    pub fn is_running(&self) -> bool {
        self.state == TimerState::Running
    }

    /// `true` after a countdown reaches zero.
    pub fn is_finished(&self) -> bool {
        self.state == TimerState::Finished
    }
}

// ── controller ────────────────────────────────────────────────────────────────

pub struct TimerController {
    timers: Vec<IslandTimer>,
    focused_id: Option<String>,
    next_id: u64,
}

impl TimerController {
    pub fn new() -> Self {
        Self {
            timers: Vec::new(),
            focused_id: None,
            next_id: 1,
        }
    }

    // ── mutation ──────────────────────────────────────────────────────────────

    /// Adds a new countdown timer and returns its id.
    pub fn add_countdown(&mut self, duration: Duration, label: &str) -> String {
        let id = self.fresh_id();
        self.timers
            .push(IslandTimer::new(id.clone(), TimerKind::Countdown, Some(duration), label));
        id
    }

    /// Adds a new stopwatch and returns its id.
    pub fn add_stopwatch(&mut self) -> String {
        let id = self.fresh_id();
        self.timers
            .push(IslandTimer::new(id.clone(), TimerKind::Stopwatch, None, "Stopwatch"));
        id
    }

    /// Start if idle/paused, pause if running. No-op for finished timers.
    pub fn toggle(&mut self, id: &str) {
        let now = Instant::now();
        if let Some(t) = self.get_mut(id) {
            match t.state {
                TimerState::Idle | TimerState::Paused => {
                    t.started_at = Some(now);
                    t.state = TimerState::Running;
                }
                TimerState::Running => {
                    // Commit in-progress time to `elapsed` before pausing.
                    t.elapsed = t.elapsed_at(now);
                    t.started_at = None;
                    t.state = TimerState::Paused;
                }
                TimerState::Finished => {}
            }
        }
    }

    /// Reset to initial state, preserving kind, duration, and label.
    pub fn reset(&mut self, id: &str) {
        if let Some(t) = self.get_mut(id) {
            t.elapsed = Duration::ZERO;
            t.started_at = None;
            t.state = TimerState::Idle;
            t.laps.clear();
        }
    }

    /// Resume a paused or finished timer without toggling (explicit resume).
    pub fn resume(&mut self, id: &str) {
        let now = Instant::now();
        if let Some(t) = self.get_mut(id) {
            if t.state == TimerState::Paused || t.state == TimerState::Finished {
                t.started_at = Some(now);
                t.state = TimerState::Running;
            }
        }
    }

    /// Record a lap for a running stopwatch (snapshot of current elapsed time).
    pub fn lap(&mut self, id: &str) {
        let now = Instant::now();
        if let Some(t) = self.get_mut(id) {
            if t.kind == TimerKind::Stopwatch && t.state == TimerState::Running {
                let snap = t.elapsed_at(now);
                t.laps.push(snap);
            }
        }
    }

    /// Remove a timer by id.
    pub fn remove(&mut self, id: &str) {
        self.timers.retain(|t| t.id != id);
        if self.focused_id.as_deref() == Some(id) {
            self.focused_id = None;
        }
    }

    /// Set the focused timer.
    pub fn focus(&mut self, id: &str) {
        if self.timers.iter().any(|t| t.id == id) {
            self.focused_id = Some(id.to_string());
        }
    }

    // ── queries ───────────────────────────────────────────────────────────────

    /// Returns the focused timer if set, otherwise the first running timer,
    /// otherwise `None`.
    pub fn primary(&self) -> Option<&IslandTimer> {
        if let Some(ref fid) = self.focused_id {
            if let Some(t) = self.timers.iter().find(|t| &t.id == fid) {
                return Some(t);
            }
        }
        self.timers.iter().find(|t| t.is_running())
    }

    /// Immutable slice of all timers.
    pub fn timers(&self) -> &[IslandTimer] {
        &self.timers
    }

    // ── tick ──────────────────────────────────────────────────────────────────

    /// Called once per frame / once per second. Marks countdowns as `Finished`
    /// when they reach zero while running.
    pub fn tick(&mut self, now: Instant) {
        for t in &mut self.timers {
            if t.state != TimerState::Running {
                continue;
            }
            if t.kind == TimerKind::Countdown {
                if let Some(target) = t.duration {
                    let elapsed = t.elapsed + now.saturating_duration_since(
                        t.started_at.expect("running timer must have started_at"),
                    );
                    if elapsed >= target {
                        // Clamp elapsed so remaining is exactly zero.
                        t.elapsed = target;
                        t.started_at = None;
                        t.state = TimerState::Finished;
                    }
                }
            }
        }
    }

    // ── private ───────────────────────────────────────────────────────────────

    fn fresh_id(&mut self) -> String {
        let id = format!("timer_{}", self.next_id);
        self.next_id += 1;
        id
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut IslandTimer> {
        self.timers.iter_mut().find(|t| t.id == id)
    }
}

impl Default for TimerController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn countdown_lifecycle() {
        let mut ctrl = TimerController::new();
        let id = ctrl.add_countdown(Duration::from_secs(10), "Test");

        // Idle → Running
        ctrl.toggle(&id);
        assert!(ctrl.timers()[0].is_running());

        // Running → Paused
        ctrl.toggle(&id);
        assert_eq!(ctrl.timers()[0].state, TimerState::Paused);

        // Reset clears elapsed
        ctrl.reset(&id);
        assert_eq!(ctrl.timers()[0].elapsed, Duration::ZERO);
    }

    #[test]
    fn stopwatch_lap() {
        let mut ctrl = TimerController::new();
        let id = ctrl.add_stopwatch();
        ctrl.toggle(&id);
        ctrl.lap(&id);
        assert_eq!(ctrl.timers()[0].laps.len(), 1);
    }

    #[test]
    fn primary_falls_back_to_running() {
        let mut ctrl = TimerController::new();
        ctrl.add_countdown(Duration::from_secs(5), "A");
        let b = ctrl.add_countdown(Duration::from_secs(5), "B");
        ctrl.toggle(&b);
        let p = ctrl.primary().unwrap();
        assert_eq!(p.id, b);
    }
}
