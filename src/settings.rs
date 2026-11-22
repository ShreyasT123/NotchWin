use std::path::PathBuf;

// ── settings struct ───────────────────────────────────────────────────────────

/// Stable page identifiers, in pager order. Kept here so settings files do
/// not depend on UI indices.
pub const PAGE_IDS: [&str; 8] = [
    "now", "media", "timers", "system", "calendar", "shortcuts", "shelf", "settings",
];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AppSettings {
    /// Display time in 24-hour format rather than 12-hour.
    pub use_24_hour_clock: bool,
    /// Expand the island on hover without clicking.
    pub hover_to_peek: bool,
    /// Scroll wheel on the island adjusts system volume.
    pub wheel_adjusts_volume: bool,
    /// Show a short transient pill when the system volume changes.
    #[serde(default = "default_true")]
    pub volume_hud_enabled: bool,
    /// Whether the user has seen the first-run introduction.
    pub intro_seen: bool,
    /// Seconds of inactivity before the island collapses; 0 means never.
    pub auto_collapse_seconds: u32,
    /// Hide the island completely when not in use.
    pub auto_hide: bool,
    /// Scale factor for the island widget; valid range 0.75–1.35.
    pub island_scale: f32,
    /// Accent preset index; see `crate::ACCENT_PRESETS`. The value equal to
    /// the preset count selects `custom_accent` instead.
    #[serde(default)]
    pub accent: usize,
    /// Custom RGB accent used when `accent == CUSTOM_ACCENT_INDEX`.
    #[serde(default)]
    pub custom_accent: Option<(u8, u8, u8)>,
    /// Opacity multiplier for the glass surface (0.55–1.0).
    #[serde(default = "default_surface_opacity")]
    pub surface_opacity: f32,
    /// Multiplier for top and bottom corner radii (0.6–1.4).
    #[serde(default = "default_corner_scale")]
    pub corner_scale: f32,
    /// Spring speed multiplier. 0 snaps, 1 matches Flutter, 1.4 is liveliest.
    #[serde(default = "default_motion_intensity")]
    pub motion_intensity: f32,
    /// Withdraw while a maximised or full-screen app owns the foreground.
    #[serde(default = "default_true")]
    pub hide_when_covered: bool,
    /// Register the executable in HKCU Run so it starts with Windows.
    #[serde(default)]
    pub start_with_windows: bool,
    /// Page identifiers switched off by the user. Settings is always available.
    #[serde(default)]
    pub disabled_pages: Vec<String>,
    /// App or action identifiers pinned to the shortcuts bar.
    #[serde(default)]
    pub pinned_shortcuts: Vec<String>,
}

fn default_motion_intensity() -> f32 { 1.0 }
fn default_surface_opacity() -> f32 { 1.0 }
fn default_corner_scale() -> f32 { 1.0 }
fn default_true() -> bool { true }

impl AppSettings {
    /// Pages that should be built and shown in the pager.
    pub fn page_enabled(&self, index: usize) -> bool {
        if index >= PAGE_IDS.len() {
            return false;
        }
        // Settings is the escape hatch back to these switches; never hide it.
        if index == PAGE_IDS.len() - 1 {
            return true;
        }
        !self.disabled_pages.iter().any(|id| id == PAGE_IDS[index])
    }

    /// Clamp loaded/imported values into the ranges the UI can cycle through.
    pub fn sanitize(&mut self) {
        self.island_scale = self.island_scale.clamp(0.75, 1.35);
        self.motion_intensity = self.motion_intensity.clamp(0.0, 1.4);
        self.surface_opacity = self.surface_opacity.clamp(0.55, 1.0);
        self.corner_scale = self.corner_scale.clamp(0.6, 1.4);
        if self.accent > crate::CUSTOM_ACCENT_INDEX {
            self.accent = 0;
        }
    }

    /// Toggle a page's visibility. Returns the new state.
    pub fn toggle_page(&mut self, index: usize) -> bool {
        if index >= PAGE_IDS.len() || self.page_enabled(index) && index == PAGE_IDS.len() - 1 {
            return self.page_enabled(index);
        }
        let id = PAGE_IDS[index];
        if let Some(pos) = self.disabled_pages.iter().position(|v| v == id) {
            self.disabled_pages.remove(pos);
            true
        } else {
            self.disabled_pages.push(id.to_string());
            self.page_enabled(index)
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            use_24_hour_clock: false,
            hover_to_peek: true,
            wheel_adjusts_volume: true,
            volume_hud_enabled: true,
            intro_seen: false,
            auto_collapse_seconds: 5,
            auto_hide: false,
            island_scale: 1.0,
            accent: 0,
            custom_accent: None,
            surface_opacity: 1.0,
            corner_scale: 1.0,
            motion_intensity: 1.0,
            hide_when_covered: true,
            start_with_windows: false,
            disabled_pages: Vec::new(),
            pinned_shortcuts: Vec::new(),
        }
    }
}

// ── controller ────────────────────────────────────────────────────────────────

pub struct SettingsController {
    pub settings: AppSettings,
    path: PathBuf,
}

impl SettingsController {
    /// Determine the path to `settings.json` inside `%APPDATA%\notchWin\`.
    fn settings_path() -> PathBuf {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                // Fallback: place next to the executable.
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| PathBuf::from("."))
            });
        base.join("notchWin").join("settings.json")
    }

    /// Load settings from disk. Falls back to `AppSettings::default()` if the
    /// file does not exist or cannot be parsed.
    pub fn load() -> Self {
        let path = Self::settings_path();
        let mut settings = Self::read_from(&path).unwrap_or_default();
        settings.sanitize();
        Self { settings, path }
    }

    /// Persist the current settings to disk. Errors are silently swallowed so a
    /// missing or read-only filesystem never crashes the application.
    pub fn save(&self) {
        let _ = self.write_to(&self.path);
    }

    /// Path of the portable settings snapshot next to `settings.json`.
    pub fn export_path(&self) -> PathBuf {
        self.path.with_file_name("settings-export.json")
    }

    /// True when a previously exported snapshot exists.
    pub fn has_export(&self) -> bool {
        self.export_path().exists()
    }

    /// Write the current settings to the portable snapshot.
    pub fn export(&self) -> bool {
        let json = match serde_json::to_string_pretty(&self.settings) {
            Ok(j) => j,
            Err(_) => return false,
        };
        let path = self.export_path();
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return false;
            }
        }
        std::fs::write(path, json).is_ok()
    }

    /// Replace current settings with the portable snapshot and persist it.
    /// A missing or malformed snapshot leaves the current settings untouched.
    pub fn import(&mut self) -> bool {
        let text = match std::fs::read_to_string(self.export_path()) {
            Ok(t) => t,
            Err(_) => return false,
        };
        let restored: AppSettings = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(_) => return false,
        };
        self.settings = restored;
        self.settings.sanitize();
        self.save();
        true
    }

    /// Apply a mutation function to the settings and immediately persist.
    pub fn mutate(&mut self, f: impl FnOnce(&mut AppSettings)) {
        f(&mut self.settings);
        self.save();
    }

    // ── private helpers ───────────────────────────────────────────────────────

    fn read_from(path: &PathBuf) -> Option<AppSettings> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_to(&self, path: &PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.settings)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_are_sane() {
        let s = AppSettings::default();
        assert!(!s.use_24_hour_clock);
        assert!(s.hover_to_peek);
        assert_eq!(s.island_scale, 1.0);
        assert_eq!(s.auto_collapse_seconds, 5);
        assert_eq!(s.accent, 0);
        assert_eq!(s.motion_intensity, 1.0);
        assert_eq!(s.surface_opacity, 1.0);
        assert_eq!(s.corner_scale, 1.0);
        assert!(s.custom_accent.is_none());
        assert!(s.hide_when_covered);
        assert!(s.disabled_pages.is_empty());
    }

    #[test]
    fn round_trip_serialization() {
        let original = AppSettings::default();
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: AppSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.use_24_hour_clock, original.use_24_hour_clock);
        assert_eq!(restored.island_scale, original.island_scale);
        assert_eq!(restored.accent, original.accent);
        assert_eq!(restored.motion_intensity, original.motion_intensity);
        assert_eq!(restored.disabled_pages, original.disabled_pages);
    }

    #[test]
    fn page_visibility_toggles_and_settings_stays_on() {
        let mut s = AppSettings::default();
        assert!(s.page_enabled(0));
        assert!(!s.toggle_page(0));
        assert!(!s.page_enabled(0));
        assert!(s.toggle_page(0));
        assert!(s.page_enabled(0));

        // The settings page cannot be removed: it is the way back.
        assert!(s.page_enabled(7));
        assert!(s.toggle_page(7));
        assert!(s.page_enabled(7));
    }

    #[test]
    fn older_settings_files_get_new_defaults() {
        // A file saved before the appearance settings existed must still load.
        let old = r#"{
            "use_24_hour_clock": true,
            "hover_to_peek": false,
            "wheel_adjusts_volume": true,
            "intro_seen": true,
            "auto_collapse_seconds": 8,
            "auto_hide": false,
            "island_scale": 1.1,
            "accent": 2,
            "motion_intensity": 0.5
        }"#;
        let restored: AppSettings = serde_json::from_str(old).expect("parse legacy settings");
        assert_eq!(restored.surface_opacity, 1.0);
        assert_eq!(restored.corner_scale, 1.0);
        assert!(restored.custom_accent.is_none());
        assert!(restored.volume_hud_enabled);
    }

    #[test]
    fn sanitize_clamps_out_of_range_values() {
        let mut s = AppSettings {
            island_scale: 9.0,
            motion_intensity: -1.0,
            surface_opacity: 3.0,
            corner_scale: 0.1,
            accent: 99,
            ..AppSettings::default()
        };
        s.sanitize();
        assert_eq!(s.island_scale, 1.35);
        assert_eq!(s.motion_intensity, 0.0);
        assert_eq!(s.surface_opacity, 1.0);
        assert_eq!(s.corner_scale, 0.6);
        assert_eq!(s.accent, 0);
    }

    #[test]
    fn export_and_import_round_trip() {
        let dir = std::env::temp_dir().join("notchWin_export_test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        let mut ctrl = SettingsController {
            settings: AppSettings::default(),
            path,
        };
        assert!(!ctrl.has_export());
        ctrl.settings.surface_opacity = 0.78;
        ctrl.settings.custom_accent = Some((1, 2, 3));
        assert!(ctrl.export());
        assert!(ctrl.has_export());

        ctrl.settings.surface_opacity = 1.0;
        ctrl.settings.custom_accent = None;
        assert!(ctrl.import());
        assert_eq!(ctrl.settings.surface_opacity, 0.78);
        assert_eq!(ctrl.settings.custom_accent, Some((1, 2, 3)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mutate_updates_field() {
        // Use a temp path so the test doesn't touch %APPDATA%.
        let dir = std::env::temp_dir().join("notchWin_test");
        let path = dir.join("settings.json");
        let _ = std::fs::remove_dir_all(&dir);

        let mut ctrl = SettingsController {
            settings: AppSettings::default(),
            path: path.clone(),
        };

        ctrl.mutate(|s| s.use_24_hour_clock = true);
        assert!(ctrl.settings.use_24_hour_clock);

        // The file should now exist with the updated value.
        let loaded: AppSettings =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(loaded.use_24_hour_clock);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
