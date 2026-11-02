//! Operations whose bytes pass validation and whose domain write a constraint refuses: apply
//! refuses one row, keeps going, and leaves the merge state equal on every replica. A writer
//! stands for another build by not enforcing the receiver's constraint, so its operations are
//! validly sealed and its merge state is the one the receiver must reach.

use std::collections::{BTreeSet, VecDeque};

use ganbaru_sync_contracts::{
    Change, ChangeAction, Content, Field, GroupId, Hlc, RowKey, TableId, Value,
};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::domains::chat::ChatAdapter;
use crate::manifest::group_id;
use crate::manifest::vault::chat::{
    PROFILE_REVISIONS_TABLE, PROFILES_TABLE, profile_revision_group,
};
use crate::manifest::vault::distractions::{SETTINGS_KEY, SETTINGS_TABLE, settings_group};
use crate::refused::MAX_REFUSED_RETRIES_PER_FINISH;
use crate::visibility::{DeriveScope, Visibility};
use crate::{ApplyReport, HoldReason, StoreOutcome, SyncResult};

use super::authorization::{seal_at, view};
use super::block_on;
use super::calendar::{calendar_sql, event_sql};
use super::distractions::block_sql;
use super::engine::{take_forged, text_field, with_group};
use super::exclusion::note;
use super::fixture::{
    ASK, FORBIDDEN, ITEMS_TABLE, REFUSED, RESERVED, UNATTENDED, test_engine_adapting,
};
use super::focus::{close_run_sql, open_run_sql, segment_id};
use super::holds::sealed_create;
use super::replica::{Replica, converge, forge_change, pair, quoted, rank};
use super::sim::Rng;

const CREATED: &str = "'2027-01-01T00:00:00.000Z'";

/// The access profile of the immutable revision scenarios.
const PROFILE: &str = "profile-custom";
/// The revision a hostile writer creates again.
const REVISION: &str = "revision-custom";
/// The capability of the revision its first writer saves.
const FIRST_CAPABILITY: &str = "read";
/// The capability of the hostile Create of that revision.
const FORGED_CAPABILITY: &str = "publish";
/// The replica of the immutable revision scenarios that writes the hostile Create.
const FORGER: usize = 2;
/// Replicas of the immutable revision scenarios.
const FORGED_REPLICAS: usize = 4;
/// How far past the first writer the clock of a Create that loses the Immutable minimum runs.
const CLOCK_AHEAD_MS: u64 = 1_000;

/// Seeds of the built-in key claim simulation.
const CLAIM_SEEDS: u64 = 8;
/// Replicas of the claim simulation, the last of them hostile.
const CLAIM_REPLICAS: usize = 3;
/// First vault salt of the claim simulation.
const CLAIM_SALT: u64 = 7300;
/// The seeded profile whose values the hostile create copies.
const CLAIMED_SEED: &str = "access-profile:read-only";

/// A replica that does not enforce `CHECK` constraints, so it seals values others refuse.
async fn unchecked(index: usize, salt: u64) -> Replica {
    let mut replica = Replica::new(index, salt).await;
    replica.exec("PRAGMA ignore_check_constraints = ON").await;
    replica
}

/// Stores what `from` has and `to` lacks and returns the apply result.
pub(super) async fn take(
    replicas: &mut [Replica],
    to: usize,
    from: usize,
) -> SyncResult<Vec<ApplyReport>> {
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
            std::slice::from_ref(last)
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

        for replica in replicas.iter_mut().take(2) {
            assert!(keys(replica, "fixture_overrides").await.is_empty());
            assert_eq!(refused(replica, "hidden").await, ["o-1"]);
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

/// Replicas that seeded their existing rows and exchanged them, as the app does at vault open.
async fn seeded_replicas(count: usize, salt: u64) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        let mut replica = Replica::new(index, salt).await;
        let now = replica.now_ms;
        replica
            .engine
            .seed_captures(&mut replica.conn, now)
            .await
            .unwrap();
        replicas.push(replica);
    }
    converge(&mut replicas).await;
    replicas
}

/// SQL creating access profile `id` with `builtin_key`.
fn claiming_profile_sql(id: &str, builtin_key: &str) -> String {
    format!(
        "INSERT INTO chat_access_profiles (id, builtin_key, display_name, created_at, updated_at)
         VALUES ({}, {}, 'Claim', {CREATED}, {CREATED});",
        quoted(id),
        quoted(builtin_key),
    )
}

/// The Chat adapter without its row checks, so a duplicate built-in key passes admission and the
/// repair behind it can be reached.
#[derive(Debug)]
struct ChatWithoutRowChecks;

impl DomainAdapter for ChatWithoutRowChecks {
    fn name(&self) -> &'static str {
        ChatAdapter.name()
    }

    fn derived(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        ChatAdapter.derived(table, group, value)
    }

    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        ChatAdapter.priority(table, group, value)
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        ChatAdapter.validate(table, group, value)
    }

    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        ChatAdapter.presentation(table, values)
    }
}

/// A writer without the unique index claims built-in keys under keys on both sides of the seeded
/// ones, past an admission check that would hold them: repair seals nothing in the tables no
/// operation deletes from, and the seeded rows and their revisions stay in the domain tables.
#[test]
fn repair_seals_no_tombstone_in_a_never_deleted_table() {
    block_on(async {
        let mut replicas = seeded_replicas(2, 70).await;
        for replica in &mut replicas {
            replica.engine = test_engine_adapting(Box::new(ChatWithoutRowChecks));
        }
        replicas[1]
            .exec("DROP INDEX idx_chat_access_profiles_builtin")
            .await;
        let claims = format!(
            "{}{}",
            claiming_profile_sql("access-profile:aaa", "edit_files"),
            claiming_profile_sql("access-profile:zzz", "read_only"),
        );
        exec_and_seal(&mut replicas[1], &claims).await;

        take(&mut replicas, 0, 1).await.unwrap();
        take(&mut replicas, 1, 0).await.unwrap();

        let tombstones = format!(
            "SELECT count(*) FROM sync_tombstones WHERE table_id IN ({}, {})",
            PROFILES_TABLE.0, PROFILE_REVISIONS_TABLE.0
        );
        for replica in &mut replicas {
            assert_eq!(replica.count(&tombstones).await, 0);
        }
        let profiles = keys(&mut replicas[0], "chat_access_profiles").await;
        for seeded in ["access-profile:read-only", "access-profile:edit-files"] {
            assert!(profiles.iter().any(|key| key == seeded), "{profiles:?}");
        }
        assert!(!profiles.iter().any(|key| key == "access-profile:zzz"));
        assert_eq!(
            replicas[0]
                .count(
                    "SELECT count(*) FROM chat_access_profile_revisions
                     WHERE id = 'access-profile-revision:edit-files:1'"
                )
                .await,
            1
        );
    });
}

/// A hidden duplicate of a never-deleted table stays in its domain table, and that counts as
/// settled: loading visibility again marks no row dirty, so no later finish plans it again.
#[test]
fn a_kept_hidden_duplicate_of_a_never_deleted_table_is_settled() {
    block_on(async {
        let mut replicas = seeded_replicas(2, 73).await;
        for replica in &mut replicas {
            replica.engine = test_engine_adapting(Box::new(ChatWithoutRowChecks));
        }
        replicas[1]
            .exec("DROP INDEX idx_chat_access_profiles_builtin")
            .await;
        let claims = format!(
            "{}{}",
            claiming_profile_sql("access-profile:aaa", "edit_files"),
            claiming_profile_sql("access-profile:zzz", "read_only"),
        );
        exec_and_seal(&mut replicas[1], &claims).await;
        take(&mut replicas, 0, 1).await.unwrap();
        take(&mut replicas, 1, 0).await.unwrap();

        for replica in &mut replicas {
            let mut dirty = BTreeSet::new();
            let visibility = Visibility::load(
                &replica.engine,
                &mut replica.conn,
                &mut dirty,
                DeriveScope::All,
            )
            .await
            .unwrap();
            let hidden: BTreeSet<(TableId, String)> = visibility
                .duplicates
                .iter()
                .flat_map(|(table, rows)| rows.keys().map(|key| (*table, key.clone())))
                .collect();
            assert!(
                !hidden.is_empty(),
                "replica {} holds a hidden duplicate",
                replica.index
            );
            let replanned: Vec<_> = hidden.intersection(&dirty).collect();
            assert!(
                replanned.is_empty(),
                "replica {} plans kept rows again: {replanned:?}",
                replica.index
            );
        }
    });
}

/// A hostile writer claims a built-in key under a lower row key while the seeds of every device
/// are still in flight. Whatever order the operations arrive in, the claim is held, apply never
/// fails, and the honest replicas converge on the seeded profiles alone.
#[test]
fn simulation_builtin_key_claim_never_stops_apply() {
    block_on(async {
        for seed in 0..CLAIM_SEEDS {
            let mut rng = Rng(seed);
            let mut replicas = Vec::with_capacity(CLAIM_REPLICAS);
            for index in 0..CLAIM_REPLICAS {
                let mut replica = Replica::new(index, CLAIM_SALT + seed).await;
                let now = replica.now_ms;
                replica
                    .engine
                    .seed_captures(&mut replica.conn, now)
                    .await
                    .unwrap();
                replica.seal().await.unwrap();
                replicas.push(replica);
            }

            let attacker = CLAIM_REPLICAS - 1;
            let mut claim =
                sealed_create(&mut replicas[attacker], PROFILES_TABLE, CLAIMED_SEED).await;
            claim.row = RowKey::new("access-profile:aaa").unwrap();
            let forged = forge_change(&mut replicas[attacker], claim).await;

            let mut sent = Vec::with_capacity(CLAIM_REPLICAS);
            for replica in &mut replicas {
                sent.push(replica.own_ops().await);
            }
            sent[attacker].push(forged.clone());

            for (receiver, replica) in replicas.iter_mut().enumerate().take(attacker) {
                let mut streams: Vec<VecDeque<Vec<u8>>> = (0..CLAIM_REPLICAS)
                    .filter(|writer| *writer != receiver)
                    .map(|writer| sent[writer].iter().cloned().collect())
                    .collect();
                while !streams.is_empty() {
                    let stream = rng.below(streams.len());
                    let op = streams[stream].pop_front().unwrap();
                    let outcomes = replica.receive(std::slice::from_ref(&op)).await;
                    if op == forged {
                        assert_eq!(
                            outcomes,
                            vec![StoreOutcome::Stored {
                                held: Some(HoldReason::Invalid)
                            }],
                            "seed {seed}"
                        );
                    }
                    let applied = replica.apply_all().await;
                    assert!(applied.is_ok(), "seed {seed}: {applied:?}");
                    if streams[stream].is_empty() {
                        streams.swap_remove(stream);
                    }
                }
            }

            converge(&mut replicas[..attacker]).await;
            let other = merge_state(&mut replicas[1]).await;
            assert_eq!(merge_state(&mut replicas[0]).await, other, "seed {seed}");
            for replica in &mut replicas[..attacker] {
                let profiles = keys(replica, "chat_access_profiles").await;
                assert_eq!(profiles.len(), 5, "seed {seed}: {profiles:?}");
                assert!(!profiles.iter().any(|key| key == "access-profile:aaa"));
            }
        }
    });
}

/// A write of the singleton from a writer whose create never arrives hides the row until one
/// does, and the seeded row stays: nothing refuses that delete, so it would go unnoticed.
#[test]
fn distractions_settings_survive_a_hidden_row() {
    block_on(async {
        let mut other = seeded_replicas(1, 71).await.remove(0);
        let mut write = sealed_create(&mut other, SETTINGS_TABLE, SETTINGS_KEY).await;
        write.action = ChangeAction::Write;
        write
            .groups
            .retain(|value| value.group == settings_group::LIMITS);

        let mut replicas = vec![Replica::new(0, 71).await, Replica::new(1, 71).await];
        exec_and_seal(&mut replicas[1], &note("note-1", "One", 0)).await;
        take(&mut replicas, 0, 1).await.unwrap();
        // The merge state a change from an excluded forked branch leaves for a row with no
        // creation.
        replicas[0]
            .exec(&format!(
                "INSERT INTO sync_rows (table_id, row_key, state, creation_pending)
                 VALUES ({}, {}, 'live', 1)",
                SETTINGS_TABLE.0,
                quoted(SETTINGS_KEY),
            ))
            .await;
        let forged = forge_change(&mut replicas[1], write).await;
        assert_eq!(
            replicas[0].receive(&[forged]).await,
            vec![StoreOutcome::Stored { held: None }]
        );
        replicas[0].apply_all().await.unwrap();

        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM distractions_settings")
                .await,
            1
        );
    });
}

/// A delete that reaches a trigger through a cascade the manifest does not declare keeps the
/// row, whichever table refused it, and apply goes on.
#[test]
fn delete_of_a_parent_cascading_into_a_trigger_keeps_the_row() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 72).await, Replica::new(1, 72).await];
        replicas[0]
            .exec(
                "CREATE TRIGGER fixture_links_keep BEFORE DELETE ON fixture_links
                 BEGIN SELECT RAISE(ABORT, 'kept'); END",
            )
            .await;
        exec_and_seal(
            &mut replicas[1],
            &format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                 VALUES ('i-1', {CREATED}, 'One', 0);
                 INSERT INTO fixture_links (id, owner_id, note) VALUES ('l-1', 'i-1', 'One');"
            ),
        )
        .await;
        take(&mut replicas, 0, 1).await.unwrap();

        exec_and_seal(
            &mut replicas[1],
            "DELETE FROM fixture_items WHERE id = 'i-1'",
        )
        .await;
        take(&mut replicas, 0, 1).await.unwrap();

        assert_eq!(keys(&mut replicas[0], "fixture_items").await, ["i-1"]);
        assert_eq!(keys(&mut replicas[0], "fixture_links").await, ["l-1"]);
        assert!(
            refused(&mut replicas[0], "kept")
                .await
                .contains(&"i-1".to_string())
        );
        assert!(refused(&mut replicas[0], "hidden").await.is_empty());
        let sender = merge_state(&mut replicas[1]).await;
        assert_eq!(merge_state(&mut replicas[0]).await, sender);
    });
}

/// Deletes of ordinary tables and archives of never-deleted rows keep replicating.
#[test]
fn ordinary_deletes_and_archives_keep_replicating() {
    block_on(async {
        let mut replicas = seeded_replicas(2, 73).await;
        exec_and_seal(
            &mut replicas[1],
            &format!(
                "{}
                 INSERT INTO chat_access_profiles (id, display_name, created_at, updated_at)
                 VALUES ('access-profile:custom', 'Custom', {CREATED}, {CREATED});",
                note("note-1", "One", 0),
            ),
        )
        .await;
        converge(&mut replicas).await;
        exec_and_seal(
            &mut replicas[1],
            &format!(
                "DELETE FROM quick_notes WHERE id = 'note-1';
                 UPDATE chat_access_profiles SET archived_at = {CREATED}
                 WHERE id = 'access-profile:custom';"
            ),
        )
        .await;
        converge(&mut replicas).await;

        for replica in &mut replicas {
            assert_eq!(replica.count("SELECT count(*) FROM quick_notes").await, 0);
            assert_eq!(
                replica
                    .count(
                        "SELECT count(*) FROM chat_access_profiles
                         WHERE id = 'access-profile:custom' AND archived_at IS NOT NULL"
                    )
                    .await,
                1
            );
        }
    });
}

/// SQL creating the custom access profile `id`.
fn custom_profile_sql(id: &str) -> String {
    format!(
        "INSERT INTO chat_access_profiles (id, display_name, created_at, updated_at)
         VALUES ({}, 'Custom', {CREATED}, {CREATED});",
        quoted(id),
    )
}

/// Replicas sharing [`PROFILE`], with [`REVISION`] sealed by the first and held by no other.
/// [`FORGER`] has sealed, so its next operation is valid, and has not seen the revision.
async fn replicas_with_revision(salt: u64) -> Vec<Replica> {
    let mut replicas = seeded_replicas(FORGED_REPLICAS, salt).await;
    exec_and_seal(&mut replicas[FORGER], &custom_profile_sql(PROFILE)).await;
    converge(&mut replicas).await;
    exec_and_seal(
        &mut replicas[0],
        &format!(
            "INSERT INTO chat_access_profile_revisions
                (id, access_profile_id, default_read_history, default_participate,
                 default_history_boundary, maximum_folder_capability, created_at)
             VALUES ({}, {}, 1, 1, 'entire', {}, {CREATED});",
            quoted(REVISION),
            quoted(PROFILE),
            quoted(FIRST_CAPABILITY),
        ),
    )
    .await;
    replicas
}

/// The Create of [`REVISION`] again, with [`FORGED_CAPABILITY`].
async fn forged_revision_create(creator: &mut Replica) -> Change {
    let create = sealed_create(creator, PROFILE_REVISIONS_TABLE, REVISION).await;
    with_group(
        create,
        profile_revision_group::CREATED,
        vec![
            text_field(PROFILE),
            Field::Integer(1),
            Field::Integer(1),
            text_field("entire"),
            text_field(FORGED_CAPABILITY),
            text_field(CREATED.trim_matches('\'')),
        ],
    )
}

/// The operation `replica` seals with `change` at `clock`, outside the engine.
async fn forge_at(replica: &mut Replica, change: Change, clock: Hlc) -> Vec<u8> {
    let revision = view(replica).await.revision;
    let last = replica.own_ops().await.pop().unwrap();
    seal_at(
        &last,
        &replica.identity.key,
        Content::Changes(vec![change]),
        revision,
        clock,
    )
}

/// The capability `replica` shows for [`REVISION`].
async fn revision_capability(replica: &mut Replica) -> Option<String> {
    replica
        .scalar_text(&format!(
            "SELECT maximum_folder_capability FROM chat_access_profile_revisions WHERE id = {}",
            quoted(REVISION)
        ))
        .await
}

/// A Create that saw the immutable revision leaves it alone: a replica holding the row, one that
/// takes the row and the Create together, and the first writer agree with nothing refused.
#[test]
fn forged_immutable_create_converges() {
    const HOLDER: usize = 1;
    const LATE: usize = 3;
    const OTHER_PROFILE: &str = "profile-other";
    block_on(async {
        let mut replicas = replicas_with_revision(71).await;
        take(&mut replicas, HOLDER, 0).await.unwrap();
        take(&mut replicas, FORGER, 0).await.unwrap();
        exec_and_seal(&mut replicas[FORGER], &custom_profile_sql(OTHER_PROFILE)).await;

        let create = forged_revision_create(&mut replicas[0]).await;
        let forged = forge_change(&mut replicas[FORGER], create).await;
        let (holder, forger) = pair(&mut replicas, HOLDER, FORGER);
        holder.pull(forger).await;
        take_forged(holder, &forged).await;
        take(&mut replicas, LATE, HOLDER).await.unwrap();
        take(&mut replicas, 0, HOLDER).await.unwrap();

        for index in [0, HOLDER, LATE] {
            let replica = &mut replicas[index];
            assert_eq!(
                revision_capability(replica).await.as_deref(),
                Some(FIRST_CAPABILITY),
                "replica {index} keeps the first value"
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM sync_refused_rows")
                    .await,
                0,
                "nothing is refused on replica {index}"
            );
        }
        let holder = replicas[HOLDER].dump().await;
        assert_eq!(replicas[0].dump().await, holder);
        assert_eq!(replicas[LATE].dump().await, holder);
    });
}

/// Asserts the outcome of a Create that did not see the immutable revision: merge state agrees,
/// `loser_holder` lists its row `kept`, `winner_holder` lists nothing, and a later write changes
/// neither replica.
async fn assert_loser_stays_kept(
    replicas: &mut [Replica],
    (loser_holder, loser): (usize, &str),
    (winner_holder, winner): (usize, &str),
) {
    for pass in ["after the Create", "after a later write"] {
        assert_eq!(
            merge_state(&mut replicas[loser_holder]).await,
            merge_state(&mut replicas[winner_holder]).await,
            "merge state agrees {pass}"
        );
        assert_eq!(
            revision_capability(&mut replicas[loser_holder])
                .await
                .as_deref(),
            Some(loser),
            "the row of replica {loser_holder} stays {pass}"
        );
        assert_eq!(
            refused(&mut replicas[loser_holder], "kept").await,
            vec![REVISION.to_owned()],
            "replica {loser_holder} lists the row {pass}"
        );
        assert_eq!(
            revision_capability(&mut replicas[winner_holder])
                .await
                .as_deref(),
            Some(winner),
            "replica {winner_holder} shows the winner {pass}"
        );
        assert_eq!(
            replicas[winner_holder]
                .count("SELECT count(*) FROM sync_refused_rows")
                .await,
            0,
            "replica {winner_holder} refuses nothing {pass}"
        );

        exec_and_seal(
            &mut replicas[0],
            &format!(
                "UPDATE chat_access_profiles SET display_name = {} WHERE id = {}",
                quoted(pass),
                quoted(PROFILE)
            ),
        )
        .await;
        take(replicas, loser_holder, 0).await.unwrap();
        take(replicas, winner_holder, 0).await.unwrap();
    }
}

/// Stores what the first replica sealed and then `forged` on the second replica, and the same
/// operations in the other order on the last. Returns those two replicas.
async fn deliver_in_both_orders(replicas: &mut [Replica], forged: &[u8]) -> (usize, usize) {
    const HONEST_FIRST: usize = 1;
    const FORGED_FIRST: usize = 3;
    take(replicas, HONEST_FIRST, 0).await.unwrap();
    take_forged(&mut replicas[HONEST_FIRST], forged).await;
    take_forged(&mut replicas[FORGED_FIRST], forged).await;
    take(replicas, FORGED_FIRST, 0).await.unwrap();
    (HONEST_FIRST, FORGED_FIRST)
}

#[test]
fn a_create_winning_an_immutable_minimum_it_did_not_see_leaves_the_kept_row_different() {
    block_on(async {
        let mut replicas = replicas_with_revision(72).await;
        let create = forged_revision_create(&mut replicas[0]).await;
        let forged = forge_at(&mut replicas[FORGER], create, Hlc::new(1, 0)).await;
        let (honest_first, forged_first) = deliver_in_both_orders(&mut replicas, &forged).await;
        assert_loser_stays_kept(
            &mut replicas,
            (honest_first, FIRST_CAPABILITY),
            (forged_first, FORGED_CAPABILITY),
        )
        .await;
    });
}

#[test]
fn a_create_losing_an_immutable_minimum_it_did_not_see_leaves_the_kept_row_different() {
    block_on(async {
        let mut replicas = replicas_with_revision(73).await;
        let create = forged_revision_create(&mut replicas[0]).await;
        let clock = Hlc::new(replicas[0].now_ms + CLOCK_AHEAD_MS, 0);
        let forged = forge_at(&mut replicas[FORGER], create, clock).await;
        let (honest_first, forged_first) = deliver_in_both_orders(&mut replicas, &forged).await;
        assert_loser_stays_kept(
            &mut replicas,
            (forged_first, FORGED_CAPABILITY),
            (honest_first, FIRST_CAPABILITY),
        )
        .await;
    });
}

/// A row whose delete passes is hidden when its update is refused, so a retry inserts the winner
/// and the replicas converge: the difference [`assert_loser_stays_kept`] pins needs both
/// triggers.
#[test]
fn a_row_refusing_only_updates_converges_on_the_immutable_winner() {
    const ITEM: &str = "item-1";
    const FIRST_CREATED_AT: &str = "2027-01-02T00:00:00.000Z";
    const FORGED_CREATED_AT: &str = "2027-01-03T00:00:00.000Z";
    const SELECT_CREATED_AT: &str = "SELECT created_at FROM fixture_items WHERE id = 'item-1'";
    const FROZEN_SQL: &str = "CREATE TRIGGER frozen_item BEFORE UPDATE OF created_at
         ON fixture_items BEGIN SELECT RAISE(ABORT, 'frozen'); END;";
    block_on(async {
        let mut replicas = seeded_replicas(FORGED_REPLICAS, 74).await;
        exec_and_seal(&mut replicas[FORGER], &note("note-forger", "Note", 1)).await;
        converge(&mut replicas).await;
        exec_and_seal(
            &mut replicas[0],
            &format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                 VALUES ({}, {}, 'One', 0)",
                quoted(ITEM),
                quoted(FIRST_CREATED_AT)
            ),
        )
        .await;
        let create = sealed_create(&mut replicas[0], ITEMS_TABLE, ITEM).await;
        let create = with_group(create, group_id(0), vec![text_field(FORGED_CREATED_AT)]);
        let forged = forge_at(&mut replicas[FORGER], create, Hlc::new(1, 0)).await;

        replicas[1].exec(FROZEN_SQL).await;
        replicas[3].exec(FROZEN_SQL).await;
        let (honest_first, forged_first) = deliver_in_both_orders(&mut replicas, &forged).await;
        assert_eq!(
            refused(&mut replicas[honest_first], "hidden").await,
            vec![ITEM.to_owned()]
        );
        assert_eq!(
            replicas[forged_first]
                .scalar_text(SELECT_CREATED_AT)
                .await
                .as_deref(),
            Some(FORGED_CREATED_AT)
        );

        exec_and_seal(
            &mut replicas[0],
            &format!(
                "UPDATE fixture_items SET label = 'Two' WHERE id = {}",
                quoted(ITEM)
            ),
        )
        .await;
        take(&mut replicas, honest_first, 0).await.unwrap();
        take(&mut replicas, forged_first, 0).await.unwrap();
        for index in [honest_first, forged_first] {
            let replica = &mut replicas[index];
            assert_eq!(
                replica.scalar_text(SELECT_CREATED_AT).await.as_deref(),
                Some(FORGED_CREATED_AT),
                "replica {index} shows the winner"
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM sync_refused_rows")
                    .await,
                0,
                "nothing stays refused on replica {index}"
            );
        }
        let first = replicas[honest_first].dump().await;
        assert_eq!(replicas[forged_first].dump().await, first);
    });
}

/// SQL creating list `id` with the owned entry `entry`.
fn list_sql(id: &str, entry: &str) -> String {
    format!(
        "INSERT INTO fixture_lists (id, created_at, title) VALUES ({id}, {CREATED}, 'List');
         INSERT INTO fixture_list_entries (list_id, ordinal, entry_id, body)
         VALUES ({id}, 0, {entry}, 'body');",
        id = quoted(id),
        entry = quoted(entry),
    )
}

/// A row a refused delete keeps still holds its unique value, which merge state no longer
/// counts. A later row claiming that value is refused wherever the kept row stayed, whichever of
/// the delete and the claim arrived first, and shows on a replica that never held the kept row.
/// Merge state agrees throughout, a retry does not cure the refusal, and a write that moves the
/// claim off the value does.
#[test]
fn kept_row_unique_value_refuses_only_where_the_row_stayed() {
    const KEPT: &str = "list-kept";
    const CLAIM: &str = "list-claim";
    const SHARED_ENTRY: &str = "entry-shared";
    const MOVED_ENTRY: &str = "entry-moved";
    const KEEP_LISTS: &str = "CREATE TRIGGER fixture_lists_keep BEFORE DELETE ON fixture_lists
         BEGIN SELECT RAISE(ABORT, 'kept'); END";
    const DELETE_THEN_CLAIM: usize = 0;
    const CLAIM_THEN_DELETE: usize = 1;
    const NEVER_HELD: usize = 2;
    const CLAIM_BEFORE_ROW: usize = 3;
    const RECEIVERS: usize = 4;
    const DELETER: usize = RECEIVERS;
    const CLAIMER: usize = RECEIVERS + 1;
    const HOLDERS: [usize; 2] = [DELETE_THEN_CLAIM, CLAIM_THEN_DELETE];
    const OTHERS: [usize; 2] = [NEVER_HELD, CLAIM_BEFORE_ROW];

    block_on(async {
        let mut replicas = Vec::with_capacity(CLAIMER + 1);
        for index in 0..=CLAIMER {
            replicas.push(Replica::new(index, 75).await);
        }
        for replica in &mut replicas[..RECEIVERS] {
            replica.exec(KEEP_LISTS).await;
        }

        exec_and_seal(&mut replicas[DELETER], &list_sql(KEPT, SHARED_ENTRY)).await;
        exec_and_seal(&mut replicas[CLAIMER], &list_sql(CLAIM, SHARED_ENTRY)).await;
        take(&mut replicas, DELETE_THEN_CLAIM, DELETER)
            .await
            .unwrap();
        take(&mut replicas, CLAIM_THEN_DELETE, DELETER)
            .await
            .unwrap();
        take(&mut replicas, CLAIM_THEN_DELETE, CLAIMER)
            .await
            .unwrap();
        take(&mut replicas, CLAIM_BEFORE_ROW, CLAIMER)
            .await
            .unwrap();
        take(&mut replicas, CLAIM_BEFORE_ROW, DELETER)
            .await
            .unwrap();

        exec_and_seal(
            &mut replicas[DELETER],
            &format!("DELETE FROM fixture_lists WHERE id = {}", quoted(KEPT)),
        )
        .await;
        for (to, from) in [
            (DELETE_THEN_CLAIM, DELETER),
            (DELETE_THEN_CLAIM, CLAIMER),
            (CLAIM_THEN_DELETE, DELETER),
            (NEVER_HELD, DELETER),
            (NEVER_HELD, CLAIMER),
            (CLAIM_BEFORE_ROW, DELETER),
        ] {
            take(&mut replicas, to, from).await.unwrap();
        }

        for index in HOLDERS {
            let replica = &mut replicas[index];
            assert_eq!(keys(replica, "fixture_lists").await, [KEPT], "{index}");
            assert_eq!(refused(replica, "kept").await, [KEPT], "{index}");
            assert_eq!(refused(replica, "hidden").await, [CLAIM], "{index}");
        }
        for index in OTHERS {
            let replica = &mut replicas[index];
            assert_eq!(keys(replica, "fixture_lists").await, [CLAIM], "{index}");
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM sync_refused_rows")
                    .await,
                0,
                "{index}"
            );
        }

        exec_and_seal(
            &mut replicas[CLAIMER],
            &format!(
                "UPDATE fixture_lists SET title = 'Renamed' WHERE id = {}",
                quoted(CLAIM)
            ),
        )
        .await;
        for index in 0..RECEIVERS {
            take(&mut replicas, index, CLAIMER).await.unwrap();
        }
        for index in HOLDERS {
            assert_eq!(
                refused(&mut replicas[index], "hidden").await,
                [CLAIM],
                "a write that keeps the value leaves replica {index} refusing"
            );
        }

        exec_and_seal(
            &mut replicas[CLAIMER],
            &format!(
                "UPDATE fixture_list_entries SET entry_id = {} WHERE list_id = {}",
                quoted(MOVED_ENTRY),
                quoted(CLAIM)
            ),
        )
        .await;
        for index in 0..RECEIVERS {
            take(&mut replicas, index, CLAIMER).await.unwrap();
        }
        for index in HOLDERS {
            let replica = &mut replicas[index];
            assert_eq!(keys(replica, "fixture_lists").await, [CLAIM, KEPT]);
            assert_eq!(refused(replica, "kept").await, [KEPT], "{index}");
            assert!(refused(replica, "hidden").await.is_empty(), "{index}");
        }
        for index in OTHERS {
            assert_eq!(keys(&mut replicas[index], "fixture_lists").await, [CLAIM]);
        }
        let reference = merge_state(&mut replicas[0]).await;
        for replica in &mut replicas[1..RECEIVERS] {
            assert_eq!(merge_state(replica).await, reference);
        }
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

#[test]
fn null_run_block_events_and_tree_null_writes_stay_valid() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 70).await, Replica::new(1, 70).await];
        // An event outside a run names neither a run nor a phase, as the spool records it.
        replicas[0]
            .exec(&format!(
                "{}{}{}{}{}",
                calendar_sql(),
                event_sql("event-a", "uid-a", "Deep work"),
                open_run_sql("run-a", "event-a", 2, 2),
                block_sql("block-free", None),
                close_run_sql("run-a", 2),
            ))
            .await;
        converge(&mut replicas).await;
        assert_eq!(
            replicas[1]
                .count(
                    "SELECT count(*) FROM distractions_block_events
                     WHERE id = 'block-free' AND run_id IS NULL AND phase IS NULL"
                )
                .await,
            1
        );

        // Deleting a segment clears the optional reference of its run events.
        replicas[0]
            .exec(&format!(
                "DELETE FROM pomodoro_segments WHERE id = {}",
                quoted(&segment_id("run-a", 0))
            ))
            .await;
        converge(&mut replicas).await;
        assert_eq!(
            replicas[1]
                .count(
                    "SELECT count(*) FROM pomodoro_run_events
                     WHERE id = 'run-a-event-0' AND segment_id IS NULL"
                )
                .await,
            1
        );

        // Concurrent moves close a cycle, and the repair moves a node to the root.
        replicas[0]
            .exec(&format!(
                "INSERT INTO fixture_nodes (id, parent_id, position, label)
                 VALUES ('n-1', NULL, {first}, 'one'), ('n-2', NULL, {second}, 'two');",
                first = quoted(&rank(1)),
                second = quoted(&rank(2)),
            ))
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("UPDATE fixture_nodes SET parent_id = 'n-2' WHERE id = 'n-1'")
            .await;
        replicas[1]
            .exec("UPDATE fixture_nodes SET parent_id = 'n-1' WHERE id = 'n-2'")
            .await;
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM fixture_nodes WHERE parent_id IS NULL")
                    .await,
                1,
                "the repair writes a null parent on replica {}",
                replica.index
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM sync_refused_rows")
                    .await,
                0,
                "nothing is refused on replica {}",
                replica.index
            );
        }
        let other = merge_state(&mut replicas[1]).await;
        assert_eq!(merge_state(&mut replicas[0]).await, other);
    });
}
