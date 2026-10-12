use super::helpers::{assert_plan_uses, migrated_memory_pool, query_plan};

#[test]
fn hot_domain_queries_use_expected_indexes() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let cases = [
            (
                "project task summaries",
                "SELECT status_id, COUNT(*) FROM project_tasks WHERE project_id = 'project-1' GROUP BY status_id",
                "idx_project_tasks_project_status",
            ),
            (
                "project dependency relationships",
                "SELECT * FROM project_task_dependencies WHERE blocked_task_id = 'task-1'",
                "idx_project_task_dependencies_blocked",
            ),
            (
                "Notes sidebar",
                "SELECT id FROM notes_pages WHERE in_trash = 0 AND archived = 0 ORDER BY last_edited_time DESC, title ASC LIMIT 50",
                "idx_notes_pages_active",
            ),
            (
                "Notes block children",
                "SELECT id FROM notes_blocks WHERE parent_block_id = 'block-1' AND in_trash = 0 ORDER BY sort_order, id",
                "idx_notes_blocks_parent_block",
            ),
            (
                "Music session checkpoint",
                "SELECT device_id FROM music_session_checkpoints WHERE device_id = 'device:a'",
                "sqlite_autoindex_music_session_checkpoints_1",
            ),
            (
                "Distractions usage window",
                "SELECT id FROM distractions_usage_samples WHERE local_date >= '2026-07-01' AND local_date <= '2026-07-11' ORDER BY local_date, source_type, source_key",
                "idx_distractions_usage_samples_date_source",
            ),
            (
                "open Pomodoro run",
                "SELECT id FROM pomodoro_runs WHERE ended_at IS NULL LIMIT 1",
                "idx_pomodoro_runs_open",
            ),
            (
                "active Pomodoro segment",
                "SELECT id FROM pomodoro_segments WHERE status = 'active' LIMIT 1",
                "idx_pomodoro_segments_single_active",
            ),
            (
                "active Quick notes",
                "SELECT id FROM quick_notes WHERE archived = 0 AND trashed_at IS NULL ORDER BY pinned DESC, order_key ASC, id ASC LIMIT 60",
                "idx_quick_notes_active",
            ),
            (
                "tagged Quick notes",
                "SELECT id FROM quick_notes WHERE tag_id = 'tag-1' AND archived = 0 AND trashed_at IS NULL ORDER BY pinned DESC, order_key ASC, id ASC LIMIT 60",
                "idx_quick_notes_tag_active",
            ),
        ];
        for (name, sql, expected) in cases {
            let plan = query_plan(&pool, sql).await;
            assert_plan_uses(&plan, expected);
            assert!(
                !plan.contains("USE TEMP B-TREE"),
                "{name} uses a temporary sort:\n{plan}"
            );
        }
    });
}
