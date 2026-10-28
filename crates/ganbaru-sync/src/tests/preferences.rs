//! Portable preferences across replicas: values merge per key, concurrent creation of one key
//! converges, and remote changes report the changed keys.

use super::block_on;
use super::replica::{Replica, converge, pair};
use crate::ChangeNotice;
use crate::domains::preferences::PREFERENCE_TOPIC;

const EARLY: &str = "2027-01-01T00:00:00.000Z";
const LATE: &str = "2027-01-02T00:00:00.000Z";

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 16).await);
    }
    replicas
}

async fn set(replica: &mut Replica, key: &str, value: &str, updated_at: &str) {
    replica
        .exec(&format!(
            "INSERT INTO vault_preferences (key, value, updated_at) VALUES ('{key}', '{value}', '{updated_at}')
             ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
        ))
        .await;
}

async fn value(replica: &mut Replica, key: &str) -> Option<String> {
    replica
        .scalar_text(&format!(
            "SELECT value || '@' || updated_at FROM vault_preferences WHERE key = '{key}'"
        ))
        .await
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

fn notice(key: &str) -> ChangeNotice {
    ChangeNotice {
        topic: PREFERENCE_TOPIC,
        key: key.into(),
    }
}

#[test]
fn preferences_remote_changes_report_their_keys() {
    block_on(async {
        let mut replicas = replicas(2).await;
        set(&mut replicas[0], "profile.color", "\"#ff0000\"", EARLY).await;
        set(&mut replicas[0], "preferences.language", "\"es\"", EARLY).await;
        replicas[0].seal().await.unwrap();
        let mut notices = pull_notices(&mut replicas, 1, 0).await;
        notices.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(
            notices,
            [notice("preferences.language"), notice("profile.color")]
        );
        assert_eq!(
            value(&mut replicas[1], "profile.color").await.as_deref(),
            Some(format!("\"#ff0000\"@{EARLY}").as_str())
        );

        set(&mut replicas[1], "profile.color", "\"#00ff00\"", LATE).await;
        replicas[1].seal().await.unwrap();
        assert_eq!(
            pull_notices(&mut replicas, 0, 1).await,
            [notice("profile.color")]
        );
        assert_eq!(
            value(&mut replicas[0], "profile.color").await.as_deref(),
            Some(format!("\"#00ff00\"@{LATE}").as_str())
        );

        replicas[0]
            .exec("DELETE FROM vault_preferences WHERE key = 'preferences.language'")
            .await;
        replicas[0].seal().await.unwrap();
        assert_eq!(
            pull_notices(&mut replicas, 1, 0).await,
            [notice("preferences.language")]
        );
        assert_eq!(value(&mut replicas[1], "preferences.language").await, None);
    });
}

#[test]
fn preferences_concurrent_creation_of_one_key_converges() {
    block_on(async {
        let mut replicas = replicas(3).await;
        set(&mut replicas[0], "focus.length", "25", EARLY).await;
        set(&mut replicas[1], "focus.length", "50", LATE).await;
        set(&mut replicas[2], "profile.name", "\"Ana\"", EARLY).await;
        converge(&mut replicas).await;
        let first = value(&mut replicas[0], "focus.length").await;
        assert!(first.is_some());
        for replica in &mut replicas[1..] {
            assert_eq!(value(replica, "focus.length").await, first);
            assert_eq!(
                value(replica, "profile.name").await.as_deref(),
                Some(format!("\"Ana\"@{EARLY}").as_str())
            );
        }
        // The latest timestamp wins its own group whichever value wins the register.
        assert!(first.unwrap().ends_with(LATE));
        let dump = replicas[0].domain_dump().await;
        for replica in &mut replicas[1..] {
            assert_eq!(replica.domain_dump().await, dump);
        }
    });
}

#[test]
fn preferences_chat_behavior_replicates_and_reports_its_key() {
    block_on(async {
        let mut replicas = replicas(2).await;
        set(
            &mut replicas[0],
            "chat.behavior.sendKey",
            "\"mod_enter\"",
            EARLY,
        )
        .await;
        set(
            &mut replicas[0],
            "chat.behavior.terminalScrollbackLines",
            "2000",
            EARLY,
        )
        .await;
        replicas[0].seal().await.unwrap();
        let mut notices = pull_notices(&mut replicas, 1, 0).await;
        notices.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(
            notices,
            [
                notice("chat.behavior.sendKey"),
                notice("chat.behavior.terminalScrollbackLines")
            ]
        );
        assert_eq!(
            value(&mut replicas[1], "chat.behavior.sendKey")
                .await
                .as_deref(),
            Some(format!("\"mod_enter\"@{EARLY}").as_str())
        );

        set(&mut replicas[1], "chat.behavior.sendKey", "\"enter\"", LATE).await;
        replicas[1].seal().await.unwrap();
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                value(replica, "chat.behavior.sendKey").await.as_deref(),
                Some(format!("\"enter\"@{LATE}").as_str())
            );
            assert_eq!(
                value(replica, "chat.behavior.terminalScrollbackLines")
                    .await
                    .as_deref(),
                Some(format!("2000@{EARLY}").as_str())
            );
        }
    });
}
