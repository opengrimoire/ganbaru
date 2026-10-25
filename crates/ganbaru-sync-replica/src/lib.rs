//! A vault replica's part in concurrent sync, independent of Tauri: guarded database access that
//! signals commits, the anchor's founding keys, charter, and proposal sequencing, the admin
//! proposal outbox, the installation writer with its keys and device-local record, carry-forward of local operations
//! across whole-vault replacement, recovery offers, Reset sync state, both sides of the LAN exchange of operations
//! and managed asset files over the handoff protocol, the sweep of unreferenced asset files, and
//! whole-vault archives for backups and bootstrap snapshots. The app owns the service lifecycle, the transport, and the frontend projection.

pub mod access;
pub mod assets;
pub mod authority;
pub mod backup;
pub mod carry_forward;
pub mod client;
#[cfg(desktop)]
pub mod hub;
pub mod outbox;
pub mod quarantine;
pub mod recovery;
pub mod reset;
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
