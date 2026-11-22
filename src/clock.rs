use chrono::{DateTime, Datelike, Local, Timelike, Weekday};
use std::time::{Duration, Instant};

/// Tracks which positions of the clock string just changed so only those
/// digits roll. Mirrors `RollingText` in `design/components/rolling_text.dart`.
#[derive(Clone, Default)]
pub struct DigitRoll {
    previous: String,
    changed: Vec<bool>,
    direction: Vec<i8>,
    started: Option<Instant>,
}

impl DigitRoll {
    const DURATION: Duration = Duration::from_millis(380);

    /// Set the baseline without animating (used at startup).
    pub fn seed(&mut self, text: &str) {
        self.previous = text.to_string();
        self.changed.clear();
        self.direction.clear();
        self.started = None;
    }

    /// Compare `text` with the last value. Returns true while a roll is active.
    pub fn update(&mut self, text: &str, now: Instant) -> bool {
        if self.previous == text {
            return self.started.is_some_and(|t| now.duration_since(t) < Self::DURATION);
        }
        let old: Vec<char> = self.previous.chars().collect();
        let new: Vec<char> = text.chars().collect();
        let len = old.len().max(new.len());
        self.changed = Vec::with_capacity(len);
        self.direction = Vec::with_capacity(len);
        for i in 0..len {
            let a = old.get(i).copied().unwrap_or('0');
            let b = new.get(i).copied().unwrap_or('0');
            let is_digit = |c: char| c.is_ascii_digit();
            let changed = a != b && is_digit(a) && is_digit(b);
            self.changed.push(changed);
            self.direction.push(if b > a { 1 } else { -1 });
        }
        self.previous = text.to_string();
        self.started = Some(now);
        true
    }

    /// Eased 0→1 progress for the current roll. 1 means no motion.
    pub fn progress(&self, now: Instant) -> f32 {
        let Some(started) = self.started else { return 1.0 };
        let t = (now.duration_since(started).as_secs_f32()
            / Self::DURATION.as_secs_f32()).clamp(0.0, 1.0);
        // cubic ease-out, matching DbCurve.standard's soft landing
        1.0 - (1.0 - t).powi(3)
    }

    pub fn is_changed(&self, index: usize) -> bool {
        self.changed.get(index).copied().unwrap_or(false)
    }

    pub fn direction(&self, index: usize) -> i8 {
        self.direction.get(index).copied().unwrap_or(1)
    }
}

pub struct ClockService;

impl ClockService {
    /// Returns the current local time.
    pub fn now() -> DateTime<Local> {
        Local::now()
    }

    /// "HH:MM" in 24-hour mode, or "h:MM" (no leading zero) in 12-hour mode.
    pub fn format_time(now: &DateTime<Local>, use_24h: bool) -> String {
        if use_24h {
            format!("{:02}:{:02}", now.hour(), now.minute())
        } else {
            let h = now.hour12().1; // 1-based, no leading zero needed
            format!("{}:{:02}", h, now.minute())
        }
    }

    /// ":SS" — always two digits with a leading colon.
    pub fn format_seconds(now: &DateTime<Local>) -> String {
        format!(":{:02}", now.second())
    }

    /// "AM" or "PM".
    pub fn format_meridiem(now: &DateTime<Local>) -> String {
        if now.hour() < 12 {
            "AM".to_string()
        } else {
            "PM".to_string()
        }
    }

    /// "1 Jan", "14 Feb" — day without a leading zero, abbreviated month.
    pub fn format_short_date(now: &DateTime<Local>) -> String {
        let month = short_month(now.month());
        format!("{} {}", now.day(), month)
    }

    /// "Tuesday, 8 September" — full weekday, day without leading zero, full month.
    pub fn format_long_date(now: &DateTime<Local>) -> String {
        let weekday = long_weekday(now.weekday());
        let month = long_month(now.month());
        format!("{}, {} {}", weekday, now.day(), month)
    }

    /// ISO 8601 week number (1–53).
    pub fn iso_week(now: &DateTime<Local>) -> u32 {
        now.iso_week().week()
    }

    /// Stopwatch display: "MM:SS.hh" where hh is hundredths of a second.
    /// Example: 194.263 s → "03:14.26"
    pub fn format_stopwatch(d: Duration) -> String {
        let total_ms = d.as_millis();
        let centis = (total_ms / 10) % 100;
        let secs = (total_ms / 1000) % 60;
        let mins = total_ms / 60_000;
        format!("{:02}:{:02}.{:02}", mins, secs, centis)
    }

    /// Countdown display: "M:SS" for < 10 minutes, "MM:SS" for ≥ 10 minutes.
    /// Example: 300 s → "5:00", 863 s → "14:23"
    pub fn format_countdown(d: Duration) -> String {
        let total_secs = d.as_secs();
        let secs = total_secs % 60;
        let mins = total_secs / 60;
        if mins < 10 {
            format!("{}:{:02}", mins, secs)
        } else {
            format!("{:02}:{:02}", mins, secs)
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn short_month(m: u32) -> &'static str {
    match m {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "",
    }
}

fn long_month(m: u32) -> &'static str {
    match m {
        1 => "January",   2 => "February", 3 => "March",    4 => "April",
        5 => "May",       6 => "June",     7 => "July",     8 => "August",
        9 => "September", 10 => "October", 11 => "November",12 => "December",
        _ => "",
    }
}

fn long_weekday(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Monday",    Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday", Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",    Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stopwatch_format() {
        // 3 minutes, 14 seconds, 260 ms → "03:14.26"
        let d = Duration::from_millis(3 * 60_000 + 14_000 + 260);
        assert_eq!(ClockService::format_stopwatch(d), "03:14.26");
    }

    #[test]
    fn digit_roll_only_marks_changed_digits() {
        let mut roll = DigitRoll::default();
        roll.seed("09:59");
        let t = Instant::now();
        assert!(roll.update("10:00", t));
        // "09:59" → "10:00": first two digits and the last change; colon does not.
        assert!(roll.is_changed(0));
        assert!(roll.is_changed(1));
        assert!(!roll.is_changed(2));
        assert!(roll.is_changed(3));
        assert!(roll.direction(0) > 0);
        assert!(roll.direction(3) < 0);
        assert!(roll.progress(t) < 0.1);
        assert_eq!(roll.progress(t + Duration::from_secs(1)), 1.0);
    }

    #[test]
    fn countdown_short() {
        assert_eq!(ClockService::format_countdown(Duration::from_secs(300)), "5:00");
    }

    #[test]
    fn countdown_long() {
        assert_eq!(ClockService::format_countdown(Duration::from_secs(863)), "14:23");
    }
}
