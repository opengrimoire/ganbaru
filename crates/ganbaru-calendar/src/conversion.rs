//! Calendar conversion pass run at vault open, before sync captures are seeded.
//!
//! The Calendar replication migration leaves `'pending:' || rowid` placeholders in the archive
//! import object `sync_key`. This pass rewrites each placeholder to the key every device derives
//! from the same row. It is idempotent, so a vault that is already converted is left unchanged.

use ganbaru_sync_contracts::PENDING_KEY_PREFIX;
use sqlx::SqlitePool;

use crate::keys::archive_import_object_key;

/// Matches placeholder keys.
const PENDING_CONDITION: &str = "substr(sync_key, 1, length(?1)) = ?1";

/// Rewrites placeholder archive import object keys in one transaction. Returns the rewritten count.
pub async fn convert(pool: &SqlitePool) -> Result<u64, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin calendar conversion: {error}"))?;
    let rows = sqlx::query_as::<_, (i64, String, String)>(sqlx::AssertSqlSafe(format!(
        "SELECT rowid, archive_event_id, object_id FROM calendar_event_archive_import_objects
         WHERE {PENDING_CONDITION} ORDER BY rowid"
    )))
    .bind(PENDING_KEY_PREFIX)
    .fetch_all(&mut *tx)
    .await
    .map_err(|error| format!("load calendar archive import object placeholder keys: {error}"))?;
    for (rowid, archive_event_id, object_id) in &rows {
        sqlx::query(
            "UPDATE calendar_event_archive_import_objects SET sync_key = ?1 WHERE rowid = ?2",
        )
        .bind(archive_import_object_key(archive_event_id, object_id)?)
        .bind(rowid)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("write calendar archive import object key: {error}"))?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit calendar conversion: {error}"))?;
    Ok(rows.len() as u64)
}

/// Refuses a vault where any Calendar key is still a placeholder, so sync never publishes one.
pub async fn assert_converted(pool: &SqlitePool) -> Result<(), String> {
    let remaining: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM calendar_event_archive_import_objects WHERE {PENDING_CONDITION}"
    )))
    .bind(PENDING_KEY_PREFIX)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("check calendar archive import object keys: {error}"))?;
    if remaining > 0 {
        return Err(format!(
            "calendar_event_archive_import_objects still has {remaining} placeholder keys; run the calendar conversion first"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{assert_converted, convert};
    use crate::keys::archive_import_object_key;

    async fn pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        pool
    }

    async fn insert_placeholder(pool: &sqlx::SqlitePool) {
        for statement in [
            "INSERT INTO icalendar_objects (id, import_id, calendar_id, source_kind, source_name, source_fingerprint, created_at, updated_at)
             VALUES ('object', 'import', 'local', 'import-file', 'a.ics', 'f', 't', 't')",
            "INSERT INTO calendar_event_archives (id, source_event_id, archived_at, start_time, end_time, calendar_id, created_at, updated_at)
             VALUES ('archive', 'event', 't', 's', 'e', 'local', 't', 't')",
            "INSERT INTO calendar_event_archive_import_objects (archive_event_id, object_id, sync_key)
             VALUES ('archive', 'object', 'pending:1')",
        ] {
            sqlx::query(statement).execute(pool).await.unwrap();
        }
    }

    #[test]
    fn conversion_derives_archive_import_object_keys_once() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            insert_placeholder(&pool).await;
            assert!(assert_converted(&pool).await.is_err());

            assert_eq!(convert(&pool).await.unwrap(), 1);
            assert_converted(&pool).await.unwrap();
            let key: String =
                sqlx::query_scalar("SELECT sync_key FROM calendar_event_archive_import_objects")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(key, archive_import_object_key("archive", "object").unwrap());
            assert_eq!(convert(&pool).await.unwrap(), 0);
        });
    }
}
