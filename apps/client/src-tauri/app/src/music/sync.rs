//! Music work that follows rows sync applied.
//!
//! Post-apply hooks run inside the engine transaction with only the connection and a change
//! sink, so the headless Android pass runs them too: they rebuild the search rows the applied
//! rows feed and report the roots that received new or changed locations. On desktop the app
//! host hands those reports to [`on_sync_notices`], which schedules one debounced refresh of each
//! reported root that is bound on this device, so the availability it observes catches up.

use ganbaru_music_library::sync_apply;
#[cfg(desktop)]
use ganbaru_sync::ChangeNotice;
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::music;
use ganbaru_sync::{ChangeSink, HookError, HookFuture, Hooks, MaterializeHook, rebuild_every_key};
use sqlx::SqliteConnection;

/// Change notice topic of a root that received new or changed locations; the key is the root id.
pub(crate) const MUSIC_ROOT_TOPIC: &str = "music-root";

/// Tables whose applied rows feed the search index, each registered with [`MusicApplyHook`].
const HOOKED_TABLES: [&TableSpec; 8] = [
    &music::ITEMS,
    &music::SIGNALS,
    &music::LOCATIONS,
    &music::COLLECTIONS,
    &music::COLLECTION_ITEMS,
    &music::PLAYLISTS,
    &music::INTENDED_USES,
    &music::MEMBERSHIPS,
];

/// Rebuilds search rows after sync materializes Music rows and reports changed roots.
struct MusicApplyHook;

fn hook_error(error: ganbaru_music_library::MusicLibraryError) -> HookError {
    HookError::from(error.to_string())
}

impl MaterializeHook for MusicApplyHook {
    fn after_materialize<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            sync_apply::refresh_search_after_apply(conn, table.name, changed_keys)
                .await
                .map_err(hook_error)?;
            if table.id == music::LOCATIONS.id {
                for root in sync_apply::roots_of_locations(conn, changed_keys)
                    .await
                    .map_err(hook_error)?
                {
                    changes.report(MUSIC_ROOT_TOPIC, root);
                }
            }
            Ok(())
        })
    }

    fn rebuild_all<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        rebuild_every_key(self, conn, table, changes)
    }
}

/// `hooks` with the Music post-apply hooks added.
pub(crate) fn with_hooks(hooks: Hooks) -> Hooks {
    HOOKED_TABLES.into_iter().fold(hooks, |hooks, table| {
        hooks.with(table.id, Box::new(MusicApplyHook))
    })
}

/// The distinct root ids among sync change notices.
#[cfg(desktop)]
fn reported_roots<'a>(
    notices: impl Iterator<Item = &'a ChangeNotice>,
) -> std::collections::BTreeSet<String> {
    notices
        .filter(|notice| notice.topic == MUSIC_ROOT_TOPIC)
        .map(|notice| notice.key.clone())
        .collect()
}

#[cfg(desktop)]
pub(crate) use scheduler::{RootRefreshScheduler, on_sync_notices};

#[cfg(desktop)]
mod scheduler {
    use super::reported_roots;
    use ganbaru_music_library::{
        MusicAvailableRootPath, MusicLocalRefreshRequest, local_refresh, sync_apply,
    };
    use ganbaru_sync::ChangeNotice;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tauri::{AppHandle, Manager, Runtime};

    /// Quiet time after the last reported root before the refresh runs, so one sync pass that
    /// applies many batches refreshes each root once.
    const REFRESH_DEBOUNCE: Duration = Duration::from_secs(2);

    /// Roots waiting for their debounced refresh.
    #[derive(Default)]
    pub(crate) struct RootRefreshScheduler {
        state: Mutex<PendingRoots>,
    }

    #[derive(Default)]
    struct PendingRoots {
        roots: BTreeSet<String>,
        generation: u64,
    }

    impl RootRefreshScheduler {
        /// Adds `roots` and returns the generation a timer must still hold to run them.
        fn add(&self, roots: BTreeSet<String>) -> Option<u64> {
            let mut state = self.state.lock().ok()?;
            state.roots.extend(roots);
            state.generation = state.generation.wrapping_add(1);
            Some(state.generation)
        }

        /// Takes the pending roots when no later report restarted the timer.
        fn take_if_current(&self, generation: u64) -> BTreeSet<String> {
            match self.state.lock() {
                Ok(mut state) if state.generation == generation => std::mem::take(&mut state.roots),
                _ => BTreeSet::new(),
            }
        }
    }

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64
    }

    /// Schedules one debounced refresh of the roots that sync change `notices` report.
    pub(crate) async fn on_sync_notices<R: Runtime>(app: &AppHandle<R>, notices: &[ChangeNotice]) {
        let roots = reported_roots(notices.iter());
        if roots.is_empty() {
            return;
        }
        let Some(scheduler) = app.try_state::<RootRefreshScheduler>() else {
            return;
        };
        let Some(generation) = scheduler.add(roots) else {
            return;
        };
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(REFRESH_DEBOUNCE).await;
            let roots = app
                .state::<RootRefreshScheduler>()
                .take_if_current(generation);
            for root in roots {
                if let Err(error) = refresh_bound_root(&app, &root).await {
                    eprintln!("refresh music root {root} after sync: {error}");
                }
            }
        });
    }

    /// Device-local folder bindings of the active vault's roots.
    fn active_bindings<R: Runtime>(app: &AppHandle<R>) -> Result<BTreeMap<String, String>, String> {
        let vault_id = crate::vault::active_vault_id(app)?;
        let state = crate::vault::read_app_state(app)?;
        Ok(state
            .music_root_bindings
            .get(&vault_id)
            .cloned()
            .unwrap_or_default())
    }

    /// Refreshes every discoverable source of `root` when the root is bound on this device. An
    /// unbound root is skipped; its items stay listed but not playable here.
    async fn refresh_bound_root<R: Runtime>(app: &AppHandle<R>, root: &str) -> Result<(), String> {
        let bindings = active_bindings(app)?;
        let Some(folder_path) = bindings.get(root).cloned() else {
            return Ok(());
        };
        let available_roots: Vec<MusicAvailableRootPath> = bindings
            .iter()
            .map(|(root_id, folder_path)| MusicAvailableRootPath {
                root_id: root_id.clone(),
                folder_path: folder_path.clone(),
            })
            .collect();
        let pool = crate::db::connect_vault(app).await?;
        let collections = sync_apply::refreshable_collections_of_root(&pool, root)
            .await
            .map_err(|error| error.to_string())?;
        for collection_id in collections {
            let requested_at_ms = now_ms();
            let request = MusicLocalRefreshRequest {
                job_id: format!("sync-refresh:{collection_id}:{requested_at_ms}"),
                root_id: root.to_string(),
                collection_id,
                folder_path: folder_path.clone(),
                available_roots: available_roots.clone(),
                requested_at_ms,
            };
            local_refresh::prepare(&pool, &request)
                .await
                .map_err(|error| error.to_string())?;
            let refresh_pool = pool.clone();
            tauri::async_runtime::spawn_blocking(move || {
                tauri::async_runtime::block_on(local_refresh::run_prepared(&refresh_pool, request))
            })
            .await
            .map_err(|error| format!("run music refresh after sync: {error}"))?
            .map_err(|error| error.to_string())?;
        }
        crate::sync::emit_applied(app, vec![super::music::LOCATIONS.name.to_string()]);
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn roots(ids: &[&str]) -> BTreeSet<String> {
            ids.iter().map(|id| (*id).to_string()).collect()
        }

        #[test]
        fn only_the_latest_report_takes_the_pending_roots() {
            let scheduler = RootRefreshScheduler::default();
            let first = scheduler.add(roots(&["root-a"])).unwrap();
            let second = scheduler.add(roots(&["root-b", "root-a"])).unwrap();
            assert!(scheduler.take_if_current(first).is_empty());
            assert_eq!(
                scheduler.take_if_current(second),
                roots(&["root-a", "root-b"])
            );
            assert!(scheduler.take_if_current(second).is_empty());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(topic: &'static str, key: &str) -> ChangeNotice {
        ChangeNotice {
            topic,
            key: key.to_string(),
        }
    }

    #[test]
    fn reported_roots_keep_music_root_notices_once() {
        let notices = [
            notice(MUSIC_ROOT_TOPIC, "root-b"),
            notice("vault_preferences", "root-a"),
            notice(MUSIC_ROOT_TOPIC, "root-a"),
            notice(MUSIC_ROOT_TOPIC, "root-b"),
        ];
        let roots: Vec<String> = reported_roots(notices.iter()).into_iter().collect();
        assert_eq!(roots, vec!["root-a".to_string(), "root-b".to_string()]);
    }

    #[test]
    fn every_search_source_table_has_a_hook_and_names_its_table() {
        let mut hooked: Vec<&str> = HOOKED_TABLES.iter().map(|table| table.name).collect();
        let mut sources = sync_apply::SEARCH_SOURCE_TABLES.to_vec();
        hooked.sort_unstable();
        sources.sort_unstable();
        assert_eq!(hooked, sources);
    }
}
