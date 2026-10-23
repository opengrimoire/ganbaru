//! Publication predicates, rows published with their parent, and capture seeding.

use ganbaru_sync_contracts::TableId;
use sqlx::Connection;

use super::block_on;
use super::fixture::{
    DRAFT_PARTS_TABLE, DRAFTS_TABLE, FIXTURE_TABLE_IDS, ITEMS_TABLE, MEMOS_TABLE,
};
use super::replica::{Replica, converge};
use crate::{SyncError, seed};

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 3).await);
    }
    replicas
}

/// Captured fixture rows as `table key created mask`, in table and key order. Vault domain
/// rows every vault seeds, such as the Music soundscape state, are left out.
async fn captures(replica: &mut Replica) -> Vec<String> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT table_id || ' ' || row_key || ' ' || created || ' ' || mask FROM sync_capture
         WHERE table_id >= ? ORDER BY table_id, row_key",
    )
    .bind(i64::from(FIXTURE_TABLE_IDS))
    .fetch_all(&mut replica.conn)
    .await
    .unwrap();
    rows
}

async fn ids(replica: &mut Replica, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut replica.conn)
        .await
        .unwrap()
}

#[test]
fn a_draft_publishes_with_its_parts_and_never_becomes_a_draft_again() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let author = &mut replicas[0];
        author
            .exec(
                "INSERT INTO fixture_drafts (id, title, published) VALUES ('draft-1', 'One', 0);
                 INSERT INTO fixture_draft_parts (id, draft_id, body)
                     VALUES ('part-1', 'draft-1', 'body');
                 UPDATE fixture_drafts SET title = 'Edited' WHERE id = 'draft-1';
                 UPDATE fixture_draft_parts SET body = 'edited' WHERE id = 'part-1';
                 INSERT INTO fixture_drafts (id, title, published) VALUES ('draft-2', 'Two', 0);
                 INSERT INTO fixture_draft_parts (id, draft_id, body)
                     VALUES ('part-2', 'draft-2', 'body');
                 DELETE FROM fixture_drafts WHERE id = 'draft-2';",
            )
            .await;
        assert!(
            captures(author).await.is_empty(),
            "a draft and its parts stay local"
        );

        author
            .exec("UPDATE fixture_drafts SET published = 1 WHERE id = 'draft-1'")
            .await;
        let created = |table: TableId, key: &str| format!("{} {key} 1 3", table.0);
        assert_eq!(
            captures(author).await,
            vec![
                created(DRAFTS_TABLE, "draft-1"),
                created(DRAFT_PARTS_TABLE, "part-1"),
            ],
            "publishing captures the draft and its parts with every group"
        );
        assert!(
            !author
                .try_exec("UPDATE fixture_drafts SET published = 0 WHERE id = 'draft-1'")
                .await,
            "a published draft cannot become a draft again"
        );
        author
            .exec(
                "INSERT INTO fixture_draft_parts (id, draft_id, body)
                     VALUES ('part-3', 'draft-1', 'later');",
            )
            .await;
        assert!(
            captures(author)
                .await
                .contains(&created(DRAFT_PARTS_TABLE, "part-3")),
            "a part added to a published draft publishes at once"
        );

        author.seal().await.unwrap();
        converge(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(
            ids(reader, "SELECT id || ' ' || title FROM fixture_drafts").await,
            vec!["draft-1 Edited"]
        );
        assert_eq!(
            ids(
                reader,
                "SELECT id || ' ' || body FROM fixture_draft_parts ORDER BY id"
            )
            .await,
            vec!["part-1 edited", "part-3 later"]
        );

        reader
            .exec("DELETE FROM fixture_drafts WHERE id = 'draft-1'")
            .await;
        reader.seal().await.unwrap();
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM fixture_draft_parts")
                    .await,
                0
            );
        }
    });
}

#[test]
fn seeding_captures_published_rows_once_per_table() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let seeder = &mut replicas[0];
        seeder
            .exec(
                "INSERT INTO fixture_drafts (id, title, published) VALUES
                     ('draft-1', 'Sent', 1), ('draft-2', 'Unsent', 0);
                 INSERT INTO fixture_draft_parts (id, draft_id, body) VALUES
                     ('part-1', 'draft-1', 'sent'), ('part-2', 'draft-2', 'unsent');
                 INSERT INTO fixture_memos (id, label) VALUES ('memo-1', 'Memo');
                 DELETE FROM sync_capture;",
            )
            .await;
        let now = seeder.now_ms;
        let seeded = seeder
            .engine
            .seed_captures(&mut seeder.conn, now)
            .await
            .unwrap();
        for table in ["fixture_items", "fixture_drafts", "fixture_draft_parts"] {
            assert!(seeded.contains(&table), "{table} should be seeded");
        }
        assert!(
            !seeded.contains(&"quick_notes"),
            "the baseline records tables replicated from the start"
        );
        assert_eq!(
            captures(seeder).await,
            vec![
                format!("{} draft-1 1 3", DRAFTS_TABLE.0),
                format!("{} part-1 1 3", DRAFT_PARTS_TABLE.0),
                format!("{} memo-1 1 1", MEMOS_TABLE.0),
            ],
            "only published rows are seeded"
        );

        seeder.exec("DELETE FROM sync_capture").await;
        let again = seeder
            .engine
            .seed_captures(&mut seeder.conn, now)
            .await
            .unwrap();
        assert!(again.is_empty());
        assert!(captures(seeder).await.is_empty());

        seeder
            .exec(&format!(
                "DELETE FROM sync_seeded_tables WHERE table_id = {}",
                DRAFTS_TABLE.0
            ))
            .await;
        let reseeded = seeder
            .engine
            .seed_captures(&mut seeder.conn, now)
            .await
            .unwrap();
        assert_eq!(reseeded, vec!["fixture_drafts"]);
        assert_eq!(
            captures(seeder).await,
            vec![format!("{} draft-1 1 3", DRAFTS_TABLE.0)]
        );

        seeder
            .exec(&format!(
                "DELETE FROM sync_seeded_tables WHERE table_id IN ({}, {})",
                DRAFT_PARTS_TABLE.0, MEMOS_TABLE.0
            ))
            .await;
        seeder
            .engine
            .seed_captures(&mut seeder.conn, now)
            .await
            .unwrap();
        assert_eq!(captures(seeder).await.len(), 3);
        seeder.seal().await.unwrap();
        converge(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(
            ids(reader, "SELECT id FROM fixture_drafts").await,
            vec!["draft-1"]
        );
        assert_eq!(
            ids(reader, "SELECT id FROM fixture_draft_parts").await,
            vec!["part-1"]
        );
        assert_eq!(
            ids(reader, "SELECT id FROM fixture_memos").await,
            vec!["memo-1"]
        );
    });
}

#[test]
fn seeding_refuses_a_pending_key_and_records_nothing() {
    block_on(async {
        let mut replica = Replica::new(0, 3).await;
        replica
            .exec(&format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('pending:item', '2027-01-01T00:00:00.000Z', 'Pending', 0);
                 DELETE FROM sync_seeded_tables WHERE table_id >= {}",
                ITEMS_TABLE.0
            ))
            .await;
        let now = replica.now_ms;
        let refused = replica.engine.seed_captures(&mut replica.conn, now).await;
        assert!(
            matches!(refused, Err(SyncError::PendingKey("fixture_items"))),
            "{refused:?}"
        );
        assert_eq!(
            replica
                .count(&format!(
                    "SELECT count(*) FROM sync_seeded_tables WHERE table_id >= {}",
                    ITEMS_TABLE.0
                ))
                .await,
            0,
            "a refused seeding records no table"
        );

        // A conversion pass replaces the placeholder and seeds in its own transaction.
        let manifest = replica.engine.manifest();
        let mut tx = replica.conn.begin().await.unwrap();
        sqlx::raw_sql(
            "DELETE FROM fixture_items WHERE id = 'pending:item';
             INSERT INTO fixture_items (id, created_at, label, label_edits)
                 VALUES ('item-1', '2027-01-01T00:00:00.000Z', 'Pending', 0);",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        let seeded = seed::seed_captures(&mut tx, manifest, now).await.unwrap();
        tx.commit().await.unwrap();
        assert!(seeded.contains(&"fixture_items"));
        assert!(
            captures(&mut replica)
                .await
                .contains(&format!("{} item-1 1 3", ITEMS_TABLE.0))
        );
    });
}
