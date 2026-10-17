//! Concurrent sync of the active vault: the service that seals local changes and exchanges with
//! the hub, the paired transport, and the recovery offers and device names the frontend shows.
//! Replica access, the local writer, carry-forward, and the exchange live in
//! `ganbaru-sync-replica`.

pub(crate) mod commands;
pub(crate) mod devices;
mod recovery;
mod service;
mod status;
mod transport;

#[cfg(desktop)]
pub(crate) use ganbaru_sync_replica::hub;
use ganbaru_sync_replica::{access, carry_forward, client, writer};

#[cfg(test)]
mod tests;

pub(crate) use ganbaru_sync_replica::now_ms;
#[cfg(desktop)]
pub(crate) use service::sync_hub;
pub(crate) use service::{
    SyncRuntime, export_for_replacement, quiesced_sync, resume_after_vault_handoff, seal_quiesced,
    setup, stop_for_vault_handoff, sync_files,
};
pub(crate) use status::SyncSubscribers;
