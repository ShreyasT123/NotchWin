//! Shortcuts page — a grid of tappable shortcut tiles.
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
pub struct ShortcutEntry {
    pub id: String,
    /// Human-readable label shown beneath the icon.
    pub label: String,
    /// Single character used as the tile icon (emoji or Unicode symbol).
    pub icon_char: char,
    pub is_pinned: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn draw_shortcuts_page(
    frame: &mut Frame,
    w: f32,
    h: f32,
    opacity: f32,
    shortcuts: &[ShortcutEntry],
    hovered_id: Option<&str>,
) {
    if opacity < 0.004 {
        return;
    }

    let pad_x = 12.0_f32;
    let pad_y = 12.0_f32;
    let tile_w = 64.0_f32;
    let tile_h = 64.0_f32;
    let gap = 8.0_f32;

    // How many columns fit in the available width
    let avail_w = w - pad_x * 2.0;
    let cols = ((avail_w + gap) / (tile_w + gap)).floor() as usize;
    let cols = cols.max(1);

    for (i, entry) in shortcuts.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;

        let tx = pad_x + col as f32 * (tile_w + gap);
        let ty = pad_y + row as f32 * (tile_h + gap);

        // Stop drawing if the tile would fall outside the visible area
        if ty + tile_h > h + 2.0 {
            break;
        }

        let is_hovered = hovered_id.map_or(false, |id| id == entry.id.as_str());

        // Tile background colour: brighten RGB channels by 0.06 when hovered
        let base_color = SURFACE_RAISED;
        let tile_color = if is_hovered {
            Color {
                r: (base_color.r + 0.06).min(1.0),
                g: (base_color.g + 0.06).min(1.0),
                b: (base_color.b + 0.06).min(1.0),
                a: base_color.a,
            }
        } else {
            base_color
        };

        // Tile background
        let tile_path = rounded_rect(tx, ty, tile_w, tile_h, 11.0);
        frame.fill(&tile_path, Fill::from(Color { a: opacity, ..tile_color }));

        // Tile border
        let border_alpha = if is_hovered { 0.22 } else { 0.12 };
        frame.stroke(
            &tile_path,
            Stroke {
                style: Style::Solid(Color {
                    a: opacity * border_alpha,
                    r: STROKE.r,
                    g: STROKE.g,
                    b: STROKE.b,
                }),
                width: if is_hovered { 1.0 } else { 0.75 },
                ..Stroke::default()
            },
        );

        // Icon character centred in the tile (upper region)
        let icon_y = ty + tile_h * 0.38;
        frame.fill_text(Text {
            content: entry.icon_char.to_string(),
            position: Point::new(tx + tile_w / 2.0, icon_y),
            color: Color { a: opacity * 0.90, ..TEXT_PRIMARY },
            size: iced::Pixels(16.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });

        // Label below the icon (9px, TEXT_SECONDARY)
        let label_y = ty + tile_h - 11.0;
        let max_chars = (tile_w / 5.5) as usize;
        frame.fill_text(Text {
            content: truncate_str(&entry.label, max_chars),
            position: Point::new(tx + tile_w / 2.0, label_y),
            color: Color { a: opacity * 0.70, ..TEXT_SECONDARY },
            size: iced::Pixels(9.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });

        // Pinned indicator dot — top-right corner of the tile
        if entry.is_pinned {
            let dot_cx = tx + tile_w - 8.0;
            let dot_cy = ty + 8.0;
            frame.fill(
                &canvas::Path::circle(Point::new(dot_cx, dot_cy), 3.5),
                Fill::from(Color { a: opacity * 0.90, ..accent() }),
            );
        }
    }

    // Empty state
    if shortcuts.is_empty() {
        frame.fill_text(Text {
            content: "No shortcuts added".to_string(),
            position: Point::new(w / 2.0, h / 2.0),
            color: Color { a: opacity * 0.45, ..TEXT_TERTIARY },
            size: iced::Pixels(12.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
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

// ── Catalog ───────────────────────────────────────────────────────────────────

/// `(id, label, icon, shell target)` — a direct port of `ShortcutCatalog.all`.
/// Windows resolves URLs, executables, `shell:` folders and `ms-settings:`
/// URIs in ShellExecuteW, so the target is stored verbatim.
const CATALOG: &[(&str, &str, char, &str)] = &[
    ("youtube", "YouTube", '▶', "https://www.youtube.com"),
    ("ytmusic", "YT Music", '♫', "https://music.youtube.com"),
    ("spotify", "Spotify", '♫', "https://open.spotify.com"),
    ("netflix", "Netflix", '▣', "https://www.netflix.com"),
    ("twitch", "Twitch", '▣', "https://www.twitch.tv"),
    ("gmail", "Gmail", '✉', "https://mail.google.com"),
    ("gcalendar", "Calendar", '▦', "https://calendar.google.com"),
    ("gdrive", "Drive", '☁', "https://drive.google.com"),
    ("maps", "Maps", '◇', "https://www.google.com/maps"),
    ("translate", "Translate", '文', "https://translate.google.com"),
    ("github", "GitHub", '⌘', "https://github.com"),
    ("claude", "Claude", '✧', "https://claude.ai"),
    ("chatgpt", "ChatGPT", '✧', "https://chatgpt.com"),
    ("whatsapp", "WhatsApp", '✧', "https://web.whatsapp.com"),
    ("x", "X", '✧', "https://x.com"),
    ("reddit", "Reddit", '✧', "https://www.reddit.com"),
    ("explorer", "Explorer", '▤', "shell:MyComputerFolder"),
    ("downloads", "Downloads", '↓', "shell:Downloads"),
    ("documents", "Documents", '▤', "shell:Personal"),
    ("pictures", "Pictures", '▧', "shell:My Pictures"),
    ("recycle", "Recycle Bin", '♲', "shell:RecycleBinFolder"),
    ("terminal", "Terminal", '⌨', "wt.exe"),
    ("taskmgr", "Task Manager", '◈', "taskmgr.exe"),
    ("calculator", "Calculator", '=', "calc.exe"),
    ("notepad", "Notepad", '▤', "notepad.exe"),
    ("paint", "Paint", '▧', "mspaint.exe"),
    ("snip", "Screen snip", '✂', "ms-screenclip:"),
    ("camera", "Camera", '◉', "microsoft.windows.camera:"),
    ("store", "Store", '▣', "ms-windows-store:"),
    ("settings", "Settings", '⚙', "ms-settings:"),
    ("sound", "Sound", '♪', "ms-settings:sound"),
    ("bluetooth", "Bluetooth", '⌁', "ms-settings:bluetooth"),
    ("wifi", "Wi-Fi", '⌁', "ms-settings:network-wifi"),
    ("displaysettings", "Display", '▣', "ms-settings:display"),
    ("nightlight", "Night light", '☾', "ms-settings:nightlight"),
    ("batterysettings", "Battery", '⚡', "ms-settings:batterysaver"),
    ("focus", "Focus", '◐', "ms-settings:quiethours"),
    ("notificationsettings", "Notifications", '♢', "ms-settings:notifications"),
    ("appsfeatures", "Installed apps", '▤', "ms-settings:appsfeatures"),
    ("storage", "Storage", '▣', "ms-settings:storagesense"),
    ("update", "Windows Update", '↻', "ms-settings:windowsupdate"),
];

pub fn catalog_entry(id: &str) -> Option<(&'static str, char, &'static str)> {
    CATALOG
        .iter()
        .find(|(key, ..)| *key == id)
        .map(|(_, label, icon, target)| (*label, *icon, *target))
}

pub fn target_for(id: &str) -> Option<&'static str> {
    catalog_entry(id).map(|(_, _, target)| target)
}

/// What a new installation pins, ported from `ShortcutCatalog.defaultPinned`.
pub fn default_pinned() -> Vec<String> {
    [
        "youtube", "ytmusic", "spotify", "gmail", "gcalendar", "github",
        "explorer", "downloads", "terminal", "snip", "sound", "settings",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
