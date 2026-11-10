use super::*;
use crate::project_commands::create_project;
use sqlx::SqlitePool;
use std::fs;
use std::path::PathBuf;

fn project_create(id: &str) -> ProjectCreate {
    ProjectCreate {
        id: id.to_string(),
        group_id: Some(ROUTINE_GROUP_ID.to_string()),
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
                Some("projects/Created test".to_string()),
            )
        );
        assert_eq!(
            location(&pool, "working-folder:project-created-test").await,
            Some(("Created test".to_string(), "created test".to_string()))
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
        let directory = vault_root.join("projects/Created test");
        assert!(directory.is_dir());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::remove_dir_all(vault_root).unwrap();
    });
}

async fn location(pool: &SqlitePool, working_folder_id: &str) -> Option<(String, String)> {
    sqlx::query_as(
        "SELECT folder_name, folder_key FROM project_managed_folder_locations
         WHERE working_folder_id = ?",
    )
    .bind(working_folder_id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

/// Inserts the groups `group-a` and `group-b`.
async fn insert_groups(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO project_groups (id, name, icon, order_key) VALUES
            ('group-a', 'Group A', 'lucide:folder', 'a1'),
            ('group-b', 'Group B', 'lucide:folder', 'a2')",
    )
    .execute(pool)
    .await
    .unwrap();
}

fn named_project(id: &str, group_id: &str, name: &str) -> ProjectCreate {
    ProjectCreate {
        group_id: Some(group_id.to_string()),
        name: name.to_string(),
        ..project_create(id)
    }
}

fn ungrouped_project(id: &str, name: &str) -> ProjectCreate {
    ProjectCreate {
        group_id: None,
        name: name.to_string(),
        ..project_create(id)
    }
}

#[test]
fn seeded_routine_folders_are_named_after_their_projects() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let paths: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, managed_relative_path FROM project_working_folders
             WHERE kind = 'managed' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(paths.len(), 12);
        for (id, path) in paths {
            let path = path.unwrap();
            let name = path.strip_prefix("projects/").unwrap();
            assert!(!name.starts_with("project-"), "{id} keeps {path}");
            assert_eq!(
                location(&pool, &id).await,
                Some((name.to_string(), name.to_lowercase()))
            );
        }
        assert_eq!(
            location(&pool, "working-folder-routine-eat").await,
            Some(("Eating".to_string(), "eating".to_string()))
        );
    });
}

#[test]
fn project_folders_skip_claimed_names_and_entries_left_in_projects() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let vault_root = empty_vault_root("project-folder-collisions");
        insert_groups(&pool).await;
        fs::create_dir_all(vault_root.join("projects").join("leftover")).unwrap();

        for (id, group, name) in [
            ("project-a", "group-a", "Thesis"),
            ("project-b", "group-b", "thesis"),
            ("project-c", "group-a", "Leftover"),
            ("project-d", "group-a", "Learning"),
            ("project-e", "group-a", "Work: Q3/Q4?"),
        ] {
            create_project(&pool, &vault_root, named_project(id, group, name))
                .await
                .unwrap();
        }

        for (id, folder) in [
            ("project-a", "Thesis"),
            ("project-b", "thesis 2"),
            ("project-c", "Leftover 2"),
            ("project-d", "Learning 2"),
            ("project-e", "Work Q3 Q4"),
        ] {
            let working_folder = format!("working-folder:{id}");
            let path: String = sqlx::query_scalar(
                "SELECT managed_relative_path FROM project_working_folders WHERE id = ?",
            )
            .bind(&working_folder)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(path, format!("projects/{folder}"));
            assert_eq!(
                location(&pool, &working_folder).await.map(|(name, _)| name),
                Some(folder.to_string())
            );
            assert!(
                vault_root.join("projects").join(folder).is_dir(),
                "{folder}"
            );
        }
        fs::remove_dir_all(vault_root).unwrap();
    });
}

#[test]
fn a_replicated_folder_claims_its_own_location_once() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let vault_root = empty_vault_root("project-folder-claims");
        insert_groups(&pool).await;
        create_project(
            &pool,
            &vault_root,
            named_project("project-local", "group-a", "Shared"),
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, order_key)
             VALUES ('project-remote', 'group-a', 'Shared', 'lucide:folder', 'a5')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_working_folders
                (id, project_id, display_name, kind, managed_relative_path, sort_order)
             VALUES ('working-folder:project-remote', 'project-remote', 'Shared', 'managed',
                     'projects/Shared', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(location(&pool, "working-folder:project-remote").await, None);

        let claimed = crate::managed_folders::claim_managed_folder(
            &pool,
            &vault_root,
            "working-folder:project-remote",
        )
        .await
        .unwrap();
        assert_eq!(claimed, "Shared 2");
        sqlx::query("UPDATE projects SET name = 'Renamed' WHERE id = 'project-remote'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            crate::managed_folders::claim_managed_folder(
                &pool,
                &vault_root,
                "working-folder:project-remote",
            )
            .await
            .unwrap(),
            "Shared 2"
        );
        let stored_hint: String = sqlx::query_scalar(
            "SELECT managed_relative_path FROM project_working_folders
             WHERE id = 'working-folder:project-remote'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored_hint, "projects/Shared");
        let error = crate::managed_folders::claim_managed_folder(
            &pool,
            &vault_root,
            "working-folder:missing",
        )
        .await
        .unwrap_err();
        assert!(error.contains("not found"), "{error}");
        fs::remove_dir_all(vault_root).unwrap();
    });
}

#[test]
fn project_names_are_unique_within_a_group_ignoring_case() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let vault_root = empty_vault_root("project-name-uniqueness");
        insert_groups(&pool).await;
        create_project(
            &pool,
            &vault_root,
            named_project("project-a", "group-a", "Thesis"),
        )
        .await
        .unwrap();
        create_project(
            &pool,
            &vault_root,
            named_project("project-b", "group-b", "Thesis"),
        )
        .await
        .unwrap();

        let error = create_project(
            &pool,
            &vault_root,
            named_project("project-c", "group-a", "  thesis "),
        )
        .await
        .err()
        .expect("a name taken in the group is refused");
        assert!(
            error.starts_with(crate::project_commands::PROJECT_NAME_TAKEN_PREFIX),
            "{error}"
        );
        let stored: i64 =
            sqlx::query_scalar("SELECT count(*) FROM projects WHERE id = 'project-c'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, 0);
        let routine = create_project(
            &pool,
            &vault_root,
            named_project("project-d", ROUTINE_GROUP_ID, "LEARNING"),
        )
        .await
        .err()
        .expect("a Routine project name is taken in its group");
        assert!(
            routine.starts_with(crate::project_commands::PROJECT_NAME_TAKEN_PREFIX),
            "{routine}"
        );

        create_project(&pool, &vault_root, ungrouped_project("project-e", "Thesis"))
            .await
            .expect("projects outside every group form a name scope of their own");
        let ungrouped =
            create_project(&pool, &vault_root, ungrouped_project("project-f", "THESIS"))
                .await
                .err()
                .expect("a name taken outside every group is refused there");
        assert!(
            ungrouped.starts_with(crate::project_commands::PROJECT_NAME_TAKEN_PREFIX),
            "{ungrouped}"
        );
        let group_id: Option<String> =
            sqlx::query_scalar("SELECT group_id FROM projects WHERE id = 'project-e'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(group_id, None);
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
