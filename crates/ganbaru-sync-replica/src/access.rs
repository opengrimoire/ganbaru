//! Per-connection setup of writable vault pools: the commit signal that wakes the sync service.

use ganbaru_db::{ConnectionHook, connection_hook};
use std::sync::Arc;
use tokio::sync::Notify;

/// The hook every writable vault connection runs: it signals `commits` after each commit.
pub fn vault_connection_hook(commits: Arc<Notify>) -> ConnectionHook {
    connection_hook(move |conn| {
        let commits = commits.clone();
        Box::pin(async move {
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
