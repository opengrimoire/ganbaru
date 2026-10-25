//! Reset sync state on the coordinator. Vaults that hold sync state the current build refuses,
//! or a person who wants to start the space over, clear the engine tables and this
//! installation's writer state; the next pass founds a new space with a new charter and
//! republishes every current row. Every other device unlinks and pairs again, because it pinned
//! the old charter.

use crate::carry_forward;
use crate::outbox::Outbox;
use crate::writer::{SyncFiles, SyncKey, SyncKeyStore, read_record};
use ganbaru_sync::local;
use sqlx::SqliteConnection;

/// Clears every engine table of the vault database, keeping domain rows.
pub async fn reset_database(conn: &mut SqliteConnection) -> Result<(), String> {
    local::reset_sync_state(conn)
        .await
        .map_err(|error| format!("reset sync state: {error}"))
}

/// Removes this installation's writer state of a vault: the writer keys its record names, the
/// record, a carry-forward bundle, and the proposal outbox, which all belong to the cleared
/// chain. The device key and the anchor keys stay, so the new charter names the same device and
/// admin keys. Run it after [`reset_database`]: a crash between the two leaves a record whose
/// writer the space lacks, which the next pass replaces with a successor. Key stores may block,
/// so callers run this off the async runtime.
pub fn remove_installation_state(files: &SyncFiles, keys: &dyn SyncKeyStore) -> Result<(), String> {
    let record_path = files.record_path();
    // An unreadable record names no key this installation can still find, and the reset
    // removes it either way.
    if let Ok(Some(record)) = read_record(&record_path) {
        for writer_id in record
            .retired_keys
            .iter()
            .chain(std::iter::once(&record.writer_id))
        {
            keys.remove(&files.vault_id, SyncKey::Writer(writer_id))?;
        }
    }
    carry_forward::remove_bundle(files)?;
    Outbox::remove(&files.outbox_path())?;
    match std::fs::remove_file(&record_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove sync writer record: {error}")),
    }
}
