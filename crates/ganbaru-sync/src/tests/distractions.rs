//! Distraction rules across replicas: toggles of different settings groups both survive,
//! concurrent toggles of one rule converge, a host rule removed on one replica and added again
//! on another comes back under the same key everywhere, and remote rule changes report the
//! changed tables. Daily usage totals converge to each device's largest flush whatever order
//! flushes and exchanges interleave in, usage samples never leave their device, and only the
//! device a total belongs to can write it.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{
    Change, ChangeAction, Entry, Field, GroupId, GroupValue, LateResolution, LateRow, ResolveLate,
    RowKey, Value, derived_key, device_owner_tag,
};

use super::block_on;
use super::calendar::{calendar_sql, event_sql};
use super::exclusion::{deliver, late_changes};
use super::focus::{close_run_sql, open_run_sql};
use super::replica::{Replica, converge, device_id, forge_change, pair, quoted, sealed_change};
use super::sim::Rng;
use crate::domains::distractions::{MAX_USAGE_DAY_SECONDS, RULES_TOPIC};
use crate::manifest::vault::distractions::{
    HOST_RULES, HOST_RULES_TABLE, USAGE_TOTAL_KEY_PREFIX, USAGE_TOTAL_OWNER_PREFIX, USAGE_TOTALS,
    USAGE_TOTALS_TABLE, host_group, usage_total_group,
};
use crate::manifest::{RowOwner, TableSpec};
use crate::{ChangeNotice, HoldReason, StoreOutcome, validate};

const HOST: &str = "video.example.com";

/// Seeds of the interleaved usage flush scenario.
const USAGE_SEEDS: u64 = 6;

/// Steps of one usage scenario: a flush or a one-way exchange each.
const USAGE_STEPS: usize = 24;

/// Local day every usage scenario records.
const USAGE_DATE: &str = "2026-10-23";

/// Generated device pairs of the owner tag property.
const OWNER_PROPERTY_ROUNDS: usize = 512;

/// Distinct device ids the owner tag property draws from, small enough to repeat a device.
const OWNER_PROPERTY_DEVICES: usize = 32;

/// Replicas that each seeded their settings row at vault open, as the app does once the
/// tables start to replicate, with operations exchanged.
async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        let mut replica = Replica::new(index, 61).await;
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

/// The key of a global blocked host rule, as rule writers derive it.
fn host_key(host: &str) -> String {
    derived_key("host", &["blocked", "", host])
        .expect("rule parts are valid")
        .to_string()
}

/// SQL adding a global blocked host rule created at `created_at_ms`.
fn host_rule_sql(host: &str, created_at_ms: i64) -> String {
    format!(
        "INSERT INTO distractions_host_rules (id, list_kind, scope, host, created_at_ms)
         VALUES ({}, 'blocked', '', {}, {created_at_ms});",
        quoted(&host_key(host)),
        quoted(host),
    )
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

#[test]
fn distractions_toggles_of_different_settings_from_two_replicas_both_survive() {
    block_on(async {
        let mut replicas = replicas(2).await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("UPDATE distractions_settings SET desktop_enabled = 0")
            .await;
        replicas[1]
            .exec("UPDATE distractions_settings SET mobile_block_during_long_breaks = 0, mode = 'whitelist'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .scalar_text(
                        "SELECT mode || ':' || desktop_enabled || ':' || mobile_block_during_long_breaks
                         || ':' || mobile_enabled FROM distractions_settings WHERE id = 'vault'"
                    )
                    .await
                    .as_deref(),
                Some("whitelist:0:0:1")
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM distractions_settings")
                    .await,
                1
            );
        }
    });
}

#[test]
fn distractions_concurrent_toggles_of_one_rule_converge() {
    block_on(async {
        let mut replicas = replicas(3).await;
        replicas[0].exec(&host_rule_sql(HOST, 1_000)).await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("UPDATE distractions_host_rules SET enabled = 0")
            .await;
        replicas[1]
            .exec("UPDATE distractions_host_rules SET enabled = 0")
            .await;
        replicas[1]
            .exec("UPDATE distractions_host_rules SET enabled = 1")
            .await;
        replicas[2]
            .exec("INSERT INTO distractions_category_rules (id, enabled) VALUES ('news', 1)")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM distractions_host_rules")
                    .await,
                1
            );
            assert_eq!(
                replica
                    .scalar_text("SELECT CAST(enabled AS TEXT) FROM distractions_category_rules WHERE id = 'news'")
                    .await
                    .as_deref(),
                Some("1")
            );
        }
    });
}

#[test]
fn distractions_host_rule_removed_and_added_again_returns_everywhere() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&host_rule_sql(HOST, 1_000)).await;
        replicas[0].seal().await.unwrap();
        let mut added = pull_notices(&mut replicas, 1, 0).await;
        added.retain(|notice| notice.key == "distractions_host_rules");
        assert_eq!(
            added,
            [ChangeNotice {
                topic: RULES_TOPIC,
                key: "distractions_host_rules".into(),
            }]
        );

        replicas[1]
            .exec(&format!(
                "DELETE FROM distractions_host_rules WHERE id = {}",
                quoted(&host_key(HOST))
            ))
            .await;
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM distractions_host_rules")
                    .await,
                0
            );
        }

        replicas[0].exec(&host_rule_sql(HOST, 2_000)).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .scalar_text(
                        "SELECT host || ':' || enabled || ':' || created_at_ms
                         FROM distractions_host_rules"
                    )
                    .await,
                Some(format!("{HOST}:1:2000"))
            );
        }
    });
}

#[test]
fn distractions_removed_limit_takes_its_entries_and_custom_category_its_hosts() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!(
                "INSERT INTO distractions_custom_categories (id, created_at_ms, name)
                 VALUES ('games', 1000, 'Games');
                 INSERT INTO distractions_host_rules (id, list_kind, scope, host, category_id, created_at_ms)
                 VALUES ({}, 'category', 'games', 'play.example.com', 'games', 1000);
                 INSERT INTO distractions_usage_limits (id, created_at_ms, name, minutes_per_day)
                 VALUES ('limit-a', 1000, 'Video', 30);
                 INSERT INTO distractions_usage_limit_entries (limit_id, ordinal, id, website_host)
                 VALUES ('limit-a', 0, 'entry-a', 'video.example.com'),
                        ('limit-a', 1, 'entry-b', 'clips.example.com');",
                quoted(
                    &derived_key("host", &["category", "games", "play.example.com"])
                        .unwrap()
                        .to_string()
                ),
            ))
            .await;
        converge(&mut replicas).await;
        assert_eq!(
            replicas[1]
                .scalar_text(
                    "SELECT group_concat(website_host, ',') FROM
                     (SELECT website_host FROM distractions_usage_limit_entries ORDER BY ordinal)"
                )
                .await
                .as_deref(),
            Some("video.example.com,clips.example.com")
        );
        replicas[1]
            .exec(
                "DELETE FROM distractions_usage_limits WHERE id = 'limit-a';
                 DELETE FROM distractions_custom_categories WHERE id = 'games';",
            )
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            for table in [
                "distractions_usage_limits",
                "distractions_usage_limit_entries",
                "distractions_custom_categories",
                "distractions_host_rules",
            ] {
                assert_eq!(
                    replica
                        .count(&format!("SELECT count(*) FROM {table}"))
                        .await,
                    0,
                    "{table}"
                );
            }
        }
    });
}

/// The key of a usage total, as the flushing device derives it.
fn usage_key(device_id: &str, source_key: &str) -> String {
    let derived = derived_key(
        USAGE_TOTAL_KEY_PREFIX,
        &[device_id, "website", source_key, USAGE_DATE],
    )
    .expect("usage parts are valid")
    .to_string();
    let digest = derived
        .strip_prefix(&format!("{USAGE_TOTAL_KEY_PREFIX}:"))
        .expect("a derived key starts with its prefix");
    format!(
        "{USAGE_TOTAL_KEY_PREFIX}:{tag}:{digest}",
        tag = device_owner_tag(device_id)
    )
}

/// SQL flushing a usage total the way a device recomputes its own bucket: the total only grows
/// and the label follows the latest flush.
fn flush_sql(device_id: &str, source_key: &str, elapsed_seconds: i64) -> String {
    format!(
        "INSERT INTO distractions_usage_totals
            (id, device_id, source_type, source_key, local_date, elapsed_seconds, display_name)
         VALUES ({key}, {device}, 'website', {source}, '{USAGE_DATE}', {elapsed_seconds}, {source})
         ON CONFLICT (id) DO UPDATE SET
            elapsed_seconds = max(elapsed_seconds, excluded.elapsed_seconds),
            display_name = excluded.display_name
         WHERE excluded.elapsed_seconds > elapsed_seconds
            OR excluded.display_name IS NOT display_name;",
        key = quoted(&usage_key(device_id, source_key)),
        device = quoted(device_id),
        source = quoted(source_key),
    )
}

/// SQL recording one local usage sample, which never replicates.
fn sample_sql(id: &str, source_key: &str) -> String {
    format!(
        "INSERT INTO distractions_usage_samples
            (id, source_type, source_key, started_at_ms, elapsed_seconds, local_date, created_at_ms)
         VALUES ({}, 'website', {}, 1, 1, '{USAGE_DATE}', 1);",
        quoted(id),
        quoted(source_key),
    )
}

async fn usage_totals(replica: &mut Replica) -> BTreeMap<String, i64> {
    sqlx::query_as::<_, (String, i64)>("SELECT id, elapsed_seconds FROM distractions_usage_totals")
        .fetch_all(&mut replica.conn)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

#[test]
fn distractions_usage_totals_converge_to_each_devices_largest_flush() {
    const SOURCES: [&str; 2] = ["video.example.com", "news.example.com"];
    block_on(async {
        for seed in 0..USAGE_SEEDS {
            let mut rng = Rng(seed);
            let mut replicas = replicas(3).await;
            let mut expected = BTreeMap::new();
            let mut samples = vec![0_i64; replicas.len()];
            for step in 0..USAGE_STEPS {
                let device = rng.below(replicas.len());
                if rng.chance(60) {
                    let device_id = format!("device-{device}");
                    let source = SOURCES[rng.below(SOURCES.len())];
                    let key = usage_key(&device_id, source);
                    let total = expected.get(&key).copied().unwrap_or(0)
                        + 1
                        + i64::try_from(rng.below(600)).unwrap();
                    replicas[device]
                        .exec(&format!(
                            "{} {}",
                            sample_sql(&format!("sample-{step}"), source),
                            flush_sql(&device_id, source, total),
                        ))
                        .await;
                    samples[device] += 1;
                    expected.insert(key, total);
                    if rng.chance(50) {
                        replicas[device].seal().await.unwrap();
                    }
                } else {
                    let from = (device + 1 + rng.below(replicas.len() - 1)) % replicas.len();
                    replicas[from].seal().await.unwrap();
                    let notices = pull_notices(&mut replicas, device, from).await;
                    assert!(
                        notices
                            .iter()
                            .all(|notice| notice.key != "distractions_usage_totals"),
                        "usage totals report no rule change at seed {seed}"
                    );
                }
            }
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for (index, replica) in replicas.iter_mut().enumerate() {
                assert_eq!(usage_totals(replica).await, expected, "seed {seed}");
                assert_eq!(
                    replica
                        .count("SELECT count(*) FROM distractions_usage_samples")
                        .await,
                    samples[index],
                    "samples stay on the device that recorded them at seed {seed}"
                );
            }
        }
    });
}

/// SQL recording a browser block of `HOST` with its rule snapshot, during `run` when given.
pub(super) fn block_sql(id: &str, run: Option<&str>) -> String {
    let (run, phase) = match run {
        Some(run) => (quoted(run), "'focus'"),
        None => ("NULL".to_owned(), "NULL"),
    };
    format!(
        "INSERT INTO distractions_block_events
             (id, run_id, occurred_at, source_type, source_key, display_name, phase, decision,
              created_at)
         VALUES ({id}, {run}, '2027-01-05T09:10:00.000Z', 'browser', {host}, {host}, {phase},
                 'blocked', '2027-01-05T09:10:00.000Z');
         INSERT INTO distractions_block_event_rule_snapshots
             (block_event_id, rule_kind, rule_label, blocker_mode)
         VALUES ({id}, 'domain', {label}, 'blacklist');",
        id = quoted(id),
        host = quoted(HOST),
        label = quoted(&format!("blocked host: {HOST}")),
    )
}

/// Block events on `replica` as `id run rule_kind`, in id order.
async fn block_history(replica: &mut Replica) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT e.id || ' ' || coalesce(e.run_id, '-') || ' ' || coalesce(s.rule_kind, '-')
         FROM distractions_block_events e
         LEFT JOIN distractions_block_event_rule_snapshots s ON s.block_event_id = e.id
         ORDER BY e.id",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
}

#[test]
fn distractions_block_events_publish_at_once_or_with_their_focus_run_and_leave_with_it() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}{}",
                calendar_sql(),
                event_sql("event-a", "uid-a", "Deep work"),
                open_run_sql("run-a", "event-a", 2, 1),
                block_sql("block-free", None),
            ))
            .await;
        replicas[0]
            .exec(&block_sql("block-during", Some("run-a")))
            .await;
        converge(&mut replicas).await;
        assert_eq!(
            block_history(&mut replicas[1]).await,
            ["block-free - domain"],
            "an event outside a run publishes at once and one during an open run waits"
        );

        replicas[0].exec(&close_run_sql("run-a", 2)).await;
        converge(&mut replicas).await;
        replicas[0]
            .exec(&block_sql("block-late", Some("run-a")))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            block_history(&mut replicas[1]).await,
            [
                "block-during run-a domain",
                "block-free - domain",
                "block-late run-a domain"
            ],
            "events publish with their closed run, also when they arrive after it"
        );

        replicas[1]
            .exec("DELETE FROM pomodoro_runs WHERE id = 'run-a'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                block_history(replica).await,
                ["block-free - domain"],
                "deleting a run deletes its block events on replica {}",
                replica.index
            );
        }
    });
}

/// The stored total of `key`, when the row exists.
async fn stored_elapsed(replica: &mut Replica, key: &str) -> Option<i64> {
    let total = format!(
        "SELECT elapsed_seconds FROM distractions_usage_totals WHERE id = {}",
        quoted(key)
    );
    let rows = format!(
        "SELECT count(*) FROM distractions_usage_totals WHERE id = {}",
        quoted(key)
    );
    if replica.count(&rows).await == 0 {
        return None;
    }
    Some(replica.count(&total).await)
}

const HELD_INVALID: StoreOutcome = StoreOutcome::Stored {
    held: Some(HoldReason::Invalid),
};

/// A device signs a total for another device, as a create and as a write of the total alone. A
/// fourth replica holds both, keeps the honest row, and still takes the owner's own flushes.
#[test]
fn usage_total_from_another_device_is_held_invalid() {
    block_on(async {
        let mut replicas = replicas(4).await;
        let owner = device_id(0);
        let key = usage_key(&owner, HOST);

        replicas[0].exec(&flush_sql(&owner, HOST, 60)).await;
        replicas[0].seal().await.unwrap();
        let create = sealed_change(
            &mut replicas[0],
            USAGE_TOTALS_TABLE,
            &key,
            ChangeAction::Create,
        )
        .await;
        let forged = forge_change(&mut replicas[1], create).await;
        assert_eq!(replicas[3].receive(&[forged]).await, vec![HELD_INVALID]);
        replicas[3].apply_all().await.unwrap();
        assert_eq!(stored_elapsed(&mut replicas[3], &key).await, None);

        let (victim, source) = pair(&mut replicas, 3, 0);
        victim.pull(source).await;
        assert_eq!(stored_elapsed(&mut replicas[3], &key).await, Some(60));

        replicas[0].exec(&flush_sql(&owner, HOST, 9_000)).await;
        replicas[0].seal().await.unwrap();
        let write = sealed_change(
            &mut replicas[0],
            USAGE_TOTALS_TABLE,
            &key,
            ChangeAction::Write,
        )
        .await;
        let forged = forge_change(&mut replicas[2], write).await;
        assert_eq!(replicas[3].receive(&[forged]).await, vec![HELD_INVALID]);
        replicas[3].apply_all().await.unwrap();
        assert_eq!(stored_elapsed(&mut replicas[3], &key).await, Some(60));

        let (victim, source) = pair(&mut replicas, 3, 0);
        victim.pull(source).await;
        assert_eq!(stored_elapsed(&mut replicas[3], &key).await, Some(9_000));
    });
}

/// A device signing under its own tag cannot name another device in the row.
#[test]
fn create_with_mismatched_device_column_is_held() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let owner = device_id(0);
        let key = usage_key(&owner, HOST);
        replicas[0].exec(&flush_sql(&owner, HOST, 60)).await;
        replicas[0].seal().await.unwrap();
        let mut create = sealed_change(
            &mut replicas[0],
            USAGE_TOTALS_TABLE,
            &key,
            ChangeAction::Create,
        )
        .await;
        let (receiver, source) = pair(&mut replicas, 1, 0);
        receiver.pull(source).await;

        let forged_key = usage_key(&owner, "other.example.com");
        create.row = RowKey::new(forged_key.clone()).unwrap();
        let bucket = create
            .groups
            .iter_mut()
            .find(|value| value.group == usage_total_group::BUCKET)
            .expect("a create carries its bucket");
        let mut fields = bucket.value.fields().to_vec();
        fields[0] = Field::Text(device_id(1));
        *bucket = GroupValue::new(
            USAGE_TOTALS_TABLE,
            usage_total_group::BUCKET,
            Value::new(fields).unwrap(),
        );

        let forged = forge_change(&mut replicas[0], create).await;
        assert_eq!(replicas[1].receive(&[forged]).await, vec![HELD_INVALID]);
        replicas[1].apply_all().await.unwrap();
        assert_eq!(stored_elapsed(&mut replicas[1], &forged_key).await, None);
    });
}

/// A copied vault folder shares its device id, so both installations hold the owner tag and
/// their flushes of one total merge to the larger.
#[test]
fn cloned_device_usage_totals_converge_to_the_larger_flush() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let mut clone = replicas[0].clone_device().await;
        clone.rotate();
        let device = device_id(0);
        replicas[0].exec(&flush_sql(&device, HOST, 500)).await;
        clone.exec(&flush_sql(&device, HOST, 200)).await;
        replicas.push(clone);

        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let key = usage_key(&device, HOST);
        for replica in &mut replicas {
            assert_eq!(stored_elapsed(replica, &key).await, Some(500));
        }
    });
}

/// Forgers sign totals of honest devices with honest flushes around them and restates between
/// them. Every honest replica stores the same operations, holds each forgery, and keeps the
/// honest total of every device.
#[test]
fn simulation_foreign_owned_writes_converge_as_held() {
    const HONEST: usize = 3;
    block_on(async {
        let mut honest = replicas(HONEST * 2).await;
        let mut forgers = honest.split_off(HONEST);
        let mut expected = BTreeMap::new();
        let mut foreign_owned_rows = 0;
        for (round, forger) in forgers.iter_mut().enumerate() {
            let owner = device_id(round);
            let key = usage_key(&owner, HOST);
            let total = 60 * i64::try_from(round + 1).unwrap();
            honest[round].exec(&flush_sql(&owner, HOST, total)).await;
            honest[round].seal().await.unwrap();
            expected.insert(key.clone(), total);

            let create = sealed_change(
                &mut honest[round],
                USAGE_TOTALS_TABLE,
                &key,
                ChangeAction::Create,
            )
            .await;
            let forged = forge_change(forger, with_elapsed(create, MAX_USAGE_DAY_SECONDS)).await;
            let victim = &mut honest[(round + 1) % HONEST];
            assert_eq!(victim.receive(&[forged]).await, vec![HELD_INVALID]);
            restate_usage_totals(victim).await;
            foreign_owned_rows += victim.seal().await.unwrap().foreign_owned_rows;
            converge(&mut honest).await;
        }
        assert!(foreign_owned_rows > 0, "a restate met another device's row");
        assert_converged(&mut honest).await;
        for replica in &mut honest {
            assert_eq!(usage_totals(replica).await, expected);
        }
    });
}

/// `change` with its total set to `elapsed_seconds`.
fn with_elapsed(mut change: Change, elapsed_seconds: i64) -> Change {
    let elapsed = change
        .groups
        .iter_mut()
        .find(|value| value.group == usage_total_group::ELAPSED)
        .expect("a create carries its total");
    *elapsed = GroupValue::new(
        USAGE_TOTALS_TABLE,
        usage_total_group::ELAPSED,
        Value::new(vec![Field::Integer(elapsed_seconds)]).unwrap(),
    );
    change
}

/// A write of the one group `group` of the row `key`.
fn group_write(table: &TableSpec, key: &str, group: GroupId, field: Field) -> Change {
    Change {
        table: table.id,
        row: RowKey::new(key.to_owned()).unwrap(),
        action: ChangeAction::Write,
        groups: vec![GroupValue::new(
            table.id,
            group,
            Value::new(vec![field]).unwrap(),
        )],
        replaced_by: None,
        cause: None,
    }
}

#[test]
fn property_owner_tag_binds_only_own_device() {
    let mut rng = Rng(0);
    for _ in 0..OWNER_PROPERTY_ROUNDS {
        let owner = format!("device-{}", rng.below(OWNER_PROPERTY_DEVICES));
        let writer = format!("device-{}", rng.below(OWNER_PROPERTY_DEVICES));
        let key = usage_key(&owner, HOST);
        let same = owner == writer;

        assert!(validate::device_owns_row(&USAGE_TOTALS, &key, &owner));
        assert_eq!(
            validate::device_owns_row(&USAGE_TOTALS, &key, &writer),
            same
        );
        let change = group_write(
            &USAGE_TOTALS,
            &key,
            usage_total_group::ELAPSED,
            Field::Integer(60),
        );
        assert_eq!(
            validate::writer_change(&USAGE_TOTALS, &change, &writer),
            if same {
                Ok(())
            } else {
                Err(validate::OWNER_MISMATCH)
            }
        );

        let tag = device_owner_tag(&owner);
        for almost in [
            format!("{USAGE_TOTAL_OWNER_PREFIX}{tag}"),
            format!("{USAGE_TOTAL_OWNER_PREFIX}{tag}0:digest"),
            format!("{tag}:digest"),
        ] {
            assert!(!validate::device_owns_row(&USAGE_TOTALS, &almost, &owner));
        }
    }
}

/// Existence groups merge on their own, so any device may keep a row alive, while every other
/// group and every create stays with the owner.
#[test]
fn owner_binding_leaves_existence_writes_to_their_group_merge() {
    const OWNED_HOSTS: TableSpec = TableSpec {
        owner: Some(RowOwner::KeyIsDevice {
            device_column: "id",
        }),
        ..HOST_RULES
    };
    let (owner, other) = (device_id(0), device_id(1));
    let exists = group_write(&OWNED_HOSTS, &owner, host_group::EXISTS, Field::Integer(1));
    assert_eq!(
        validate::writer_change(&OWNED_HOSTS, &exists, &other),
        Ok(())
    );

    let enabled = group_write(&OWNED_HOSTS, &owner, host_group::ENABLED, Field::Integer(0));
    assert_eq!(
        validate::writer_change(&OWNED_HOSTS, &enabled, &other),
        Err(validate::OWNER_MISMATCH)
    );
    assert_eq!(
        validate::writer_change(&OWNED_HOSTS, &enabled, &owner),
        Ok(())
    );

    let create = Change {
        table: HOST_RULES_TABLE,
        action: ChangeAction::Create,
        ..exists
    };
    assert_eq!(
        validate::writer_change(&OWNED_HOSTS, &create, &other),
        Err(validate::OWNER_MISMATCH)
    );
}

/// Forgets that `replica` restated usage totals and restates them.
async fn restate_usage_totals(replica: &mut Replica) {
    replica
        .exec(&format!(
            "DELETE FROM sync_seeded_tables WHERE table_id = {}",
            USAGE_TOTALS_TABLE.0
        ))
        .await;
    let now = replica.now_ms;
    replica
        .engine
        .seed_captures(&mut replica.conn, now)
        .await
        .unwrap();
}

/// Restating a table publishes the local device's rows and consumes the captures of rows that
/// belong to other devices.
#[test]
fn restate_does_not_seal_foreign_owned_rows() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let (own, foreign) = (device_id(1), device_id(0));
        let replica = &mut replicas[1];
        replica
            .exec(&format!(
                "{}{}",
                flush_sql(&own, HOST, 30),
                flush_sql(&foreign, HOST, 40)
            ))
            .await;
        restate_usage_totals(replica).await;

        let report = replica.seal().await.unwrap();

        assert!(report.invalid_rows.is_empty(), "{:?}", report.invalid_rows);
        assert_eq!(report.changes, 1);
        assert_eq!(report.foreign_owned_rows, 1);
        assert_eq!(
            replica
                .count(&format!(
                    "SELECT count(*) FROM sync_capture WHERE table_id = {}",
                    USAGE_TOTALS_TABLE.0
                ))
                .await,
            0
        );
        let (own_key, foreign_key) = (usage_key(&own, HOST), usage_key(&foreign, HOST));
        sealed_change(replica, USAGE_TOTALS_TABLE, &own_key, ChangeAction::Create).await;
        assert_eq!(
            stored_elapsed(replica, &foreign_key).await,
            Some(40),
            "the local copy of a foreign row stays"
        );
    });
}

/// Another device cannot republish a removed device's total, so restoring the late change
/// records the decision and seals nothing.
#[test]
fn restoring_a_removed_devices_usage_total_seals_nothing() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 71).await;
        let mut removed = Replica::new(1, 71).await;
        let mut honest = Replica::new(2, 71).await;
        let device = removed.device_id.clone();
        removed.exec(&flush_sql(&device, HOST, 60)).await;
        removed.seal().await.unwrap();
        deliver(&mut honest, &mut removed).await;
        let removal = anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        deliver(&mut anchor, &mut honest).await;

        let key = usage_key(&device, HOST);
        let listed = late_changes(&mut anchor).await;
        let late = listed
            .iter()
            .find(|late| late.row_key == key)
            .expect("the total is a late change")
            .clone();
        assert!(
            !late.restorable,
            "the listing says restoring writes nothing back"
        );
        let before = anchor.stored_vector().await;
        let report = anchor
            .engine
            .restore_late_change(
                &mut anchor.conn,
                &anchor.ctx,
                &late,
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(report.sealed_ops, 0);
        assert_eq!(anchor.stored_vector().await, before);
        assert_eq!(late_changes(&mut anchor).await.len(), listed.len());

        anchor
            .sequence(Entry::ResolveLate(ResolveLate {
                removal_revision: removal,
                rows: vec![LateRow {
                    table: USAGE_TOTALS_TABLE,
                    row: RowKey::new(key.clone()).unwrap(),
                    resolution: LateResolution::Restore,
                }],
            }))
            .await
            .unwrap()
            .unwrap();
        anchor.apply_all().await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        for replica in [&mut anchor, &mut honest] {
            assert!(late_changes(replica).await.is_empty());
            assert_eq!(stored_elapsed(replica, &key).await, None);
        }
    });
}

/// Offers `to` every operation `from` sealed, including those at sequences `to` already stores
/// from another branch, so it stores the fork evidence, then delivers the rest.
async fn cross(to: &mut Replica, from: &mut Replica) {
    let own = from.own_ops().await;
    to.receive(&own).await;
    deliver(to, from).await;
}

/// No writer but the owner's can seal a total again, so after a fork the anchor keeps the
/// excluded branch total with no version while a member that retained restores the total it
/// displaced. The owner's re-seal of its own branch writes the total again and settles both.
#[test]
fn a_forked_total_the_anchor_cannot_seal_again_settles_on_the_owners_reseal() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 91).await;
        let mut original = Replica::new(1, 91).await;
        let mut member = Replica::new(2, 91).await;
        let device = original.device_id.clone();
        let key = usage_key(&device, HOST);
        original.exec(&flush_sql(&device, HOST, 60)).await;
        original.seal().await.unwrap();
        deliver(&mut anchor, &mut original).await;
        deliver(&mut member, &mut original).await;
        let mut clone = original.clone_device().await;
        original.exec(&flush_sql(&device, HOST, 100)).await;
        original.seal().await.unwrap();
        clone.exec(&flush_sql(&device, HOST, 300)).await;
        clone.seal().await.unwrap();
        deliver(&mut anchor, &mut clone).await;
        deliver(&mut member, &mut clone).await;
        assert_eq!(stored_elapsed(&mut anchor, &key).await, Some(300));

        cross(&mut anchor, &mut original).await;
        deliver(&mut member, &mut anchor).await;
        assert_eq!(anchor.evidence().await.len(), 1);
        assert_eq!(member.evidence().await.len(), 1);
        let report = anchor.seal().await.unwrap();
        assert!(report.foreign_owned_rows > 0, "the anchor skips the total");
        assert_eq!(stored_elapsed(&mut anchor, &key).await, Some(300));
        assert_eq!(stored_elapsed(&mut member, &key).await, Some(60));

        drop(clone);
        original.take_evidence(&mut anchor).await;
        let forked = original.writer();
        original.apply_all().await.unwrap();
        assert_ne!(
            original.writer(),
            forked,
            "the original re-seals its branch"
        );
        let mut replicas = vec![anchor, member, original];
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(stored_elapsed(replica, &key).await, Some(100));
        }
    });
}

/// A replica can hold another device's total with no sync state, as one does after a new space
/// is founded over a vault that already held it. The owner's Create of that row applies over it,
/// with the owner's value, on every replica.
#[test]
fn an_owners_create_over_a_total_without_sync_state_applies_everywhere() {
    block_on(async {
        let mut replicas = replicas(3).await;
        let owner = device_id(1);
        let key = usage_key(&owner, HOST);
        for (index, elapsed) in [(0, 300), (2, 40)] {
            replicas[index]
                .exec(&flush_sql(&owner, HOST, elapsed))
                .await;
            let report = replicas[index].seal().await.unwrap();
            assert_eq!(report.foreign_owned_rows, 1);
        }
        replicas[1].exec(&flush_sql(&owner, HOST, 100)).await;
        replicas[1].seal().await.unwrap();
        sealed_change(
            &mut replicas[1],
            USAGE_TOTALS_TABLE,
            &key,
            ChangeAction::Create,
        )
        .await;

        let ops = replicas[1].own_ops().await;
        for index in [0, 2] {
            let outcomes = replicas[index].receive(&ops).await;
            assert!(
                outcomes.iter().all(|outcome| !matches!(
                    outcome,
                    StoreOutcome::Refused(_) | StoreOutcome::Stored { held: Some(_) }
                )),
                "{outcomes:?}"
            );
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(stored_elapsed(replica, &key).await, Some(100));
            replica.check_invariants().await;
        }
    });
}
