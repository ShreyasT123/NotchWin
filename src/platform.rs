// platform.rs — Win32 / WinRT platform integration for notchWin.
//
// Three independent subsystems, each running on its own OS thread or callback:
//
//   HwndSetup     — DWM alpha + overlay styles, applied once HWND is known.
//   MediaWatcher  — GlobalSystemMediaTransportControlsSessionManager (SMTC).
//   TrayIcon      — Shell_NotifyIcon tray presence + context menu.
//   HotkeyWindow  — RegisterHotKey on a hidden message window.
//   MouseHook     — WH_MOUSE_LL for auto-hide proximity and outside-click.
//
// All cross-thread communication goes through `std::sync::mpsc` channels
// whose Sender<PlatformEvent> is cloned into each subsystem. The iced
// subscription polls the receiver on every frame via try_recv.

#![allow(dead_code)]
#![allow(unsafe_code)]
#![allow(unsafe_op_in_unsafe_fn)]

use std::sync::{
    atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{
            RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_DOWN, VK_RIGHT,
            VK_SPACE,
        },
        Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
            NOTIFYICONDATAW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CallNextHookEx, CreatePopupMenu, CreateWindowExW,
            DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
            GetCursorPos, GetMessageW, PostMessageW, PostQuitMessage,
            RegisterClassExW, SetForegroundWindow, SetWindowsHookExW,
            TrackPopupMenu, TranslateMessage, UnhookWindowsHookEx, CS_HREDRAW,
            CS_VREDRAW, HHOOK, MF_SEPARATOR, MF_STRING, MSG, TPM_LEFTALIGN,
            TPM_RETURNCMD, TPM_TOPALIGN, WH_MOUSE_LL, WM_APP, WM_COMMAND,
            WM_HOTKEY, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
        },
    },
};

/// Raw Win32 handles are `*mut c_void` and therefore neither `Send` nor
/// `Sync`. They are only ever used from the thread that created them (or, for
/// the stage HWND, from the iced thread), so moving the value between threads
/// through a channel is safe as long as it is not dereferenced there.
struct SendHandle<T>(T);
unsafe impl<T> Send for SendHandle<T> {}

// ── Event types sent back to iced ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PlatformEvent {
    /// SMTC: media state changed.
    MediaChanged(Option<MediaInfo>),
    /// Global hotkey fired.
    HotkeyToggle,
    HotkeyExpand,
    HotkeyHide,
    /// Tray menu item chosen.
    TrayQuit,
    TrayPause,
    TraySettings,
    TrayOpen,
    /// Mouse hook: cursor is near the reveal zone (top of screen).
    CursorNearTop(bool),
    /// Mouse hook: a click landed outside the island rect — collapse.
    OutsideClick,
    /// Default render endpoint volume changed (level is 0.0–1.0).
    VolumeChanged { available: bool, level: f32, muted: bool },
    /// CapabilityAccessManager reports a camera/microphone in use.
    PrivacyChanged { microphone: bool, camera: bool },
    /// WLAN interface state / connected SSID changed.
    NetworkChanged { connected: bool, ssid: String },
    /// Downloads folder watcher: a download started, progressed or finished.
    Download(DownloadEvent),
}

// ── System status snapshots ────────────────────────────────────────────────────

/// Last known system volume. Mirrors Flutter's `VolumeStatus`.
#[derive(Debug, Clone, Copy, Default)]
pub struct VolumeStatus {
    pub available: bool,
    pub level: f32,
    pub muted: bool,
}

/// Last known privacy indicators. Mirrors Flutter's `PrivacyStatus`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PrivacyStatus {
    pub microphone: bool,
    pub camera: bool,
}

/// Last known Wi-Fi state. Mirrors Flutter's `NetworkStatus` (connected/SSID).
#[derive(Debug, Clone, Default)]
pub struct NetworkStatus {
    pub connected: bool,
    pub ssid: String,
}

impl PrivacyStatus {
    pub fn any_in_use(self) -> bool {
        self.microphone || self.camera
    }
}

/// Events from the Downloads folder watcher. Mirrors the native Flutter
/// `downloads` service's `started` / `progress` / `finished` events.
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    Started { id: String, name: String },
    Progress { id: String, name: String, bytes: u64, bytes_per_second: f64 },
    Finished { id: String, name: String, completed: bool },
}

// ── MediaInfo (mirrors MediaState used by the canvas page painter) ─────────────

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub title:        String,
    pub artist:       String,
    pub album:        String,
    pub app_name:     String,
    pub is_playing:   bool,
    pub position_secs: f32,
    pub duration_secs: f32,
}

// ── Shared rectangles for the mouse hook ──────────────────────────────────────
// Written on the iced thread, read inside the hook callback (which runs on a
// dedicated hook thread). An SRWLOCK is too much; a pair of atomics for the
// four corners (stored as i32 screen-pixel values) is sufficient.

#[derive(Default)]
pub struct SharedRects {
    /// HWND of the iced stage, stored as `isize` so this struct stays `Send`.
    pub stage_hwnd: AtomicIsize,
    // Island hit-rect in screen pixels (grow immediately, shrink late).
    pub hit_left:   AtomicI32,
    pub hit_top:    AtomicI32,
    pub hit_right:  AtomicI32,
    pub hit_bottom: AtomicI32,
    // Reveal zone (wider band at the top of the screen).
    pub rev_left:   AtomicI32,
    pub rev_right:  AtomicI32,
    pub rev_bottom: AtomicI32,
    // Set true while a mouse button is held — no style changes during a drag.
    pub button_down: AtomicBool,
}

// ── PlatformHandle — returned to main, keeps subsystems alive ─────────────────

pub struct PlatformHandle {
    pub events:      Receiver<PlatformEvent>,
    pub shared:      Arc<SharedRects>,
    hotkey_hwnd:     Option<HWND>,
    tray_hwnd:       Option<HWND>,
    hook:            Option<HHOOK>,
    hook_thread_tx:  Option<Sender<()>>,
    system_stop_tx:  Option<Sender<()>>,
    downloads_stop_tx: Option<Sender<()>>,
}

impl Drop for PlatformHandle {
    fn drop(&mut self) {
        // Signal the hook thread to exit.
        if let Some(tx) = self.hook_thread_tx.take() {
            let _ = tx.send(());
        }
        if let Some(tx) = self.system_stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(tx) = self.downloads_stop_tx.take() {
            let _ = tx.send(());
        }
        unsafe {
            if let Some(h) = self.hook.take() {
                let _ = UnhookWindowsHookEx(h);
            }
            if let Some(w) = self.tray_hwnd.take() {
                // Remove tray icon before the window goes away.
                let mut nid = blank_nid(w, 1);
                let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
                let _ = DestroyWindow(w);
            }
            if let Some(w) = self.hotkey_hwnd.take() {
                let _ = DestroyWindow(w);
            }
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Spin up all platform subsystems and return a handle.
/// Call this once, from the iced boot closure, after `NotchApp::default()`.
pub fn start(stage_hwnd: HWND) -> PlatformHandle {
    let (tx, rx) = mpsc::channel::<PlatformEvent>();
    let shared   = Arc::new(SharedRects::default());
    shared.stage_hwnd.store(stage_hwnd.0 as isize, Ordering::Relaxed);
    STAGE_HWND.store(stage_hwnd.0 as isize, Ordering::Relaxed);

    // Apply overlay styles. Transparency is owned by winit/iced; our hit-test
    // subclass makes everything outside the island rect click-through.
    crate::win32::setup_overlay(stage_hwnd);
    crate::win32::install_hit_test(stage_hwnd, Arc::clone(&shared));

    // Position stage at top-centre of primary monitor.
    place_stage(stage_hwnd);

    // Start SMTC media watcher on its own thread.
    start_media_watcher(tx.clone());

    // Start tray icon on its own thread.
    let tray_hwnd = start_tray(tx.clone());

    // Start hotkey window on its own thread.
    let hotkey_hwnd = start_hotkeys(tx.clone());

    // Start mouse hook on its own thread.
    let (hook_stop_tx, hook) = start_mouse_hook(tx.clone(), Arc::clone(&shared));

    // Volume / privacy sensor thread (Core Audio + ConsentStore registry).
    let (system_stop_tx, system_stop_rx) = mpsc::channel::<()>();
    start_system_monitor(tx.clone(), system_stop_rx);

    // Downloads folder watcher (partial-file tracking).
    let (downloads_stop_tx, downloads_stop_rx) = mpsc::channel::<()>();
    start_downloads_watcher(tx, downloads_stop_rx);

    PlatformHandle {
        events:          rx,
        shared,
        hotkey_hwnd:     hotkey_hwnd,
        tray_hwnd:       tray_hwnd,
        hook:            hook,
        hook_thread_tx:  Some(hook_stop_tx),
        system_stop_tx:  Some(system_stop_tx),
        downloads_stop_tx: Some(downloads_stop_tx),
    }
}

// ── Stage placement ───────────────────────────────────────────────────────────
// Position the stage at the top-centre of the primary monitor.
// Called once at startup; re-call on WM_DISPLAYCHANGE (future).

fn place_stage(hwnd: HWND) {
    use windows::Win32::{
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
        },
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER},
        },
    };

    unsafe {
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut info).as_bool() {
            // The process is per-monitor DPI aware, so SetWindowPos takes
            // physical pixels while STAGE_W/H are iced logical pixels.
            let dpi   = GetDpiForWindow(hwnd);
            let scale = (dpi as f32 / 96.0).max(1.0);
            let sw    = (crate::metrics::STAGE_W * scale) as i32;
            let sh    = (crate::metrics::STAGE_H * scale) as i32;
            let mon_w = info.rcMonitor.right - info.rcMonitor.left;
            let x     = info.rcMonitor.left + (mon_w - sw) / 2;
            let y     = info.rcMonitor.top;
            let _ = SetWindowPos(hwnd, None, x, y, sw, sh, SWP_NOACTIVATE | SWP_NOZORDER);
        }
    }
}

// ── SMTC media watcher ────────────────────────────────────────────────────────
// Uses GlobalSystemMediaTransportControlsSessionManager (WinRT) to track the
// currently playing media session.  Runs in a background thread; pushes
// MediaChanged events when state changes.

fn start_media_watcher(tx: Sender<PlatformEvent>) {
    thread::Builder::new()
        .name("smtc-watcher".into())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            smtc_loop(tx);
        })
        .expect("spawn smtc thread");
}

/// Block until a WinRT async operation finishes. `IAsyncOperation` in the
/// 0.62 bindings no longer exposes `.get()`; polling `Status()` keeps this
/// dependency-free and is only used during watcher startup / song changes.
fn await_async<T: windows::core::RuntimeType + 'static>(
    op: windows_future::IAsyncOperation<T>,
) -> Option<T> {
    for _ in 0..500 {
        match op.Status().ok()?.0 {
            1 => return op.GetResults().ok(), // Completed
            0 => thread::sleep(Duration::from_millis(5)), // Started
            _ => return None, // Canceled / Error
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn smtc_loop(tx: Sender<PlatformEvent>) {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
    };

    // Initialize WinRT on this thread.
    let _ro = unsafe {
        windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        )
    };

    let manager = match Manager::RequestAsync().ok().and_then(await_async) {
        Some(m) => m,
        None => return,
    };

    let mut last_signature = String::new();

    loop {
        let info = read_session(&manager);

        // Only push when something the UI draws actually changed. Comparing a
        // signature (not just the title) keeps pause/resume and playback
        // position in sync with the real session.
        let signature = info.as_ref().map(|i| {
            format!(
                "{}|{}|{}|{}|{}|{}",
                i.title,
                i.artist,
                i.is_playing,
                i.position_secs as u32 / 5,
                i.duration_secs as u32,
                i.app_name,
            )
        }).unwrap_or_default();

        if signature != last_signature {
            last_signature = signature;
            if tx.send(PlatformEvent::MediaChanged(info)).is_err() {
                break;
            }
        }

        thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(target_os = "windows")]
fn read_session(
    manager: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager,
) -> Option<MediaInfo> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;

    let session = manager.GetCurrentSession().ok()?;

    let props = await_async(session.TryGetMediaPropertiesAsync().ok()?)?;

    let title  = props.Title().ok()?.to_string();
    let artist = props.Artist().ok()?.to_string();
    let album  = props.AlbumTitle().ok()?.to_string();

    let playback = session.GetPlaybackInfo().ok()?;
    let status   = playback.PlaybackStatus().ok()?;
    let is_playing = status == Status::Playing;

    let timeline  = session.GetTimelineProperties().ok()?;
    let position  = timeline.Position().ok()?.Duration as f32 / 10_000_000.0;
    let end_time  = timeline.EndTime().ok()?.Duration as f32 / 10_000_000.0;

    let app_id  = session.SourceAppUserModelId().ok()?.to_string();
    let app_name = derive_app_name(&app_id);

    Some(MediaInfo {
        title,
        artist,
        album,
        app_name,
        is_playing,
        position_secs: position,
        duration_secs: end_time,
    })
}

fn derive_app_name(source_app_id: &str) -> String {
    // SourceAppUserModelId is e.g. "Spotify.exe" or a package family name.
    let trimmed = source_app_id.split('!').next().unwrap_or(source_app_id);
    let file = trimmed
        .split(|c| c == '/' || c == '\\')
        .last()
        .unwrap_or(trimmed);
    let stem = if file.to_lowercase().ends_with(".exe") {
        &file[..file.len() - 4]
    } else {
        file
    };
    if stem.is_empty() {
        return String::new();
    }
    let mut s = stem.to_string();
    if let Some(first) = s.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    s
}

// ── System sensors: volume + privacy ──────────────────────────────────────────
// Mirrors `system_service.cpp` in the Flutter Windows runner. Volume uses the
// default multimedia render endpoint; privacy uses the same
// CapabilityAccessManager ConsentStore keys as the native implementation.

fn start_system_monitor(tx: Sender<PlatformEvent>, stop_rx: Receiver<()>) {
    thread::Builder::new()
        .name("system-monitor".into())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            system_monitor_loop(tx, stop_rx);
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (tx, stop_rx);
            }
        })
        .expect("spawn system-monitor thread");
}

#[cfg(target_os = "windows")]
fn system_monitor_loop(tx: Sender<PlatformEvent>, stop_rx: Receiver<()>) {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{ERROR_NO_MORE_ITEMS, HANDLE, WIN32_ERROR},
            Media::Audio::Endpoints::IAudioEndpointVolume,
            System::{
                Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
                Registry::{
                    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
                    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
                },
            },
        },
    };

    let _ro = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

    /// Open the default render endpoint once; reacquire on device change.
    fn acquire_endpoint() -> Option<IAudioEndpointVolume> {
        use windows::Win32::{
            Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator},
            System::Com::{CLSCTX_ALL, CoCreateInstance},
        };
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
            device.Activate(CLSCTX_ALL, None).ok()
        }
    }

    /// True when any app under this consent-store key has `LastUsedTimeStop == 0`.
    unsafe fn key_has_active_apps(key: HKEY) -> bool {
        let mut index = 0_u32;
        loop {
            let mut name = [0_u16; 256];
            let mut name_len = name.len() as u32;
            let rc = RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            );
            if rc == ERROR_NO_MORE_ITEMS || rc != WIN32_ERROR(0) {
                break;
            }
            index += 1;
            let child_name = String::from_utf16_lossy(&name[..name_len as usize]);
            let child_name = windows::core::HSTRING::from(child_name);
            let mut child = HKEY::default();
            if RegOpenKeyExW(key, &child_name, None, KEY_READ, &mut child).is_err() {
                continue;
            }
            let mut data = [0_u8; 8];
            let mut size = data.len() as u32;
            let query = RegQueryValueExW(
                child,
                windows::core::w!("LastUsedTimeStop"),
                None,
                None,
                Some(data.as_mut_ptr()),
                Some(&mut size),
            );
            let _ = RegCloseKey(child);
            if query == WIN32_ERROR(0) && size >= 8 {
                let stopped = u64::from_le_bytes(data[..8].try_into().unwrap());
                if stopped == 0 {
                    return true;
                }
            }
        }
        false
    }

    unsafe fn capability_in_use(root: HKEY, capability: &str) -> bool {
        let subkey = format!(
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{capability}"
        );
        let subkey = windows::core::HSTRING::from(subkey);
        let mut key = HKEY::default();
        if RegOpenKeyExW(root, &subkey, None, KEY_READ, &mut key).is_err() {
            return false;
        }
        let mut in_use = key_has_active_apps(key);

        // Packaged apps live directly under the capability key; unpackaged
        // desktop apps are collected under `NonPackaged`.
        if !in_use {
            let non_packaged = windows::core::w!("NonPackaged");
            let mut np = HKEY::default();
            if RegOpenKeyExW(key, non_packaged, None, KEY_READ, &mut np).is_ok() {
                in_use = key_has_active_apps(np);
                let _ = RegCloseKey(np);
            }
        }
        let _ = RegCloseKey(key);
        in_use
    }

    fn read_privacy() -> PrivacyStatus {
        unsafe {
            PrivacyStatus {
                microphone: capability_in_use(HKEY_CURRENT_USER, "microphone")
                    || capability_in_use(HKEY_LOCAL_MACHINE, "microphone"),
                camera: capability_in_use(HKEY_CURRENT_USER, "webcam")
                    || capability_in_use(HKEY_LOCAL_MACHINE, "webcam"),
            }
        }
    }

    /// Read the SSID of the connected WLAN interface via WlanQueryInterface.
    fn read_network(client: Option<HANDLE>) -> NetworkStatus {
        use windows::Win32::NetworkManagement::WiFi::{
            wlan_intf_opcode_current_connection, wlan_interface_state_connected,
            WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST, WlanEnumInterfaces,
            WlanFreeMemory, WlanQueryInterface,
        };
        let Some(client) = client else {
            return NetworkStatus::default();
        };
        unsafe {
            let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(client, None, &mut list_ptr) != 0 || list_ptr.is_null() {
                return NetworkStatus::default();
            }
            let mut result = NetworkStatus::default();
            let count = (*list_ptr).dwNumberOfItems as usize;
            for i in 0..count {
                let info = &*((*list_ptr).InterfaceInfo.as_ptr().add(i));
                if info.isState != wlan_interface_state_connected {
                    continue;
                }
                let mut size = 0_u32;
                let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
                let rc = WlanQueryInterface(
                    client,
                    &info.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut size,
                    &mut data,
                    None,
                );
                if rc == 0 && !data.is_null() && size as usize >= std::mem::size_of::<WLAN_CONNECTION_ATTRIBUTES>() {
                    let attributes = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
                    let ssid = &attributes.wlanAssociationAttributes.dot11Ssid;
                    let len = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
                    result.ssid = String::from_utf8_lossy(&ssid.ucSSID[..len]).into_owned();
                    result.connected = true;
                }
                if !data.is_null() {
                    WlanFreeMemory(data);
                }
                break;
            }
            WlanFreeMemory(list_ptr as *const core::ffi::c_void);
            result
        }
    }

    // One WLAN client handle is reused for the lifetime of the monitor.
    let wlan_client = {
        use windows::Win32::NetworkManagement::WiFi::WlanOpenHandle;
        let mut version = 0_u32;
        let mut handle = HANDLE::default();
        if unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) } == 0 {
            Some(handle)
        } else {
            None
        }
    };

    let mut endpoint = acquire_endpoint();
    let mut last_volume: Option<(f32, bool)> = None;
    let mut last_privacy = PrivacyStatus::default();
    let mut next_privacy_poll = Instant::now();
    let mut last_network = NetworkStatus::default();
    let mut next_network_poll = Instant::now();

    loop {
        if stop_rx.recv_timeout(Duration::from_millis(200)).is_ok() {
            break;
        }

        let volume = endpoint.as_ref().and_then(|ep| unsafe {
            Some((
                ep.GetMasterVolumeLevelScalar().ok()?.clamp(0.0, 1.0),
                ep.GetMute().ok()?.as_bool(),
            ))
        });
        if volume.is_none() {
            // Endpoint can disappear when the default device changes.
            endpoint = acquire_endpoint();
        }

        let signature = volume.map(|(level, muted)| (level, muted));
        if signature != last_volume {
            last_volume = signature;
            let (level, muted) = signature.unwrap_or((0.0, true));
            if tx
                .send(PlatformEvent::VolumeChanged {
                    available: volume.is_some(),
                    level,
                    muted,
                })
                .is_err()
            {
                break;
            }
        }

        if Instant::now() >= next_privacy_poll {
            next_privacy_poll = Instant::now() + Duration::from_millis(1000);
            let privacy = read_privacy();
            if privacy.microphone != last_privacy.microphone
                || privacy.camera != last_privacy.camera
            {
                last_privacy = privacy;
                if tx
                    .send(PlatformEvent::PrivacyChanged {
                        microphone: privacy.microphone,
                        camera: privacy.camera,
                    })
                    .is_err()
                {
                    break;
                }
            }
        }

        if Instant::now() >= next_network_poll {
            next_network_poll = Instant::now() + Duration::from_millis(2000);
            let network = read_network(wlan_client);
            if network.connected != last_network.connected || network.ssid != last_network.ssid {
                last_network = network.clone();
                if tx
                    .send(PlatformEvent::NetworkChanged {
                        connected: network.connected,
                        ssid: network.ssid,
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
    }
    if let Some(client) = wlan_client {
        unsafe { windows::Win32::NetworkManagement::WiFi::WlanCloseHandle(client, None) };
    }
    unsafe { CoUninitialize() };
}

// ── Downloads folder watcher ──────────────────────────────────────────────────
// Mirrors `downloads_service.cpp`: browsers create files with a partial
// extension while downloading, then rename them to the final name. Watching
// file sizes gives us progress without needing per-browser integrations.

const PARTIAL_EXTENSIONS: [&str; 8] = [
    ".crdownload", ".part", ".download", ".partial",
    ".opdownload", ".tmp", ".!ut", ".aria2",
];

fn partial_display_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    PARTIAL_EXTENSIONS
        .iter()
        .find(|ext| lower.ends_with(**ext))
        .map(|ext| name[..name.len() - ext.len()].to_string())
}

fn start_downloads_watcher(tx: Sender<PlatformEvent>, stop_rx: Receiver<()>) {
    thread::Builder::new()
        .name("downloads-watcher".into())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            downloads_loop(tx, stop_rx);
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (tx, stop_rx);
            }
        })
        .expect("spawn downloads-watcher thread");
}

#[cfg(target_os = "windows")]
struct DownloadRecord {
    name: String,
    bytes: u64,
    observed_at: Instant,
    rate: f64,
    last_emit: Instant,
}

#[cfg(target_os = "windows")]
fn downloads_folder() -> Option<std::path::PathBuf> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath},
    };
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Downloads, Default::default(), None).ok()?;
        let text = path.to_string().unwrap_or_default();
        CoTaskMemFree(Some(path.0 as *const core::ffi::c_void));
        Some(std::path::PathBuf::from(text))
    }
}

#[cfg(target_os = "windows")]
fn downloads_loop(tx: Sender<PlatformEvent>, stop_rx: Receiver<()>) {
    use std::collections::HashMap;

    let Some(folder) = downloads_folder() else {
        let _ = stop_rx.recv();
        return;
    };
    let mut active: HashMap<String, DownloadRecord> = HashMap::new();

    loop {
        if stop_rx.recv_timeout(Duration::from_millis(400)).is_ok() {
            break;
        }
        let now = Instant::now();

        let entries: Vec<(String, u64)> = std::fs::read_dir(&folder)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if !entry.file_type().ok()?.is_file() {
                    return None;
                }
                let bytes = entry.metadata().ok()?.len();
                Some((entry.file_name().to_string_lossy().into_owned(), bytes))
            })
            .collect();

        let mut present: Vec<String> = Vec::new();

        for (file_name, bytes) in entries {
            let Some(name) = partial_display_name(&file_name) else {
                continue;
            };
            let id = file_name.clone();
            present.push(id.clone());

            match active.get_mut(&id) {
                None => {
                    if active.len() >= 8 {
                        continue;
                    }
                    let _ = tx.send(PlatformEvent::Download(DownloadEvent::Started {
                        id: id.clone(),
                        name: name.clone(),
                    }));
                    let _ = tx.send(PlatformEvent::Download(DownloadEvent::Progress {
                        id: id.clone(),
                        name: name.clone(),
                        bytes,
                        bytes_per_second: 0.0,
                    }));
                    active.insert(
                        id,
                        DownloadRecord {
                            name,
                            bytes,
                            observed_at: now,
                            rate: 0.0,
                            last_emit: now,
                        },
                    );
                }
                Some(record) => {
                    let seconds = now.duration_since(record.observed_at).as_secs_f64();
                    if bytes != record.bytes && seconds > 0.0 {
                        let instant_rate = (bytes.saturating_sub(record.bytes)) as f64 / seconds;
                        record.rate = record.rate * 0.6 + instant_rate * 0.4;
                        record.bytes = bytes;
                        record.observed_at = now;
                        if now.duration_since(record.last_emit) >= Duration::from_millis(250) {
                            record.last_emit = now;
                            let _ = tx.send(PlatformEvent::Download(DownloadEvent::Progress {
                                id: id.clone(),
                                name: record.name.clone(),
                                bytes,
                                bytes_per_second: record.rate,
                            }));
                        }
                    }
                }
            }
        }

        // A partial file disappearing means either completion (renamed to the
        // final name) or cancellation. Both end the live activity.
        let finished: Vec<(String, String, bool)> = active
            .iter()
            .filter(|(id, _)| !present.contains(id))
            .map(|(id, record)| {
                let completed = folder.join(&record.name).is_file();
                (id.clone(), record.name.clone(), completed)
            })
            .collect();
        for (id, name, completed) in finished {
            active.remove(&id);
            let _ = tx.send(PlatformEvent::Download(DownloadEvent::Finished {
                id,
                name,
                completed,
            }));
        }
    }
}

// ── Tray icon ─────────────────────────────────────────────────────────────────

const WM_TRAY: u32 = WM_APP + 1;
const ID_TRAY_OPEN: usize     = 1001;
const ID_TRAY_PAUSE: usize    = 1002;
const ID_TRAY_SETTINGS: usize = 1003;
const ID_TRAY_QUIT: usize     = 1004;

/// The tray window procedure needs access to the event sender. It is set once
/// before the window is created and never replaced.
static TRAY_TX: Mutex<Option<Sender<PlatformEvent>>> = Mutex::new(None);

fn start_tray(tx: Sender<PlatformEvent>) -> Option<HWND> {
    // The tray lives on its own thread — it needs a message pump.
    *TRAY_TX.lock().unwrap() = Some(tx.clone());
    let (hwnd_tx, hwnd_rx) = mpsc::sync_channel::<Option<SendHandle<HWND>>>(1);

    thread::Builder::new()
        .name("tray-thread".into())
        .spawn(move || {
            let hwnd = unsafe { create_tray_window(&tx) };
            let _ = hwnd_tx.send(hwnd.map(SendHandle));
            if let Some(w) = hwnd {
                tray_message_loop(w, &tx);
            }
        })
        .ok();

    hwnd_rx.recv().ok().flatten().map(|h| h.0)
}

unsafe fn create_tray_window(tx: &Sender<PlatformEvent>) -> Option<HWND> {
    let hinstance = GetModuleHandleW(None).ok()?;

    let class_name: Vec<u16> = "notchWin_tray\0".encode_utf16().collect();
    let window_name: Vec<u16> = "notchX tray\0".encode_utf16().collect();
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(tray_wnd_proc),
        hInstance: HINSTANCE::from(hinstance),
        lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    RegisterClassExW(&wc);

    let hwnd = CreateWindowExW(
        Default::default(),
        windows::core::PCWSTR(class_name.as_ptr()),
        windows::core::PCWSTR(window_name.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        0, 0, 0, 0,
        None, None, Some(HINSTANCE::from(hinstance)), None,
    ).ok()?;

    // Add tray icon.
    let mut tip = [0u16; 128];
    for (i, c) in "notchX\0".encode_utf16().enumerate() {
        if i < 128 { tip[i] = c; }
    }
    let mut nid = NOTIFYICONDATAW {
        cbSize:           std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd:             hwnd,
        uID:              1,
        uFlags:           NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        szTip:            tip,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_ADD, &mut nid);

    Some(hwnd)
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_TRAY {
        // Low word of lparam is the shell notification message.
        let event = (lparam.0 & 0xFFFF) as u32;

        if event == 0x0203 {
            // Double-click → open.
            send_tray_event(PlatformEvent::TrayOpen);
        } else if event == 0x0205 || event == 0x007B {
            // Right-click / context menu key.
            show_tray_menu(hwnd);
        }
    }
    if msg == WM_COMMAND {
        match wparam.0 & 0xFFFF {
            ID_TRAY_OPEN => send_tray_event(PlatformEvent::TrayOpen),
            ID_TRAY_PAUSE => send_tray_event(PlatformEvent::TrayPause),
            ID_TRAY_SETTINGS => send_tray_event(PlatformEvent::TraySettings),
            ID_TRAY_QUIT => send_tray_event(PlatformEvent::TrayQuit),
            _ => {}
        }
    }
    // WM_POWERBROADCAST (0x0214) and WM_DISPLAYCHANGE (0x007E): the DWM can
    // drop TOPMOST/layered state across sleep, lock or monitor changes.
    if msg == 0x0214 || msg == 0x007E {
        reassert_stage();
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn send_tray_event(ev: PlatformEvent) {
    if let Ok(guard) = TRAY_TX.lock() {
        if let Some(ref tx) = *guard {
            let _ = tx.send(ev);
        }
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let Ok(menu) = CreatePopupMenu() else { return };
    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_OPEN, &HSTRING::from("Open"));
    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_SETTINGS, &HSTRING::from("Settings"));
    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_PAUSE, &HSTRING::from("Pause"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None::<&windows::core::PCWSTR>);
    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_QUIT, &HSTRING::from("Quit"));

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let selected = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN,
        pt.x,
        pt.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);

    match selected.0 as usize {
        ID_TRAY_OPEN => send_tray_event(PlatformEvent::TrayOpen),
        ID_TRAY_SETTINGS => send_tray_event(PlatformEvent::TraySettings),
        ID_TRAY_PAUSE => send_tray_event(PlatformEvent::TrayPause),
        ID_TRAY_QUIT => send_tray_event(PlatformEvent::TrayQuit),
        _ => {}
    }
    // Dismiss the popup's activation state.
    let _ = PostMessageW(Some(hwnd), 0, WPARAM(0), LPARAM(0));
}

fn tray_message_loop(hwnd: HWND, _tx: &Sender<PlatformEvent>) {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }
}

fn blank_nid(hwnd: HWND, id: u32) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: id,
        ..Default::default()
    }
}

// ── Hotkey window ─────────────────────────────────────────────────────────────
// Ctrl+Alt+Space = toggle (matches the Flutter HotkeyBridge default).

const HOTKEY_TOGGLE:  i32 = 1;
const HOTKEY_EXPAND:  i32 = 2;
const HOTKEY_HIDE:    i32 = 3;

fn start_hotkeys(tx: Sender<PlatformEvent>) -> Option<HWND> {
    let (hwnd_tx, hwnd_rx) = mpsc::sync_channel::<Option<SendHandle<HWND>>>(1);

    thread::Builder::new()
        .name("hotkey-thread".into())
        .spawn(move || {
            let hwnd = unsafe { create_hotkey_window() };
            let _ = hwnd_tx.send(hwnd.map(SendHandle));
            if let Some(w) = hwnd {
                unsafe { register_hotkeys(w) };
                hotkey_message_loop(w, &tx);
            }
        })
        .ok();

    hwnd_rx.recv().ok().flatten().map(|h| h.0)
}

/// `WNDCLASSEXW.lpfnWndProc` requires an `extern "system"` function pointer,
/// while the generated `DefWindowProcW` binding is a plain Rust `unsafe fn`.
unsafe extern "system" fn default_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe fn create_hotkey_window() -> Option<HWND> {
    let hinstance = GetModuleHandleW(None).ok()?;
    let class_name: Vec<u16> = "notchWin_hotkey\0".encode_utf16().collect();
    let window_name: Vec<u16> = "notchX hotkey\0".encode_utf16().collect();
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(default_wnd_proc),
        hInstance: HINSTANCE::from(hinstance),
        lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    RegisterClassExW(&wc);
    CreateWindowExW(
        Default::default(),
        windows::core::PCWSTR(class_name.as_ptr()),
        windows::core::PCWSTR(window_name.as_ptr()),
        Default::default(),
        0, 0, 0, 0,
        None, None, Some(HINSTANCE::from(hinstance)), None,
    ).ok()
}

unsafe fn register_hotkeys(hwnd: HWND) {
    let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
    // Ctrl+Alt+Space — toggle. Ctrl+Alt+→ — expand. Ctrl+Alt+↓ — hide.
    let _ = RegisterHotKey(Some(hwnd), HOTKEY_TOGGLE, mods, VK_SPACE.0 as u32);
    let _ = RegisterHotKey(Some(hwnd), HOTKEY_EXPAND, mods, VK_RIGHT.0 as u32);
    let _ = RegisterHotKey(Some(hwnd), HOTKEY_HIDE, mods, VK_DOWN.0 as u32);
}

fn hotkey_message_loop(hwnd: HWND, tx: &Sender<PlatformEvent>) {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, Some(hwnd), 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                let ev = match msg.wParam.0 as i32 {
                    HOTKEY_TOGGLE => PlatformEvent::HotkeyToggle,
                    HOTKEY_EXPAND => PlatformEvent::HotkeyExpand,
                    HOTKEY_HIDE   => PlatformEvent::HotkeyHide,
                    _ => continue,
                };
                if tx.send(ev).is_err() {
                    break;
                }
            }
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }
}

// ── Low-level mouse hook ──────────────────────────────────────────────────────
// Runs on a dedicated thread.  The callback compares the cursor position
// against two pre-computed rectangles (island hit-rect, reveal zone) and
// posts messages to the hook thread's own queue — it never touches the UI
// thread while inside the callback.

struct HookState {
    tx:     Sender<PlatformEvent>,
    shared: Arc<SharedRects>,
    // Cached from previous callback so we only send on change.
    was_near:    bool,
    was_in_hit:  bool,
}

// SAFETY: the hook callback is only ever called from the single hook thread.
static HOOK_STATE: Mutex<Option<HookState>> = Mutex::new(None);

/// Stage HWND, mirrored here so the hidden message window can re-assert the
/// overlay after a display change or power resume.
static STAGE_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn reassert_stage() {
    let raw = STAGE_HWND.load(Ordering::Relaxed);
    if raw == 0 { return; }
    let hwnd = HWND(raw as *mut core::ffi::c_void);
    crate::win32::reassert_topmost(hwnd);
    place_stage(hwnd);
}

fn start_mouse_hook(
    tx: Sender<PlatformEvent>,
    shared: Arc<SharedRects>,
) -> (Sender<()>, Option<HHOOK>) {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (hook_ready_tx, hook_ready_rx) = mpsc::sync_channel::<Option<SendHandle<HHOOK>>>(1);

    {
        let mut guard = HOOK_STATE.lock().unwrap();
        *guard = Some(HookState { tx, shared, was_near: false, was_in_hit: false });
    }

    thread::Builder::new()
        .name("mouse-hook".into())
        .spawn(move || {
            let hook = unsafe {
                SetWindowsHookExW(WH_MOUSE_LL, Some(ll_mouse_proc), None, 0).ok()
            };
            let _ = hook_ready_tx.send(hook.map(SendHandle));

            // Pump messages until stop signal.
            unsafe {
                let mut msg = MSG::default();
                loop {
                    // Non-blocking check for stop signal.
                    if stop_rx.try_recv().is_ok() {
                        PostQuitMessage(0);
                    }
                    // GetMessage blocks until a hook message arrives.
                    if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    let _ = DispatchMessageW(&msg);
                }
            }
        })
        .ok();

    let hook = hook_ready_rx.recv().ok().flatten().map(|h| h.0);
    (stop_tx, hook)
}

unsafe extern "system" fn ll_mouse_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 {
        // Read the struct without taking the lock — just get the cursor pos
        // from the hook info which is already on this stack.
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);

        if let Ok(mut guard) = HOOK_STATE.try_lock() {
            if let Some(ref mut state) = *guard {
                let shared = &state.shared;

                // Remember button state so a style flip never happens in the
                // middle of a drag (which would cancel the drag).
                if wparam.0 == 0x0201 {
                    shared.button_down.store(true, Ordering::Relaxed);
                } else if wparam.0 == 0x0202 {
                    shared.button_down.store(false, Ordering::Relaxed);
                }

                // Reveal zone check (top of screen, wider than the island).
                let near = pt.x >= shared.rev_left.load(Ordering::Relaxed)
                    && pt.x <= shared.rev_right.load(Ordering::Relaxed)
                    && pt.y <= shared.rev_bottom.load(Ordering::Relaxed);

                if near != state.was_near {
                    state.was_near = near;
                    let _ = state.tx.send(PlatformEvent::CursorNearTop(near));
                }

                let in_hit = pt.x >= shared.hit_left.load(Ordering::Relaxed)
                    && pt.x <= shared.hit_right.load(Ordering::Relaxed)
                    && pt.y >= shared.hit_top.load(Ordering::Relaxed)
                    && pt.y <= shared.hit_bottom.load(Ordering::Relaxed);

                // Click-through is handled by the WM_NCHITTEST subclass in
                // win32.rs. Do NOT toggle WS_EX_TRANSPARENT here: that flag
                // disables hit testing for the whole 1284x900 stage and makes
                // every other app unclickable once the cursor enters the pill.

                // Outside-click: left button down outside the hit-rect.
                // wparam 0x0201 = WM_LBUTTONDOWN
                if wparam.0 == 0x0201 && !in_hit {
                    let _ = state.tx.send(PlatformEvent::OutsideClick);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

// ── Helper: update shared rects from the iced thread ─────────────────────────

/// Update the hit-rect (island interactive region) in screen pixels.
/// Called from `main.rs` whenever the island geometry changes.
pub fn update_hit_rect(shared: &SharedRects, x: i32, y: i32, w: i32, h: i32) {
    shared.hit_left.store(x, Ordering::Relaxed);
    shared.hit_top.store(y, Ordering::Relaxed);
    shared.hit_right.store(x + w, Ordering::Relaxed);
    shared.hit_bottom.store(y + h, Ordering::Relaxed);
}

/// Update the reveal zone (wider band at top of screen for auto-hide reveal).
pub fn update_reveal_zone(shared: &SharedRects, x: i32, right: i32, bottom: i32) {
    shared.rev_left.store(x, Ordering::Relaxed);
    shared.rev_right.store(right, Ordering::Relaxed);
    shared.rev_bottom.store(bottom, Ordering::Relaxed);
}

/// Recompute both rectangles from the live stage window position, the current
/// island geometry and the monitor DPI. Called on the iced thread whenever the
/// island geometry changes; the hook only ever *reads* the atomics.
pub fn sync_rects(
    shared: &SharedRects,
    island_x: f32,
    island_y: f32,
    island_w: f32,
    island_h: f32,
) {
    use windows::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
        },
        UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::GetWindowRect},
    };

    let hwnd = HWND(shared.stage_hwnd.load(Ordering::Relaxed) as *mut core::ffi::c_void);
    if hwnd.0.is_null() {
        return;
    }

    unsafe {
        let mut wr = RECT::default();
        if GetWindowRect(hwnd, &mut wr).is_err() {
            return;
        }
        let scale = (GetDpiForWindow(hwnd) as f32 / 96.0).max(0.5);

        let (hx, hy, hw, hh) = crate::metrics::hit_rect(island_x, island_y, island_w, island_h);
        update_hit_rect(
            shared,
            wr.left + (hx * scale) as i32,
            wr.top + (hy.max(0.0) * scale) as i32,
            (hw * scale) as i32,
            (hh * scale) as i32,
        );

        // Clip the stage to the island itself. This is the hard guarantee that
        // no grey client area is visible outside the pill, even if Windows
        // fails to honour iced's per-pixel transparency on this machine.
        // No SetWindowRgn here. Dynamic region changes were the source of the
        // flicker and desktop-wide repaint lag. iced's transparent stage is
        // enough now that the stage is correctly sized and DPI-aware.

        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut info).as_bool() {
            let cx = wr.left + (wr.right - wr.left) / 2;
            let half = ((crate::metrics::COLLAPSED_W / 2.0
                + crate::metrics::REVEAL_MARGIN_X)
                * scale) as i32;
            let bottom = info.rcMonitor.top
                + (crate::metrics::REVEAL_MARGIN_Y * scale) as i32;
            update_reveal_zone(shared, cx - half, cx + half, bottom);
        }
    }
}

/// Open a shortcut target with the Windows shell. The target may be a URL, an
/// executable, a `shell:` folder or an `ms-settings:` URI — ShellExecuteW
/// resolves all four, so no per-kind logic is needed here.
pub fn shell_open(target: &str) {
    use windows::{
        core::HSTRING,
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };

    let target = HSTRING::from(target);
    let open = HSTRING::from("open");
    let hwnd = HWND(0isize as *mut core::ffi::c_void);
    let _ = unsafe {
        ShellExecuteW(
            None,
            &open,
            &target,
            None::<&windows::core::PCWSTR>,
            None::<&windows::core::PCWSTR>,
            SW_SHOWNORMAL,
        )
    };
    let _ = hwnd;
}

/// Send a media key (play/pause, next, previous) to the system. This works
/// without package identity, unlike WinRT notification mirroring.
pub fn media_key(action: MediaKey) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYEVENTF_KEYUP, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE,
        VK_MEDIA_PREV_TRACK,
    };

    let vk = match action {
        MediaKey::PlayPause => VK_MEDIA_PLAY_PAUSE,
        MediaKey::Next => VK_MEDIA_NEXT_TRACK,
        MediaKey::Previous => VK_MEDIA_PREV_TRACK,
    };
    unsafe {
        keybd_event(vk.0 as u8, 0, Default::default(), 0);
        keybd_event(vk.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MediaKey {
    PlayPause,
    Next,
    Previous,
}

/// Seek the current SMTC session to an absolute position in seconds.
/// Runs on its own COM-initialised thread so the call never blocks iced.
pub fn media_seek(seconds: f32) {
    #[cfg(target_os = "windows")]
    {
        thread::Builder::new()
            .name("smtc-seek".into())
            .spawn(move || {
                use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                {
                    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Manager;
                    if let Some(manager) = Manager::RequestAsync().ok().and_then(await_async) {
                        if let Ok(session) = manager.GetCurrentSession() {
                            // TimeSpan ticks: 100 ns each.
                            let ticks = (seconds.max(0.0) * 10_000_000.0) as i64;
                            if let Ok(op) = session.TryChangePlaybackPositionAsync(ticks) {
                                let _ = await_async(op);
                            }
                        }
                    }
                }
                unsafe { CoUninitialize() };
            })
            .expect("spawn smtc-seek thread");
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = seconds;
    }
}

/// Adjust the system volume with the standard volume keys. One step is 2%.
pub fn volume_step(direction: f32) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYEVENTF_KEYUP, VK_VOLUME_DOWN, VK_VOLUME_UP,
    };

    if direction == 0.0 {
        return;
    }
    let vk = if direction > 0.0 { VK_VOLUME_UP } else { VK_VOLUME_DOWN };
    for _ in 0..direction.abs().clamp(1.0, 5.0) as i32 {
        unsafe {
            keybd_event(vk.0 as u8, 0, Default::default(), 0);
            keybd_event(vk.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

/// Put UTF-16 text on the clipboard (used by the shelf's Copy action).
pub fn copy_text_to_clipboard(text: &str) {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::{
            Foundation::HANDLE,
            System::{
                DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
                Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
                Ole::CF_UNICODETEXT,
            },
        };

        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = wide.len() * 2;
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, bytes) else { return };
        let ptr = GlobalLock(mem);
        if ptr.is_null() {
            return;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
        let _ = GlobalUnlock(mem);

        if OpenClipboard(None).is_ok() {
            let _ = EmptyClipboard();
            let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0)));
            let _ = CloseClipboard();
        }
    }
}

/// Battery charge percentage from `GetSystemPowerStatus`, if a battery exists.
pub fn battery_percent() -> Option<u8> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut status = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut status) }.is_ok()
            && status.BatteryLifePercent != 255
        {
            return Some(status.BatteryLifePercent.min(100));
        }
    }
    None
}

pub fn battery_charging() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut status = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut status) }.is_ok() {
            return status.ACLineStatus == 1;
        }
    }
    false
}

/// True when a maximised or full-screen window owns the foreground. The
/// overlay must withdraw so it never sits on top of a game or presentation.
pub fn foreground_blocks_overlay() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::{
            Foundation::RECT,
            Graphics::Gdi::{
                GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
            },
            UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect,
                SW_MAXIMIZE, WINDOWPLACEMENT, GWL_STYLE, WS_POPUP,
            },
        };

        unsafe {
            let fg = GetForegroundWindow();
            if fg.0.is_null() || fg.0 as isize == STAGE_HWND.load(Ordering::Relaxed) {
                return false;
            }

            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if GetWindowPlacement(fg, &mut placement).is_ok()
                && placement.showCmd == SW_MAXIMIZE.0 as u32
            {
                return true;
            }

            // Borderless full-screen windows are not reported as maximised.
            let style = GetWindowLongPtrW(fg, GWL_STYLE);
            if (style & WS_POPUP.0 as isize) == 0 {
                return false;
            }
            let mut window_rect = RECT::default();
            if GetWindowRect(fg, &mut window_rect).is_err() {
                return false;
            }
            let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return false;
            }
            let monitor_rect = info.rcMonitor;
            window_rect.left <= monitor_rect.left + 2
                && window_rect.top <= monitor_rect.top + 2
                && window_rect.right >= monitor_rect.right - 2
                && window_rect.bottom >= monitor_rect.bottom - 2
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Poll the Win32 session state for the lock screen. Used as a counterpart to
/// `WM_POWERBROADCAST`, which iced's event loop does not expose directly.
pub fn session_locked() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::{
            core::PWSTR,
            Win32::System::RemoteDesktop::{
                WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx,
                WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTSINFOEXW,
            },
        };

        unsafe {
            let mut buffer = PWSTR::null();
            let mut bytes = 0_u32;
            if WTSQuerySessionInformationW(
                None,
                WTS_CURRENT_SESSION,
                WTSSessionInfoEx,
                &mut buffer,
                &mut bytes,
            )
            .is_err()
                || buffer.is_null()
            {
                return false;
            }
            let info = &*(buffer.0 as *const WTSINFOEXW);
            let flags = info.Data.WTSInfoExLevel1.SessionFlags as u32;
            WTSFreeMemory(buffer.0 as *mut core::ffi::c_void);
            flags == WTS_SESSIONSTATE_LOCK
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Add or remove `notchX` in the per-user Run key. Returns true on success.
pub fn set_startup(enabled: bool) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::{
            core::HSTRING,
            Win32::System::Registry::{
                RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
                HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
            },
        };

        let subkey = HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Run");
        let value_name = HSTRING::from("notchX");

        unsafe {
            let mut hkey = Default::default();
            if !RegOpenKeyExW(
                HKEY_CURRENT_USER,
                &subkey,
                None,
                KEY_SET_VALUE,
                &mut hkey,
            )
            .is_ok()
            {
                return false;
            }

            let status = if enabled {
                let Some(exe) = std::env::current_exe().ok() else {
                    let _ = RegCloseKey(hkey);
                    return false;
                };
                let command = format!("\"{}\"", exe.to_string_lossy());
                let mut wide: Vec<u16> = command.encode_utf16().collect();
                wide.push(0);
                let bytes =
                    std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                RegSetValueExW(hkey, &value_name, None, REG_SZ, Some(bytes))
            } else {
                RegDeleteValueW(hkey, &value_name)
            };
            let _ = RegCloseKey(hkey);
            return status.is_ok();
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
        false
    }
}
