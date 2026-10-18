//! Files the operating system dropped on a window. Tauri's native drag-and-drop handler takes
//! file drops before the webview sees them, so features import dropped files through commands
//! that read the paths recorded here, never paths sent from the webview.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Longest time a drop waits for the webview to claim it. Imports already running delay the claim, so
/// this allows for slow imports while still refusing stale drops.
const DROP_CLAIM_WINDOW: Duration = Duration::from_secs(60);

struct PendingDrop {
    paths: Vec<PathBuf>,
    dropped_at: Instant,
}

/// The latest file drop on each window, until a feature claims it.
#[derive(Default)]
pub struct FileDrops {
    pending: Mutex<HashMap<String, PendingDrop>>,
}

impl FileDrops {
    /// Records files dropped on a window, replacing any unclaimed earlier drop.
    pub fn record(&self, window: &str, paths: Vec<PathBuf>, now: Instant) {
        let mut pending = self.lock();
        pending.retain(|_, drop| !expired(drop, now));
        pending.insert(
            window.to_string(),
            PendingDrop {
                paths,
                dropped_at: now,
            },
        );
    }

    /// Takes the files last dropped on a window. A drop is claimed once, and only shortly after it
    /// happened.
    pub fn take(&self, window: &str, now: Instant) -> Vec<PathBuf> {
        self.lock()
            .remove(window)
            .filter(|drop| !expired(drop, now))
            .map(|drop| drop.paths)
            .unwrap_or_default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingDrop>> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn expired(drop: &PendingDrop, now: Instant) -> bool {
    now.saturating_duration_since(drop.dropped_at) > DROP_CLAIM_WINDOW
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn hands_each_drop_to_its_own_window_once() {
        let drops = FileDrops::default();
        let now = Instant::now();
        drops.record("main", paths(&["/a.png"]), now);
        drops.record("tab-1", paths(&["/b.png"]), now);

        assert_eq!(drops.take("tab-1", now), paths(&["/b.png"]));
        assert_eq!(drops.take("main", now), paths(&["/a.png"]));
        assert!(drops.take("main", now).is_empty());
        assert!(drops.take("other", now).is_empty());
    }

    #[test]
    fn keeps_only_the_latest_drop_of_a_window() {
        let drops = FileDrops::default();
        let now = Instant::now();
        drops.record("main", paths(&["/old.png"]), now);
        drops.record("main", paths(&["/new.png", "/second.png"]), now);

        assert_eq!(drops.take("main", now), paths(&["/new.png", "/second.png"]));
    }

    #[test]
    fn refuses_drops_left_unclaimed_too_long() {
        let drops = FileDrops::default();
        let dropped_at = Instant::now();
        drops.record("main", paths(&["/a.png"]), dropped_at);

        let late = dropped_at + DROP_CLAIM_WINDOW + Duration::from_millis(1);
        assert!(drops.take("main", late).is_empty());
        assert!(drops.take("main", dropped_at).is_empty());
    }
}
