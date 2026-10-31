//! Restoring Projects rows through recovery offers between a hub and a phone.

use super::{
    DirectTransport, HUB_DEVICE, PHONE_DEVICE, Replica, block_on, converge, edit_sql, hub,
    music_texts, recovery_titles,
};
use crate::recovery::{
    RecoveredRow, RecoveryAction, RecoveryChoice, RecoveryRestorers, RestoreFuture,
};
use ganbaru_sync::Engine;
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::calendar;
use ganbaru_sync::manifest::vault::projects::{GROUPS, PROJECTS, SECTIONS, STATUSES, TASKS};
use ganbaru_sync_contracts::OrderKey;
use sqlx::SqliteConnection;
use tokio::sync::watch;

/// Restores a recovered Projects row through its domain persistence, as the app does.
fn restore_projects_row(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move {
        ganbaru_projects::recovery::restore_recovered_row(
            conn,
            row.table,
            &row.row_key,
            &row.values,
            &row.descendants,
        )
        .await
    })
}

/// Restores a recovered event through Calendar persistence, as the app does.
fn restore_calendar_event(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move {
        ganbaru_calendar::recovery::restore_recovered_event(
            conn,
            &row.row_key,
            &row.values,
            &row.descendants,
        )
        .await
    })
}

/// Restores the events of a recovered calendar through Calendar persistence, as the app does.
fn restore_calendar(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move {
        ganbaru_calendar::recovery::restore_recovered_calendar(conn, &row.descendants).await
    })
}

pub(super) fn rank(index: u32) -> String {
    OrderKey::rank(index).expect("order key").into_string()
}

/// Groups `group-a` and `group-b`; project `project-1` in `group-a` with sections `section-1`
/// and `section-2`, statuses `status-1` and `status-2`, and task `task-1` in the first of each.
fn seed_sql() -> String {
    format!(
        "INSERT INTO project_groups (id, name, order_key)
         VALUES ('group-a', 'Work', '{r0}'), ('group-b', 'Home', '{r1}');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project-1', 'group-a', 'Alpha', '{r0}');
         INSERT INTO project_sections (id, project_id, name, order_key)
         VALUES ('section-1', 'project-1', 'General', '{r0}'),
                ('section-2', 'project-1', 'Later', '{r1}');
         INSERT INTO project_statuses (id, project_id, name, category, color, order_key)
         VALUES ('status-1', 'project-1', 'To do', 'not_started', 3, '{r0}'),
                ('status-2', 'project-1', 'Doing', 'active', 4, '{r1}');
         INSERT INTO project_tasks
            (id, project_id, section_id, status_id, title, section_order_key, status_order_key)
         VALUES ('task-1', 'project-1', 'section-1', 'status-1', 'Write', '{r0}', '{r0}');",
        r0 = rank(0),
        r1 = rank(1),
    )
}

/// Removes the groups a migrated test vault starts with, outside capture. A real vault seeds
/// them as captures once, but these tests never seed, so each replica would keep its own copies
/// that the engine does not know and that a restore could pick as a fallback group.
async fn remove_unpublished_groups(replica: &Replica) {
    sqlx::raw_sql(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         DELETE FROM project_groups;
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;",
    )
    .execute(&replica.pool)
    .await
    .expect("remove unpublished groups");
}

/// A hub and a phone that both hold the seeded Projects rows.
pub(super) struct Pair {
    pub(super) hub: Replica,
    pub(super) phone: Replica,
    transport: DirectTransport,
    _changes: watch::Sender<u64>,
}

impl Pair {
    pub(super) async fn new(label: &str) -> Self {
        let mut hub_replica = Replica::new(&format!("{label}-hub"), HUB_DEVICE).await;
        let mut phone = Replica::new(&format!("{label}-phone"), PHONE_DEVICE).await;
        let (transport, changes) = hub(&hub_replica);
        remove_unpublished_groups(&hub_replica).await;
        remove_unpublished_groups(&phone).await;
        edit_sql(&mut hub_replica, seed_sql()).await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        let pair = Self {
            hub: hub_replica,
            phone,
            transport,
            _changes: changes,
        };
        assert_eq!(
            pair.texts("SELECT id FROM project_groups ORDER BY id".to_string())
                .await,
            ["group-a", "group-b"]
        );
        pair
    }

    pub(super) async fn finish(self) {
        self.hub.close().await;
        self.phone.close().await;
    }

    pub(super) async fn hub_edit(&mut self, sql: &str) {
        edit_sql(&mut self.hub, sql.to_string()).await;
    }

    pub(super) async fn phone_edit(&mut self, sql: &str) {
        edit_sql(&mut self.phone, sql.to_string()).await;
    }

    pub(super) async fn converge(&mut self) {
        converge(&mut self.hub, &mut self.phone, &self.transport).await;
    }

    /// Keys of the open recovery offers, the same on both replicas.
    pub(super) async fn offers(&self) -> Vec<String> {
        let keys = |titles: Vec<(String, String)>| {
            titles.into_iter().map(|(key, _)| key).collect::<Vec<_>>()
        };
        let on_hub = keys(recovery_titles(&self.hub).await);
        assert_eq!(on_hub, keys(recovery_titles(&self.phone).await));
        on_hub
    }

    /// Restores or discards an offer on the hub.
    pub(super) async fn close(
        &mut self,
        table: &TableSpec,
        row_key: &str,
        action: RecoveryAction,
    ) -> Result<Option<String>, String> {
        let restorers = RecoveryRestorers::new()
            .with(&GROUPS, restore_projects_row)
            .with(&PROJECTS, restore_projects_row)
            .with(&SECTIONS, restore_projects_row)
            .with(&STATUSES, restore_projects_row)
            .with(&TASKS, restore_projects_row)
            .with(&calendar::CALENDARS, restore_calendar)
            .with(&calendar::EVENTS, restore_calendar_event);
        crate::recovery::close(
            &Engine::vault(),
            &self.hub.pool,
            &self.hub.ctx,
            &mut self.hub.writer,
            &restorers,
            &RecoveryChoice {
                table: table.id,
                row_key: row_key.to_string(),
                action,
            },
            super::NOW_MS + 1,
        )
        .await
    }

    /// Restores an offer on the hub and converges, returning the new root id.
    pub(super) async fn restore(&mut self, table: &TableSpec, row_key: &str) -> String {
        let restored = self
            .close(table, row_key, RecoveryAction::Restore)
            .await
            .expect("restore")
            .expect("restored id");
        assert_ne!(restored, row_key);
        self.converge().await;
        assert!(self.offers().await.is_empty());
        restored
    }

    /// One text column of `sql`, which must read the same on both replicas.
    pub(super) async fn texts(&self, sql: String) -> Vec<String> {
        let on_hub = music_texts(&self.hub, sql.clone()).await;
        assert_eq!(on_hub, music_texts(&self.phone, sql).await);
        on_hub
    }

    /// A refused restore keeps the offer, which a discard then closes on both replicas.
    pub(super) async fn refuse_then_discard(&mut self, table: &TableSpec, row_key: &str) {
        let refused = self.close(table, row_key, RecoveryAction::Restore).await;
        assert!(refused.is_err(), "restore without a parent: {refused:?}");
        assert_eq!(self.offers().await, [row_key.to_string()]);
        assert_eq!(
            self.close(table, row_key, RecoveryAction::Discard).await,
            Ok(None)
        );
        self.converge().await;
        assert!(self.offers().await.is_empty());
    }
}

const RENAME_TASK_ON_PHONE: &str =
    "UPDATE project_tasks SET title = 'Write more' WHERE id = 'task-1'";

/// A task added to `section-2` and `status-2` on the phone.
fn add_task_on_phone() -> String {
    format!(
        "INSERT INTO project_tasks
            (id, project_id, section_id, status_id, title, section_order_key, status_order_key)
         VALUES ('task-2', 'project-1', 'section-2', 'status-2', 'Plan', '{r0}', '{r0}')",
        r0 = rank(0),
    )
}

#[test]
fn a_restored_project_group_brings_back_its_projects_and_tasks() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-group").await;
        pair.hub_edit("DELETE FROM project_groups WHERE id = 'group-a'")
            .await;
        pair.phone_edit(RENAME_TASK_ON_PHONE).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["group-a"]);

        let group = pair.restore(&GROUPS, "group-a").await;
        assert_eq!(
            pair.texts("SELECT id FROM project_groups ORDER BY order_key, id".to_string())
                .await,
            ["group-b".to_string(), group.clone()]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT task.title || '|' || section.name || '|' || status.name
                 FROM project_tasks AS task
                 JOIN projects AS project ON project.id = task.project_id
                 JOIN project_sections AS section ON section.id = task.section_id
                 JOIN project_statuses AS status ON status.id = task.status_id
                 WHERE project.group_id = '{group}' AND project.name = 'Alpha'"
            ))
            .await,
            ["Write more|General|To do"]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT folder.kind FROM project_working_folders AS folder
                 JOIN project_primary_working_folders AS primary_folder
                   ON primary_folder.working_folder_id = folder.id
                 JOIN projects AS project ON project.id = folder.project_id
                 WHERE project.group_id = '{group}'"
            ))
            .await,
            ["managed"]
        );
        pair.finish().await;
    });
}

#[test]
fn a_restored_project_returns_to_its_live_group() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-project-live").await;
        pair.hub_edit("DELETE FROM projects WHERE id = 'project-1'")
            .await;
        pair.phone_edit(RENAME_TASK_ON_PHONE).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["project-1"]);

        let project = pair.restore(&PROJECTS, "project-1").await;
        assert_eq!(
            pair.texts(format!(
                "SELECT group_id || '|' || name FROM projects WHERE id = '{project}'"
            ))
            .await,
            ["group-a|Alpha"]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT name FROM project_sections WHERE project_id = '{project}'
                 ORDER BY order_key, id"
            ))
            .await,
            ["General", "Later"]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT title FROM project_tasks WHERE project_id = '{project}'"
            ))
            .await,
            ["Write more"]
        );
        pair.finish().await;
    });
}

/// A project's Chat channel is deleted with it, so the offer carries Chat descendants. They do
/// not stop the restore, and the restored project starts without them.
#[test]
fn a_restored_project_leaves_its_deleted_chat_channel_behind() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-project-chat").await;
        pair.hub_edit(
            "INSERT INTO chat_conversations
                (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
             VALUES ('conversation-1', 'project-1', 'channel', '2026-10-24T09:00:00.000Z',
                     '2026-10-24T09:00:00.000Z', '2026-10-24T09:00:00.000Z');
             INSERT INTO chat_channels
                (id, project_id, conversation_id, name, created_at, updated_at)
             VALUES ('channel-1', 'project-1', 'conversation-1', 'design',
                     '2026-10-24T09:00:00.000Z', '2026-10-24T09:00:00.000Z');",
        )
        .await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM projects WHERE id = 'project-1'")
            .await;
        pair.phone_edit(RENAME_TASK_ON_PHONE).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["project-1"]);

        let project = pair.restore(&PROJECTS, "project-1").await;
        assert_eq!(
            pair.texts(format!("SELECT name FROM projects WHERE id = '{project}'"))
                .await,
            ["Alpha"]
        );
        assert!(
            pair.texts("SELECT id FROM chat_channels".to_string())
                .await
                .is_empty()
        );
        pair.finish().await;
    });
}

#[test]
fn a_restored_project_whose_group_is_gone_moves_to_the_first_group() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-project-orphan").await;
        pair.hub_edit("DELETE FROM projects WHERE id = 'project-1'")
            .await;
        pair.phone_edit(RENAME_TASK_ON_PHONE).await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM project_groups WHERE id = 'group-a'")
            .await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["project-1"]);

        let project = pair.restore(&PROJECTS, "project-1").await;
        assert_eq!(
            pair.texts(format!(
                "SELECT group_id FROM projects WHERE id = '{project}'"
            ))
            .await,
            ["group-b"]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT title FROM project_tasks WHERE project_id = '{project}'"
            ))
            .await,
            ["Write more"]
        );
        pair.finish().await;
    });
}

#[test]
fn a_restored_section_returns_with_its_tasks_while_its_project_exists() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-section-live").await;
        pair.hub_edit("DELETE FROM project_sections WHERE id = 'section-2'")
            .await;
        pair.phone_edit(&add_task_on_phone()).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["section-2"]);

        let section = pair.restore(&SECTIONS, "section-2").await;
        assert_eq!(
            pair.texts(
                "SELECT id FROM project_sections WHERE project_id = 'project-1'
                 ORDER BY order_key, id"
                    .to_string()
            )
            .await,
            ["section-1".to_string(), section.clone()]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT title || '|' || status_id FROM project_tasks
                 WHERE section_id = '{section}'"
            ))
            .await,
            ["Plan|status-2"]
        );
        pair.finish().await;
    });
}

#[test]
fn a_section_whose_project_is_gone_is_refused_and_can_be_discarded() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-section-orphan").await;
        pair.hub_edit("DELETE FROM project_sections WHERE id = 'section-2'")
            .await;
        pair.phone_edit(&add_task_on_phone()).await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM projects WHERE id = 'project-1'")
            .await;
        pair.converge().await;
        pair.refuse_then_discard(&SECTIONS, "section-2").await;
        pair.finish().await;
    });
}

#[test]
fn a_restored_status_returns_with_its_tasks_while_its_project_exists() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-status-live").await;
        pair.hub_edit("DELETE FROM project_statuses WHERE id = 'status-2'")
            .await;
        pair.phone_edit(&add_task_on_phone()).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["status-2"]);

        let status = pair.restore(&STATUSES, "status-2").await;
        assert_eq!(
            pair.texts(format!(
                "SELECT name || '|' || category FROM project_statuses WHERE id = '{status}'"
            ))
            .await,
            ["Doing|active"]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT title || '|' || section_id FROM project_tasks
                 WHERE status_id = '{status}'"
            ))
            .await,
            ["Plan|section-2"]
        );
        pair.finish().await;
    });
}

#[test]
fn a_status_whose_project_is_gone_is_refused_and_can_be_discarded() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-status-orphan").await;
        pair.hub_edit("DELETE FROM project_statuses WHERE id = 'status-2'")
            .await;
        pair.phone_edit(&add_task_on_phone()).await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM projects WHERE id = 'project-1'")
            .await;
        pair.converge().await;
        pair.refuse_then_discard(&STATUSES, "status-2").await;
        pair.finish().await;
    });
}

#[test]
fn a_restored_task_returns_to_its_section_and_status() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-task-live").await;
        pair.hub_edit("DELETE FROM project_tasks WHERE id = 'task-1'")
            .await;
        pair.phone_edit(RENAME_TASK_ON_PHONE).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["task-1"]);

        let task = pair.restore(&TASKS, "task-1").await;
        assert_eq!(
            pair.texts(format!(
                "SELECT title || '|' || section_id || '|' || status_id FROM project_tasks
                 WHERE id = '{task}'"
            ))
            .await,
            ["Write more|section-1|status-1"]
        );
        pair.finish().await;
    });
}

#[test]
fn a_restored_task_whose_section_is_gone_moves_to_the_first_section() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-task-section").await;
        pair.hub_edit("DELETE FROM project_tasks WHERE id = 'task-1'")
            .await;
        pair.phone_edit(RENAME_TASK_ON_PHONE).await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM project_sections WHERE id = 'section-1'")
            .await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["task-1"]);

        let task = pair.restore(&TASKS, "task-1").await;
        assert_eq!(
            pair.texts(format!(
                "SELECT title || '|' || section_id || '|' || status_id FROM project_tasks
                 WHERE id = '{task}'"
            ))
            .await,
            ["Write more|section-2|status-1"]
        );
        pair.finish().await;
    });
}

#[test]
fn a_task_whose_project_is_gone_is_refused_and_can_be_discarded() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-task-orphan").await;
        pair.hub_edit("DELETE FROM project_tasks WHERE id = 'task-1'")
            .await;
        pair.phone_edit(RENAME_TASK_ON_PHONE).await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM projects WHERE id = 'project-1'")
            .await;
        pair.converge().await;
        pair.refuse_then_discard(&TASKS, "task-1").await;
        pair.finish().await;
    });
}

/// Top-level tasks `task-2` and `task-3` in the first section and status, with `task-3` a
/// subtask of `task-2` instead when `subtask` is set.
fn add_tasks_on_hub(subtask: bool) -> String {
    let parent = if subtask { "'task-2'" } else { "NULL" };
    format!(
        "INSERT INTO project_tasks
            (id, project_id, section_id, status_id, title, section_order_key, status_order_key)
         VALUES ('task-2', 'project-1', 'section-1', 'status-1', 'Plan', '{r1}', '{r1}');
         INSERT INTO project_tasks
            (id, project_id, section_id, status_id, parent_task_id, title, section_order_key,
             status_order_key)
         VALUES ('task-3', 'project-1', 'section-1', 'status-1', {parent}, 'Draft', '{r2}',
                 '{r2}');",
        r1 = rank(1),
        r2 = rank(2),
    )
}

fn dependency_sql(blocking: &str, blocked: &str) -> String {
    let key = ganbaru_projects::keys::dependency_key(blocking, blocked).expect("dependency key");
    format!(
        "INSERT INTO project_task_dependencies
            (id, sync_key, blocking_task_id, blocked_task_id, dependency_type)
         VALUES ('{key}', '{key}', '{blocking}', '{blocked}', 'blocks');"
    )
}

const RENAME_TASK_3_ON_PHONE: &str =
    "UPDATE project_tasks SET title = 'Draft more' WHERE id = 'task-3'";

#[test]
fn a_restored_subtask_whose_parent_became_a_subtask_is_top_level() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-subtask-depth").await;
        pair.hub_edit(&add_tasks_on_hub(true)).await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM project_tasks WHERE id = 'task-3'")
            .await;
        pair.phone_edit(RENAME_TASK_3_ON_PHONE).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["task-3"]);
        pair.hub_edit("UPDATE project_tasks SET parent_task_id = 'task-1' WHERE id = 'task-2'")
            .await;
        pair.converge().await;

        let task = pair.restore(&TASKS, "task-3").await;
        assert_eq!(
            pair.texts(
                "SELECT id || '>' || coalesce(parent_task_id, '-') FROM project_tasks
                 WHERE id IN ('task-1', 'task-2') ORDER BY id"
                    .to_string()
            )
            .await,
            ["task-1>-", "task-2>task-1"]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT title || '|' || coalesce(parent_task_id, '-') FROM project_tasks
                 WHERE id = '{task}'"
            ))
            .await,
            ["Draft more|-"]
        );
        pair.finish().await;
    });
}

#[test]
fn a_restored_dependency_that_would_close_a_cycle_is_left_out() {
    block_on(async {
        let mut pair = Pair::new("projects-recovery-dependency-cycle").await;
        pair.hub_edit(&format!(
            "{}{}{}",
            add_tasks_on_hub(false),
            dependency_sql("task-3", "task-1"),
            dependency_sql("task-2", "task-3"),
        ))
        .await;
        pair.converge().await;
        pair.hub_edit("DELETE FROM project_tasks WHERE id = 'task-3'")
            .await;
        pair.phone_edit(RENAME_TASK_3_ON_PHONE).await;
        pair.converge().await;
        assert_eq!(pair.offers().await, ["task-3"]);
        pair.hub_edit(&dependency_sql("task-1", "task-2")).await;
        pair.converge().await;

        let task = pair.restore(&TASKS, "task-3").await;
        let edges = pair
            .texts(
                "SELECT blocking_task_id || '>' || blocked_task_id FROM project_task_dependencies
                 ORDER BY 1"
                    .to_string(),
            )
            .await;
        // Either restored edge alone closes no cycle with task-1 > task-2; both together would.
        assert_eq!(edges.len(), 2, "{edges:?}");
        assert!(edges.contains(&"task-1>task-2".to_string()), "{edges:?}");
        assert!(
            edges.contains(&format!("{task}>task-1")) || edges.contains(&format!("task-2>{task}")),
            "{edges:?}"
        );
        pair.finish().await;
    });
}
