//! Stability on the coordinator: which devices count and what each acknowledges.

use ganbaru_sync_contracts::VersionVector;

use super::block_on;
use super::replica::{Replica, device_id, person, template_vector};
use crate::stability::{CountedDevice, DORMANCY_MS, DeviceStanding, Stability};

async fn acknowledge(replica: &mut Replica) {
    replica
        .engine
        .acknowledge(
            &mut replica.conn,
            &replica.ctx,
            &mut replica.identity.local(),
            replica.now_ms,
        )
        .await
        .unwrap();
}

async fn deliver(to: &mut Replica, from: &mut Replica) {
    to.pull(from).await;
    to.apply_all().await.unwrap();
}

fn counted(index: usize, since_ms: u64) -> CountedDevice {
    CountedDevice {
        device_id: device_id(index),
        linked_at_ms: Some(since_ms),
    }
}

async fn stability(hub: &mut Replica, devices: &[CountedDevice]) -> Stability {
    hub.engine
        .stability(&mut hub.conn, &hub.ctx, devices, hub.now_ms)
        .await
        .unwrap()
}

/// The coordinator, replica A with one tag, and replica B, each having acknowledged what the
/// others sent through the coordinator.
async fn linked() -> (Replica, Replica, Replica) {
    let mut hub = Replica::new(0, 1).await;
    let mut a = Replica::new(1, 1).await;
    let mut b = Replica::new(2, 1).await;
    a.exec("INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0')")
        .await;
    a.seal().await.unwrap();
    deliver(&mut hub, &mut a).await;
    acknowledge(&mut hub).await;
    deliver(&mut b, &mut hub).await;
    acknowledge(&mut b).await;
    deliver(&mut hub, &mut b).await;
    (hub, a, b)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn stability_is_what_every_counted_device_and_the_coordinator_hold() {
    block_on(async {
        let (mut hub, a, b) = linked().await;
        let since = hub.now_ms;
        let result = stability(&mut hub, &[counted(1, since), counted(2, since)]).await;
        let mut expected = template_vector(&person());
        expected.set(a.writer(), 2);
        assert_eq!(result.stable, expected);
        assert_eq!(result.devices.len(), 2);
        assert!(result.devices[1].acknowledged.covers(&a.writer(), 2));
        assert!(result.devices[1].acknowledged.covers(&hub.writer(), 2));
        assert!(result.devices[1].acknowledged.covers(&b.writer(), 2));
        assert!(
            result
                .devices
                .iter()
                .all(|device| device.standing == DeviceStanding::Counted)
        );

        // Without counted devices, stability is everything the coordinator applied.
        let alone = stability(&mut hub, &[]).await;
        assert_eq!(alone.stable, hub.applied_vector().await);
    });
}

#[test]
fn stability_counts_a_writerless_member_as_holding_nothing() {
    block_on(async {
        let (mut hub, _a, _b) = linked().await;
        let since = hub.now_ms;
        let result = stability(
            &mut hub,
            &[counted(1, since), counted(2, since), counted(5, since)],
        )
        .await;
        assert!(result.stable.is_empty());
        assert_eq!(result.devices[2].acknowledged, VersionVector::new());
        assert_eq!(result.devices[2].last_stored_ms, None);
    });
}

#[test]
fn stability_ignores_a_dormant_device_until_it_returns() {
    block_on(async {
        let (mut hub, a, _b) = linked().await;
        let since = hub.now_ms;
        let long_ago = since - DORMANCY_MS - 1;
        let devices = [counted(1, since), counted(2, since), counted(5, long_ago)];
        let result = stability(&mut hub, &devices).await;
        assert_eq!(result.devices[2].standing, DeviceStanding::Dormant);
        assert!(result.stable.covers(&a.writer(), 2));

        // A dormancy counted from the link time ends exactly at the limit.
        let edge = [counted(5, since - DORMANCY_MS)];
        assert_eq!(
            stability(&mut hub, &edge).await.devices[0].standing,
            DeviceStanding::Counted
        );

        let mut returning = Replica::new(5, 1).await;
        acknowledge(&mut returning).await;
        deliver(&mut hub, &mut returning).await;
        let result = stability(&mut hub, &devices).await;
        assert_eq!(result.devices[2].standing, DeviceStanding::Counted);
        assert_eq!(result.devices[2].last_stored_ms, Some(hub.now_ms));
        assert_eq!(result.stable, template_vector(&person()));

        // Devices whose newest operation is older than the limit are dormant too.
        hub.now_ms += DORMANCY_MS + 1;
        let result = stability(&mut hub, &devices).await;
        assert!(
            result
                .devices
                .iter()
                .all(|device| device.standing == DeviceStanding::Dormant)
        );
        assert_eq!(result.stable, hub.applied_vector().await);
    });
}

#[test]
fn a_device_without_a_link_time_needs_a_new_link_and_never_holds_stability() {
    block_on(async {
        let (mut hub, a, _b) = linked().await;
        let since = hub.now_ms;
        let unlinked = CountedDevice {
            device_id: super::replica::device_id(5),
            linked_at_ms: None,
        };
        let result = stability(
            &mut hub,
            &[counted(1, since), counted(2, since), unlinked.clone()],
        )
        .await;
        assert_eq!(result.devices[2].standing, DeviceStanding::RelinkRequired);
        assert!(
            result.stable.covers(&a.writer(), 2),
            "a device holding nothing does not hold stability back"
        );

        // A device that syncs is still not counted while its link time is unknown, and time
        // never makes it dormant.
        let mut returning = Replica::new(5, 1).await;
        acknowledge(&mut returning).await;
        deliver(&mut hub, &mut returning).await;
        hub.now_ms += DORMANCY_MS + 1;
        let result = stability(&mut hub, std::slice::from_ref(&unlinked)).await;
        assert_eq!(result.devices[0].standing, DeviceStanding::RelinkRequired);
        assert_eq!(result.stable, hub.applied_vector().await);
    });
}

#[test]
fn stability_never_counts_inactive_writers_or_unknown_contexts() {
    block_on(async {
        for state in ["retired", "revoked", "forked"] {
            let (mut hub, a, _b) = linked().await;
            let since = hub.now_ms;
            hub.exec(&format!(
                "UPDATE sync_writers SET state = '{state}', cutoff_seq = 2
                 WHERE writer_id = x'{}'",
                hex(a.writer().as_bytes())
            ))
            .await;
            let result = stability(&mut hub, &[counted(1, since)]).await;
            assert!(result.devices[0].acknowledged.is_empty(), "{state}");
            assert!(result.stable.is_empty(), "{state}");
        }

        let (mut hub, a, _b) = linked().await;
        let since = hub.now_ms;
        hub.exec(&format!(
            "UPDATE sync_ops SET context = x'' WHERE writer_id = x'{}' AND seq = 2",
            hex(a.writer().as_bytes())
        ))
        .await;
        let result = stability(&mut hub, &[counted(1, since)]).await;
        let genesis: VersionVector = [(a.writer(), 1)].into_iter().collect();
        assert_eq!(result.devices[0].acknowledged, genesis);
        assert_eq!(result.stable, genesis);
    });
}
