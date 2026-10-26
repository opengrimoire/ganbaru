use super::helpers::*;
use crate::sync_keys::{DERIVED_KEY_TABLES, assert_converted, convert};

/// Every derived-key row as `(table, rowid, sync_key)`, in a stable order.
async fn sync_keys(pool: &sqlx::SqlitePool) -> Vec<(String, i64, String)> {
    let mut keys = Vec::new();
    for table in DERIVED_KEY_TABLES {
        let rows: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT rowid, sync_key FROM {} ORDER BY rowid",
            table.table
        )))
        .fetch_all(pool)
        .await
        .unwrap();
        keys.extend(
            rows.into_iter()
                .map(|(rowid, key)| (table.table.to_string(), rowid, key)),
        );
    }
    keys
}

#[test]
fn placeholder_keys_convert_to_derived_keys_once_and_gate_the_assertion() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_A,
        )
        .await;
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                    "Stage": {
                        "id": "stage",
                        "name": "Stage",
                        "type": "select",
                        "select": { "options": [
                            { "id": "todo", "name": "To do", "color": "gray" },
                            { "id": "done", "name": "Done", "color": "green" }
                        ] }
                    },
                    "Blocks": {
                        "id": "blocks",
                        "name": "Blocks",
                        "type": "relation",
                        "relation": { "data_source_id": DATA_SOURCE_A }
                    }
                }),
            },
        )
        .await
        .unwrap();
        for (page_id, block_id, title) in [(PAGE_B, BLOCK_B, "First"), (PAGE_C, BLOCK_C, "Second")]
        {
            data_sources::rows::create_data_source_row_page(
                &pool,
                DATA_SOURCE_A,
                NoteDataSourceRowPageCreate {
                    id: page_id.to_string(),
                    title: title.to_string(),
                    first_block_id: block_id.to_string(),
                    properties: None,
                },
            )
            .await
            .unwrap();
        }
        for (property_id, value) in [("stage", json!("Done")), ("blocks", json!([PAGE_C]))] {
            data_sources::layouts::table::update_data_source_row_property(
                &pool,
                DATA_SOURCE_A,
                PAGE_B,
                NoteDataSourceRowPropertyUpdate {
                    property_id: property_id.to_string(),
                    value,
                },
            )
            .await
            .unwrap();
        }

        let derived = sync_keys(&pool).await;
        for table in DERIVED_KEY_TABLES {
            assert!(
                derived.iter().any(|(name, _, _)| name == table.table),
                "{} has no rows to convert",
                table.table
            );
        }
        assert!(
            derived
                .iter()
                .all(|(_, _, key)| !key.starts_with("pending:"))
        );
        assert_converted(&pool).await.unwrap();
        assert_eq!(convert(&pool).await.unwrap(), 0);

        // Sync keeps published keys from changing, except a placeholder that becomes its derived
        // key, so planting placeholders again drops that guard, as before the tables replicated.
        for table in DERIVED_KEY_TABLES {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP TRIGGER sync_immutable_{table};
                 UPDATE {table} SET sync_key = 'pending:' || rowid",
                table = table.table
            )))
            .execute(&pool)
            .await
            .unwrap();
        }
        let refusal = assert_converted(&pool).await.unwrap_err();
        assert!(refusal.contains("placeholder keys"), "{refusal}");

        assert_eq!(convert(&pool).await.unwrap(), derived.len() as u64);
        assert_eq!(sync_keys(&pool).await, derived);
        assert_converted(&pool).await.unwrap();
        assert_eq!(convert(&pool).await.unwrap(), 0);
        assert_eq!(sync_keys(&pool).await, derived);
    });
}
