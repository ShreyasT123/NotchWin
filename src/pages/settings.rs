//! Settings page — a compact canvas list of the switches that matter most.
//! Each row is hit-tested in `main.rs`; this module only paints.

use iced::{
    alignment,
    widget::canvas::{self, Frame, Text},
    Color, Point,
};

use crate::{
    accent_display, settings::PAGE_IDS, STROKE, SURFACE_RAISED,
    TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TERTIARY,
};

/// One row's current value, already formatted by the application.
pub struct SettingsRow {
    pub label: &'static str,
    pub value: String,
}

pub fn settings_rows(settings: &crate::settings::AppSettings) -> Vec<SettingsRow> {
    vec![
        SettingsRow {
            label: "24-hour clock",
            value: on_off(settings.use_24_hour_clock),
        },
        SettingsRow {
            label: "Hover to peek",
            value: on_off(settings.hover_to_peek),
        },
        SettingsRow {
            label: "Wheel = volume",
            value: on_off(settings.wheel_adjusts_volume),
        },
        SettingsRow {
            label: "Volume HUD",
            value: on_off(settings.volume_hud_enabled),
        },
        SettingsRow {
            label: "Auto-hide pill",
            value: on_off(settings.auto_hide),
        },
        SettingsRow {
            label: "Auto-collapse",
            value: if settings.auto_collapse_seconds == 0 {
                "off".into()
            } else {
                format!("{}s", settings.auto_collapse_seconds)
            },
        },
        SettingsRow {
            label: "Island scale",
            value: format!("{:.2}×", settings.island_scale),
        },
        SettingsRow {
            label: "Accent",
            value: accent_display(settings.accent, settings.custom_accent),
        },
        SettingsRow {
            label: "Motion",
            value: motion_label(settings.motion_intensity).into(),
        },
        SettingsRow {
            label: "Visible pages",
            value: format!(
                "{}/{}",
                (0..PAGE_IDS.len()).filter(|i| settings.page_enabled(*i)).count(),
                PAGE_IDS.len()
            ),
        },
        SettingsRow {
            label: "Hide when covered",
            value: on_off(settings.hide_when_covered),
        },
        SettingsRow {
            label: "Launch at login",
            value: on_off(settings.start_with_windows),
        },
        SettingsRow {
            label: "First-run tip",
            value: if settings.intro_seen { "seen" } else { "show" }.into(),
        },
        SettingsRow {
            label: "Appearance",
            value: "›".into(),
        },
        SettingsRow {
            label: "Backup & sync",
            value: "›".into(),
        },
    ]
}

/// Rows for the appearance sub-page.
pub fn appearance_rows(settings: &crate::settings::AppSettings) -> Vec<SettingsRow> {
    vec![
        SettingsRow {
            label: "Accent preset",
            value: accent_display(settings.accent, settings.custom_accent),
        },
        SettingsRow {
            label: "Accent hue",
            value: if settings.accent == crate::CUSTOM_ACCENT_INDEX {
                accent_display(settings.accent, settings.custom_accent)
            } else {
                "tap to use custom".into()
            },
        },
        SettingsRow {
            label: "Surface opacity",
            value: format!("{:.0}%", (settings.surface_opacity * 100.0).round()),
        },
        SettingsRow {
            label: "Corner radius",
            value: format!("{:.2}×", settings.corner_scale),
        },
        SettingsRow {
            label: "Motion",
            value: motion_label(settings.motion_intensity).into(),
        },
    ]
}

/// Rows for the portable settings snapshot sub-page.
pub fn backup_rows(has_export: bool) -> Vec<SettingsRow> {
    vec![
        SettingsRow {
            label: "Export settings",
            value: if has_export { "overwrite" } else { "create" }.into(),
        },
        SettingsRow {
            label: "Import settings",
            value: if has_export { "available" } else { "none" }.into(),
        },
        SettingsRow {
            label: "Sample calendar",
            value: "load".into(),
        },
    ]
}

fn motion_label(v: f32) -> &'static str {
    if v <= 0.01 { "off" } else if v < 0.75 { "gentle" } else if v < 1.2 { "normal" } else { "lively" }
}

fn on_off(v: bool) -> String {
    if v { "on" } else { "off" }.into()
}

pub fn draw_settings_page(
    frame: &mut Frame,
    w: f32,
    h: f32,
    opacity: f32,
    rows: &[SettingsRow],
    title: &str,
) {
    frame.fill_text(Text {
        content: title.into(),
        position: Point::new(w / 2.0, 12.0),
        color: Color { a: opacity, ..TEXT_PRIMARY },
        size: iced::Pixels(14.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });

    let top = 32.0_f32;
    let row_h = ((h - top - 24.0) / rows.len() as f32).min(34.0);
    for (i, row) in rows.iter().enumerate() {
        let y = top + i as f32 * row_h;
        let path = crate::pages::rounded_rect_shared(8.0, y, w - 16.0, row_h - 4.0, 7.0);
        frame.fill(&path, canvas::Fill::from(Color {
            a: opacity * 0.55,
            ..SURFACE_RAISED
        }));
        frame.stroke(&path, canvas::Stroke {
            style: canvas::Style::Solid(Color { a: opacity * 0.09, ..STROKE }),
            width: 0.5,
            ..canvas::Stroke::default()
        });

        frame.fill_text(Text {
            content: row.label.into(),
            position: Point::new(18.0, y + (row_h - 4.0) / 2.0),
            color: Color { a: opacity * 0.92, ..TEXT_PRIMARY },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });

        frame.fill_text(Text {
            content: row.value.clone(),
            position: Point::new(w - 20.0, y + (row_h - 4.0) / 2.0),
            color: Color { a: opacity * 0.75, ..TEXT_SECONDARY },
            size: iced::Pixels(10.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Right.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
    }

    frame.fill_text(Text {
        content: "‹ back    ·    click a row to change".into(),
        position: Point::new(w / 2.0, h - 12.0),
        color: Color { a: opacity * 0.5, ..TEXT_TERTIARY },
        size: iced::Pixels(9.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Bottom,
        ..Text::default()
    });
}

/// Labels for the per-page visibility sub-page, in pager order.
pub const PAGE_LABELS: [&str; PAGE_IDS.len()] = [
    "Now", "Media", "Timers", "System", "Calendar", "Shortcuts", "Shelf", "Settings",
];

/// Rows for the per-page visibility sub-page.
pub fn page_rows(settings: &crate::settings::AppSettings) -> Vec<SettingsRow> {
    PAGE_LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| SettingsRow {
            label,
            value: if i == PAGE_IDS.len() - 1 {
                "always".into()
            } else if settings.page_enabled(i) {
                "on".into()
            } else {
                "off".into()
            },
        })
        .collect()
}

/// Row hit box, in page-local coordinates. Mirrored exactly by the painter.
pub fn row_rect(w: f32, h: f32, index: usize, count: usize) -> (f32, f32, f32, f32) {
    let count = count.max(1);
    let top = 32.0_f32;
    let row_h = ((h - top - 24.0) / count as f32).min(34.0);
    (8.0, top + index as f32 * row_h, w - 16.0, row_h - 4.0)
}
