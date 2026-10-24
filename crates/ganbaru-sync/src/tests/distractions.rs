//! Distraction rules across replicas: toggles of different settings groups both survive,
//! concurrent toggles of one rule converge, a host rule removed on one replica and added again
//! on another comes back under the same key everywhere, and remote rule changes report the
//! changed tables. Daily usage totals converge to each device's largest flush whatever order
//! flushes and exchanges interleave in, and usage samples never leave their device.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::derived_key;

use super::block_on;
use super::replica::{Replica, converge, pair, quoted};
use super::sim::Rng;
use crate::ChangeNotice;
use crate::domains::distractions::RULES_TOPIC;
use crate::manifest::vault::distractions::USAGE_TOTAL_KEY_PREFIX;

const HOST: &str = "video.example.com";

/// Seeds of the interleaved usage flush scenario.
const USAGE_SEEDS: u64 = 6;

/// Steps of one usage scenario: a flush or a one-way exchange each.
const USAGE_STEPS: usize = 24;

/// Local day every usage scenario records.
const USAGE_DATE: &str = "2026-10-23";

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
    derived_key(
        USAGE_TOTAL_KEY_PREFIX,
        &[device_id, "website", source_key, USAGE_DATE],
    )
    .expect("usage parts are valid")
    .to_string()
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
            // A cloned installation shares one device id; the larger flush wins everywhere.
            replicas[1]
                .exec(&flush_sql("cloned", SOURCES[0], 500))
                .await;
            replicas[2]
                .exec(&flush_sql("cloned", SOURCES[0], 200))
                .await;
            expected.insert(usage_key("cloned", SOURCES[0]), 500);
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
