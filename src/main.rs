// notchWin — Rust/iced port of the Flutter notchX Dynamic Island overlay.
//
// Architecture:
//   spring.rs         ← SpringGroup (matches DbSpring timing from db_motion.dart)
//   metrics.rs        ← IslandMetrics / DbSize / DbRadius constants
//   state.rs          ← IslandController intent / phase / geometry logic
//   win32.rs          ← overlay window Win32 helpers
//   settings.rs       ← AppSettings + SettingsController
//   timers.rs         ← IslandTimer + TimerController
//   notifications.rs  ← IslandNotification + NotificationCenter
//   clock.rs          ← ClockService formatting helpers
//   pages/            ← per-page canvas painters
//   main.rs           ← iced application, canvas, subscriptions (this file)

#![windows_subsystem = "windows"]
#![allow(unused_variables)]
#![allow(dead_code)]

mod activities;
mod calendar_events;
mod clock;
mod downloads;
mod metrics;
mod notifications;
pub mod pages;
mod platform;
mod settings;
mod spring;
mod state;
mod timers;
mod win32;

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use chrono::{Datelike, Local};
use iced::{
    alignment,
    event::{self, Event},
    mouse,
    widget::canvas::{self, Action as CanvasAction, Event as CanvasEvent, Canvas, Geometry, Program},
    window,
    Color, Element, Point, Rectangle, Renderer, Size, Subscription, Task, Theme,
};
use iced::widget::{container, row};

use state::{Intent, Phase};
use spring::{Spring, SpringGroup};

// ── Palette (dark — matches IslandPalette.dark) ──────────────────────────────
// All constants are `pub` so page modules can reference `crate::TEXT_PRIMARY` etc.

pub const SURFACE: Color         = Color { r: 0.043, g: 0.047, b: 0.059, a: 0.96 };
pub const SURFACE_RAISED: Color  = Color { r: 0.100, g: 0.106, b: 0.126, a: 1.0  };
pub const SURFACE_SUNKEN: Color  = Color { r: 0.020, g: 0.022, b: 0.028, a: 1.0  };
pub const INNER_HIGHLIGHT: Color = Color { r: 1.0,   g: 1.0,   b: 1.0,   a: 0.10 };
pub const STROKE: Color          = Color { r: 1.0,   g: 1.0,   b: 1.0,   a: 0.07 };
pub const STROKE_STRONG: Color   = Color { r: 1.0,   g: 1.0,   b: 1.0,   a: 0.14 };
pub const TEXT_PRIMARY: Color    = Color { r: 0.957, g: 0.961, b: 0.969, a: 1.0  };
pub const TEXT_SECONDARY: Color  = Color { r: 0.612, g: 0.639, b: 0.686, a: 1.0  };
pub const TEXT_TERTIARY: Color   = Color { r: 0.400, g: 0.420, b: 0.460, a: 1.0  };
pub const ACCENT: Color          = Color { r: 0.765, g: 0.792, b: 0.839, a: 1.0  };
pub const COLOR_POSITIVE: Color  = Color { r: 0.380, g: 0.780, b: 0.520, a: 1.0  };
pub const COLOR_WARNING: Color   = Color { r: 0.980, g: 0.740, b: 0.330, a: 1.0  };

/// Accent presets mirrored from `db_colors.dart` (exact sRGB values).
pub const ACCENT_PRESETS: [(u8, u8, u8, &'static str); 6] = [
    (0xC3, 0xCA, 0xD6, "Graphite"),
    (0x3B, 0x8C, 0xFF, "Blue"),
    (0x9A, 0x7C, 0xFF, "Purple"),
    (0x35, 0xD0, 0x7F, "Emerald"),
    (0xF3, 0xB3, 0x3D, "Amber"),
    (0xFF, 0x5C, 0x63, "Crimson"),
];

/// Packed RGB of the active accent. Page painters are pure canvas functions,
/// so the selected preset is shared through an atomic rather than threaded
/// through every draw call.
static ACCENT_RGB: AtomicU32 = AtomicU32::new(0xC3CAD6);

/// Resolve the current accent as an iced colour.
pub fn accent() -> Color {
    let packed = ACCENT_RGB.load(Ordering::Relaxed);
    Color {
        r: ((packed >> 16) & 0xFF) as f32 / 255.0,
        g: ((packed >> 8) & 0xFF) as f32 / 255.0,
        b: (packed & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

/// Apply an accent preset index, clamping out-of-range values to graphite.
pub fn set_accent(index: usize) {
    let (r, g, b, _) = ACCENT_PRESETS.get(index).copied().unwrap_or(ACCENT_PRESETS[0]);
    ACCENT_RGB.store(((r as u32) << 16) | ((g as u32) << 8) | b as u32, Ordering::Relaxed);
}

pub fn accent_label(index: usize) -> &'static str {
    ACCENT_PRESETS.get(index).copied().unwrap_or(ACCENT_PRESETS[0]).3
}

/// Sentinel accent index meaning "use the user's custom RGB value".
pub const CUSTOM_ACCENT_INDEX: usize = ACCENT_PRESETS.len();

/// Apply either a preset or the stored custom colour to the painter palette.
pub fn apply_accent(index: usize, custom: Option<(u8, u8, u8)>) {
    if index == CUSTOM_ACCENT_INDEX {
        let (r, g, b) = custom.unwrap_or((0xC3, 0xCA, 0xD6));
        ACCENT_RGB.store(((r as u32) << 16) | ((g as u32) << 8) | b as u32, Ordering::Relaxed);
    } else {
        set_accent(index);
    }
}

/// Human-readable accent value for the settings row.
pub fn accent_display(index: usize, custom: Option<(u8, u8, u8)>) -> String {
    if index == CUSTOM_ACCENT_INDEX {
        let (r, g, b) = custom.unwrap_or((0xC3, 0xCA, 0xD6));
        format!("Custom #{r:02X}{g:02X}{b:02X}")
    } else {
        accent_label(index).to_string()
    }
}

/// Rotate a colour's hue by `degrees`, preserving saturation and value.
pub fn rotate_hue(rgb: (u8, u8, u8), degrees: f32) -> (u8, u8, u8) {
    let r = rgb.0 as f32 / 255.0;
    let g = rgb.1 as f32 / 255.0;
    let b = rgb.2 as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta <= 0.0001 {
        return rgb;
    }
    let mut hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } * 60.0;
    hue = (hue + degrees).rem_euclid(360.0);
    let saturation = delta / max;
    let value = max;

    let c = value * saturation;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = value - c;
    let (rp, gp, bp) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let conv = |v: f32| -> u8 { ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8 };
    (conv(rp), conv(gp), conv(bp))
}

/// Hit box for the first-run banner's dismiss button, in island-local pixels.
fn intro_button_rect(w: f32, h: f32) -> (f32, f32, f32, f32) {
    (w - 88.0, h - 40.0, 64.0, 18.0)
}

// ── Timing ───────────────────────────────────────────────────────────────────

/// Hover-open debounce (IslandCoordinator._hoverOpenDelay).
const HOVER_OPEN_DELAY:   Duration = Duration::from_millis(70);
/// Grace period before reverting peek after cursor leaves.
const HOVER_CANCEL_GRACE: Duration = Duration::from_millis(180);
/// Animation tick (~120 Hz keeps springs smooth and idle-free).
const TICK_INTERVAL:      Duration = Duration::from_millis(16);
/// Number of system-stat samples kept in rolling history.
const HISTORY_LEN: usize = 60;

// ── App state ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    // Core animation / input
    Tick(Instant),
    MouseEntered,
    MouseLeft,
    MouseMoved(Point),
    CanvasPressed(Point),
    CanvasWheel(f32),
    Clicked,
    Escape,

    // Pager
    PageLeft,
    PageRight,
    PageSelect(usize),
    PagerHoverEnter,
    PagerHoverExit,

    // Timer composer
    ComposerSetMinutes(u32),
    TimerStart,
    TimerStartStopwatch,
    TimerToggle(String),
    TimerReset(String),
    TimerRemove(String),
    TimerFocus(String),

    // Notifications
    DismissNotification,

    // Page actions
    CalendarMonth(i32),
    CalendarSelectDay(u32),
    ShortcutActivate(String),
    MediaControl(platform::MediaKey),
    ShelfSlot(usize),
    ShelfClear(usize),
    ShelfCopy(usize),
    ShelfOpen(usize),
    FilesDropped(Vec<std::path::PathBuf>),

    // Settings
    ToggleSettings,
    SettingToggle(&'static str, bool),
    SettingCycle(&'static str),

    // System stats
    SystemTick,

    // Application
    Quit,

    // Platform events (from mouse hook / hotkey / tray / SMTC)
    WindowOpened(window::Id),
    StageHwnd(isize),
    PlatformEvents,
}

/// Which list the Settings page is currently showing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    Main,
    Pages,
    Appearance,
    Backup,
}

impl SettingsSection {
    fn is_subpage(self) -> bool {
        self != Self::Main
    }

    fn canvas_code(self) -> u8 {
        match self {
            Self::Main => 0,
            Self::Pages => 1,
            Self::Appearance => 2,
            Self::Backup => 3,
        }
    }
}

/// Compact content shown on the collapsed pill for transient events.
/// Priority is notification > playing media > running timer > clock.
#[derive(Clone)]
struct TransientFace {
    title: String,
    subtitle: String,
    progress: Option<f32>,
    accent: Color,
}

struct NotchApp {
    // ── Island logic ──────────────────────────────────────────────────────────
    intent:  Intent,
    phase:   Phase,
    hovered: bool,
    focused: bool,

    // ── Spring animation ──────────────────────────────────────────────────────
    springs:   SpringGroup,
    animating: bool,
    last_tick: Instant,

    // ── Hover debounce ────────────────────────────────────────────────────────
    hover_enter_time: Option<Instant>,
    hover_leave_time: Option<Instant>,

    // ── Clock ─────────────────────────────────────────────────────────────────
    now: chrono::DateTime<Local>,
    clock_roll: clock::DigitRoll,

    // ── Canvas repaint cache ──────────────────────────────────────────────────
    island_cache: canvas::Cache,
    /// Last pager change, used for the short page cross-fade.
    page_changed_at: Instant,

    // ── Settings ──────────────────────────────────────────────────────────────
    settings: crate::settings::SettingsController,

    // ── Local calendar events (%APPDATA%\notchWin\calendar.json) ──────────────
    calendar: crate::calendar_events::CalendarSource,

    // ── Timers ────────────────────────────────────────────────────────────────
    timers: crate::timers::TimerController,

    // ── Notifications ─────────────────────────────────────────────────────────
    notifications: crate::notifications::NotificationCenter,

    // ── Live system status (volume, privacy, downloads) ───────────────────────
    volume:   platform::VolumeStatus,
    privacy:  platform::PrivacyStatus,
    network:  platform::NetworkStatus,
    downloads: crate::downloads::DownloadCenter,

    // ── System stats (polled at 1 Hz) ─────────────────────────────────────────
    cpu_history:      Vec<f32>,  // 0..1, last HISTORY_LEN samples
    mem_history:      Vec<f32>,
    net_down_history: Vec<f32>,  // bytes/s
    net_up_history:   Vec<f32>,
    cpu_pct:          Option<f32>,
    mem_pct:          Option<f32>,
    mem_used_gb:      f32,
    mem_total_gb:     f32,
    net_down_bps:     f32,
    net_up_bps:       f32,
    last_sys_sample:  Option<Instant>,
    last_cpu_times:   Option<(u64, u64)>, // (idle, total) for delta

    // ── Media (fed by the SMTC watcher in platform.rs) ────────────────────────
    media_state: Option<crate::pages::media::MediaState>,

    // ── Pager ─────────────────────────────────────────────────────────────────
    // 0=Now, 1=Media, 2=Timers, 3=System, 4=Calendar, 5=Shortcuts,
    // 6=Shelf, 7=Settings
    page_index:    usize,
    page_count:    usize,
    /// When on page 7, show the per-page visibility sub-page instead.
    settings_section: SettingsSection,
    settings_return_page: usize,
    pager_hovered: bool,
    hovered_shortcut: Option<String>,

    // ── Page state ────────────────────────────────────────────────────────────
    calendar_state: crate::pages::calendar::CalendarState,
    shelf_state:    crate::pages::shelf::ShelfState,

    // ── Win32 stage handle (kept as isize; HWND is !Send) ─────────────────────
    hwnd: Option<isize>,
    last_platform_poll: Instant,

    // ── Timer composer ────────────────────────────────────────────────────────
    composer_minutes: u32,

    // ── Auto-collapse ─────────────────────────────────────────────────────────
    last_interaction: Instant,

    // ── Win32 passthrough ─────────────────────────────────────────────────────
    passthrough: bool,

    // ── Platform subsystems (tray, hotkey, SMTC, mouse hook) ──────────────────
    // Option so Default can construct without an HWND; initialised in
    // WindowOpened once iced hands us the real window handle.
    platform: Option<platform::PlatformHandle>,

    // ── Auto-hide state ───────────────────────────────────────────────────────
    // True while the island is hidden *only* because auto-hide put it there,
    // i.e. a CursorNearTop(true) event should bring it back.
    hidden_for_auto_hide: bool,
    /// Hidden because a maximised/full-screen window owns the foreground.
    hidden_for_foreground: bool,
    /// Lock screen state from the Win32 session poll.
    session_locked: bool,
    last_visibility_poll: Instant,
}

impl Default for NotchApp {
    fn default() -> Self {
        let init_w  = metrics::COLLAPSED_W * 0.72;
        let springs = SpringGroup::new(init_w, 0.0, metrics::RADIUS_COLLAPSED_BOTTOM);
        let now     = Instant::now();
        let now_local = Local::now();
        let settings = crate::settings::SettingsController::load();
        apply_accent(settings.settings.accent, settings.settings.custom_accent);
        let initial_clock = if settings.settings.use_24_hour_clock {
            now_local.format("%H:%M").to_string()
        } else {
            now_local.format("%-I:%M").to_string()
        };
        let mut clock_roll = clock::DigitRoll::default();
        clock_roll.seed(&initial_clock);
        let calendar = crate::calendar_events::CalendarSource::load();
        let mut app = Self {
            intent:  Intent::Collapsed,
            phase:   Phase::Collapsed,
            hovered: false,
            focused: false,
            springs,
            animating: true,
            last_tick: now,
            hover_enter_time: None,
            hover_leave_time: None,
            now: now_local,
            clock_roll,
            island_cache: canvas::Cache::new(),
            page_changed_at: now,

            settings,
            calendar,
            timers:        crate::timers::TimerController::new(),
            notifications: crate::notifications::NotificationCenter::new(),

            volume:    platform::VolumeStatus::default(),
            privacy:   platform::PrivacyStatus::default(),
            network:   platform::NetworkStatus::default(),
            downloads: crate::downloads::DownloadCenter::new(),

            cpu_history:      Vec::with_capacity(HISTORY_LEN),
            mem_history:      Vec::with_capacity(HISTORY_LEN),
            net_down_history: Vec::with_capacity(HISTORY_LEN),
            net_up_history:   Vec::with_capacity(HISTORY_LEN),
            cpu_pct:          None,
            mem_pct:          None,
            mem_used_gb:      0.0,
            mem_total_gb:     0.0,
            net_down_bps:     0.0,
            net_up_bps:       0.0,
            last_sys_sample:  None,
            last_cpu_times:   None,

            media_state: None,

            page_index:       0,
            page_count:       8,
            settings_section: SettingsSection::Main,
            settings_return_page: 0,
            pager_hovered:    false,
            hovered_shortcut: None,
            calendar_state: pages::calendar::CalendarState {
                year:         now_local.year(),
                month:        now_local.month(),
                selected_day: now_local.day(),
                today_day:    now_local.day(),
                today_month:  now_local.month(),
                today_year:   now_local.year(),
                entries:      Vec::new(),
            },
            shelf_state: pages::shelf::ShelfState::load(),
            hwnd:               None,
            last_platform_poll: now,

            composer_minutes: 5,

            last_interaction: now,
            passthrough: false,
            platform: None,
            hidden_for_auto_hide: false,
            hidden_for_foreground: false,
            session_locked: false,
            last_visibility_poll: now,
        };
        app.refresh_calendar_entries();
        app
    }
}

impl NotchApp {
    // ── State transitions ─────────────────────────────────────────────────────

    fn set_intent(&mut self, next: Intent) {
        if self.intent == next { return; }
        self.intent = next;
        self.retarget_springs();
    }

    fn expand(&mut self) {
        if self.phase.is_open() { return; }
        self.set_intent(Intent::Expanded);
        #[cfg(target_os = "windows")]
        if let Some(raw) = self.hwnd {
            use windows::Win32::Foundation::HWND;
            crate::win32::activate_window(HWND(raw as *mut core::ffi::c_void));
        }
    }

    fn collapse(&mut self) {
        self.focused = false;
        self.set_intent(Intent::Collapsed);
    }

    /// Retarget all springs toward the geometry for the current intent/hover.
    fn retarget_springs(&mut self) {
        let geo = state::resolve_geometry(self.intent, self.hovered);

        let opening = geo.height > self.springs.height.value + 0.5
            || geo.expansion > self.springs.expansion.value + 0.01;
        let peeking = !matches!(self.intent, Intent::Expanded)
            && self.hovered
            && self.intent == Intent::Collapsed;
        let withdrawing = self.intent == Intent::Hidden;
        let returning = !withdrawing
            && !matches!(self.intent, Intent::Expanded)
            && self.springs.height.value < 1.0
            && geo.height > 1.0;

        let shape: Spring = if withdrawing {
            spring::profile::withdraw(0.0)
        } else if returning {
            spring::profile::reveal(0.0)
        } else if peeking {
            spring::profile::peek(0.0)
        } else if opening {
            spring::profile::arrival(0.0)
        } else {
            spring::profile::departure(0.0)
        };

        apply_profile(&mut self.springs.width,  &shape, geo.width);
        apply_profile(&mut self.springs.height, &shape, geo.height);

        let r_spring = spring::profile::contour(0.0);
        apply_profile(&mut self.springs.radius, &r_spring, geo.bottom_radius);

        let exp_spring = if opening {
            spring::profile::content_in(0.0)
        } else {
            spring::profile::content_out(0.0)
        };
        apply_profile(&mut self.springs.expansion, &exp_spring, geo.expansion);

        let atm = spring::profile::atmosphere(0.0);
        apply_profile(&mut self.springs.shadow,    &atm, geo.shadow);
        apply_profile(&mut self.springs.highlight, &atm, if self.hovered { 1.0 } else { 0.0 });

        self.animating = true;
        self.island_cache.clear();
        self.apply_window_focus();
        // Grow the clip region immediately when opening so the expanding pill
        // is never cut off. Shrinking is deferred until the phase settles.
        if opening {
            self.apply_target_region(geo);
        }
    }

    // ── Hover debounce ────────────────────────────────────────────────────────

    fn on_mouse_entered(&mut self) {
        self.hover_enter_time = Some(Instant::now());
        self.hover_leave_time = None;
    }

    fn on_mouse_left(&mut self) {
        self.hover_enter_time = None;
        self.hover_leave_time = Some(Instant::now());
    }

    fn update_hover_state(&mut self, now: Instant) {
        if let Some(t) = self.hover_enter_time {
            if now.duration_since(t) >= HOVER_OPEN_DELAY && !self.hovered {
                self.hovered = true;
                if !matches!(self.intent, Intent::Expanded) {
                    self.retarget_springs();
                }
            }
        }
        if let Some(t) = self.hover_leave_time {
            if now.duration_since(t) >= HOVER_CANCEL_GRACE && self.hovered {
                self.hovered = false;
                self.hover_leave_time = None;
                if !matches!(self.intent, Intent::Expanded) {
                    self.retarget_springs();
                }
            }
        }
    }

    // ── Phase derivation ──────────────────────────────────────────────────────

    fn refresh_phase(&mut self) {
        let p = self.springs.expansion.value;
        let v = self.springs.expansion.velocity;
        let next = state::derive_phase(self.intent, p, v, self.focused);
        if next != self.phase {
            let became_open = next.is_open() && !self.phase.is_open();
            let fully_closed = matches!(next, Phase::Collapsed | Phase::Hidden);
            self.phase = next;
            if became_open || fully_closed {
                self.apply_target_region(state::resolve_geometry(self.intent, self.hovered));
            }
            self.apply_window_focus();
        }
    }

    // ── System stat sampling ──────────────────────────────────────────────────

    fn sample_system_stats(&mut self) {
        // ── CPU via GetSystemTimes ────────────────────────────────────────────
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Threading::GetSystemTimes;
            use windows::Win32::Foundation::FILETIME;

            unsafe {
                let mut idle   = FILETIME::default();
                let mut kernel = FILETIME::default();
                let mut user   = FILETIME::default();

                if GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_ok() {
                    let to_u64 = |ft: &FILETIME| -> u64 {
                        ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
                    };

                    let idle_v   = to_u64(&idle);
                    let kernel_v = to_u64(&kernel); // includes idle
                    let user_v   = to_u64(&user);
                    let total_v  = kernel_v + user_v;

                    if let Some((prev_idle, prev_total)) = self.last_cpu_times {
                        let d_idle  = idle_v.saturating_sub(prev_idle) as f32;
                        let d_total = total_v.saturating_sub(prev_total) as f32;
                        if d_total > 0.0 {
                            let cpu = (1.0 - d_idle / d_total).clamp(0.0, 1.0);
                            self.cpu_pct = Some(cpu);
                            push_history(&mut self.cpu_history, cpu);
                        }
                    }
                    self.last_cpu_times = Some((idle_v, total_v));
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            push_history(&mut self.cpu_history, 0.0);
            self.cpu_pct = Some(0.0);
        }

        // ── Memory via GlobalMemoryStatusEx ───────────────────────────────────
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::SystemInformation::{
                GlobalMemoryStatusEx, MEMORYSTATUSEX,
            };

            unsafe {
                let mut ms = MEMORYSTATUSEX {
                    dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                    ..Default::default()
                };
                if GlobalMemoryStatusEx(&mut ms).is_ok() {
                    let used  = ms.ullTotalPhys.saturating_sub(ms.ullAvailPhys);
                    let total = ms.ullTotalPhys;
                    if total > 0 {
                        let frac = used as f32 / total as f32;
                        self.mem_pct      = Some(frac);
                        self.mem_used_gb  = used  as f32 / 1_073_741_824.0;
                        self.mem_total_gb = total as f32 / 1_073_741_824.0;
                        push_history(&mut self.mem_history, frac);
                    }
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            push_history(&mut self.mem_history, 0.0);
            self.mem_pct = Some(0.0);
        }

        // ── Network (stub — real sampling is a future phase) ─────────────────
        push_history(&mut self.net_down_history, 0.0);
        push_history(&mut self.net_up_history,   0.0);
        self.net_down_bps = 0.0;
        self.net_up_bps   = 0.0;
    }

    // ── Window style helpers ──────────────────────────────────────────────────

    fn apply_window_focus(&self) {
        #[cfg(target_os = "windows")]
        if let Some(raw) = self.hwnd {
            use windows::Win32::Foundation::HWND;
            let hwnd = HWND(raw as *mut core::ffi::c_void);
            // Clear NOACTIVATE as soon as the island becomes interactive, not
            // only after the expand spring has finished. Otherwise the click
            // that opens it is swallowed and the user has to Alt-Tab to it.
            let interactive = self.phase.is_open()
                || self.hovered
                || matches!(self.intent, Intent::Expanded);
            crate::win32::set_no_activate(hwnd, !interactive);
        }
    }

    /// Clip the stage to the *target* island shape. Called only when the intent
    /// or settled phase changes, so Windows never repaints 30x/second.
    fn apply_target_region(&self, geo: state::Geometry) {
        #[cfg(target_os = "windows")]
        if let Some(raw) = self.hwnd {
            use windows::Win32::Foundation::HWND;
            let s = self.settings.settings.island_scale.max(0.1);
            // Stage rect is physical pixels; canvas geometry is logical.
            let dpi = crate::win32::get_dpi(HWND(raw as *mut core::ffi::c_void));
            let d = (dpi as f32 / 96.0).max(0.5);
            let w = (geo.width * s * d).round() as i32;
            let h = (geo.height * s * d).round() as i32;
            let logical_x = (metrics::STAGE_W - geo.width * s) / 2.0;
            let x = (logical_x * d).round() as i32;
            let r = ((geo.bottom_radius * s).max(metrics::RADIUS_TOP * s) * d).round() as i32;
            crate::win32::set_island_region(
                HWND(raw as *mut core::ffi::c_void), x, 0, w, h, r,
            );
        }
    }

    /// Page indices that survive the user's per-page visibility switches.
    fn enabled_pages(&self) -> Vec<usize> {
        (0..self.page_count)
            .filter(|i| self.settings.settings.page_enabled(*i))
            .collect()
    }

    /// Change the visible page and restart the short cross-fade.
    fn set_page(&mut self, index: usize) {
        if self.page_index == index {
            return;
        }
        self.page_index = index;
        self.page_changed_at = Instant::now();
        self.island_cache.clear();
    }

    fn page_left(&mut self) {
        if self.settings_section.is_subpage() {
            self.settings_section = SettingsSection::Main;
            self.island_cache.clear();
            return;
        }
        let pages = self.enabled_pages();
        if let Some(pos) = pages.iter().position(|i| *i == self.page_index) {
            if pos > 0 {
                self.set_page(pages[pos - 1]);
            }
        }
    }

    fn page_right(&mut self) {
        let pages = self.enabled_pages();
        if let Some(pos) = pages.iter().position(|i| *i == self.page_index) {
            if pos + 1 < pages.len() {
                self.set_page(pages[pos + 1]);
            }
        }
    }

    fn open_settings(&mut self) {
        if self.page_index != 7 {
            self.settings_return_page = self.page_index;
            self.set_page(7);
        }
        self.settings_section = SettingsSection::Main;
        self.expand();
    }

    fn toggle_settings(&mut self) {
        if self.page_index == 7 {
            if self.settings_section.is_subpage() {
                self.settings_section = SettingsSection::Main;
            } else {
                let return_page = self.settings_return_page;
                self.set_page(return_page);
            }
            self.island_cache.clear();
        } else {
            self.open_settings();
        }
    }

    /// Apply a boolean setting by its settings-page key.
    fn set_bool_setting(&mut self, key: &str, value: bool) {
        let changed = match key {
            "clock24" | "24-hour clock" => self.settings.settings.use_24_hour_clock != value,
            "hover" | "Hover to peek" => self.settings.settings.hover_to_peek != value,
            "wheel" | "Wheel = volume" => self.settings.settings.wheel_adjusts_volume != value,
            "hud" | "Volume HUD" => self.settings.settings.volume_hud_enabled != value,
            "autohide" | "Auto-hide pill" => self.settings.settings.auto_hide != value,
            "covered" | "Hide when covered" => self.settings.settings.hide_when_covered != value,
            "intro" | "First-run tip" => self.settings.settings.intro_seen != value,
            "startup" | "Launch at login" => self.settings.settings.start_with_windows != value,
            _ => false,
        };
        if !changed { return; }
        // The registry write is the source of truth for this switch. Do not
        // persist a value we failed to apply.
        if matches!(key, "startup" | "Launch at login") && !platform::set_startup(value) {
            return;
        }
        self.settings.mutate(|s| match key {
            "clock24" | "24-hour clock" => s.use_24_hour_clock = value,
            "hover" | "Hover to peek" => s.hover_to_peek = value,
            "wheel" | "Wheel = volume" => s.wheel_adjusts_volume = value,
            "hud" | "Volume HUD" => s.volume_hud_enabled = value,
            "autohide" | "Auto-hide pill" => s.auto_hide = value,
            "covered" | "Hide when covered" => s.hide_when_covered = value,
            "intro" | "First-run tip" => s.intro_seen = value,
            "startup" | "Launch at login" => s.start_with_windows = value,
            _ => {}
        });
        self.island_cache.clear();
    }

    /// Advance a cyclic setting by its settings-page key.
    fn cycle_setting(&mut self, key: &str) {
        self.settings.mutate(|s| match key {
            "collapse" | "auto_collapse" | "Auto-collapse" => {
                s.auto_collapse_seconds = match s.auto_collapse_seconds {
                    0 => 3,
                    3 => 5,
                    5 => 8,
                    8 => 15,
                    _ => 0,
                };
            }
            "scale" | "island_scale" | "Island scale" => {
                s.island_scale = match s.island_scale {
                    v if v < 0.9 => 1.0,
                    v if v < 1.1 => 1.15,
                    v if v < 1.2 => 1.3,
                    _ => 0.85,
                };
            }
            "accent" | "Accent" => {
                s.accent = (s.accent + 1) % (ACCENT_PRESETS.len() + 1);
            }
            "hue" | "Accent hue" => {
                let base = s.custom_accent.unwrap_or((0x3B, 0x8C, 0xFF));
                s.custom_accent = Some(rotate_hue(base, 30.0));
                s.accent = CUSTOM_ACCENT_INDEX;
            }
            "opacity" | "Surface opacity" => {
                s.surface_opacity = match s.surface_opacity {
                    v if v > 0.95 => 0.85,
                    v if v > 0.80 => 0.70,
                    _ => 1.0,
                };
            }
            "corners" | "Corner radius" => {
                s.corner_scale = match s.corner_scale {
                    v if v > 1.2 => 0.7,
                    v if v > 0.9 => 1.25,
                    _ => 1.0,
                };
            }
            "motion" | "Motion" => {
                s.motion_intensity = match s.motion_intensity {
                    v if v <= 0.01 => 0.5,
                    v if v < 0.75 => 1.0,
                    v if v < 1.2 => 1.4,
                    _ => 0.0,
                };
            }
            "intro" | "First-run tip" => s.intro_seen = !s.intro_seen,
            _ => {}
        });
        // Keep the global painter palette in sync with the persisted choice.
        apply_accent(self.settings.settings.accent, self.settings.settings.custom_accent);
        self.island_cache.clear();
    }

    /// Rows currently shown on the Settings page.
    fn settings_rows_for(&self, section: SettingsSection) -> Vec<pages::settings::SettingsRow> {
        match section {
            SettingsSection::Main => pages::settings::settings_rows(&self.settings.settings),
            SettingsSection::Pages => pages::settings::page_rows(&self.settings.settings),
            SettingsSection::Appearance => pages::settings::appearance_rows(&self.settings.settings),
            SettingsSection::Backup => pages::settings::backup_rows(self.settings.has_export()),
        }
    }

    /// Handle a tap on one of the Settings rows.
    fn activate_settings_row(&mut self, section: SettingsSection, index: usize) {
        match section {
            SettingsSection::Pages => {
                if index + 1 < settings::PAGE_IDS.len() {
                    self.settings.mutate(|s| { s.toggle_page(index); });
                }
            }
            SettingsSection::Appearance => match index {
                0 => self.cycle_setting("accent"),
                1 => self.cycle_setting("hue"),
                2 => self.cycle_setting("opacity"),
                3 => self.cycle_setting("corners"),
                4 => self.cycle_setting("motion"),
                _ => {}
            },
            SettingsSection::Backup => match index {
                0 => { self.settings.export(); }
                1 => {
                    if self.settings.import() {
                        apply_accent(
                            self.settings.settings.accent,
                            self.settings.settings.custom_accent,
                        );
                        self.island_cache.clear();
                    }
                }
                2 => {
                    if self.calendar.write_samples() {
                        self.refresh_calendar_entries();
                    }
                }
                _ => {}
            },
            SettingsSection::Main => match index {
                0 => self.set_bool_setting("clock24", !self.settings.settings.use_24_hour_clock),
                1 => self.set_bool_setting("hover", !self.settings.settings.hover_to_peek),
                2 => self.set_bool_setting("wheel", !self.settings.settings.wheel_adjusts_volume),
                3 => self.set_bool_setting("hud", !self.settings.settings.volume_hud_enabled),
                4 => self.set_bool_setting("autohide", !self.settings.settings.auto_hide),
                5 => self.cycle_setting("collapse"),
                6 => self.cycle_setting("scale"),
                7 => self.cycle_setting("accent"),
                8 => self.cycle_setting("motion"),
                9 => {
                    self.settings_section = SettingsSection::Pages;
                    self.island_cache.clear();
                }
                10 => self.set_bool_setting("covered", !self.settings.settings.hide_when_covered),
                11 => self.set_bool_setting("startup", !self.settings.settings.start_with_windows),
                12 => self.set_bool_setting("intro", !self.settings.settings.intro_seen),
                13 => {
                    self.settings_section = SettingsSection::Appearance;
                    self.island_cache.clear();
                }
                14 => {
                    self.settings_section = SettingsSection::Backup;
                    self.island_cache.clear();
                }
                _ => {}
            },
        }
    }

    /// Store a dropped file in the next free shelf slot (or replace the
    /// currently selected one). The label doubles as the shell target.
    fn accept_dropped_file(&mut self, path: std::path::PathBuf) {
        let label = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let target = path.to_string_lossy().into_owned();

        let target_slot = if self.shelf_state.slot_a_label.is_none() {
            0
        } else if self.shelf_state.slot_b_label.is_none() {
            1
        } else {
            self.page_index % 2
        };
        if target_slot == 0 {
            self.shelf_state.slot_a_label = Some(label);
            self.shelf_state.slot_a_path = Some(target);
        } else {
            self.shelf_state.slot_b_label = Some(label);
            self.shelf_state.slot_b_path = Some(target);
        }
        self.shelf_state.save();
        self.island_cache.clear();
    }

    // ── Canvas hit-testing ────────────────────────────────────────────────────

    /// Island size in *rendered* pixels (spring geometry × user scale).
    fn island_size(&self) -> (f32, f32) {
        let s = self.settings.settings.island_scale;
        (
            (self.springs.width.value * s).max(0.0),
            (self.springs.height.value * s).max(0.0),
        )
    }

    /// Translate a canvas point into pager-content coordinates, matching the
    /// padding used in `IslandCanvas::draw_expanded_face`.
    fn page_point(&self, p: Point) -> Option<(f32, f32, f32, f32)> {
        if !self.phase.is_open() {
            return None;
        }
        let (iw, ih) = self.island_size();
        const PAD_X: f32 = 20.0;
        const PAD_Y: f32 = 16.0;
        // Bottom reservation = lower padding (16) + pager-dot row (20).
        const DOTS_H: f32 = 36.0;
        if p.x < PAD_X || p.x > iw - PAD_X || p.y < PAD_Y || p.y > ih - DOTS_H {
            return None;
        }
        Some((p.x - PAD_X, p.y - PAD_Y, iw - PAD_X * 2.0, ih - PAD_Y - DOTS_H))
    }

    fn shortcut_at(&self, canvas_point: Point) -> Option<String> {
        let (x, y, w, _) = self.page_point(canvas_point)?;
        let ids = &self.settings.settings.pinned_shortcuts;
        let cols = (((w - 16.0) / 72.0).floor() as usize).max(1);
        for (i, id) in ids.iter().enumerate() {
            let sx = 12.0 + (i % cols) as f32 * 72.0;
            let sy = 12.0 + (i / cols) as f32 * 72.0;
            if x >= sx && x <= sx + 64.0 && y >= sy && y <= sy + 64.0 {
                return Some(id.clone());
            }
        }
        None
    }

    fn handle_canvas_press(&mut self, p: Point) -> Task<Message> {
        if !self.phase.is_open() {
            self.expand();
            return Task::none();
        }

        let (iw, ih) = self.island_size();

        // Chevrons fade in under the pointer; keep their hit boxes generous.
        let pages = self.enabled_pages();
        if p.x <= 26.0 && (p.y - ih / 2.0).abs() <= 16.0 {
            self.page_left();
            return Task::none();
        }
        if p.x >= iw - 26.0 && (p.y - ih / 2.0).abs() <= 16.0 {
            if pages.last() != Some(&self.page_index) {
                self.page_right();
            }
            return Task::none();
        }

        // Page dots at y = h - 10 (see `draw_pager_dots`). Only enabled pages
        // are shown, so the hit boxes must use the same filtered list.
        let active_w = 14.0_f32;
        let inactive_w = 4.0_f32;
        let gap = 5.0_f32;
        let n = pages.len().max(1) as f32;
        let total_w = active_w + (n - 1.0) * (inactive_w + gap);
        let mut cursor_x = (iw - total_w) / 2.0;
        if (ih - 14.0..=ih - 6.0).contains(&p.y) {
            for i in pages {
                let dw = if i == self.page_index { active_w } else { inactive_w };
                if p.x >= cursor_x && p.x <= cursor_x + dw {
                    self.set_page(i);
                    self.settings_section = SettingsSection::Main;
                    return Task::none();
                }
                cursor_x += dw + gap;
            }
        }

        // First-run banner dismiss button, just above the pager dots.
        if !self.settings.settings.intro_seen {
            let (bx, by, bw, bh) = intro_button_rect(iw, ih);
            if p.x >= bx && p.x <= bx + bw && p.y >= by && p.y <= by + bh {
                self.set_bool_setting("intro", true);
                return Task::none();
            }
        }

        let Some((x, y, w, h)) = self.page_point(p) else {
            return Task::none();
        };

        match self.page_index {
            // Now — no actions yet.
            0 => {}

            // Media: tap the seek bar to jump, then the transport controls.
            1 => {
                if let Some(media) = self.media_state.as_ref() {
                    if media.duration_secs > 0.0 {
                        let seek_x = 102.0_f32;
                        let seek_w = w - 118.0;
                        let seek_y = 74.0_f32;
                        if seek_w > 0.0
                            && (seek_y - 6.0..=seek_y + 8.0).contains(&y)
                            && (seek_x..=seek_x + seek_w).contains(&x)
                        {
                            let ratio = ((x - seek_x) / seek_w).clamp(0.0, 1.0);
                            platform::media_seek(ratio * media.duration_secs);
                            return Task::none();
                        }
                    }
                }
                let ctrl_y = 86.0;
                let spacing = (w - 118.0) / 4.0;
                if (ctrl_y - 12.0..=ctrl_y + 18.0).contains(&y) {
                    let idx = ((x - 102.0) / spacing) as i32;
                    let action = match idx {
                        0 => Some(platform::MediaKey::Previous),
                        1 => Some(platform::MediaKey::PlayPause),
                        2 => Some(platform::MediaKey::Next),
                        _ => None,
                    };
                    if let Some(action) = action {
                        platform::media_key(action);
                    }
                }
            }

            // Timer composer, primary timer controls and secondary slots.
            2 => {
                const PRESETS: [u32; 7] = [1, 3, 5, 10, 15, 25, 45];
                let cols = (((w / 2.0 - 24.0) / 38.0).floor() as usize).max(1);
                for (i, mins) in PRESETS.iter().enumerate() {
                    let px = w / 2.0 + 12.0 + (i % cols) as f32 * 38.0;
                    let py = 38.0 + (i / cols) as f32 * 26.0;
                    if x >= px && x <= px + 32.0 && y >= py && y <= py + 20.0 {
                        self.composer_minutes = *mins;
                        return Task::none();
                    }
                }
                let rows = PRESETS.len().div_ceil(cols);
                let sx = w / 2.0 + 12.0;
                let sy = 46.0 + 26.0 * rows as f32;
                if x >= sx && x <= sx + (w / 2.0 - 26.0) && y >= sy && y <= sy + 24.0 {
                    let id = self.timers.add_countdown(
                        Duration::from_secs(self.composer_minutes as u64 * 60),
                        &format!("{}m timer", self.composer_minutes),
                    );
                    self.timers.toggle(&id);
                    self.timers.focus(&id);
                    return Task::none();
                }

                let (primary, others) = self.build_timer_displays();
                let ring_area_h = if others.is_empty() { h } else { h - 40.0 };
                let ry = (ring_area_h / 2.0 - 38.0).max(14.0);
                let by = ry + 42.0;
                let right_x = w / 2.0 + 12.0;
                let mut targets: Vec<(String, usize)> = Vec::new();
                if let Some(ref p) = primary {
                    targets.push((p.id.clone(), 0));
                }
                for (i, t) in others.iter().enumerate() {
                    let slot_w = (w - 20.0) / others.len().min(3) as f32;
                    let sx = 10.0 + i as f32 * slot_w;
                    if i < 3 && x >= sx && x <= sx + slot_w && y >= ring_area_h + 6.0 {
                        self.timers.focus(&t.id);
                        return Task::none();
                    }
                }
                if let Some((id, _)) = targets.first() {
                    if x >= right_x && y >= by && y <= by + 22.0 {
                        let idx = ((x - right_x) / 34.0) as usize;
                        match idx {
                            0 => self.timers.toggle(id),
                            1 => self.timers.reset(id),
                            2 => self.timers.remove(id),
                            _ => {}
                        }
                    }
                }
            }

            // System — display only.
            3 => {}

            // Calendar: month chevrons, then day cells.
            4 => {
                let c = &self.calendar_state;
                if (0.0..=20.0).contains(&y) {
                    if (2.0..=22.0).contains(&x) {
                        self.shift_month(-1);
                        return Task::none();
                    }
                    let grid_w = w * 0.44;
                    if (grid_w - 22.0..=grid_w - 2.0).contains(&x) {
                        self.shift_month(1);
                        return Task::none();
                    }
                }
                let grid_w = w * 0.44;
                let cell_w = (grid_w - 12.0) / 7.0;
                let first = pages::calendar::weekday_of_first(c.year, c.month);
                let days = pages::calendar::days_in_month(c.year, c.month);
                for day in 1..=days {
                    let idx = first + day as usize - 1;
                    let cx = 6.0 + (idx % 7) as f32 * cell_w + cell_w / 2.0;
                    let cy = 56.0 + (idx / 7) as f32 * 20.0;
                    if (cx - 9.0..=cx + 9.0).contains(&x)
                        && (cy - 10.0..=cy + 10.0).contains(&y)
                    {
                        self.calendar_state.selected_day = day;
                        self.refresh_calendar_entries();
                        return Task::none();
                    }
                }
            }

            // Shortcuts: open the shell target for the tapped tile.
            5 => {
                if let Some(id) = self.shortcut_at(p) {
                    if let Some(target) = pages::shortcuts::target_for(&id) {
                        platform::shell_open(target);
                    }
                }
            }

            // Shelf: slot buttons and the three actions under each slot.
            6 => {
                let slot_w = (w - 38.0) / 2.0;
                let slot_h = 0.52 * (h - 24.0);
                for slot in 0..2 {
                    let sx = if slot == 0 { 14.0 } else { (w + 10.0) / 2.0 };
                    if x >= sx && x <= sx + slot_w && y >= 12.0 && y <= 12.0 + slot_h {
                        // A click on a filled slot opens the stored image.
                        let _ = Message::ShelfSlot(slot);
                        self.shelf_open(slot);
                        return Task::none();
                    }
                    let action_y = 16.0 + slot_h;
                    if (action_y..=action_y + 18.0).contains(&y)
                        && x >= sx
                        && x <= sx + slot_w
                    {
                        let third = slot_w / 3.0;
                        let which = ((x - sx) / third) as usize;
                        match which {
                            0 => self.shelf_copy(slot),
                            1 => self.shelf_open(slot),
                            2 => {
                                self.shelf_state = match slot {
                                    0 => pages::shelf::ShelfState {
                                        slot_a_label: None,
                                        slot_a_path: None,
                                        ..self.shelf_state.clone()
                                    },
                                    _ => pages::shelf::ShelfState {
                                        slot_b_label: None,
                                        slot_b_path: None,
                                        ..self.shelf_state.clone()
                                    },
                                };
                                self.shelf_state.save();
                            }
                            _ => {}
                        }
                        return Task::none();
                    }
                }
            }

            // Settings rows, including the visibility/appearance/backup sub-pages.
            7 => {
                let section = self.settings_section;
                let rows = self.settings_rows_for(section);
                for i in 0..rows.len() {
                    let (rx, ry, rw, rh) = pages::settings::row_rect(w, h, i, rows.len());
                    if x >= rx && x <= rx + rw && y >= ry && y <= ry + rh {
                        self.activate_settings_row(section, i);
                        return Task::none();
                    }
                }
            }

            _ => {}
        }
        Task::none()
    }

    /// Re-read the local event file and fill the selected day's entry list.
    fn refresh_calendar_entries(&mut self) {
        self.calendar.reload();
        let (y, m, d) = (
            self.calendar_state.year,
            self.calendar_state.month,
            self.calendar_state.selected_day,
        );
        self.calendar_state.entries = self
            .calendar
            .events_for(y, m, d)
            .into_iter()
            .map(|e| pages::calendar::CalEntry {
                time_label: e.time,
                text: e.title,
                notified: e.notified,
            })
            .collect();
        self.island_cache.clear();
    }

    fn shift_month(&mut self, delta: i32) {
        let mut y = self.calendar_state.year;
        let mut m = self.calendar_state.month as i32 + delta;
        if m < 1 {
            m += 12;
            y -= 1;
        } else if m > 12 {
            m -= 12;
            y += 1;
        }
        self.calendar_state.year = y;
        self.calendar_state.month = m as u32;
        self.calendar_state.selected_day = 1;
        self.refresh_calendar_entries();
    }

    fn shelf_target(&self, slot: usize) -> Option<&str> {
        let (path, label) = if slot == 0 {
            (
                self.shelf_state.slot_a_path.as_deref(),
                self.shelf_state.slot_a_label.as_deref(),
            )
        } else {
            (
                self.shelf_state.slot_b_path.as_deref(),
                self.shelf_state.slot_b_label.as_deref(),
            )
        };
        path.or(label)
    }

    fn shelf_open(&self, slot: usize) {
        if let Some(path) = self.shelf_target(slot) {
            platform::shell_open(path);
        }
    }

    fn shelf_copy(&self, slot: usize) {
        if let Some(path) = self.shelf_target(slot) {
            platform::copy_text_to_clipboard(path);
        }
    }

    // ── Build canvas snapshot types from app state ────────────────────────────

    /// Build the `TimerDisplay` list that the timers page painter expects.
    fn build_timer_displays(&self) -> (Option<pages::timers_page::TimerDisplay>, Vec<pages::timers_page::TimerDisplay>) {
        use crate::timers::{TimerKind, TimerState};
        use crate::clock::ClockService;
        use crate::pages::timers_page::TimerDisplay;

        let now = Instant::now();
        let timers = self.timers.timers();

        let make = |t: &crate::timers::IslandTimer| TimerDisplay {
            id:       t.id.clone(),
            label:    t.label.clone(),
            readout:  match t.kind {
                TimerKind::Countdown => ClockService::format_countdown(t.remaining_at(now)),
                TimerKind::Stopwatch => ClockService::format_stopwatch(t.elapsed_at(now)),
            },
            progress: t.progress_at(now),
            sweep: if t.kind == TimerKind::Stopwatch {
                let elapsed_secs = t.elapsed_at(now).as_secs_f32();
                Some((elapsed_secs % 60.0) / 60.0)
            } else {
                None
            },
            is_running:         t.state == TimerState::Running,
            is_finished:        t.state == TimerState::Finished,
            kind_is_countdown:  t.kind == TimerKind::Countdown,
        };

        let primary_idx = self.timers.primary().map(|p| p.id.clone());

        let mut primary: Option<TimerDisplay> = None;
        let mut others:  Vec<TimerDisplay>    = Vec::new();

        for t in timers {
            if Some(&t.id) == primary_idx.as_ref() {
                primary = Some(make(t));
            } else {
                others.push(make(t));
            }
        }

        (primary, others)
    }

}

// ── History helper ────────────────────────────────────────────────────────────

fn push_history(v: &mut Vec<f32>, value: f32) {
    if v.len() >= HISTORY_LEN {
        v.remove(0);
    }
    v.push(value);
}

// ── iced application ─────────────────────────────────────────────────────────

fn update(app: &mut NotchApp, message: Message) -> Task<Message> {
    match message {
        // ── Core animation tick ───────────────────────────────────────────────
        Message::Tick(now) => {
            let dt = now.duration_since(app.last_tick).as_secs_f32();
            app.last_tick = now;

            app.update_hover_state(now);

            // Tick timers (marks finished countdowns).
            app.timers.tick(now);

            // Retire short-lived notices (volume HUD, download alerts).
            if app.notifications.expire(now) {
                app.island_cache.clear();
            }

            // Keep repainting while the page cross-fade is in progress.
            if now.saturating_duration_since(app.page_changed_at)
                < Duration::from_millis(220)
            {
                app.island_cache.clear();
            }

            if app.animating {
                let motion = app.settings.settings.motion_intensity.clamp(0.0, 1.4);
                if motion <= 0.01 {
                    // "off" snaps immediately instead of freezing mid-move.
                    app.springs.width.finish();
                    app.springs.height.finish();
                    app.springs.radius.finish();
                    app.springs.expansion.finish();
                    app.springs.shadow.finish();
                    app.springs.highlight.finish();
                    app.animating = false;
                } else {
                    app.animating = app.springs.tick(dt.min(0.05) * motion);
                }
                app.refresh_phase();
                app.island_cache.clear();
            }

            // Refresh clock on second boundary. Only digits that actually
            // changed roll, so the colon and unchanged neighbours stay still.
            let new_now = Local::now();
            let clock_text = if app.settings.settings.use_24_hour_clock {
                new_now.format("%H:%M").to_string()
            } else {
                new_now.format("%-I:%M").to_string()
            };
            if new_now.timestamp() != app.now.timestamp() {
                app.now = new_now;
                app.clock_roll.update(&clock_text, now);
                app.island_cache.clear();
            } else if app.clock_roll.progress(now) < 1.0 {
                app.island_cache.clear();
            }

            // Auto-collapse.
            let auto_secs = app.settings.settings.auto_collapse_seconds;
            if auto_secs > 0 && app.phase.is_open() {
                let idle = now.duration_since(app.last_interaction);
                if idle >= Duration::from_secs(auto_secs as u64) {
                    app.collapse();
                }
            }

            // Foreground / lock-screen visibility. Polled at 4 Hz: the iced
            // event stream does not expose WM_WTSSESSION_CHANGE or
            // SetWinEventHook foreground notifications.
            if now.duration_since(app.last_visibility_poll) >= Duration::from_millis(250) {
                app.last_visibility_poll = now;
                let locked = platform::session_locked();
                if locked != app.session_locked {
                    app.session_locked = locked;
                    if locked {
                        platform::reassert_stage();
                        app.set_intent(state::Intent::Hidden);
                    } else if !app.hidden_for_auto_hide && !app.hidden_for_foreground {
                        app.set_intent(state::Intent::Collapsed);
                    }
                }
                if !app.session_locked && app.settings.settings.hide_when_covered {
                    let blocked = platform::foreground_blocks_overlay();
                    if blocked && !app.hidden_for_foreground {
                        app.hidden_for_foreground = true;
                        if app.intent != state::Intent::Hidden {
                            app.set_intent(state::Intent::Hidden);
                        }
                    } else if !blocked && app.hidden_for_foreground {
                        app.hidden_for_foreground = false;
                        if !app.hidden_for_auto_hide {
                            app.set_intent(state::Intent::Collapsed);
                        }
                    }
                }
            }

            // Auto-hide: once collapsed and idle for 3× the collapse window,
            // withdraw into the bezel until the cursor approaches.
            if app.settings.settings.auto_hide
                && !app.hidden_for_auto_hide
                && !app.hidden_for_foreground
                && !app.session_locked
                && app.intent == state::Intent::Collapsed
                && !app.hovered
            {
                let idle = now.duration_since(app.last_interaction);
                let hide_after = Duration::from_secs(
                    (app.settings.settings.auto_collapse_seconds.max(2) as u64) * 3,
                );
                if idle >= hide_after {
                    app.set_intent(state::Intent::Hidden);
                    app.hidden_for_auto_hide = true;
                }
            }

            // Poll the platform channels and refresh the mouse-hook rects at
            // ~30 Hz. Every poll is a separate iced message, so doing it on
            // all 120 animation ticks would triple view rebuilds and issue
            // needless Win32 calls for no latency benefit.
            if app.platform.is_some()
                && now.duration_since(app.last_platform_poll) >= Duration::from_millis(32)
            {
                app.last_platform_poll = now;
                if let Some(ph) = app.platform.as_ref() {
                    let (iw, ih) = app.island_size();
                    let ix = ((metrics::STAGE_W - iw) / 2.0).max(0.0);
                    platform::sync_rects(&ph.shared, ix, 0.0, iw, ih);
                }
                return Task::done(Message::PlatformEvents);
            }
        }

        // ── System stats 1 Hz tick ────────────────────────────────────────────
        Message::SystemTick => {
            app.sample_system_stats();
            app.last_sys_sample = Some(Instant::now());
            if app.phase.is_open() {
                app.island_cache.clear();
            }
        }

        // ── Mouse ─────────────────────────────────────────────────────────────
        Message::MouseEntered => {
            app.on_mouse_entered();
            app.last_interaction = Instant::now();
            if app.phase.is_open() {
                app.pager_hovered = true;
            }
            app.apply_window_focus();
        }

        Message::MouseLeft => {
            app.on_mouse_left();
            app.pager_hovered = false;
            app.hovered_shortcut = None;
            app.island_cache.clear();
        }

        Message::MouseMoved(pos) => {
            // Hover feedback inside the pager is drawn on the canvas, so it
            // needs a repaint only when the highlighted target actually moves.
            if app.phase.is_open() && app.page_index == 5 {
                let hovered = app.shortcut_at(pos);
                if hovered != app.hovered_shortcut {
                    app.hovered_shortcut = hovered;
                    app.island_cache.clear();
                }
            }
        }

        Message::CanvasPressed(pos) => {
            let task = app.handle_canvas_press(pos);
            app.last_interaction = Instant::now();
            app.island_cache.clear();
            return task;
        }

        Message::CanvasWheel(delta) => {
            if app.phase.is_open() {
                // Expanded: the wheel pages through modules.
                if delta > 0.0 {
                    app.page_right();
                } else if delta < 0.0 {
                    app.page_left();
                }
            } else if app.settings.settings.wheel_adjusts_volume {
                platform::volume_step(delta);
            }
            app.last_interaction = Instant::now();
            app.island_cache.clear();
        }

        Message::Clicked => {
            app.last_interaction = Instant::now();
            if !app.phase.is_open() {
                app.expand();
            }
        }

        Message::Escape => {
            if app.phase.is_open() {
                app.collapse();
            }
        }

        // ── Pager ─────────────────────────────────────────────────────────────
        Message::PageLeft => {
            if app.page_index > 0 {
                let target = app.page_index - 1;
                app.set_page(target);
            }
            app.last_interaction = Instant::now();
        }

        Message::PageRight => {
            if app.page_index + 1 < app.page_count {
                let target = app.page_index + 1;
                app.set_page(target);
            }
            app.last_interaction = Instant::now();
        }

        Message::PageSelect(idx) => {
            if idx < app.page_count && app.settings.settings.page_enabled(idx) {
                app.set_page(idx);
                app.settings_section = SettingsSection::Main;
            }
            app.last_interaction = Instant::now();
        }

        Message::PagerHoverEnter => {
            app.pager_hovered = true;
            app.island_cache.clear();
        }

        Message::PagerHoverExit => {
            app.pager_hovered = false;
            app.island_cache.clear();
        }

        // ── Timer composer ────────────────────────────────────────────────────
        Message::ComposerSetMinutes(m) => {
            app.composer_minutes = m.max(1);
            app.last_interaction = Instant::now();
        }

        Message::TimerStart => {
            let dur   = Duration::from_secs(app.composer_minutes as u64 * 60);
            let label = format!("{}m timer", app.composer_minutes);
            let id    = app.timers.add_countdown(dur, &label);
            app.timers.toggle(&id);
            app.timers.focus(&id);
            app.last_interaction = Instant::now();
        }

        Message::TimerStartStopwatch => {
            let id = app.timers.add_stopwatch();
            app.timers.toggle(&id);
            app.timers.focus(&id);
            app.last_interaction = Instant::now();
        }

        Message::TimerToggle(id) => {
            app.timers.toggle(&id);
            app.last_interaction = Instant::now();
        }

        Message::TimerReset(id) => {
            app.timers.reset(&id);
            app.last_interaction = Instant::now();
        }

        Message::TimerRemove(id) => {
            app.timers.remove(&id);
            app.last_interaction = Instant::now();
        }

        Message::TimerFocus(id) => {
            app.timers.focus(&id);
            app.last_interaction = Instant::now();
        }

        // ── Notifications ─────────────────────────────────────────────────────
        Message::DismissNotification => {
            app.notifications.dismiss_current();
            app.island_cache.clear();
        }

        // ── Page actions ───────────────────────────────────────────────────────
        Message::CalendarMonth(delta) => {
            app.shift_month(delta);
            app.last_interaction = Instant::now();
            app.island_cache.clear();
        }

        Message::CalendarSelectDay(day) => {
            let days = pages::calendar::days_in_month(
                app.calendar_state.year,
                app.calendar_state.month,
            );
            if day >= 1 && day <= days {
                app.calendar_state.selected_day = day;
                app.refresh_calendar_entries();
            }
            app.last_interaction = Instant::now();
        }

        Message::ShortcutActivate(id) => {
            if let Some(target) = pages::shortcuts::target_for(&id) {
                platform::shell_open(target);
            }
            app.last_interaction = Instant::now();
        }

        Message::MediaControl(key) => {
            platform::media_key(key);
            app.last_interaction = Instant::now();
        }

        Message::ShelfSlot(slot) => {
            app.shelf_open(slot);
            app.last_interaction = Instant::now();
        }

        Message::ShelfClear(slot) => {
            if slot == 0 {
                app.shelf_state.slot_a_label = None;
                app.shelf_state.slot_a_path = None;
            } else {
                app.shelf_state.slot_b_label = None;
                app.shelf_state.slot_b_path = None;
            }
            app.shelf_state.save();
            app.island_cache.clear();
        }

        Message::ShelfCopy(slot) => {
            app.shelf_copy(slot);
            app.last_interaction = Instant::now();
        }

        Message::ShelfOpen(slot) => {
            app.shelf_open(slot);
            app.last_interaction = Instant::now();
        }

        Message::FilesDropped(paths) => {
            for path in paths {
                app.accept_dropped_file(path);
            }
            app.last_interaction = Instant::now();
        }

        // ── Settings ───────────────────────────────────────────────────────────
        Message::ToggleSettings => {
            app.toggle_settings();
            app.last_interaction = Instant::now();
        }

        Message::SettingToggle(key, value) => {
            app.set_bool_setting(key, value);
            app.last_interaction = Instant::now();
        }

        Message::SettingCycle(key) => {
            app.cycle_setting(key);
            app.last_interaction = Instant::now();
        }

        // ── Application ───────────────────────────────────────────────────────
        Message::Quit => {
            return iced::exit();
        }

        // ── Window ready: ask iced for the real HWND ──────────────────────────
        Message::WindowOpened(win_id) => {
            // `window::run` hands us the live iced window on the UI thread.
            // RawWindowHandle is the only supported way to reach its HWND.
            return window::run(win_id, |win| {
                use window::raw_window_handle::RawWindowHandle;
                win.window_handle()
                    .ok()
                    .and_then(|handle| match handle.as_raw() {
                        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
                        _ => None,
                    })
                    .unwrap_or(0)
            })
            .map(Message::StageHwnd);
        }

        Message::StageHwnd(raw) => {
            if raw == 0 || app.platform.is_some() {
                return Task::none();
            }
            app.hwnd = Some(raw);
            #[cfg(target_os = "windows")]
            {
                use windows::Win32::Foundation::HWND;
                let hwnd = HWND(raw as *mut core::ffi::c_void);
                app.platform = Some(platform::start(hwnd));
                // Keep the persisted switch and the real Run key in sync after
                // reinstalls or a moved executable.
                let _ = platform::set_startup(app.settings.settings.start_with_windows);
                app.apply_window_focus();
                app.apply_target_region(state::resolve_geometry(Intent::Collapsed, false));
            }
        }

        // ── Drain platform events (hotkey / tray / SMTC / mouse hook) ─────────
        Message::PlatformEvents => {
            // Drain the queue into a local vec first so the receiver borrow
            // ends before we mutate the rest of `app`.
            let pending: Vec<platform::PlatformEvent> = app
                .platform
                .as_ref()
                .map(|ph| {
                    let mut out = Vec::new();
                    while let Ok(ev) = ph.events.try_recv() {
                        out.push(ev);
                    }
                    out
                })
                .unwrap_or_default();

            for ev in pending {
                    match ev {
                        platform::PlatformEvent::HotkeyToggle => {
                            if app.phase.is_open() {
                                app.collapse();
                            } else {
                                app.expand();
                            }
                        }
                        platform::PlatformEvent::HotkeyExpand => {
                            app.expand();
                        }
                        platform::PlatformEvent::HotkeyHide => {
                            app.set_intent(state::Intent::Hidden);
                        }
                        platform::PlatformEvent::TrayOpen => {
                            app.set_intent(state::Intent::Collapsed);
                            app.hidden_for_auto_hide = false;
                        }
                        platform::PlatformEvent::TrayQuit => {
                            return iced::exit();
                        }
                        platform::PlatformEvent::TrayPause => {
                            app.set_intent(state::Intent::Hidden);
                        }
                        platform::PlatformEvent::TraySettings => {
                            app.open_settings();
                        }
                        platform::PlatformEvent::OutsideClick => {
                            if app.phase.is_open() {
                                app.collapse();
                            }
                        }
                        platform::PlatformEvent::CursorNearTop(near) => {
                            if near && app.settings.settings.auto_hide
                                && app.hidden_for_auto_hide
                            {
                                // Reveal: drop back down to collapsed.
                                app.set_intent(state::Intent::Collapsed);
                                app.hidden_for_auto_hide = false;
                            }
                        }
                        platform::PlatformEvent::MediaChanged(info) => {
                            app.media_state = info.map(|i| pages::media::MediaState {
                                title:         i.title,
                                artist:        i.artist,
                                album:         i.album,
                                app_name:      i.app_name,
                                is_playing:    i.is_playing,
                                position_secs: i.position_secs,
                                duration_secs: i.duration_secs,
                                art_color:     None,
                            });
                            app.island_cache.clear();
                        }
                        platform::PlatformEvent::VolumeChanged { available, level, muted } => {
                            app.volume = platform::VolumeStatus { available, level, muted };
                            if available && app.settings.settings.volume_hud_enabled {
                                use crate::notifications::{IslandNotification, NotificationKind};
                                app.notifications.post_timed(
                                    IslandNotification {
                                        id: "system.volume".into(),
                                        title: if muted {
                                            "Muted".into()
                                        } else {
                                            format!("Volume {}%", (level * 100.0).round() as u32)
                                        },
                                        body: String::new(),
                                        kind: NotificationKind::System,
                                        progress: Some(if muted { 0.0 } else { level }),
                                        accent: None,
                                    },
                                    Duration::from_millis(1400),
                                );
                            }
                            app.island_cache.clear();
                        }
                        platform::PlatformEvent::PrivacyChanged { microphone, camera } => {
                            app.privacy = platform::PrivacyStatus { microphone, camera };
                            app.island_cache.clear();
                        }
                        platform::PlatformEvent::NetworkChanged { connected, ssid } => {
                            app.network = platform::NetworkStatus { connected, ssid };
                            app.island_cache.clear();
                        }
                        platform::PlatformEvent::Download(ev) => match ev {
                            platform::DownloadEvent::Started { id, name } => {
                                app.downloads.started(id, name);
                                app.island_cache.clear();
                            }
                            platform::DownloadEvent::Progress { id, name, bytes, bytes_per_second } => {
                                app.downloads.progress(&id, name, bytes, bytes_per_second);
                                app.island_cache.clear();
                            }
                            platform::DownloadEvent::Finished { id, name, completed } => {
                                use crate::notifications::{IslandNotification, NotificationKind};
                                app.downloads.finished(&id, name.clone(), completed);
                                app.notifications.post_timed(
                                    IslandNotification {
                                        id: format!("downloads.{}", id),
                                        title: if completed { "Download complete" } else { "Download stopped" }.into(),
                                        body: name,
                                        kind: if completed { NotificationKind::Success } else { NotificationKind::Warning },
                                        progress: None,
                                        accent: None,
                                    },
                                    Duration::from_secs(3),
                                );
                                app.island_cache.clear();
                            }
                        },
                    }
            }
        }
    }

    Task::none()
}

fn view(app: &NotchApp) -> Element<'_, Message> {
    let stage_w = metrics::STAGE_W;
    let stage_h = metrics::STAGE_H;

    let scale    = app.settings.settings.island_scale.max(0.1);
    let island_w = (app.springs.width.value * scale).max(0.0);
    let island_h = (app.springs.height.value * scale).max(0.0);
    let radius   = (app.springs.radius.value * scale).max(0.0);
    let shadow    = app.springs.shadow.value.clamp(0.0, 1.0);
    let highlight = app.springs.highlight.value.clamp(0.0, 1.0);
    let expansion = app.springs.expansion.value.clamp(0.0, 1.0);

    if island_h < 0.5 {
        return container(iced::widget::Space::new())
            .width(stage_w)
            .height(stage_h)
            .into();
    }

    // Build the page-specific data snapshots up front so they can be moved
    // into the canvas struct.
    let (timer_primary, timer_others) = app.build_timer_displays();
    let cal_state   = app.calendar_state.clone();

    // Resolve pinned ids against the catalog so labels and icons match the
    // Flutter ShortcutCatalog rather than showing raw ids.
    let shortcuts: Vec<pages::shortcuts::ShortcutEntry> = app
        .settings
        .settings
        .pinned_shortcuts
        .iter()
        .map(|id| {
            let (label, icon, _) = pages::shortcuts::catalog_entry(id)
                .unwrap_or((id.as_str(), '⚡', ""));
            pages::shortcuts::ShortcutEntry {
                id: id.clone(),
                label: label.to_string(),
                icon_char: icon,
                is_pinned: true,
            }
        })
        .collect();

    // Now-page agenda preview: active timers are the events this app owns.
    let mut agenda: Vec<pages::now::AgendaItem> = Vec::new();
    for timer in timer_primary.iter().chain(timer_others.iter()) {
        if timer.is_running || timer.is_finished {
            agenda.push(pages::now::AgendaItem {
                time_label: String::new(),
                text: format!("{} · {}", timer.label, timer.readout),
                is_timer: true,
            });
        }
    }
    if app.privacy.any_in_use() {
        let text = match (app.privacy.microphone, app.privacy.camera) {
            (true, true) => "Microphone & camera in use".to_string(),
            (true, false) => "Microphone in use".to_string(),
            (false, true) => "Camera in use".to_string(),
            (false, false) => String::new(),
        };
        agenda.push(pages::now::AgendaItem { time_label: String::new(), text, is_timer: false });
    }
    for download in app.downloads.active() {
        agenda.push(pages::now::AgendaItem {
            time_label: String::new(),
            text: format!("↓ {} · {}", download.name, crate::downloads::format_rate(download.bytes_per_second)),
            is_timer: false,
        });
    }
    let settings_rows = app.settings_rows_for(app.settings_section);
    let enabled_pages = app.enabled_pages();

    let clock_text = if app.settings.settings.use_24_hour_clock {
        app.now.format("%H:%M").to_string()
    } else {
        app.now.format("%-I:%M").to_string()
    };
    let roll_len = clock_text.chars().count();
    let roll_progress = if app.settings.settings.motion_intensity <= 0.01 {
        1.0
    } else {
        app.clock_roll.progress(Instant::now())
    };
    let roll_changed: Vec<bool> = (0..roll_len)
        .map(|i| app.clock_roll.is_changed(i))
        .collect();
    let roll_direction: Vec<i8> = (0..roll_len)
        .map(|i| app.clock_roll.direction(i))
        .collect();

    // Choose what the collapsed pill shows instead of the clock.
    let transient = if let Some(n) = app.notifications.current() {
        Some(TransientFace {
            title: n.title.clone(),
            subtitle: n.body.clone(),
            progress: n.progress,
            accent: n.accent.map_or_else(accent, |[r, g, b, a]| Color {
                r, g, b, a,
            }),
        })
    } else if app.privacy.any_in_use() {
        // Privacy indicators outrank ambient media/timer faces, matching
        // Flutter's PresentationRouter ordering.
        let (title, icon) = match (app.privacy.microphone, app.privacy.camera) {
            (true, true) => ("Mic & camera in use", "●"),
            (true, false) => ("Microphone in use", "●"),
            (false, true) => ("Camera in use", "●"),
            (false, false) => ("", ""),
        };
        Some(TransientFace {
            title: format!("{} {}", icon, title),
            subtitle: "Privacy".into(),
            progress: None,
            accent: Color { r: 1.0, g: 0.36, b: 0.36, a: 1.0 },
        })
    } else if let Some(d) = app.downloads.primary() {
        let count = app.downloads.active_count();
        Some(TransientFace {
            title: if count > 1 {
                format!("{} · {} downloads", d.name, count)
            } else {
                d.name.clone()
            },
            subtitle: crate::downloads::format_rate(app.downloads.total_rate()),
            progress: None,
            accent: accent(),
        })
    } else if let Some(m) = app.media_state.as_ref().filter(|m| m.is_playing) {
        Some(TransientFace {
            title: m.title.clone(),
            subtitle: if m.artist.is_empty() {
                m.app_name.clone()
            } else {
                m.artist.clone()
            },
            progress: if m.duration_secs > 0.0 {
                Some((m.position_secs / m.duration_secs).clamp(0.0, 1.0))
            } else {
                None
            },
            accent: accent(),
        })
    } else if let Some(t) = timer_primary
        .as_ref()
        .filter(|t| t.is_running || t.is_finished)
    {
        Some(TransientFace {
            title: t.readout.clone(),
            subtitle: t.label.clone(),
            progress: t.progress,
            accent: if t.is_finished { COLOR_POSITIVE } else { accent() },
        })
    } else {
        None
    };

    // Live Activities: derived from the same module state as the faces above,
    // then ordered by urgency for the two collapsed "ears" beside the clock.
    let mut live_activities: Vec<activities::LiveActivity> = Vec::new();
    if app.privacy.any_in_use() {
        let (icon, label) = match (app.privacy.microphone, app.privacy.camera) {
            (true, true) => ('●', "AV"),
            (true, false) => ('●', "MIC"),
            (false, true) => ('◉', "CAM"),
            (false, false) => ('●', ""),
        };
        live_activities.push(activities::LiveActivity {
            id: "privacy".into(),
            kind: activities::ActivityKind::Recording,
            icon,
            label: label.into(),
            progress: None,
            tint: Some(Color { r: 1.0, g: 0.36, b: 0.36, a: 1.0 }),
            urgent: true,
        });
    }
    if let Some(t) = timer_primary
        .as_ref()
        .filter(|t| t.is_running || t.is_finished)
    {
        live_activities.push(activities::LiveActivity {
            id: format!("timer.{}", t.id),
            kind: activities::ActivityKind::Timer,
            icon: '⏱',
            label: t.readout.clone(),
            progress: t.progress,
            tint: if t.is_finished {
                Some(COLOR_POSITIVE)
            } else {
                Some(accent())
            },
            urgent: t.is_finished,
        });
    }
    if let Some(d) = app.downloads.primary() {
        live_activities.push(activities::LiveActivity {
            id: format!("download.{}", d.id),
            kind: activities::ActivityKind::Download,
            icon: '↓',
            label: crate::downloads::format_rate(app.downloads.total_rate()),
            progress: None,
            tint: Some(accent()),
            urgent: false,
        });
    }
    if let Some(battery) = platform::battery_percent().filter(|p| *p <= 20) {
        live_activities.push(activities::LiveActivity {
            id: "battery".into(),
            kind: activities::ActivityKind::Battery,
            icon: '⚡',
            label: format!("{}%", battery),
            progress: None,
            tint: Some(COLOR_WARNING),
            urgent: true,
        });
    }
    if let Some(m) = app.media_state.as_ref().filter(|m| m.is_playing) {
        live_activities.push(activities::LiveActivity {
            id: "media".into(),
            kind: activities::ActivityKind::Media,
            icon: '♪',
            label: if m.artist.is_empty() { m.app_name.clone() } else { m.artist.clone() },
            progress: if m.duration_secs > 0.0 {
                Some((m.position_secs / m.duration_secs).clamp(0.0, 1.0))
            } else {
                None
            },
            tint: Some(accent()),
            urgent: false,
        });
    }
    let ears = if transient.is_some() {
        Vec::new()
    } else {
        activities::visible(live_activities)
    };

    let canvas_widget = Canvas::new(IslandCanvas {
        // Geometry / animation
        width:            island_w,
        height:           island_h,
        radius,
        shadow_strength:  shadow,
        highlight,
        expansion,
        page_fade:        {
            let elapsed = Instant::now().saturating_duration_since(app.page_changed_at);
            (elapsed.as_secs_f32() / 0.18).clamp(0.0, 1.0)
        },
        corner_scale:     app.settings.settings.corner_scale,
        surface_opacity:  app.settings.settings.surface_opacity,
        now:              app.now,
        phase:            app.phase,
        transient,
        ears,
        // Pager
        page_index:       app.page_index,
        page_count:       app.page_count,
        enabled_pages,
        pager_hovered:    app.pager_hovered,
        settings_subpage: app.settings_section.canvas_code(),
        // System stats
        cpu_pct:          app.cpu_pct,
        mem_pct:          app.mem_pct,
        mem_used_gb:      app.mem_used_gb,
        mem_total_gb:     app.mem_total_gb,
        net_down_bps:     app.net_down_bps,
        net_up_bps:       app.net_up_bps,
        cpu_history:      app.cpu_history.clone(),
        mem_history:      app.mem_history.clone(),
        net_down_history: app.net_down_history.clone(),
        net_up_history:   app.net_up_history.clone(),
        battery_pct:      platform::battery_percent(),
        battery_charging: platform::battery_charging(),
        volume_pct:       if app.volume.available {
            Some((app.volume.level * 100.0).round() as u8)
        } else {
            None
        },
        volume_muted:     app.volume.muted,
        mic_in_use:       app.privacy.microphone,
        camera_in_use:    app.privacy.camera,
        network_connected: app.network.connected,
        network_ssid:      app.network.ssid.clone(),
        // Media
        media_state:      app.media_state.clone(),
        // Timers
        timer_primary,
        timer_others,
        composer_minutes: app.composer_minutes,
        // Calendar
        cal_state,
        // Shortcuts
        shortcuts,
        hovered_shortcut: app.hovered_shortcut.clone(),
        // Shelf
        shelf_state: app.shelf_state.clone(),
        // Settings
        settings_rows,
        // Now
        agenda,
        use_24h:          app.settings.settings.use_24_hour_clock,
        intro_seen:       app.settings.settings.intro_seen,
        roll_progress,
        roll_changed,
        roll_direction,
        // Cache
        cache:            &app.island_cache,
    })
    .width(island_w)
    .height(island_h);

    // No `on_press` here: mouse_area captures the button event when that is
    // set, which would stop the canvas Program from receiving the precise
    // press position it needs for page hit-testing. CanvasPressed handles
    // both collapsed expansion and expanded clicks.
    let interactive = iced::widget::mouse_area(canvas_widget)
        .on_enter(Message::MouseEntered)
        .on_exit(Message::MouseLeft);

    let island_x = ((stage_w - island_w) / 2.0).max(0.0);
    let spacer_l  = iced::widget::Space::new().width(island_x);

    container(row![spacer_l, interactive])
        .width(stage_w)
        .height(stage_h)
        .into()
}

fn subscription(_app: &NotchApp) -> Subscription<Message> {
    // 120 Hz animation tick.
    let tick = iced::time::every(TICK_INTERVAL).map(Message::Tick);

    // 1 Hz system-stats tick.
    let sys_tick = iced::time::every(Duration::from_secs(1)).map(|_| Message::SystemTick);

    // Fire once when the first window opens so we can wire up platform.
    let win_open = window::open_events().map(Message::WindowOpened);

    let input = event::listen_with(|ev, _status, _win| {
        use iced::keyboard::{Event as KbEv, Key, key::Named};

        match ev {
            Event::Keyboard(KbEv::KeyPressed { key, .. }) => match key {
                Key::Named(Named::Escape) => Some(Message::Escape),
                Key::Named(Named::ArrowLeft) => Some(Message::PageLeft),
                Key::Named(Named::ArrowRight) => Some(Message::PageRight),
                Key::Named(Named::ArrowUp) => Some(Message::CanvasWheel(-1.0)),
                Key::Named(Named::ArrowDown) => Some(Message::CanvasWheel(1.0)),
                Key::Character(c) if c.eq_ignore_ascii_case("s") => {
                    Some(Message::ToggleSettings)
                }
                _ => None,
            },
            Event::Window(window::Event::CloseRequested) => Some(Message::Quit),
            Event::Window(window::Event::FileDropped(path)) => {
                Some(Message::FilesDropped(vec![path]))
            }
            _ => None,
        }
    });

    Subscription::batch([tick, sys_tick, win_open, input])
}

// ── Island canvas ─────────────────────────────────────────────────────────────

struct IslandCanvas<'a> {
    // Geometry / animation
    width:           f32,
    height:          f32,
    radius:          f32,
    shadow_strength: f32,
    highlight:       f32,
    expansion:       f32,
    page_fade:       f32,
    corner_scale:    f32,
    surface_opacity: f32,
    now:             chrono::DateTime<Local>,
    phase:           Phase,
    transient:       Option<TransientFace>,
    ears:            Vec<activities::LiveActivity>,

    // Pager
    page_index:    usize,
    page_count:    usize,
    enabled_pages: Vec<usize>,
    pager_hovered: bool,
    /// 0 = main list, 1 = visible pages, 2 = appearance, 3 = backup.
    settings_subpage: u8,

    // System stats
    cpu_pct:          Option<f32>,
    mem_pct:          Option<f32>,
    mem_used_gb:      f32,
    mem_total_gb:     f32,
    net_down_bps:     f32,
    net_up_bps:       f32,
    cpu_history:      Vec<f32>,
    mem_history:      Vec<f32>,
    net_down_history: Vec<f32>,
    net_up_history:   Vec<f32>,
    battery_pct:      Option<u8>,
    battery_charging: bool,
    volume_pct:       Option<u8>,
    volume_muted:     bool,
    mic_in_use:       bool,
    camera_in_use:    bool,
    network_connected: bool,
    network_ssid:      String,

    // Media
    media_state: Option<crate::pages::media::MediaState>,

    // Timers
    timer_primary:    Option<crate::pages::timers_page::TimerDisplay>,
    timer_others:     Vec<crate::pages::timers_page::TimerDisplay>,
    composer_minutes: u32,

    // Calendar
    cal_state: crate::pages::calendar::CalendarState,

    // Shortcuts
    shortcuts:        Vec<crate::pages::shortcuts::ShortcutEntry>,
    hovered_shortcut: Option<String>,

    // Shelf
    shelf_state: crate::pages::shelf::ShelfState,

    // Settings
    settings_rows: Vec<crate::pages::settings::SettingsRow>,

    // Now
    agenda:   Vec<crate::pages::now::AgendaItem>,
    use_24h:  bool,
    intro_seen: bool,
    roll_progress: f32,
    roll_changed: Vec<bool>,
    roll_direction: Vec<i8>,

    // Repaint cache
    cache: &'a canvas::Cache,
}

impl<'a> Program<Message> for IslandCanvas<'a> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &CanvasEvent,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<CanvasAction<Message>> {
        match event {
            CanvasEvent::Mouse(mouse::Event::CursorMoved { .. }) => cursor
                .position_in(bounds)
                .map(|p| CanvasAction::publish(Message::MouseMoved(p))),

            CanvasEvent::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor
                .position_in(bounds)
                .map(|p| {
                    CanvasAction::publish(Message::CanvasPressed(p)).and_capture()
                }),

            CanvasEvent::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let y = match delta {
                    mouse::ScrollDelta::Lines { y, .. }
                    | mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                Some(CanvasAction::publish(Message::CanvasWheel(y)).and_capture())
            }

            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let frame_geo = self.cache.draw(renderer, bounds.size(), |frame| {
            let w     = bounds.width;
            let h     = bounds.height;
            let corner = self.corner_scale.clamp(0.6, 1.4);
            // Round the top to match the bottom capsule. The original Flutter
            // surface uses topRadius=0 only because it is clipped by the notch.
            let r_top = self.radius.max(metrics::RADIUS_TOP) * corner;
            let r_bot = self.radius * corner;

            // ── Drop shadow ──────────────────────────────────────────────────
            let shadow_a = self.shadow_strength;
            if shadow_a > 0.01 {
                for i in 1..=12u32 {
                    let spread = i as f32 * 1.5;
                    let alpha  = shadow_a * 0.18 * (1.0 - i as f32 / 13.0);
                    let path = rounded_rect_path(
                        -spread, -spread / 2.0,
                        w + spread * 2.0, h + spread * 2.0,
                        r_top + spread, r_bot + spread,
                    );
                    frame.fill(&path, canvas::Fill::from(Color { a: alpha, ..Color::BLACK }));
                }
            }

            // ── Glass surface ────────────────────────────────────────────────
            let top_tint  = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.06 };
            let bot_tint  = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.08 };
            let base      = Color {
                a: SURFACE.a * self.surface_opacity.clamp(0.55, 1.0),
                ..SURFACE
            };
            let top_color = blend(top_tint, base);
            let bot_color = blend(bot_tint, base);

            let shape = rounded_rect_path(0.0, 0.0, w, h, r_top, r_bot);
            frame.fill(&shape, canvas::Fill::from(base));

            if h * 0.42 > 1.0 {
                let top_half = rounded_rect_path(0.0, 0.0, w, h * 0.42, r_top, 0.0);
                let tint = Color {
                    a: (top_color.a - base.a).max(0.0) * 0.5,
                    r: top_color.r, g: top_color.g, b: top_color.b,
                };
                frame.fill(&top_half, canvas::Fill::from(tint));
            }
            if h * 0.3 > 1.0 {
                let bot_start = h * 0.7;
                let bot_part  = rounded_rect_path(0.0, bot_start, w, h - bot_start, 0.0, r_bot);
                let tint = Color {
                    a: (bot_color.a - base.a).max(0.0) * 0.5,
                    r: bot_color.r, g: bot_color.g, b: bot_color.b,
                };
                frame.fill(&bot_part, canvas::Fill::from(tint));
            }

            // ── Edge stroke ──────────────────────────────────────────────────
            let edge_alpha = lerp(STROKE.a, STROKE_STRONG.a, self.highlight);
            frame.stroke(
                &shape,
                canvas::Stroke {
                    style: canvas::Style::Solid(Color { a: edge_alpha, ..STROKE }),
                    width: 1.0,
                    ..canvas::Stroke::default()
                },
            );

            // ── Lower-lip highlight ──────────────────────────────────────────
            let lip_strength = 0.7 + 0.3 * self.highlight;
            if h > 6.0 {
                let lip_alpha = INNER_HIGHLIGHT.a * lip_strength;
                let lip_y = h - r_bot * 0.5;
                let lip = canvas::Path::line(
                    Point::new(r_bot + 2.0, lip_y),
                    Point::new(w - r_bot - 2.0, lip_y),
                );
                frame.stroke(
                    &lip,
                    canvas::Stroke {
                        style: canvas::Style::Solid(Color { a: lip_alpha, ..INNER_HIGHLIGHT }),
                        width: 1.0,
                        line_cap: canvas::LineCap::Round,
                        ..canvas::Stroke::default()
                    },
                );
            }

            // ── Content cross-fade ───────────────────────────────────────────
            let t = self.expansion;
            let collapsed_opacity = (1.0 - t * 2.4).clamp(0.0, 1.0);
            let expanded_opacity  = ((t - 0.42) / 0.48).clamp(0.0, 1.0);

            if collapsed_opacity > 0.004 {
                draw_collapsed_face(
                    frame, w, h, collapsed_opacity, &self.now, self.use_24h,
                    self.transient.as_ref(), &self.ears,
                );
            }

            if expanded_opacity > 0.004 {
                self.draw_expanded_face(frame, w, h, expanded_opacity);
            }
        });

        vec![frame_geo]
    }
}

impl<'a> IslandCanvas<'a> {
    /// Route to the correct page painter, then draw pager UI on top.
    fn draw_expanded_face(
        &self,
        frame: &mut canvas::Frame,
        w: f32,
        h: f32,
        opacity: f32,
    ) {
        // Inner content area: a small pad keeps content from kissing the shell.
        let pad_x = 20.0_f32;
        let pad_y = 16.0_f32;
        // 20 px at the bottom is reserved for the pager dots row.
        let cw = w - pad_x * 2.0;
        let ch = h - pad_y * 2.0 - 20.0;

        // Only the page content cross-fades; dots, chevrons and the intro
        // banner stay at full opacity so the shell never looks half-drawn.
        let page_opacity = opacity * self.page_fade;

        // Page painters work in their own local coordinate space; shift the
        // frame so their (0,0) matches the hit-test origin used by
        // `NotchApp::page_point`.
        frame.translate(iced::Vector::new(pad_x, pad_y));

        match self.page_index {
            0 => crate::pages::now::draw_now_page(
                frame, cw, ch, page_opacity,
                &self.now,
                self.use_24h,
                &self.agenda,
                self.roll_progress,
                &self.roll_changed,
                &self.roll_direction,
                self.battery_pct,
                self.battery_charging,
                self.volume_pct,
                self.volume_muted,
                self.network_connected,
                &self.network_ssid
            ),

            1 => crate::pages::media::draw_media_page(
                frame, cw, ch, page_opacity,
                self.media_state.as_ref(),
            ),

            2 => {
                let (ref primary, ref others) = (&self.timer_primary, &self.timer_others);
                crate::pages::timers_page::draw_timers_page(
                    frame, cw, ch, page_opacity,
                    primary.as_ref(),
                    others,
                    self.composer_minutes,
                    primary.is_none() && others.is_empty(), // is_composer when no timers
                );
            }

            3 => crate::pages::system::draw_system_page(
                frame, cw, ch, page_opacity,
                &self.cpu_history,
                &self.mem_history,
                &self.net_down_history,
                &self.net_up_history,
                self.cpu_pct,
                self.mem_pct,
                self.mem_used_gb,
                self.mem_total_gb,
                self.net_down_bps,
                self.net_up_bps,
                self.network_connected,
                self.battery_pct,
                self.battery_charging,
                &self.network_ssid,
                self.network_connected,
                self.volume_pct,
                self.volume_muted,
                self.mic_in_use,
                self.camera_in_use,
            ),

            4 => crate::pages::calendar::draw_calendar_page(
                frame, cw, ch, page_opacity,
                &self.cal_state,
            ),

            5 => crate::pages::shortcuts::draw_shortcuts_page(
                frame, cw, ch, page_opacity,
                &self.shortcuts,
                self.hovered_shortcut.as_deref(),
            ),

            6 => crate::pages::shelf::draw_shelf_page(
                frame, cw, ch, page_opacity,
                &self.shelf_state,
            ),

            7 => crate::pages::settings::draw_settings_page(
                frame, cw, ch, page_opacity,
                &self.settings_rows,
                match self.settings_subpage {
                    1 => "Visible pages",
                    2 => "Appearance",
                    3 => "Backup & sync",
                    _ => "Settings",
                },
            ),

            _ => {}
        }

        frame.translate(iced::Vector::new(-pad_x, -pad_y));

        if !self.intro_seen {
            draw_intro_banner(frame, w, h, opacity);
        }

        // ── Pager dots ────────────────────────────────────────────────────────
        self.draw_pager_dots(frame, w, h, opacity);

        // ── Navigation chevrons ───────────────────────────────────────────────
        if self.pager_hovered && self.expansion > 0.9 {
            self.draw_chevrons(frame, w, h, opacity);
        }
    }

    /// Pager page dots at y = h - 10, centred.
    /// Active dot: 14 px wide pill. Inactive: 4 px wide pill, 45% alpha.
    fn draw_pager_dots(&self, frame: &mut canvas::Frame, w: f32, h: f32, opacity: f32) {
        let dot_y       = h - 10.0;
        let n           = self.enabled_pages.len().max(1);
        let gap         = 5.0_f32;
        let active_w    = 14.0_f32;
        let inactive_w  = 4.0_f32;
        let dot_h       = 3.0_f32;
        let r           = dot_h / 2.0;

        // Total width for centering.
        let total_w: f32 =
            active_w + (n as f32 - 1.0) * inactive_w + (n as f32 - 1.0) * gap;
        let mut cursor_x = (w - total_w) / 2.0;

        for &i in self.enabled_pages.iter() {
            let is_active = i == self.page_index;
            let dw = if is_active { active_w } else { inactive_w };

            let color = if is_active {
                Color { a: TEXT_SECONDARY.a * opacity, ..TEXT_SECONDARY }
            } else {
                Color { a: TEXT_TERTIARY.a * 0.45 * opacity, ..TEXT_TERTIARY }
            };

            // Pill = left-cap + fill + right-cap
            frame.fill(
                &canvas::Path::circle(Point::new(cursor_x + r, dot_y), r),
                canvas::Fill::from(color),
            );
            if dw > dot_h {
                frame.fill_rectangle(
                    Point::new(cursor_x + r, dot_y - r),
                    iced::Size::new(dw - dot_h, dot_h),
                    color,
                );
            }
            frame.fill(
                &canvas::Path::circle(Point::new(cursor_x + dw - r, dot_y), r),
                canvas::Fill::from(color),
            );

            cursor_x += dw + gap;
        }
    }

    /// `<` / `>` chevron arrows drawn at the left/right edges of the island
    /// when `pager_hovered && expansion > 0.9`.
    fn draw_chevrons(&self, frame: &mut canvas::Frame, w: f32, h: f32, opacity: f32) {
        let cy = h / 2.0;
        let col = Color { a: TEXT_TERTIARY.a * opacity * 0.7, ..TEXT_TERTIARY };

        if self.enabled_pages.first() != Some(&self.page_index) || self.settings_subpage != 0 {
            frame.fill_text(canvas::Text {
                content: "<".into(),
                position: Point::new(10.0, cy),
                color: col,
                size: iced::Pixels(14.0),
                font: iced::Font::DEFAULT,
                align_x: alignment::Horizontal::Center.into(),
                align_y: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }

        if self.enabled_pages.last() != Some(&self.page_index) && self.settings_subpage == 0 {
            frame.fill_text(canvas::Text {
                content: ">".into(),
                position: Point::new(w - 10.0, cy),
                color: col,
                size: iced::Pixels(14.0),
                font: iced::Font::DEFAULT,
                align_x: alignment::Horizontal::Center.into(),
                align_y: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
    }
}

// ── First-run banner ──────────────────────────────────────────────────────────

/// One-time hint for an overlay with no title bar: where navigation is, and
/// how to reopen it after hiding. Mirrors `intro_banner.dart`.
fn draw_intro_banner(frame: &mut canvas::Frame, w: f32, h: f32, opacity: f32) {
    let banner_y = h - 44.0;
    let banner_h = 26.0;
    let banner_x = 14.0;
    let banner_w = w - 28.0;

    let bg = rounded_rect_path(banner_x, banner_y, banner_w, banner_h, 8.0, 8.0);
    let acc = accent();
    frame.fill(&bg, canvas::Fill::from(Color { a: opacity * 0.12, ..acc }));
    frame.stroke(&bg, canvas::Stroke {
        style: canvas::Style::Solid(Color { a: opacity * 0.28, ..acc }),
        width: 0.75,
        ..canvas::Stroke::default()
    });

    frame.fill_text(canvas::Text {
        content: "Arrows, scroll or dots move between pages".into(),
        position: Point::new(banner_x + 10.0, banner_y + banner_h / 2.0),
        color: Color { a: opacity * 0.85, ..TEXT_SECONDARY },
        size: iced::Pixels(9.5),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });

    let (bx, by, bw, bh) = intro_button_rect(w, h);
    let btn = rounded_rect_path(bx, by, bw, bh, 6.0, 6.0);
    frame.fill(&btn, canvas::Fill::from(Color { a: opacity * 0.18, ..acc }));
    frame.fill_text(canvas::Text {
        content: "Got it".into(),
        position: Point::new(bx + bw / 2.0, by + bh / 2.0),
        color: Color { a: opacity, ..TEXT_PRIMARY },
        size: iced::Pixels(9.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}

// ── Collapsed face ────────────────────────────────────────────────────────────

fn draw_collapsed_face(
    frame: &mut canvas::Frame,
    w: f32,
    h: f32,
    opacity: f32,
    now: &chrono::DateTime<Local>,
    use_24h: bool,
    transient: Option<&TransientFace>,
    ears: &[activities::LiveActivity],
) {
    // Live dot — tinted by the active transient face when there is one. When
    // no transient is showing, the dot is replaced by the first activity ear.
    let dot_color = transient.map_or_else(accent, |t| t.accent);
    if transient.is_some() || ears.is_empty() {
        frame.fill(
            &canvas::Path::circle(Point::new(w / 2.0 - 52.0, h / 2.0), 2.5),
            canvas::Fill::from(Color {
                a: dot_color.a * opacity * 0.85,
                ..dot_color
            }),
        );
    }

    if let Some(t) = transient {
        let title: String = t.title.chars().take(22).collect();
        frame.fill_text(canvas::Text {
            content: title,
            position: Point::new(w / 2.0 + 4.0, h / 2.0 - 5.0),
            color: Color { a: opacity, ..TEXT_PRIMARY },
            size: iced::Pixels(11.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..canvas::Text::default()
        });

        if !t.subtitle.is_empty() {
            let subtitle: String = t.subtitle.chars().take(26).collect();
            frame.fill_text(canvas::Text {
                content: subtitle,
                position: Point::new(w / 2.0 + 4.0, h / 2.0 + 7.0),
                color: Color { a: TEXT_SECONDARY.a * opacity, ..TEXT_SECONDARY },
                size: iced::Pixels(9.0),
                font: iced::Font::DEFAULT,
                align_x: alignment::Horizontal::Center.into(),
                align_y: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }

        if let Some(progress) = t.progress {
            let bar_w = (w - 40.0).max(0.0);
            frame.fill_rectangle(
                Point::new(20.0, h - 3.0),
                iced::Size::new(bar_w, 2.0),
                Color { a: 0.16 * opacity, ..TEXT_TERTIARY },
            );
            frame.fill_rectangle(
                Point::new(20.0, h - 3.0),
                iced::Size::new(bar_w * progress.clamp(0.0, 1.0), 2.0),
                Color { a: opacity * 0.85, ..dot_color },
            );
        }
        return;
    }

    // Clock
    let time_str = if use_24h {
        now.format("%H:%M").to_string()
    } else {
        now.format("%-I:%M %p").to_string()
    };
    frame.fill_text(canvas::Text {
        content: time_str,
        position: Point::new(w / 2.0, h / 2.0),
        color: Color { a: opacity, ..TEXT_PRIMARY },
        size: iced::Pixels(13.0),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });

    if let Some(leading) = ears.first() {
        draw_activity_ear(frame, leading, 24.0, h / 2.0, false, opacity);
    } else {
        // Short date only when no trailing activity occupies the right ear.
        let date_str = now.format("%-d %b").to_string();
        frame.fill_text(canvas::Text {
            content: date_str,
            position: Point::new(w / 2.0 + 44.0, h / 2.0),
            color: Color { a: TEXT_SECONDARY.a * opacity, ..TEXT_SECONDARY },
            size: iced::Pixels(10.0),
            font: iced::Font::DEFAULT,
            align_x: alignment::Horizontal::Left.into(),
            align_y: alignment::Vertical::Center,
            ..canvas::Text::default()
        });
    }
    if let Some(trailing) = ears.get(1) {
        draw_activity_ear(frame, trailing, w - 24.0, h / 2.0, true, opacity);
    }
}

/// One `LiveActivity` beside the collapsed clock: a glyph (wrapped in a
/// progress ring when applicable) plus a very short value. Mirrors
/// `_ActivityEar` / `_ActivityGlyph` in Flutter's `collapsed_face.dart`.
fn draw_activity_ear(
    frame: &mut canvas::Frame,
    activity: &activities::LiveActivity,
    glyph_x: f32,
    cy: f32,
    align_end: bool,
    opacity: f32,
) {
    let tint = activity.tint.unwrap_or_else(accent);
    let ring_r = 6.0_f32;

    frame.stroke(
        &canvas::Path::circle(Point::new(glyph_x, cy), ring_r),
        canvas::Stroke {
            style: canvas::Style::Solid(Color { a: 0.22 * opacity, ..SURFACE_SUNKEN }),
            width: 1.6,
            ..canvas::Stroke::default()
        },
    );
    if let Some(progress) = activity.progress {
        let end = -std::f32::consts::FRAC_PI_2
            + std::f32::consts::TAU * progress.clamp(0.0, 1.0);
        let arc = canvas::Path::new(|b| {
            b.arc(canvas::path::Arc {
                center: Point::new(glyph_x, cy),
                radius: ring_r,
                start_angle: iced::Radians(-std::f32::consts::FRAC_PI_2),
                end_angle: iced::Radians(end),
            });
        });
        frame.stroke(
            &arc,
            canvas::Stroke {
                style: canvas::Style::Solid(Color { a: 0.9 * opacity, ..tint }),
                width: 1.6,
                line_cap: canvas::LineCap::Round,
                ..canvas::Stroke::default()
            },
        );
    }

    frame.fill_text(canvas::Text {
        content: activity.icon.to_string(),
        position: Point::new(glyph_x, cy),
        color: Color {
            a: if activity.urgent { opacity } else { 0.85 * opacity },
            ..tint
        },
        size: iced::Pixels(activity.progress.map_or(9.0, |_| 7.0)),
        font: iced::Font::DEFAULT,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });

    let label: String = activity.label.chars().take(7).collect();
    let (label_x, align) = if align_end {
        (glyph_x - 10.0, alignment::Horizontal::Right)
    } else {
        (glyph_x + 10.0, alignment::Horizontal::Left)
    };
    frame.fill_text(canvas::Text {
        content: label,
        position: Point::new(label_x, cy),
        color: Color {
            a: if activity.urgent { 0.95 * opacity } else { 0.72 * opacity },
            ..if activity.urgent { tint } else { TEXT_SECONDARY }
        },
        size: iced::Pixels(9.0),
        font: iced::Font::DEFAULT,
        align_x: align.into(),
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}

// ── Rounded-rect path ─────────────────────────────────────────────────────────

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r_top: f32, r_bot: f32) -> canvas::Path {
    let r_top = r_top.min(w / 2.0).min(h / 2.0).max(0.0);
    let r_bot = r_bot.min(w / 2.0).min(h / 2.0).max(0.0);

    canvas::Path::new(|b| {
        b.move_to(Point::new(x + r_top, y));
        b.line_to(Point::new(x + w - r_top, y));
        if r_top > 0.0 {
            b.arc_to(Point::new(x + w, y), Point::new(x + w, y + r_top), r_top);
        }
        b.line_to(Point::new(x + w, y + h - r_bot));
        if r_bot > 0.0 {
            b.arc_to(Point::new(x + w, y + h), Point::new(x + w - r_bot, y + h), r_bot);
        }
        b.line_to(Point::new(x + r_bot, y + h));
        if r_bot > 0.0 {
            b.arc_to(Point::new(x, y + h), Point::new(x, y + h - r_bot), r_bot);
        }
        b.line_to(Point::new(x, y + r_top));
        if r_top > 0.0 {
            b.arc_to(Point::new(x, y), Point::new(x + r_top, y), r_top);
        }
        b.close();
    })
}

// ── Colour math ───────────────────────────────────────────────────────────────

/// Alpha-composite `src` over `dst` (matches Flutter `Color.alphaBlend`).
fn blend(src: Color, dst: Color) -> Color {
    let a = src.a + dst.a * (1.0 - src.a);
    if a < 0.0001 { return Color::TRANSPARENT; }
    Color {
        r: (src.r * src.a + dst.r * dst.a * (1.0 - src.a)) / a,
        g: (src.g * src.a + dst.g * dst.a * (1.0 - src.a)) / a,
        b: (src.b * src.a + dst.b * dst.a * (1.0 - src.a)) / a,
        a,
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

// ── Spring retarget helper ────────────────────────────────────────────────────

fn apply_profile(track: &mut Spring, profile: &Spring, target: f32) {
    track.stiffness = profile.stiffness;
    track.damping   = profile.damping;
    track.set_target(target);
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn main() -> iced::Result {
    // Make Win32 coordinates physical pixels. Without this, the process is
    // DPI-virtualized: iced lays out in logical pixels while GetWindowRect /
    // SetWindowPos / window regions use virtualized values, so the pill is
    // off-centre and region updates flicker on scaled displays.
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // Only one island process may own the overlay. The named mutex is kept
    // alive for the whole run; a second launch exits quietly.
    #[cfg(target_os = "windows")]
    let _single_instance: Option<windows::Win32::Foundation::HANDLE> = {
        use windows::{
            core::HSTRING,
            Win32::{
                Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
                System::Threading::CreateMutexW,
            },
        };
        let mutex_name = HSTRING::from(r"Local\DynamicBar.SingleInstance");
        unsafe {
            match CreateMutexW(None, true, &mutex_name) {
                Ok(handle) => {
                    if GetLastError() == ERROR_ALREADY_EXISTS {
                        return Ok(());
                    }
                    Some(handle)
                }
                Err(_) => None,
            }
        }
    };

    let stage_w = metrics::STAGE_W;
    let stage_h = metrics::STAGE_H;

    let window_settings = window::Settings {
        size: Size::new(stage_w, stage_h),
        position: window::Position::Specific(Point::ORIGIN),
        resizable: false,
        decorations: false,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        visible: true,
        ..Default::default()
    };

    iced::application(
        || {
            let app = NotchApp::default();
            (app, Task::none())
        },
        update,
        view,
    )
    .subscription(subscription)
    .window(window_settings)
    .title("notchX")
    .theme(|_: &NotchApp| Theme::Dark)
    .antialiasing(true)
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_rotation_changes_colour_and_wraps() {
        let blue = (0x3B, 0x8C, 0xFF);
        let rotated = rotate_hue(blue, 30.0);
        assert_ne!(rotated, blue);
        let full_turn = rotate_hue(blue, 360.0);
        assert_eq!(full_turn, blue);
    }

    #[test]
    fn grey_colours_have_no_hue_to_rotate() {
        let grey = (0x80, 0x80, 0x80);
        assert_eq!(rotate_hue(grey, 90.0), grey);
    }

    #[test]
    fn custom_accent_display_includes_hex() {
        assert_eq!(
            accent_display(CUSTOM_ACCENT_INDEX, Some((0x12, 0x34, 0xAB))),
            "Custom #1234AB"
        );
        assert_eq!(accent_display(1, None), "Blue");
    }
}
