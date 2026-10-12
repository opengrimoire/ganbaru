use super::helpers::*;
use crate::models::NoteDataSourceAttach;
use crate::{data_sources, databases};
use serde_json::Value;

const SOURCE_PROJECT: &str = "view-history-source-owner";
const SHELL_PROJECT: &str = "view-history-requesting-shell";

async fn attached_view(pool: &SqlitePool, kind: &str) -> Value {
    match kind {
        "table" => serde_json::to_value(
            data_sources::layouts::table::data_source_table_view(
                pool,
                DATA_SOURCE_A,
                Some(DATABASE_B),
                None,
            )
            .await
            .unwrap(),
        )
        .unwrap(),
        "board" => serde_json::to_value(
            data_sources::layouts::board::data_source_board_view(
                pool,
                DATA_SOURCE_A,
                Some(DATABASE_B),
                None,
            )
            .await
            .unwrap(),
        )
        .unwrap(),
        "gallery" => serde_json::to_value(
            data_sources::layouts::gallery::data_source_gallery_view(
                pool,
                DATA_SOURCE_A,
                Some(DATABASE_B),
                None,
            )
            .await
            .unwrap(),
        )
        .unwrap(),
        "list" => serde_json::to_value(
            data_sources::layouts::list::data_source_list_view(
                pool,
                DATA_SOURCE_A,
                Some(DATABASE_B),
                None,
            )
            .await
            .unwrap(),
        )
        .unwrap(),
        "calendar" => serde_json::to_value(
            data_sources::layouts::calendar::data_source_calendar_view(
                pool,
                DATA_SOURCE_A,
                Some(DATABASE_B),
                None,
            )
            .await
            .unwrap(),
        )
        .unwrap(),
        "timeline" => serde_json::to_value(
            data_sources::layouts::timeline::data_source_timeline_view(
                pool,
                DATA_SOURCE_A,
                Some(DATABASE_B),
                None,
            )
            .await
            .unwrap(),
        )
        .unwrap(),
        _ => unreachable!("unsupported test layout"),
    }
}

async fn update_attached_view(
    pool: &SqlitePool,
    kind: &str,
    view_id: &str,
    update: Value,
) -> Result<(), String> {
    match kind {
        "table" => data_sources::layouts::table::update_data_source_table_view(
            pool,
            DATA_SOURCE_A,
            Some(DATABASE_B),
            Some(view_id),
            serde_json::from_value(update).unwrap(),
        )
        .await
        .map(|_| ()),
        "board" => data_sources::layouts::board::update_data_source_board_view(
            pool,
            DATA_SOURCE_A,
            Some(DATABASE_B),
            Some(view_id),
            serde_json::from_value(update).unwrap(),
        )
        .await
        .map(|_| ()),
        "gallery" => data_sources::layouts::gallery::update_data_source_gallery_view(
            pool,
            DATA_SOURCE_A,
            Some(DATABASE_B),
            Some(view_id),
            serde_json::from_value(update).unwrap(),
        )
        .await
        .map(|_| ()),
        "list" => data_sources::layouts::list::update_data_source_list_view(
            pool,
            DATA_SOURCE_A,
            Some(DATABASE_B),
            Some(view_id),
            serde_json::from_value(update).unwrap(),
        )
        .await
        .map(|_| ()),
        "calendar" => data_sources::layouts::calendar::update_data_source_calendar_view(
            pool,
            DATA_SOURCE_A,
            Some(DATABASE_B),
            Some(view_id),
            serde_json::from_value(update).unwrap(),
        )
        .await
        .map(|_| ()),
        "timeline" => data_sources::layouts::timeline::update_data_source_timeline_view(
            pool,
            DATA_SOURCE_A,
            Some(DATABASE_B),
            Some(view_id),
            serde_json::from_value(update).unwrap(),
        )
        .await
        .map(|_| ()),
        _ => unreachable!("unsupported test layout"),
    }
}

#[test]
fn attached_view_updates_checkpoint_both_projects_and_locked_failures_leave_history_clean() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO project_groups (id, name) VALUES ('view-history', 'View history')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for project in [SOURCE_PROJECT, SHELL_PROJECT] {
            sqlx::query("INSERT INTO projects (id, group_id, name) VALUES (?, 'view-history', ?)")
                .bind(project)
                .bind(project)
                .execute(&pool)
                .await
                .unwrap();
        }
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        for (page, project) in [(PAGE_A, SOURCE_PROJECT), (PAGE_B, SHELL_PROJECT)] {
            sqlx::query("UPDATE notes_pages SET properties = json_set(properties, '$.__ganbaru_project_id', ?) WHERE id = ?")
                .bind(project).bind(page).execute(&pool).await.unwrap();
        }
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Source",
            BLOCK_A,
        )
        .await;
        databases::create_database(
            &pool,
            NoteDatabaseCreate {
                id: DATABASE_B.to_string(),
                data_source_id: DATA_SOURCE_B.to_string(),
                view_id: DATABASE_VIEW_B.to_string(),
                title: "Requesting shell".to_string(),
                parent: Some(page_parent(PAGE_B)),
                after_block_id: Some(BLOCK_B.to_string()),
                replace_block_id: None,
                icon: None,
                cover: None,
            },
        )
        .await
        .unwrap();
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                    "Date": { "id": "date", "name": "Date", "type": "date", "date": {} },
                }),
            },
        )
        .await
        .unwrap();
        data_sources::management::attach_data_source(
            &pool,
            NoteDataSourceAttach {
                data_source_id: DATA_SOURCE_A.to_string(),
                database_id: DATABASE_B.to_string(),
                view_id: BLOCK_C.to_string(),
                view_name: "Shared table".to_string(),
            },
        )
        .await
        .unwrap();
        let source_schema: String =
            sqlx::query_scalar("SELECT properties FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap();

        for kind in ["table", "board", "gallery", "list", "calendar", "timeline"] {
            let loaded = attached_view(&pool, kind).await;
            let view_id = loaded["view"]["id"].as_str().unwrap();
            let mut configuration = loaded["view"]["configuration"][kind].clone();
            let next_mode = if configuration["row_open_mode"] == "side_panel" {
                "full_page"
            } else {
                "side_panel"
            };
            configuration["row_open_mode"] = json!(next_mode);
            sqlx::query("DELETE FROM notes_project_history_dirty")
                .execute(&pool)
                .await
                .unwrap();
            update_attached_view(
                &pool,
                kind,
                view_id,
                json!({
                    "filter": [], "sorts": [], "configuration": configuration,
                }),
            )
            .await
            .unwrap();
            let dirty: Vec<String> = sqlx::query_scalar(
                "SELECT project_id FROM notes_project_history_dirty ORDER BY project_id",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(
                dirty,
                vec![SHELL_PROJECT.to_string(), SOURCE_PROJECT.to_string()],
                "{kind}"
            );
            let saved: String =
                sqlx::query_scalar("SELECT configuration FROM notes_database_views WHERE id = ?")
                    .bind(view_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let saved: Value = serde_json::from_str(&saved).unwrap();
            assert_eq!(saved[kind]["row_open_mode"], next_mode, "{kind}");
        }

        sqlx::query("UPDATE notes_project_history_dirty SET force_checkpoint = 1")
            .execute(&pool)
            .await
            .unwrap();
        let schedule = project_history::notes_flush_due_project_history(&pool)
            .await
            .unwrap();
        assert_eq!(schedule.created_count, 2);
        for project in [SOURCE_PROJECT, SHELL_PROJECT] {
            let checkpoints: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_project_history_versions WHERE project_id = ? AND reason = 'checkpoint'")
                .bind(project).fetch_one(&pool).await.unwrap();
            assert_eq!(checkpoints, 1);
        }
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT properties FROM notes_data_sources WHERE id = ?"
            )
            .bind(DATA_SOURCE_A)
            .fetch_one(&pool)
            .await
            .unwrap(),
            source_schema
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT database_id FROM notes_data_sources WHERE id = ?"
            )
            .bind(DATA_SOURCE_A)
            .fetch_one(&pool)
            .await
            .unwrap(),
            DATABASE_A
        );

        databases::editing_lock::set_database_editing_lock(&pool, DATABASE_B, true)
            .await
            .unwrap();
        let loaded = attached_view(&pool, "table").await;
        let mut configuration = loaded["view"]["configuration"]["table"].clone();
        let previous_configuration = configuration.clone();
        configuration["row_open_mode"] =
            json!(if previous_configuration["row_open_mode"] == "side_panel" {
                "full_page"
            } else {
                "side_panel"
            });
        sqlx::query("DELETE FROM notes_project_history_dirty")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            update_attached_view(
                &pool,
                "table",
                BLOCK_C,
                json!({
                    "filter": [], "sorts": [], "configuration": configuration,
                })
            )
            .await
            .unwrap_err()
            .contains("locked")
        );
        let dirty_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_project_history_dirty")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(dirty_count, 0);
        let saved: String =
            sqlx::query_scalar("SELECT configuration FROM notes_database_views WHERE id = ?")
                .bind(BLOCK_C)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&saved).unwrap()["table"],
            previous_configuration
        );
    });
}
