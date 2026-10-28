//! Concurrent sync of the active vault: the service that seals local changes and exchanges with
//! the hub, the paired transport, and the recovery offers and device names the frontend shows.
//! Replica access, the local writer, carry-forward, and the exchange live in
//! `ganbaru-sync-replica`. A pass runs through a host, so the app and a headless pass without a
//! Tauri runtime share it.

mod asset_collection;
mod authority;
pub(crate) mod commands;
mod connectivity;
pub(crate) mod devices;
pub(crate) mod discovery_keys;
#[cfg(any(target_os = "android", test))]
pub(crate) mod headless;
mod host;
pub(crate) mod key_protection;
mod peers;
mod recovery;
mod service;
mod status;
mod transport;
mod vault_assets;

pub(crate) use ganbaru_sync_replica::hub;
use ganbaru_sync_replica::{assets, carry_forward, client, writer};

#[cfg(test)]
mod tests;

#[cfg(target_os = "linux")]
pub(crate) use connectivity::firewall::changed as firewall_changed;
pub(crate) use ganbaru_sync_replica::now_ms;
pub(crate) use peers::{peer_hints, read_peer_book, reported_members};
pub(crate) use service::{
    SyncRuntime, ensure_device_key, member_admission_source, member_hub, quiesced_sync,
    release_runtime_handle, resume_after_vault_handoff, seal_quiesced, setup,
    stop_for_vault_handoff, sync_files,
};
#[cfg(desktop)]
pub(crate) use service::{
    enrollment_sealer, ensure_admin_key, quarantined_devices, stored_admin_key, sync_hub,
};
pub(crate) use status::SyncSubscribers;
#[cfg(desktop)]
pub(crate) use status::emit_applied;
