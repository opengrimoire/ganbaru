//! Operations whose bytes pass validation and whose domain write a constraint refuses: apply
//! refuses one row, keeps going, and leaves the merge state equal on every replica. A writer
//! stands for another build by not enforcing the receiver's constraint, so its operations are
//! validly sealed and its merge state is the one the receiver must reach.

use crate::apply::MAX_REFUSED_RETRIES_PER_FINISH;
use crate::{ApplyReport, SyncResult};

use super::block_on;
use super::fixture::{ASK, FORBIDDEN, REFUSED, RESERVED, UNATTENDED};
use super::replica::{Replica, converge, pair, quoted};

const CREATED: &str = "'2027-01-01T00:00:00.000Z'";

/// A replica that does not enforce `CHECK` constraints, so it seals values others refuse.
async fn unchecked(index: usize, salt: u64) -> Replica {
    let mut replica = Replica::new(index, salt).await;
    replica.exec("PRAGMA ignore_check_constraints = ON").await;
    replica
}

/// Stores what `from` has and `to` lacks and returns the apply result.
async fn take(replicas: &mut [Replica], to: usize, from: usize) -> SyncResult<Vec<ApplyReport>> {
    let (to, from) = pair(replicas, to, from);
    let known = to.stored_vector().await;
    let ops = from.ops_since(&known).await;
    to.receive(&ops).await;
    to.apply_all().await
}

fn override_sql(key: &str, fallback: &str, policy: &str) -> String {
    format!(
        "INSERT INTO fixture_overrides (id, created_at, fallback, policy)
         VALUES ({}, {CREATED}, {}, {});",
        quoted(key),
        quoted(fallback),
        quoted(policy)
    )
}

async fn exec_and_seal(replica: &mut Replica, sql: &str) {
    replica.exec(sql).await;
    assert!(replica.seal().await.unwrap().sealed_ops > 0, "{sql}");
}

/// Keys of the rows listed as refused with `kind`.
async fn refused(replica: &mut Replica, kind: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT row_key FROM sync_refused_rows WHERE kind = ? ORDER BY row_key")
        .bind(kind)
        .fetch_all(&mut replica.conn)
        .await
        .unwrap()
}

async fn keys(replica: &mut Replica, table: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM {table} ORDER BY id"
    )))
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
}

/// The merge state a replica compares with the others, leaving out domain rows.
async fn merge_state(replica: &mut Replica) -> Vec<(&'static str, Vec<String>)> {
    let mut dump = replica.dump().await;
    dump.retain(|(table, _)| table.starts_with("sync_"));
    dump
}

#[test]
fn apply_hides_a_row_whose_insert_a_check_refuses() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 61).await, unchecked(1, 61).await];
        exec_and_seal(&mut replicas[1], &override_sql("o-1", ASK, RESERVED)).await;

        let refused_apply = take(&mut replicas, 0, 1).await;
        assert!(
            refused_apply.is_ok(),
            "a refused row must not stop apply: {refused_apply:?}"
        );
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM sync_ops WHERE state <> 'applied'")
                .await,
            0
        );
        assert!(keys(&mut replicas[0], "fixture_overrides").await.is_empty());
        assert_eq!(refused(&mut replicas[0], "hidden").await, ["o-1"]);

        // A later operation in the same space applies.
        exec_and_seal(&mut replicas[1], &override_sql("o-2", ASK, UNATTENDED)).await;
        take(&mut replicas, 0, 1).await.unwrap();
        assert_eq!(keys(&mut replicas[0], "fixture_overrides").await, ["o-2"]);
        let sender = merge_state(&mut replicas[1]).await;
        assert_eq!(merge_state(&mut replicas[0]).await, sender);
        replicas[0].check_invariants().await;
    });
}

#[test]
fn apply_hides_the_children_of_a_refused_insert() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 62).await, unchecked(1, 62).await];
        let refused_parent = format!(
            "INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', {});
             INSERT INTO fixture_cycle_b (id, label, peer_id) VALUES ('b-1', 'child', 'a-1');",
            quoted(REFUSED)
        );
        exec_and_seal(&mut replicas[1], &refused_parent).await;

        take(&mut replicas, 0, 1).await.unwrap();

        assert!(keys(&mut replicas[0], "fixture_cycle_a").await.is_empty());
        assert!(keys(&mut replicas[0], "fixture_cycle_b").await.is_empty());
        assert_eq!(refused(&mut replicas[0], "hidden").await, ["a-1"]);
        let sender = merge_state(&mut replicas[1]).await;
        assert_eq!(merge_state(&mut replicas[0]).await, sender);
    });
}

#[test]
fn a_refused_row_returns_when_a_valid_write_fixes_it() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 63).await, unchecked(1, 63).await];
        let refused_parent = format!(
            "INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', {});
             INSERT INTO fixture_cycle_b (id, label, peer_id) VALUES ('b-1', 'child', 'a-1');",
            quoted(REFUSED)
        );
        exec_and_seal(&mut replicas[1], &refused_parent).await;
        take(&mut replicas, 0, 1).await.unwrap();

        exec_and_seal(
            &mut replicas[1],
            "UPDATE fixture_cycle_a SET label = 'valid' WHERE id = 'a-1'",
        )
        .await;
        take(&mut replicas, 0, 1).await.unwrap();

        assert_eq!(keys(&mut replicas[0], "fixture_cycle_a").await, ["a-1"]);
        assert_eq!(keys(&mut replicas[0], "fixture_cycle_b").await, ["b-1"]);
        assert!(refused(&mut replicas[0], "hidden").await.is_empty());
        let sender = replicas[1].domain_dump().await;
        assert_eq!(replicas[0].domain_dump().await, sender);
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM sync_stale_rows")
                .await,
            0
        );
    });
}

#[test]
fn a_valid_write_brings_back_a_refused_row_past_the_retry_limit() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 64).await, unchecked(1, 64).await];
        let keys_refused: Vec<String> = (0..=MAX_REFUSED_RETRIES_PER_FINISH)
            .map(|index| format!("o-{index:04}"))
            .collect();
        let inserts: String = keys_refused
            .iter()
            .map(|key| override_sql(key, ASK, RESERVED))
            .collect();
        exec_and_seal(&mut replicas[1], &inserts).await;
        take(&mut replicas, 0, 1).await.unwrap();
        assert_eq!(refused(&mut replicas[0], "hidden").await, keys_refused);

        // The retry order reaches this row last.
        let last = keys_refused.last().unwrap();
        exec_and_seal(
            &mut replicas[1],
            &format!(
                "UPDATE fixture_overrides SET policy = {} WHERE id = {}",
                quoted(ASK),
                quoted(last)
            ),
        )
        .await;
        take(&mut replicas, 0, 1).await.unwrap();

        assert_eq!(
            keys(&mut replicas[0], "fixture_overrides").await,
            [last.clone()]
        );
        assert_eq!(
            refused(&mut replicas[0], "hidden").await,
            keys_refused[..MAX_REFUSED_RETRIES_PER_FINISH]
        );
    });
}

#[test]
fn refused_update_hides_the_row_on_replicas_of_different_histories() {
    block_on(async {
        let mut replicas = vec![
            Replica::new(0, 65).await,
            Replica::new(1, 65).await,
            unchecked(2, 65).await,
        ];
        exec_and_seal(&mut replicas[2], &override_sql("o-1", ASK, UNATTENDED)).await;
        take(&mut replicas, 0, 2).await.unwrap();
        take(&mut replicas, 1, 2).await.unwrap();

        exec_and_seal(
            &mut replicas[2],
            "UPDATE fixture_overrides SET fallback = 'unattended' WHERE id = 'o-1'",
        )
        .await;
        take(&mut replicas, 0, 2).await.unwrap();
        exec_and_seal(
            &mut replicas[2],
            &format!(
                "UPDATE fixture_overrides SET policy = {} WHERE id = 'o-1'",
                quoted(RESERVED)
            ),
        )
        .await;
        take(&mut replicas, 0, 2).await.unwrap();
        take(&mut replicas, 1, 2).await.unwrap();

        for index in 0..2 {
            assert!(
                keys(&mut replicas[index], "fixture_overrides")
                    .await
                    .is_empty()
            );
            assert_eq!(refused(&mut replicas[index], "hidden").await, ["o-1"]);
        }
        let other = replicas[1].domain_dump().await;
        assert_eq!(replicas[0].domain_dump().await, other);
        let other = merge_state(&mut replicas[1]).await;
        assert_eq!(merge_state(&mut replicas[0]).await, other);
    });
}

#[test]
fn apply_keeps_a_row_whose_delete_a_trigger_refuses() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 66).await, unchecked(1, 66).await];
        replicas[1]
            .exec("DROP TRIGGER fixture_overrides_keep_forbidden")
            .await;
        exec_and_seal(&mut replicas[1], &override_sql("o-1", ASK, FORBIDDEN)).await;
        take(&mut replicas, 0, 1).await.unwrap();
        assert_eq!(keys(&mut replicas[0], "fixture_overrides").await, ["o-1"]);

        exec_and_seal(
            &mut replicas[1],
            "DELETE FROM fixture_overrides WHERE id = 'o-1'",
        )
        .await;
        take(&mut replicas, 0, 1).await.unwrap();

        assert_eq!(keys(&mut replicas[0], "fixture_overrides").await, ["o-1"]);
        assert_eq!(refused(&mut replicas[0], "kept").await, ["o-1"]);
        assert!(refused(&mut replicas[0], "hidden").await.is_empty());
        let sender = merge_state(&mut replicas[1]).await;
        assert_eq!(merge_state(&mut replicas[0]).await, sender);
    });
}

#[test]
fn move_aside_leaves_no_placeholder_when_an_update_is_refused() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 67).await, unchecked(1, 67).await];
        exec_and_seal(
            &mut replicas[1],
            "INSERT INTO fixture_handles (id, label, code) VALUES ('h-1', 'one', 'x');
             INSERT INTO fixture_handles (id, label, code) VALUES ('h-2', 'two', 'y');",
        )
        .await;
        take(&mut replicas, 0, 1).await.unwrap();

        let swap = format!(
            "UPDATE fixture_handles SET code = 'swapping' WHERE id = 'h-1';
             UPDATE fixture_handles SET code = 'x' WHERE id = 'h-2';
             UPDATE fixture_handles SET code = 'y', label = {} WHERE id = 'h-1';",
            quoted(REFUSED)
        );
        exec_and_seal(&mut replicas[1], &swap).await;
        take(&mut replicas, 0, 1).await.unwrap();

        let codes: Vec<String> = sqlx::query_scalar("SELECT code FROM fixture_handles")
            .fetch_all(&mut replicas[0].conn)
            .await
            .unwrap();
        assert!(
            codes.iter().all(|code| code == "x" || code == "y"),
            "a placeholder stayed behind: {codes:?}"
        );
        assert!(
            refused(&mut replicas[0], "hidden")
                .await
                .contains(&"h-1".to_string())
        );
        let sender = merge_state(&mut replicas[1]).await;
        assert_eq!(merge_state(&mut replicas[0]).await, sender);
    });
}

#[test]
fn refusals_stay_out_of_keepers_and_repairs() {
    block_on(async {
        let mut replicas = vec![
            Replica::new(0, 68).await,
            unchecked(1, 68).await,
            unchecked(2, 68).await,
        ];
        let keeper = format!(
            "INSERT INTO fixture_handles (id, label, code) VALUES ('h-1', 'same', {});",
            quoted(REFUSED)
        );
        exec_and_seal(&mut replicas[2], &keeper).await;
        exec_and_seal(
            &mut replicas[1],
            "INSERT INTO fixture_handles (id, label, code) VALUES ('h-2', 'same', 'other')",
        )
        .await;

        converge(&mut replicas).await;

        assert_eq!(refused(&mut replicas[0], "hidden").await, ["h-1"]);
        assert!(keys(&mut replicas[0], "fixture_handles").await.is_empty());
        assert_eq!(keys(&mut replicas[1], "fixture_handles").await, ["h-1"]);
        let tombstones = "SELECT row_key || ':' || replaced_by FROM sync_tombstones
                          WHERE replaced_by IS NOT NULL";
        for replica in &mut replicas {
            assert_eq!(
                replica.scalar_text(tombstones).await.as_deref(),
                Some("h-2:h-1")
            );
        }
        let other = merge_state(&mut replicas[1]).await;
        assert_eq!(merge_state(&mut replicas[0]).await, other);
    });
}

#[test]
fn refused_rows_do_not_keep_apply_rounds_running() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 69).await, unchecked(1, 69).await];
        exec_and_seal(&mut replicas[1], &override_sql("o-1", ASK, RESERVED)).await;

        let reports = take(&mut replicas, 0, 1).await.unwrap();

        assert!(!reports.last().unwrap().more);
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM sync_stale_rows")
                .await,
            0
        );
        let idle = replicas[0].apply_once().await.unwrap();
        assert!(!idle.more);
        assert_eq!(refused(&mut replicas[0], "hidden").await, ["o-1"]);
    });
}
