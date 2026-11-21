//! Chat daily agent usage across replicas: each device's summary of an agent's day arrives
//! with every count, the counts converge to each device's largest flush whatever order replicas
//! exchange in, and a summary one device signs for another is held.

use super::*;
use std::collections::BTreeMap;

use crate::manifest::vault::chat::AGENT_USAGE_DAILY_TABLE;
use crate::tests::replica::{pair, sealed_change};
use ganbaru_sync_contracts::chat_usage::agent_usage_key;

/// The local date every summary in these scenarios counts.
const USAGE_DATE: &str = "2026-11-12";

/// Seeds of the concurrent flush scenario.
const FLUSH_SEEDS: u64 = 8;

/// Steps of one concurrent flush scenario.
const FLUSH_STEPS: usize = 24;

/// A summary's counted tokens, working seconds, and runs.
type Counts = (i64, i64, i64);

const HELD_INVALID: StoreOutcome = StoreOutcome::Stored {
    held: Some(HoldReason::Invalid),
};

/// The key of `device_id`'s summary of [`AGENT`] on [`USAGE_DATE`].
fn usage_key(device_id: &str) -> String {
    agent_usage_key(device_id, AGENT, USAGE_DATE)
        .expect("usage parts are valid")
        .to_string()
}

/// SQL flushing `device_id`'s summary the way the Chat flush does: every count only grows.
fn flush_sql(device_id: &str, (tokens, seconds, runs): Counts) -> String {
    format!(
        "INSERT INTO chat_agent_usage_daily
            (id, device_id, agent_id, local_date, counted_tokens, working_seconds, run_count)
         VALUES ({key}, {device}, {agent}, '{USAGE_DATE}', {tokens}, {seconds}, {runs})
         ON CONFLICT (id) DO UPDATE SET
            counted_tokens = max(counted_tokens, excluded.counted_tokens),
            working_seconds = max(working_seconds, excluded.working_seconds),
            run_count = max(run_count, excluded.run_count)
         WHERE excluded.counted_tokens > counted_tokens
            OR excluded.working_seconds > working_seconds
            OR excluded.run_count > run_count;",
        key = quoted(&usage_key(device_id)),
        device = quoted(device_id),
        agent = quoted(AGENT),
    )
}

/// Every stored summary by key.
async fn summaries(replica: &mut Replica) -> BTreeMap<String, Counts> {
    sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT id, counted_tokens, working_seconds, run_count FROM chat_agent_usage_daily",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
    .into_iter()
    .map(|(id, tokens, seconds, runs)| (id, (tokens, seconds, runs)))
    .collect()
}

/// Replicas that share [`AGENT`].
async fn replicas_with_agent(count: usize) -> Vec<Replica> {
    let mut replicas = replicas(count).await;
    replicas[0].exec(&agent_sql()).await;
    converge(&mut replicas).await;
    replicas
}

#[test]
fn chat_agent_usage_round_trips_with_every_count() {
    block_on(async {
        let mut replicas = replicas_with_agent(2).await;
        let owner = device_id(0);
        replicas[0].exec(&flush_sql(&owner, (1_200, 95, 2))).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            summaries(&mut replicas[1]).await,
            BTreeMap::from([(usage_key(&owner), (1_200, 95, 2))])
        );
        assert_eq!(
            replicas[1]
                .scalar_text(
                    "SELECT device_id || ' ' || agent_id || ' ' || local_date
                     FROM chat_agent_usage_daily"
                )
                .await
                .as_deref(),
            Some("device-0 participant:reviewer 2026-11-12")
        );
    });
}

#[test]
fn chat_agent_usage_converges_to_each_devices_largest_flush() {
    block_on(async {
        for seed in 0..FLUSH_SEEDS {
            let mut rng = Rng(seed);
            let mut replicas = replicas_with_agent(3).await;
            let mut expected: BTreeMap<String, Counts> = BTreeMap::new();
            for _ in 0..FLUSH_STEPS {
                let device = rng.below(replicas.len());
                if rng.chance(60) {
                    let device_id = device_id(device);
                    let key = usage_key(&device_id);
                    let (tokens, seconds, runs) = expected.get(&key).copied().unwrap_or_default();
                    let grown = (
                        tokens + i64::try_from(rng.below(5_000)).unwrap(),
                        seconds + i64::try_from(rng.below(120)).unwrap(),
                        runs + i64::try_from(rng.below(2)).unwrap(),
                    );
                    replicas[device].exec(&flush_sql(&device_id, grown)).await;
                    expected.insert(key, grown);
                    if rng.chance(50) {
                        replicas[device].seal().await.unwrap();
                    }
                } else {
                    let from = (device + 1 + rng.below(replicas.len() - 1)) % replicas.len();
                    let (target, source) = pair(&mut replicas, device, from);
                    source.seal().await.unwrap();
                    target.pull(source).await;
                }
            }
            converge(&mut replicas).await;
            assert_converged(&mut replicas).await;
            for replica in &mut replicas {
                assert_eq!(summaries(replica).await, expected, "seed {seed}");
            }
        }
    });
}

/// A device signs a summary for another device. Another replica holds it and still takes the
/// owner's own flush.
#[test]
fn chat_agent_usage_from_another_device_is_held_invalid() {
    block_on(async {
        let mut replicas = replicas_with_agent(3).await;
        let owner = device_id(0);
        let key = usage_key(&owner);
        replicas[0].exec(&flush_sql(&owner, (10, 5, 1))).await;
        replicas[0].seal().await.unwrap();
        let create = sealed_change(
            &mut replicas[0],
            AGENT_USAGE_DAILY_TABLE,
            &key,
            ChangeAction::Create,
        )
        .await;
        let forged = forge_change(&mut replicas[1], create).await;
        assert_eq!(replicas[2].receive(&[forged]).await, vec![HELD_INVALID]);
        replicas[2].apply_all().await.unwrap();
        assert!(summaries(&mut replicas[2]).await.is_empty());

        let (target, source) = pair(&mut replicas, 2, 0);
        target.pull(source).await;
        assert_eq!(
            summaries(&mut replicas[2]).await,
            BTreeMap::from([(key, (10, 5, 1))])
        );
    });
}
