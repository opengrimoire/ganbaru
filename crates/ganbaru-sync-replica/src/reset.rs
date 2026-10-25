//! Reset sync state on the coordinator. Vaults that hold sync state the current build refuses,
//! or a person who wants to start the space over, clear the engine tables and this
//! installation's writer state; the next pass founds a new space with a new charter and
//! republishes every current row. Every other device unlinks and pairs again, because it pinned
//! the old charter.

use crate::carry_forward;
use crate::outbox::Outbox;
use crate::writer::{SyncFiles, SyncKey, SyncKeyStore, WriterRecord, read_record, write_record};
use ganbaru_sync::local;
use sqlx::SqliteConnection;

/// Drops the anchor's authorization high-water from the writer record, the first step of a
/// Reset. Founding refuses a space without a charter while the record names a high-water,
/// because that database was restored from before the space was founded; a Reset that crashes
/// after clearing the database must still found anew on the next pass. A crash before the
/// database is cleared leaves the old log without a high-water until the anchor sequences again.
pub fn forget_high_water(files: &SyncFiles) -> Result<(), String> {
    let path = files.record_path();
    match read_record(&path) {
        Ok(Some(record)) if record.sequenced.is_some() => write_record(
            &path,
            &WriterRecord {
                sequenced: None,
                ..record
            },
        ),
        // An unreadable record is removed by the rest of the reset.
        Ok(_) | Err(_) => Ok(()),
    }
}

/// Refuses to found a new space while the writer record still names the high-water of a log:
/// the database lost its charter to a restore from before the space was founded, and a new
/// charter would leave every linked device refusing it. Only Reset sync state founds anew.
pub fn refuse_refounding(record: Option<&WriterRecord>) -> Result<(), String> {
    if record.is_some_and(|record| record.sequenced.is_some()) {
        return Err(
            "this database was restored from before the vault's sync space was founded; restore \
             a newer backup or reset sync state"
                .to_string(),
        );
    }
    Ok(())
}

/// Clears every engine table of the vault database, keeping domain rows.
pub async fn reset_database(conn: &mut SqliteConnection) -> Result<(), String> {
    local::reset_sync_state(conn)
        .await
        .map_err(|error| format!("reset sync state: {error}"))
}

/// Removes this installation's writer state of a vault: the proposal outbox, a carry-forward
/// bundle, the writer keys its record names, and the record, which all belong to the cleared
/// chain. The outbox goes first so a crash part way never leaves proposals for the old charter
/// beside a fresh record; the anchor refuses such proposals anyway because they name the old
/// charter. The device key and the anchor keys stay, so the new charter names the same device and
/// admin keys. Run it after [`reset_database`]: a crash between the two leaves a record whose
/// writer the space lacks, which the next pass replaces with a successor. Key stores may block,
/// so callers run this off the async runtime.
pub fn remove_installation_state(files: &SyncFiles, keys: &dyn SyncKeyStore) -> Result<(), String> {
    Outbox::remove(&files.outbox_path())?;
    carry_forward::remove_bundle(files)?;
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
    match std::fs::remove_file(&record_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove sync writer record: {error}")),
    }
}
