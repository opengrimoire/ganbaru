//! Chat provider device status across replicas: each device's status of a provider family
//! arrives with its catalog and limits, a later change replaces it, and a status one device
//! signs for another is held.

use super::*;
use std::collections::BTreeMap;

use crate::manifest::vault::chat::PROVIDER_STATUS_TABLE;
use crate::tests::replica::{pair, sealed_change};
use ganbaru_sync_contracts::chat_provider_status::provider_status_key;

/// The provider family every status in these scenarios names.
const FAMILY: &str = "codex";

/// A canonical catalog with one model.
const CATALOG: &str =
    r#"{"models":[{"availability":"available","displayName":"GPT-5","id":"gpt-5","options":[]}]}"#;

/// The canonical catalog of a provider with no models.
const EMPTY_CATALOG: &str = r#"{"models":[]}"#;

/// Canonical limits of a provider that is not limited.
const LIMITS: &str = r#"{"limited":false,"providerData":null,"resetsAt":null}"#;

const HELD_INVALID: StoreOutcome = StoreOutcome::Stored {
    held: Some(HoldReason::Invalid),
};

/// A status's availability, catalog, and limits.
type Status = (i64, String, Option<String>);

/// The key of `device_id`'s status of [`FAMILY`].
fn status_key(device_id: &str) -> String {
    provider_status_key(device_id, FAMILY)
        .expect("status parts are valid")
        .to_string()
}

/// SQL publishing `device_id`'s status the way the Chat writer does: a row changes only when
/// its content does.
fn publish_sql(device_id: &str, available: bool, catalog: &str, limits: Option<&str>) -> String {
    let limits_sql = limits.map_or_else(|| "NULL".to_string(), quoted);
    let observed = if limits.is_some() {
        quoted(CREATED)
    } else {
        "NULL".to_string()
    };
    format!(
        "INSERT INTO chat_provider_device_status
            (id, device_id, provider_family_id, available, status_changed_at, catalog_data,
             catalog_changed_at, limits_data, limits_observed_at)
         VALUES ({key}, {device}, {family}, {available}, {at}, {catalog}, {at}, {limits_sql},
             {observed})
         ON CONFLICT (id) DO UPDATE SET
            available = excluded.available,
            catalog_data = excluded.catalog_data,
            limits_data = excluded.limits_data,
            limits_observed_at = excluded.limits_observed_at
         WHERE available IS NOT excluded.available
            OR catalog_data IS NOT excluded.catalog_data
            OR limits_data IS NOT excluded.limits_data;",
        key = quoted(&status_key(device_id)),
        device = quoted(device_id),
        family = quoted(FAMILY),
        available = i64::from(available),
        at = quoted(CREATED),
        catalog = quoted(catalog),
    )
}

/// Every stored status by key.
async fn statuses(replica: &mut Replica) -> BTreeMap<String, Status> {
    sqlx::query_as::<_, (String, i64, String, Option<String>)>(
        "SELECT id, available, catalog_data, limits_data FROM chat_provider_device_status",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
    .into_iter()
    .map(|(id, available, catalog, limits)| (id, (available, catalog, limits)))
    .collect()
}

#[test]
fn chat_provider_status_round_trips_and_follows_its_device() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let owner = device_id(0);
        replicas[0]
            .exec(&publish_sql(&owner, true, CATALOG, Some(LIMITS)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            statuses(&mut replicas[1]).await,
            BTreeMap::from([(
                status_key(&owner),
                (1, CATALOG.to_string(), Some(LIMITS.to_string()))
            )])
        );

        replicas[0]
            .exec(&publish_sql(&owner, false, EMPTY_CATALOG, None))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            statuses(&mut replicas[1]).await,
            BTreeMap::from([(status_key(&owner), (0, EMPTY_CATALOG.to_string(), None))])
        );
    });
}

/// A device signs a status for another device. Another replica holds it and still takes the
/// owner's own status.
#[test]
fn chat_provider_status_from_another_device_is_held_invalid() {
    block_on(async {
        let mut replicas = replicas(3).await;
        let owner = device_id(0);
        let key = status_key(&owner);
        replicas[0]
            .exec(&publish_sql(&owner, true, CATALOG, None))
            .await;
        replicas[0].seal().await.unwrap();
        let create = sealed_change(
            &mut replicas[0],
            PROVIDER_STATUS_TABLE,
            &key,
            ChangeAction::Create,
        )
        .await;
        let forged = forge_change(&mut replicas[1], create).await;
        assert_eq!(replicas[2].receive(&[forged]).await, vec![HELD_INVALID]);
        replicas[2].apply_all().await.unwrap();
        assert!(statuses(&mut replicas[2]).await.is_empty());

        let (target, source) = pair(&mut replicas, 2, 0);
        target.pull(source).await;
        assert_eq!(
            statuses(&mut replicas[2]).await,
            BTreeMap::from([(key, (1, CATALOG.to_string(), None))])
        );
    });
}
