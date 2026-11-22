//! System page — CPU, memory, network sparklines + status chips.
//! Pure canvas; no iced widget tree.

use iced::{
    alignment,
    widget::canvas::{self, Frame, Fill, Stroke, Style, Text, LineCap},
    Color, Point,
};

use crate::{
    accent, COLOR_POSITIVE, COLOR_WARNING, STROKE, SURFACE_SUNKEN, SURFACE_RAISED,
    TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TERTIARY,
};

// ── Entry point ───────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn draw_system_page(
    frame: &mut Frame,
    w: f32,
    h: f32,
    opacity: f32,
    cpu_history: &[f32],
    mem_history: &[f32],
    net_down_history: &[f32],
    net_up_history: &[f32],
    cpu_pct: Option<f32>,
    mem_pct: Option<f32>,
    mem_used_gb: f32,
    mem_total_gb: f32,
    net_down_bps: f32,
    net_up_bps: f32,
    network_valid: bool,
    battery_pct: Option<u8>,
    battery_charging: bool,
    network_ssid: &str,
    network_connected: bool,
    volume_pct: Option<u8>,
    volume_muted: bool,
    mic_in_use: bool,
    camera_in_use: bool,
) {
    if opacity < 0.004 {
        return;
    }

    let pad_x = 18.0_f32;
    let mut y = 14.0_f32;

    // ── Page title ────────────────────────────────────────────────────────────
    frame.fill_text(Text {
        content: "SYSTEM".to_string(),
        position: Point::new(pad_x, y),
        color: Color { a: opacity * 0.55, ..TEXT_TERTIARY },
        size: iced::Pixels(10.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
    y += 14.0;

    // Divider
    draw_divider(frame, pad_x, y, w - pad_x * 2.0, opacity);
    y += 8.0;

    // ── Live system strip ─────────────────────────────────────────────────────
    // A compact snapshot of the transient faces shown on the collapsed pill:
    // output volume, microphone/camera consent and the active Wi-Fi network.
    draw_system_strip(
        frame,
        pad_x,
        y,
        w - pad_x * 2.0,
        opacity,
        volume_pct,
        volume_muted,
        mic_in_use,
        camera_in_use,
        network_connected,
        network_ssid,
    );
    y += 34.0 + 8.0;

    // ── CPU section ───────────────────────────────────────────────────────────
    let cpu_label = match cpu_pct {
        Some(p) => format!("Processor  {:.0}%", p * 100.0),
        None    => "Processor  --".to_string(),
    };
    draw_section_header(frame, pad_x, y, w - pad_x * 2.0, &cpu_label, cpu_pct, opacity);
    y += 14.0;

    let sparkline_w = w - pad_x * 2.0;
    draw_sparkline(frame, pad_x, y, sparkline_w, 34.0, cpu_history, 1.0, accent(), opacity, false);
    y += 34.0 + 8.0;

    // ── Memory section ────────────────────────────────────────────────────────
    let mem_label = match mem_pct {
        Some(p) => format!(
            "Memory  {:.0}%  ·  {:.1} of {:.1} GB",
            p * 100.0,
            mem_used_gb,
            mem_total_gb
        ),
        None => format!("Memory  ·  {:.1} of {:.1} GB", mem_used_gb, mem_total_gb),
    };
    draw_section_header(frame, pad_x, y, w - pad_x * 2.0, &mem_label, mem_pct, opacity);
    y += 14.0;

    draw_sparkline(frame, pad_x, y, sparkline_w, 28.0, mem_history, 1.0, COLOR_POSITIVE, opacity, false);
    y += 28.0 + 8.0;

    // ── Network section ───────────────────────────────────────────────────────
    let down_label = format_bytes(net_down_bps);
    let up_label   = format_bytes(net_up_bps);
    let net_header = format!("Network  ↓ {}  ↑ {}", down_label, up_label);
    draw_section_header(frame, pad_x, y, w - pad_x * 2.0, &net_header, None, opacity);
    y += 14.0;

    // Determine dynamic max for both directions so the two sparklines share scale
    let net_max = net_down_history
        .iter()
        .chain(net_up_history.iter())
        .cloned()
        .fold(1.0_f32, f32::max);

    if network_valid {
        draw_sparkline(frame, pad_x, y, sparkline_w, 18.0, net_down_history, net_max, accent(), opacity, false);
        y += 18.0 + 2.0;
        draw_sparkline(frame, pad_x, y, sparkline_w, 14.0, net_up_history, net_max, COLOR_WARNING, opacity, true);
        y += 14.0 + 8.0;
    } else {
        frame.fill_text(Text {
            content: "No network".to_string(),
            position: Point::new(pad_x, y + 6.0),
            color: Color { a: opacity * 0.45, ..TEXT_TERTIARY },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
        y += 30.0;
    }

    // ── Status chips ─────────────────────────────────────────────────────────
    let chip_y = h - 28.0;
    let mut cx = pad_x;

    if let Some(pct) = battery_pct {
        let icon = if battery_charging { "⚡" } else { "🔋" };
        let label = format!("{} {}%", icon, pct);
        cx = draw_chip(frame, cx, chip_y, &label, opacity) + 8.0;
    }

    if network_connected {
        let ssid = if network_ssid.is_empty() { "Wi-Fi" } else { network_ssid };
        let label = format!("⌔ {}", truncate_str(ssid, 14));
        draw_chip(frame, cx, chip_y, &label, opacity);
    } else {
        draw_chip(frame, cx, chip_y, "⊘ Offline", opacity);
    }

    // Suppress unused variable warning when network section renders nothing at y
    let _ = y;
}

// ── Live system strip ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_system_strip(
    frame: &mut Frame,
    x: f32,
    y: f32,
    w: f32,
    opacity: f32,
    volume_pct: Option<u8>,
    volume_muted: bool,
    mic_in_use: bool,
    camera_in_use: bool,
    network_connected: bool,
    network_ssid: &str,
) {
    let strip = rounded_rect(x, y, w, 34.0, 8.0);
    frame.fill(&strip, Fill::from(Color { a: opacity * 0.34, ..SURFACE_RAISED }));
    frame.stroke(
        &strip,
        Stroke {
            style: Style::Solid(Color { a: opacity * 0.22, ..STROKE }),
            width: 0.5,
            ..Stroke::default()
        },
    );

    let volume_value = if volume_muted {
        "Muted".to_string()
    } else {
        volume_pct.map(|p| format!("{p}%")).unwrap_or_else(|| "—".into())
    };
    let wifi_value = if network_connected {
        let ssid = if network_ssid.is_empty() { "Wi-Fi" } else { network_ssid };
        truncate_str(ssid, 10)
    } else {
        "Offline".into()
    };

    let tiles: [(&str, String, Color); 4] = [
        ("VOLUME", volume_value, if volume_muted { COLOR_WARNING } else { accent() }),
        ("MIC", if mic_in_use { "In use".into() } else { "Idle".into() },
            if mic_in_use { COLOR_WARNING } else { TEXT_SECONDARY }),
        ("CAMERA", if camera_in_use { "In use".into() } else { "Idle".into() },
            if camera_in_use { COLOR_WARNING } else { TEXT_SECONDARY }),
        ("NETWORK", wifi_value, if network_connected { COLOR_POSITIVE } else { COLOR_WARNING }),
    ];

    let tile_w = w / tiles.len() as f32;
    for (i, (label, value, color)) in tiles.iter().enumerate() {
        let color = *color;
        let tx = x + i as f32 * tile_w;
        if i > 0 {
            let sep = canvas::Path::line(
                Point::new(tx, y + 5.0),
                Point::new(tx, y + 29.0),
            );
            frame.stroke(
                &sep,
                Stroke {
                    style: Style::Solid(Color { a: opacity * 0.15, ..STROKE }),
                    width: 0.5,
                    ..Stroke::default()
                },
            );
        }
        frame.fill_text(Text {
            content: (*label).to_string(),
            position: Point::new(tx + tile_w / 2.0, y + 5.0),
            color: Color { a: opacity * 0.5, ..TEXT_TERTIARY },
            size: iced::Pixels(8.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
        frame.fill_text(Text {
            content: value.clone(),
            position: Point::new(tx + tile_w / 2.0, y + 16.0),
            color: Color { a: opacity, ..color },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }
}

// ── Sparkline ─────────────────────────────────────────────────────────────────

/// Catmull-Rom smoothed sparkline drawn as a filled area chart.
///
/// `mirrored = true` renders the chart upside-down (net-up direction).
pub fn draw_sparkline(
    frame: &mut Frame,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    values: &[f32],
    max_val: f32,
    color: Color,
    opacity: f32,
    mirrored: bool,
) {
    if values.is_empty() || w < 2.0 || h < 2.0 || max_val <= 0.0 {
        return;
    }

    let n = values.len();
    // Map value index → pixel
    let px = |i: usize| -> f32 { x + i as f32 / (n - 1).max(1) as f32 * w };
    let py = |v: f32| -> f32 {
        let norm = (v / max_val).clamp(0.0, 1.0);
        if mirrored {
            y + norm * h
        } else {
            y + h - norm * h
        }
    };
    let baseline_y = if mirrored { y } else { y + h };

    // Build smooth path using quadratic Catmull-Rom via midpoints.
    // For each pair of adjacent points, the midpoint is used as the "hard"
    // waypoint; the original point becomes the control point. This produces a
    // curve that passes through the midpoints and smoothly interpolates.
    let fill_path = canvas::Path::new(|b| {
        // Start at the baseline below the first sample
        b.move_to(Point::new(px(0), baseline_y));
        // Rise to the first data point
        b.line_to(Point::new(px(0), py(values[0])));

        if n >= 2 {
            // First segment: line to first midpoint
            let mx0 = (px(0) + px(1)) / 2.0;
            let my0 = (py(values[0]) + py(values[1])) / 2.0;
            b.line_to(Point::new(mx0, my0));

            for i in 1..n - 1 {
                let mid_x = (px(i) + px(i + 1)) / 2.0;
                let mid_y = (py(values[i]) + py(values[i + 1])) / 2.0;
                b.quadratic_curve_to(Point::new(px(i), py(values[i])), Point::new(mid_x, mid_y));
            }

            // Final segment: line to last point
            b.line_to(Point::new(px(n - 1), py(values[n - 1])));
        }

        // Drop down to baseline and close
        b.line_to(Point::new(px(n - 1), baseline_y));
        b.close();
    });

    // Fill at 30% alpha
    frame.fill(
        &fill_path,
        Fill::from(Color { a: opacity * color.a * 0.30, r: color.r, g: color.g, b: color.b }),
    );

    // Stroke the top line at full alpha — rebuild the path without the fill sides
    let line_path = canvas::Path::new(|b| {
        b.move_to(Point::new(px(0), py(values[0])));

        if n >= 2 {
            let mx0 = (px(0) + px(1)) / 2.0;
            let my0 = (py(values[0]) + py(values[1])) / 2.0;
            b.line_to(Point::new(mx0, my0));

            for i in 1..n - 1 {
                let mid_x = (px(i) + px(i + 1)) / 2.0;
                let mid_y = (py(values[i]) + py(values[i + 1])) / 2.0;
                b.quadratic_curve_to(Point::new(px(i), py(values[i])), Point::new(mid_x, mid_y));
            }

            b.line_to(Point::new(px(n - 1), py(values[n - 1])));
        }
    });

    frame.stroke(
        &line_path,
        Stroke {
            style: Style::Solid(Color { a: opacity * color.a, r: color.r, g: color.g, b: color.b }),
            width: 1.25,
            line_cap: LineCap::Round,
            ..Stroke::default()
        },
    );
}

// ── Layout helpers ────────────────────────────────────────────────────────────

fn draw_section_header(
    frame: &mut Frame,
    x: f32,
    y: f32,
    _w: f32,
    label: &str,
    pct: Option<f32>,
    opacity: f32,
) {
    // Label text
    frame.fill_text(Text {
        content: label.to_string(),
        position: Point::new(x, y),
        color: Color { a: opacity * 0.85, ..TEXT_PRIMARY },
        size: iced::Pixels(11.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    // If there's a percentage, draw a thin bar indicator on the right side
    if let Some(p) = pct {
        let bar_w = 60.0_f32;
        let bar_h = 3.0_f32;
        let bar_x = x + _w - bar_w;
        let bar_y = y + 5.0;

        let track = rounded_rect(bar_x, bar_y, bar_w, bar_h, 1.5);
        frame.fill(&track, Fill::from(Color { a: opacity * 0.3, ..SURFACE_SUNKEN }));

        let fill_w = (bar_w * p.clamp(0.0, 1.0)).max(2.0);
        let fill = rounded_rect(bar_x, bar_y, fill_w, bar_h, 1.5);
        frame.fill(&fill, Fill::from(Color { a: opacity * 0.7, ..accent() }));
    }
}

fn draw_divider(frame: &mut Frame, x: f32, y: f32, w: f32, opacity: f32) {
    let path = canvas::Path::line(Point::new(x, y), Point::new(x + w, y));
    frame.stroke(
        &path,
        Stroke {
            style: Style::Solid(Color { a: opacity * 0.25, ..STROKE }),
            width: 0.5,
            ..Stroke::default()
        },
    );
}

fn draw_chip(frame: &mut Frame, x: f32, y: f32, label: &str, opacity: f32) -> f32 {
    let cw = label.len() as f32 * 6.2 + 14.0;
    let ch = 16.0_f32;
    let path = rounded_rect(x, y, cw, ch, 5.0);
    frame.fill(&path, Fill::from(Color { a: opacity * 0.35, ..SURFACE_RAISED }));
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

// ── Utilities ─────────────────────────────────────────────────────────────────

fn format_bytes(bps: f32) -> String {
    if bps >= 1_000_000.0 {
        format!("{:.1} MB/s", bps / 1_000_000.0)
    } else if bps >= 1_000.0 {
        format!("{:.0} KB/s", bps / 1_000.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

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
