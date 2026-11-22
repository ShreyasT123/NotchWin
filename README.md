# notchWin

Rust/iced port of the notchX Dynamic Island overlay. The Flutter implementation
in the repository root is the behavioural reference; all port code lives here
in `notchWin/`.

## Build and run

```powershell
cd notchWin
cargo build --release --offline
cargo run --release --offline
cargo test --offline
cargo check --offline --message-format short
```

Use the release build for normal runs; the debug build is significantly heavier
and was used only for development smoke tests.

The binary is a frameless, always-on-top Win32 overlay centred over the primary
monitor. Only the live island rect accepts input; everything else falls through
to the windows underneath via a `WM_NCHITTEST` subclass. Use the tray icon or
`Ctrl+Alt+Space` to open it; `Ctrl+Alt+Right` / `Ctrl+Alt+Down` change pages and
collapse it. Press `S` while expanded to jump to Settings.

## Data files

All persistent state lives under `%APPDATA%\notchWin\`:

| File | Purpose |
|---|---|
| `settings.json` | Settings; written on every change. Missing keys get defaults. |
| `settings-export.json` | Portable snapshot created by Settings → Backup & sync. |
| `calendar.json` | Calendar events shown on the Calendar page. |
| `shelf.json` | Two image-slot paths pinned to the Shelf page (no note field). |

`calendar.json` is an array of events. Editing it is picked up the next time
you change the selected day or month:

```json
[
  { "date": "2026-11-22", "time": "09:30", "title": "Stand-up" },
  { "date": "2026-11-22", "time": "", "title": "Rent due", "notified": true }
]
```

An empty `[]` template is created on first run.

## Implementation notes

- `spring.rs`, `metrics.rs` and `state.rs` mirror the Flutter motion/geometry
  implementation; `src/pages/` are pure canvas painters.
- Win32 integration (`platform.rs`, `win32.rs`) uses the real iced HWND. A
  `WM_NCHITTEST` subclass returns `HTTRANSPARENT` outside the live island rect;
  the whole window is never made transparent to input.
- The process is per-monitor DPI aware, and the clipping region is updated only
  when the target island shape changes. That avoids both the grey sheet and the
  repaint storm that came from changing the region every animation tick.
- `WS_EX_NOACTIVATE` is cleared as soon as the island is interactive, and an
  explicit expand focuses the window so arrow keys work without Alt-Tab.
- The launcher uses `ShellExecuteW` so the app does not need package identity.
- Volume/mic/camera/Wi-Fi and Downloads are polled rather than using COM
  callbacks; this trades a little latency for a much smaller surface area.
- See `ROADMAP.md` for phase-by-phase parity notes and
  `IMPLEMENTATION_STATUS.md` for the detailed current state, verification and
  deferred work.
