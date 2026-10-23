//! Listing and resolving Projects conflicts through the domain commands between a hub and a
//! phone.

use std::collections::HashMap;

use super::projects_recovery::{Pair, rank};
use super::{HUB_DEVICE, NOW_MS, PHONE_DEVICE, Replica, VAULT_ID, block_on};
use ganbaru_projects::conflicts::{
    ProjectsConflictError, ProjectsConflictResolution, conflicts_from_pool, resolve_from_pool,
};
use ganbaru_projects::keys::dependency_key;
use ganbaru_sync::DeviceNames;
use serde_json::{Value, json};

/// The conflicts of `project-1` on `replica`, as the frontend receives them.
async fn conflicts(replica: &Replica, device: &str) -> Value {
    let read = conflicts_from_pool(
        &replica.pool,
        VAULT_ID,
        &DeviceNames::new(device, HashMap::new()),
        "project-1",
    )
    .await
    .expect("read conflicts");
    serde_json::to_value(read).expect("serialize conflicts")
}

/// Asserts that neither replica has a conflict left.
async fn assert_no_conflicts(pair: &Pair) {
    for (replica, device) in [(&pair.hub, HUB_DEVICE), (&pair.phone, PHONE_DEVICE)] {
        let read = conflicts(replica, device).await;
        assert_eq!(read["fields"], json!([]));
        assert_eq!(read["dependencies"], json!([]));
    }
}

/// Resolves on the phone through the command entry point, as the frontend requests it.
async fn resolve(pair: &Pair, request: Value) -> Result<(), ProjectsConflictError> {
    let request: ProjectsConflictResolution =
        serde_json::from_value(request).expect("resolution request");
    resolve_from_pool(&pair.phone.pool, VAULT_ID, request, NOW_MS + 1)
        .await
        .map(|_| ())
}

/// A second top-level task `task-2` in the first section and status.
fn add_second_task() -> String {
    format!(
        "INSERT INTO project_tasks
            (id, project_id, section_id, status_id, title, section_order_key, status_order_key)
         VALUES ('task-2', 'project-1', 'section-1', 'status-1', 'Plan', '{r1}', '{r1}')",
        r1 = rank(1),
    )
}

fn insert_dependency(blocking: &str, blocked: &str) -> String {
    let key = dependency_key(blocking, blocked).expect("dependency key");
    format!(
        "INSERT INTO project_task_dependencies
            (id, sync_key, blocking_task_id, blocked_task_id, dependency_type)
         VALUES ('{key}', '{key}', '{blocking}', '{blocked}', 'blocks')"
    )
}

/// Dependencies as `blocking>blocked`, the same on both replicas.
async fn dependencies(pair: &Pair) -> Vec<String> {
    pair.texts(
        "SELECT blocking_task_id || '>' || blocked_task_id FROM project_task_dependencies
         ORDER BY 1"
            .to_string(),
    )
    .await
}

/// A pair whose devices each added one direction between `task-1` and `task-2`, so one edge
/// is hidden as a cycle conflict. Returns the pair and the hidden edge's conflict.
async fn cycle_conflict(label: &str) -> (Pair, Value) {
    let mut pair = Pair::new(label).await;
    pair.hub_edit(&add_second_task()).await;
    pair.converge().await;
    pair.hub_edit(&insert_dependency("task-1", "task-2")).await;
    pair.phone_edit(&insert_dependency("task-2", "task-1"))
        .await;
    pair.converge().await;
    assert_eq!(dependencies(&pair).await.len(), 1);

    let read = conflicts(&pair.phone, PHONE_DEVICE).await;
    let hidden = read["dependencies"].as_array().expect("dependencies");
    assert_eq!(hidden.len(), 1);
    let hidden = hidden[0].clone();
    assert_eq!(hidden["wouldCycle"], true);
    let surviving = dependencies(&pair).await;
    assert_eq!(
        surviving,
        [format!(
            "{}>{}",
            hidden["blockedTaskId"].as_str().expect("blocked"),
            hidden["blockingTaskId"].as_str().expect("blocking"),
        )]
    );
    (pair, hidden)
}

fn dependency_request(hidden: &Value, choice: &str) -> Value {
    json!({
        "kind": "dependency",
        "blockingTaskId": hidden["blockingTaskId"],
        "blockedTaskId": hidden["blockedTaskId"],
        "choice": choice,
    })
}

#[test]
fn a_task_title_resolved_with_the_other_version_closes_everywhere_and_enters_its_history() {
    block_on(async {
        let mut pair = Pair::new("projects-conflict-title").await;
        pair.hub_edit("UPDATE project_tasks SET title = 'Hub title' WHERE id = 'task-1'")
            .await;
        pair.phone_edit("UPDATE project_tasks SET title = 'Phone title' WHERE id = 'task-1'")
            .await;
        pair.converge().await;

        let read = conflicts(&pair.phone, PHONE_DEVICE).await;
        assert_eq!(read["dependencies"], json!([]));
        let fields = read["fields"].as_array().expect("fields");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0]["target"], "task");
        assert_eq!(fields[0]["id"], "task-1");
        assert_eq!(fields[0]["field"], "title");
        let versions = fields[0]["versions"].as_array().expect("versions");
        assert_eq!(versions.len(), 2);
        let displayed = versions
            .iter()
            .find(|version| version["displayed"] == true)
            .expect("displayed version");
        let other = versions
            .iter()
            .find(|version| version["displayed"] == false)
            .expect("other version");
        assert_eq!(
            pair.texts("SELECT title FROM project_tasks WHERE id = 'task-1'".to_string())
                .await,
            [displayed["text"].as_str().expect("displayed text")]
        );
        let hub_read = conflicts(&pair.hub, HUB_DEVICE).await;
        assert_eq!(hub_read["fields"].as_array().map(Vec::len), Some(1));

        resolve(
            &pair,
            json!({
                "kind": "field",
                "target": "task",
                "id": "task-1",
                "field": "title",
                "choice": { "kind": "version", "version": other["version"] },
            }),
        )
        .await
        .expect("use the other version");
        let history: Vec<String> = sqlx::query_scalar(
            "SELECT event_type || '|' || field_name || '|' || old_value || '|' || new_value
             FROM project_task_change_events WHERE task_id = 'task-1'",
        )
        .fetch_all(&pair.phone.pool)
        .await
        .expect("read task history");
        assert_eq!(
            history,
            [format!(
                "updated|title|{}|{}",
                displayed["text"].as_str().expect("displayed text"),
                other["text"].as_str().expect("other text"),
            )]
        );

        pair.phone.seal().await;
        pair.converge().await;
        assert_eq!(
            pair.texts("SELECT title FROM project_tasks WHERE id = 'task-1'".to_string())
                .await,
            [other["text"].as_str().expect("other text")]
        );
        assert_no_conflicts(&pair).await;
        pair.finish().await;
    });
}

#[test]
fn a_cycle_hidden_dependency_is_refused_while_it_would_cycle_and_restores_after() {
    block_on(async {
        let (mut pair, hidden) = cycle_conflict("projects-conflict-restore").await;
        let refused = resolve(&pair, dependency_request(&hidden, "restore")).await;
        assert!(
            matches!(refused, Err(ProjectsConflictError::WouldCycle(_))),
            "{refused:?}"
        );
        let surviving = dependencies(&pair).await;
        assert_eq!(surviving.len(), 1);

        // Removing the surviving edge lets the hidden one return. A seal would bring it back by
        // itself, so the choice is made between the local edit and the next seal.
        sqlx::raw_sql("DELETE FROM project_task_dependencies")
            .execute(&pair.phone.pool)
            .await
            .expect("remove the surviving dependency");
        let read = conflicts(&pair.phone, PHONE_DEVICE).await;
        assert_eq!(read["dependencies"][0]["wouldCycle"], false, "{read}");
        resolve(&pair, dependency_request(&hidden, "restore"))
            .await
            .expect("restore the dependency");

        pair.phone.seal().await;
        pair.converge().await;
        assert_eq!(
            dependencies(&pair).await,
            [format!(
                "{}>{}",
                hidden["blockingTaskId"].as_str().expect("blocking"),
                hidden["blockedTaskId"].as_str().expect("blocked"),
            )]
        );
        assert_no_conflicts(&pair).await;
        pair.finish().await;
    });
}

#[test]
fn keeping_a_cycle_hidden_dependency_removed_closes_its_conflict_everywhere() {
    block_on(async {
        let (mut pair, hidden) = cycle_conflict("projects-conflict-keep-removed").await;
        let surviving = dependencies(&pair).await;
        resolve(&pair, dependency_request(&hidden, "keep_removed"))
            .await
            .expect("keep the dependency removed");

        pair.phone.seal().await;
        pair.converge().await;
        assert_eq!(dependencies(&pair).await, surviving);
        assert_no_conflicts(&pair).await;
        pair.finish().await;
    });
}
