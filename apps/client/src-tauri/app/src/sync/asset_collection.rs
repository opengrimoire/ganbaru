//! The daily collection of managed asset files nothing references any more. It runs after a
//! sync pass on every device with a writable vault, linked or not, and holds the replicated
//! asset permit of each class's domain while it removes that class's files, so a vault handoff
//! fences it.

use super::host::SyncHost;
use ganbaru_sync_replica::assets::{
    AssetSweep, InFlightAssets, UNREFERENCED_ASSET_GRACE, collect_unreferenced_assets,
};
use ganbaru_sync_replica::backup::ArchiveGate;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// How often one vault's asset files are collected.
const COLLECTION_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// The vault and time of a last collection attempt.
type LastCollection = Mutex<Option<(String, Instant)>>;

/// This process's last collection attempt. A process starts with none, so the first pass after
/// launch collects.
static LAST_COLLECTION: LastCollection = Mutex::new(None);

/// Collects the unreferenced asset files of the active vault when its last collection in this
/// process is a day old. A vault that cannot be written yet leaves the interval unclaimed, so
/// the next pass tries again; a failure is logged and waits for the next interval.
pub(super) async fn collect_if_due<H: SyncHost>(host: &H) {
    let Ok(vault_id) = host.active_vault_id() else {
        return;
    };
    let pool = match host.connect_for_sync().await {
        Ok(Some(pool)) => pool,
        Ok(None) => return,
        Err(error) => {
            eprintln!("sync asset collection: {error}");
            return;
        }
    };
    if !claim_due(&LAST_COLLECTION, &vault_id, Instant::now()) {
        return;
    }
    match collect(host, &pool, &vault_id).await {
        Ok(0) => {}
        Ok(removed) => eprintln!("sync: removed {removed} unreferenced asset files"),
        Err(error) => eprintln!("sync asset collection: {error}"),
    }
}

/// Whether a collection of `vault_id` is due at `now`, recording the attempt in `last` when it
/// is.
fn claim_due(last: &LastCollection, vault_id: &str, now: Instant) -> bool {
    let Ok(mut last) = last.lock() else {
        return false;
    };
    let due = match last.as_ref() {
        Some((last_vault, at)) if last_vault == vault_id => {
            now.saturating_duration_since(*at) >= COLLECTION_INTERVAL
        }
        _ => true,
    };
    if due {
        *last = Some((vault_id.to_string(), now));
    }
    due
}

/// Sweeps the active vault through `pool` unless the active vault changed from `vault_id`.
async fn collect<H: SyncHost>(
    host: &H,
    pool: &sqlx::SqlitePool,
    vault_id: &str,
) -> Result<usize, String> {
    if host.active_vault_id()? != vault_id {
        return Ok(0);
    }
    let root = host.active_vault_path()?;
    let engine = host.engine()?;
    let in_flight = InFlightAssets::process();
    let sweep = AssetSweep {
        engine: &engine,
        pool,
        vault_root: &root,
        in_flight: &in_flight,
        grace: UNREFERENCED_ASSET_GRACE,
        now: SystemTime::now(),
    };
    collect_unreferenced_assets(&ArchiveGate::process(), sweep, |class| {
        let (domain, _) = engine.manifest().asset_class(class.name)?;
        // A refused permit means the domain is fenced or read-only here, so its files stay.
        let permit = host.replicated_asset_path(domain.name).ok()?;
        (*permit == *root).then_some(permit)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_is_due_once_a_day_per_vault() {
        let last = Mutex::new(None);
        let start = Instant::now();
        assert!(claim_due(&last, "vault", start));
        assert!(!claim_due(&last, "vault", start + Duration::from_secs(60)));
        assert!(claim_due(&last, "vault", start + COLLECTION_INTERVAL));
        assert!(claim_due(&last, "other", start + COLLECTION_INTERVAL));
        assert!(claim_due(&last, "vault", start + COLLECTION_INTERVAL));
    }
}
