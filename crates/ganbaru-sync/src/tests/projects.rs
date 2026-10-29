//! Projects across replicas: concurrent moves settle on one whole move, a merge that nests
//! tasks too deep or closes a dependency cycle is repaired the same way everywhere, soft
//! deleted statuses keep the tasks moved into them, deleted options drop the selections that
//! named them, a value cleared while edited follows the newest existence write, concurrent
//! views stay separate, a primary folder survives its archiving, and a deletion that missed a
//! concurrent edit or insert is offered for recovery.

use ganbaru_sync_contracts::{GroupMask, derived_key};

use super::block_on;
use super::replica::{Replica, converge, device_id, pair, quoted, rank};
use crate::manifest::TableSpec;
use crate::manifest::vault::projects::{
    DEPENDENCIES, DEPENDENCIES_TABLE, GROUPS_TABLE, PRIMARY_FOLDERS_TABLE, PROJECTS,
    PROJECTS_TABLE, TASKS, VALUES, dependency_group, value_group,
};
use crate::{ChangeNotice, ConflictRow};

const CREATED: &str = "2027-01-01T00:00:00.000Z";
const EDITED: &str = "2027-01-02T00:00:00.000Z";
const GROUP: &str = "group-shared";
pub(super) const PROJECT: &str = "project-shared";
const SECTION_A: &str = "section-a";
const SECTION_B: &str = "section-b";
const OPEN: &str = "status-open";
const DONE: &str = "status-done";

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 96).await);
    }
    replicas
}

/// The derived row key of `table` for `parts`, in its key input order.
pub(super) fn key(table: &TableSpec, parts: &[&str]) -> String {
    let derived = table
        .derived_key
        .as_ref()
        .expect("table should have a derived key");
    let prefix = derived.prefix.expect("derived key should have a prefix");
    derived_key(prefix, parts)
        .expect("derived key parts should be valid")
        .into_string()
}

/// A group holding one project with two sections and an open and a done status.
pub(super) fn project_sql() -> String {
    let created = quoted(CREATED);
    format!(
        "INSERT INTO project_groups (id, name, created_at, updated_at, order_key)
         VALUES ({group}, 'Shared', {created}, {created}, {first});
         INSERT INTO projects (id, group_id, name, created_at, updated_at, order_key)
         VALUES ({project}, {group}, 'Launch', {created}, {created}, {first});
         INSERT INTO project_sections (id, project_id, name, created_at, updated_at, order_key)
         VALUES ({section_a}, {project}, 'Backlog', {created}, {created}, {first}),
                ({section_b}, {project}, 'Next', {created}, {created}, {second});
         INSERT INTO project_statuses
             (id, project_id, name, category, terminal, created_at, updated_at, order_key)
         VALUES ({open}, {project}, 'Open', 'not_started', 0, {created}, {created}, {first}),
                ({done}, {project}, 'Done', 'done', 1, {created}, {created}, {second});",
        group = quoted(GROUP),
        project = quoted(PROJECT),
        section_a = quoted(SECTION_A),
        section_b = quoted(SECTION_B),
        open = quoted(OPEN),
        done = quoted(DONE),
        first = quoted(&rank(1)),
        second = quoted(&rank(2)),
    )
}

/// SQL adding an open root task in the first section at `position`.
pub(super) fn task_sql(id: &str, title: &str, position: u32) -> String {
    let created = quoted(CREATED);
    let order = quoted(&rank(position));
    format!(
        "INSERT INTO project_tasks
             (id, project_id, section_id, status_id, title, section_order_key, status_order_key,
              created_at, updated_at)
         VALUES ({}, {}, {}, {}, {}, {order}, {order}, {created}, {created});",
        quoted(id),
        quoted(PROJECT),
        quoted(SECTION_A),
        quoted(OPEN),
        quoted(title),
    )
}

/// Every replica holding the shared project and `sql`, with operations exchanged.
async fn shared(count: usize, sql: &str) -> Vec<Replica> {
    let mut replicas = replicas(count).await;
    replicas[0].exec(&format!("{}{sql}", project_sql())).await;
    converge(&mut replicas).await;
    replicas
}

async fn texts(replica: &mut Replica, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut replica.conn)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

async fn assert_converged(replicas: &mut [Replica]) {
    let first = replicas[0].domain_dump().await;
    for replica in replicas.iter_mut().skip(1) {
        assert_eq!(
            replica.domain_dump().await,
            first,
            "replica {} diverged",
            replica.index
        );
    }
}

/// Every task of the shared project as `id>parent`, with `-` for a root task.
async fn task_parents(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT id || '>' || coalesce(parent_task_id, '-') FROM project_tasks
         WHERE project_id = 'project-shared' ORDER BY id",
    )
    .await
}

/// Open conflicts and recovery offers on `replica`.
async fn surfaced(replica: &mut Replica) -> (i64, usize) {
    let conflicts = replica
        .count("SELECT count(*) FROM sync_rows WHERE conflict_mask <> 0")
        .await;
    let offers = replica
        .engine
        .recovery_entries(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
        .len();
    (conflicts, offers)
}

#[test]
fn projects_concurrent_moves_of_one_task_settle_on_one_whole_move() {
    block_on(async {
        let mut replicas = shared(2, &task_sql("task-a", "Draft", 1)).await;
        let moves = [(SECTION_B, rank(5), OPEN), (SECTION_A, rank(9), DONE)];
        for (replica, (section, order, status)) in replicas.iter_mut().zip(&moves) {
            replica
                .exec(&format!(
                    "UPDATE project_tasks
                     SET section_id = {}, section_order_key = {}, status_id = {},
                         status_order_key = {}, updated_at = {}
                     WHERE id = 'task-a'",
                    quoted(section),
                    quoted(order),
                    quoted(status),
                    quoted(order),
                    quoted(EDITED),
                ))
                .await;
            replica.seal().await.unwrap();
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let placed = texts(
            &mut replicas[0],
            "SELECT section_id || ':' || section_order_key || ':' || status_id || ':'
                 || status_order_key FROM project_tasks WHERE id = 'task-a'",
        )
        .await;
        let whole: Vec<String> = moves
            .iter()
            .map(|(section, order, status)| format!("{section}:{order}:{status}:{order}"))
            .collect();
        assert!(
            whole.contains(&placed[0]),
            "a move should win whole: {placed:?}"
        );
    });
}

#[test]
fn projects_concurrent_subtask_moves_beyond_depth_one_move_the_deepest_task_to_the_root() {
    block_on(async {
        let sql = format!(
            "{}{}{}",
            task_sql("task-t", "T", 1),
            task_sql("task-u", "U", 2),
            task_sql("task-v", "V", 3),
        );
        let mut replicas = shared(2, &sql).await;
        // Each move alone keeps every task within one level of nesting.
        replicas[0]
            .exec("UPDATE project_tasks SET parent_task_id = 'task-u' WHERE id = 'task-t'")
            .await;
        replicas[1]
            .exec("UPDATE project_tasks SET parent_task_id = 'task-v' WHERE id = 'task-u'")
            .await;
        replicas[0].seal().await.unwrap();
        replicas[1].seal().await.unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                task_parents(replica).await,
                ["task-t>-", "task-u>task-v", "task-v>-"]
            );
        }
    });
}

#[test]
fn projects_a_large_seal_that_moves_a_parent_and_nests_under_it_applies_without_repairs() {
    block_on(async {
        let fillers: Vec<String> = (0..2 * crate::MAX_CHANGES_PER_APPLY)
            .map(|index| format!("task-m{index:03}"))
            .collect();
        let mut sql = format!(
            "{}{}",
            task_sql("task-a", "A", 1),
            task_sql("task-c", "C", 2)
        );
        for (index, id) in fillers.iter().enumerate() {
            sql.push_str(&task_sql(id, "Filler", 3 + index as u32));
        }
        sql.push_str(&task_sql("task-z", "B", 1000));
        sql.push_str("UPDATE project_tasks SET parent_task_id = 'task-c' WHERE id = 'task-z';");
        let mut replicas = shared(2, &sql).await;
        // Alone, A under B nests two levels while B is still under C. The task writes outnumber
        // what one apply transaction takes, and A's write orders before B's.
        replicas[0]
            .exec(&format!(
                "UPDATE project_tasks SET parent_task_id = NULL WHERE id = 'task-z';
                 UPDATE project_tasks SET parent_task_id = 'task-z' WHERE id = 'task-a';
                 UPDATE project_tasks SET title = 'Edited', updated_at = {}
                 WHERE id LIKE 'task-m%';",
                quoted(EDITED),
            ))
            .await;
        let sealed = replicas[0].seal().await.unwrap();
        assert!(sealed.changes > crate::MAX_CHANGES_PER_APPLY);
        let known = replicas[1].stored_vector().await;
        let ops = replicas[0].ops_since(&known).await;
        replicas[1].receive(&ops).await;
        for report in replicas[1].apply_all().await.unwrap() {
            assert!(
                report.seal.is_none(),
                "a receiver should seal no repair: {:?}",
                report.seal
            );
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            let parents = task_parents(replica).await;
            assert!(
                parents.contains(&"task-a>task-z".to_string()),
                "{parents:?}"
            );
            assert!(parents.contains(&"task-z>-".to_string()), "{parents:?}");
        }
    });
}

#[test]
fn projects_a_large_cascaded_deletion_applies_without_repairs() {
    block_on(cascaded_deletion_applies_without_repairs(
        Children::ChecklistItems,
        2 * crate::MAX_CHANGES_PER_APPLY,
        1,
    ));
}

#[test]
fn projects_a_cascaded_deletion_past_one_operation_applies_without_repairs() {
    // Subtasks seal after their parent in key order, so the operation that ends the seal holds
    // deletions the root's operation already implies.
    block_on(cascaded_deletion_applies_without_repairs(
        Children::Subtasks,
        ganbaru_sync_contracts::bounds::MAX_CHANGES_PER_OPERATION,
        2,
    ));
}

/// Rows that cascade from a deleted task.
enum Children {
    ChecklistItems,
    Subtasks,
}

/// Deletes a task with `count` children and checks that a receiver applies the seal of `ops`
/// operations without sealing a cascade tombstone of its own. The deleted root and the
/// deletions naming it as their cause must apply together.
async fn cascaded_deletion_applies_without_repairs(children: Children, count: usize, ops: usize) {
    let mut sql = task_sql("task-a", "A", 1);
    for index in 0..count {
        sql.push_str(&match children {
            Children::ChecklistItems => format!(
                "INSERT INTO project_checklist_items (id, task_id, title, order_key)
                 VALUES ('item-{index:04}', 'task-a', 'Step', {});",
                quoted(&rank(index as u32 + 1)),
            ),
            Children::Subtasks => task_sql(&format!("task-s{index:04}"), "Step", index as u32 + 2),
        });
    }
    sql.push_str("UPDATE project_tasks SET parent_task_id = 'task-a' WHERE id LIKE 'task-s%';");
    let mut replicas = shared(2, &sql).await;
    replicas[0]
        .exec("DELETE FROM project_tasks WHERE id = 'task-a'")
        .await;
    let sealed = replicas[0].seal().await.unwrap();
    assert_eq!((sealed.sealed_ops, sealed.changes), (ops, count + 1));
    let known = replicas[1].stored_vector().await;
    let ops = replicas[0].ops_since(&known).await;
    replicas[1].receive(&ops).await;
    for report in replicas[1].apply_all().await.unwrap() {
        assert!(
            report.seal.is_none(),
            "a receiver should seal no cascade tombstone: {:?}",
            report.seal
        );
    }
    converge(&mut replicas).await;
    assert_converged(&mut replicas).await;
    for replica in &mut replicas {
        assert_eq!(
            replica
                .count(
                    "SELECT (SELECT count(*) FROM project_checklist_items)
                         + (SELECT count(*) FROM project_tasks)"
                )
                .await,
            0
        );
    }
}

#[test]
fn projects_concurrent_dependencies_that_close_a_cycle_hide_the_newest_as_a_conflict() {
    block_on(async {
        let sql = format!(
            "{}{}",
            task_sql("task-t", "T", 1),
            task_sql("task-u", "U", 2)
        );
        let mut replicas = shared(2, &sql).await;
        let added = [("task-t", "task-u"), ("task-u", "task-t")];
        let mut ranks = Vec::new();
        for (replica, (blocking, blocked)) in replicas.iter_mut().zip(added) {
            let id = key(&DEPENDENCIES, &[blocking, blocked]);
            replica
                .exec(&format!(
                    "INSERT INTO project_task_dependencies
                         (id, blocking_task_id, blocked_task_id, created_at, sync_key)
                     VALUES ({id}, {}, {}, {}, {id});",
                    quoted(blocking),
                    quoted(blocked),
                    quoted(CREATED),
                    id = quoted(&id),
                ))
                .await;
            replica.seal().await.unwrap();
            let clock = replica
                .count(&format!(
                    "SELECT max(clock) FROM sync_register_versions
                     WHERE table_id = {} AND row_key = {} AND group_id = {}",
                    DEPENDENCIES_TABLE.0,
                    quoted(&id),
                    dependency_group::EXISTS.get()
                ))
                .await;
            ranks.push((clock, replica.writer(), id));
        }
        let hidden = ranks.iter().max().map(|(_, _, id)| id.clone()).unwrap();
        let kept = ranks.iter().min().map(|(_, _, id)| id.clone()).unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(replica, "SELECT id FROM project_task_dependencies").await,
                std::slice::from_ref(&kept)
            );
            assert_eq!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, DEPENDENCIES_TABLE)
                    .await
                    .unwrap(),
                vec![ConflictRow {
                    table: DEPENDENCIES_TABLE,
                    row_key: hidden.clone(),
                    groups: GroupMask::of(dependency_group::EXISTS),
                }]
            );
        }
    });
}

#[test]
fn projects_keeping_a_cycle_hidden_dependency_removed_closes_its_conflict_everywhere() {
    block_on(async {
        let sql = format!(
            "{}{}",
            task_sql("task-t", "T", 1),
            task_sql("task-u", "U", 2)
        );
        let mut replicas = shared(2, &sql).await;
        let added = [("task-t", "task-u"), ("task-u", "task-t")];
        let mut ranks = Vec::new();
        for (replica, (blocking, blocked)) in replicas.iter_mut().zip(added) {
            let id = key(&DEPENDENCIES, &[blocking, blocked]);
            replica
                .exec(&format!(
                    "INSERT INTO project_task_dependencies
                         (id, blocking_task_id, blocked_task_id, created_at, sync_key)
                     VALUES ({id}, {}, {}, {}, {id});",
                    quoted(blocking),
                    quoted(blocked),
                    quoted(CREATED),
                    id = quoted(&id),
                ))
                .await;
            replica.seal().await.unwrap();
            let clock = replica
                .count(&format!(
                    "SELECT max(clock) FROM sync_register_versions
                     WHERE table_id = {} AND row_key = {} AND group_id = {}",
                    DEPENDENCIES_TABLE.0,
                    quoted(&id),
                    dependency_group::EXISTS.get()
                ))
                .await;
            ranks.push((clock, replica.writer(), id, blocking, blocked));
        }
        let (_, _, hidden, blocking, blocked) = ranks.iter().max().unwrap().clone();
        converge(&mut replicas).await;
        // The hidden edge's tasks stay readable from its retained identity, which a domain
        // needs to present the conflict.
        for replica in &mut replicas {
            let identities = crate::local::retained_group_values(
                &replica.engine,
                &mut replica.conn,
                DEPENDENCIES_TABLE,
                dependency_group::IDENTITY,
            )
            .await
            .unwrap();
            assert!(identities.iter().any(|value| {
                value
                    == &ganbaru_sync_contracts::Value::new(vec![
                        ganbaru_sync_contracts::Field::Text(blocking.to_string()),
                        ganbaru_sync_contracts::Field::Text(blocked.to_string()),
                    ])
                    .unwrap()
            }));
        }
        // The hidden edge is absent from the domain table; inserting and deleting it again
        // seals an existence write of zero.
        replicas[0]
            .exec(&format!(
                "INSERT INTO project_task_dependencies
                     (id, blocking_task_id, blocked_task_id, created_at, sync_key)
                 VALUES ({id}, {}, {}, {}, {id});
                 DELETE FROM project_task_dependencies WHERE id = {id};",
                quoted(blocking),
                quoted(blocked),
                quoted(CREATED),
                id = quoted(&hidden),
            ))
            .await;
        replicas[0].seal().await.unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, DEPENDENCIES_TABLE)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM project_task_dependencies")
                    .await,
                1
            );
        }
    });
}

#[test]
fn projects_a_task_moved_into_a_concurrently_deleted_status_stays_there() {
    block_on(async {
        let mut replicas = shared(2, &task_sql("task-a", "Draft", 1)).await;
        replicas[0]
            .exec(&format!(
                "UPDATE project_statuses SET deleted_at = {} WHERE id = 'status-done'",
                quoted(EDITED)
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE project_tasks SET status_id = 'status-done', completed_at = {edited},
                     status_order_key = {}, updated_at = {edited}
                 WHERE id = 'task-a'",
                quoted(&rank(1)),
                edited = quoted(EDITED),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT t.status_id || ':' || s.deleted_at FROM project_tasks t
                     JOIN project_statuses s ON s.id = t.status_id WHERE t.id = 'task-a'"
                )
                .await,
                [format!("{DONE}:{EDITED}")]
            );
            assert_eq!(surfaced(replica).await, (0, 0));
        }
    });
}

#[test]
fn projects_a_group_deleted_while_a_task_inside_it_is_edited_is_offered_for_recovery() {
    block_on(async {
        let mut replicas = shared(2, &task_sql("task-a", "Draft", 1)).await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM project_groups WHERE id = {}",
                quoted(GROUP)
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE project_tasks SET title = 'Final', updated_at = {} WHERE id = 'task-a'",
                quoted(EDITED)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            for table in ["project_groups", "projects", "project_tasks"] {
                assert_eq!(
                    replica
                        .count(&format!(
                            "SELECT count(*) FROM {table} WHERE id IN ('group-shared',
                                 'project-shared', 'task-a')"
                        ))
                        .await,
                    0,
                    "{table} should hold no deleted row"
                );
            }
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert_eq!(entries.len(), 1, "{entries:?}");
            let entry = &entries[0];
            assert_eq!((entry.table, entry.row_key.as_str()), (GROUPS_TABLE, GROUP));
            assert_eq!(entry.device_id, device_id(1));
            assert!(entry.descendants >= 4, "{entry:?}");
        }
        // Discarding the offer closes it everywhere.
        let replica = &mut replicas[0];
        replica
            .engine
            .close_recovery(
                &mut replica.conn,
                &replica.ctx,
                GROUPS_TABLE,
                GROUP,
                &mut replica.identity.local(),
                replica.now_ms,
            )
            .await
            .unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(surfaced(replica).await, (0, 0));
        }
    });
}

#[test]
fn projects_a_project_deleted_while_a_task_is_added_to_it_is_offered_for_recovery() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM projects WHERE id = {}",
                quoted(PROJECT)
            ))
            .await;
        replicas[1].exec(&task_sql("task-new", "Added", 1)).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM project_tasks WHERE id = 'task-new'")
                    .await,
                0
            );
            assert_eq!(
                replica
                    .count(
                        "SELECT count(*) FROM project_sections WHERE project_id = 'project-shared'"
                    )
                    .await,
                0
            );
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert_eq!(entries.len(), 1, "{entries:?}");
            assert_eq!(
                (entries[0].table, entries[0].row_key.as_str()),
                (PROJECTS_TABLE, PROJECT)
            );
            assert_eq!(entries[0].presentation.title, "Launch");
        }
    });
}

#[test]
fn projects_a_primary_folder_set_in_a_concurrently_deleted_project_leaves_with_it() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM projects WHERE id = {}",
                quoted(PROJECT)
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "INSERT INTO project_primary_working_folders
                     (project_id, working_folder_id, created_at, updated_at)
                 VALUES ({}, 'folder-external', {created}, {created})",
                quoted(PROJECT),
                created = quoted(CREATED),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            // The binding is deleted with the project it is keyed by, naming it as the cause.
            let causes: Vec<(Option<i64>, Option<String>)> = sqlx::query_as(
                "SELECT cause_table, cause_row FROM sync_tombstones
                 WHERE table_id = ? AND row_key = ?",
            )
            .bind(i64::from(PRIMARY_FOLDERS_TABLE.0))
            .bind(PROJECT)
            .fetch_all(&mut replica.conn)
            .await
            .unwrap();
            assert_eq!(
                causes,
                vec![(Some(i64::from(PROJECTS_TABLE.0)), Some(PROJECT.to_string()))]
            );
            assert_eq!(
                replica
                    .count(&format!(
                        "SELECT count(*) FROM project_primary_working_folders WHERE project_id = {}",
                        quoted(PROJECT)
                    ))
                    .await,
                0
            );
            // A binding holds no recoverable group, so the deletion offers nothing to recover.
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert!(entries.is_empty(), "{entries:?}");
        }
    });
}

/// A select field with two options and a task whose value selects the first.
fn select_sql() -> String {
    let created = quoted(CREATED);
    let value = quoted(&key(&VALUES, &["task-a", "field-stage"]));
    format!(
        "{task}
         INSERT INTO project_custom_fields
             (id, project_id, name, field_type, created_at, updated_at, order_key)
         VALUES ('field-stage', {project}, 'Stage', 'select', {created}, {created}, {first});
         INSERT INTO project_custom_field_options
             (id, field_id, name, created_at, updated_at, order_key)
         VALUES ('option-alpha', 'field-stage', 'Alpha', {created}, {created}, {first}),
                ('option-beta', 'field-stage', 'Beta', {created}, {created}, {second});
         INSERT INTO project_custom_field_values (task_id, field_id, sync_key, updated_at)
         VALUES ('task-a', 'field-stage', {value}, {created});
         INSERT INTO project_custom_field_option_values (value_key, ordinal, option_id)
         VALUES ({value}, 0, 'option-alpha');",
        task = task_sql("task-a", "Draft", 1),
        project = quoted(PROJECT),
        first = quoted(&rank(1)),
        second = quoted(&rank(2)),
    )
}

#[test]
fn projects_a_selection_of_a_concurrently_deleted_option_leaves_no_orphan() {
    block_on(async {
        let mut replicas = shared(2, &select_sql()).await;
        replicas[0]
            .exec("DELETE FROM project_custom_field_options WHERE id = 'option-beta'")
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE project_custom_field_option_values SET option_id = 'option-beta';
                 UPDATE project_custom_field_values SET updated_at = {};",
                quoted(EDITED)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count(
                        "SELECT count(*) FROM project_custom_field_option_values v
                         LEFT JOIN project_custom_field_options o ON o.id = v.option_id
                         WHERE o.id IS NULL"
                    )
                    .await,
                0
            );
            assert_eq!(
                texts(replica, "SELECT id FROM project_custom_field_options").await,
                ["option-alpha"]
            );
        }
    });
}

#[test]
fn projects_a_value_cleared_while_another_device_edits_it_follows_the_newest_existence() {
    block_on(async {
        let value = key(&VALUES, &["task-a", "field-note"]);
        let created = quoted(CREATED);
        let sql = format!(
            "{task}
             INSERT INTO project_custom_fields
                 (id, project_id, name, field_type, created_at, updated_at, order_key)
             VALUES ('field-note', {project}, 'Note', 'text', {created}, {created}, {first});
             INSERT INTO project_custom_field_values
                 (task_id, field_id, sync_key, text_value, updated_at)
             VALUES ('task-a', 'field-note', {value}, 'first', {created});",
            task = task_sql("task-a", "Draft", 1),
            project = quoted(PROJECT),
            first = quoted(&rank(1)),
            value = quoted(&value),
        );
        let mut replicas = shared(2, &sql).await;
        let edits = [
            (
                "DELETE FROM project_custom_field_values WHERE field_id = 'field-note'".to_string(),
                None,
            ),
            (
                format!(
                    "UPDATE project_custom_field_values SET text_value = 'second', updated_at = {}
                     WHERE field_id = 'field-note'",
                    quoted(EDITED)
                ),
                Some("second"),
            ),
        ];
        let mut ranks = Vec::new();
        for (replica, (sql, outcome)) in replicas.iter_mut().zip(&edits) {
            replica.exec(sql).await;
            replica.seal().await.unwrap();
            // An edit seals existence with its value, so both writes are existence versions.
            let clock = replica
                .count(&format!(
                    "SELECT max(clock) FROM sync_register_versions
                     WHERE table_id = {} AND row_key = {} AND group_id = {}",
                    VALUES.id.0,
                    quoted(&value),
                    value_group::EXISTS.get()
                ))
                .await;
            ranks.push((clock, replica.writer(), *outcome));
        }
        let expected: Vec<String> = ranks
            .iter()
            .max()
            .and_then(|(_, _, outcome)| *outcome)
            .map(str::to_string)
            .into_iter()
            .collect();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT text_value FROM project_custom_field_values"
                )
                .await,
                expected
            );
        }
    });
}

#[test]
fn projects_notes_views_added_concurrently_keep_two_views() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        for (index, replica) in replicas.iter_mut().enumerate() {
            replica
                .exec(&format!(
                    "INSERT INTO project_views
                         (id, project_id, view_type, created_at, updated_at, order_key)
                     VALUES ('view-notes-{index}', {}, 'notes', {}, {}, {});",
                    quoted(PROJECT),
                    quoted(CREATED),
                    quoted(CREATED),
                    quoted(&rank(1)),
                ))
                .await;
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT id FROM project_views WHERE project_id = 'project-shared' AND view_type = 'notes' ORDER BY order_key, id"
                )
                .await,
                ["view-notes-0", "view-notes-1"]
            );
        }
    });
}

#[test]
fn projects_a_folder_made_primary_while_archived_converges_without_a_failure() {
    block_on(async {
        let created = quoted(CREATED);
        let sql = format!(
            "INSERT INTO project_working_folders
                 (id, project_id, display_name, kind, managed_relative_path, created_at, updated_at)
             VALUES ('working-folder:project-shared', {project}, 'Launch', 'managed',
                     'projects/project-shared', {created}, {created}),
                    ('folder-external', {project}, 'Repository', 'external', NULL,
                     {created}, {created});
             INSERT INTO project_primary_working_folders
                 (project_id, working_folder_id, created_at, updated_at)
             VALUES ({project}, 'working-folder:project-shared', {created}, {created});",
            project = quoted(PROJECT),
        );
        let mut replicas = shared(2, &sql).await;
        replicas[0]
            .exec(&format!(
                "UPDATE project_working_folders SET archived_at = {edited}, updated_at = {edited}
                 WHERE id = 'folder-external'",
                edited = quoted(EDITED)
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE project_primary_working_folders
                 SET working_folder_id = 'folder-external', updated_at = {}",
                quoted(EDITED)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT p.working_folder_id || ':' || f.archived_at
                     FROM project_primary_working_folders p
                     JOIN project_working_folders f ON f.id = p.working_folder_id
                     WHERE p.project_id = 'project-shared'"
                )
                .await,
                [format!("folder-external:{EDITED}")]
            );
        }
    });
}

#[test]
fn projects_applied_rows_are_reported_under_their_table_names() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0].exec(&task_sql("task-a", "Draft", 1)).await;
        replicas[0].seal().await.unwrap();
        let (to, from) = pair(&mut replicas, 1, 0);
        let known = to.stored_vector().await;
        let ops = from.ops_since(&known).await;
        to.receive(&ops).await;
        let notices: Vec<ChangeNotice> = to
            .apply_all()
            .await
            .unwrap()
            .into_iter()
            .flat_map(|report| report.notices)
            .collect();
        assert!(
            notices.contains(&ChangeNotice {
                topic: TASKS.name,
                key: "task-a".to_string(),
            }),
            "{notices:?}"
        );
        assert!(
            !notices.iter().any(|notice| notice.topic == PROJECTS.name),
            "{notices:?}"
        );
    });
}
