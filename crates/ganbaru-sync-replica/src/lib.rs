//! A vault replica's part in concurrent sync, independent of Tauri: guarded database access that
//! signals commits, the installation writer with its keys and device-local record, carry-forward
//! of local operations across whole-vault replacement, recovery offers, and both sides of the LAN
//! exchange over the handoff protocol. The app owns the service lifecycle, the transport, and
//! the frontend projection.

pub mod access;
pub mod carry_forward;
pub mod client;
#[cfg(desktop)]
pub mod hub;
pub mod recovery;
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
