use super::*;

#[test]
fn project_update_rejects_blank_automation_default_ids() {
    let project = ProjectUpdate {
        id: "project-a".to_string(),
        group_id: "group-a".to_string(),
        name: "Project A".to_string(),
        icon: "lucide:folder".to_string(),
        color: None,
        sort_order: 100,
        status: "active".to_string(),
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
        group_id: "group-a".to_string(),
        name: "Project A".to_string(),
        icon: "lucide:folder".to_string(),
        color: None,
        sort_order: 100,
        status: "active".to_string(),
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
        group_id: "group-a".to_string(),
        name: "Renamed project".to_string(),
        icon: "lucide:folder".to_string(),
        color: None,
        sort_order: 100,
        status: "active".to_string(),
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
                soundscape_id: None,
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
            "INSERT INTO project_groups (id, name, icon, sort_order)
         VALUES ('group-a', 'Group A', 'lucide:folder', 100)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, sort_order)
         VALUES ('project-a', 'group-a', 'Original project', 'lucide:folder', 100)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
        "INSERT INTO music_playlists (id, name, icon, shuffle_enabled, repeat_mode, created_at_ms, updated_at_ms)
         VALUES ('playlist-a', 'Focus', 'lucide:laptop', 1, 'off', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

        super::super::project_commands::update_project(&pool, valid_project_update())
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
            super::super::project_commands::update_project(&pool, invalid)
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
