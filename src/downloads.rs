// downloads.rs — app-side mirror of Flutter's `DownloadsController`.
//
// The Win32 folder watcher lives in `platform.rs`; this module keeps the
// active/recent lists and formatting used by the transient faces and the
// Now page. It is deliberately UI-independent so it can be unit tested.

const MAX_RECENT: usize = 5;

#[derive(Clone, Debug)]
pub struct DownloadDisplay {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    pub bytes_per_second: f64,
    pub completed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DownloadCenter {
    active: Vec<DownloadDisplay>,
    recent: Vec<DownloadDisplay>,
}

impl DownloadCenter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn started(&mut self, id: String, name: String) {
        if self.active.iter().any(|d| d.id == id) || self.recent.iter().any(|d| d.id == id) {
            return;
        }
        self.active.push(DownloadDisplay {
            id,
            name,
            bytes: 0,
            bytes_per_second: 0.0,
            completed: false,
        });
    }

    pub fn progress(&mut self, id: &str, name: String, bytes: u64, bytes_per_second: f64) {
        if let Some(item) = self.active.iter_mut().find(|d| d.id == id) {
            item.name = name;
            item.bytes = bytes;
            item.bytes_per_second = bytes_per_second;
        } else {
            self.started(id.to_string(), name);
            self.progress(id, String::new(), bytes, bytes_per_second);
        }
    }

    pub fn finished(&mut self, id: &str, name: String, completed: bool) {
        if let Some(pos) = self.active.iter().position(|d| d.id == id) {
            let mut item = self.active.remove(pos);
            item.name = if name.is_empty() { item.name } else { name };
            item.completed = completed;
            self.recent.insert(0, item);
            self.recent.truncate(MAX_RECENT);
        }
    }

    pub fn active(&self) -> &[DownloadDisplay] {
        &self.active
    }

    pub fn recent(&self) -> &[DownloadDisplay] {
        &self.recent
    }

    pub fn primary(&self) -> Option<&DownloadDisplay> {
        self.active.first()
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn total_rate(&self) -> f64 {
        self.active.iter().map(|d| d.bytes_per_second).sum()
    }
}

/// Human-readable transfer rate, matching the Flutter `download_face.dart`.
pub fn format_rate(bytes_per_second: f64) -> String {
    if bytes_per_second <= 0.0 {
        return "—".into();
    }
    if bytes_per_second >= 1_000_000.0 {
        format!("{:.1} MB/s", bytes_per_second / 1_000_000.0)
    } else if bytes_per_second >= 1_000.0 {
        format!("{:.0} KB/s", bytes_per_second / 1_000.0)
    } else {
        format!("{:.0} B/s", bytes_per_second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_lifecycle() {
        let mut center = DownloadCenter::new();
        center.started("a".into(), "report.pdf.part".into());
        assert_eq!(center.active_count(), 1);
        center.progress("a", "report.pdf.part".into(), 512, 128.0);
        assert_eq!(center.primary().unwrap().bytes, 512);
        center.finished("a", "report.pdf".into(), true);
        assert_eq!(center.active_count(), 0);
        assert_eq!(center.recent()[0].name, "report.pdf");
        assert!(center.recent()[0].completed);
    }

    #[test]
    fn rate_formatting() {
        assert_eq!(format_rate(0.0), "—");
        assert_eq!(format_rate(2_400_000.0), "2.4 MB/s");
        assert_eq!(format_rate(64_000.0), "64 KB/s");
    }
}
