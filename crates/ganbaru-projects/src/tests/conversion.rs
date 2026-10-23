use super::*;
use crate::conversion::{ConversionReport, assert_converted, convert};
use crate::icons::MISSING_ICON_FALLBACK;
use crate::keys::{dependency_key, task_tag_key};
use std::fs;

fn conversion_vault(name: &str) -> std::path::PathBuf {
    let vault_root = std::env::temp_dir().join(format!(
        "ganbaru-projects-conversion-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&vault_root);
    fs::create_dir_all(vault_root.join("assets/project-icons")).unwrap();
    fs::write(vault_root.join("assets/project-icons/kept.png"), [7_u8; 12]).unwrap();
    vault_root
}

/// Writes rows the way the order keys migration leaves them. The size triggers refuse new writes without a size, so
/// they are dropped while the migrated state is staged.
async fn insert_placeholder_rows(pool: &SqlitePool) {
    sqlx::raw_sql(
        "DROP TRIGGER project_groups_icon_size_update;
         DROP TRIGGER projects_icon_size_update;
         DROP TRIGGER project_custom_emojis_asset_size_insert;
         INSERT INTO project_tags (id, project_id, name, order_key) VALUES ('tag-a', 'project-a', 'Backend', 'a1');
         INSERT INTO project_task_dependencies (id, sync_key, blocking_task_id, blocked_task_id, dependency_type)
             VALUES ('pending:1', 'pending:1', 'task-a', 'task-b', 'blocks');
         INSERT INTO project_task_tag_links (task_id, tag_id, sync_key) VALUES ('task-a', 'tag-a', 'pending:2');
         UPDATE project_groups SET icon = 'asset:project-icons/kept.png', icon_byte_size = NULL WHERE id = 'group-a';
         UPDATE projects SET icon = 'asset:project-icons/missing.png', icon_byte_size = NULL WHERE id = 'project-a';
         INSERT INTO project_custom_emojis (id, name, asset_path, order_key)
             VALUES ('emoji-kept', 'Kept', 'project-icons/kept.png', 'a1'),
                    ('emoji-gone', 'Gone', 'project-icons/missing.png', 'a2');",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn conversion_rewrites_placeholder_keys_and_icon_sizes_and_a_second_pass_changes_nothing() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        insert_placeholder_rows(&pool).await;
        let vault_root = conversion_vault("idempotent");

        let error = assert_converted(&pool).await.err().unwrap();
        assert!(error.contains("placeholder keys"), "{error}");

        let first = convert(&pool, &vault_root).await.unwrap();
        assert!(first.rekeyed_rows >= 2, "{first:?}");
        assert_eq!(first.sized_icons, 2);
        assert_eq!(first.reset_icons, 1);
        assert_eq!(first.removed_emojis, 1);
        assert_converted(&pool).await.unwrap();

        let dependency_key = dependency_key("task-a", "task-b").unwrap();
        let dependency: (String, String) =
            sqlx::query_as("SELECT id, sync_key FROM project_task_dependencies")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(dependency, (dependency_key.clone(), dependency_key));
        let link: String = sqlx::query_scalar("SELECT sync_key FROM project_task_tag_links")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(link, task_tag_key("task-a", "tag-a").unwrap());
        let group: (String, Option<i64>) =
            sqlx::query_as("SELECT icon, icon_byte_size FROM project_groups WHERE id = 'group-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            group,
            ("asset:project-icons/kept.png".to_string(), Some(12))
        );
        let project: (String, Option<i64>) =
            sqlx::query_as("SELECT icon, icon_byte_size FROM projects WHERE id = 'project-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(project, (MISSING_ICON_FALLBACK.to_string(), None));
        let emojis: Vec<(String, Option<i64>)> =
            sqlx::query_as("SELECT id, asset_byte_size FROM project_custom_emojis ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(emojis, [("emoji-kept".to_string(), Some(12))]);

        let second = convert(&pool, &vault_root).await.unwrap();
        assert_eq!(second, ConversionReport::default());
        let _ = fs::remove_dir_all(&vault_root);
    });
}

#[test]
fn conversion_assert_refuses_a_dependency_whose_id_alone_is_still_a_placeholder() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_task_dependencies (id, sync_key, blocking_task_id, blocked_task_id, dependency_type)
             VALUES ('pending:9', ?, 'task-a', 'task-b', 'blocks')",
        )
        .bind(dependency_key("task-a", "task-b").unwrap())
        .execute(&pool)
        .await
        .unwrap();
        assert!(assert_converted(&pool).await.is_err());
        convert(&pool, &std::env::temp_dir()).await.unwrap();
        assert_converted(&pool).await.unwrap();
    });
}
