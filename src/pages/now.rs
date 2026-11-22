//! "Now" page — live clock, agenda, status chips.
//! Pure canvas; no iced widget tree.

use iced::{
    alignment,
    widget::canvas::{self, Frame, Fill, Stroke, Style, Text, LineCap},
    Color, Point,
};

use crate::{
    accent, STROKE, SURFACE_SUNKEN,
    TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TERTIARY,
};

// ── Public types ─────────────────────────────────────────────────────────────

/// A single row in the agenda card.
pub struct AgendaItem {
    /// "09:00" for timed events, "" for untimed notes.
    pub time_label: String,
    /// Display text.
    pub text: String,
    /// True for active countdown / stopwatch entries.
    pub is_timer: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn draw_now_page(
    frame: &mut Frame,
    w: f32,
    h: f32,
    opacity: f32,
    now: &chrono::DateTime<chrono::Local>,
    settings_use_24h: bool,
    agenda_items: &[AgendaItem],
    roll_progress: f32,
    roll_changed: &[bool],
    roll_direction: &[i8],
    battery_pct: Option<u8>,
    battery_charging: bool,
    volume_pct: Option<u8>,
    volume_muted: bool,
    network_connected: bool,
    network_ssid: &str,
) {
    if opacity < 0.004 {
        return;
    }

    let pad_x = 18.0_f32;
    let pad_y = 14.0_f32;

    // ── Row 1 — large clock + seconds/meridiem + week chip ───────────────────
    let row1_y = pad_y + 10.0;

    // Large clock (HH:MM or h:MM)
    let (clock_str, seconds_str, meridiem) = if settings_use_24h {
        let c = now.format("%H:%M").to_string();
        let s = now.format("%S").to_string();
        (c, s, String::new())
    } else {
        let c = now.format("%-I:%M").to_string();
        let s = now.format("%S").to_string();
        let m = now.format("%p").to_string();
        (c, s, m)
    };

    // Draw character by character so an unchanged colon or neighbour never
    // moves. Each changed digit slides in and fades over ~380 ms.
    for (i, ch) in clock_str.chars().enumerate() {
        let rolling = roll_changed.get(i).copied().unwrap_or(false) && roll_progress < 1.0;
        let (offset_y, alpha) = if rolling {
            let dir = if roll_direction.get(i).copied().unwrap_or(1) >= 0 { 1.0 } else { -1.0 };
            ((1.0 - roll_progress) * 10.0 * dir, opacity * (0.25 + 0.75 * roll_progress))
        } else {
            (0.0, opacity)
        };
        frame.fill_text(Text {
            content: ch.to_string(),
            position: Point::new(pad_x + i as f32 * 14.0, row1_y + offset_y),
            color: Color { a: alpha, ..TEXT_PRIMARY },
            size: iced::Pixels(24.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }

    // Seconds (small, accent)
    let secs_x = pad_x + 86.0;
    frame.fill_text(Text {
        content: seconds_str,
        position: Point::new(secs_x, row1_y + 2.0),
        color: Color { a: opacity * 0.9, ..accent() },
        size: iced::Pixels(12.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    // Meridiem if 12h mode
    if !meridiem.is_empty() {
        frame.fill_text(Text {
            content: meridiem,
            position: Point::new(secs_x, row1_y + 16.0),
            color: Color { a: opacity * 0.7, ..accent() },
            size: iced::Pixels(10.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }

    // Week chip — small rounded rect on the right
    let week_num = now.format("%V").to_string();
    let week_label = format!("Wk {}", week_num);
    let chip_w = 42.0_f32;
    let chip_h = 16.0_f32;
    let chip_x = w - pad_x - chip_w;
    let chip_y = row1_y + 4.0;

    let chip_bg = rounded_rect(chip_x, chip_y, chip_w, chip_h, 5.0);
    frame.fill(&chip_bg, Fill::from(Color { a: opacity * 0.6, ..SURFACE_SUNKEN }));
    frame.stroke(
        &chip_bg,
        Stroke {
            style: Style::Solid(Color { a: opacity * 0.5, ..STROKE }),
            width: 0.75,
            ..Stroke::default()
        },
    );
    frame.fill_text(Text {
        content: week_label,
        position: Point::new(chip_x + chip_w / 2.0, chip_y + chip_h / 2.0),
        color: Color { a: opacity * 0.75, ..TEXT_SECONDARY },
        size: iced::Pixels(9.5),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });

    // ── Row 2 — weekday · date ────────────────────────────────────────────────
    let row2_y = row1_y + 32.0;

    let weekday = now.format("%A").to_string().to_uppercase();
    let day_month = now.format("%-d %B").to_string().to_uppercase();

    frame.fill_text(Text {
        content: weekday.clone(),
        position: Point::new(pad_x, row2_y),
        color: Color { a: opacity * 0.9, ..accent() },
        size: iced::Pixels(11.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    // Separator bar
    let sep_x = pad_x + weekday.len() as f32 * 7.2 + 4.0;
    let sep_path = canvas::Path::line(
        Point::new(sep_x, row2_y + 2.0),
        Point::new(sep_x, row2_y + 10.0),
    );
    frame.stroke(
        &sep_path,
        Stroke {
            style: Style::Solid(Color { a: opacity * 0.35, ..TEXT_SECONDARY }),
            width: 1.0,
            line_cap: LineCap::Round,
            ..Stroke::default()
        },
    );

    frame.fill_text(Text {
        content: day_month,
        position: Point::new(sep_x + 7.0, row2_y),
        color: Color { a: opacity * 0.65, ..TEXT_SECONDARY },
        size: iced::Pixels(11.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    // ── Agenda card ───────────────────────────────────────────────────────────
    let card_top = row2_y + 22.0;
    let row_h = 20.0_f32;
    let shown = agenda_items.len().min(3);
    let card_h = 24.0 + shown as f32 * row_h + 10.0;
    let card_right = w - pad_x;
    let card_w = card_right - pad_x;

    let card_path = rounded_rect(pad_x, card_top, card_w, card_h, 10.0);
    frame.fill(&card_path, Fill::from(Color { a: opacity * 0.55, ..SURFACE_SUNKEN }));
    frame.stroke(
        &card_path,
        Stroke {
            style: Style::Solid(Color { a: opacity * 0.4, ..STROKE }),
            width: 0.75,
            ..Stroke::default()
        },
    );

    // "TODAY" overline
    frame.fill_text(Text {
        content: "TODAY".to_string(),
        position: Point::new(pad_x + 10.0, card_top + 7.0),
        color: Color { a: opacity * 0.55, ..TEXT_TERTIARY },
        size: iced::Pixels(9.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    // "+N more" if there would be more than 3 items
    // (the caller passes at most 3 but we signal the original count via a
    //  convention: the third item's text may start with "+N more" — we just
    //  render whatever we receive; the chip is shown when shown == 3 and the
    //  last item signals overflow by convention).
    // For a cleaner approach we render a chip on the right of the overline row.
    if agenda_items.len() > 3 {
        let overflow = agenda_items.len() - 3;
        frame.fill_text(Text {
            content: format!("+{} more", overflow),
            position: Point::new(card_right - 10.0, card_top + 7.0),
            color: Color { a: opacity * 0.45, ..TEXT_TERTIARY },
            size: iced::Pixels(9.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Right.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }

    // Divider under overline
    let div_y = card_top + 20.0;
    let div = canvas::Path::line(
        Point::new(pad_x + 8.0, div_y),
        Point::new(card_right - 8.0, div_y),
    );
    frame.stroke(
        &div,
        Stroke {
            style: Style::Solid(Color { a: opacity * 0.25, ..STROKE }),
            width: 0.5,
            ..Stroke::default()
        },
    );

    // Agenda rows
    for (i, item) in agenda_items.iter().take(3).enumerate() {
        let ry = div_y + 4.0 + i as f32 * row_h;

        // Bullet dot
        let bullet_color = if item.is_timer {
            Color { a: opacity * 0.9, ..accent() }
        } else if item.time_label.is_empty() {
            Color { a: opacity * 0.55, ..TEXT_TERTIARY }
        } else {
            Color { a: opacity * 0.75, ..TEXT_SECONDARY }
        };

        frame.fill(
            &canvas::Path::circle(Point::new(pad_x + 12.0, ry + 7.0), 2.0),
            Fill::from(bullet_color),
        );

        // Icon/type prefix
        let icon = if item.is_timer {
            "⏱"
        } else if item.time_label.is_empty() {
            "📝"
        } else {
            "⏰"
        };
        frame.fill_text(Text {
            content: icon.to_string(),
            position: Point::new(pad_x + 20.0, ry + 1.0),
            color: Color { a: opacity * 0.7, ..TEXT_SECONDARY },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });

        // Time label (if any)
        if !item.time_label.is_empty() {
            frame.fill_text(Text {
                content: item.time_label.clone(),
                position: Point::new(pad_x + 36.0, ry + 2.0),
                color: Color { a: opacity * 0.7, ..accent() },
                size: iced::Pixels(10.0),
                font: iced::Font::DEFAULT,
                align_x: alignment::Horizontal::Left.into(),
                align_y: alignment::Vertical::Top,
                ..Text::default()
            });
        }

        // Event text — truncate at card width
        let text_x = pad_x + 86.0;
        let max_chars = ((card_w - 90.0) / 6.5) as usize;
        let display = truncate_str(&item.text, max_chars);
        frame.fill_text(Text {
            content: display,
            position: Point::new(text_x, ry + 2.0),
            color: Color { a: opacity * 0.85, ..TEXT_PRIMARY },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }

    // Empty state inside card
    if shown == 0 {
        frame.fill_text(Text {
            content: "Nothing scheduled today".to_string(),
            position: Point::new(pad_x + card_w / 2.0, div_y + 14.0),
            color: Color { a: opacity * 0.45, ..TEXT_TERTIARY },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }

    // ── Status chips row ──────────────────────────────────────────────────────
    let status_y = h - pad_y - 14.0;
    let mut chip_cursor = pad_x;

    // Battery chip
    if let Some(pct) = battery_pct {
        let icon = if battery_charging { "⚡" } else { "🔋" };
        let label = format!("{} {}%", icon, pct);
        chip_cursor = draw_status_chip(frame, chip_cursor, status_y, &label, opacity);
        chip_cursor += 10.0;
    }

    // Network chip
    {
        let icon = if network_connected { "Wi-Fi" } else { "●" };
        let label = if network_connected && !network_ssid.is_empty() {
            format!("⌔ {}", truncate_str(network_ssid, 12))
        } else if network_connected {
            format!("⌔ {}", icon)
        } else {
            "⊘ No network".to_string()
        };
        chip_cursor = draw_status_chip(frame, chip_cursor, status_y, &label, opacity);
        chip_cursor += 10.0;
    }

    // Volume chip
    if let Some(vol) = volume_pct {
        let icon = if volume_muted { "🔇" } else { "🔊" };
        let label = format!("{} {}%", icon, vol);
        draw_status_chip(frame, chip_cursor, status_y, &label, opacity);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Draws a small status chip and returns the x position after the chip.
fn draw_status_chip(
    frame: &mut Frame,
    x: f32,
    y: f32,
    label: &str,
    opacity: f32,
) -> f32 {
    let cw = label.len() as f32 * 6.5 + 14.0;
    let ch = 16.0_f32;
    let path = rounded_rect(x, y, cw, ch, 5.0);
    frame.fill(
        &path,
        Fill::from(Color { a: opacity * 0.35, ..SURFACE_SUNKEN }),
    );
    frame.stroke(
        &path,
        Stroke {
            style: Style::Solid(Color { a: opacity * 0.3, ..STROKE }),
            width: 0.5,
            ..Stroke::default()
        },
    );
    frame.fill_text(Text {
        content: label.to_string(),
        position: Point::new(x + cw / 2.0, y + ch / 2.0),
        color: Color { a: opacity * 0.7, ..TEXT_SECONDARY },
        size: iced::Pixels(9.5),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
    x + cw
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> canvas::Path {
    let r = r.min(w / 2.0).min(h / 2.0);
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
        let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}
