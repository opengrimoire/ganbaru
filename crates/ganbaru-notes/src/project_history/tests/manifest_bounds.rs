use super::*;
use crate::project_history::bundles::{HISTORY_VERSION_READ_LIMIT_ERROR, store_bundle_tx};
use crate::project_history::reads::{
    load_manifest_rows_within_tx, load_manifest_tx, notes_load_project_history_tree,
};

async fn checkpointed_version(pool: &SqlitePool) -> String {
    seed_project(pool).await;
    create_checkpoint(pool, PROJECT_ID, "baseline", None, None, "Initial version")
        .await
        .unwrap()
        .expect("the baseline creates a version")
        .id
}

#[test]
fn version_rows_load_within_their_decoded_byte_budget() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        let version_id = checkpointed_version(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let (manifest, _) = load_manifest_tx(&mut tx, PROJECT_ID, &version_id)
            .await
            .unwrap();
        let mut row_bytes = 0;
        for hash in manifest.rows_by_table.values().flatten() {
            let size: i64 = sqlx::query_scalar(
                "SELECT uncompressed_bytes FROM notes_history_bundles WHERE hash = ?",
            )
            .bind(hash)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            row_bytes += usize::try_from(size).unwrap();
        }
        assert!(row_bytes > 0);

        let rows = load_manifest_rows_within_tx(&mut tx, &manifest, row_bytes)
            .await
            .unwrap();
        assert_eq!(
            rows.values().map(Vec::len).sum::<usize>(),
            manifest.rows_by_table.values().map(Vec::len).sum::<usize>()
        );
        let error = load_manifest_rows_within_tx(&mut tx, &manifest, row_bytes - 1)
            .await
            .unwrap_err();
        assert_eq!(error, HISTORY_VERSION_READ_LIMIT_ERROR);
    });
}

#[test]
fn manifests_that_repeat_a_row_are_refused() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        let version_id = checkpointed_version(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let (mut manifest, _) = load_manifest_tx(&mut tx, PROJECT_ID, &version_id)
            .await
            .unwrap();
        let hashes = manifest
            .rows_by_table
            .get_mut("notes_blocks")
            .expect("the version records the page body");
        let repeated = hashes[0].clone();
        hashes.extend(std::iter::repeat_n(repeated, 1000));
        let raw = serde_json::to_vec(&manifest).unwrap();
        let manifest_hash = store_bundle_tx(&mut tx, "manifest", &raw).await.unwrap();
        sqlx::query("UPDATE notes_project_history_versions SET manifest_hash = ? WHERE id = ?")
            .bind(&manifest_hash)
            .bind(&version_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let Err(error) =
            notes_load_project_history_tree(&pool, PROJECT_ID.to_string(), version_id).await
        else {
            panic!("a manifest that repeats a row loaded");
        };
        assert_eq!(
            error,
            "Notes project history manifest repeats a row in notes_blocks"
        );
    });
}

#[test]
fn versions_saved_in_an_older_notes_format_stay_listed_but_refuse_to_open_or_restore() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        let version_id = checkpointed_version(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let (mut manifest, _) = load_manifest_tx(&mut tx, PROJECT_ID, &version_id)
            .await
            .unwrap();
        manifest.schema_version = 1;
        let raw = serde_json::to_vec(&manifest).unwrap();
        let manifest_hash = store_bundle_tx(&mut tx, "manifest", &raw).await.unwrap();
        sqlx::query("UPDATE notes_project_history_versions SET manifest_hash = ? WHERE id = ?")
            .bind(&manifest_hash)
            .bind(&version_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let listed = crate::project_history::reads::notes_list_project_history_versions(
            &pool,
            PROJECT_ID.to_string(),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(
            listed
                .versions
                .iter()
                .any(|version| version.id == version_id)
        );
        let Err(open_error) =
            notes_load_project_history_tree(&pool, PROJECT_ID.to_string(), version_id.clone())
                .await
        else {
            panic!("an older format version opened");
        };
        assert_eq!(open_error, crate::project_history::OLDER_NOTES_FORMAT_ERROR);
        let Err(restore_error) = restore::restore_version(&pool, PROJECT_ID, &version_id).await
        else {
            panic!("an older format version restored");
        };
        assert_eq!(
            restore_error,
            crate::project_history::OLDER_NOTES_FORMAT_ERROR
        );
    });
}
