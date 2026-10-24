//! Distraction rules across replicas: toggles of different settings groups both survive,
//! concurrent toggles of one rule converge, a host rule removed on one replica and added again
//! on another comes back under the same key everywhere, and remote rule changes report the
//! changed tables.

use ganbaru_sync_contracts::derived_key;

use super::block_on;
use super::replica::{Replica, converge, pair, quoted};
use crate::ChangeNotice;
use crate::domains::distractions::RULES_TOPIC;

const HOST: &str = "video.example.com";

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
