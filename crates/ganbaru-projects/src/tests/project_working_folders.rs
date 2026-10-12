use super::*;
use crate::project_commands::create_project;
use std::fs;

#[test]
fn project_creation_persists_and_creates_its_managed_working_folder() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let vault_root = std::env::temp_dir().join(format!(
            "ganbaru-project-working-folder-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&vault_root);
        fs::create_dir_all(&vault_root).unwrap();
        let project = ProjectCreate {
            id: "project-created-test".to_string(),
            group_id: ROUTINE_GROUP_ID.to_string(),
            template_id: "blank".to_string(),
            name: "Created test".to_string(),
            icon: "lucide:folder".to_string(),
            color: None,
            sort_order: 900,
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
        };

        create_project(&pool, &vault_root, project).await.unwrap();

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
        let directory = vault_root.join("projects/project-created-test");
        assert!(directory.is_dir());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::remove_dir_all(vault_root).unwrap();
    });
}
