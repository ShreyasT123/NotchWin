//! Local calendar event source.
//!
//! The Flutter original shows events supplied by a platform calendar bridge.
//! Keeping portability (and avoiding WinRT package-identity requirements), the
//! Rust port reads a small JSON file from `%APPDATA%\notchWin\calendar.json`:
//!
//! ```json
//! [
//!   { "date": "2026-11-22", "time": "09:30", "title": "Stand-up" },
//!   { "date": "2026-11-22", "time": "", "title": "Rent due", "notified": true }
//! ]
//! ```
//!
//! An empty template is created on first run so the file is discoverable.
//! Reload is cheap and happens whenever the user changes the selected day or
//! month, so editing the file is picked up without restarting the app.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// ISO date, `YYYY-MM-DD`.
    pub date: String,
    /// Optional `HH:MM` label. Empty means an all-day entry.
    #[serde(default)]
    pub time: String,
    /// Text shown in the day panel.
    pub title: String,
    /// Draws the small "already notified" dot.
    #[serde(default)]
    pub notified: bool,
}

pub struct CalendarSource {
    path: PathBuf,
    events: Vec<CalendarEvent>,
}

impl CalendarSource {
    fn source_path() -> PathBuf {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| PathBuf::from("."))
            });
        base.join("notchWin").join("calendar.json")
    }

    /// Load the event list. A missing or malformed file yields an empty list;
    /// a missing file is replaced with an empty template on disk.
    pub fn load() -> Self {
        let path = Self::source_path();
        let events = Self::read_from(&path).unwrap_or_default();
        if !path.exists() {
            let _ = Self::write_template(&path);
        }
        Self { path, events }
    }

    /// Re-read the JSON file; used when navigating the calendar so edits made
    /// in an external editor become visible immediately.
    pub fn reload(&mut self) {
        self.events = Self::read_from(&self.path).unwrap_or_default();
    }

    /// Events for a single day, sorted with timed entries before all-day ones
    /// and then by their time label.
    pub fn events_for(&self, year: i32, month: u32, day: u32) -> Vec<CalendarEvent> {
        let key = format!("{year:04}-{month:02}-{day:02}");
        let mut out: Vec<CalendarEvent> = self
            .events
            .iter()
            .filter(|e| e.date == key)
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            match (a.time.is_empty(), b.time.is_empty()) {
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
                _ => a.time.cmp(&b.time).then_with(|| a.title.cmp(&b.title)),
            }
        });
        out
    }

    /// Populate the JSON file with a few events around today. Useful for a
    /// first-run demo and harmless to overwrite from the Settings backup page.
    pub fn write_samples(&mut self) -> bool {
        use chrono::{Datelike, Duration as ChronoDuration, Local};

        let today = Local::now().date_naive();
        let tomorrow = today + ChronoDuration::days(1);
        let key = |d: chrono::NaiveDate| -> String {
            format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
        };
        let samples = vec![
            CalendarEvent {
                date: key(today),
                time: "09:30".into(),
                title: "Team stand-up".into(),
                notified: false,
            },
            CalendarEvent {
                date: key(today),
                time: "13:00".into(),
                title: "Focus block".into(),
                notified: false,
            },
            CalendarEvent {
                date: key(today),
                time: "".into(),
                title: "Submit expense report".into(),
                notified: false,
            },
            CalendarEvent {
                date: key(tomorrow),
                time: "18:30".into(),
                title: "Dinner with Sam".into(),
                notified: false,
            },
        ];
        let json = match serde_json::to_string_pretty(&samples) {
            Ok(j) => j,
            Err(_) => return false,
        };
        if let Some(parent) = self.path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return false;
            }
        }
        if std::fs::write(&self.path, json).is_err() {
            return false;
        }
        self.events = samples;
        true
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn read_from(path: &PathBuf) -> Option<Vec<CalendarEvent>> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_template(path: &PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, "[]\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_by_day_and_sorted_timed_entries_first() {
        let source = CalendarSource {
            path: PathBuf::from("unused.json"),
            events: vec![
                CalendarEvent { date: "2026-11-22".into(), time: "15:00".into(), title: "Late".into(), notified: false },
                CalendarEvent { date: "2026-11-23".into(), time: "09:00".into(), title: "Tomorrow".into(), notified: false },
                CalendarEvent { date: "2026-11-22".into(), time: "".into(), title: "All day".into(), notified: true },
                CalendarEvent { date: "2026-11-22".into(), time: "09:30".into(), title: "Stand-up".into(), notified: false },
            ],
        };
        let found = source.events_for(2026, 11, 22);
        let titles: Vec<&str> = found.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Stand-up", "Late", "All day"]);
        assert!(found[2].notified);
    }

    #[test]
    fn parses_optional_event_fields() {
        let parsed: CalendarEvent = serde_json::from_str(
            r#"{"date":"2026-11-22","title":"Rent due"}"#,
        )
        .expect("parse event");
        assert_eq!(parsed.time, "");
        assert!(!parsed.notified);
    }
}
