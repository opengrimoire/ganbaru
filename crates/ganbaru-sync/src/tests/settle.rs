//! Hooks that settle concurrent versions of a group: an inheriting approval override ranked
//! least strict that resolves stricter than a concurrent explicit one is settled on the
//! stricter value by every device that decides both, each device writes at most one settle,
//! equal settles converge quietly, and a crash before the settle is sealed loses nothing. A
//! device that cannot seal settles nothing, settles wait for an open chain, refused values
//! leave apply going, and an elided winner is stored back before its row is overwritten. A
//! settle may take a reference a concurrent version names, and one naming another row is
//! refused.

use ganbaru_sync_contracts::bounds::MAX_OPERATIONS_PER_GROUP;
use ganbaru_sync_contracts::{Field, GroupId, Value};
use sqlx::SqliteConnection;

use super::block_on;
use super::fixture::{
    ASK, FORBIDDEN, LINKS, LINKS_TABLE, LISTS_TABLE, OVERRIDES_TABLE, OverrideHook, RESERVED,
    TEST_MANIFEST, UNATTENDED, UNDERIVABLE, list_group, override_group, test_adapters,
    test_engine_hooking, test_engine_with,
};
use super::groups::{insert_note, new_ops, reseal_chain};
use super::replica::{Replica, connection_from, converge, pair, quoted};
use crate::error::failpoints;
use crate::hooks::{
    ChangeSink, ConcurrentVersion, HookFuture, Hooks, MaterializeHook, SettleFuture,
};
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
fn a_settled_value_that_breaks_the_group_rules_is_reported_and_not_written() {
    block_on(async {
        // Refused by the adapter's validation, by a CHECK of the domain table the manifest does
        // not describe, by the adapter's derived columns, and by a domain trigger that aborts.
        for replacement in ["never", RESERVED, UNDERIVABLE, FORBIDDEN] {
            let mut replicas = concurrent_clear_and_loose_set().await;
            replicas[0].engine = test_engine_with(OverrideHook {
                replacement: Some(replacement),
            });
            let reports = pull(&mut replicas, 0, 1).await;
            let refused: Vec<_> = reports
                .iter()
                .filter_map(|report| report.seal.as_ref())
                .flat_map(|seal| &seal.invalid_rows)
                .collect();
            assert_eq!(refused.len(), 1, "{replacement}");
            assert_eq!(refused[0].table, OVERRIDES_TABLE);
            assert_eq!(refused[0].row_key, KEY);
            let replica = &mut replicas[0];
            assert_eq!(policy(replica).await.as_deref(), Some(UNATTENDED));
            assert_eq!(pending(replica).await, 0);
            assert_eq!(policy_versions(replica).await, (2, 2));
            replica.check_invariants().await;
            replica.check_version_values().await;

            // Apply keeps going: a later operation on the row applies.
            set_fallback(&mut replicas[1], UNATTENDED).await;
            let reports = pull(&mut replicas, 0, 1).await;
            assert!(reports.iter().all(|report| !report.pending_captures));
            assert_eq!(fallback(&mut replicas[0]).await, UNATTENDED);
        }
    });
}

/// Sets the fallback of [`KEY`] locally and seals it.
async fn set_fallback(replica: &mut Replica, fallback: &str) {
    replica
        .exec(&format!(
            "UPDATE fixture_overrides SET fallback = {} WHERE id = {}",
            quoted(fallback),
            quoted(KEY)
        ))
        .await;
    assert_eq!(replica.seal().await.unwrap().sealed_ops, 1);
}

async fn fallback(replica: &mut Replica) -> String {
    sqlx::query_scalar("SELECT fallback FROM fixture_overrides WHERE id = ?")
        .bind(KEY)
        .fetch_one(&mut replica.conn)
        .await
        .unwrap()
}

async fn apply_without_writer(replica: &mut Replica) -> ApplyReport {
    replica
        .engine
        .apply(&mut replica.conn, &replica.ctx, None, replica.now_ms)
        .await
        .unwrap()
}

/// Delivers what `from` has and `to` lacks without applying it.
async fn deliver(replicas: &mut [Replica], to: usize, from: usize) {
    let (to, from) = pair(replicas, to, from);
    let known = to.stored_vector().await;
    let ops = from.ops_since(&known).await;
    to.receive(&ops).await;
}

#[test]
fn a_device_without_a_writer_settles_nothing_and_keeps_applying_the_row() {
    block_on(async {
        let mut replicas = concurrent_clear_and_loose_set().await;
        deliver(&mut replicas, 0, 1).await;
        let report = apply_without_writer(&mut replicas[0]).await;
        assert!(report.applied > 0);
        assert_eq!(policy(&mut replicas[0]).await.as_deref(), Some(UNATTENDED));
        assert_eq!(pending(&mut replicas[0]).await, 0);
        assert_eq!(policy_versions(&mut replicas[0]).await, (2, 2));

        // A capture left behind would stop every later operation on the row.
        set_fallback(&mut replicas[1], UNATTENDED).await;
        deliver(&mut replicas, 0, 1).await;
        let report = apply_without_writer(&mut replicas[0]).await;
        assert!(!report.pending_captures);
        assert!(report.applied > 0);
        assert_eq!(fallback(&mut replicas[0]).await, UNATTENDED);
        assert_eq!(pending(&mut replicas[0]).await, 0);
        replicas[0].check_invariants().await;
    });
}

#[test]
fn a_device_whose_writer_is_no_longer_active_settles_nothing() {
    block_on(async {
        for state in ["retired", "revoked", "forked"] {
            let mut replicas = concurrent_clear_and_loose_set().await;
            let writer = replicas[0].writer();
            let head = replicas[0].stored_vector().await.get(&writer);
            let hex: String = writer
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            replicas[0]
                .exec(&format!(
                    "UPDATE sync_writers SET state = '{state}', cutoff_seq = {head}
                     WHERE writer_id = x'{hex}'"
                ))
                .await;
            deliver(&mut replicas, 0, 1).await;
            let report = replicas[0].apply_once().await.unwrap();
            assert!(report.applied > 0, "{state}");
            let replica = &mut replicas[0];
            assert_eq!(
                policy(replica).await.as_deref(),
                Some(UNATTENDED),
                "{state}"
            );
            assert_eq!(pending(replica).await, 0, "{state}");
            assert_eq!(policy_versions(replica).await, (2, 2), "{state}");
        }
    });
}

#[test]
fn settles_wait_for_an_open_chain_and_its_last_pass_settles() {
    block_on(async {
        let mut replicas = shared(ASK, Some(ASK)).await;
        replicas[1].exec(&insert_note(1, 9_999)).await;
        replicas[1].seal().await.unwrap();
        converge(&mut replicas).await;
        set_policy(&mut replicas[0], None).await;

        // Replica 1's chain sets the looser policy in its first group and then adds notes.
        let author = &mut replicas[1];
        let previous = author.own_ops().await.pop().unwrap();
        let known = author.stored_vector().await;
        set_policy(author, Some(UNATTENDED)).await;
        for index in 0..MAX_OPERATIONS_PER_GROUP {
            author.exec(&insert_note(1, index)).await;
            assert_eq!(author.seal().await.unwrap().sealed_ops, 1);
        }
        let ops = new_ops(author, &known).await;
        let mut flags = vec![true; ops.len()];
        flags[ops.len() - 1] = false;
        let chain = reseal_chain(author, &previous, &ops, &flags, |_, _| {});

        let receiver = &mut replicas[0];
        receiver.receive(&chain).await;
        let first = receiver.apply_once().await.unwrap();
        assert_eq!(first.applied, MAX_OPERATIONS_PER_GROUP);
        assert!(first.more);
        assert_eq!(policy(receiver).await.as_deref(), Some(UNATTENDED));
        assert_eq!(pending(receiver).await, 0);
        assert!(receiver.count("SELECT count(*) FROM sync_stale_rows").await > 0);

        receiver.apply_all().await.unwrap();
        assert_eq!(policy(receiver).await.as_deref(), Some(ASK));
        assert_eq!(pending(receiver).await, 1);
        assert_eq!(
            receiver.count("SELECT count(*) FROM sync_stale_rows").await,
            0
        );
        assert_eq!(receiver.seal().await.unwrap().sealed_ops, 1);
        assert_eq!(policy_versions(receiver).await, (1, 1));
        receiver.check_invariants().await;
        receiver.check_version_values().await;
    });
}

/// How many policy versions of [`KEY`] by `writer` keep a null, elided value.
async fn elided_policy(replica: &mut Replica, writer: ganbaru_sync_contracts::WriterId) -> i64 {
    let hex: String = writer
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    replica
        .count(&format!(
            "SELECT count(*) FROM sync_register_versions
             WHERE table_id = {} AND row_key = {} AND group_id = {} AND writer_id = x'{hex}'
                AND value IS NULL",
            OVERRIDES_TABLE.0,
            quoted(KEY),
            override_group::POLICY.get()
        ))
        .await
}

#[test]
fn a_settle_stores_back_the_elided_winner_before_overwriting_its_row() {
    block_on(async {
        let mut replicas = concurrent_clear_and_loose_set().await;
        let loose = replicas[1].writer();
        assert_eq!(elided_policy(&mut replicas[1], loose).await, 1);

        pull(&mut replicas, 1, 0).await;
        let replica = &mut replicas[1];
        assert_eq!(policy(replica).await.as_deref(), Some(ASK));
        assert_eq!(pending(replica).await, 1);
        assert_eq!(elided_policy(replica, loose).await, 0);
        assert_eq!(policy_versions(replica).await, (2, 2));
        let versions =
            crate::merge::versions(&replica.engine, &mut replica.conn, OVERRIDES_TABLE, KEY)
                .await
                .unwrap();
        assert!(versions.iter().any(|version| {
            version.writer == loose
                && version.value.fields() == [Field::Text(UNATTENDED.to_string())]
        }));
        replica.check_invariants().await;
        replica.check_version_values().await;
    });
}

/// A hook that names the owned entries group of lists as settled.
struct SettlesOwnedEntries;

impl MaterializeHook for SettlesOwnedEntries {
    fn rebuild_all<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(std::future::ready(Ok(())))
    }

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

/// The ends group of links, a register with references to both cycle tables.
const LINK_ENDS: GroupId = LINKS.groups[1].id;

const LINK: &str = "l-1";

/// Settles the ends of a link on the winner's value with the highest left end any version
/// names, or with `invented` as the left end, for tests of refused references.
struct LinkEndsHook {
    invented: Option<&'static str>,
}

impl MaterializeHook for LinkEndsHook {
    fn rebuild_all<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn after_materialize<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        _changed_keys: &'a [String],
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn settled_groups(&self, _table: &'static TableSpec) -> &'static [GroupId] {
        &[LINK_ENDS]
    }

    fn settle<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        _key: &'a str,
        _group: GroupId,
        versions: &'a [ConcurrentVersion],
    ) -> SettleFuture<'a> {
        let left = |version: &ConcurrentVersion| match version.value.fields().get(1) {
            Some(Field::Text(left)) => Some(left.clone()),
            _ => None,
        };
        let settled = versions
            .iter()
            .find(|version| version.won)
            .and_then(|winner| {
                let highest = self
                    .invented
                    .map(str::to_string)
                    .or_else(|| versions.iter().filter_map(left).max())?;
                if left(winner).as_ref() == Some(&highest) {
                    return None;
                }
                let mut fields = winner.value.fields().to_vec();
                fields[1] = Field::Text(highest);
                Value::new(fields).ok()
            });
        Box::pin(std::future::ready(Ok(settled)))
    }
}

/// Two replicas running [`LinkEndsHook`] that share [`LINK`] from `a-1` and cycle rows `a-1`,
/// `a-2`, and `a-9`; replica 0 moves the left end to `a-2` while replica 1 edits the note.
async fn concurrent_end_and_note(invented: Option<&'static str>) -> Vec<Replica> {
    let mut replicas = vec![Replica::new(0, 37).await, Replica::new(1, 37).await];
    for replica in replicas.iter_mut() {
        replica.engine = test_engine_hooking(LINKS_TABLE, Box::new(LinkEndsHook { invented }));
    }
    replicas[0]
        .exec(
            "INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'one'), ('a-2', 'two'),
                 ('a-9', 'nine');
             INSERT INTO fixture_links (id, note, left_id) VALUES ('l-1', 'n', 'a-1')",
        )
        .await;
    converge(&mut replicas).await;
    replicas[0]
        .exec("UPDATE fixture_links SET left_id = 'a-2' WHERE id = 'l-1'")
        .await;
    assert_eq!(replicas[0].seal().await.unwrap().sealed_ops, 1);
    replicas[1]
        .exec("UPDATE fixture_links SET note = 'x' WHERE id = 'l-1'")
        .await;
    assert_eq!(replicas[1].seal().await.unwrap().sealed_ops, 1);
    replicas
}

/// The note and left end of [`LINK`].
async fn link(replica: &mut Replica) -> (String, Option<String>) {
    sqlx::query_as("SELECT note, left_id FROM fixture_links WHERE id = ?")
        .bind(LINK)
        .fetch_one(&mut replica.conn)
        .await
        .unwrap()
}

#[test]
fn a_settle_may_choose_a_reference_a_concurrent_version_names() {
    block_on(async {
        let mut replicas = concurrent_end_and_note(None).await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                link(replica).await,
                ("x".to_string(), Some("a-2".to_string()))
            );
            assert_eq!(pending(replica).await, 0);
            replica.check_invariants().await;
            replica.check_version_values().await;
        }
        let first = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, first);
    });
}

#[test]
fn a_settled_reference_no_concurrent_version_names_is_refused() {
    block_on(async {
        let mut replicas = concurrent_end_and_note(Some("a-9")).await;
        let reports = pull(&mut replicas, 1, 0).await;
        let refused: Vec<_> = reports
            .iter()
            .filter_map(|report| report.seal.as_ref())
            .flat_map(|seal| &seal.invalid_rows)
            .collect();
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].table, LINKS_TABLE);
        assert_eq!(refused[0].row_key, LINK);
        let replica = &mut replicas[1];
        assert_ne!(link(replica).await.1.as_deref(), Some("a-9"));
        assert_eq!(pending(replica).await, 0);
        replica.check_invariants().await;
        replica.check_version_values().await;
    });
}
