use super::*;
use sqlx::sqlite::SqlitePoolOptions;

const NOW: &str = "2026-11-12T09:00:00.000Z";
const ASSIGNMENT: &str = "assignment:review-pair";
const FOLDER: &str = "folder:review-pair";

/// A migrated in-memory pool with foreign keys off, so runs, turns, and checkpoints need
/// none of the rows their dispatch would create.
async fn migrated_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=OFF")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

fn assignment() -> ChatWorkAssignmentId {
    ChatWorkAssignmentId::new(ASSIGNMENT).unwrap()
}

async fn execute(pool: &SqlitePool, sql: String) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(pool)
        .await
        .unwrap();
}

async fn seed_assignment(pool: &SqlitePool, executing_device_id: &str) {
    execute(
        pool,
        format!(
            "INSERT INTO chat_work_assignments
                (id, reply_thread_id, agent_id, triggering_message_item_id, state,
                 executing_device_id, model_selection_schema_version, model_selection_data,
                 created_at, updated_at)
             VALUES ('{ASSIGNMENT}', 'reply:pair', 'participant:helper', 'item:root',
                     'ready_for_review', '{executing_device_id}', 1, '{{}}', '{NOW}', '{NOW}');"
        ),
    )
    .await;
}

async fn seed_checkpoint(pool: &SqlitePool, id: &str, thread: &str, turn_count: u32, repo: &str) {
    let oid = format!("{turn_count:0>40}");
    execute(
        pool,
        format!(
            "INSERT INTO chat_checkpoints
                (id, thread_id, turn_count, repository_identity, hidden_ref_name, git_object_id,
                 status, created_at, index_tree_oid, worktree_tree_oid, head_oid)
             VALUES ('{id}', '{thread}', {turn_count}, '{repo}', 'refs/ganbaru/chat/{id}',
                     '{oid}', 'available', '{NOW}', '{oid}', '{oid}', '{oid}');"
        ),
    )
    .await;
}

/// Seeds run `ordinal` on `thread` whose turn has the given checkpoints.
async fn seed_run(
    pool: &SqlitePool,
    ordinal: u32,
    thread: &str,
    folder: Option<&str>,
    pre: Option<&str>,
    post: Option<&str>,
) {
    let quoted =
        |value: Option<&str>| value.map_or("NULL".to_string(), |value| format!("'{value}'"));
    let environment = folder.map(|_| "'environment:pair'").unwrap_or("NULL");
    let digest = "a".repeat(64);
    execute(
        pool,
        format!(
            "INSERT INTO chat_turns
                (id, thread_id, ordinal, state, safety_mode, interaction_mode,
                 pre_checkpoint_id, post_checkpoint_id, created_at, updated_at)
             VALUES ('turn:pair-{ordinal}', '{thread}', {ordinal}, 'completed',
                     'ask_for_approval', 'build', {pre}, {post}, '{NOW}', '{NOW}');
             UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
             INSERT INTO chat_agent_runs
                (id, assignment_id, project_id, working_folder_id, execution_environment_id,
                 authorization_revision_id,
                 authorization_scope_digest, provider_turn_id, provider_thread_id, state,
                 run_ordinal, created_at, updated_at)
             VALUES ('run:pair-{ordinal}', '{ASSIGNMENT}', 'project:pair', {folder},
                     {environment}, 'authorization:pair', '{digest}',
                     'turn:pair-{ordinal}', '{thread}', 'completed', {ordinal}, '{NOW}', '{NOW}');
             UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;",
            pre = quoted(pre),
            post = quoted(post),
            folder = quoted(folder),
        ),
    )
    .await;
}

fn reason(error: &ChatError) -> &str {
    error.details.as_ref().unwrap()["reason"].as_str().unwrap()
}

#[tokio::test]
async fn assignment_pair_spans_runs_on_different_provider_threads() {
    let pool = migrated_pool().await;
    seed_assignment(&pool, "device:here").await;
    seed_checkpoint(&pool, "checkpoint:first-pre", "thread:first", 1, "repo").await;
    seed_checkpoint(&pool, "checkpoint:first-post", "thread:first", 2, "repo").await;
    seed_checkpoint(&pool, "checkpoint:second-pre", "thread:second", 1, "repo").await;
    seed_checkpoint(&pool, "checkpoint:second-post", "thread:second", 2, "repo").await;
    seed_run(
        &pool,
        1,
        "thread:first",
        Some(FOLDER),
        Some("checkpoint:first-pre"),
        Some("checkpoint:first-post"),
    )
    .await;
    seed_run(
        &pool,
        2,
        "thread:second",
        Some(FOLDER),
        Some("checkpoint:second-pre"),
        Some("checkpoint:second-post"),
    )
    .await;

    let pair = assignment_checkpoint_pair(&pool, &assignment())
        .await
        .unwrap();

    assert_eq!(pair.pre.id.as_str(), "checkpoint:first-pre");
    assert_eq!(pair.post.id.as_str(), "checkpoint:second-post");
    assert_eq!(pair.pre.thread_id.as_str(), "thread:first");
    assert_eq!(pair.post.thread_id.as_str(), "thread:second");
    assert_eq!(
        pair.working_folder_ids,
        vec![Some(FOLDER.to_string()), Some(FOLDER.to_string())]
    );
}

#[tokio::test]
async fn the_revision_scope_changes_after_a_later_run() {
    let pool = migrated_pool().await;
    seed_assignment(&pool, "device:here").await;
    seed_checkpoint(&pool, "checkpoint:pre", "thread:first", 1, "repo").await;
    seed_checkpoint(&pool, "checkpoint:post", "thread:first", 2, "repo").await;
    seed_checkpoint(&pool, "checkpoint:later-post", "thread:first", 3, "repo").await;
    seed_run(
        &pool,
        1,
        "thread:first",
        Some(FOLDER),
        Some("checkpoint:pre"),
        Some("checkpoint:post"),
    )
    .await;
    let before = assignment_checkpoint_pair(&pool, &assignment())
        .await
        .unwrap()
        .revision_scope(&assignment());

    seed_run(
        &pool,
        2,
        "thread:first",
        Some(FOLDER),
        Some("checkpoint:post"),
        Some("checkpoint:later-post"),
    )
    .await;
    let after = assignment_checkpoint_pair(&pool, &assignment())
        .await
        .unwrap()
        .revision_scope(&assignment());

    assert_eq!(before, "assignment:assignment:review-pair:checkpoint:post");
    assert_eq!(
        after,
        "assignment:assignment:review-pair:checkpoint:later-post"
    );
}

#[tokio::test]
async fn an_assignment_without_local_runs_reports_where_it_ran_and_its_files() {
    let pool = migrated_pool().await;
    seed_assignment(&pool, "device:other").await;
    let payload = |paths: &[&str]| {
        serde_json::json!({
            "changedFiles": paths
                .iter()
                .map(|path| serde_json::json!({ "relativePath": path }))
                .collect::<Vec<_>>(),
        })
        .to_string()
    };
    for (index, paths) in [vec!["src/a.rs", "src/b.rs"], vec!["src/b.rs", "docs/c.md"]]
        .iter()
        .enumerate()
    {
        sqlx::query(
            "INSERT INTO chat_work_semantic_updates
                (item_id, assignment_id, update_kind, payload_data, created_at)
             VALUES (?, ?, 'review', ?, ?)",
        )
        .bind(format!("item:review-{index}"))
        .bind(ASSIGNMENT)
        .bind(payload(paths))
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
    }

    let error = assignment_checkpoint_pair(&pool, &assignment())
        .await
        .unwrap_err();

    assert_eq!(error.code, ChatErrorCode::NotFound);
    let details = error.details.unwrap();
    assert_eq!(details["reason"], "executed_elsewhere");
    assert_eq!(details["executingDeviceId"], "device:other");
    assert_eq!(
        details["changedFiles"],
        serde_json::json!(["docs/c.md", "src/a.rs", "src/b.rs"])
    );
    assert_eq!(details["changedFilesTruncated"], false);
}

#[tokio::test]
async fn a_last_run_without_a_post_checkpoint_falls_back_to_the_last_settled_one() {
    let pool = migrated_pool().await;
    seed_assignment(&pool, "device:here").await;
    seed_checkpoint(&pool, "checkpoint:pre", "thread:first", 1, "repo").await;
    seed_checkpoint(&pool, "checkpoint:post", "thread:first", 2, "repo").await;
    seed_run(
        &pool,
        1,
        "thread:first",
        Some(FOLDER),
        Some("checkpoint:pre"),
        Some("checkpoint:post"),
    )
    .await;
    seed_run(
        &pool,
        2,
        "thread:first",
        Some(FOLDER),
        Some("checkpoint:post"),
        None,
    )
    .await;

    let pair = assignment_checkpoint_pair(&pool, &assignment())
        .await
        .unwrap();

    assert_eq!(pair.post.id.as_str(), "checkpoint:post");
}

#[tokio::test]
async fn runs_without_a_post_checkpoint_report_missing_assignment_checkpoints() {
    let pool = migrated_pool().await;
    seed_assignment(&pool, "device:here").await;
    seed_checkpoint(&pool, "checkpoint:pre", "thread:first", 1, "repo").await;
    seed_run(
        &pool,
        1,
        "thread:first",
        Some(FOLDER),
        Some("checkpoint:pre"),
        None,
    )
    .await;

    let error = assignment_checkpoint_pair(&pool, &assignment())
        .await
        .unwrap_err();

    assert_eq!(error.code, ChatErrorCode::NotFound);
    assert_eq!(reason(&error), "assignment_checkpoints");
}

#[tokio::test]
async fn an_invalidated_checkpoint_reports_missing_assignment_checkpoints() {
    let pool = migrated_pool().await;
    seed_assignment(&pool, "device:here").await;
    seed_checkpoint(&pool, "checkpoint:pre", "thread:first", 1, "repo").await;
    seed_checkpoint(&pool, "checkpoint:post", "thread:first", 2, "repo").await;
    seed_run(
        &pool,
        1,
        "thread:first",
        Some(FOLDER),
        Some("checkpoint:pre"),
        Some("checkpoint:post"),
    )
    .await;
    execute(
        &pool,
        "UPDATE chat_checkpoints SET status = 'invalid' WHERE id = 'checkpoint:pre';".to_string(),
    )
    .await;

    let error = assignment_checkpoint_pair(&pool, &assignment())
        .await
        .unwrap_err();

    assert_eq!(reason(&error), "assignment_checkpoints");
}

#[tokio::test]
async fn runs_across_repositories_report_a_changed_repository() {
    let pool = migrated_pool().await;
    seed_assignment(&pool, "device:here").await;
    seed_checkpoint(&pool, "checkpoint:pre", "thread:first", 1, "repo-one").await;
    seed_checkpoint(&pool, "checkpoint:post", "thread:second", 1, "repo-two").await;
    seed_run(
        &pool,
        1,
        "thread:first",
        Some(FOLDER),
        Some("checkpoint:pre"),
        None,
    )
    .await;
    seed_run(
        &pool,
        2,
        "thread:second",
        Some(FOLDER),
        None,
        Some("checkpoint:post"),
    )
    .await;

    let error = assignment_checkpoint_pair(&pool, &assignment())
        .await
        .unwrap_err();

    assert_eq!(error.code, ChatErrorCode::NotFound);
    assert_eq!(reason(&error), "assignment_repository_changed");
}

#[tokio::test]
async fn an_unknown_assignment_is_not_found_without_a_source_reason() {
    let pool = migrated_pool().await;

    let error = assignment_checkpoint_pair(&pool, &assignment())
        .await
        .unwrap_err();

    assert_eq!(error.code, ChatErrorCode::NotFound);
    assert!(error.details.is_none());
}
