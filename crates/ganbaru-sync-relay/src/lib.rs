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

use crate::config::{
    DATABASE_FILE, RemoveSpaceConfig, RevokeSetupConfig, ServeConfig, SetupConfig,
};
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
    let _lock = lock_data_dir(&config.data_dir)?;
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

/// File in the data directory that a serving relay holds an exclusive lock on.
pub const SERVE_LOCK_FILE: &str = "relay.lock";

/// An exclusive lock on a data directory, released when dropped.
#[derive(Debug)]
pub struct DataDirLock {
    _file: std::fs::File,
}

/// Takes the exclusive serve lock of `data_dir`, or explains that a relay holds it already.
pub fn lock_data_dir(data_dir: &Path) -> Result<DataDirLock, String> {
    let path = data_dir.join(SERVE_LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|error| format!("open {}: {error}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(DataDirLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => Err(format!(
            "a relay is serving from {}; stop it first",
            data_dir.display()
        )),
        Err(std::fs::TryLockError::Error(error)) => {
            Err(format!("lock {}: {error}", path.display()))
        }
    }
}

async fn open_existing_store(data_dir: &Path) -> Result<RelayStore, String> {
    let path = data_dir.join(DATABASE_FILE);
    if !path.is_file() {
        return Err(format!("no relay database at {}", path.display()));
    }
    RelayStore::open(&path, RetentionPolicy::default()).await
}

/// Lists the claimed spaces and returns the text to print.
pub async fn list_spaces(data_dir: &Path) -> Result<String, String> {
    let store = open_existing_store(data_dir).await?;
    let spaces = store.spaces().await;
    store.close().await;
    let spaces = spaces.map_err(|error| format!("list spaces: {error}"))?;
    if spaces.is_empty() {
        return Ok("No spaces are claimed on this relay.\n".to_string());
    }
    let mut text = format!(
        "{:<32}  {:>5}  {:>12}  {}\n",
        "space id", "epoch", "stored bytes", "claimed at (unix ms)"
    );
    for space in spaces {
        text.push_str(&format!(
            "{:<32}  {:>5}  {:>12}  {}\n",
            space.space.to_hex(),
            space.current_epoch,
            space.stored_bytes,
            space.created_at_ms
        ));
    }
    Ok(text)
}

/// Deletes one space while no relay serves from its data directory, and returns the text to
/// print. A serving relay keeps space state in memory, so removal waits until it stops.
pub async fn remove_space(config: RemoveSpaceConfig) -> Result<String, String> {
    let _lock = lock_data_dir(&config.data_dir)?;
    let store = open_existing_store(&config.data_dir).await?;
    let removed = store.remove_space(&config.space).await;
    store.close().await;
    let hex = config.space.to_hex();
    if removed.map_err(|error| format!("remove space: {error}"))? {
        Ok(format!(
            "Removed space {hex} and everything stored for it.\n"
        ))
    } else {
        Err(format!("no space {hex} on this relay"))
    }
}

/// Revokes unused setup codes and returns the text to print. A serving relay checks tokens in
/// its database on every claim, so this works while it runs.
pub async fn revoke_setup(config: RevokeSetupConfig) -> Result<String, String> {
    let store = open_existing_store(&config.data_dir).await?;
    let result = match &config.token {
        Some(token) => store.revoke_setup_token(token).await.map(|revoked| {
            if revoked {
                "Revoked the setup code.\n".to_string()
            } else {
                "That setup code was already used, expired, or revoked.\n".to_string()
            }
        }),
        None => store
            .revoke_all_setup_tokens()
            .await
            .map(|count| format!("Revoked {count} unused setup codes.\n")),
    };
    store.close().await;
    result.map_err(|error| format!("revoke setup codes: {error}"))
}
