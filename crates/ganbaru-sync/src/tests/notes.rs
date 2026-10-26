//! Notes structure across replicas: concurrent edits to different block groups merge, received
//! rows get their hook-derived parents, pages, and plain text from the real Notes hook, a block
//! cycle from concurrent moves is repaired by the Notes repair pass and the repair replicates,
//! a remote delete of a parent block removes the page nested under it everywhere, a parked
//! `child_page` block survives the delete of its former parent and nests again, a trashed page
//! keeps a concurrent title edit and restores, and duplicate page aliases settle on one row.
//!
//! Notes databases across replicas: received databases take their parents from their block,
//! concurrent option adds and edits to different property groups all survive, a value written
//! against a concurrent type change stays as a mismatch, dual relation links and unlinks read the
//! same on both sides, a template snapshot replicates whole, and deleting a database block
//! removes its data source, rows, and values everywhere.

use sqlx::SqliteConnection;

use super::block_on;
use super::replica::{Replica, converge, quoted, rank};
use crate::Hooks;
use crate::hooks::{ChangeSink, HookFuture, MaterializeHook};
use crate::manifest::TableSpec;
use crate::manifest::vault::notes::{
    DOMAIN, OPTION_KEY_PREFIX, PROPERTY_KEY_PREFIX, RELATION_EDGE_KEY_PREFIX, VALUE_KEY_PREFIX,
};

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
fn notes_rows_replicate_without_the_notes_hook() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[1].engine = crate::Engine::with(
            &super::fixture::TEST_MANIFEST,
            super::fixture::test_adapters(),
            crate::domains::vault_hooks().with(
                super::fixture::LISTS_TABLE,
                Box::new(super::fixture::FixtureHook),
            ),
        )
        .unwrap();
        let nested = format!(
            "{shared}
             BEGIN;
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
            shared = page_sql(),
            page = quoted(PAGE),
            a = quoted(BLOCK_A),
            order = quoted(&rank(1)),
            created = quoted(CREATED),
        );
        replicas[0].exec(&nested).await;
        converge(&mut replicas).await;

        let replicated = "SELECT id || '|' || coalesce(parent_page_id, '-') || '|'
                || coalesce(parent_block_id, '-') || '|' || order_key || '|' || type || '|'
                || text || '|' || in_trash
            FROM notes_blocks ORDER BY id";
        let pages = "SELECT id || '|' || coalesce(child_block_id, '-') || '|' || title
            FROM notes_pages ORDER BY id";
        assert_eq!(
            texts(&mut replicas[1], replicated).await,
            texts(&mut replicas[0], replicated).await
        );
        assert_eq!(
            texts(&mut replicas[1], pages).await,
            texts(&mut replicas[0], pages).await
        );
        let receiver = &mut replicas[1];
        assert_eq!(
            block_placements(receiver).await,
            vec![
                "block-a>page-shared@".to_string(),
                "block-b>page-shared@".to_string(),
                "child>block-a@".to_string(),
            ],
            "without the hook, received blocks keep the empty page the hook would fill"
        );
        assert_eq!(
            texts(
                receiver,
                "SELECT parent_type || ':' || coalesce(parent_block_id, '-')
                 FROM notes_pages WHERE id = 'child'",
            )
            .await,
            vec!["workspace:-".to_string()]
        );
        for table in ["notes_page_history_pending", "notes_sync_repair_pending"] {
            let queued = texts(
                receiver,
                &format!("SELECT CAST(COUNT(*) AS TEXT) FROM {table}"),
            )
            .await;
            assert_eq!(queued, vec!["0".to_string()], "{table}");
        }
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

/// SQL nesting page `id` under page `parent` through its own `child_page` block at `position`.
fn nest_page_sql(id: &str, parent: &str, position: u32) -> String {
    format!(
        "BEGIN;
         UPDATE notes_blocks
         SET page_id = {parent}, parent_type = 'page_id', parent_page_id = {parent},
             parent_block_id = NULL, in_trash = 0, order_key = {order}
         WHERE id = {id};
         UPDATE notes_pages
         SET parent_type = 'page_id', parent_page_id = {parent}, child_block_id = {id}
         WHERE id = {id};
         COMMIT;",
        id = quoted(id),
        parent = quoted(parent),
        order = quoted(&rank(position)),
    )
}

#[test]
fn a_parked_child_page_block_survives_its_former_parent_delete_and_nests_again() {
    block_on(async {
        let setup = format!(
            "INSERT INTO notes_pages (id, parent_type, title, created_time, last_edited_time)
             VALUES ('former', 'workspace', 'Former', {created}, {created}),
                    ('next', 'workspace', 'Next', {created}, {created});
             BEGIN;
             INSERT INTO notes_pages
                 (id, parent_type, parent_page_id, child_block_id, title, created_time,
                  last_edited_time)
             VALUES ('child', 'page_id', 'former', 'child', 'Child', {created}, {created});
             INSERT INTO notes_blocks
                 (id, page_id, parent_type, parent_page_id, type, plain_text, order_key,
                  created_time, last_edited_time)
             VALUES ('child', 'former', 'page_id', 'former', 'child_page', 'Child', {order},
                     {created}, {created});
             COMMIT;",
            created = quoted(CREATED),
            order = quoted(&rank(1)),
        );
        let mut replicas = shared(2, &setup).await;
        replicas[0]
            .exec(
                "BEGIN;
                 UPDATE notes_blocks SET in_trash = 1 WHERE id = 'child';
                 UPDATE notes_pages
                 SET parent_type = 'workspace', parent_page_id = NULL, child_block_id = NULL
                 WHERE id = 'child';
                 COMMIT;",
            )
            .await;
        converge(&mut replicas).await;

        // The delete parks the block under its own page; both changes travel in one batch.
        replicas[0]
            .exec("DELETE FROM notes_pages WHERE id = 'former';")
            .await;
        converge(&mut replicas).await;
        let block = "SELECT id || '>' || parent_page_id || '@' || page_id || ':' || in_trash
                     FROM notes_blocks WHERE id = 'child'";
        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(replica, block).await,
                vec!["child>child@child:1".to_string()],
                "replica {}",
                replica.index
            );
            assert_eq!(
                texts(replica, "SELECT id FROM notes_pages ORDER BY id").await,
                vec!["child".to_string(), "next".to_string(), PAGE.to_string()],
                "replica {}",
                replica.index
            );
        }
        assert_converged(&mut replicas).await;

        replicas[1].exec(&nest_page_sql("child", "next", 1)).await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(replica, block).await,
                vec!["child>next@next:0".to_string()],
                "replica {}",
                replica.index
            );
            assert_eq!(
                texts(
                    replica,
                    "SELECT parent_type || ':' || parent_page_id || ':' || child_block_id
                     FROM notes_pages WHERE id = 'child'",
                )
                .await,
                vec!["page_id:next:child".to_string()],
                "replica {}",
                replica.index
            );
        }
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

/// The derived sync key of a Notes row with identity `parts` under `prefix`.
fn derived(prefix: &str, parts: &[&str]) -> String {
    ganbaru_sync_contracts::derived_key(prefix, parts)
        .unwrap()
        .as_str()
        .to_string()
}

/// SQL adding a property of data source `source`.
fn property_sql(
    source: &str,
    id: &str,
    name: &str,
    kind: &str,
    config: &str,
    order: u32,
) -> String {
    format!(
        "INSERT INTO notes_data_source_properties
             (data_source_id, property_id, sync_key, name, type, config, order_key)
         VALUES ({source}, {id}, {key}, {name}, {kind}, {config}, {order});",
        key = quoted(&derived(PROPERTY_KEY_PREFIX, &[source, id])),
        source = quoted(source),
        id = quoted(id),
        name = quoted(name),
        kind = quoted(kind),
        config = quoted(config),
        order = quoted(&rank(order)),
    )
}

/// SQL adding option `id` to property `property` of data source `source`.
fn option_sql(source: &str, property: &str, id: &str, name: &str, position: u32) -> String {
    format!(
        "INSERT INTO notes_data_source_property_options
             (data_source_id, property_id, option_id, sync_key, property_key, name, color,
              position)
         VALUES ({source_q}, {property_q}, {id_q}, {key}, {property_key}, {name}, 'default',
                 {position});",
        key = quoted(&derived(OPTION_KEY_PREFIX, &[source, property, id])),
        property_key = quoted(&derived(PROPERTY_KEY_PREFIX, &[source, property])),
        source_q = quoted(source),
        property_q = quoted(property),
        id_q = quoted(id),
        name = quoted(name),
        position = quoted(&rank(position)),
    )
}

/// SQL storing value `value` of type `kind` for property `property` of row page `page`.
fn value_sql(page: &str, property: &str, kind: &str, value: &str) -> String {
    format!(
        "INSERT INTO notes_page_property_values (page_id, property_id, sync_key, value_type, value)
         VALUES ({page_q}, {property_q}, {key}, {kind}, {value})
         ON CONFLICT (page_id, property_id) DO UPDATE SET
             value_type = excluded.value_type, value = excluded.value;",
        key = quoted(&derived(VALUE_KEY_PREFIX, &[page, property])),
        page_q = quoted(page),
        property_q = quoted(property),
        kind = quoted(kind),
        value = quoted(value),
    )
}

/// SQL adding the relation edge from `source` under `property` to `target`.
fn edge_sql(source: &str, property: &str, target: &str) -> String {
    format!(
        "INSERT INTO notes_relation_edges
             (source_page_id, source_property_id, target_page_id, sync_key, created_time)
         VALUES ({source_q}, {property_q}, {target_q}, {key}, {created});",
        key = quoted(&derived(
            RELATION_EDGE_KEY_PREFIX,
            &[source, property, target]
        )),
        source_q = quoted(source),
        property_q = quoted(property),
        target_q = quoted(target),
        created = quoted(CREATED),
    )
}

/// SQL adding a `child_database` block, its database, and data source `source` on the shared
/// page, with a title property and row pages `rows`.
fn database_sql(database: &str, source: &str, position: u32, rows: &[&str]) -> String {
    let mut sql = format!(
        "BEGIN;
         INSERT INTO notes_blocks
             (id, page_id, parent_type, parent_page_id, type, order_key, created_time,
              last_edited_time)
         VALUES ({database_q}, {page}, 'page_id', {page}, 'child_database', {order}, {created},
                 {created});
         INSERT INTO notes_databases
             (id, block_id, parent_type, parent_page_id, title, created_time, last_edited_time)
         VALUES ({database_q}, {database_q}, 'page_id', {page}, 'Tasks', {created}, {created});
         INSERT INTO notes_data_sources (id, database_id, title, created_time, last_edited_time)
         VALUES ({source_q}, {database_q}, 'Tasks', {created}, {created});
         {title}",
        database_q = quoted(database),
        source_q = quoted(source),
        page = quoted(PAGE),
        order = quoted(&rank(10 + position)),
        created = quoted(CREATED),
        title = property_sql(source, "title", "Name", "title", "{}", 0),
    );
    for row in rows {
        sql.push_str(&format!(
            "INSERT INTO notes_pages
                 (id, parent_type, parent_data_source_id, title, created_time, last_edited_time)
             VALUES ({row}, 'data_source_id', {source_q}, {row}, {created}, {created});",
            row = quoted(row),
            source_q = quoted(source),
            created = quoted(CREATED),
        ));
    }
    sql.push_str("COMMIT;");
    sql
}

#[test]
fn received_databases_get_their_parents_from_their_block() {
    block_on(async {
        let mut replicas = shared(2, &database_sql("db", "source", 0, &["row-1"])).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(
                    replica,
                    "SELECT parent_type || ':' || parent_page_id FROM notes_databases"
                )
                .await,
                vec![format!("page_id:{PAGE}")],
                "replica {}",
                replica.index
            );
        }
        // Notes writes the parent columns with a local move; the receiver derives them.
        replicas[0]
            .exec(&format!(
                "{}UPDATE notes_databases
                 SET parent_type = 'block_id', parent_page_id = NULL, parent_block_id = {};",
                nest_sql("db", BLOCK_A),
                quoted(BLOCK_A),
            ))
            .await;
        converge(&mut replicas).await;
        assert_eq!(
            texts(
                &mut replicas[1],
                "SELECT parent_type || ':' || parent_block_id FROM notes_databases"
            )
            .await,
            vec![format!("block_id:{BLOCK_A}")]
        );
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn concurrent_option_adds_and_property_edits_all_survive() {
    block_on(async {
        let setup = format!(
            "{}{}",
            database_sql("db", "source", 0, &["row-1"]),
            property_sql("source", "stage", "Stage", "select", "{\"select\":{}}", 1),
        );
        let mut replicas = shared(2, &setup).await;
        replicas[0]
            .exec(&format!(
                "{}UPDATE notes_data_source_properties SET name = 'Phase' WHERE property_id = 'stage';",
                option_sql("source", "stage", "todo", "To do", 1)
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "{}UPDATE notes_data_source_properties SET order_key = {} WHERE property_id = 'stage';",
                option_sql("source", "stage", "done", "Done", 2),
                quoted(&rank(5)),
            ))
            .await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(
                    replica,
                    "SELECT option_id || ':' || name FROM notes_data_source_property_options
                     ORDER BY position, option_id"
                )
                .await,
                vec!["todo:To do".to_string(), "done:Done".to_string()],
                "replica {}",
                replica.index
            );
            assert_eq!(
                texts(
                    replica,
                    "SELECT name || ':' || order_key FROM notes_data_source_properties
                     WHERE property_id = 'stage'"
                )
                .await,
                vec![format!("Phase:{}", rank(5))],
                "replica {}",
                replica.index
            );
        }
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_value_written_against_a_concurrent_type_change_stays_as_a_mismatch() {
    block_on(async {
        let setup = format!(
            "{}{}",
            database_sql("db", "source", 0, &["row-1"]),
            property_sql(
                "source",
                "notes",
                "Notes",
                "rich_text",
                "{\"rich_text\":{}}",
                1
            ),
        );
        let mut replicas = shared(2, &setup).await;
        replicas[0]
            .exec(
                "UPDATE notes_data_source_properties
                 SET type = 'number', config = '{\"number\":{\"format\":\"number\"}}'
                 WHERE property_id = 'notes';",
            )
            .await;
        replicas[1]
            .exec(&value_sql(
                "row-1",
                "notes",
                "rich_text",
                "{\"rich_text\":[{\"plain_text\":\"seven\"}]}",
            ))
            .await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(
                    replica,
                    "SELECT property.type || ':' || value.value_type
                     FROM notes_page_property_values AS value
                     JOIN notes_data_source_properties AS property
                       ON property.property_id = value.property_id
                     WHERE value.page_id = 'row-1'"
                )
                .await,
                vec!["number:rich_text".to_string()],
                "replica {} keeps the value with its own type",
                replica.index
            );
        }
        assert_converged(&mut replicas).await;
    });
}

/// A pair of data sources whose relation properties name each other.
fn dual_relation_sql() -> String {
    let relation = |target: &str, synced: &str| {
        serde_json::json!({
            "relation": {
                "data_source_id": target,
                "type": "dual_property",
                "dual_property": { "synced_property_id": synced },
            }
        })
        .to_string()
    };
    format!(
        "{}{}{}{}",
        database_sql("db-a", "source-a", 0, &["task-1"]),
        database_sql("db-b", "source-b", 1, &["person-1"]),
        property_sql(
            "source-a",
            "people",
            "People",
            "relation",
            &relation("source-b", "tasks"),
            1
        ),
        property_sql(
            "source-b",
            "tasks",
            "Tasks",
            "relation",
            &relation("source-a", "people"),
            1
        ),
    )
}

/// Every link `notes_relation_links` reads, as `source>property>target`.
async fn links(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT DISTINCT source_page_id || '>' || source_property_id || '>' || target_page_id
         FROM notes_relation_links ORDER BY 1",
    )
    .await
}

#[test]
fn dual_relation_links_and_unlinks_converge_on_both_sides() {
    block_on(async {
        let mut replicas = shared(2, &dual_relation_sql()).await;
        // Device 0 links from the task side, stored under the canonical property.
        replicas[0]
            .exec(&edge_sql("task-1", "people", "person-1"))
            .await;
        converge(&mut replicas).await;
        let linked = vec![
            "person-1>tasks>task-1".to_string(),
            "task-1>people>person-1".to_string(),
        ];
        for replica in replicas.iter_mut() {
            assert_eq!(links(replica).await, linked, "replica {}", replica.index);
        }

        // Device 1 unlinks from the person side while device 0 links the same pair again,
        // written as the reverse orientation by a device that saw the pairing differently.
        replicas[1]
            .exec("DELETE FROM notes_relation_edges WHERE source_page_id = 'task-1';")
            .await;
        replicas[0]
            .exec(&edge_sql("person-1", "tasks", "task-1"))
            .await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                links(replica).await,
                linked,
                "replica {}: the reverse edge still links both sides",
                replica.index
            );
        }

        replicas[1].exec("DELETE FROM notes_relation_edges;").await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert!(links(replica).await.is_empty(), "replica {}", replica.index);
        }
        assert_converged(&mut replicas).await;
    });
}

/// SQL replacing the blocks of template `template` with paragraphs named by `texts`, the second
/// and later nested under the first.
fn template_blocks_sql(template: &str, texts: &[&str]) -> String {
    let mut sql = format!(
        "BEGIN; DELETE FROM notes_page_template_blocks WHERE template_id = {};",
        quoted(template)
    );
    for (ordinal, text) in texts.iter().enumerate() {
        let (parent_type, parent) = if ordinal == 0 {
            ("template", "NULL".to_string())
        } else {
            ("block_id", quoted(&format!("{template}-{}", texts[0])))
        };
        sql.push_str(&format!(
            "INSERT INTO notes_page_template_blocks
                 (template_id, ordinal, id, parent_type, parent_block_id, type, payload,
                  plain_text, created_time, last_edited_time)
             VALUES ({template_q}, {ordinal}, {id}, '{parent_type}', {parent}, 'paragraph',
                     {payload}, {text_q}, {created}, {created});",
            template_q = quoted(template),
            id = quoted(&format!("{template}-{text}")),
            payload = quoted(&rich_text(text)),
            text_q = quoted(text),
            created = quoted(CREATED),
        ));
    }
    sql.push_str("COMMIT;");
    sql
}

/// Template `template`'s blocks as `id<parent` in ordinal order.
async fn template_blocks(replica: &mut Replica, template: &str) -> Vec<String> {
    texts(
        replica,
        &format!(
            "SELECT id || '<' || coalesce(parent_block_id, '-') FROM notes_page_template_blocks
             WHERE template_id = {} ORDER BY ordinal",
            quoted(template)
        ),
    )
    .await
}

#[test]
fn a_template_snapshot_replicates_whole_and_concurrent_snapshots_never_mix() {
    block_on(async {
        let setup = format!(
            "INSERT INTO notes_page_templates (id, name, created_time, last_edited_time)
             VALUES ('tpl', 'Weekly', {created}, {created});
             {blocks}",
            created = quoted(CREATED),
            blocks = template_blocks_sql("tpl", &["goals", "review"]),
        );
        let mut replicas = shared(2, &setup).await;
        assert_eq!(
            template_blocks(&mut replicas[1], "tpl").await,
            vec![
                "tpl-goals<-".to_string(),
                "tpl-review<tpl-goals".to_string()
            ]
        );
        replicas[0]
            .exec(&format!(
                "{}UPDATE notes_page_templates SET name = 'Weekly plan' WHERE id = 'tpl';",
                template_blocks_sql("tpl", &["plan", "wins", "risks"])
            ))
            .await;
        replicas[1]
            .exec(&template_blocks_sql("tpl", &["notes"]))
            .await;
        converge(&mut replicas).await;
        let first = template_blocks(&mut replicas[0], "tpl").await;
        assert!(
            first
                == vec![
                    "tpl-plan<-".to_string(),
                    "tpl-wins<tpl-plan".to_string(),
                    "tpl-risks<tpl-plan".to_string(),
                ]
                || first == vec!["tpl-notes<-".to_string()],
            "one whole snapshot wins: {first:?}"
        );
        for replica in replicas.iter_mut() {
            assert_eq!(template_blocks(replica, "tpl").await, first);
            assert_eq!(
                texts(replica, "SELECT name FROM notes_page_templates").await,
                vec!["Weekly plan".to_string()]
            );
        }
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn deleting_a_database_block_removes_its_data_source_rows_and_values_everywhere() {
    block_on(async {
        let setup = format!(
            "{}{}{}",
            database_sql("db", "source", 0, &["row-1", "row-2"]),
            property_sql(
                "source",
                "notes",
                "Notes",
                "rich_text",
                "{\"rich_text\":{}}",
                1
            ),
            value_sql("row-1", "notes", "rich_text", "{\"rich_text\":[]}"),
        );
        let mut replicas = shared(2, &setup).await;
        replicas[1]
            .exec("UPDATE notes_pages SET title = 'Renamed' WHERE id = 'row-2';")
            .await;
        replicas[0]
            .exec("DELETE FROM notes_blocks WHERE id = 'db';")
            .await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            for table in [
                "notes_databases",
                "notes_data_sources",
                "notes_data_source_properties",
                "notes_page_property_values",
            ] {
                assert_eq!(
                    texts(
                        replica,
                        &format!("SELECT CAST(COUNT(*) AS TEXT) FROM {table}")
                    )
                    .await,
                    vec!["0".to_string()],
                    "replica {} {table}",
                    replica.index
                );
            }
            assert_eq!(
                texts(replica, "SELECT id FROM notes_pages ORDER BY id").await,
                vec![PAGE.to_string()],
                "replica {}: row pages go with their data source",
                replica.index
            );
        }
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_remotely_removed_value_queues_the_history_of_its_page() {
    block_on(async {
        let setup = format!(
            "{}{}{}",
            database_sql("db", "source", 0, &["row-1"]),
            property_sql(
                "source",
                "notes",
                "Notes",
                "rich_text",
                "{\"rich_text\":{}}",
                1
            ),
            value_sql("row-1", "notes", "rich_text", "{\"rich_text\":[]}"),
        );
        let mut replicas = shared(2, &setup).await;
        replicas[1]
            .exec("DELETE FROM notes_page_history_pending;")
            .await;
        replicas[0]
            .exec("DELETE FROM notes_page_property_values WHERE page_id = 'row-1';")
            .await;
        converge(&mut replicas).await;
        assert_eq!(
            texts(
                &mut replicas[1],
                "SELECT page_id FROM notes_page_history_pending ORDER BY page_id"
            )
            .await,
            vec!["row-1".to_string()]
        );
        assert_converged(&mut replicas).await;
    });
}
