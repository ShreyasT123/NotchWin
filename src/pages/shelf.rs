//! Shelf page — two image-drop slots.
//! Pure canvas; no iced widget tree.

use std::path::PathBuf;

use iced::{
    alignment,
    widget::canvas::{self, Frame, Fill, Stroke, Style, Text, LineCap},
    Color, Point,
};

use crate::{
    STROKE, SURFACE_RAISED,
    TEXT_SECONDARY, TEXT_TERTIARY,
};

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ShelfState {
    /// Filename displayed in slot A, `None` = slot is empty.
    pub slot_a_label: Option<String>,
    /// Filename displayed in slot B, `None` = slot is empty.
    pub slot_b_label: Option<String>,
    /// Full path used by copy/open actions for slot A.
    pub slot_a_path: Option<String>,
    /// Full path used by copy/open actions for slot B.
    pub slot_b_path: Option<String>,
}

impl ShelfState {
    fn store_path() -> PathBuf {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| PathBuf::from("."))
            });
        base.join("notchWin").join("shelf.json")
    }

    /// Load persisted slots from `%APPDATA%\notchWin\shelf.json`.
    pub fn load() -> Self {
        std::fs::read_to_string(Self::store_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Persist the current slots. Failures are intentionally non-fatal.
    pub fn save(&self) {
        let path = Self::store_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn draw_shelf_page(
    frame: &mut Frame,
    w: f32,
    h: f32,
    opacity: f32,
    state: &ShelfState,
) {
    if opacity < 0.004 {
        return;
    }

    let pad_x = 14.0_f32;
    let pad_y = 12.0_f32;
    let gap = 10.0_f32;

    // ── Image slots ───────────────────────────────────────────────────────
    // Leave a small strip under the slots for the Copy/Open/Clear captions.
    let labels_h = 14.0_f32;
    let slots_h = (h - pad_y * 2.0 - labels_h).max(0.0);
    let slot_w = (w - pad_x * 2.0 - gap) / 2.0;
    let slot_corner = 14.0_f32;

    draw_slot(
        frame,
        pad_x,
        pad_y,
        slot_w,
        slots_h,
        slot_corner,
        state.slot_a_label.as_deref(),
        opacity,
    );

    draw_slot(
        frame,
        pad_x + slot_w + gap,
        pad_y,
        slot_w,
        slots_h,
        slot_corner,
        state.slot_b_label.as_deref(),
        opacity,
    );

}

// ── Slot painter ──────────────────────────────────────────────────────────────

fn draw_slot(
    frame: &mut Frame,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    corner: f32,
    label: Option<&str>,
    opacity: f32,
) {
    let path = rounded_rect(x, y, w, h, corner);

    if let Some(filename) = label {
        // Occupied: solid SURFACE_RAISED background
        frame.fill(&path, Fill::from(Color { a: opacity, ..SURFACE_RAISED }));
        frame.stroke(
            &path,
            Stroke {
                style: Style::Solid(Color { a: opacity * 0.25, ..STROKE }),
                width: 0.75,
                ..Stroke::default()
            },
        );

        // Filename centred (truncated to 18 chars)
        let display = truncate_str(filename, 18);
        frame.fill_text(Text {
            content: display,
            position: Point::new(x + w / 2.0, y + h / 2.0),
            color: Color { a: opacity * 0.75, ..TEXT_SECONDARY },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
    } else {
        // Empty: lighter fill with dashed stroke
        frame.fill(&path, Fill::from(Color { a: opacity * 0.25, ..SURFACE_RAISED }));
        draw_dashed_border(frame, x, y, w, h, corner, opacity);

        // "Drop here" label
        frame.fill_text(Text {
            content: "Drop here".to_string(),
            position: Point::new(x + w / 2.0, y + h / 2.0 - 6.0),
            color: Color { a: opacity * 0.40, ..TEXT_TERTIARY },
            size: iced::Pixels(12.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });

        // Downward-arrow indicator
        frame.fill_text(Text {
            content: "↓".to_string(),
            position: Point::new(x + w / 2.0, y + h / 2.0 + 10.0),
            color: Color { a: opacity * 0.30, ..TEXT_TERTIARY },
            size: iced::Pixels(14.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
    }

    // Action labels below the slot
    let action_y = y + h + 4.0;
    frame.fill_text(Text {
        content: "Copy · Open · Clear".to_string(),
        position: Point::new(x + w / 2.0, action_y),
        color: Color { a: opacity * 0.35, ..TEXT_TERTIARY },
        size: iced::Pixels(9.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
}

// ── Dashed border ─────────────────────────────────────────────────────────────

/// Approximates a dashed border by drawing short line segments around the slot.
fn draw_dashed_border(
    frame: &mut Frame,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    opacity: f32,
) {
    let dash_len = 5.0_f32;
    let gap_len  = 4.0_f32;

    // We only draw the four straight edges; for simplicity the corners are left
    // as gaps (this matches a typical dashed-border look well enough in this
    // small canvas context).
    let stroke = Stroke {
        style: Style::Solid(Color { a: opacity * 0.40, ..STROKE }),
        width: 0.75,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };

    // Top edge
    draw_dashed_line(frame, x + r, y, x + w - r, y, dash_len, gap_len, &stroke);
    // Bottom edge
    draw_dashed_line(frame, x + r, y + h, x + w - r, y + h, dash_len, gap_len, &stroke);
    // Left edge
    draw_dashed_line(frame, x, y + r, x, y + h - r, dash_len, gap_len, &stroke);
    // Right edge
    draw_dashed_line(frame, x + w, y + r, x + w, y + h - r, dash_len, gap_len, &stroke);
}

fn draw_dashed_line(
    frame: &mut Frame,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    dash: f32,
    gap: f32,
    stroke: &Stroke,
) {
    let total = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
    if total < 1.0 {
        return;
    }
    let dx = (x2 - x1) / total;
    let dy = (y2 - y1) / total;

    let mut pos = 0.0_f32;
    let mut drawing = true;

    while pos < total {
        let seg_len = if drawing { dash } else { gap };
        let end_pos = (pos + seg_len).min(total);

        if drawing {
            let sx = x1 + dx * pos;
            let sy = y1 + dy * pos;
            let ex = x1 + dx * end_pos;
            let ey = y1 + dy * end_pos;
            let seg = canvas::Path::line(Point::new(sx, sy), Point::new(ex, ey));
            frame.stroke(&seg, stroke.clone());
        }

        pos = end_pos;
        drawing = !drawing;
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
