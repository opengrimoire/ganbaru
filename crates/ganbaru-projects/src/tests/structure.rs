use super::*;
use crate::mutations::{delete_tag_with_history, delete_unused_priority, delete_unused_status};
use crate::structure_commands::{delete_group, reorder_group_projects, reorder_groups};

#[test]
fn delete_group_ungroups_its_projects_and_keeps_everything_they_own() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time, project_id)
             VALUES ('event-a', 'Event A', '2026-06-24T10:00:00Z', '2026-06-24T11:00:00Z', 'project-a')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_task_event_links (task_id, event_id, sync_key)
             VALUES ('task-a', 'event-a', 'link-a')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mutation = delete_group(&pool, " group-a ".to_string()).await.unwrap();

        assert!(matches!(
            mutation.removals.as_slice(),
            [ProjectMutationRemoval::Group { id }] if id == "group-a"
        ));
        let returned: Vec<(&str, Option<&str>)> = mutation
            .projects
            .iter()
            .map(|project| (project.id.as_str(), project.group_id.as_deref()))
            .collect();
        assert_eq!(returned, [("project-a", None)]);
        assert_eq!(
            delete_group(&pool, "group-a".to_string()).await.err(),
            Some("project group not found".to_string())
        );
        let group_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_groups WHERE id = 'group-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let project_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = 'project-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let task_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_tasks WHERE project_id = 'project-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let link_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_task_event_links WHERE event_id = 'event-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let event_project_id: Option<String> =
            sqlx::query_scalar("SELECT project_id FROM calendar_events WHERE id = 'event-a'")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(group_count, 0);
        assert_eq!(project_count, 1);
        assert_eq!(task_count, 3);
        assert_eq!(link_count, 1);
        assert_eq!(event_project_id.as_deref(), Some("project-a"));
    });
}

#[test]
fn delete_status_soft_deletes_an_empty_status_and_keeps_its_row() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name, category, order_key, terminal)
             VALUES ('status-empty', 'project-a', 'Later', 'active', 'a2', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        delete_unused_status(&mut tx, "status-empty").await.unwrap();
        tx.commit().await.unwrap();

        let status_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_statuses WHERE id = 'status-empty' AND deleted_at IS NOT NULL")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(status_count, 1);
        let mut tx = pool.begin().await.unwrap();
        assert!(delete_unused_status(&mut tx, "status-empty").await.is_err());
    });
}

#[test]
fn delete_status_rejects_status_with_tasks() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name, category, order_key, terminal)
             VALUES ('status-empty', 'project-a', 'Later', 'active', 'a2', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = delete_unused_status(&mut tx, "status-a").await;

        assert_eq!(
            result,
            Err("move or delete tasks before deleting this task status".to_string())
        );
    });
}

#[test]
fn delete_status_rejects_last_project_status() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, order_key)
             VALUES ('group-empty', 'Group Empty', 'lucide:folder', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, order_key)
             VALUES ('project-empty', 'group-empty', 'Project Empty', 'lucide:folder', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name, category, order_key, terminal)
             VALUES ('status-empty', 'project-empty', 'To do', 'not_started', 'a1', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = delete_unused_status(&mut tx, "status-empty").await;

        assert_eq!(
            result,
            Err("project must keep at least one task status".to_string())
        );
    });
}

async fn insert_priorities(pool: &SqlitePool, rows: &[(&str, Option<&str>)]) {
    for (index, (priority_id, deleted_at)) in rows.iter().enumerate() {
        sqlx::query(
            "INSERT INTO project_priorities
                (id, project_id, name, color, order_key, deleted_at, sync_key)
             VALUES (?1, 'project-a', ?1, 1, ?2, ?3, 'priority-key-' || ?1)",
        )
        .bind(priority_id)
        .bind(format!("a{index}"))
        .bind(deleted_at)
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn priority_deleted_at(pool: &SqlitePool, priority_id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT deleted_at FROM project_priorities WHERE id = ?")
        .bind(priority_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn delete_priority_rejects_priority_used_by_a_task() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        insert_priorities(&pool, &[("priority-high", None), ("priority-low", None)]).await;
        sqlx::query("UPDATE project_tasks SET priority = 'priority-high' WHERE id = 'task-a'")
            .execute(&pool)
            .await
            .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = delete_unused_priority(&mut tx, "project-a", "priority-high").await;
        drop(tx);

        assert_eq!(
            result,
            Err("move or delete tasks before deleting this task priority".to_string())
        );
        assert_eq!(priority_deleted_at(&pool, "priority-high").await, None);
    });
}

#[test]
fn delete_priority_soft_deletes_an_unused_priority_and_keeps_the_last_live_one() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        insert_priorities(
            &pool,
            &[
                ("priority-high", None),
                ("priority-low", None),
                ("priority-retired", Some("2026-10-23T00:00:00.000Z")),
            ],
        )
        .await;

        let mut tx = pool.begin().await.unwrap();
        delete_unused_priority(&mut tx, "project-a", "priority-low")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(priority_deleted_at(&pool, "priority-low").await.is_some());

        let mut tx = pool.begin().await.unwrap();
        let last = delete_unused_priority(&mut tx, "project-a", "priority-high").await;
        let retired = delete_unused_priority(&mut tx, "project-a", "priority-retired").await;
        drop(tx);

        assert_eq!(
            last,
            Err("project must keep at least one task priority".to_string())
        );
        assert_eq!(retired, Err("project priority not found".to_string()));
        assert_eq!(priority_deleted_at(&pool, "priority-high").await, None);
    });
}

#[test]
fn delete_tag_removes_links_and_records_task_history() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_tags (id, project_id, name, order_key)
             VALUES ('tag-a', 'project-a', 'Backend', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for task_id in ["task-a", "task-b"] {
            sqlx::query(
                "INSERT INTO project_task_tag_links (task_id, tag_id, sync_key)
                 VALUES (?, 'tag-a', ?)",
            )
            .bind(task_id)
            .bind(task_id)
            .execute(&pool)
            .await
            .unwrap();
        }

        let mut tx = pool.begin().await.unwrap();
        delete_tag_with_history(&mut tx, "tag-a").await.unwrap();
        tx.commit().await.unwrap();

        let tag_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_tags WHERE id = 'tag-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let link_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_task_tag_links WHERE tag_id = 'tag-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let history_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM project_task_change_events
             WHERE field_name = 'tags'
               AND old_value = 'Backend'
               AND new_value IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(tag_count, 0);
        assert_eq!(link_count, 0);
        assert_eq!(history_count, 2);
    });
}

#[test]
fn reordering_groups_requires_every_group_and_keeps_modified_times() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, order_key, updated_at)
             VALUES ('group-b', 'Group B', 'lucide:folder', 'a2', '2026-01-01T00:00:00.000Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut group_ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM project_groups ORDER BY order_key, name")
                .fetch_all(&pool)
                .await
                .unwrap();
        let reorder = |ids: &[String]| ProjectGroupReorder {
            group_ids: ids.to_vec(),
        };

        let duplicated = [group_ids.clone(), vec!["group-a".to_string()]].concat();
        let unknown = [group_ids.clone(), vec!["other".to_string()]].concat();
        for incomplete in [&group_ids[1..], &duplicated[..], &unknown[..]] {
            assert!(reorder_groups(&pool, reorder(incomplete)).await.is_err());
        }

        group_ids.reverse();
        group_ids[0] = format!(" {} ", group_ids[0]);
        let mutation = reorder_groups(&pool, reorder(&group_ids)).await.unwrap();
        let returned: Vec<&str> = mutation
            .groups
            .iter()
            .map(|group| group.id.as_str())
            .collect();
        let expected: Vec<&str> = group_ids.iter().map(|id| id.trim()).collect();
        assert_eq!(returned, expected);
        assert!(
            mutation
                .groups
                .windows(2)
                .all(|pair| (&pair[0].order_key, &pair[0].id) < (&pair[1].order_key, &pair[1].id))
        );
        let updated_at: String =
            sqlx::query_scalar("SELECT updated_at FROM project_groups WHERE id = 'group-b'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(updated_at, "2026-01-01T00:00:00.000Z");
    });
}

#[test]
fn reordering_group_projects_covers_inactive_projects_of_that_group_only() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        for (id, status) in [("project-b", "hidden"), ("project-c", "archived")] {
            sqlx::query(
                "INSERT INTO projects (id, group_id, name, icon, order_key, status, updated_at)
                 VALUES (?, 'group-a', ?, 'lucide:folder', 'a1', ?, '2026-01-01T00:00:00.000Z')",
            )
            .bind(id)
            .bind(id)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        }
        let reorder = |group_id: Option<&str>, ids: &[&str]| GroupProjectReorder {
            group_id: group_id.map(str::to_string),
            project_ids: ids.iter().map(|id| id.to_string()).collect(),
        };

        assert!(
            reorder_group_projects(&pool, reorder(Some("group-a"), &["project-c", "project-a"]))
                .await
                .is_err()
        );
        assert!(
            reorder_group_projects(&pool, reorder(Some("missing"), &[]))
                .await
                .is_err()
        );
        assert!(
            reorder_group_projects(&pool, reorder(Some(" "), &[]))
                .await
                .is_err()
        );
        assert!(
            reorder_group_projects(&pool, reorder(None, &["project-a"]))
                .await
                .is_err()
        );

        let mutation = reorder_group_projects(
            &pool,
            reorder(Some("group-a"), &["project-c", "project-a", "project-b"]),
        )
        .await
        .unwrap();
        let returned: Vec<&str> = mutation
            .projects
            .iter()
            .map(|project| project.id.as_str())
            .collect();
        assert_eq!(returned, ["project-c", "project-a", "project-b"]);
        assert!(
            mutation
                .projects
                .windows(2)
                .all(|pair| (&pair[0].order_key, &pair[0].id) < (&pair[1].order_key, &pair[1].id))
        );
        let updated_at: String =
            sqlx::query_scalar("SELECT updated_at FROM projects WHERE id = 'project-c'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(updated_at, "2026-01-01T00:00:00.000Z");
    });
}

#[test]
fn reordering_ungrouped_projects_leaves_grouped_ones_alone() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        for (id, order_key) in [("loose-a", "a1"), ("loose-b", "a2")] {
            sqlx::query(
                "INSERT INTO projects (id, group_id, name, icon, order_key)
                 VALUES (?, NULL, ?, 'lucide:folder', ?)",
            )
            .bind(id)
            .bind(id)
            .bind(order_key)
            .execute(&pool)
            .await
            .unwrap();
        }
        let grouped_key: String =
            sqlx::query_scalar("SELECT order_key FROM projects WHERE id = 'project-a'")
                .fetch_one(&pool)
                .await
                .unwrap();

        let mutation = reorder_group_projects(
            &pool,
            GroupProjectReorder {
                group_id: None,
                project_ids: vec!["loose-b".to_string(), "loose-a".to_string()],
            },
        )
        .await
        .unwrap();

        let returned: Vec<(&str, Option<&str>)> = mutation
            .projects
            .iter()
            .map(|project| (project.id.as_str(), project.group_id.as_deref()))
            .collect();
        assert_eq!(returned, [("loose-b", None), ("loose-a", None)]);
        let grouped_key_after: String =
            sqlx::query_scalar("SELECT order_key FROM projects WHERE id = 'project-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(grouped_key_after, grouped_key);
    });
}

async fn section_lifecycle_columns(pool: &SqlitePool) -> (Option<String>, Option<String>) {
    sqlx::query_as("SELECT hidden_at, archived_at FROM project_sections WHERE id = 'section-a'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn a_section_edit_keeps_a_hidden_section_hidden() {
    use crate::lifecycle::{Lifecycle, SectionLifecycleChange, set_section_lifecycle};
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        set_section_lifecycle(
            &pool,
            SectionLifecycleChange {
                section_id: "section-a".to_string(),
                expected: Lifecycle::Active,
                next: Lifecycle::Hidden,
            },
        )
        .await
        .unwrap();
        let edit: crate::models::ProjectSectionUpdate = serde_json::from_value(serde_json::json!({
            "id": "section-a",
            "name": "Renamed",
            "collapsed": true,
            "hiddenAt": null,
            "archivedAt": null,
        }))
        .unwrap();

        let mutation = crate::structure_commands::update_section(&pool, edit)
            .await
            .unwrap();

        assert_eq!(mutation.sections[0].name, "Renamed");
        let (hidden_at, archived_at) = section_lifecycle_columns(&pool).await;
        assert!(hidden_at.is_some());
        assert!(archived_at.is_none());
    });
}

#[test]
fn a_section_lifecycle_move_applies_only_from_the_expected_state() {
    use crate::lifecycle::{Lifecycle, SectionLifecycleChange, set_section_lifecycle};
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let change = |expected, next| SectionLifecycleChange {
            section_id: "section-a".to_string(),
            expected,
            next,
        };

        set_section_lifecycle(&pool, change(Lifecycle::Active, Lifecycle::Hidden))
            .await
            .unwrap();
        let (hidden_at, archived_at) = section_lifecycle_columns(&pool).await;
        assert!(hidden_at.is_some() && archived_at.is_none());

        let stale =
            set_section_lifecycle(&pool, change(Lifecycle::Active, Lifecycle::Archived)).await;
        assert_eq!(
            stale.err().as_deref(),
            Some("the section lifecycle changed since it was read")
        );
        assert_eq!(section_lifecycle_columns(&pool).await, (hidden_at, None));

        set_section_lifecycle(&pool, change(Lifecycle::Hidden, Lifecycle::Archived))
            .await
            .unwrap();
        let (hidden_at, archived_at) = section_lifecycle_columns(&pool).await;
        assert!(hidden_at.is_none() && archived_at.is_some());

        set_section_lifecycle(&pool, change(Lifecycle::Archived, Lifecycle::Active))
            .await
            .unwrap();
        assert_eq!(section_lifecycle_columns(&pool).await, (None, None));

        let missing = set_section_lifecycle(
            &pool,
            SectionLifecycleChange {
                section_id: "section-missing".to_string(),
                expected: Lifecycle::Active,
                next: Lifecycle::Hidden,
            },
        )
        .await;
        assert_eq!(missing.err().as_deref(), Some("project section not found"));
    });
}

#[test]
fn a_section_cannot_be_hidden_and_archived_at_once() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let both = sqlx::query(
            "UPDATE project_sections
             SET hidden_at = '2026-11-22T00:00:00.000Z', archived_at = '2026-11-22T00:00:00.000Z'
             WHERE id = 'section-a'",
        )
        .execute(&pool)
        .await;
        assert!(both.is_err());
    });
}
