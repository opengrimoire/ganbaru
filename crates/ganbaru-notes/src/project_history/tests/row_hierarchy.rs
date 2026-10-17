use super::*;
use crate::models::{NoteDataSourceRowPageCreate, NoteDatabaseCreate, NoteParent};
use crate::{data_sources, databases};

async fn seed_snapshot_hierarchy(pool: &SqlitePool) -> (&'static str, &'static str, &'static str) {
    seed_project(pool).await;
    sqlx::query("UPDATE notes_blocks SET payload = ? WHERE id = ?")
        .bind(crate::writes::default_text_payload("First version").to_string())
        .bind(BLOCK_ID)
        .execute(pool)
        .await
        .unwrap();
    let source = "62626262-6262-4262-8262-626262626262";
    let parent = "64646464-6464-4464-8464-646464646464";
    let child = "66666666-6666-4666-8666-666666666666";
    databases::create_database(
        pool,
        NoteDatabaseCreate {
            id: "61616161-6161-4161-8161-616161616161".into(),
            data_source_id: source.into(),
            view_id: "63636363-6363-4363-8363-636363636363".into(),
            title: "Tree".into(),
            parent: Some(NoteParent::PageId {
                page_id: PAGE_ID.into(),
            }),
            after_block_id: Some(BLOCK_ID.into()),
            replace_block_id: None,
            icon: None,
            cover: None,
        },
    )
    .await
    .unwrap();
    data_sources::rows::create_data_source_row_page(
        pool,
        source,
        NoteDataSourceRowPageCreate {
            id: parent.into(),
            first_block_id: "65656565-6565-4565-8565-656565656565".into(),
            title: "Parent".into(),
            properties: None,
        },
    )
    .await
    .unwrap();
    data_sources::row_hierarchy::create_subitem(
        pool,
        source,
        parent,
        NoteDataSourceRowPageCreate {
            id: child.into(),
            first_block_id: "67676767-6767-4767-8767-676767676767".into(),
            title: "Child".into(),
            properties: None,
        },
    )
    .await
    .unwrap();
    (source, parent, child)
}

#[test]
fn database_subitems_history_rejects_corrupt_cycles_and_foreign_edges_without_replacing_current_rows()
 {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        let (source, parent, child) = seed_snapshot_hierarchy(&pool).await;
        insert_project_page(&pool, LATER_PAGE_ID, LATER_BLOCK_ID, "Outside project").await;
        sqlx::query("UPDATE notes_pages SET properties = json_remove(properties, '$.__ganbaru_project_id') WHERE id = ?")
            .bind(LATER_PAGE_ID).execute(&pool).await.unwrap();
        for (index, invalid_parent) in [child, LATER_PAGE_ID, PAGE_ID].into_iter().enumerate() {
            if invalid_parent == child {
                sqlx::query("INSERT INTO notes_data_source_row_hierarchy(row_page_id, data_source_id, parent_row_page_id) VALUES (?, ?, ?)")
                    .bind(parent).bind(source).bind(child).execute(&pool).await.unwrap();
            } else {
                sqlx::query("UPDATE notes_data_source_row_hierarchy SET parent_row_page_id = ? WHERE row_page_id = ?")
                    .bind(invalid_parent).bind(child).execute(&pool).await.unwrap();
            }
            let invalid_version = create_checkpoint(
                &pool,
                PROJECT_ID,
                "manual",
                None,
                None,
                "Corrupt imported snapshot",
            )
            .await
            .unwrap()
            .unwrap();
            sqlx::query("DELETE FROM notes_data_source_row_hierarchy WHERE row_page_id = ?")
                .bind(parent)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("UPDATE notes_data_source_row_hierarchy SET parent_row_page_id = ? WHERE row_page_id = ?")
                .bind(parent).bind(child).execute(&pool).await.unwrap();
            let current_title = format!("Current child {index}");
            sqlx::query("UPDATE notes_pages SET title = ? WHERE id = ?")
                .bind(&current_title)
                .bind(child)
                .execute(&pool)
                .await
                .unwrap();
            let error = restore::restore_version(&pool, PROJECT_ID, &invalid_version.id)
                .await
                .err()
                .expect("corrupt sub-items must be rejected");
            assert!(error.contains("sub-items"), "{error}");
            let unchanged: String =
                sqlx::query_scalar("SELECT title FROM notes_pages WHERE id = ?")
                    .bind(child)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(unchanged, current_title);
            let retained_parent: String = sqlx::query_scalar("SELECT parent_row_page_id FROM notes_data_source_row_hierarchy WHERE row_page_id = ?")
                .bind(child).fetch_one(&pool).await.unwrap();
            assert_eq!(retained_parent, parent);
            let edges: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_source_row_hierarchy")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(edges, 1);
        }
    });
}

#[test]
fn database_subitems_history_restores_canonical_edges_and_copies_moved_graphs_independently() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_project(&pool).await;
        sqlx::query("UPDATE notes_blocks SET payload = ? WHERE id = ?")
            .bind(crate::writes::default_text_payload("First version").to_string())
            .bind(BLOCK_ID)
            .execute(&pool)
            .await
            .unwrap();
        let database_id = "61616161-6161-4161-8161-616161616161";
        let source_id = "62626262-6262-4262-8262-626262626262";
        let view_id = "63636363-6363-4363-8363-636363636363";
        let parent_id = "64646464-6464-4464-8464-646464646464";
        let child_id = "66666666-6666-4666-8666-666666666666";
        databases::create_database(
            &pool,
            NoteDatabaseCreate {
                id: database_id.into(),
                data_source_id: source_id.into(),
                view_id: view_id.into(),
                title: "Tree".into(),
                parent: Some(NoteParent::PageId {
                    page_id: PAGE_ID.into(),
                }),
                after_block_id: Some(BLOCK_ID.into()),
                replace_block_id: None,
                icon: None,
                cover: None,
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            source_id,
            NoteDataSourceRowPageCreate {
                id: parent_id.into(),
                first_block_id: "65656565-6565-4565-8565-656565656565".into(),
                title: "Parent".into(),
                properties: None,
            },
        )
        .await
        .unwrap();
        data_sources::row_hierarchy::create_subitem(
            &pool,
            source_id,
            parent_id,
            NoteDataSourceRowPageCreate {
                id: child_id.into(),
                first_block_id: "67676767-6767-4767-8767-676767676767".into(),
                title: "Child".into(),
                properties: None,
            },
        )
        .await
        .unwrap();
        let version = create_checkpoint(&pool, PROJECT_ID, "manual", None, None, "Sub-items")
            .await
            .unwrap()
            .unwrap();
        data_sources::row_hierarchy::set_row_parent(
            &pool,
            source_id,
            child_id,
            data_sources::row_hierarchy::NoteDataSourceRowParentUpdate {
                parent_row_page_id: None,
            },
        )
        .await
        .unwrap();
        restore::restore_version(&pool, PROJECT_ID, &version.id)
            .await
            .unwrap();
        let restored_parent: String = sqlx::query_scalar(
            "SELECT parent_row_page_id FROM notes_data_source_row_hierarchy WHERE row_page_id = ?",
        )
        .bind(child_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(restored_parent, parent_id);

        // Moving the owning note outside this project leaves its graph intact, so restore inserts a copy with new identities.
        sqlx::query("UPDATE notes_pages SET properties = json_remove(properties, '$.__ganbaru_project_id') WHERE id = ?")
            .bind(PAGE_ID).execute(&pool).await.unwrap();
        restore::restore_version(&pool, PROJECT_ID, &version.id)
            .await
            .unwrap();
        let edges: Vec<(String, String, String)> = sqlx::query_as("SELECT data_source_id, row_page_id, parent_row_page_id FROM notes_data_source_row_hierarchy ORDER BY data_source_id")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(edges.len(), 2);
        assert!(
            edges
                .iter()
                .any(|(source, child, parent)| source == source_id
                    && child == child_id
                    && parent == parent_id)
        );
        let copied = edges
            .iter()
            .find(|(source, _, _)| source != source_id)
            .unwrap();
        assert_ne!(copied.1, child_id);
        assert_ne!(copied.2, parent_id);
        let shared_ownership: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_pages WHERE id IN (?, ?) AND parent_data_source_id = ?",
        )
        .bind(&copied.1)
        .bind(&copied.2)
        .bind(&copied.0)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(shared_ownership, 2);
    });
}
