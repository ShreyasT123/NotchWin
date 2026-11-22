use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

// ── types ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum NotificationKind {
    Info,
    Success,
    Warning,
    Error,
    Media,
    Download,
    Clipboard,
    Timer,
    System,
}

#[derive(Clone, Debug)]
pub struct IslandNotification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub kind: NotificationKind,
    /// Optional progress bar value in 0.0–1.0 (e.g. download percentage).
    pub progress: Option<f32>,
    /// Optional RGBA accent colour override [r, g, b, a] each in 0.0–1.0.
    pub accent: Option<[f32; 4]>,
}

// ── center ────────────────────────────────────────────────────────────────────

pub struct NotificationCenter {
    queue: VecDeque<IslandNotification>,
    dismissed: HashSet<String>,
    /// Id -> wall-clock deadline for auto-dismissing transient notices such as
    /// the volume HUD. Kept separate from the notification model so callers
    /// that post plain in-app messages do not need to supply timestamps.
    expiry: HashMap<String, Instant>,
}

impl NotificationCenter {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            dismissed: HashSet::new(),
            expiry: HashMap::new(),
        }
    }

    /// Post a notification. If a notification with the same `id` already exists
    /// in the queue, it is replaced in-place (preserving its queue position) and
    /// the dismissed set is NOT updated, so a re-post of a dismissed id will
    /// re-add it to the back. Otherwise the notification is appended.
    pub fn post(&mut self, n: IslandNotification) {
        // Skip IDs the user already dismissed in this session.
        if self.dismissed.contains(&n.id) {
            // Allow re-posting by clearing the dismissed record.
            self.dismissed.remove(&n.id);
        }

        // Replace in-place if the id is already queued.
        if let Some(existing) = self.queue.iter_mut().find(|x| x.id == n.id) {
            *existing = n;
        } else {
            self.queue.push_back(n);
        }
    }

    /// Post a notice that disappears automatically after `lifetime`. Re-posting
    /// the same id refreshes both the content and the deadline.
    pub fn post_timed(&mut self, n: IslandNotification, lifetime: Duration) {
        let id = n.id.clone();
        self.post(n);
        self.expiry.insert(id, Instant::now() + lifetime);
    }

    /// Remove notices whose lifetime elapsed. Returns true if the visible face
    /// may have changed and the canvas cache should be cleared.
    pub fn expire(&mut self, now: Instant) -> bool {
        let expired: Vec<String> = self
            .expiry
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(id, _)| id.clone())
            .collect();
        if expired.is_empty() {
            return false;
        }
        for id in expired {
            self.expiry.remove(&id);
            self.queue.retain(|n| n.id != id);
        }
        true
    }

    /// The notification currently shown (front of the queue), if any.
    pub fn current(&self) -> Option<&IslandNotification> {
        self.queue.front()
    }

    /// Dismiss the current notification (pop from front) and record its id so
    /// subsequent duplicate posts are handled correctly.
    pub fn dismiss_current(&mut self) {
        if let Some(n) = self.queue.pop_front() {
            self.expiry.remove(&n.id);
            self.dismissed.insert(n.id);
        }
    }

    /// Number of notifications waiting behind the current one.
    pub fn pending_count(&self) -> usize {
        self.queue.len().saturating_sub(1)
    }

    /// Whether the queue is entirely empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Total number of notifications in the queue (including the current one).
    pub fn total(&self) -> usize {
        self.queue.len()
    }
}

impl Default for NotificationCenter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(id: &str, title: &str) -> IslandNotification {
        IslandNotification {
            id: id.to_string(),
            title: title.to_string(),
            body: String::new(),
            kind: NotificationKind::Info,
            progress: None,
            accent: None,
        }
    }

    #[test]
    fn post_and_current() {
        let mut center = NotificationCenter::new();
        center.post(make("a", "First"));
        center.post(make("b", "Second"));

        assert_eq!(center.current().unwrap().id, "a");
        assert_eq!(center.pending_count(), 1);
    }

    #[test]
    fn post_replaces_in_place() {
        let mut center = NotificationCenter::new();
        center.post(make("a", "Old title"));
        center.post(make("b", "B"));
        center.post(make("a", "New title")); // should update "a" in place

        // "a" is still at the front with the updated title
        assert_eq!(center.current().unwrap().title, "New title");
        assert_eq!(center.total(), 2); // no duplicate
    }

    #[test]
    fn dismiss_advances_queue() {
        let mut center = NotificationCenter::new();
        center.post(make("a", "A"));
        center.post(make("b", "B"));

        center.dismiss_current();
        assert_eq!(center.current().unwrap().id, "b");
        assert_eq!(center.pending_count(), 0);
    }

    #[test]
    fn pending_count_empty() {
        let center = NotificationCenter::new();
        assert_eq!(center.pending_count(), 0);
    }

    #[test]
    fn timed_notice_expires() {
        let mut center = NotificationCenter::new();
        let start = Instant::now();
        center.post_timed(make("system.volume", "Volume 40%"), Duration::from_millis(100));
        assert!(center.current().is_some());
        assert!(!center.expire(start + Duration::from_millis(50)));
        assert!(center.expire(start + Duration::from_millis(150)));
        assert!(center.is_empty());
    }
}
