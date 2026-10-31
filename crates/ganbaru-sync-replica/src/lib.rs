//! A vault replica's part in concurrent sync, independent of Tauri: database access that
//! signals commits, the anchor's founding keys, charter, and proposal sequencing, the admin
//! proposal outbox, the installation writer with its keys and device-local record, the installation's space keys with epoch keys held in memory only, carry-forward of local operations
//! across whole-vault replacement, recovery offers, Reset sync state, both sides of the LAN exchange of operations
//! and managed asset files over the handoff protocol, the relay pass that moves sealed records and asset chunks through a user-configured relay and keeps its epochs and admin set in step, the sweep of unreferenced asset files,
//! whole-vault archives for backups and bootstrap snapshots, the pinned TLS listeners behind a
//! routes trait, and the sync service loop over a host trait. The app owns the host on its
//! Tauri runtime, the listener routes, and the frontend projection.

pub mod access;
pub mod anchor_link;
#[cfg(desktop)]
pub mod anchor_setup;
pub mod assets;
pub mod authority;
pub mod backup;
pub mod carry_forward;
pub mod client;
pub mod hub;
pub mod keys;
pub mod outbox;
pub mod peers;
pub mod quarantine;
pub mod records;
pub mod recovery;
pub mod relay;
pub mod reset;
pub mod serve;
pub mod service;
mod wire;
pub mod writer;

#[cfg(test)]
mod tests;

/// Current wall-clock time in Unix milliseconds.
pub fn now_ms() -> Result<u64, String> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch".to_string())?;
    u64::try_from(elapsed.as_millis()).map_err(|_| "system clock is out of range".to_string())
}
