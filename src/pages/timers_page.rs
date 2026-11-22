//! Timers page — circular ring timer display and composer dial.
//! Pure canvas; no iced widget tree.

use std::f32::consts::{PI, TAU};

use iced::{
    alignment,
    widget::canvas::{self, Frame, Fill, Stroke, Style, Text, LineCap},
    Color, Point,
};

use crate::{
    accent, COLOR_POSITIVE, STROKE, SURFACE_RAISED, SURFACE_SUNKEN,
    TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TERTIARY,
};

// ── Public types ─────────────────────────────────────────────────────────────

pub struct TimerDisplay {
    /// Stable id, used by canvas hit-testing to route button presses.
    pub id: String,
    /// Short human label for the timer.
    pub label: String,
    /// Formatted readout: "M:SS" or "MM:SS.hh".
    pub readout: String,
    /// 0..1 remaining fraction for countdown rings. `None` for stopwatch.
    pub progress: Option<f32>,
    /// 0..1 fraction of 60 s sweep for stopwatch hand. `None` for countdown.
    pub sweep: Option<f32>,
    pub is_running: bool,
    pub is_finished: bool,
    pub kind_is_countdown: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn draw_timers_page(
    frame: &mut Frame,
    w: f32,
    h: f32,
    opacity: f32,
    primary: Option<&TimerDisplay>,
    others: &[TimerDisplay],
    composer_minutes: u32,
    is_composer: bool,
) {
    if opacity < 0.004 {
        return;
    }

    if is_composer {
        draw_composer(frame, w, h, opacity, composer_minutes);
        return;
    }

    match primary {
        Some(timer) => {
            draw_stage(frame, w, h, opacity, timer, others);
        }
        None => {
            draw_empty(frame, w, h, opacity);
        }
    }
}

// ── Empty state ───────────────────────────────────────────────────────────────

fn draw_empty(frame: &mut Frame, w: f32, h: f32, opacity: f32) {
    frame.fill_text(Text {
        content: "No timers running".to_string(),
        position: Point::new(w / 2.0, h / 2.0),
        color: Color { a: opacity * 0.50, ..TEXT_SECONDARY },
        size: iced::Pixels(13.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
}

// ── Composer mode ─────────────────────────────────────────────────────────────

fn draw_composer(frame: &mut Frame, w: f32, h: f32, opacity: f32, composer_minutes: u32) {
    let half = w / 2.0;

    // ── Left half — circular dial ─────────────────────────────────────────
    let cx = half / 2.0;
    let cy = h / 2.0;
    let outer_r = (half / 2.0 - 20.0).max(10.0);
    let ring_thick = 10.0_f32;
    let inner_r = outer_r - ring_thick;

    // Track arc (full circle in SURFACE_SUNKEN)
    draw_arc_ring(frame, cx, cy, outer_r, ring_thick, 0.0, TAU, SURFACE_SUNKEN, opacity * 0.6);

    // Filled arc for composer_minutes / 60
    let fraction = (composer_minutes as f32 / 60.0).clamp(0.0, 1.0);
    if fraction > 0.001 {
        draw_arc_ring(frame, cx, cy, outer_r, ring_thick, -PI / 2.0, fraction * TAU, accent(), opacity * 0.9);
    }

    // Centre: large minute number
    frame.fill_text(Text {
        content: composer_minutes.to_string(),
        position: Point::new(cx, cy),
        color: Color { a: opacity, ..TEXT_PRIMARY },
        size: iced::Pixels(28.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });

    // ── Right half — presets ───────────────────────────────────────────────
    let right_x = half + 12.0;
    let mut ry = 20.0_f32;

    frame.fill_text(Text {
        content: "PRESETS".to_string(),
        position: Point::new(right_x, ry),
        color: Color { a: opacity * 0.50, ..TEXT_TERTIARY },
        size: iced::Pixels(9.5),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
    ry += 18.0;

    let presets: &[u32] = &[1, 3, 5, 10, 15, 25, 45];
    let pill_w = 32.0_f32;
    let pill_h = 20.0_f32;
    let pill_gap = 6.0_f32;
    let pills_per_row = ((w - right_x - 12.0) / (pill_w + pill_gap)).floor() as usize;
    let pills_per_row = pills_per_row.max(1);

    for (idx, &minutes) in presets.iter().enumerate() {
        let col = idx % pills_per_row;
        let row = idx / pills_per_row;
        let px = right_x + col as f32 * (pill_w + pill_gap);
        let py = ry + row as f32 * (pill_h + pill_gap);

        let selected = minutes == composer_minutes;
        let bg_alpha = if selected { 0.18 } else { 0.08 };
        let border_color = if selected { accent() } else { STROKE };
        let border_alpha = if selected { opacity * 0.9 } else { opacity * 0.4 };

        let pill = rounded_rect(px, py, pill_w, pill_h, pill_h / 2.0);
        frame.fill(&pill, Fill::from(Color { a: opacity * bg_alpha, ..border_color }));
        frame.stroke(
            &pill,
            Stroke {
                style: Style::Solid(Color { a: border_alpha, ..border_color }),
                width: 1.0,
                ..Stroke::default()
            },
        );
        frame.fill_text(Text {
            content: format!("{}m", minutes),
            position: Point::new(px + pill_w / 2.0, py + pill_h / 2.0),
            color: Color { a: opacity * if selected { 1.0 } else { 0.75 }, ..TEXT_PRIMARY },
            size: iced::Pixels(10.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
    }

    // Start button
    let rows_used = (presets.len() + pills_per_row - 1) / pills_per_row;
    let btn_y = ry + rows_used as f32 * (pill_h + pill_gap) + 8.0;
    let btn_w = w - right_x - 14.0;
    let btn_h = 24.0_f32;

    if btn_y + btn_h < h - 10.0 {
        // Soft glow behind the button
        for gi in 1..=4u32 {
            let spread = gi as f32 * 1.5;
            let ga = opacity * 0.08 * (1.0 - gi as f32 / 5.0);
            let glow = rounded_rect(
                right_x - spread,
                btn_y - spread,
                btn_w + spread * 2.0,
                btn_h + spread * 2.0,
                (btn_h / 2.0) + spread,
            );
            frame.fill(&glow, Fill::from(Color { a: ga, ..accent() }));
        }

        let btn = rounded_rect(right_x, btn_y, btn_w, btn_h, btn_h / 2.0);
        frame.fill(&btn, Fill::from(Color { a: opacity * 0.12, ..accent() }));
        frame.stroke(
            &btn,
            Stroke {
                style: Style::Solid(Color { a: opacity * 0.80, ..accent() }),
                width: 1.0,
                ..Stroke::default()
            },
        );

        let label = if composer_minutes > 0 {
            format!("Start {}m", composer_minutes)
        } else {
            "Start".to_string()
        };
        frame.fill_text(Text {
            content: label,
            position: Point::new(right_x + btn_w / 2.0, btn_y + btn_h / 2.0),
            color: Color { a: opacity * 0.95, ..accent() },
            size: iced::Pixels(12.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
    }
}

// ── Stage mode ────────────────────────────────────────────────────────────────

fn draw_stage(
    frame: &mut Frame,
    w: f32,
    h: f32,
    opacity: f32,
    timer: &TimerDisplay,
    others: &[TimerDisplay],
) {
    let half = w / 2.0;
    let ring_area_h = if others.is_empty() { h } else { h - 40.0 };

    // ── Left — timer ring ─────────────────────────────────────────────────
    let cx = half / 2.0;
    let cy = ring_area_h / 2.0;
    let outer_r = (half / 2.0 - 18.0).max(10.0);
    let ring_thick = 10.0_f32;

    // Ring colour: green if finished, accent otherwise
    let ring_color = if timer.is_finished { COLOR_POSITIVE } else { accent() };

    // Track
    draw_arc_ring(frame, cx, cy, outer_r, ring_thick, 0.0, TAU, SURFACE_SUNKEN, opacity * 0.5);

    // Filled arc
    let sweep_angle = if timer.is_finished {
        TAU
    } else if let Some(p) = timer.progress {
        // Countdown: arc length = 1 - progress (remaining portion)
        (1.0 - p).clamp(0.0, 1.0) * TAU
    } else if let Some(s) = timer.sweep {
        s.clamp(0.0, 1.0) * TAU
    } else {
        0.0
    };

    if sweep_angle > 0.001 {
        draw_arc_ring(frame, cx, cy, outer_r, ring_thick, -PI / 2.0, sweep_angle, ring_color, opacity * 0.9);
    }

    // Centre readout
    frame.fill_text(Text {
        content: timer.readout.clone(),
        position: Point::new(cx, cy),
        color: Color { a: opacity, ..TEXT_PRIMARY },
        size: iced::Pixels(20.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });

    // ── Right — label + state + controls ─────────────────────────────────
    let right_x = half + 12.0;
    let mut ry = cy - 38.0;
    ry = ry.max(14.0);

    // Timer label
    frame.fill_text(Text {
        content: truncate_str(&timer.label, 20),
        position: Point::new(right_x, ry),
        color: Color { a: opacity * 0.90, ..TEXT_PRIMARY },
        size: iced::Pixels(13.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
    ry += 18.0;

    // State label
    let state_text = if timer.is_finished {
        "Finished"
    } else if timer.is_running {
        "Running"
    } else {
        "Paused"
    };
    frame.fill_text(Text {
        content: state_text.to_string(),
        position: Point::new(right_x, ry),
        color: Color { a: opacity * 0.55, ..TEXT_SECONDARY },
        size: iced::Pixels(11.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
    ry += 24.0;

    // Control buttons: ▶/⏸  ↺  ✕
    let play_pause = if timer.is_running { "⏸" } else { "▶" };
    let ctrl_glyphs = [play_pause, "↺", "✕"];
    let ctrl_btn_w = 28.0_f32;
    let ctrl_btn_h = 22.0_f32;
    let ctrl_gap = 6.0_f32;

    for (i, &glyph) in ctrl_glyphs.iter().enumerate() {
        let bx = right_x + i as f32 * (ctrl_btn_w + ctrl_gap);
        let by = ry;
        let btn = rounded_rect(bx, by, ctrl_btn_w, ctrl_btn_h, 7.0);
        frame.fill(&btn, Fill::from(Color { a: opacity * 0.15, ..SURFACE_RAISED }));
        frame.stroke(
            &btn,
            Stroke {
                style: Style::Solid(Color { a: opacity * 0.35, ..STROKE }),
                width: 0.75,
                ..Stroke::default()
            },
        );
        frame.fill_text(Text {
            content: glyph.to_string(),
            position: Point::new(bx + ctrl_btn_w / 2.0, by + ctrl_btn_h / 2.0),
            color: Color { a: opacity * 0.90, ..TEXT_PRIMARY },
            size: iced::Pixels(13.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
    }

    // ── Others list at the bottom ─────────────────────────────────────────
    if !others.is_empty() {
        let list_y = ring_area_h + 6.0;
        let divider = canvas::Path::line(
            Point::new(14.0, list_y - 2.0),
            Point::new(w - 14.0, list_y - 2.0),
        );
        frame.stroke(
            &divider,
            Stroke {
                style: Style::Solid(Color { a: opacity * 0.20, ..STROKE }),
                width: 0.5,
                ..Stroke::default()
            },
        );

        let slot_w = (w - 20.0) / (others.len().min(3) as f32);
        for (i, other) in others.iter().take(3).enumerate() {
            let sx = 10.0 + i as f32 * slot_w;
            let sy = list_y;

            // Small ring
            let sr = 7.0_f32;
            let ring_thick_s = 2.5_f32;
            let scx = sx + sr + 4.0;
            let scy = sy + 10.0;
            draw_arc_ring(frame, scx, scy, sr, ring_thick_s, 0.0, TAU, SURFACE_SUNKEN, opacity * 0.4);

            let s_sweep = if other.is_finished {
                TAU
            } else if let Some(p) = other.progress {
                (1.0 - p).clamp(0.0, 1.0) * TAU
            } else if let Some(s) = other.sweep {
                s.clamp(0.0, 1.0) * TAU
            } else {
                0.0
            };
            let s_color = if other.is_finished { COLOR_POSITIVE } else { accent() };
            if s_sweep > 0.001 {
                draw_arc_ring(frame, scx, scy, sr, ring_thick_s, -PI / 2.0, s_sweep, s_color, opacity * 0.75);
            }

            // Readout
            frame.fill_text(Text {
                content: other.readout.clone(),
                position: Point::new(scx + sr + 6.0, sy + 2.0),
                color: Color { a: opacity * 0.80, ..TEXT_PRIMARY },
                size: iced::Pixels(10.0),
                font: iced::Font::DEFAULT,
                align_x: alignment::Horizontal::Left.into(),
                align_y: alignment::Vertical::Top,
                ..Text::default()
            });

            // Label
            let max_label_chars = (slot_w / 7.0) as usize;
            frame.fill_text(Text {
                content: truncate_str(&other.label, max_label_chars),
                position: Point::new(scx + sr + 6.0, sy + 14.0),
                color: Color { a: opacity * 0.50, ..TEXT_TERTIARY },
                size: iced::Pixels(9.0),
                font: iced::Font::DEFAULT,
                align_x: alignment::Horizontal::Left.into(),
                align_y: alignment::Vertical::Top,
                ..Text::default()
            });
        }
    }
}

// ── Arc ring primitive ────────────────────────────────────────────────────────

/// Draws an arc (or full circle) as a stroked ring segment.
///
/// `start_angle` is in radians from 3 o'clock (standard canvas convention).
/// `sweep` is the total angle to draw; TAU draws a full circle.
fn draw_arc_ring(
    frame: &mut Frame,
    cx: f32,
    cy: f32,
    outer_r: f32,
    thickness: f32,
    start_angle: f32,
    sweep: f32,
    color: Color,
    alpha: f32,
) {
    // Build arc path using mid-radius as the stroke centre.
    let mid_r = outer_r - thickness / 2.0;
    if mid_r <= 0.0 || sweep < 0.001 {
        return;
    }

    let steps = ((sweep / (PI / 60.0)).ceil() as usize).max(2);
    let arc_path = canvas::Path::new(|b| {
        let start_pt = Point::new(
            cx + mid_r * start_angle.cos(),
            cy + mid_r * start_angle.sin(),
        );
        b.move_to(start_pt);
        for s in 1..=steps {
            let angle = start_angle + sweep * s as f32 / steps as f32;
            b.line_to(Point::new(
                cx + mid_r * angle.cos(),
                cy + mid_r * angle.sin(),
            ));
        }
    });

    frame.stroke(
        &arc_path,
        Stroke {
            style: Style::Solid(Color { a: alpha, r: color.r, g: color.g, b: color.b }),
            width: thickness,
            line_cap: LineCap::Round,
            ..Stroke::default()
        },
    );
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
