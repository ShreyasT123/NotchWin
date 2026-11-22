// Win32 helpers for the overlay window.
//
// Responsibilities:
//  - Keep the window TOPMOST and tool-styled (no taskbar button).
//  - Hit-test subclass: clicks outside the live island rect fall through to
//    the windows beneath instead of being swallowed by the large transparent
//    stage window.
//
// winit/iced already make the window transparent (`window::Settings.transparent`
// -> DwmEnableBlurBehindWindow with an empty region). Do NOT also add
// WS_EX_LAYERED or DwmExtendFrameIntoClientArea(-1): those make the client area
// composite as an opaque grey sheet on Windows 11.

use std::sync::{
    atomic::{AtomicIsize, Ordering},
    Arc, Mutex, OnceLock,
};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{
            CreateRectRgn, CreateRoundRectRgn, GetMonitorInfoW, MonitorFromWindow,
            SetWindowRgn, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
        },
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CallWindowProcW, DefWindowProcW, GetWindowLongPtrW, SetForegroundWindow,
                SetWindowLongPtrW, SetWindowPos, GWLP_WNDPROC, GWL_EXSTYLE, HTTRANSPARENT,
                HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_NOACTIVATE,
                WM_NCHITTEST, WNDPROC, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                WS_EX_TOPMOST, WS_EX_TRANSPARENT,
            },
        },
    },
};

use crate::platform::SharedRects;

/// Original wndproc we subclassed, stored as a raw pointer-sized value.
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);
/// Live hit rectangles shared with the mouse hook / iced update loop.
static HIT_RECTS: OnceLock<Arc<SharedRects>> = OnceLock::new();
/// Last applied clipping region. Updated only on intent/phase changes, never
/// on every animation tick.
static LAST_REGION: Mutex<Option<(i32, i32, i32, i32, i32)>> = Mutex::new(None);

/// Apply the window styles to `hwnd`.
/// Safe to call again after a session lock / Explorer restart.
pub fn setup_overlay(hwnd: HWND) {
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let styles = current
            | WS_EX_TOPMOST.0 as isize
            | WS_EX_TOOLWINDOW.0 as isize
            | WS_EX_NOACTIVATE.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, styles);

        // Keep on top.
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Subclass the iced window so only the live island rect is interactive.
/// Everywhere else returns `HTTRANSPARENT`, which lets clicks and focus reach
/// the application underneath. This replaces whole-window WS_EX_TRANSPARENT
/// toggling: that flag made the entire 1284x900 stage clickable as soon as the
/// cursor entered the island, blocking every other app.
pub fn install_hit_test(hwnd: HWND, shared: Arc<SharedRects>) {
    let _ = HIT_RECTS.set(shared);
    unsafe {
        let original = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        ORIGINAL_WNDPROC.store(original, Ordering::SeqCst);
        let proc: extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
            hit_test_proc;
        let _ = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc as usize as isize);
    }
}

extern "system" fn hit_test_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCHITTEST {
        if let Some(shared) = HIT_RECTS.get() {
            // WM_NCHITTEST packs client-independent screen coords into LPARAM.
            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let left = shared.hit_left.load(Ordering::Relaxed);
            let top = shared.hit_top.load(Ordering::Relaxed);
            let right = shared.hit_right.load(Ordering::Relaxed);
            let bottom = shared.hit_bottom.load(Ordering::Relaxed);

            if x < left || x > right || y < top || y > bottom {
                return LRESULT(HTTRANSPARENT as isize);
            }
        }
    }

    let original = ORIGINAL_WNDPROC.load(Ordering::SeqCst);
    unsafe {
        if original == 0 {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let proc: WNDPROC = Some(std::mem::transmute::<
            isize,
            unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
        >(original));
        CallWindowProcW(proc, hwnd, msg, wparam, lparam)
    }
}

/// Enable or disable click-through for the stage window.
pub fn set_click_through(hwnd: HWND, passthrough: bool) {
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = if passthrough {
            current | WS_EX_TRANSPARENT.0 as isize
        } else {
            current & !(WS_EX_TRANSPARENT.0 as isize)
        };
        if next != current {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
        }
    }
}

/// Clip the stage window to the island's rounded shape.
///
/// Called only when the target geometry changes (expand / collapse / peek),
/// never on every spring tick. `redraw = true` is important: `false` leaves the
/// old transparent pixels behind until iced's next paint, which shows up as a
/// flickering rectangle around the pill.
pub fn set_island_region(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, radius: i32) {
    let key = (x, y, w, h, radius);
    {
        let mut last = match LAST_REGION.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if last.as_ref() == Some(&key) {
            return;
        }
        *last = Some(key);
    }

    unsafe {
        let region = if w <= 0 || h <= 0 {
            CreateRectRgn(0, 0, 0, 0)
        } else {
            let r = radius.clamp(0, w.min(h) / 2) * 2;
            CreateRoundRectRgn(x, y, x + w, y + h, r, r)
        };
        if !region.0.is_null() {
            // SetWindowRgn takes ownership of the region.
            let _ = SetWindowRgn(hwnd, Some(region), true);
        }
    }
}

/// Keep the stage out of the focus/tab order while collapsed. An expanded
/// island needs keyboard focus for arrow-key navigation, so the flag is cleared
/// on expand and restored on collapse.
pub fn set_no_activate(hwnd: HWND, no_activate: bool) {
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = if no_activate {
            current | WS_EX_NOACTIVATE.0 as isize
        } else {
            current & !(WS_EX_NOACTIVATE.0 as isize)
        };
        if next != current {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
        }
    }
}

/// Give the island keyboard focus after an explicit expand/click.
///
/// The window starts as `WS_EX_NOACTIVATE` so it never steals focus merely by
/// being topmost. Once the user clicks to expand it, however, arrow keys and
/// Escape must reach the canvas. `SetForegroundWindow` is allowed here because
/// the call happens directly inside the click/hotkey that opened the island.
pub fn activate_window(hwnd: HWND) {
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Re-assert TOPMOST after lock/wake/Explorer-restart.
pub fn reassert_topmost(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Returns `(left, top, width, height)` of the monitor work-area in physical pixels.
#[allow(dead_code)]
pub fn get_monitor_work_area(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut info).as_bool() {
            let r = info.rcWork;
            Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
        } else {
            None
        }
    }
}

/// DPI of the monitor hosting `hwnd` (96 = 100%).
#[allow(dead_code)]
pub fn get_dpi(hwnd: HWND) -> u32 {
    unsafe { GetDpiForWindow(hwnd) }
}
