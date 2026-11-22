# notchWin — Port Roadmap

> Flutter original: `lib/` and `windows/dynamicbar/` — Rust port: `notchWin/src/`
>
> Status is based on source inspection plus local verification:
> `cargo check --offline --message-format short` and `cargo test --offline`
> (28 tests) both pass. Basic Win32 geometry (DPI-aware stage placement and
> region box) was also inspected on a live process. Hardware-specific behavior
> (tray messages, low-level hook, DPI *changes*, lock screen, fullscreen,
> multi-monitor, Wi-Fi) still needs a real-machine pass. See
> `IMPLEMENTATION_STATUS.md` for the full current state and the overlay
> regression history.

---

## Progress at a glance

```
Shell & physics  ████████████████████  100%
Window / Win32   ██████████████████░░   90%
Collapsed face   ██████████████████░░   90%
Expand/collapse  ████████████████████  100%
Hover / hit-rect ████████████████████  100%
Module pager     ███████████████████░   95%
Module pages     ███████████████████░   95%
Live Activities  ████████████████████  100%
Notifications    ███████████████░░░░░   75%  (in-app + system events, no OS toast listener)
System sensors   ████████████████████  100%
Tray / hotkeys   ███████████████████░   95%
Settings panel   ████████████████████  100%

Overall          ███████████████████░   ~96%
```

---

## What is done

### ✅ Physics engine (`spring.rs`)
Full port of `SpringGroup` / `DbSpring`. Semi-implicit Euler at 1/480 s
sub-step, all named profiles: `arrival`, `departure`, `peek`, `atmosphere`,
`reveal`, `withdraw`, `contour`, `content_in`, `content_out`. Interruptible,
idle-free (ticker stops when settled), retarget keeps velocity. Motion
intensity scales the spring timestep; `0.0` snaps immediately.

### ✅ Geometry (`metrics.rs`)
All constants from `IslandMetrics`, `DbSize`, `DbRadius` — collapsed/expanded
sizes, radii, hit-rect padding, stage size, reveal margins — plus island-scale
setting support.

### ✅ State machine (`state.rs`)
`Intent` / `Phase` enum pair, `derive_phase()` matching the Flutter function
exactly (epsilon comparisons, velocity-based direction), `resolve_geometry()`
for all three intents including hover-peek growth.

### ✅ Glass shell (canvas in `main.rs`)
- Multi-layer drop shadow (12 concentric rounded-rect fills)
- Three-stop vertical gradient fill (top-tint → base → bottom-tint)
- Hairline edge stroke, brightens under cursor (`highlight` spring)
- Lower-lip highlight (single gradient stroke at the bottom curve)
- Content cross-fade with the same `_collapsedOpacity` / `_expandedOpacity`
  curves as `IslandContent` in Flutter
- Page painters are frame-translated by `pad_x = 20`, `pad_y = 16` to match
  Flutter's `NotchApp.page_point` hit-testing

### ✅ Expand / collapse lifecycle
`expand()` / `collapse()` / `set_intent()` with spring retargeting that picks
the right profile per move character — exactly mirrors `_retarget` in
`island_stage.dart`. Auto-collapse is configurable and persisted.

### ✅ Hover debounce
70 ms open delay, 180 ms cancel grace — same values as `IslandCoordinator`.
Hover peek uses the `peek` spring (bouncy, 0.34 s / 0.55 damping) and is
toggleable in Settings.

### ✅ Hit-rect precision
Hit-rect is the island + `HIT_PAD_X/Y` padding, computed every tick, never
handed back mid-animation; mouse-hook rect sync and platform events are
throttled to ~30 Hz inside `Message::Tick`.

### ✅ Win32 overlay layer (`win32.rs`)
`WS_EX_TOPMOST | WS_EX_TOOLWINDOW` for an always-on-top tool window, an
`HTTRANSPARENT` `WM_NCHITTEST` subclass so only the live island rect receives
input, and a cached rounded `SetWindowRgn` applied only on target-geometry
changes. `WS_EX_NOACTIVATE` suppresses taskbar/focus stealing while collapsed
and is cleared as soon as the island becomes interactive. TOPMOST is re-asserted
for lock/wake/Explorer-restart. DWM frame extension and whole-window
`WS_EX_TRANSPARENT` were deliberately removed after they caused grey-sheet and
other-app-blocking regressions.

### ✅ Real HWND & stage placement (`main.rs`, `platform.rs`)
`iced::window::run` with `raw_window_handle` supplies the real HWND, sent as
`Message::StageHwnd(isize)` into `platform::start(HWND)`. `main()` enables
per-monitor DPI awareness before iced starts; the stage is placed in physical
pixels at the top-centre of the primary monitor and the canvas uses logical
pixels. Click-through is per-rect, never whole-window.

### ✅ Collapsed face (`pages/now.rs`, `clock.rs`, `main.rs`)
Clock with digit-roll animation (only changed digits move), live dot, short
date, transient face priority notification > privacy > downloads > media >
timer > clock, and a progress bar for active media/timers. Up to two
`LiveActivity` "ears" with glyphs, progress rings and short labels flank the
clock when no transient face is active.

### ✅ Expanded island pager (`main.rs`)
8 canvas pages with dots, chevrons, press/drag/wheel handling directly on
`IslandCanvas` (no `mouse_area` press capture). Per-page visibility is stored
as `disabled_pages`; the pager skips disabled pages and Settings is always
available as the escape hatch. First-run intro banner is dismissible.

### ✅ Module pages (`src/pages/`)
- **Now** — clock, date, compact agenda with active/finished timers, privacy
  and download entries, status chips (battery, Wi-Fi SSID, volume), Quick Tools
- **Media** — SMTC title/artist/app, play/pause/prev/next, seek via
  `TryChangePlaybackPositionAsync`
- **Timers** — countdown input, start/pause/resume/reset/delete, completion alerts
- **System** — CPU/RAM/battery from Win32, network state + SSID from WLAN API
- **Calendar** — month grid, today marker, per-day events loaded from
  `%APPDATA%\notchWin\calendar.json` and refreshed on navigation
- **Shortcuts** — catalog, search, launch via `ShellExecuteW`
- **Shelf** — drag/drop files via iced `FileDropped`, launch/reveal/remove,
  JSON persistence
- **Settings** — 15 main rows plus Visible pages, Appearance and Backup
  sub-pages, all persisted; portable export/import snapshot

### ✅ Live status & transient faces (`platform.rs`, `downloads.rs`, `activities.rs`)
- **Volume** — Core Audio default render endpoint; HUD notice for 1.4 s on
  change (toggleable, mirrors Flutter's `volumeHudEnabled`)
- **Privacy** — CapabilityAccessManager `microphone`/`webcam` ConsentStore
  polling (packaged + `NonPackaged` apps); collapsed privacy face + agenda row
- **Downloads** — Downloads-folder partial-file watcher with started/progress/
  finished events, rate smoothing, collapsed face and completion notices
- **Wi-Fi** — `WlanQueryInterface` reports connected SSID to the Now/System pages
- **ActivityCenter** — derived, urgency-sorted `LiveActivity` list (recording,
  timer, download, battery, media) feeding the two collapsed ears; this
  matches Flutter's `ActivityCenter` exactly (it is a derived view with no
  card-specific demo data in the reference)
- **System strip** — expanded System page shows volume/mic/camera/Wi-Fi tiles
  sourced from the same snapshots as the collapsed face
- Notifications have timed auto-expiry; volume and download notices use it

### ✅ Platform integration (`platform.rs`)
Tray icon (Open, Pause/resume, Quit), global hotkeys (`Ctrl+Alt+Space`,
`Ctrl+Alt+Right`, `Ctrl+Alt+Down`), single-instance mutex
`Local\DynamicBar.SingleInstance`, SMTC watcher, media keys via `keybd_event`,
outside-click collapse, auto-hide, auto-collapse, foreground maximized/fullscreen
detection, session-lock polling (4 Hz), and startup registry entry
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run\notchX`.

### ✅ Settings (`settings.rs`, `pages/settings.rs`)
Accent presets (Graphite/Blue/Purple/Emerald/Amber/Crimson), island scale,
motion intensity, 24h clock, hover peek, wheel volume, volume HUD, auto-hide,
hide-when-covered, auto-collapse, launch-at-login, first-run tip, per-page
visibility, custom accent hue, surface opacity and corner-radius scale. All
persisted to JSON; startup switch syncs with the real Run key and failed
writes are not persisted. Settings can be exported to / imported from
`%APPDATA%\notchWin\settings-export.json`.

---

## What is not done yet

### Phase 1 — Make the shell production-ready (95%)

| Task | Flutter source | Status / notes |
|---|---|---|
| Wire Win32 HWND into iced via `raw-window-handle` | `island_window.cpp` | ✅ `Message::StageHwnd` |
| Click-through outside the island | `window_channel.cpp` | ✅ `WM_NCHITTEST` → `HTTRANSPARENT`; no whole-window flag |
| Position stage at top-centre of primary monitor | `island_window.cpp` | ✅ physical-pixel placement at startup |
| DPI-aware sizing | `WM_DPICHANGED` | ⚠ startup DPI handled; runtime DPI change still pending |
| Keyboard focus on expand | `IslandCoordinator.requestFocus` | ✅ clears `WS_EX_NOACTIVATE` + `SetForegroundWindow`; manual retest pending |
| Hardware verification | — | ⏳ tray, hook, DPI, lock, fullscreen, multi-monitor |

### Phase 2 — Collapsed face, complete (90%)

| Task | Flutter source | Status / notes |
|---|---|---|
| Notification collapsed face | `notification_face.dart` | ✅ |
| Media mini-face (progress bar) | `media_face.dart` | ✅ |
| Timer face | `timer_face.dart` | ✅ |
| Clock digit roll animation | `now_page.dart` | ✅ |
| Live Activity ears (ring + label) | `collapsed_face.dart` | ✅ glyph, label, progress ring, urgency tint |
| Privacy indicator face | `privacy_face.dart` | ✅ collapsed face + agenda row |
| Download progress face | `download_face.dart` | ✅ collapsed face + watcher + notices |
| System strip (volume HUD) | `system_strip.dart` | ✅ volume chip, HUD notice and 4-tile expanded System strip |

### Phase 3 — Expanded island pager (95%)

| Task | Flutter source | Status |
|---|---|---|
| `IslandPager` with left/right swipe | `island_pager.dart` | ✅ canvas drag |
| Page dots indicator | `page_dots.dart` | ✅ |
| Arrow-key / scroll-wheel navigation | `_InteractionLayer` | ✅ |
| Page scaffold | `page_scaffold.dart` | ✅ |
| Intro / first-run banner | `intro_banner.dart` | ✅ |
| Animated page transitions | — | ⏳ optional polish |

### Phase 4 — Module pages (95%)

| Page | Flutter source | Status / remaining |
|---|---|---|
| **Now** | `now_page.dart` | ✅ clock, agenda, privacy/download, status chips |
| **Media** | `media_page.dart` | ✅ controls/seek; ⏳ real album-art tint (SMTC does not expose other apps' art) |
| **Calendar** | `calendar_page.dart` | ✅ grid + local JSON event source |
| **Timers** | `timers_page.dart` | ✅ |
| **Shelf** | `shelf_page.dart` | ✅ via `FileDropped` + JSON |
| **Shortcuts** | `shortcuts_page.dart` | ✅ |
| **System** | `system_page.dart` | ✅ CPU/RAM/battery/Wi-Fi SSID |
| **Settings** | `settings_panel.dart` | ✅ 15 rows + 3 subpages |

### Phase 5 — Transient faces (95%)

| Face | Flutter source | Status |
|---|---|---|
| Media mini-face | `media_face.dart` | ✅ |
| Notification face | `notification_face.dart` | ✅ in-app + system volume/download notices |
| Timer face | `timer_face.dart` | ✅ |
| Download face | `download_face.dart` | ✅ |
| Privacy face | `privacy_face.dart` | ✅ |
| Volume HUD | `system_strip.dart` | ✅ |
| Full ActivityCenter behaviour | `activity_center.dart` | ✅ derived urgency-sorted model + two collapsed ears; Flutter has no card-specific demo layouts |
| OS toast mirroring | — | ⏳ not implemented in the Flutter original either; requires a companion bridge |

### Phase 6 — Platform integration (95%)

| Feature | Flutter / Win32 source | Status |
|---|---|---|
| System tray icon + context menu | `tray_bridge.dart` | ✅ |
| Global hotkeys | `hotkey_bridge.dart` | ✅ |
| Auto-hide with proximity reveal | `IslandCoordinator` | ✅ |
| Auto-collapse timer | `IslandCoordinator._autoCollapse` | ✅ |
| Hide behind maximised apps | foreground watch | ✅ polling at 4 Hz |
| Outside-click collapse | `WH_MOUSE_LL` | ✅ |
| Single-instance mutex | `Local\DynamicBar.SingleInstance` | ✅ |
| Startup registry entry | `startup_bridge.dart` | ✅ |
| Session lock / wake re-assert | `WTS...` | ✅ 4 Hz lock polling |
| Volume / privacy / network sensors | `system_service.cpp` | ✅ Core Audio, ConsentStore, WLAN |
| Downloads watcher | `downloads_service.cpp` | ✅ partial-file polling |

### Phase 7 — Settings (100%)

| Feature | Flutter source | Status |
|---|---|---|
| Settings panel | `settings_panel.dart` | ✅ page 7 with 3 sub-pages |
| Per-page on/off toggles | `AppSettings.enabledPages` | ✅ subpage |
| Island scale slider | `AppSettings.islandScale` | ✅ |
| Accent colour presets | `AccentPreset` | ✅ 6 presets |
| Motion intensity | `MotionProfile` | ✅ |
| 24-hour clock / hover peek / wheel volume | `AppSettings` | ✅ |
| Volume HUD toggle | `AppSettings.volumeHudEnabled` | ✅ |
| Auto-hide / hide-when-covered / auto-collapse | — | ✅ |
| Launch-at-login | `startup_bridge.dart` | ✅ |
| Surface opacity / corner radius scale / custom accent hue | — | ✅ Appearance sub-page |
| Settings import/export | — | ✅ portable JSON snapshot |

---

## Recommended next implementation order

1. **Manual release-build retest** — hover → first click → arrow keys without
   Alt-Tab, and confirm no grey rectangle/flicker during expand/collapse.
2. **Album artwork** — read SMTC `Thumbnail()`, decode JPEG/PNG/WebP, port the
   24 × 24 circular hue average from `artwork_tint.dart`, then render cover/tint.
   See `IMPLEMENTATION_STATUS.md §7` for the full plan.
3. **Visual parity** — HomeDeck panels first, then media rich states, timer dial
   motion, collapsed-face switch and pager slide/parallax.
4. **Hardware verification pass** — DPI changes, tray, hook, lock screen,
   fullscreen, multi-monitor, Wi-Fi/ConsentStore permissions on real Windows.
5. **Optional: OS toast mirroring** — not present in the Flutter reference;
   would need a companion AppUserModelId bridge or UI Automation listener.

---

## Notes on intentional divergence from Flutter

- `ShellExecuteW` and `keybd_event` are used instead of WinRT
  package-identity-dependent launchers, so the port works unpackaged.
- Shelf drop uses iced's `FileDropped` window event plus JSON persistence
  rather than a classic OLE `IDropTarget`.
- Accent is shared through global atomics (`ACCENT_RGB`, `accent()`,
  `set_accent()`) because the page painters are pure canvas functions.
- Downloads and sensors use polling (400 ms / 200 ms / 1–2 s) rather than
  `ReadDirectoryChangesW` / `IAudioEndpointVolumeCallback`; this keeps the
  implementation compact and thread-safe at the cost of slight latency.
- Calendar events come from a small local JSON file instead of WinRT Calendar
  access, so the port works unpackaged. Settings snapshots are plain JSON too.
- The top corner radius is `16` on Windows instead of Flutter's `0`; Flutter can
  hide its square top edge behind the physical notch, while this overlay floats
  on the desktop.
- The Shelf note text area was removed at the user's request; `shelf.json` keeps
  only the two image-slot paths.
- The process opts into per-monitor DPI awareness and clips the stage with a
  rounded `SetWindowRgn` on target-geometry changes only. This is Windows-specific
  plumbing with no Flutter equivalent.
- Not every Flutter design token is exposed in the Rust settings UI yet; the
  remaining values are baked into the canvas painters.

---

## Comparison to Flutter original

| Concern | Flutter | Rust/iced |
|---|---|---|
| Hover area bug | Hit-rect can lag or be sized wrong | Computed fresh every spring tick; never shrinks mid-animation |
| Click opens then closes | Toggle on shell tap | Shell tap only expands; never collapses from shell |
| Taskbar interference | `WS_EX_TOOLWINDOW` set in C++ runner | Same flag in `win32::setup_overlay` |
| Glitchy animation | Flutter raster thread + compositor can tear | iced/wgpu; single GPU surface; canvas cache suppresses repaints when settled |
| High RAM | Dart VM + Flutter engine ~180 MB baseline | Rust binary; wgpu GPU memory only while animating |
