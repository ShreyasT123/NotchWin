//! Calendar page — month grid + day detail panel.
//! Pure canvas; no iced widget tree.

use iced::{
    alignment,
    widget::canvas::{self, Frame, Fill, Stroke, Style, Text},
    Color, Point,
};

use crate::{
    accent, STROKE, SURFACE_RAISED,
    TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TERTIARY,
};

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CalendarState {
    pub year: i32,
    pub month: u32,         // 1-12
    pub selected_day: u32,
    pub today_day: u32,
    pub today_month: u32,
    pub today_year: i32,
    pub entries: Vec<CalEntry>,
}

#[derive(Clone)]
pub struct CalEntry {
    pub time_label: String, // "" for untimed
    pub text: String,
    pub notified: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn draw_calendar_page(
    frame: &mut Frame,
    w: f32,
    h: f32,
    opacity: f32,
    state: &CalendarState,
) {
    if opacity < 0.004 {
        return;
    }

    let grid_w = w * 0.44;
    let panel_x = grid_w + 10.0;
    let panel_w = w - panel_x - 8.0;

    draw_month_grid(frame, 0.0, 0.0, grid_w, h, opacity, state);
    draw_day_panel(frame, panel_x, 0.0, panel_w, h, opacity, state);
}

// ── Month grid ────────────────────────────────────────────────────────────────

fn draw_month_grid(
    frame: &mut Frame,
    gx: f32,
    _gy: f32,
    gw: f32,
    _gh: f32,
    opacity: f32,
    state: &CalendarState,
) {
    let pad_x = 6.0_f32;
    let header_y = 10.0_f32;

    // Month + year header
    let month_name = month_name(state.month);
    let header = format!("{} {}", month_name, state.year);
    frame.fill_text(Text {
        content: header,
        position: Point::new(gx + gw / 2.0, header_y),
        color: Color { a: opacity, ..TEXT_PRIMARY },
        size: iced::Pixels(13.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    // Month navigation chevrons. Their hit boxes are mirrored in
    // `calendar_month_hit` in main.rs.
    for (label, x) in [("‹", gx + 12.0), ("›", gx + gw - 12.0)] {
        frame.fill_text(Text {
            content: label.into(),
            position: Point::new(x, header_y),
            color: Color { a: opacity * 0.65, ..TEXT_SECONDARY },
            size: iced::Pixels(15.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }

    // Weekday labels Mon-Sun
    let day_labels = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    let cell_w = (gw - pad_x * 2.0) / 7.0;
    let labels_y = header_y + 18.0;

    for (i, &lbl) in day_labels.iter().enumerate() {
        let lx = gx + pad_x + i as f32 * cell_w + cell_w / 2.0;
        frame.fill_text(Text {
            content: lbl.to_string(),
            position: Point::new(lx, labels_y),
            color: Color { a: opacity * 0.55, ..TEXT_TERTIARY },
            size: iced::Pixels(10.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }

    // Divider
    let div_y = labels_y + 14.0;
    let div = canvas::Path::line(
        Point::new(gx + pad_x, div_y),
        Point::new(gx + gw - pad_x, div_y),
    );
    frame.stroke(
        &div,
        Stroke {
            style: Style::Solid(Color { a: opacity * 0.20, ..STROKE }),
            width: 0.5,
            ..Stroke::default()
        },
    );

    // Day cells
    // Determine the weekday of the first of the month (Mon=0, Sun=6)
    let first_weekday = weekday_of_first(state.year, state.month); // 0=Mon, 6=Sun
    let days_in_month = days_in_month(state.year, state.month);

    let cells_top = div_y + 4.0;
    let cell_h = 20.0_f32;

    for day in 1..=days_in_month {
        let cell_index = (first_weekday + day as usize - 1) % 7
            + (first_weekday + day as usize - 1) / 7 * 7;
        let col = (first_weekday + day as usize - 1) % 7;
        let row = (first_weekday + day as usize - 1) / 7;

        let cx = gx + pad_x + col as f32 * cell_w + cell_w / 2.0;
        let cy = cells_top + row as f32 * cell_h + cell_h / 2.0;

        let is_today = day == state.today_day
            && state.month == state.today_month
            && state.year == state.today_year;
        let is_selected = day == state.selected_day;

        // Today: accent circle
        if is_today {
            frame.fill(
                &canvas::Path::circle(Point::new(cx, cy), 9.0),
                Fill::from(Color { a: opacity * 0.80, ..accent() }),
            );
        }

        // Selected (and not today): surface raised rect
        if is_selected && !is_today {
            let sel_path = rounded_rect(cx - 9.0, cy - 9.0, 18.0, 18.0, 5.0);
            frame.fill(&sel_path, Fill::from(Color { a: opacity * 0.35, ..SURFACE_RAISED }));
        }

        // Day number colour: today gets surface/dark, past gets tertiary, rest primary
        let is_past = {
            let today_ord = date_ord(state.today_year, state.today_month, state.today_day);
            let this_ord  = date_ord(state.year, state.month, day);
            this_ord < today_ord
        };

        let text_color = if is_today {
            Color { r: 0.05, g: 0.05, b: 0.07, a: opacity * 0.95 }
        } else if is_past {
            Color { a: opacity * 0.40, ..TEXT_TERTIARY }
        } else {
            Color { a: opacity * 0.90, ..TEXT_PRIMARY }
        };

        frame.fill_text(Text {
            content: day.to_string(),
            position: Point::new(cx, cy),
            color: text_color,
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });

        let _ = cell_index; // suppress unused warning
    }
}

// ── Day panel ─────────────────────────────────────────────────────────────────

fn draw_day_panel(
    frame: &mut Frame,
    px: f32,
    _py: f32,
    pw: f32,
    _ph: f32,
    opacity: f32,
    state: &CalendarState,
) {
    // Vertical divider separating grid from panel
    let div = canvas::Path::line(Point::new(px - 5.0, 8.0), Point::new(px - 5.0, _ph - 8.0));
    frame.stroke(
        &div,
        Stroke {
            style: Style::Solid(Color { a: opacity * 0.18, ..STROKE }),
            width: 0.5,
            ..Stroke::default()
        },
    );

    // "8 SEPTEMBER" header
    let month_name = month_name(state.month).to_uppercase();
    let day_header = format!("{} {}", state.selected_day, month_name);
    frame.fill_text(Text {
        content: day_header,
        position: Point::new(px + 4.0, 10.0),
        color: Color { a: opacity * 0.90, ..TEXT_PRIMARY },
        size: iced::Pixels(12.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    if state.entries.is_empty() {
        frame.fill_text(Text {
            content: "Nothing on this day".to_string(),
            position: Point::new(px + pw / 2.0, 50.0),
            color: Color { a: opacity * 0.40, ..TEXT_TERTIARY },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
        frame.fill_text(Text {
            content: "Settings → Backup → Sample calendar".to_string(),
            position: Point::new(px + pw / 2.0, 68.0),
            color: Color { a: opacity * 0.28, ..TEXT_TERTIARY },
            size: iced::Pixels(8.5),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
        return;
    }

    let entry_top = 28.0_f32;
    let row_h = 22.0_f32;

    for (i, entry) in state.entries.iter().take(6).enumerate() {
        let ey = entry_top + i as f32 * row_h;

        // Time label (accent, tabular)
        if !entry.time_label.is_empty() {
            frame.fill_text(Text {
                content: entry.time_label.clone(),
                position: Point::new(px + 4.0, ey),
                color: Color { a: opacity * 0.80, ..accent() },
                size: iced::Pixels(10.0),
                font: iced::Font::DEFAULT,
                align_x: alignment::Horizontal::Left.into(),
                align_y: alignment::Vertical::Top,
                ..Text::default()
            });
        }

        // Event text
        let text_x = if entry.time_label.is_empty() { px + 4.0 } else { px + 38.0 };
        let max_chars = ((pw - 42.0) / 6.2) as usize;
        frame.fill_text(Text {
            content: truncate_str(&entry.text, max_chars),
            position: Point::new(text_x, ey),
            color: Color {
                a: opacity * if entry.notified { 0.65 } else { 0.85 },
                ..TEXT_SECONDARY
            },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });

        // Notified dot
        if entry.notified {
            frame.fill(
                &canvas::Path::circle(Point::new(px + pw - 8.0, ey + 5.0), 2.5),
                Fill::from(Color { a: opacity * 0.65, ..accent() }),
            );
        }
    }
}

// ── Calendar arithmetic ───────────────────────────────────────────────────────

/// Returns the ISO weekday of the 1st of the given month: 0 = Monday, 6 = Sunday.
pub(crate) fn weekday_of_first(year: i32, month: u32) -> usize {
    // Using chrono for correctness.
    use chrono::{Datelike, NaiveDate};
    let d = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default();
    // chrono Weekday: Mon=0 … Sun=6 via num_days_from_monday
    d.weekday().num_days_from_monday() as usize
}

pub(crate) fn days_in_month(year: i32, month: u32) -> u32 {
    use chrono::NaiveDate;
    // The first day of the next month minus one day gives the last day of this month.
    let (y2, m2) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let next_first = NaiveDate::from_ymd_opt(y2, m2, 1).unwrap_or_default();
    let this_first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default();
    (next_first - this_first).num_days() as u32
}

/// Simple ordinal for date comparison (not Julian; just for before/after).
fn date_ord(year: i32, month: u32, day: u32) -> i64 {
    year as i64 * 10000 + month as i64 * 100 + day as i64
}

fn month_name(month: u32) -> &'static str {
    match month {
        1  => "January",
        2  => "February",
        3  => "March",
        4  => "April",
        5  => "May",
        6  => "June",
        7  => "July",
        8  => "August",
        9  => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _  => "?",
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
