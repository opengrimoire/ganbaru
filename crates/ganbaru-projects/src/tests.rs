use super::*;
use ganbaru_db::run_migrations;
use sqlx::SqlitePool;
use std::future::Future;

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create projects test runtime")
        .block_on(future)
}

pub(super) async fn migrated_memory_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

pub(super) async fn insert_project_graph_fixture(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO project_groups (id, name, icon, order_key)
         VALUES ('group-a', 'Group A', 'lucide:folder', 'a1')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, group_id, name, icon, order_key)
         VALUES ('project-a', 'group-a', 'Project A', 'lucide:folder', 'a1')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_sections (id, project_id, name, order_key)
         VALUES ('section-a', 'project-a', 'General', 'a1')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_statuses (id, project_id, name, category, order_key, terminal)
         VALUES ('status-a', 'project-a', 'To do', 'not_started', 'a1', 0)",
    )
    .execute(pool)
    .await
    .unwrap();
    for (task_id, order_key) in [("task-a", "a1"), ("task-b", "a2"), ("task-c", "a3")] {
        sqlx::query(
            "INSERT INTO project_tasks
                (id, project_id, section_id, status_id, title, section_order_key, status_order_key)
             VALUES (?, 'project-a', 'section-a', 'status-a', ?, ?, ?)",
        )
        .bind(task_id)
        .bind(task_id)
        .bind(order_key)
        .bind(order_key)
        .execute(pool)
        .await
        .unwrap();
    }
}

pub(super) fn task_update_from_row(task: &ProjectTaskRow) -> ProjectTaskUpdate {
    ProjectTaskUpdate {
        id: task.id.clone(),
        section_id: task.section_id.clone(),
        status_id: task.status_id.clone(),
        parent_task_id: task.parent_task_id.clone(),
        title: task.title.clone(),
        description: task.description.clone(),
        priority: task.priority.clone(),
        task_type: task.task_type.clone(),
        section_placement: None,
        status_placement: None,
        estimate_minutes: task.estimate_minutes,
        due_date: task.due_date.clone(),
        due_time: task.due_time.clone(),
        due_reminder_minutes: task.due_reminder_minutes,
        start_date: task.start_date.clone(),
        start_time: task.start_time.clone(),
        start_reminder_minutes: task.start_reminder_minutes,
        target_end_date: task.target_end_date.clone(),
        archived_at: task.archived_at.clone(),
        blocker_reason: task.blocker_reason.clone(),
        milestone: task.milestone != 0,
        change_reason: None,
        expected_revision: None,
    }
}

mod conversion;
mod custom_fields;
mod dependency_cascade;
mod emojis;
mod project_lifecycle;
mod project_working_folders;
mod relationships;
mod reorder;
mod routine;
mod structure;
mod sync_keys;
mod task_bulk;
mod task_lifecycle;
mod task_reminders;
mod task_summaries;
mod task_views;
mod views;
