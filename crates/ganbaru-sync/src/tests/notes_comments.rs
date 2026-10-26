//! Notes comments across replicas: threads, anchors, comments, reactions, and suggestions
//! round trip and receivers fill their pages, concurrent comment edits and resolves both apply,
//! a comment delete and a concurrent edit both apply, a comment added on a block deleted
//! elsewhere is hidden and offered for recovery, a suggestion accept and a concurrent block
//! edit both apply, an accept outranks a concurrent reject, and a crash while sealing or
//! applying comment rows leaves nothing partial.

use ganbaru_sync_contracts::derived_key;

use super::block_on;
use super::chat::after_rollback;
use super::notes::{BLOCK_A, CREATED, EDITED, PAGE, assert_converged, shared, texts};
use super::replica::{Replica, converge, quoted, rank};
use crate::manifest::vault::notes::{COMMENTS_TABLE, REACTION_KEY_PREFIX};

const BLOCK_B: &str = "block-b";
const OTHER_PAGE: &str = "page-other";
const PERSON: &str = "person-a";

/// Comment rich text JSON holding `content`.
fn comment_text(content: &str) -> String {
    serde_json::json!([{
        "type": "text",
        "text": { "content": content },
        "plain_text": content,
    }])
    .to_string()
}

/// SQL adding comment `id` to `thread` with `content`.
fn comment_sql(id: &str, thread: &str, content: &str) -> String {
    format!(
        "INSERT INTO notes_comments
             (id, thread_id, rich_text, plain_text, created_by, created_time, last_edited_time)
         VALUES ({id}, {thread}, {text}, {plain}, {person}, {created}, {created});",
        id = quoted(id),
        thread = quoted(thread),
        text = quoted(&comment_text(content)),
        plain = quoted(content),
        person = quoted(PERSON),
        created = quoted(CREATED),
    )
}

/// SQL editing the body of comment `id` to `content`.
fn edit_sql(id: &str, content: &str) -> String {
    format!(
        "UPDATE notes_comments
         SET rich_text = {text}, plain_text = {plain}, last_edited_time = {edited}
         WHERE id = {id};",
        id = quoted(id),
        text = quoted(&comment_text(content)),
        plain = quoted(content),
        edited = quoted(EDITED),
    )
}

/// A second page, the local user, a thread anchored on block A with one comment and one
/// reaction, and a suggestion on block B.
fn comments_sql() -> String {
    let reaction = derived_key(REACTION_KEY_PREFIX, &["comment-a", PERSON, "+1"]).unwrap();
    format!(
        "INSERT INTO notes_pages (id, parent_type, title, created_time, last_edited_time)
         VALUES ({other}, 'workspace', 'Other', {created}, {created});
         INSERT INTO notes_local_users (id, created_time) VALUES ({person}, {created});
         INSERT INTO notes_comment_threads
             (id, page_id, parent_type, parent_block_id, created_time, last_edited_time)
         VALUES ('thread-a', {page}, 'block_id', {a}, {created}, {created});
         INSERT INTO notes_comment_thread_anchors
             (thread_id, page_id, block_id, start_offset, end_offset, anchor_text, created_time,
              last_edited_time)
         VALUES ('thread-a', {page}, {a}, 0, 5, 'first', {created}, {created});
         {comment}
         INSERT INTO notes_comment_reactions (comment_id, created_by, emoji, sync_key, created_time)
         VALUES ('comment-a', {person}, '+1', {reaction}, {created});
         INSERT INTO notes_suggestions
             (id, page_id, block_id, created_by, range_start, range_end, original_text,
              proposed_text, created_time, last_edited_time)
         VALUES ('suggestion-a', {page}, {b}, {person}, 0, 6, 'second', 'later', {created},
                 {created});",
        other = quoted(OTHER_PAGE),
        person = quoted(PERSON),
        page = quoted(PAGE),
        a = quoted(BLOCK_A),
        b = quoted(BLOCK_B),
        comment = comment_sql("comment-a", "thread-a", "Check this"),
        reaction = quoted(reaction.as_str()),
        created = quoted(CREATED),
    )
}

async fn comments(count: usize) -> Vec<Replica> {
    shared(count, &comments_sql()).await
}

/// The page of the thread, its anchor, and the suggestion.
async fn comment_pages(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT 'thread@' || page_id FROM notes_comment_threads
         UNION ALL SELECT 'anchor@' || page_id FROM notes_comment_thread_anchors
         UNION ALL SELECT 'suggestion@' || page_id FROM notes_suggestions",
    )
    .await
}

async fn single(replica: &mut Replica, sql: &str) -> String {
    let mut values = texts(replica, sql).await;
    assert_eq!(values.len(), 1, "{sql}: {values:?}");
    values.remove(0)
}

#[test]
fn comment_rows_round_trip_and_receivers_fill_their_pages() {
    block_on(async {
        let mut replicas = comments(2).await;
        assert_converged(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(
            comment_pages(reader).await,
            [
                format!("thread@{PAGE}"),
                format!("anchor@{PAGE}"),
                format!("suggestion@{PAGE}")
            ]
        );
        assert_eq!(
            single(
                reader,
                "SELECT plain_text || ' ' || created_by || ' ' || rich_text FROM notes_comments",
            )
            .await,
            format!("Check this {PERSON} {}", comment_text("Check this"))
        );
        assert_eq!(
            single(
                reader,
                "SELECT anchor_text || ' ' || start_offset || '-' || end_offset
                 FROM notes_comment_thread_anchors",
            )
            .await,
            "first 0-5"
        );
        assert_eq!(
            single(reader, "SELECT emoji || ' ' || sync_key FROM notes_comment_reactions").await,
            format!(
                "+1 {}",
                derived_key(REACTION_KEY_PREFIX, &["comment-a", PERSON, "+1"]).unwrap()
            )
        );

        // A block moved to another page takes its thread, anchor, and comments along.
        replicas[0]
            .exec(&format!(
                "UPDATE notes_blocks
                 SET page_id = {other}, parent_page_id = {other}, order_key = {order},
                     last_edited_time = {edited}
                 WHERE id = {a};
                 UPDATE notes_comment_threads SET page_id = {other};
                 UPDATE notes_comment_thread_anchors SET page_id = {other};",
                other = quoted(OTHER_PAGE),
                order = quoted(&rank(1)),
                edited = quoted(EDITED),
                a = quoted(BLOCK_A),
            ))
            .await;
        replicas[1]
            .exec("DELETE FROM notes_page_history_pending;")
            .await;
        converge(&mut replicas).await;
        assert_eq!(
            comment_pages(&mut replicas[1]).await,
            [
                format!("thread@{OTHER_PAGE}"),
                format!("anchor@{OTHER_PAGE}"),
                format!("suggestion@{PAGE}")
            ]
        );
        assert!(
            texts(
                &mut replicas[1],
                "SELECT page_id FROM notes_page_history_pending ORDER BY page_id",
            )
            .await
            .contains(&OTHER_PAGE.to_string())
        );
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn concurrent_comment_edits_and_resolves_both_apply() {
    block_on(async {
        let mut replicas = comments(2).await;
        replicas[0].exec(&edit_sql("comment-a", "Edited")).await;
        replicas[1]
            .exec(&format!(
                "UPDATE notes_comment_threads
                 SET status = 'resolved', resolved_at = {edited}, resolved_by = {person},
                     last_edited_time = {edited}
                 WHERE id = 'thread-a';",
                edited = quoted(EDITED),
                person = quoted(PERSON),
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                single(
                    replica,
                    "SELECT thread.status || ' ' || comment.plain_text
                     FROM notes_comments AS comment
                     JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id",
                )
                .await,
                "resolved Edited",
                "replica {}",
                replica.index
            );
        }

        // Concurrent edits of one body settle on one whole body.
        replicas[0].exec(&edit_sql("comment-a", "Left")).await;
        replicas[1].exec(&edit_sql("comment-a", "Right")).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let body = single(
            &mut replicas[0],
            "SELECT plain_text || ' ' || rich_text FROM notes_comments",
        )
        .await;
        assert!(
            [
                format!("Left {}", comment_text("Left")),
                format!("Right {}", comment_text("Right"))
            ]
            .contains(&body),
            "{body}"
        );
    });
}

#[test]
fn a_comment_delete_and_a_concurrent_edit_both_apply() {
    block_on(async {
        let mut replicas = comments(2).await;
        replicas[0]
            .exec(&format!(
                "UPDATE notes_comments SET deleted_at = {} WHERE id = 'comment-a';",
                quoted(EDITED)
            ))
            .await;
        replicas[1].exec(&edit_sql("comment-a", "Edited")).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                single(
                    replica,
                    "SELECT coalesce(deleted_at, '-') || ' ' || plain_text FROM notes_comments",
                )
                .await,
                format!("{EDITED} Edited"),
                "replica {}",
                replica.index
            );
        }
    });
}

#[test]
fn a_comment_added_on_a_block_deleted_elsewhere_is_hidden_and_offered_for_recovery() {
    block_on(async {
        let mut replicas = comments(2).await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM notes_blocks WHERE id = {};",
                quoted(BLOCK_A)
            ))
            .await;
        replicas[1]
            .exec(&comment_sql("comment-b", "thread-a", "Late reply"))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in replicas.iter_mut() {
            for table in [
                "notes_comment_threads",
                "notes_comment_thread_anchors",
                "notes_comments",
                "notes_comment_reactions",
            ] {
                assert_eq!(
                    replica
                        .count(&format!("SELECT count(*) FROM {table}"))
                        .await,
                    0,
                    "replica {} keeps {table} rows",
                    replica.index
                );
            }
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            let mut offered = false;
            for entry in &entries {
                let descendants = replica
                    .engine
                    .recovery_descendants(&mut replica.conn, entry.table, &entry.row_key)
                    .await
                    .unwrap()
                    .unwrap_or_default();
                offered |= descendants.iter().any(|descendant| {
                    descendant.table == COMMENTS_TABLE && descendant.row_key == "comment-b"
                });
            }
            assert!(offered, "replica {} offers {entries:?}", replica.index);
        }
    });
}

/// SQL setting the text of block B to `content`.
fn block_text_sql(content: &str) -> String {
    let text = serde_json::json!({
        "rich_text": [{
            "type": "text",
            "text": { "content": content },
            "plain_text": content,
        }]
    });
    format!(
        "UPDATE notes_blocks
         SET text = {text}, plain_text = {plain}, last_edited_time = {edited}
         WHERE id = {b};",
        text = quoted(&text.to_string()),
        plain = quoted(content),
        edited = quoted(EDITED),
        b = quoted(BLOCK_B),
    )
}

/// SQL accepting the suggestion, which also writes its proposed text into block B.
fn accept_sql() -> String {
    format!(
        "UPDATE notes_suggestions
         SET status = 'accepted', accepted_at = {edited}, accepted_by = {person},
             last_edited_time = {edited}
         WHERE id = 'suggestion-a';
         {text}",
        edited = quoted(EDITED),
        person = quoted(PERSON),
        text = block_text_sql("later"),
    )
}

/// SQL rejecting the suggestion.
fn reject_sql() -> String {
    format!(
        "UPDATE notes_suggestions
         SET status = 'rejected', rejected_at = {edited}, rejected_by = {person},
             last_edited_time = {edited}
         WHERE id = 'suggestion-a';",
        edited = quoted("2027-01-03T00:00:00.000Z"),
        person = quoted(PERSON),
    )
}

async fn suggestion_status(replica: &mut Replica) -> String {
    single(
        replica,
        "SELECT status || ' ' || coalesce(accepted_by, '-') || ' ' || coalesce(rejected_by, '-')
         FROM notes_suggestions",
    )
    .await
}

#[test]
fn a_suggestion_accept_and_a_concurrent_block_edit_both_apply() {
    block_on(async {
        let mut replicas = comments(2).await;
        replicas[0].exec(&accept_sql()).await;
        replicas[1].exec(&block_text_sql("second edited")).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        // The accept stands while the block text settles on one whole register value.
        let text = single(
            &mut replicas[0],
            &format!(
                "SELECT plain_text FROM notes_blocks WHERE id = {}",
                quoted(BLOCK_B)
            ),
        )
        .await;
        assert!(["later", "second edited"].contains(&text.as_str()), "{text}");
        for replica in replicas.iter_mut() {
            assert_eq!(
                suggestion_status(replica).await,
                format!("accepted {PERSON} -"),
                "replica {}",
                replica.index
            );
        }
    });
}

#[test]
fn an_accepted_suggestion_outranks_a_concurrent_reject() {
    block_on(async {
        let mut replicas = comments(2).await;
        replicas[0].exec(&accept_sql()).await;
        replicas[1].exec(&reject_sql()).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                suggestion_status(replica).await,
                format!("accepted {PERSON} -"),
                "replica {}",
                replica.index
            );
        }
    });
}

#[test]
fn a_crash_while_sealing_or_applying_comment_rows_leaves_nothing_partial() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0].exec(&comments_sql()).await;
        for name in ["seal.reserve", "seal.before_commit"] {
            let report = after_rollback(
                &mut replicas[0],
                name,
                async |r: &mut Replica| r.seal().await,
            )
            .await;
            assert_eq!(report.sealed_ops, 1, "{name}: the rows seal together");
            if name == "seal.reserve" {
                replicas[0].exec(&edit_sql("comment-a", "Edited")).await;
            }
        }

        let (author, rest) = replicas.split_at_mut(1);
        let reader = &mut rest[0];
        let known = reader.stored_vector().await;
        let ops = author[0].ops_since(&known).await;
        reader.receive(&ops).await;
        assert_eq!(reader.count("SELECT count(*) FROM notes_comments").await, 0);
        after_rollback(reader, "apply.before_commit", async |r: &mut Replica| {
            r.apply_all().await
        })
        .await;
        assert_eq!(
            comment_pages(reader).await,
            [
                format!("thread@{PAGE}"),
                format!("anchor@{PAGE}"),
                format!("suggestion@{PAGE}")
            ]
        );
        assert_eq!(
            single(reader, "SELECT plain_text FROM notes_comments").await,
            "Edited"
        );
        assert_converged(&mut replicas).await;
    });
}
