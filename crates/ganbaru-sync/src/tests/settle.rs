//! Hooks that settle concurrent versions of a group: an inheriting approval override ranked
//! least strict that resolves stricter than a concurrent explicit one is settled on the
//! stricter value by every device that decides both, each device writes at most one settle,
//! equal settles converge quietly, and a crash before the settle is sealed loses nothing.

use sqlx::SqliteConnection;

use super::block_on;
use super::fixture::{
    ASK, LISTS_TABLE, OVERRIDES_TABLE, OverrideHook, TEST_MANIFEST, UNATTENDED, list_group,
    override_group, test_adapters, test_engine_with,
};
use super::replica::{Replica, connection_from, converge, pair, quoted};
use crate::error::failpoints;
use crate::hooks::{ChangeSink, HookFuture, Hooks, MaterializeHook};
use crate::manifest::TableSpec;
use crate::{ApplyReport, Engine, SyncError, local};

const CREATED: &str = "'2027-01-01T00:00:00.000Z'";
const KEY: &str = "o-1";

/// Two replicas sharing override [`KEY`] with `fallback` and `policy` (`None` inherits).
async fn shared(fallback: &str, policy: Option<&str>) -> Vec<Replica> {
    let mut replicas = vec![Replica::new(0, 31).await, Replica::new(1, 31).await];
    replicas[0]
        .exec(&format!(
            "INSERT INTO fixture_overrides (id, created_at, fallback, policy)
             VALUES ({key}, {CREATED}, {fallback}, {policy})",
            key = quoted(KEY),
            fallback = quoted(fallback),
            policy = policy_sql(policy),
        ))
        .await;
    converge(&mut replicas).await;
    replicas
}

fn policy_sql(policy: Option<&str>) -> String {
    policy.map_or_else(|| "NULL".to_string(), quoted)
}

/// Sets the override policy of [`KEY`] locally and seals it.
async fn set_policy(replica: &mut Replica, policy: Option<&str>) {
    replica
        .exec(&format!(
            "UPDATE fixture_overrides SET policy = {} WHERE id = {}",
            policy_sql(policy),
            quoted(KEY)
        ))
        .await;
    assert_eq!(replica.seal().await.unwrap().sealed_ops, 1);
}

/// The materialized policy of [`KEY`]; `None` inherits.
async fn policy(replica: &mut Replica) -> Option<String> {
    sqlx::query_scalar("SELECT policy FROM fixture_overrides WHERE id = ?")
        .bind(KEY)
        .fetch_one(&mut replica.conn)
        .await
        .unwrap()
}

async fn pending(replica: &mut Replica) -> u64 {
    local::pending_captures(&mut replica.conn).await.unwrap()
}

/// Retained policy versions of [`KEY`] and how many distinct values they hold.
async fn policy_versions(replica: &mut Replica) -> (i64, i64) {
    let versions = replica
        .count(&format!(
            "SELECT count(*) FROM sync_register_versions
             WHERE table_id = {} AND row_key = {} AND group_id = {}",
            OVERRIDES_TABLE.0,
            quoted(KEY),
            override_group::POLICY.get()
        ))
        .await;
    let values = replica
        .count(&format!(
            "SELECT count(DISTINCT value_hash) FROM sync_register_versions
             WHERE table_id = {} AND row_key = {} AND group_id = {}",
            OVERRIDES_TABLE.0,
            quoted(KEY),
            override_group::POLICY.get()
        ))
        .await;
    (versions, values)
}

/// Stores what `from` has and `to` lacks and applies it; returns the apply reports.
async fn pull(replicas: &mut [Replica], to: usize, from: usize) -> Vec<ApplyReport> {
    let (to, from) = pair(replicas, to, from);
    let known = to.stored_vector().await;
    let ops = from.ops_since(&known).await;
    to.receive(&ops).await;
    to.apply_all().await.unwrap()
}

async fn assert_settled_everywhere(replicas: &mut [Replica], expected: &str) {
    for replica in replicas.iter_mut() {
        assert_eq!(policy(replica).await.as_deref(), Some(expected));
        assert_eq!(pending(replica).await, 0);
        assert_eq!(policy_versions(replica).await.1, 1);
        replica.check_invariants().await;
        replica.check_version_values().await;
    }
    let first = replicas[0].dump().await;
    assert_eq!(replicas[1].dump().await, first);
}

/// Replica 0 clears the override, inheriting a stricter fallback, while replica 1 sets a looser
/// explicit one, which the static priority ranks first.
async fn concurrent_clear_and_loose_set() -> Vec<Replica> {
    let mut replicas = shared(ASK, Some(ASK)).await;
    set_policy(&mut replicas[0], None).await;
    set_policy(&mut replicas[1], Some(UNATTENDED)).await;
    replicas
}

#[test]
fn both_devices_settle_a_looser_winner_once_and_their_equal_settles_converge_quietly() {
    block_on(async {
        let mut replicas = concurrent_clear_and_loose_set().await;

        let reports = pull(&mut replicas, 0, 1).await;
        assert!(reports.iter().any(|report| {
            report
                .notices
                .iter()
                .any(|notice| notice.key == KEY && notice.topic == super::fixture::OVERRIDE_TOPIC)
        }));
        assert_eq!(policy(&mut replicas[0]).await.as_deref(), Some(ASK));
        assert_eq!(pending(&mut replicas[0]).await, 1);
        assert_eq!(policy_versions(&mut replicas[0]).await, (2, 2));
        replicas[0].check_version_values().await;

        // Replica 1 decides the same versions, its domain row unchanged, and settles too.
        pull(&mut replicas, 1, 0).await;
        assert_eq!(policy(&mut replicas[1]).await.as_deref(), Some(ASK));
        assert_eq!(pending(&mut replicas[1]).await, 1);

        assert_eq!(replicas[0].seal().await.unwrap().sealed_ops, 1);
        assert_eq!(replicas[1].seal().await.unwrap().sealed_ops, 1);
        pull(&mut replicas, 0, 1).await;
        pull(&mut replicas, 1, 0).await;
        assert_eq!(policy_versions(&mut replicas[0]).await, (2, 1));
        for replica in replicas.iter_mut() {
            assert_eq!(replica.seal().await.unwrap().sealed_ops, 0);
        }
        assert_settled_everywhere(&mut replicas, ASK).await;

        // Applying again changes nothing and seals nothing.
        for replica in replicas.iter_mut() {
            let reports = replica.apply_all().await.unwrap();
            assert!(
                reports
                    .iter()
                    .all(|report| report.changed_tables.is_empty())
            );
            assert_eq!(replica.seal().await.unwrap().sealed_ops, 0);
        }
        converge(&mut replicas).await;
        assert_settled_everywhere(&mut replicas, ASK).await;
    });
}

#[test]
fn a_settle_that_reaches_the_other_device_first_leaves_it_nothing_to_settle() {
    block_on(async {
        let mut replicas = concurrent_clear_and_loose_set().await;
        pull(&mut replicas, 0, 1).await;
        assert_eq!(replicas[0].seal().await.unwrap().sealed_ops, 1);

        pull(&mut replicas, 1, 0).await;
        assert_eq!(pending(&mut replicas[1]).await, 0);
        assert_eq!(replicas[1].seal().await.unwrap().sealed_ops, 0);
        assert_eq!(policy_versions(&mut replicas[1]).await, (1, 1));
        pull(&mut replicas, 0, 1).await;
        assert_settled_everywhere(&mut replicas, ASK).await;
    });
}

#[test]
fn an_explicit_winner_stricter_than_the_inherited_value_needs_no_settle() {
    block_on(async {
        let mut replicas = shared(UNATTENDED, Some(UNATTENDED)).await;
        set_policy(&mut replicas[0], None).await;
        set_policy(&mut replicas[1], Some(ASK)).await;
        pull(&mut replicas, 0, 1).await;
        pull(&mut replicas, 1, 0).await;
        for replica in replicas.iter_mut() {
            assert_eq!(pending(replica).await, 0);
            assert_eq!(replica.seal().await.unwrap().sealed_ops, 0);
            assert_eq!(policy(replica).await.as_deref(), Some(ASK));
            // The losing inherit stays retained; deciding it again settles nothing.
            assert_eq!(policy_versions(replica).await, (2, 2));
            replica.apply_all().await.unwrap();
            assert_eq!(pending(replica).await, 0);
        }
        let first = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, first);
    });
}

#[test]
fn repeated_applies_of_the_same_versions_settle_once() {
    block_on(async {
        let mut replicas = concurrent_clear_and_loose_set().await;
        pull(&mut replicas, 0, 1).await;
        let before = replicas[0].dump().await;

        let reports = replicas[0].apply_all().await.unwrap();
        assert!(
            reports
                .iter()
                .all(|report| report.changed_tables.is_empty())
        );
        let ops = replicas[1].own_ops().await;
        replicas[0].receive(&ops).await;
        replicas[0].apply_all().await.unwrap();
        assert_eq!(replicas[0].dump().await, before);
        assert_eq!(pending(&mut replicas[0]).await, 1);
        assert_eq!(replicas[0].seal().await.unwrap().sealed_ops, 1);
        assert_eq!(replicas[0].seal().await.unwrap().sealed_ops, 0);
    });
}

#[test]
fn a_crash_before_the_settle_is_sealed_leaves_it_for_the_next_pass() {
    block_on(async {
        let mut replicas = concurrent_clear_and_loose_set().await;
        let known = replicas[0].stored_vector().await;
        let ops = replicas[1].ops_since(&known).await;
        replicas[0].receive(&ops).await;

        // A crash inside the apply rolls the settle back with the merge.
        failpoints::arm("apply.before_commit");
        let result = replicas[0].apply_once().await;
        failpoints::clear();
        assert!(matches!(result, Err(SyncError::Failpoint(_))));
        assert_eq!(policy(&mut replicas[0]).await, None);
        assert_eq!(pending(&mut replicas[0]).await, 0);

        replicas[0].apply_all().await.unwrap();
        assert_eq!(pending(&mut replicas[0]).await, 1);

        // A restart after the apply committed keeps the settle captured.
        let bytes = replicas[0].snapshot().await;
        replicas[0].conn = connection_from(&bytes).await;
        assert_eq!(policy(&mut replicas[0]).await.as_deref(), Some(ASK));
        failpoints::arm("seal.before_commit");
        let result = replicas[0].seal().await;
        failpoints::clear();
        assert!(matches!(result, Err(SyncError::Failpoint(_))));
        assert_eq!(pending(&mut replicas[0]).await, 1);
        assert_eq!(replicas[0].seal().await.unwrap().sealed_ops, 1);

        converge(&mut replicas).await;
        assert_settled_everywhere(&mut replicas, ASK).await;
    });
}

#[test]
fn a_device_that_may_not_edit_the_table_leaves_the_settle_to_one_that_may() {
    block_on(async {
        let mut replicas = concurrent_clear_and_loose_set().await;
        replicas[1]
            .exec(
                "CREATE TEMP TRIGGER sync_guard_fixture_overrides_update
                 BEFORE UPDATE ON main.fixture_overrides
                 WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) IS NOT 1
                 BEGIN SELECT RAISE(ABORT, 'read-only'); END;",
            )
            .await;
        pull(&mut replicas, 1, 0).await;
        assert_eq!(pending(&mut replicas[1]).await, 0);
        assert_eq!(policy(&mut replicas[1]).await.as_deref(), Some(UNATTENDED));

        pull(&mut replicas, 0, 1).await;
        assert_eq!(replicas[0].seal().await.unwrap().sealed_ops, 1);
        pull(&mut replicas, 1, 0).await;
        assert_settled_everywhere(&mut replicas, ASK).await;
    });
}

#[test]
fn a_settled_value_that_breaks_the_group_rules_is_reported_and_not_written() {
    block_on(async {
        let mut replicas = concurrent_clear_and_loose_set().await;
        replicas[0].engine = test_engine_with(OverrideHook {
            replacement: Some("never"),
        });
        let reports = pull(&mut replicas, 0, 1).await;
        let refused: Vec<_> = reports
            .iter()
            .filter_map(|report| report.seal.as_ref())
            .flat_map(|seal| &seal.invalid_rows)
            .collect();
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].table, OVERRIDES_TABLE);
        assert_eq!(refused[0].row_key, KEY);
        assert_eq!(policy(&mut replicas[0]).await.as_deref(), Some(UNATTENDED));
        assert_eq!(pending(&mut replicas[0]).await, 0);
    });
}

/// A hook that names the owned entries group of lists as settled.
struct SettlesOwnedEntries;

impl MaterializeHook for SettlesOwnedEntries {
    fn after_materialize<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        _changed_keys: &'a [String],
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn settled_groups(
        &self,
        _table: &'static TableSpec,
    ) -> &'static [ganbaru_sync_contracts::GroupId] {
        &[list_group::ENTRIES]
    }
}

#[test]
fn an_engine_refuses_a_hook_that_settles_a_group_with_owned_tables() {
    let hooks = Hooks::new().with(LISTS_TABLE, Box::new(SettlesOwnedEntries));
    match Engine::with(&TEST_MANIFEST, test_adapters(), hooks) {
        Err(SyncError::InvalidManifest(message)) => {
            assert!(message.contains("owned tables"), "{message}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}
