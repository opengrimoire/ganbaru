//! Notes structure across replicas: concurrent edits to different block groups merge, received
//! rows get their hook-derived parents, pages, and plain text from the real Notes hook, a block
//! cycle from concurrent moves is repaired by the Notes repair pass and the repair replicates,
//! a remote delete of a parent block removes the page nested under it everywhere, a trashed page keeps a concurrent title edit and restores, and duplicate page aliases settle
//! on one row.

use sqlx::SqliteConnection;

use super::block_on;
use super::replica::{Replica, converge, quoted, rank};
use crate::Hooks;
use crate::hooks::{ChangeSink, HookFuture, MaterializeHook};
use crate::manifest::TableSpec;
use crate::manifest::vault::notes::DOMAIN;

const CREATED: &str = "2027-01-01T00:00:00.000Z";
const EDITED: &str = "2027-01-02T00:00:00.000Z";
const PAGE: &str = "page-shared";
const BLOCK_A: &str = "block-a";
const BLOCK_B: &str = "block-b";

/// Runs the Notes crate's post-apply refresh, as the app's Notes hook does.
struct NotesTestHook;

impl MaterializeHook for NotesTestHook {
    fn after_materialize<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            ganbaru_notes::sync_apply::refresh_after_apply(conn, table.id, changed_keys).await?;
            for key in changed_keys {
                changes.report(table.name, key.clone());
            }
            Ok(())
        })
    }
}

/// `hooks` with [`NotesTestHook`] on every replicated Notes table.
pub(super) fn with_notes_hooks(hooks: Hooks) -> Hooks {
    DOMAIN.tables.iter().fold(hooks, |hooks, table| {
        hooks.with(table.id, Box::new(NotesTestHook))
    })
}

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 288).await);
    }
    replicas
}

/// Rich text JSON holding `content`.
fn rich_text(content: &str) -> String {
    serde_json::json!({
        "rich_text": [{
            "type": "text",
            "text": { "content": content },
            "plain_text": content,
        }]
    })
    .to_string()
}

/// SQL adding a root paragraph of `page` at `position`.
fn root_block_sql(id: &str, page: &str, content: &str, position: u32) -> String {
    format!(
        "INSERT INTO notes_blocks
             (id, page_id, parent_type, parent_page_id, type, text, plain_text, order_key,
              created_time, last_edited_time)
         VALUES ({id}, {page}, 'page_id', {page}, 'paragraph', {text}, {plain}, {order},
                 {created}, {created});",
        id = quoted(id),
        page = quoted(page),
        text = quoted(&rich_text(content)),
        plain = quoted(content),
        order = quoted(&rank(position)),
        created = quoted(CREATED),
    )
}

/// A workspace page holding two root paragraphs.
fn page_sql() -> String {
    format!(
        "INSERT INTO notes_pages (id, parent_type, title, created_time, last_edited_time)
         VALUES ({page}, 'workspace', 'Shared', {created}, {created});
         {a}{b}",
        page = quoted(PAGE),
        created = quoted(CREATED),
        a = root_block_sql(BLOCK_A, PAGE, "first", 1),
        b = root_block_sql(BLOCK_B, PAGE, "second", 2),
    )
}

/// Every replica holding the shared page and `sql`, with operations exchanged.
async fn shared(count: usize, sql: &str) -> Vec<Replica> {
    let mut replicas = replicas(count).await;
    replicas[0].exec(&format!("{}{sql}", page_sql())).await;
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

/// Every block as `id>parent@page`, with the parent page or block it names.
async fn block_placements(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT id || '>' || coalesce(parent_page_id, parent_block_id) || '@' || page_id
         FROM notes_blocks ORDER BY id",
    )
    .await
}

/// SQL nesting block `id` under block `parent`.
fn nest_sql(id: &str, parent: &str) -> String {
    format!(
        "UPDATE notes_blocks
         SET parent_type = 'block_id', parent_page_id = NULL, parent_block_id = {parent},
             last_edited_time = {edited}
         WHERE id = {id};",
        id = quoted(id),
        parent = quoted(parent),
        edited = quoted(EDITED),
    )
}

#[test]
fn concurrent_edits_to_different_block_groups_merge() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0]
            .exec(&format!(
                "UPDATE notes_blocks SET text = {}, plain_text = 'edited' WHERE id = {};",
                quoted(&rich_text("edited")),
                quoted(BLOCK_A),
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE notes_blocks SET type = 'to_do', attrs = '{{\"checked\":true}}'
                 WHERE id = {a};
                 UPDATE notes_blocks SET order_key = {first} WHERE id = {b};",
                a = quoted(BLOCK_A),
                b = quoted(BLOCK_B),
                first = quoted(&rank(0)),
            ))
            .await;
        converge(&mut replicas).await;

        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || ':' || type || ':' || plain_text || ':' || attrs
                     FROM notes_blocks ORDER BY order_key, id",
                )
                .await,
                vec![
                    "block-b:paragraph:second:{}".to_string(),
                    "block-a:to_do:edited:{\"checked\":true}".to_string(),
                ],
                "replica {}",
                replica.index
            );
        }
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn received_rows_get_their_hook_derived_parents_pages_and_text() {
    block_on(async {
        let nested = format!(
            "INSERT INTO notes_blocks
                 (id, page_id, parent_type, parent_block_id, type, text, plain_text, order_key,
                  created_time, last_edited_time)
             VALUES ('nested', {page}, 'block_id', {a}, 'paragraph', {text}, 'deep', {order},
                     {created}, {created});
             BEGIN;
             INSERT INTO notes_pages
                 (id, parent_type, parent_block_id, child_block_id, title, created_time,
                  last_edited_time)
             VALUES ('child', 'block_id', {a}, 'child', 'Child', {created}, {created});
             INSERT INTO notes_blocks
                 (id, page_id, parent_type, parent_block_id, type, plain_text, order_key,
                  created_time, last_edited_time)
             VALUES ('child', {page}, 'block_id', {a}, 'child_page', 'Child', {after},
                     {created}, {created});
             COMMIT;",
            page = quoted(PAGE),
            a = quoted(BLOCK_A),
            text = quoted(&rich_text("deep")),
            order = quoted(&rank(1)),
            after = quoted(&rank(2)),
            created = quoted(CREATED),
        );
        let mut replicas = shared(2, &nested).await;

        let receiver = &mut replicas[1];
        assert_eq!(
            block_placements(receiver).await,
            vec![
                "block-a>page-shared@page-shared".to_string(),
                "block-b>page-shared@page-shared".to_string(),
                "child>block-a@page-shared".to_string(),
                "nested>block-a@page-shared".to_string(),
            ]
        );
        assert_eq!(
            texts(
                receiver,
                "SELECT parent_type || ':' || coalesce(parent_page_id, '-') || ':'
                     || coalesce(parent_block_id, '-')
                 FROM notes_pages WHERE id = 'child'",
            )
            .await,
            vec!["block_id:-:block-a".to_string()]
        );
        assert_eq!(
            texts(
                receiver,
                "SELECT plain_text FROM notes_blocks WHERE id = 'nested'"
            )
            .await,
            vec!["deep".to_string()]
        );
        assert_eq!(
            texts(
                receiver,
                "SELECT page_id FROM notes_page_history_pending ORDER BY page_id"
            )
            .await,
            vec!["child".to_string(), PAGE.to_string()]
        );
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_block_cycle_from_concurrent_moves_is_repaired_and_the_repair_replicates() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0].exec(&nest_sql(BLOCK_A, BLOCK_B)).await;
        replicas[1].exec(&nest_sql(BLOCK_B, BLOCK_A)).await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || '>' || parent_block_id FROM notes_blocks ORDER BY id"
                )
                .await,
                vec!["block-a>block-b".to_string(), "block-b>block-a".to_string()],
                "replica {} should hold the merged cycle",
                replica.index
            );
        }

        let keys = vec![BLOCK_A.to_string(), BLOCK_B.to_string()];
        let report =
            ganbaru_notes::sync_repair::repair_structure_on(&mut replicas[0].conn, &[], &keys)
                .await
                .unwrap();
        assert_eq!(report.cycle_blocks, vec![BLOCK_A.to_string()]);
        converge(&mut replicas).await;

        for replica in replicas.iter_mut() {
            assert_eq!(
                block_placements(replica).await,
                vec![
                    "block-a>page-shared@page-shared".to_string(),
                    "block-b>block-a@page-shared".to_string(),
                ],
                "replica {}",
                replica.index
            );
            assert_eq!(
                texts(
                    replica,
                    "SELECT parent_type FROM notes_blocks WHERE id = 'block-a'"
                )
                .await,
                vec!["page_id".to_string()]
            );
        }
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_remote_delete_of_the_parent_block_removes_the_nested_page_everywhere() {
    block_on(async {
        let child = format!(
            "BEGIN;
             INSERT INTO notes_pages
                 (id, parent_type, parent_block_id, child_block_id, title, created_time,
                  last_edited_time)
             VALUES ('child', 'block_id', {a}, 'child', 'Child', {created}, {created});
             INSERT INTO notes_blocks
                 (id, page_id, parent_type, parent_block_id, type, plain_text, order_key,
                  created_time, last_edited_time)
             VALUES ('child', {page}, 'block_id', {a}, 'child_page', 'Child', {order},
                     {created}, {created});
             COMMIT;",
            page = quoted(PAGE),
            a = quoted(BLOCK_A),
            order = quoted(&rank(1)),
            created = quoted(CREATED),
        );
        let mut replicas = shared(2, &child).await;
        replicas[1]
            .exec(&format!(
                "DELETE FROM notes_blocks WHERE id = {};",
                quoted(BLOCK_A)
            ))
            .await;
        converge(&mut replicas).await;

        for replica in replicas.iter_mut() {
            assert_eq!(
                block_placements(replica).await,
                vec!["block-b>page-shared@page-shared".to_string()],
                "replica {}",
                replica.index
            );
            assert_eq!(
                texts(replica, "SELECT id FROM notes_pages ORDER BY id").await,
                vec![PAGE.to_string()],
                "replica {}",
                replica.index
            );
        }
        let report = ganbaru_notes::sync_repair::repair_structure_on(
            &mut replicas[0].conn,
            &["child".to_string()],
            &[BLOCK_A.to_string(), "child".to_string()],
        )
        .await
        .unwrap();
        assert!(report.is_empty());
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_trashed_page_keeps_a_concurrent_title_edit_and_restores() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0]
            .exec(&format!(
                "UPDATE notes_pages SET in_trash = 1, trashed_time = {edited} WHERE id = {page};",
                edited = quoted(EDITED),
                page = quoted(PAGE),
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE notes_pages SET title = 'Renamed' WHERE id = {};",
                quoted(PAGE)
            ))
            .await;
        converge(&mut replicas).await;
        let page_state = format!(
            "SELECT title || ':' || in_trash || ':' || coalesce(trashed_time, '-')
             FROM notes_pages WHERE id = {}",
            quoted(PAGE)
        );
        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(replica, &page_state).await,
                vec![format!("Renamed:1:{EDITED}")],
                "replica {}",
                replica.index
            );
        }

        replicas[1]
            .exec(&format!(
                "UPDATE notes_pages SET in_trash = 0, trashed_time = NULL WHERE id = {};",
                quoted(PAGE)
            ))
            .await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(replica, &page_state).await,
                vec!["Renamed:0:-".to_string()],
                "replica {}",
                replica.index
            );
        }
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn concurrent_duplicate_page_aliases_settle_on_the_lowest_key() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        let insert = |id: &str, written: &str| {
            format!(
                "INSERT INTO notes_page_aliases
                     (id, page_id, alias, normalized_alias, created_time, last_edited_time)
                 VALUES ({id}, {page}, {written}, 'home', {created}, {created});",
                id = quoted(id),
                page = quoted(PAGE),
                written = quoted(written),
                created = quoted(CREATED),
            )
        };
        replicas[0].exec(&insert("name-b", "Home")).await;
        replicas[1].exec(&insert("name-a", "HOME")).await;
        converge(&mut replicas).await;

        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || ':' || alias FROM notes_page_aliases ORDER BY id"
                )
                .await,
                vec!["name-a:HOME".to_string()],
                "replica {}",
                replica.index
            );
        }
        assert_converged(&mut replicas).await;
    });
}
