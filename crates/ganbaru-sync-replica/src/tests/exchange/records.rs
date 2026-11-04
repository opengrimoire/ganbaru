//! Record exchange ahead of the operation lanes: a removed device still takes the records that
//! removed it, and the storage backstop stops the operation lanes while records keep moving.

use super::{DirectTransport, HUB_DEVICE, PHONE_DEVICE, Replica, block_on, done, hub, phone_ids};
use crate::client::{ClientSession, Exchange, MAX_EXCHANGE_BYTES, PeerIds};
use crate::records::{BackstopState, RecordExchange, RecordLink};
use crate::tests::acquire;
use ganbaru_sync::{Engine, SyncError};
use std::collections::BTreeSet;
use std::sync::Arc;

const MIB: u64 = 1024 * 1024;

impl Replica {
    /// Exchanges with record exchange ahead of the operation lanes, naming the hub as the peer.
    async fn exchange_records(
        &self,
        transport: &DirectTransport,
        ids: &PeerIds,
        records: &RecordExchange,
    ) -> Result<Exchange, String> {
        let engine = Engine::vault();
        let own_writers = self.writer.own_writers();
        ClientSession {
            transport,
            ids,
            engine: &engine,
            pool: &self.pool,
            ctx: &self.ctx,
            writer: Some(self.writer.id()),
            own_writers: &own_writers,
            quarantined: &BTreeSet::new(),
            pull_skips: None,
            max_bytes: MAX_EXCHANGE_BYTES,
            listening: &[],
            records: Some(RecordLink {
                state: records,
                peer: HUB_DEVICE,
            }),
        }
        .exchange()
        .await
    }

    /// The revision that removed `device_id` in this replica's selected chain.
    async fn removed_revision(&self, device_id: &str) -> Option<u64> {
        let mut conn = acquire(&self.pool).await;
        Engine::vault()
            .authorization(&mut conn, &self.ctx)
            .await
            .expect("authorization")
            .expect("charter")
            .devices
            .into_iter()
            .find(|device| device.device_id == device_id)
            .and_then(|device| device.removed_revision)
    }

    /// Stored record envelope bytes.
    async fn record_bytes(&self) -> u64 {
        let mut conn = acquire(&self.pool).await;
        Engine::vault()
            .authorization_storage(&mut conn, &self.ctx)
            .await
            .expect("authorization storage")
            .bytes
    }
}

#[test]
fn a_removed_device_still_takes_the_records_that_removed_it() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-records-removed", HUB_DEVICE).await;
        let phone = Replica::new("phone-records-removed", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let records = RecordExchange::default();
        let exchanged = done(
            phone
                .exchange_records(&transport, &phone_ids(), &records)
                .await
                .expect("exchange"),
        );
        assert!(exchanged.records.is_some());
        assert_eq!(phone.removed_revision(PHONE_DEVICE).await, None);

        let revision = hub_replica
            .remove(PHONE_DEVICE)
            .await
            .expect("remove the phone");
        // The operation lanes refuse the removed phone, and the records still reach it first.
        let error = phone
            .exchange_records(&transport, &phone_ids(), &records)
            .await
            .expect_err("the operation lanes refuse a removed device");
        assert!(error.contains("member_removed"), "{error}");
        assert_eq!(phone.removed_revision(PHONE_DEVICE).await, Some(revision));
        assert_eq!(phone.record_bytes().await, hub_replica.record_bytes().await);
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn the_storage_backstop_stops_the_operation_lanes_and_keeps_records_moving() {
    block_on(async {
        let hub_replica = Replica::new("hub-records-backstop", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-records-backstop", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        phone.seal_note("note-behind-the-backstop").await;
        let records = RecordExchange::default().with_free_space(Arc::new(|| Some(256 * MIB)));
        let exchanged = done(
            phone
                .exchange_records(&transport, &phone_ids(), &records)
                .await
                .expect("exchange"),
        );
        assert_eq!(
            exchanged
                .records
                .as_ref()
                .and_then(|records| records.backstop),
            Some(BackstopState::Reserve)
        );
        assert_eq!(
            exchanged.problem.as_deref(),
            Some(SyncError::AuthorizationStorageFull.to_string().as_str())
        );
        assert_eq!(exchanged.pushed, 0);
        assert_eq!(hub_replica.stored().await.get(&phone.writer.id()), 0);

        // Under the hard stop nothing moves at all.
        let stopped = RecordExchange::default().with_free_space(Arc::new(|| Some(64 * MIB)));
        let exchanged = done(
            phone
                .exchange_records(&transport, &phone_ids(), &stopped)
                .await
                .expect("exchange"),
        );
        assert_eq!(
            exchanged
                .records
                .as_ref()
                .and_then(|records| records.backstop),
            Some(BackstopState::Stopped)
        );

        // Once the user frees space and resumes, full sync runs again.
        let roomy = RecordExchange::default().with_free_space(Arc::new(|| Some(4096 * MIB)));
        let exchanged = done(
            phone
                .exchange_records(&transport, &phone_ids(), &roomy)
                .await
                .expect("exchange"),
        );
        assert_eq!(exchanged.problem, None);
        assert!(hub_replica.stored().await.get(&phone.writer.id()) > 0);
        hub_replica.close().await;
        phone.close().await;
    });
}
