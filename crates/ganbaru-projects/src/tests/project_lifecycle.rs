use super::*;

#[test]
fn project_update_rejects_blank_automation_default_ids() {
    let project = ProjectUpdate {
        id: "project-a".to_string(),
        group_id: Some("group-a".to_string()),
        name: "Project A".to_string(),
        icon: "lucide:folder".to_string(),
        color: None,
        placement: None,
        default_event_name: None,
        default_event_time_mode: "timed".to_string(),
        default_event_duration_minutes: Some(60),
        default_pomodoro_mode: "none".to_string(),
        default_pomodoro_preset_key: None,
        default_pomodoro_focus_minutes: None,
        default_pomodoro_short_break_minutes: None,
        default_pomodoro_long_break_minutes: None,
        default_pomodoro_long_break_after_focus_count: None,
        default_idle_settings_source: "global".to_string(),
        default_idle_pause_enabled: true,
        default_idle_threshold_minutes: 3,
        focus_playlist_id: Some(" ".to_string()),
        break_playlist_id: None,
        work_environment_id: None,
        blocker_ruleset_id: None,
        music_assignments: None,
        music_assignments_updated_at: None,
    };

    assert_eq!(
        validate_project_update(&project),
        Err("focus_playlist_id cannot be empty".to_string())
    );
}

#[test]
fn project_update_rejects_blank_default_event_name() {
    let project = ProjectUpdate {
        id: "project-a".to_string(),
        group_id: Some("group-a".to_string()),
        name: "Project A".to_string(),
        icon: "lucide:folder".to_string(),
        color: None,
        placement: None,
        default_event_name: Some(" ".to_string()),
        default_event_time_mode: "timed".to_string(),
        default_event_duration_minutes: Some(60),
        default_pomodoro_mode: "none".to_string(),
        default_pomodoro_preset_key: None,
        default_pomodoro_focus_minutes: None,
        default_pomodoro_short_break_minutes: None,
        default_pomodoro_long_break_minutes: None,
        default_pomodoro_long_break_after_focus_count: None,
        default_idle_settings_source: "global".to_string(),
        default_idle_pause_enabled: true,
        default_idle_threshold_minutes: 3,
        focus_playlist_id: None,
        break_playlist_id: None,
        work_environment_id: None,
        blocker_ruleset_id: None,
        music_assignments: None,
        music_assignments_updated_at: None,
    };

    assert_eq!(
        validate_project_update(&project),
        Err("default_event_name cannot be empty".to_string())
    );
}

fn valid_project_update() -> ProjectUpdate {
    ProjectUpdate {
        id: "project-a".to_string(),
        group_id: Some("group-a".to_string()),
        name: "Renamed project".to_string(),
        icon: "lucide:folder".to_string(),
        color: None,
        placement: None,
        default_event_name: None,
        default_event_time_mode: "timed".to_string(),
        default_event_duration_minutes: Some(60),
        default_pomodoro_mode: "none".to_string(),
        default_pomodoro_preset_key: None,
        default_pomodoro_focus_minutes: None,
        default_pomodoro_short_break_minutes: None,
        default_pomodoro_long_break_minutes: None,
        default_pomodoro_long_break_after_focus_count: None,
        default_idle_settings_source: "global".to_string(),
        default_idle_pause_enabled: true,
        default_idle_threshold_minutes: 3,
        focus_playlist_id: None,
        break_playlist_id: None,
        work_environment_id: None,
        blocker_ruleset_id: None,
        music_assignments: Some(vec![
            ganbaru_music::assignments::MusicContextAssignmentDraft {
                phase: ganbaru_music::assignments::MusicActivityPhase::Focus,
                behavior: ganbaru_music::assignments::MusicAssignmentBehavior::PlayAutomatically,
                playlist_id: Some("playlist-a".to_string()),
                soundscape_preset_id: None,
                soundscape_behavior: ganbaru_music::assignments::MusicSoundscapeBehavior::Inherit,
                provenance_kind:
                    ganbaru_music::assignments::MusicAssignmentProvenanceKind::Explicit,
                provenance_id: None,
            },
        ]),
        music_assignments_updated_at: Some(1_700_000_000_000),
    }
}

#[test]
fn project_settings_and_soundtrack_defaults_commit_atomically() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, order_key)
         VALUES ('group-a', 'Group A', 'lucide:folder', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, order_key)
         VALUES ('project-a', 'group-a', 'Original project', 'lucide:folder', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
        "INSERT INTO music_playlists (id, name, icon, shuffle_enabled, repeat_mode, order_key, created_at_ms, updated_at_ms)
         VALUES ('playlist-a', 'Focus', 'lucide:laptop', 1, 'off', 'a0', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

        super::super::project_commands::update_project(
            &pool,
            &std::env::temp_dir(),
            valid_project_update(),
        )
        .await
        .unwrap();

        let name: String = sqlx::query_scalar("SELECT name FROM projects WHERE id = 'project-a'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let assignment: (String, String) = sqlx::query_as(
            "SELECT behavior, playlist_id FROM music_context_assignments
         WHERE owner_kind = 'project-default' AND owner_id = 'project-a' AND phase = 'focus'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(name, "Renamed project");
        assert_eq!(
            assignment,
            ("play-automatically".to_string(), "playlist-a".to_string())
        );

        let mut invalid = valid_project_update();
        invalid.name = "Must roll back".to_string();
        invalid.music_assignments = Some(vec![
            invalid.music_assignments.as_ref().unwrap()[0].clone(),
            invalid.music_assignments.as_ref().unwrap()[0].clone(),
        ]);
        assert!(
            super::super::project_commands::update_project(&pool, &std::env::temp_dir(), invalid)
                .await
                .is_err()
        );
        let name_after_failure: String =
            sqlx::query_scalar("SELECT name FROM projects WHERE id = 'project-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(name_after_failure, "Renamed project");
    });
}

#[test]
fn renames_and_moves_refuse_a_name_taken_in_the_target_group() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, order_key) VALUES
                ('group-a', 'Group A', 'lucide:folder', 'a1'),
                ('group-b', 'Group B', 'lucide:folder', 'a2');
             INSERT INTO projects (id, group_id, name, icon, order_key) VALUES
                ('project-a', 'group-a', 'Draft', 'lucide:folder', 'a1'),
                ('project-b', 'group-a', 'Thesis', 'lucide:folder', 'a2'),
                ('project-c', 'group-b', 'draft', 'lucide:folder', 'a1'),
                ('project-d', 'group-b', 'Draft', 'lucide:folder', 'a2');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let update = |id: &str, group_id: Option<&str>, name: &str| ProjectUpdate {
            id: id.to_string(),
            group_id: group_id.map(str::to_string),
            name: name.to_string(),
            music_assignments: None,
            music_assignments_updated_at: None,
            ..valid_project_update()
        };
        let vault_root = std::env::temp_dir();
        let update_project = |project: ProjectUpdate| {
            super::super::project_commands::update_project(&pool, &vault_root, project)
        };
        let taken = |result: Result<ProjectsMutationRows, String>| {
            result.is_err_and(|error| {
                error.starts_with(super::super::project_commands::PROJECT_NAME_TAKEN_PREFIX)
            })
        };

        assert!(taken(
            update_project(update("project-a", Some("group-a"), "THESIS")).await
        ));
        assert!(taken(
            update_project(update("project-b", Some("group-b"), "Draft")).await
        ));
        update_project(update("project-a", Some("group-a"), "draft "))
            .await
            .unwrap();
        update_project(update("project-c", Some("group-b"), "draft"))
            .await
            .unwrap();
        update_project(update("project-b", Some("group-a"), "Thesis notes"))
            .await
            .unwrap();
        let names: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM projects WHERE group_id IN ('group-a', 'group-b') ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(names, ["draft", "Thesis notes", "draft", "Draft"]);

        update_project(update("project-a", None, "Draft"))
            .await
            .unwrap();
        assert!(taken(
            update_project(update("project-c", None, "DRAFT")).await
        ));
        update_project(update("project-c", None, "Draft copy"))
            .await
            .unwrap();
        let ungrouped: Vec<(String, String)> =
            sqlx::query_as("SELECT id, name FROM projects WHERE group_id IS NULL ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            ungrouped,
            [
                ("project-a".to_string(), "Draft".to_string()),
                ("project-c".to_string(), "Draft copy".to_string()),
            ]
        );
    });
}

#[test]
fn built_in_routine_projects_stay_in_the_routine_group() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let project = ProjectUpdate {
            id: "project-routine-learning".to_string(),
            group_id: None,
            name: "Learning".to_string(),
            music_assignments: None,
            music_assignments_updated_at: None,
            ..valid_project_update()
        };

        super::super::project_commands::update_project(&pool, &std::env::temp_dir(), project)
            .await
            .unwrap();

        let group_id: Option<String> = sqlx::query_scalar(
            "SELECT group_id FROM projects WHERE id = 'project-routine-learning'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(group_id.as_deref(), Some(ROUTINE_GROUP_ID));
    });
}

#[test]
fn delete_project_removes_what_it_owns_and_unassigns_its_notes_and_events() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time, project_id)
             VALUES ('event-a', 'Event A', '2026-06-24T10:00:00Z', '2026-06-24T11:00:00Z', 'project-a');
             INSERT INTO notes_pages (id, parent_type, title, project_id)
             VALUES ('page-a', 'workspace', 'Page A', 'project-a')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mutation = crate::project_commands::delete_project(&pool, " project-a ".to_string())
            .await
            .unwrap();

        assert!(matches!(
            mutation.removals.as_slice(),
            [ProjectMutationRemoval::Project { id }] if id == "project-a"
        ));
        let count = |sql: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>(sql)
                    .fetch_one(&pool)
                    .await
                    .unwrap()
            }
        };
        assert_eq!(
            count("SELECT COUNT(*) FROM projects WHERE id = 'project-a'").await,
            0
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM project_tasks WHERE project_id = 'project-a'").await,
            0
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM project_groups WHERE id = 'group-a'").await,
            1
        );
        assert_eq!(
            count(
                "SELECT COUNT(*) FROM calendar_events WHERE id = 'event-a' AND project_id IS NULL"
            )
            .await,
            1
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM notes_pages WHERE id = 'page-a' AND project_id IS NULL")
                .await,
            1
        );
        assert_eq!(
            crate::project_commands::delete_project(&pool, "project-a".to_string())
                .await
                .err(),
            Some("project not found".to_string())
        );
    });
}

#[test]
fn delete_project_refuses_built_in_routine_projects() {
    block_on(async {
        let pool = migrated_memory_pool().await;

        let refused =
            crate::project_commands::delete_project(&pool, "project-routine-sleep".to_string())
                .await;

        assert_eq!(
            refused.err(),
            Some("built-in Routine projects cannot be deleted".to_string())
        );
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = 'project-routine-sleep'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 1);
    });
}

#[test]
fn a_project_edit_keeps_a_hidden_project_hidden() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query("UPDATE projects SET status = 'hidden' WHERE id = 'project-a'")
            .execute(&pool)
            .await
            .unwrap();
        let project = ProjectUpdate {
            music_assignments: None,
            music_assignments_updated_at: None,
            ..valid_project_update()
        };

        let mutation =
            super::super::project_commands::update_project(&pool, &std::env::temp_dir(), project)
                .await
                .unwrap();

        assert_eq!(mutation.projects[0].name, "Renamed project");
        assert_eq!(mutation.projects[0].status, "hidden");
    });
}

#[test]
fn a_project_lifecycle_move_applies_only_from_the_expected_state() {
    use crate::lifecycle::{Lifecycle, ProjectLifecycleChange, set_project_lifecycle};
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let change = |expected, next| ProjectLifecycleChange {
            project_id: "project-a".to_string(),
            expected,
            next,
        };

        let archived = set_project_lifecycle(&pool, change(Lifecycle::Active, Lifecycle::Archived))
            .await
            .unwrap();
        assert_eq!(archived.projects[0].status, "archived");

        let stale =
            set_project_lifecycle(&pool, change(Lifecycle::Active, Lifecycle::Hidden)).await;
        assert_eq!(
            stale.err().as_deref(),
            Some("the project status changed since it was read")
        );
        let unchanged =
            set_project_lifecycle(&pool, change(Lifecycle::Archived, Lifecycle::Archived)).await;
        assert!(unchanged.is_err());
        let missing = set_project_lifecycle(
            &pool,
            ProjectLifecycleChange {
                project_id: "project-missing".to_string(),
                expected: Lifecycle::Active,
                next: Lifecycle::Hidden,
            },
        )
        .await;
        assert_eq!(missing.err().as_deref(), Some("project not found"));
        let status: String =
            sqlx::query_scalar("SELECT status FROM projects WHERE id = 'project-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "archived");
    });
}
