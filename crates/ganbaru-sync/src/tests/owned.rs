//! Owned tables, nested owned rows, owned references, hook columns, and key-derived columns
//! across replicas.

use ganbaru_sync_contracts::bounds::MAX_BLOB_FIELD_BYTES;

use super::block_on;
use super::fixture::{LIST_COUNT_TOPIC, SCROLL_PAGES, TEST_MANIFEST, membership_key};
use super::replica::{Replica, converge, pair};
use crate::ChangeNotice;
use crate::manifest::{OwnedBound, OwnedTable, owned_table_problems};

const CREATED: &str = "'2027-01-01T00:00:00.000Z'";

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 7).await);
    }
    replicas
}

/// Runs a local edit with the fixture domain's follow-up, which must commit.
async fn edit(replica: &mut Replica, sql: &str) {
    assert!(replica.try_exec(sql).await, "{sql}");
}

async fn texts(replica: &mut Replica, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut replica.conn)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

/// Stores what `from` has and `to` lacks and applies it; returns the notices hooks reported.
async fn pull_notices(replicas: &mut [Replica], to: usize, from: usize) -> Vec<ChangeNotice> {
    let (to, from) = pair(replicas, to, from);
    let known = to.stored_vector().await;
    let ops = from.ops_since(&known).await;
    to.receive(&ops).await;
    to.apply_all()
        .await
        .unwrap()
        .into_iter()
        .flat_map(|report| report.notices)
        .collect()
}

fn list_notice(key: &str) -> ChangeNotice {
    ChangeNotice {
        topic: LIST_COUNT_TOPIC,
        key: key.into(),
    }
}

/// Visible entries of `list-1` in order, as `ordinal:entry_id`.
async fn entries(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT ordinal || ':' || entry_id FROM fixture_list_entries
         WHERE list_id = 'list-1' ORDER BY ordinal",
    )
    .await
}

/// The hook column and count row of `list-1`.
async fn list_counts(replica: &mut Replica) -> (i64, i64) {
    let column = replica
        .count("SELECT entry_count FROM fixture_lists WHERE id = 'list-1'")
        .await;
    let row = replica
        .count("SELECT entries FROM fixture_list_counts WHERE list_id = 'list-1'")
        .await;
    (column, row)
}

#[test]
fn a_remote_list_edit_recomputes_the_hook_column_and_reports_it() {
    block_on(async {
        let mut replicas = replicas(2).await;
        edit(
            &mut replicas[0],
            &format!(
                "INSERT INTO fixture_lists (id, created_at, title) VALUES ('list-1', {CREATED}, 'L');
                 INSERT INTO fixture_list_entries (list_id, ordinal, entry_id, body)
                     VALUES ('list-1', 0, 'e-1', 'one'), ('list-1', 1, 'e-2', 'two');"
            ),
        )
        .await;
        replicas[0].seal().await.unwrap();
        assert_eq!(
            pull_notices(&mut replicas, 1, 0).await,
            [list_notice("list-1")]
        );
        assert_eq!(list_counts(&mut replicas[1]).await, (2, 2));

        edit(
            &mut replicas[1],
            "DELETE FROM fixture_list_entries WHERE entry_id = 'e-1'",
        )
        .await;
        replicas[1].seal().await.unwrap();
        assert_eq!(
            pull_notices(&mut replicas, 0, 1).await,
            [list_notice("list-1")]
        );
        assert_eq!(entries(&mut replicas[0]).await, ["0:e-2"]);
        assert_eq!(list_counts(&mut replicas[0]).await, (1, 1));

        // The hook column is never captured: writing it seals nothing.
        replicas[0]
            .exec("UPDATE fixture_lists SET entry_count = 9 WHERE id = 'list-1'")
            .await;
        assert_eq!(replicas[0].seal().await.unwrap().sealed_ops, 0);
    });
}

#[test]
fn an_owned_entry_is_skipped_while_its_membership_is_deleted_and_returns_with_it() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let member = membership_key("item-1", "a-1");
        let add_member = format!(
            "INSERT INTO fixture_memberships (item_id, end_id, sync_key, created_at, role)
             VALUES ('item-1', 'a-1', '{member}', {CREATED}, 'member')"
        );
        edit(
            &mut replicas[0],
            &format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('item-1', {CREATED}, 'One', 0);
                 INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'A');
                 {add_member};
                 INSERT INTO fixture_lists (id, created_at, title) VALUES ('list-1', {CREATED}, 'L');
                 INSERT INTO fixture_list_entries (list_id, ordinal, entry_id, body)
                     VALUES ('list-1', 0, 'e-1', 'one');"
            ),
        )
        .await;
        converge(&mut replicas).await;

        // One device adds an entry naming the membership while another removes it. The entry
        // comes first, so skipping it renumbers the entry after it.
        edit(
            &mut replicas[0],
            &format!(
                "UPDATE fixture_list_entries SET ordinal = 1 WHERE entry_id = 'e-1';
                 INSERT INTO fixture_list_entries (list_id, ordinal, entry_id, body, member_key)
                 VALUES ('list-1', 0, 'e-2', 'two', '{member}')"
            ),
        )
        .await;
        edit(
            &mut replicas[1],
            &format!("DELETE FROM fixture_memberships WHERE sync_key = '{member}'"),
        )
        .await;
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(entries(replica).await, ["0:e-1"]);
            assert_eq!(list_counts(replica).await, (1, 1));
        }

        // The entry stays in the list value and returns with its membership.
        edit(&mut replicas[1], &add_member).await;
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(entries(replica).await, ["0:e-2", "1:e-1"]);
            assert_eq!(list_counts(replica).await, (2, 2));
        }
    });
}

#[test]
fn nested_checks_follow_their_card_ordinal_and_keep_their_ids() {
    block_on(async {
        let mut replicas = replicas(2).await;
        edit(
            &mut replicas[0],
            &format!(
                "INSERT INTO fixture_boards (id, created_at, stage, handle)
                     VALUES ('board-1', {CREATED}, 0, 'board:board-1');
                 INSERT INTO fixture_board_cards (board_id, ordinal, card_id, title)
                     VALUES ('board-1', 0, 'c-1', 'First'), ('board-1', 1, 'c-2', 'Second');
                 INSERT INTO fixture_board_checks (board_id, card_ordinal, ordinal, check_id, done)
                     VALUES ('board-1', 0, 0, 'k-1', 0), ('board-1', 0, 1, 'k-2', 1),
                            ('board-1', 1, 0, 'k-3', 0);"
            ),
        )
        .await;
        converge(&mut replicas).await;
        // Materialize writes the key-derived handle the schema pins to the row key.
        assert_eq!(
            texts(&mut replicas[1], "SELECT handle FROM fixture_boards").await,
            ["board:board-1"]
        );
        assert!(
            !replicas[1]
                .try_exec("UPDATE fixture_boards SET handle = 'board:other'")
                .await
        );

        // Swapping the cards moves their checks through the cascading parent ordinal.
        edit(
            &mut replicas[0],
            "UPDATE fixture_board_cards SET ordinal = -1 WHERE card_id = 'c-1';
             UPDATE fixture_board_cards SET ordinal = 0 WHERE card_id = 'c-2';
             UPDATE fixture_board_cards SET ordinal = 1 WHERE card_id = 'c-1';",
        )
        .await;
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT card.card_id || ':' || ordinal FROM fixture_board_cards AS card
                     WHERE board_id = 'board-1' ORDER BY ordinal",
                )
                .await,
                ["c-2:0", "c-1:1"]
            );
            assert_eq!(
                texts(
                    replica,
                    "SELECT card.card_id || ':' || check_row.ordinal || ':' || check_row.check_id
                         || ':' || check_row.done
                     FROM fixture_board_checks AS check_row
                     JOIN fixture_board_cards AS card
                         ON card.board_id = check_row.board_id
                         AND card.ordinal = check_row.card_ordinal
                     ORDER BY check_row.card_ordinal, check_row.ordinal",
                )
                .await,
                ["c-2:0:k-3:0", "c-1:0:k-1:0", "c-1:1:k-2:1"]
            );
        }
    });
}

#[test]
fn only_a_domain_bound_lets_an_owned_table_exceed_the_static_blob_bound() {
    assert!(SCROLL_PAGES.max_encoded_bytes() > MAX_BLOB_FIELD_BYTES);
    assert!(owned_table_problems(&SCROLL_PAGES, false).is_empty());
    let static_pages = OwnedTable {
        bound: OwnedBound::Static,
        ..SCROLL_PAGES
    };
    assert_eq!(
        owned_table_problems(&static_pages, false),
        ["the largest encoding exceeds the blob field bound"]
    );
    assert!(TEST_MANIFEST.validate().is_ok());
    assert!(TEST_MANIFEST.summary().contains(
        "owned fixture_scroll_pages parent scroll_id ordinal ordinal rows 4 codec generic \
         body:text(..=262144) bound domain"
    ));
}

#[test]
fn an_oversized_domain_bounded_value_stays_captured_and_is_never_published() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let page = MAX_BLOB_FIELD_BYTES / 2;
        edit(
            &mut replicas[0],
            &format!(
                "INSERT INTO fixture_scrolls (id, created_at) VALUES ('scroll-1', {CREATED});
                 INSERT INTO fixture_scroll_pages (scroll_id, ordinal, body)
                     VALUES ('scroll-1', 0, printf('%.*c', {page}, 'a')),
                            ('scroll-1', 1, printf('%.*c', {page}, 'b'));"
            ),
        )
        .await;
        let report = replicas[0].seal().await.unwrap();
        assert_eq!(
            report
                .invalid_rows
                .iter()
                .map(|row| (row.row_key.as_str(), row.reason))
                .collect::<Vec<_>>(),
            [("scroll-1", "value bounds")]
        );
        assert_eq!(report.changes, 0);
        assert_eq!(replicas[0].count("SELECT count(*) FROM sync_rows").await, 0);
        assert_eq!(
            replicas[0].count("SELECT count(*) FROM sync_capture").await,
            1
        );

        // Within the bound, the same row publishes and reaches the other replica.
        edit(
            &mut replicas[0],
            "DELETE FROM fixture_scroll_pages WHERE ordinal = 1",
        )
        .await;
        let report = replicas[0].seal().await.unwrap();
        assert!(report.invalid_rows.is_empty(), "{:?}", report.invalid_rows);
        converge(&mut replicas).await;
        assert_eq!(
            replicas[1]
                .count("SELECT length(body) FROM fixture_scroll_pages WHERE scroll_id = 'scroll-1'")
                .await,
            i64::try_from(page).unwrap()
        );
    });
}
