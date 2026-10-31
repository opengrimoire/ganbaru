use super::*;
use crate::project_commands::create_project;
use std::fs;
use std::path::PathBuf;

fn project_create(id: &str) -> ProjectCreate {
    ProjectCreate {
        id: id.to_string(),
        group_id: ROUTINE_GROUP_ID.to_string(),
        name: "Created test".to_string(),
        icon: "lucide:folder".to_string(),
        color: None,
        placement: crate::order::OrderPlacement::End,
        default_event_name: None,
        default_event_time_mode: "timed".to_string(),
        default_event_duration_minutes: None,
        default_pomodoro_mode: "none".to_string(),
        default_pomodoro_preset_key: None,
        default_pomodoro_focus_minutes: None,
        default_pomodoro_short_break_minutes: None,
        default_pomodoro_long_break_minutes: None,
        default_pomodoro_long_break_after_focus_count: None,
        default_idle_settings_source: "global".to_string(),
        default_idle_pause_enabled: true,
        default_idle_threshold_minutes: 5,
    }
}

fn empty_vault_root(name: &str) -> PathBuf {
    let vault_root = std::env::temp_dir().join(format!("ganbaru-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&vault_root);
    fs::create_dir_all(&vault_root).unwrap();
    vault_root
}

#[test]
fn project_creation_persists_and_creates_its_managed_working_folder() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let vault_root = empty_vault_root("project-working-folder");

        let mutation = create_project(&pool, &vault_root, project_create("project-created-test"))
            .await
            .unwrap();
        let view_types: Vec<&str> = mutation
            .views
            .iter()
            .map(|view| view.view_type.as_str())
            .collect();
        assert_eq!(view_types, ["dashboard", "list", "notes", "chat"]);

        let folder: (String, String, String, String, Option<String>) = sqlx::query_as(
            "SELECT id, project_id, display_name, kind, managed_relative_path
             FROM project_working_folders
             WHERE project_id = 'project-created-test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            folder,
            (
                "working-folder:project-created-test".to_string(),
                "project-created-test".to_string(),
                "Created test".to_string(),
                "managed".to_string(),
                Some("projects/project-created-test".to_string()),
            )
        );
        let primary: (String, i64) = sqlx::query_as(
            "SELECT working_folder_id, revision FROM project_primary_working_folders
             WHERE project_id = 'project-created-test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            primary,
            ("working-folder:project-created-test".to_string(), 1)
        );
        let chat_rows: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM chat_channels WHERE project_id = 'project-created-test')
                  + (SELECT count(*) FROM chat_execution_environments
                     WHERE working_folder_id = 'working-folder:project-created-test')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(chat_rows, 0);
        let directory = vault_root.join("projects/project-created-test");
        assert!(directory.is_dir());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::remove_dir_all(vault_root).unwrap();
    });
}

#[test]
fn project_creation_refuses_an_id_too_long_for_its_working_folder_id() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let vault_root = empty_vault_root("project-working-folder-long-id");
        let project_id = "p".repeat(50);

        let error = create_project(&pool, &vault_root, project_create(&project_id))
            .await
            .err()
            .expect("an overlong project id is refused");

        assert!(error.contains("working folder"), "{error}");
        let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM projects WHERE id = ?")
            .bind(&project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, 0);
        assert!(!vault_root.join("projects").join(&project_id).exists());
        fs::remove_dir_all(vault_root).unwrap();
    });
}
