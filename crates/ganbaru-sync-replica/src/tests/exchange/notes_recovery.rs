//! Restoring Notes rows through recovery offers between a hub and a phone. The phone writes
//! Notes here as a replica writer would, so offers open as they will once Notes replicas write.

use super::projects_recovery::Pair;
use crate::recovery::{
    RecoveredRow, RecoveryAction, RecoveryChoice, RecoveryRestorers, RestoreFuture,
};
use ganbaru_sync::Engine;
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::notes::{
    BLOCKS, COMMENTS, DATA_SOURCE_TEMPLATES, DATA_SOURCES, DATABASES, PAGE_TEMPLATES, PAGES,
    PROPERTIES, THREADS,
};
use sqlx::SqliteConnection;

const PAGE: &str = "00000000-0000-4000-8000-0000000000a1";
const OTHER_PAGE: &str = "00000000-0000-4000-8000-0000000000a2";
const BLOCK: &str = "00000000-0000-4000-8000-0000000000b1";
const CHILD: &str = "00000000-0000-4000-8000-0000000000b2";
const SECOND: &str = "00000000-0000-4000-8000-0000000000b3";
const THREAD: &str = "00000000-0000-4000-8000-0000000000c1";
const COMMENT: &str = "00000000-0000-4000-8000-0000000000c2";
const DATABASE: &str = "00000000-0000-4000-8000-0000000000d1";
const SOURCE: &str = "00000000-0000-4000-8000-0000000000d2";
const TEMPLATE: &str = "00000000-0000-4000-8000-0000000000e1";
const CREATED: &str = "2026-10-28T00:00:00.000Z";
const TRASH: &str = "trash-operation";

/// Restores a recovered Notes row through Notes persistence, as the app does.
fn restore_notes_row(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move {
        ganbaru_notes::recovery::restore_recovered_row(
            conn,
            row.table,
            &row.row_key,
            &row.values,
            &row.descendants,
        )
        .await
    })
}

/// Restores like [`restore_notes_row`] with the descendants in reverse order, so a restore that
/// depends on the order the engine lists them in fails one of the two.
fn restore_notes_row_reversed(
    conn: &mut SqliteConnection,
    mut row: RecoveredRow,
) -> RestoreFuture<'_> {
    row.descendants.reverse();
    restore_notes_row(conn, row)
}

fn restorers(reversed: bool) -> RecoveryRestorers {
    let restore = if reversed {
        restore_notes_row_reversed
    } else {
        restore_notes_row
    };
    [
        &PAGES,
        &BLOCKS,
        &DATABASES,
        &DATA_SOURCES,
        &DATA_SOURCE_TEMPLATES,
        &PAGE_TEMPLATES,
        &THREADS,
        &COMMENTS,
    ]
    .into_iter()
    .fold(RecoveryRestorers::new(), |restorers, table| {
        restorers.with(table, restore)
    })
}

/// Restores or discards a Notes offer on the hub.
async fn close(
    pair: &mut Pair,
    table: &TableSpec,
    row_key: &str,
    action: RecoveryAction,
    reversed: bool,
) -> Result<Option<String>, String> {
    crate::recovery::close(
        &Engine::vault(),
        &pair.hub.pool,
        &pair.hub.ctx,
        &mut pair.hub.writer,
        &restorers(reversed),
        &RecoveryChoice {
            table: table.id,
            row_key: row_key.to_string(),
            action,
        },
        super::NOW_MS + 1,
    )
    .await
}

/// Restores a Notes offer on the hub and converges, returning the new root id.
async fn restore(pair: &mut Pair, table: &TableSpec, row_key: &str, reversed: bool) -> String {
    let restored = close(pair, table, row_key, RecoveryAction::Restore, reversed)
        .await
        .expect("restore")
        .expect("restored id");
    assert_ne!(restored, row_key);
    pair.converge().await;
    assert!(pair.offers().await.is_empty());
    restored
}

/// Stored block text holding one plain run of `content`.
fn text(content: &str) -> String {
    format!(
        r#"{{"rich_text":[{{"type":"text","text":{{"content":"{content}","link":null}},"plain_text":"{content}","href":null}}]}}"#
    )
}

/// A trashed workspace page holding a paragraph with a nested child and a second paragraph,
/// with a comment thread on the child, and an empty second page.
fn page_sql() -> String {
    format!(
        "INSERT INTO notes_pages
             (id, parent_type, title, in_trash, trash_owner, trashed_time, created_time,
              last_edited_time)
         VALUES ('{PAGE}', 'workspace', 'Plans', 1, '{TRASH}', '{CREATED}', '{CREATED}',
                 '{CREATED}'),
                ('{OTHER_PAGE}', 'workspace', 'Other', 0, NULL, NULL, '{CREATED}', '{CREATED}');
         INSERT INTO notes_blocks
             (id, page_id, parent_type, parent_page_id, parent_block_id, type, text, plain_text,
              order_key, in_trash, trash_owner, created_time, last_edited_time)
         VALUES ('{BLOCK}', '{PAGE}', 'page_id', '{PAGE}', NULL, 'toggle', '{parent}', 'Parent',
                 'a0', 1, '{TRASH}', '{CREATED}', '{CREATED}'),
                ('{CHILD}', '{PAGE}', 'block_id', NULL, '{BLOCK}', 'paragraph', '{child}',
                 'Child', 'a0', 1, '{TRASH}', '{CREATED}', '{CREATED}'),
                ('{SECOND}', '{PAGE}', 'page_id', '{PAGE}', NULL, 'paragraph', '{second}',
                 'Second', 'a1', 1, '{TRASH}', '{CREATED}', '{CREATED}');
         INSERT INTO notes_comment_threads
             (id, page_id, parent_type, parent_block_id, created_time, last_edited_time)
         VALUES ('{THREAD}', '{PAGE}', 'block_id', '{CHILD}', '{CREATED}', '{CREATED}');
         INSERT INTO notes_comments
             (id, thread_id, rich_text, plain_text, created_by, created_time, last_edited_time)
         VALUES ('{COMMENT}', '{THREAD}', '[]', 'Looks good', 'person-1', '{CREATED}',
                 '{CREATED}');",
        parent = text("Parent"),
        child = text("Child"),
        second = text("Second"),
    )
}

/// A pair sharing the seeded page, with `hub_sql` run on the hub while the phone edits the
/// child paragraph, so the deletion on the hub opens an offer.
async fn pair_with_offer(label: &str, hub_sql: &str) -> Pair {
    let mut pair = Pair::new(label).await;
    pair.hub_edit(&page_sql()).await;
    pair.converge().await;
    pair.hub_edit(hub_sql).await;
    pair.phone_edit(&format!(
        "UPDATE notes_blocks SET text = '{edited}', plain_text = 'Edited child'
         WHERE id = '{CHILD}'",
        edited = text("Edited child"),
    ))
    .await;
    pair.converge().await;
    pair
}

/// Blocks of the page `page`, depth first in order, as `plain text|in trash`.
async fn page_blocks(pair: &Pair, page: &str) -> Vec<String> {
    pair.texts(format!(
        "WITH RECURSIVE tree(id, path) AS (
             SELECT id, order_key FROM notes_blocks WHERE parent_page_id = '{page}'
             UNION ALL
             SELECT child.id, tree.path || '/' || child.order_key
             FROM notes_blocks AS child JOIN tree ON child.parent_block_id = tree.id
         )
         SELECT json_extract(block.text, '$.rich_text[0].plain_text') || '|' || block.in_trash
         FROM tree JOIN notes_blocks AS block ON block.id = tree.id
         ORDER BY tree.path"
    ))
    .await
}

/// A deleted page comes back out of the trash with its blocks, its comment thread, and the
/// edit made unseen by the deletion, whichever order the descendants arrive in.
#[test]
fn a_restored_notes_page_brings_back_its_blocks_and_comments() {
    super::block_on(async {
        for reversed in [false, true] {
            let mut pair = pair_with_offer(
                &format!("notes-recovery-page-{reversed}"),
                &format!("DELETE FROM notes_pages WHERE id = '{PAGE}'"),
            )
            .await;
            assert_eq!(pair.offers().await, [PAGE]);

            let page = restore(&mut pair, &PAGES, PAGE, reversed).await;
            assert_eq!(
                pair.texts(format!(
                    "SELECT title || '|' || in_trash || '|' ||
                        coalesce(trash_owner, '') || coalesce(trashed_time, '')
                     FROM notes_pages WHERE id = '{page}'"
                ))
                .await,
                ["Plans|0|"]
            );
            assert_eq!(
                page_blocks(&pair, &page).await,
                ["Parent|0", "Edited child|0", "Second|0"]
            );
            assert_eq!(
                pair.texts(format!(
                    "SELECT comment.plain_text FROM notes_comments AS comment
                     JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id
                     JOIN notes_blocks AS block ON block.id = thread.parent_block_id
                     WHERE json_extract(block.text, '$.rich_text[0].plain_text') = 'Edited child' AND block.parent_block_id IN
                         (SELECT id FROM notes_blocks WHERE parent_page_id = '{page}')"
                ))
                .await,
                ["Looks good"]
            );
            pair.finish().await;
        }
    });
}

/// A deleted block returns under its page in its retained position, with its child.
#[test]
fn a_restored_notes_block_returns_to_its_page_with_its_children() {
    super::block_on(async {
        for reversed in [false, true] {
            let mut pair = pair_with_offer(
                &format!("notes-recovery-block-{reversed}"),
                &format!(
                    "UPDATE notes_pages SET in_trash = 0, trash_owner = NULL, trashed_time = NULL;
                     UPDATE notes_blocks SET in_trash = 0, trash_owner = NULL
                     WHERE id <> '{BLOCK}';
                     DELETE FROM notes_blocks WHERE id = '{BLOCK}';"
                ),
            )
            .await;
            assert_eq!(pair.offers().await, [BLOCK]);

            let block = restore(&mut pair, &BLOCKS, BLOCK, reversed).await;
            assert_eq!(
                page_blocks(&pair, PAGE).await,
                ["Parent|0", "Edited child|0", "Second|0"]
            );
            assert_eq!(
                pair.texts(format!(
                    "SELECT parent_page_id || '|' || order_key FROM notes_blocks
                     WHERE id = '{block}'"
                ))
                .await,
                [format!("{PAGE}|a0")]
            );
            pair.finish().await;
        }
    });
}

/// A comment thread deleted while a comment was added to it returns with both comments.
#[test]
fn a_restored_notes_comment_thread_brings_back_its_comments() {
    super::block_on(async {
        let mut pair = Pair::new("notes-recovery-thread").await;
        pair.hub_edit(&page_sql()).await;
        pair.converge().await;
        pair.hub_edit(&format!(
            "DELETE FROM notes_comment_threads WHERE id = '{THREAD}'"
        ))
        .await;
        pair.phone_edit(&format!(
            "INSERT INTO notes_comments
                 (id, thread_id, rich_text, plain_text, created_by, created_time, last_edited_time)
             VALUES ('00000000-0000-4000-8000-0000000000c3', '{THREAD}', '[]', 'Agreed',
                     'person-2', '{CREATED}', '{CREATED}')"
        ))
        .await;
        pair.converge().await;
        assert_eq!(pair.offers().await, [THREAD]);

        let thread = restore(&mut pair, &THREADS, THREAD, false).await;
        assert_eq!(
            pair.texts(format!(
                "SELECT plain_text FROM notes_comments WHERE thread_id = '{thread}'
                 ORDER BY plain_text"
            ))
            .await,
            ["Agreed", "Looks good"]
        );
        assert_eq!(
            pair.texts(format!(
                "SELECT parent_block_id FROM notes_comment_threads WHERE id = '{thread}'"
            ))
            .await,
            [CHILD]
        );
        pair.finish().await;
    });
}

/// A deleted page template returns with its blocks in order.
#[test]
fn a_restored_notes_page_template_brings_back_its_blocks() {
    super::block_on(async {
        let mut pair = Pair::new("notes-recovery-template").await;
        pair.hub_edit(&format!(
            "INSERT INTO notes_page_templates (id, name, created_time, last_edited_time)
             VALUES ('{TEMPLATE}', 'Weekly', '{CREATED}', '{CREATED}');
             INSERT INTO notes_page_template_blocks
                 (template_id, ordinal, id, parent_type, payload, plain_text, type,
                  created_time, last_edited_time)
             VALUES ('{TEMPLATE}', 0, 'one', 'template', '{goals}', 'Goals', 'paragraph',
                     '{CREATED}', '{CREATED}'),
                    ('{TEMPLATE}', 1, 'two', 'template', '{review}', 'Review', 'paragraph',
                     '{CREATED}', '{CREATED}');",
            goals = text("Goals"),
            review = text("Review"),
        ))
        .await;
        pair.converge().await;
        pair.hub_edit(&format!(
            "DELETE FROM notes_page_templates WHERE id = '{TEMPLATE}'"
        ))
        .await;
        pair.phone_edit(&format!(
            "UPDATE notes_page_template_blocks SET payload = '{goals}', plain_text = 'Goals first'
             WHERE template_id = '{TEMPLATE}' AND id = 'one'",
            goals = text("Goals first"),
        ))
        .await;
        pair.converge().await;
        assert_eq!(pair.offers().await, [TEMPLATE]);

        let template = restore(&mut pair, &PAGE_TEMPLATES, TEMPLATE, false).await;
        assert_eq!(
            pair.texts(format!(
                "SELECT template.name || '|' || block.plain_text
                 FROM notes_page_templates AS template
                 JOIN notes_page_template_blocks AS block ON block.template_id = template.id
                 WHERE template.id = '{template}' ORDER BY block.ordinal"
            ))
            .await,
            ["Weekly|Goals first", "Weekly|Review"]
        );
        pair.finish().await;
    });
}

/// A pair sharing a database block on the second page with one data source and a select
/// property, with `hub_sql` run on the hub while the phone changes the property's type, so the
/// deletion on the hub opens an offer.
async fn database_pair_with_offer(label: &str, hub_sql: &str) -> Pair {
    let mut pair = Pair::new(label).await;
    pair.hub_edit(&page_sql()).await;
    pair.hub_edit(&format!(
        "INSERT INTO notes_blocks
                 (id, page_id, parent_type, parent_page_id, type, text, order_key, created_time,
                  last_edited_time)
             VALUES ('{DATABASE}', '{OTHER_PAGE}', 'page_id', '{OTHER_PAGE}', 'child_database',
                     '{{}}', 'a0', '{CREATED}', '{CREATED}');
             INSERT INTO notes_databases (id, block_id, title, created_time, last_edited_time)
             VALUES ('{DATABASE}', '{DATABASE}', 'Tasks', '{CREATED}', '{CREATED}');
             INSERT INTO notes_data_sources (id, database_id, title, created_time,
                 last_edited_time)
             VALUES ('{SOURCE}', '{DATABASE}', 'Tasks', '{CREATED}', '{CREATED}');
             INSERT INTO notes_data_source_properties
                 (data_source_id, property_id, sync_key, name, type, config, order_key)
             VALUES ('{SOURCE}', 'stage', '{property}', 'Stage', 'select', '{{\"select\":{{}}}}',
                     'a0');",
        property = super::music_key(&PROPERTIES, &[SOURCE, "stage"]),
    ))
    .await;
    pair.converge().await;
    pair.hub_edit(hub_sql).await;
    // The property's configuration is the part of a database a deletion can make unseen.
    pair.phone_edit(&format!(
        "UPDATE notes_data_source_properties SET type = 'rich_text', config = '{{}}'
         WHERE data_source_id = '{SOURCE}'"
    ))
    .await;
    pair.converge().await;
    pair
}

/// A deleted database block returns in its page with its database, data source, and the
/// property change the deletion made unseen, whichever order the descendants arrive in.
#[test]
fn a_restored_notes_database_block_brings_back_its_database() {
    super::block_on(async {
        for reversed in [false, true] {
            let mut pair = database_pair_with_offer(
                &format!("notes-recovery-database-block-{reversed}"),
                &format!("DELETE FROM notes_blocks WHERE id = '{DATABASE}'"),
            )
            .await;
            assert_eq!(pair.offers().await, [DATABASE]);

            let block = restore(&mut pair, &BLOCKS, DATABASE, reversed).await;
            assert_eq!(
                pair.texts(format!(
                    "SELECT block.parent_page_id || '|' || database.title || '|' ||
                        source.title || '|' || property.name || '|' || property.type
                     FROM notes_blocks AS block
                     JOIN notes_databases AS database ON database.block_id = block.id
                     JOIN notes_data_sources AS source ON source.database_id = database.id
                     JOIN notes_data_source_properties AS property
                         ON property.data_source_id = source.id
                     WHERE block.id = '{block}' AND database.id = '{block}'"
                ))
                .await,
                [format!("{OTHER_PAGE}|Tasks|Tasks|Stage|rich_text")]
            );
            pair.finish().await;
        }
    });
}

/// A database deleted without its block cannot come back under the block's id, so its restore
/// is refused, the offer stays, and a discard closes it on both replicas.
#[test]
fn a_deleted_notes_database_without_its_block_can_only_be_discarded() {
    super::block_on(async {
        let mut pair = database_pair_with_offer(
            "notes-recovery-database",
            &format!("DELETE FROM notes_databases WHERE id = '{DATABASE}'"),
        )
        .await;
        assert_eq!(pair.offers().await, [DATABASE]);

        let refused = close(
            &mut pair,
            &DATABASES,
            DATABASE,
            RecoveryAction::Restore,
            false,
        )
        .await;
        assert_eq!(
            refused,
            Err(
                "a deleted Notes database comes back by restoring the block that holds it"
                    .to_string()
            )
        );
        assert_eq!(pair.offers().await, [DATABASE]);
        assert_eq!(
            close(
                &mut pair,
                &DATABASES,
                DATABASE,
                RecoveryAction::Discard,
                false
            )
            .await,
            Ok(None)
        );
        pair.converge().await;
        assert!(pair.offers().await.is_empty());
        pair.finish().await;
    });
}

/// A comment whose thread is gone is refused with the reason, and a discard closes its offer.
#[test]
fn a_restored_notes_comment_needs_its_thread() {
    super::block_on(async {
        let mut pair = Pair::new("notes-recovery-comment").await;
        pair.hub_edit(&page_sql()).await;
        pair.converge().await;
        pair.hub_edit(&format!(
            "DELETE FROM notes_comments WHERE id = '{COMMENT}'"
        ))
        .await;
        pair.phone_edit(&format!(
            "UPDATE notes_comments SET plain_text = 'Looks great' WHERE id = '{COMMENT}'"
        ))
        .await;
        pair.converge().await;
        assert_eq!(pair.offers().await, [COMMENT]);
        // The thread goes after the offer opened, so the comment has nowhere to return.
        pair.hub_edit(&format!(
            "DELETE FROM notes_comment_threads WHERE id = '{THREAD}'"
        ))
        .await;
        pair.converge().await;

        let refused = close(
            &mut pair,
            &COMMENTS,
            COMMENT,
            RecoveryAction::Restore,
            false,
        )
        .await;
        assert_eq!(
            refused,
            Err(
                "the deleted Notes comment cannot be restored because the comment thread that \
                 held it is gone"
                    .to_string()
            )
        );
        assert_eq!(
            close(
                &mut pair,
                &COMMENTS,
                COMMENT,
                RecoveryAction::Discard,
                false
            )
            .await,
            Ok(None)
        );
        pair.converge().await;
        assert!(pair.offers().await.is_empty());
        pair.finish().await;
    });
}
