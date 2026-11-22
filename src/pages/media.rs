//! Media page — now-playing card with art placeholder, seek bar, controls.
//! Pure canvas; no iced widget tree.

use iced::{
    alignment,
    widget::canvas::{self, Frame, Fill, Text},
    Color, Point,
};

use crate::{
    accent, SURFACE_RAISED, SURFACE_SUNKEN,
    TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TERTIARY,
};

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct MediaState {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app_name: String,
    pub is_playing: bool,
    /// Playback position in seconds.
    pub position_secs: f32,
    /// Total duration; 0 means unknown / streaming.
    pub duration_secs: f32,
    /// Dominant RGB sampled from album art, used to tint the placeholder.
    pub art_color: Option<[f32; 3]>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn draw_media_page(
    frame: &mut Frame,
    w: f32,
    h: f32,
    opacity: f32,
    state: Option<&MediaState>,
) {
    if opacity < 0.004 {
        return;
    }

    match state {
        None => draw_nothing_playing(frame, w, h, opacity),
        Some(s) => draw_playing(frame, w, h, opacity, s),
    }
}

// ── Nothing-playing state ─────────────────────────────────────────────────────

fn draw_nothing_playing(frame: &mut Frame, w: f32, h: f32, opacity: f32) {
    frame.fill_text(Text {
        content: "Nothing playing".to_string(),
        position: Point::new(w / 2.0, h / 2.0),
        color: Color { a: opacity * 0.55, ..TEXT_SECONDARY },
        size: iced::Pixels(13.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
}

// ── Playing state ─────────────────────────────────────────────────────────────

fn draw_playing(frame: &mut Frame, w: f32, h: f32, opacity: f32, state: &MediaState) {
    let pad_x = 16.0_f32;
    let pad_y = 20.0_f32;

    // ── Art placeholder ───────────────────────────────────────────────────────
    let art_size = 72.0_f32;
    let art_x = pad_x;
    let art_y = pad_y;

    let art_path = rounded_rect(art_x, art_y, art_size, art_size, 14.0);
    frame.fill(&art_path, Fill::from(Color { a: opacity, ..SURFACE_RAISED }));

    // Optional art tint at 20% alpha
    if let Some([r, g, b]) = state.art_color {
        let tint = Color { r, g, b, a: opacity * 0.20 };
        frame.fill(&art_path, Fill::from(tint));
    }

    // Musical note centred in the placeholder
    frame.fill_text(Text {
        content: "♪".to_string(),
        position: Point::new(art_x + art_size / 2.0, art_y + art_size / 2.0),
        color: Color { a: opacity * 0.5, ..TEXT_TERTIARY },
        size: iced::Pixels(28.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });

    // ── Metadata ──────────────────────────────────────────────────────────────
    let meta_x = art_x + art_size + 14.0;
    let meta_right = w - pad_x;
    let meta_w = (meta_right - meta_x).max(0.0);

    // Title
    frame.fill_text(Text {
        content: truncate_str(&state.title, (meta_w / 7.0) as usize),
        position: Point::new(meta_x, art_y + 4.0),
        color: Color { a: opacity, ..TEXT_PRIMARY },
        size: iced::Pixels(14.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    // Artist
    frame.fill_text(Text {
        content: truncate_str(&state.artist, (meta_w / 6.5) as usize),
        position: Point::new(meta_x, art_y + 22.0),
        color: Color { a: opacity * 0.80, ..TEXT_SECONDARY },
        size: iced::Pixels(12.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    // Album · App
    let album_app = if state.album.is_empty() {
        truncate_str(&state.app_name, (meta_w / 6.0) as usize)
    } else {
        let combined = format!("{} · {}", state.album, state.app_name);
        truncate_str(&combined, (meta_w / 6.0) as usize)
    };
    frame.fill_text(Text {
        content: album_app,
        position: Point::new(meta_x, art_y + 37.0),
        color: Color { a: opacity * 0.55, ..TEXT_TERTIARY },
        size: iced::Pixels(10.5),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    // ── Seek bar ──────────────────────────────────────────────────────────────
    let seek_y = art_y + 54.0;
    let seek_x = meta_x;
    let seek_w = meta_right - meta_x;
    let seek_h = 3.0_f32;

    if state.duration_secs > 0.0 {
        let progress = (state.position_secs / state.duration_secs).clamp(0.0, 1.0);

        // Track
        let track = rounded_rect(seek_x, seek_y, seek_w, seek_h, 1.5);
        frame.fill(&track, Fill::from(Color { a: opacity * 0.35, ..SURFACE_SUNKEN }));

        // Fill
        let fill_w = (seek_w * progress).max(seek_h); // at least as wide as height for round cap
        let fill = rounded_rect(seek_x, seek_y, fill_w, seek_h, 1.5);
        frame.fill(&fill, Fill::from(Color { a: opacity * 0.9, ..accent() }));

        // Grab handle: makes the seek target discoverable on the small canvas.
        frame.fill(
            &canvas::Path::circle(
                Point::new(seek_x + seek_w * progress, seek_y + seek_h / 2.0),
                3.0,
            ),
            Fill::from(Color { a: opacity, ..accent() }),
        );

        // Time labels
        let pos_str = format_time(state.position_secs);
        let dur_str = format_time(state.duration_secs);
        frame.fill_text(Text {
            content: pos_str,
            position: Point::new(seek_x, seek_y + 6.0),
            color: Color { a: opacity * 0.50, ..TEXT_TERTIARY },
            size: iced::Pixels(9.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
        frame.fill_text(Text {
            content: dur_str,
            position: Point::new(seek_x + seek_w, seek_y + 6.0),
            color: Color { a: opacity * 0.50, ..TEXT_TERTIARY },
            size: iced::Pixels(9.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Right.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }

    // ── Controls ──────────────────────────────────────────────────────────────
    // Three controls horizontally centred in the metadata column.
    let ctrl_y = art_y + 66.0;
    let play_pause = if state.is_playing { "⏸" } else { "▶" };
    let controls = ["◀◀", play_pause, "▶▶"];
    let ctrl_spacing = meta_w / 4.0;

    for (i, &glyph) in controls.iter().enumerate() {
        let cx = meta_x + ctrl_spacing * (i as f32 + 1.0);
        frame.fill_text(Text {
            content: glyph.to_string(),
            position: Point::new(cx, ctrl_y),
            color: Color { a: opacity * 0.90, ..TEXT_PRIMARY },
            size: iced::Pixels(16.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> canvas::Path {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    canvas::Path::new(|b| {
        b.move_to(Point::new(x + r, y));
        b.line_to(Point::new(x + w - r, y));
        b.arc_to(Point::new(x + w, y), Point::new(x + w, y + r), r);
        b.line_to(Point::new(x + w, y + h - r));
        b.arc_to(Point::new(x + w, y + h), Point::new(x + w - r, y + h), r);
        b.line_to(Point::new(x + r, y + h));
        b.arc_to(Point::new(x, y + h), Point::new(x, y + h - r), r);
        b.line_to(Point::new(x, y + r));
        b.arc_to(Point::new(x, y), Point::new(x + r, y), r);
        b.close();
    })
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", t)
    }
}

fn format_time(secs: f32) -> String {
    let total = secs as u32;
    let m = total / 60;
    let s = total % 60;
    format!("{}:{:02}", m, s)
}
