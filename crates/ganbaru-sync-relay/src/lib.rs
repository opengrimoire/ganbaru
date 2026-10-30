//! Self-hosted Ganbaru sync relay.
//!
//! In relay mode the server stores sealed relay records and asset chunks for spaces it was
//! claimed for, and hands them to devices that hold an epoch access key. It verifies the clear
//! slot headers, access signatures, epoch registrations, and admin commands, and nothing inside
//! the ciphertext. It never learns which device sent a record: devices authenticate with a key
//! shared by the whole space for one epoch.
//!
//! The wire contract lives in `ganbaru_handoff::protocol::relay` so devices share it.

pub mod config;
pub mod identity;
pub mod limits;
pub mod server;
pub mod setup;
pub mod store;

#[cfg(test)]
mod tests;

use crate::config::{DATABASE_FILE, ServeConfig, SetupConfig};
use crate::identity::RelayIdentity;
use crate::server::{ConnectionLimits, RelayServer, unix_time_ms};
use crate::store::{RelayStore, RetentionPolicy};
use std::path::Path;

fn prepare_data_dir(data_dir: &Path) -> Result<(), String> {
    ganbaru_atomic_files::create_private_directories(data_dir)
        .map_err(|error| format!("create data directory {}: {error}", data_dir.display()))
}

/// Runs the relay until the process stops.
pub async fn serve(config: ServeConfig) -> Result<(), String> {
    prepare_data_dir(&config.data_dir)?;
    let identity = RelayIdentity::load_or_create(&config.data_dir)?;
    let store = RelayStore::open(&config.data_dir.join(DATABASE_FILE), config.retention).await?;
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|error| format!("listen on {}: {error}", config.listen))?;
    let server = RelayServer::new(
        listener,
        identity.server_config()?,
        store,
        ConnectionLimits::default(),
    );
    println!(
        "Ganbaru relay listening on {} with certificate fingerprint {}",
        server.local_addr()?,
        identity.fingerprint()
    );
    server.run().await;
    Ok(())
}

/// Issues one setup code and returns the text to print.
pub async fn setup(config: SetupConfig) -> Result<String, String> {
    prepare_data_dir(&config.data_dir)?;
    let identity = RelayIdentity::load_or_create(&config.data_dir)?;
    let store = RelayStore::open(
        &config.data_dir.join(DATABASE_FILE),
        RetentionPolicy::default(),
    )
    .await?;
    let issued = setup::issue(&store, &identity, config.endpoints, unix_time_ms()).await;
    store.close().await;
    setup::describe(&issued?, &identity)
}
