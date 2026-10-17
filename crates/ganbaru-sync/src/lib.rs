//! Sync engine for replicated vault tables.
//!
//! The manifest declares which tables replicate, how their columns form field groups, and how
//! each group merges. Capture triggers rendered from the manifest record local changes in
//! `sync_capture`; the engine seals them into signed operations, stores remote operations, and
//! applies them with deterministic merges so every replica converges.
//!
//! Capture suppression depends on the single-connection invariant of vault pools: the
//! `sync_apply_state.applying` flag is only correct because nothing interleaves inside an engine
//! transaction.
//!
//! Every engine entry point takes the vault connection and runs in its own transaction, so a
//! failure leaves engine state and domain tables as they were.

use std::io;

use ganbaru_contacts::PersonPublicKey;
use ganbaru_sync_contracts::{SignedCertificate, SpaceId, WriterId, WriterKeyPair};
use sqlx::SqliteConnection;

pub mod adapter;
pub mod devices;
pub mod domains;
pub mod error;
pub mod guards;
pub mod local;
pub mod manifest;
pub mod triggers;

mod apply;
mod conflicts;
mod log;
mod materialize;
mod merge;
mod recovery;
mod repair;
mod reseal;
mod seal;
mod sql;
mod status;
mod store;
mod validate;

#[cfg(test)]
mod tests;

pub use adapter::{AdapterError, Adapters, DomainAdapter, Presentation};
pub use apply::{ApplyReport, ClockWarning, ResealNeeded};
pub use conflicts::{ConflictDetail, ConflictRow, ConflictVersion, OPEN_CONFLICT_MASK_SQL};
pub use devices::{DeviceNames, DeviceRef};
pub use error::{SyncError, SyncResult};
pub use local::{CarryReport, LocalWriterHead};
pub use log::{OpsPage, WriterState};
pub use recovery::RecoveryEntry;
pub use reseal::ResealReport;
pub use seal::{InvalidRow, SealReport};
pub use status::{HeldCounts, HeldOp, SyncStatus, WriterSummary};
pub use store::{HoldReason, StoreOutcome, StoreRefusal};

use domains::quick_notes::QuickNotesAdapter;
use manifest::Manifest;
use manifest::vault::VAULT_MANIFEST;

/// The space an engine call works in and the person key every writer must be certified by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceContext {
    /// Replication space of the vault.
    pub space: SpaceId,
    /// Trust anchor: the person key of the vault owner.
    pub anchor: PersonPublicKey,
}

/// Makes a writer sequence durable before an operation with it is signed, so a restored or
/// cloned database can never sign a second operation with the same sequence.
pub trait SequenceReservation: Send {
    /// Ensures sequences up to `seq` are reserved, advancing the durable reservation if needed.
    fn ensure_reserved(&mut self, seq: u64) -> io::Result<()>;
}

/// The local writer an engine call may seal with.
pub struct LocalWriter<'a> {
    /// Writer signing key.
    pub key: &'a WriterKeyPair,
    /// Certificate binding the key to the space anchor.
    pub certificate: &'a SignedCertificate,
    /// Durable sequence reservation of the writer record.
    pub reservation: &'a mut dyn SequenceReservation,
}

impl LocalWriter<'_> {
    /// Writer id the certificate binds.
    pub fn id(&self) -> WriterId {
        self.certificate.certificate.writer_id()
    }
}

/// The sync engine: a manifest and the domain adapters it names.
#[derive(Debug)]
pub struct Engine {
    manifest: &'static Manifest,
    adapters: Adapters,
}

impl Engine {
    /// An engine for a manifest, refusing a manifest that names an unregistered adapter.
    pub fn new(manifest: &'static Manifest, adapters: Adapters) -> SyncResult<Self> {
        for table in manifest.tables {
            if let Some(name) = table.adapter {
                if adapters.get(name).is_none() {
                    return Err(SyncError::UnregisteredAdapter(name));
                }
            }
        }
        Ok(Self { manifest, adapters })
    }

    /// The engine for the vault manifest with its built-in adapters.
    pub fn vault() -> Self {
        Self {
            manifest: &VAULT_MANIFEST,
            adapters: Adapters::new().with(Box::new(QuickNotesAdapter)),
        }
    }

    /// The manifest the engine replicates.
    pub fn manifest(&self) -> &'static Manifest {
        self.manifest
    }

    /// Adapter of a table, when the manifest names one.
    pub(crate) fn adapter(&self, table: &manifest::TableSpec) -> Option<&dyn DomainAdapter> {
        table.adapter.and_then(|name| self.adapters.get(name))
    }

    /// Creates the personal space of a vault if it does not exist and returns its id.
    pub async fn init_space(
        &self,
        conn: &mut SqliteConnection,
        vault_id: &str,
        now_ms: u64,
    ) -> SyncResult<SpaceId> {
        let space = SpaceId::personal(vault_id);
        sqlx::query(
            "INSERT INTO sync_spaces (space_id, vault_id, created_at_ms) VALUES (?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(space.as_bytes().as_slice())
        .bind(vault_id)
        .bind(log::clamp_ms(now_ms))
        .execute(&mut *conn)
        .await?;
        let stored: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT space_id FROM sync_spaces WHERE vault_id = ?")
                .bind(vault_id)
                .fetch_optional(&mut *conn)
                .await?;
        match stored {
            Some(bytes) if bytes == space.as_bytes() => Ok(space),
            _ => Err(error::corrupt("sync space does not match the vault id")),
        }
    }
}
