// activities.rs — port of Flutter's LiveActivity / ActivityCenter.
//
// Activities are derived views over the owning modules (privacy, timers,
// downloads, media, battery). The collapsed face shows the two highest
// priority ones as "ears" beside the clock, exactly like `collapsed_face.dart`.

use iced::Color;

/// Ordered by urgency, matching `LiveActivity.priority` in Flutter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Recording,
    Call,
    Timer,
    Download,
    Focus,
    Battery,
    Media,
}

impl ActivityKind {
    pub fn priority(self) -> u8 {
        match self {
            Self::Recording => 0,
            Self::Call => 1,
            Self::Timer => 2,
            Self::Download => 3,
            Self::Focus => 4,
            Self::Battery => 5,
            Self::Media => 6,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LiveActivity {
    pub id: String,
    pub kind: ActivityKind,
    /// Single-character glyph shown in the collapsed ear.
    pub icon: char,
    /// Short value shown next to the glyph (e.g. `04:32`, `1.2 MB/s`).
    pub label: String,
    /// Optional 0..1 progress, drawn as a hairline ring around the glyph.
    pub progress: Option<f32>,
    pub tint: Option<Color>,
    pub urgent: bool,
}

impl LiveActivity {
    pub fn priority(&self) -> u8 {
        self.kind.priority()
    }
}

/// Number of activities that fit beside the collapsed clock.
pub const MAX_VISIBLE: usize = 2;

/// Sort by urgency and keep the two most important activities.
pub fn visible(mut activities: Vec<LiveActivity>) -> Vec<LiveActivity> {
    activities.sort_by_key(|a| (a.priority(), a.id.clone()));
    activities.truncate(MAX_VISIBLE);
    activities
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity(id: &str, kind: ActivityKind) -> LiveActivity {
        LiveActivity {
            id: id.into(),
            kind,
            icon: '●',
            label: id.into(),
            progress: None,
            tint: None,
            urgent: false,
        }
    }

    #[test]
    fn recording_outranks_media_and_only_two_are_visible() {
        let items = visible(vec![
            activity("song", ActivityKind::Media),
            activity("timer", ActivityKind::Timer),
            activity("mic", ActivityKind::Recording),
            activity("dl", ActivityKind::Download),
        ]);
        let ids: Vec<&str> = items.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["mic", "timer"]);
    }

    #[test]
    fn stable_order_within_a_kind() {
        let items = visible(vec![
            activity("b", ActivityKind::Timer),
            activity("a", ActivityKind::Timer),
        ]);
        assert_eq!(items[0].id, "a");
    }
}
