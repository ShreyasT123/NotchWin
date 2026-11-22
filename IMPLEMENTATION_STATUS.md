# notchWin — Implementation Status

_Last updated: 2026-11-23_

This document records the Rust/iced Dynamic Island port **as it stands today**:
what was built, the decisions behind the overlay/window work, what was verified,
and what is deliberately deferred. The Flutter project in the repository root
remains the behavioural reference and was not modified.

---

## 1. Scope and constraints

- All port code lives under `notchWin/`.
- Flutter sources (`lib/`, `windows/dynamicbar/`) are reference only.
- No commits or pushes have been made; every change is still local/uncommitted.
- Priority order set by the user:
  1. Fix the grey full-screen overlay / taskbar lag / jitter.
  2. Keep other application windows clickable and focusable.
  3. Remove visible performance regressions.
  4. Resume visual parity and album-art work only after the overlay is stable.

---

## 2. Architecture

### Application shell

- `src/main.rs` owns the iced `application`, the `NotchApp` state, the `Message`
  enum, `view()`, subscriptions, keyboard/mouse input and the canvas `Program`.
- A real Win32 `HWND` is obtained with `window::run` + `raw_window_handle` and
  arrives as `Message::StageHwnd(isize)`.
- `platform::start(HWND)` starts the Win32 subsystems and returns a
  `PlatformHandle` that keeps hook/tray/media threads alive.
- Platform threads push events through an `mpsc::Receiver<PlatformEvent>`, drained
  from `Message::PlatformEvents` on the iced thread.

### Rendering

- The island is one transparent iced canvas surface, not a widget tree of native
  controls.
- `IslandCanvas` is a pure canvas painter: shell, shadow, gradients, stroke,
  collapsed face and the eight expanded pages are all drawn into a `canvas::Cache`.
- Page painters live in `src/pages/` and take a `&Frame`, local width/height,
  opacity and a small state struct. This keeps them easy to port from Flutter's
  `CustomPainter` code.

### Motion and state

- `spring.rs` ports the Flutter spring profiles with a fixed `1/480 s` sub-step:
  `arrival`, `departure`, `peek`, `atmosphere`, `reveal`, `withdraw`, `contour`,
  `content_in`, `content_out`.
- `state.rs` ports `Intent` / `Phase`, `derive_phase()` and `resolve_geometry()`.
- `metrics.rs` mirrors `IslandMetrics` / `DbSize` / `DbRadius`:
  - collapsed `160 × 30`
  - hover peek `+28 × +6`
  - expanded `486 × 348`
  - stage `720 × 520` logical pixels
  - top radius now `16` on Windows; Flutter uses `0` only because its top edge is
    hidden by the physical notch.

---

## 3. Feature inventory

### Shell and interaction

- Rounded glass shell with 12-layer drop shadow, vertical tint gradient, hairline
  stroke, hover highlight and lower-lip highlight.
- Spring-driven expand / collapse / hover-peek / hide / reveal.
- 70 ms hover-open delay and 180 ms hover-cancel grace, matching Flutter.
- Drag, wheel and precise canvas press hit-testing for the pager, chevrons,
  shortcuts and page dots.
- Page cross-fade (180 ms) with cache invalidation on page change.

### Collapsed face

- Clock with digit-roll animation; only changed digits move.
- Live dot, short date and progress bar.
- Transient face priority:
  `notification > privacy > downloads > media > timer > clock`.
- Up to two priority `LiveActivity` ears with glyph, progress ring and label;
  ears are suppressed while a transient face is active.
- Timed `system.volume` notification/HUD with a 1400 ms lifetime.

### Expanded pages

Eight pages are implemented:

1. **Now** — clock, date, compact agenda, active/finished timers, privacy and
   download entries, status chips, quick tools.
2. **Media** — SMTC title/artist/app, transport controls, seek bar, placeholder
   artwork and tint hook. Real SMTC artwork is still deferred (see §7).
3. **Timers** — countdown composer, start/pause/resume/reset/delete, completion
   alerts.
4. **System** — CPU/RAM/battery via Win32, network state/SSID via WLAN, four-tile
   live strip (volume, mic, camera, Wi-Fi).
5. **Calendar** — month grid, today marker, local JSON events, empty-state hint,
   sample-calendar action.
6. **Shortcuts** — catalog, search and launch via `ShellExecuteW`.
7. **Shelf** — two image-drop slots using iced `FileDropped`, copy/open/clear,
   JSON persistence. The old note text area was removed at the user's request.
8. **Settings** — 15 main rows plus `Main`, `Pages`, `Appearance` and
   `Backup & sync` sections; sanitized values and portable JSON export/import.

### Platform integrations

- Startup registry is the source of truth:
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\notchX`. Failed writes are
  not persisted.
- Tray icon, global hotkeys (`Ctrl+Alt+Space`, `Ctrl+Alt+Right`, `Ctrl+Alt+Down`,
  `S`), outside-click collapse, session-lock polling, auto-hide and
  hide-when-covered.
- Single-instance named mutex.
- Volume via Core Audio; privacy via ConsentStore; Wi-Fi via WLAN; Downloads by
  polling the user Downloads folder. Polling was chosen over COM callbacks to
  keep the port compact and thread-safe.
- Launchers use `ShellExecuteW` / `keybd_event` so no package identity is needed.

### Persistence

All state lives under `%APPDATA%\notchWin\`:

| File | Contents |
|---|---|
| `settings.json` | Settings; missing keys get defaults. |
| `settings-export.json` | Portable snapshot from Settings → Backup & sync. |
| `calendar.json` | Calendar events; empty `[]` template created on first run. |
| `shelf.json` | Slot A/B labels and paths only (the note field was removed). |

---

## 4. Overlay / window work (the long regression)

This was the main focus after the first visual pass. It went through several
iterations; the final design is below.

### 4.1 Grey full-screen sheet

**Symptom:** a grey rectangle appeared behind the pill and covered the screen on
hover. The taskbar could hide it, which revealed the pill underneath.

**Causes and fixes, in order:**

1. An early full-screen stage (`~880 × 596` scaled to `~1284 × 900`) plus
   `DwmExtendFrameIntoClientArea(-1)` and `WS_EX_LAYERED` composited the unused
   client area as opaque grey.
   - Removed both DWM frame extension and `WS_EX_LAYERED`.
   - Reduced the stage to fixed `720 × 520` logical pixels.
2. The process was DPI-unaware. Win32 calls returned virtualized coordinates
   while iced laid out in logical pixels, so the pill was off-centre and region
   maths disagreed with the canvas.
   - `main()` now calls
     `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`
     before iced starts.
   - `place_stage()` converts `STAGE_W/H` to physical pixels with
     `GetDpiForWindow`.
   - Live inspection after the fix: window rect `420,0,1500,780` (physical,
     i.e. `1080 × 780` at 150 % DPI), centred on a `1920`-wide monitor.
3. Removing clipping entirely fixed the lag but the grey sheet came back
   smaller, because this Windows build does not reliably honour iced's
   per-pixel alpha for the whole stage.

### 4.2 Click-through

- The whole-window `WS_EX_TRANSPARENT` toggle was removed. It made the entire
  stage transparent to input once the cursor entered the island, which blocked
  other applications.
- `win32::install_hit_test()` subclasses the iced window and handles
  `WM_NCHITTEST`:
  - inside the live hit rect → normal client hit-testing;
  - outside → `HTTRANSPARENT`, so the click/focus goes to the window below.
- The hit rect is the island rect plus `HIT_PAD_X = 10` / `HIT_PAD_Y = 6`,
  recomputed from the stage HWND, current spring geometry and monitor DPI in
  `platform::sync_rects()`. It is published through atomics and read by the
  low-level mouse hook and the subclass.

### 4.3 Region clipping, flicker and lag

**Failed intermediate:** calling `SetWindowRgn(..., redraw = true)` from
`sync_rects()` at ~30 Hz forced a full-window invalidation storm. The taskbar and
tab switching lagged visibly. Caching the key and switching to `redraw = false`
removed the lag but produced a flickering rectangular ghost, because the old
pixels stayed until iced's next paint.

**Final approach:**

- `SetWindowRgn` is back, but only on **target geometry changes**, not on spring
  ticks:
  - initial collapsed shape at `StageHwnd`;
  - immediately when a growing motion starts (expand or hover peek);
  - when the phase fully settles open/closed/hidden.
- `win32::set_island_region()` caches the last `(x, y, w, h, radius)` key and
  skips unchanged calls.
- The region is a rounded rect with `CreateRoundRectRgn`, passed with
  `redraw = true` so Windows repaints the newly exposed area immediately. There
  are only a handful of calls per interaction, so there is no repaint storm.
- Shrinking is deferred until the collapse phase settles, so the shrinking pill
  is not clipped mid-animation.
- Region coordinates are DPI-scaled in `NotchApp::apply_target_region()`; the
  live collapsed region box was measured as `420,0,659,44` relative to the
  window at 150 % DPI, i.e. a centred `239 × 44` physical pill.

### 4.4 Focus and keyboard input

**Symptom:** the user had to Alt-Tab and select the pill as an application before
the mouse clicks and arrow keys worked.

**Cause:** the window starts with `WS_EX_NOACTIVATE` to stay out of the taskbar
and focus order, but that flag was only cleared after `phase.is_open()`, which is
too late for the click that performs the expansion.

**Fix:**

- `NotchApp::apply_window_focus()` clears `WS_EX_NOACTIVATE` as soon as the
  island is interactive: `phase.is_open() || hovered || intent == Expanded`.
- It is called on hover enter, on spring retarget, and on phase changes.
- `win32::activate_window()` calls `SetForegroundWindow` from the explicit
  expand/click path, so arrow keys and Escape reach the canvas without Alt-Tab.
- `WS_EX_NOACTIVATE` is restored when the island is collapsed/hidden again.

> Manual retest is still desired: an automated injected click in the verification
> harness did not generate the same hover/press sequence as a real user, so the
> focus fix is implemented but not yet confirmed by human interaction.

### 4.5 Performance settings

- Animation tick reduced from 120 Hz (`8 ms`) to 60 Hz (`16 ms`).
- Platform polling / region hit-rect sync stays at ~30 Hz (`32 ms`).
- The canvas cache is only cleared when something visibly changed (animation,
  page cross-fade, clock roll, notices, settings).
- Release build is strongly recommended; the user reported a clear improvement
  with `cargo run --release --offline` (roughly `~90 MB` active memory and much
  less lag).

---

## 5. Recent visible changes

- **Top corners rounded.** `metrics::RADIUS_TOP` is now `16` and the canvas uses
  `max(bottom_radius, top_radius)` so the top matches the bottom capsule.
  Flutter's `topRadius = 0` remains intentional there because of the notch.
- **Shelf note removed.** `ShelfState::note_text`, the note drawing block and the
  unused imports were deleted. The two image-drop slots now use the full page
  height minus a small caption strip. `shelf.json` keeps only the slot paths.
- **Focus fix** described in §4.4.
- **Grey/regression fix** described in §4.1–4.3.

---

## 6. Verification performed

### Static / unit

```powershell
cargo check --offline --message-format short
cargo test --offline
```

Both pass. Test count: **28 passed, 0 failed**.

### Win32 live inspection

A release/debug instance was launched and queried with Win32 APIs from a
DPI-aware helper:

- Window rect: `420,0,1500,780` — centred, `1080 × 780` physical at 150 % DPI.
- Window region type: `3` (`RGN_SIMPLEREGION`), not `0`/`ERROR`.
- Collapsed region box: `420,0,659,44`, i.e. centred pill.
- `WindowFromPoint` outside the pill does not return the notchX HWND.
- Screenshots during the earlier iteration showed no full-screen grey sheet;
  the final region-scaling fix was verified by the region box numbers above.

### Smoke performance

- Debug-build idle smoke test: ~`0.5 s` CPU over `3 s` (~17 % of one core).
- Release build: user-reported lower lag and ~`90 MB` working set.
- No notchX process was left running after verification.

---

## 7. Deferred / known gaps

### Album artwork (paused)

The earlier statement that SMTC does not expose other apps' artwork was wrong.
`MediaProperties::Thumbnail()` does expose it. The Flutter native reference in
`windows/dynamicbar/media/media_service.cpp` reads it with `OpenReadAsync` /
`DataReader`; `lib/modules/media/artwork_tint.dart` averages a 24 × 24 circular
sample to derive a tint.

The Rust port currently renders placeholder art and `art_color: None`. The
planned implementation is:

1. Enable the `Storage_Streams` Windows feature in `Cargo.toml`.
2. In `platform::read_session()`, read `Thumbnail()`, open the stream, use
   `DataReader::LoadAsync` + `ReadBytes`.
3. Decode JPEG/PNG/WebP with the cached `image 0.25.10` and `image-webp 0.2.4`
   crates.
4. Port the 24 × 24 circular hue average and publish it as `MediaState.art_color`.
5. Draw cover/tint on the scrubber and play button with a cache keyed by
   artwork bytes/track; fall back gracefully.

This work is intentionally paused until the overlay is stable.

### Visual parity still missing

These Flutter components/behaviours are not ported yet:

- HomeDeck panels (largest gap).
- Calendar event editor UI.
- Editable shelf note (deliberately removed per user request).
- Rich media states (sound bars, morph icon, marquee).
- Timer dial motion.
- Collapsed-face `AnimatedSwitcher` scale/fade.
- Pager slide + parallax (`PageView` / `_Parallax`).
- Animated intro banner and fuller button states.

### Other gaps

- OS toast mirroring is not in the Flutter original and is not parity-blocking.
- ActivityCenter rich demo cards were not ported; they are not in the Flutter
  reference.
- Hardware-specific scenarios still need a real-machine pass: DPI changes,
  multi-monitor, tray messages, session lock/unlock, fullscreen apps and
  Wi-Fi/ConsentStore permissions.
- Tests are unit tests only; there is no automated UI/hover/focus harness.

---

## 8. Key decisions

| Decision | Rationale |
|---|---|
| `window::run` + `raw_window_handle` → `StageHwnd` | Only supported way to reach the live iced HWND. |
| `WM_NCHITTEST` subclass returning `HTTRANSPARENT` | Other apps stay clickable without disabling input on the island itself. |
| DPI awareness v2 in `main()` | Removes Win32/iced coordinate mismatch and fixes centring. |
| Region updates only on target change, `redraw = true` | Kills the repaint storm while avoiding the stale-pixel flicker of `redraw = false`. |
| 60 Hz tick + 30 Hz platform poll | Halves debug CPU cost without visible motion loss. |
| `ShellExecuteW` / `keybd_event` | Works unpackaged; WinRT launchers need package identity. |
| Polling for sensors/downloads | Compact, thread-safe, no COM callback lifetime hazards. |
| Startup registry as source of truth | Failed writes are not silently remembered in app state. |
| iced `FileDropped` + JSON for Shelf | Avoids an OLE `IDropTarget` implementation. |
| Local `calendar.json` and settings snapshots | Unpackaged-friendly and easy to inspect/edit. |

---

## 9. File index

| File | Role |
|---|---|
| `src/main.rs` | iced app, state, messages, view, canvas shell, subscriptions, focus/region helpers |
| `src/platform.rs` | Win32 subsystem startup, stage placement, SMTC watcher, sensors, hotkeys, tray, rect sync |
| `src/win32.rs` | Overlay styles, HWND subclass hit-testing, region clipping, focus helper, monitor/DPI helpers |
| `src/spring.rs` | Spring integration and Flutter motion profiles |
| `src/metrics.rs` | Geometry and radius constants |
| `src/state.rs` | Intent/Phase state machine |
| `src/settings.rs` | Settings model, defaults, sanitization, persistence |
| `src/calendar_events.rs` | Local calendar JSON parsing/filtering |
| `src/activities.rs` | Live activity priority and visibility |
| `src/notifications.rs` | Timed notices and in-app notification queue |
| `src/downloads.rs` | Downloads-folder polling |
| `src/clock.rs` | Clock digit-roll state |
| `src/timers.rs` | Countdown/stopwatch model |
| `src/pages/*.rs` | Pure canvas page painters |
| `Cargo.toml` | Dependencies and Windows feature flags |
| `README.md` | Build/run and data-file overview |
| `ROADMAP.md` | Phase-by-phase parity roadmap |
| `IMPLEMENTATION_STATUS.md` | This document |

---

## 10. Current next steps

1. **Manual retest** of hover → first click → arrow keys without Alt-Tab, and
   confirm the grey rectangle/flicker is gone in the release build.
2. If the region/focus behaviour is clean, resume **album artwork** (§7).
3. Then work through the visual parity list (HomeDeck first, then media/timer/
   pager motion), still inside `notchWin/`.
