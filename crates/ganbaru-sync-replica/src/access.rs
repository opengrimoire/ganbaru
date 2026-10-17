//! Per-connection setup of writable vault pools: replica guards and the commit signal that
//! wakes the sync service.

use ganbaru_db::{ConnectionHook, DatabaseAccessMode, connection_hook};
use ganbaru_sync::manifest::vault::VAULT_MANIFEST;
use std::sync::Arc;
use tokio::sync::Notify;

/// The hook every writable vault connection runs. Guarded connections get the replica guards
/// before any statement runs; every writable connection signals `commits` after each commit.
pub fn vault_connection_hook(commits: Arc<Notify>) -> ConnectionHook {
    connection_hook(move |conn, access| {
        let commits = commits.clone();
        Box::pin(async move {
            if access == DatabaseAccessMode::Guarded {
                ganbaru_sync::guards::install_guards(conn, &VAULT_MANIFEST)
                    .await
                    .map_err(|error| format!("install replica guards: {error}"))?;
            }
            let mut handle = conn
                .lock_handle()
                .await
                .map_err(|error| format!("lock connection handle: {error}"))?;
            handle.set_commit_hook(move || {
                commits.notify_one();
                true
            });
            Ok(())
        })
    })
}
