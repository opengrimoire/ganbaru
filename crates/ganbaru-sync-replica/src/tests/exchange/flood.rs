//! Record floods between replicas, under limits scaled down through configuration: a removed
//! anchor that keeps sequencing is pulled within the low budget, kit records lead what a peer
//! serves, and the storage backstop stops sync while its kit reserve still takes a kit forced
//! handover and the ancestors it wants. A record purged with a bad copy is fetched again by
//! hash from an honest peer.

use super::handover::{LAPTOP_DEVICE, enroll_mobile, laptop_ids, sequence_here};
use super::{
    DirectTransport, HUB_DEVICE, PHONE_DEVICE, Replica, block_on, contact_source, device_key, done,
    hub, hub_keys, open_hub, phone_ids, test_contact,
};
use crate::authority::{self, Enrollment, ForcedHandover};
use crate::client::{ClientSession, Exchanged, MAX_EXCHANGE_BYTES, PeerIds};
use crate::hub::SyncHub;
use crate::records::{
    self, BackstopState, RecordBudget, RecordClock, RecordExchange, RecordLimits, RecordSession,
    RecordsExchanged,
};
use crate::tests::{NOW_MS, acquire};
use ganbaru_sync::{Engine, SyncError};
use ganbaru_sync_contracts::{AdminKeyPair, DeviceKind, Envelope, WriterId};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Enrollments the removed anchor sequences after the kit took the anchor from it.
const FLOOD: usize = 6;
/// Sessions a test runs at most before it gives up on a flood moving.
const MAX_SESSIONS: usize = 256;

/// A clock the admission windows read, moved by hand.
#[derive(Clone)]
struct ManualClock(Arc<Mutex<Instant>>);

impl ManualClock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }

    fn clock(&self) -> RecordClock {
        let now = Arc::clone(&self.0);
        Arc::new(move || *now.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) += by;
    }
}

/// A hub serving `replica` that shares the record exchange state `records`.
fn record_hub(replica: &Replica, records: RecordExchange) -> DirectTransport {
    DirectTransport(
        SyncHub::new(open_hub(replica), watch::Sender::new(0))
            .with_contact(contact_source(test_contact()))
            .with_records(records),
    )
}

impl Replica {
    /// One record exchange alone with the device `peer`, which `transport` serves.
    async fn records_with(
        &self,
        transport: &DirectTransport,
        ids: &PeerIds,
        peer: &str,
        state: &RecordExchange,
    ) -> RecordsExchanged {
        let engine = Engine::vault();
        let peer_class = {
            let mut conn = acquire(&self.pool).await;
            records::peer_class(&engine, &mut conn, &self.ctx, peer)
                .await
                .expect("peer class")
        };
        RecordSession {
            transport,
            ids,
            engine: &engine,
            pool: &self.pool,
            ctx: &self.ctx,
            state,
            peer,
            peer_class,
        }
        .exchange()
        .await
        .expect("record exchange")
    }

    /// A full sync session with the device `peer`, record exchange first.
    async fn sync_with(
        &self,
        transport: &DirectTransport,
        ids: &PeerIds,
        peer: &str,
        state: &RecordExchange,
    ) -> Exchanged {
        let engine = Engine::vault();
        let own_writers = self.writer.own_writers();
        done(
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
                records: Some(records::RecordLink { state, peer }),
            }
            .exchange()
            .await
            .expect("sync session"),
        )
    }

    /// Stored record envelopes of `writer`, its certificates included.
    async fn records_of(&self, writer: WriterId) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM sync_auth_envelopes WHERE writer_id = ?")
            .bind(writer.as_bytes().as_slice())
            .fetch_one(&self.pool)
            .await
            .expect("count record envelopes")
    }

    /// Whether the selected log removed `device_id`.
    async fn removed(&self, device_id: &str) -> bool {
        let mut conn = acquire(&self.pool).await;
        Engine::vault()
            .authorization(&mut conn, &self.ctx)
            .await
            .expect("authorization")
            .expect("charter")
            .devices
            .iter()
            .any(|device| device.device_id == device_id && device.removed_revision.is_some())
    }
}

/// The hub enrolls the laptop, the laptop takes the log and forces the anchor to itself under
/// the kit, removing the hub, and the hub, which has not heard of it, keeps sequencing
/// [`FLOOD`] enrollments.
async fn forced_away(label: &str) -> (Replica, Replica) {
    let mut hub_replica = Replica::new(&format!("hub-{label}"), HUB_DEVICE).await;
    let mut laptop = Replica::new(&format!("laptop-{label}"), LAPTOP_DEVICE).await;
    let charter = hub_replica.charter().await;
    let space = hub_replica.ctx.space;
    let enroll = authority::propose(
        space,
        charter,
        &hub_keys().admin,
        authority::enroll_entry(Enrollment {
            device_id: LAPTOP_DEVICE.to_string(),
            device_key: device_key(LAPTOP_DEVICE).public_key(),
            tls_fingerprint: [33; 32],
            label: "Laptop".to_string(),
            device_kind: DeviceKind::Desktop,
        }),
        NOW_MS,
    )
    .expect("sign enrollment");
    sequence_here(&mut hub_replica, &enroll).await;
    let (transport, _changes) = hub(&hub_replica);
    laptop
        .exchange(&transport, &laptop_ids())
        .await
        .expect("pull the log");
    laptop.apply().await;

    let mut conn = acquire(&laptop.pool).await;
    let view = laptop
        .engine
        .authorization(&mut conn, &laptop.ctx)
        .await
        .expect("read authorization")
        .expect("charter");
    let recovery = authority::recovery_admin(&view, &hub_keys().recovery.kit_code()).expect("kit");
    let laptop_admin = AdminKeyPair::from_seed(&[32; 32]).expect("admin seed");
    let forced = ForcedHandover {
        signer: &recovery,
        promote: Some(laptop_admin.public_key()),
        point: None,
        removals: vec![HUB_DEVICE.to_string()],
    };
    authority::force_handover_here(
        &laptop.engine,
        &mut conn,
        &laptop.ctx,
        &forced,
        &mut laptop.writer.local(),
        NOW_MS,
    )
    .await
    .expect("force handover")
    .expect("the kit hands the anchor to the laptop");
    drop(conn);
    let seq = laptop.stored().await.get(&laptop.writer.id());
    laptop.writer.committed(seq).expect("record commit");
    assert!(laptop.removed(HUB_DEVICE).await);

    for index in 0..FLOOD {
        let seed = 60 + u8::try_from(index).expect("small flood");
        let device_id = format!("device-flood-{index}");
        sequence_here(
            &mut hub_replica,
            &enroll_mobile(space, charter, &device_id, seed),
        )
        .await;
    }
    (hub_replica, laptop)
}

/// Limits whose low budget lets one envelope that is not a kit envelope through per window.
fn one_low_envelope() -> RecordLimits {
    let one = RecordBudget {
        bytes: 1,
        envelopes: 1,
    };
    RecordLimits {
        low: one,
        low_min: one,
        ..RecordLimits::default()
    }
}

/// R7.6 test 31: the anchor the kit removed keeps sequencing. The new anchor pulls its records
/// within the low budget only, one window at a time, while a member that still sees the old
/// anchor live takes the same flood in one session under the same limits.
#[test]
fn a_removed_anchors_flood_moves_within_the_low_budget_one_window_at_a_time() {
    block_on(async {
        let (hub_replica, laptop) = forced_away("flood-low").await;
        let hub_writer = hub_replica.writer.id();
        let flooded = hub_replica.records_of(hub_writer).await;
        let before = laptop.records_of(hub_writer).await;
        assert_eq!(flooded - before, i64::try_from(FLOOD).expect("small flood"));
        let transport = record_hub(&hub_replica, RecordExchange::default());
        let limits = one_low_envelope();

        // A member that has not heard of the forced handover classes the old anchor normal.
        let phone = Replica::new("phone-flood-low", PHONE_DEVICE).await;
        let phone_state = RecordExchange::default().with_limits(limits);
        let phone_exchanged = phone
            .records_with(&transport, &phone_ids(), HUB_DEVICE, &phone_state)
            .await;
        assert!(!phone_exchanged.more);
        assert_eq!(phone.records_of(hub_writer).await, flooded);

        let clock = ManualClock::new();
        let state = RecordExchange::default()
            .with_limits(limits)
            .with_clock(clock.clock());
        let first = laptop
            .records_with(&transport, &laptop_ids(), HUB_DEVICE, &state)
            .await;
        assert!(first.more);
        let after_first = laptop.records_of(hub_writer).await;
        assert!(after_first - before <= 1, "{after_first} after {before}");
        // The window is spent: a second session in it moves nothing from the removed anchor.
        let again = laptop
            .records_with(&transport, &laptop_ids(), HUB_DEVICE, &state)
            .await;
        assert!(again.more);
        assert_eq!(again.stored, 0);
        assert_eq!(laptop.records_of(hub_writer).await, after_first);

        let mut windows = 1;
        loop {
            assert!(windows < MAX_SESSIONS, "the flood never finished moving");
            clock.advance(limits.window);
            windows += 1;
            let exchanged = laptop
                .records_with(&transport, &laptop_ids(), HUB_DEVICE, &state)
                .await;
            let held = laptop.records_of(hub_writer).await;
            assert!(held <= before + i64::try_from(windows).expect("few windows"));
            if !exchanged.more {
                break;
            }
        }
        assert_eq!(laptop.records_of(hub_writer).await, flooded);
        assert!(windows > FLOOD, "{windows} windows");
        // The flood never selects past the kit's removal.
        assert!(laptop.removed(HUB_DEVICE).await);
        hub_replica.close().await;
        laptop.close().await;
        phone.close().await;
    });
}

/// R7.6 test 35: the new anchor holds the removed anchor's flood and serves one envelope per
/// window. A member pulling from it gets the kit forced handover and its ancestors before any
/// envelope of the flood, so the removal is selected first.
#[test]
fn kit_records_lead_what_a_peer_serves_ahead_of_a_flood() {
    block_on(async {
        let (hub_replica, laptop) = forced_away("flood-order").await;
        let hub_writer = hub_replica.writer.id();
        let flooded = hub_replica.records_of(hub_writer).await;
        let (transport, _changes) = hub(&hub_replica);
        let taken = laptop
            .records_with(
                &transport,
                &laptop_ids(),
                HUB_DEVICE,
                &RecordExchange::default(),
            )
            .await;
        assert!(!taken.more);
        assert_eq!(laptop.records_of(hub_writer).await, flooded);

        let clock = ManualClock::new();
        let limits = RecordLimits {
            normal: RecordBudget {
                bytes: RecordLimits::default().normal.bytes,
                envelopes: 1,
            },
            ..RecordLimits::default()
        };
        let serving = RecordExchange::default()
            .with_limits(limits)
            .with_clock(clock.clock());
        let transport = record_hub(&laptop, serving);
        let phone = Replica::new("phone-flood-order", PHONE_DEVICE).await;
        let state = RecordExchange::default();
        // The enrollment of the laptop and the certificate of the old anchor's writer are
        // ancestors of the kit records; every later envelope of that writer is the flood.
        let ancestors = flooded - i64::try_from(FLOOD).expect("small flood");
        let mut removed_at = None;
        let mut flood_at = None;
        for session in 0..MAX_SESSIONS {
            let exchanged = phone
                .records_with(&transport, &phone_ids(), LAPTOP_DEVICE, &state)
                .await;
            if removed_at.is_none() && phone.removed(HUB_DEVICE).await {
                removed_at = Some(session);
            }
            if flood_at.is_none() && phone.records_of(hub_writer).await > ancestors {
                flood_at = Some(session);
            }
            if !exchanged.more && phone.records_of(hub_writer).await == flooded {
                break;
            }
            clock.advance(limits.window);
        }
        let (removed_at, flood_at) = (
            removed_at.expect("the removal reaches the member"),
            flood_at.expect("the flood reaches the member"),
        );
        assert!(removed_at < flood_at, "{removed_at} then {flood_at}");
        hub_replica.close().await;
        laptop.close().await;
        phone.close().await;
    });
}

/// Limits under which the backstop runs from the first exchange, with a reserve of `reserve`
/// bytes kept whole for kit records and their ancestors.
fn kit_reserve_only(reserve: u64) -> RecordLimits {
    RecordLimits {
        backstop_bytes: 1,
        reserve,
        kit_reserve: reserve,
        ..RecordLimits::default()
    }
}

/// R7.6 tests 45 and 50: a member whose record store is past the backstop syncs with the new
/// anchor. The operation lanes stop with `AuthorizationStorageFull`, the kit reserve still takes
/// the kit forced handover with the enrollment and certificate it wants, so the removal
/// selects, and nothing of the flood is stored. A reserve too small for one kit record stops
/// record exchange fully. Once the bounds are raised, full sync runs again.
#[test]
fn the_backstop_stops_sync_and_its_kit_reserve_still_takes_a_kit_removal() {
    block_on(async {
        let (hub_replica, laptop) = forced_away("flood-backstop").await;
        let hub_writer = hub_replica.writer.id();
        let laptop_writer = laptop.writer.id();
        let flooded = hub_replica.records_of(hub_writer).await;
        let (transport, _changes) = hub(&hub_replica);
        laptop
            .records_with(
                &transport,
                &laptop_ids(),
                HUB_DEVICE,
                &RecordExchange::default(),
            )
            .await;
        let transport = record_hub(&laptop, RecordExchange::default());
        let ancestors = flooded - i64::try_from(FLOOD).expect("small flood");

        // A reserve too small for any kit record stops record exchange fully.
        let stopped_phone = Replica::new("phone-flood-stopped", PHONE_DEVICE).await;
        let stopped = RecordExchange::default().with_limits(kit_reserve_only(1));
        let exchanged = stopped_phone
            .sync_with(&transport, &phone_ids(), LAPTOP_DEVICE, &stopped)
            .await;
        assert_eq!(
            exchanged.problem.as_deref(),
            Some(SyncError::AuthorizationStorageFull.to_string().as_str())
        );
        assert_eq!(stopped.backstop_state(), Some(BackstopState::Stopped));
        assert!(!stopped_phone.removed(HUB_DEVICE).await);
        assert_eq!(stopped_phone.records_of(laptop_writer).await, 0);
        let again = stopped_phone
            .sync_with(&transport, &phone_ids(), LAPTOP_DEVICE, &stopped)
            .await;
        assert_eq!(
            again.records.as_ref().and_then(|records| records.backstop),
            Some(BackstopState::Stopped)
        );
        assert_eq!(stopped_phone.records_of(laptop_writer).await, 0);

        // A kit reserve that fits the kit records takes them and their ancestors only.
        let phone = Replica::new("phone-flood-backstop", PHONE_DEVICE).await;
        let reserve = RecordExchange::default().with_limits(kit_reserve_only(1024 * 1024));
        for _ in 0..2 {
            let exchanged = phone
                .sync_with(&transport, &phone_ids(), LAPTOP_DEVICE, &reserve)
                .await;
            assert_eq!(
                exchanged.problem.as_deref(),
                Some(SyncError::AuthorizationStorageFull.to_string().as_str())
            );
            assert_eq!(exchanged.pushed, 0);
            assert_eq!(reserve.backstop_state(), Some(BackstopState::Reserve));
            assert!(phone.removed(HUB_DEVICE).await);
            assert_eq!(phone.records_of(hub_writer).await, ancestors);
            assert!(phone.records_of(laptop_writer).await > 0);
        }

        // The user raises the bounds: full sync runs again and the flood arrives.
        let raised = RecordExchange::default();
        let exchanged = phone
            .sync_with(&transport, &phone_ids(), LAPTOP_DEVICE, &raised)
            .await;
        assert_eq!(exchanged.problem, None);
        assert_eq!(raised.backstop_state(), None);
        assert_eq!(phone.records_of(hub_writer).await, flooded);
        assert!(phone.removed(HUB_DEVICE).await);
        hub_replica.close().await;
        laptop.close().await;
        phone.close().await;
        stopped_phone.close().await;
    });
}

impl Replica {
    /// Admits `envelopes` into the record store directly, as a relay that holds them would.
    async fn hold_records(&self, envelopes: &[Vec<u8>]) {
        let mut conn = acquire(&self.pool).await;
        Engine::vault()
            .admit_records(&mut conn, &self.ctx, envelopes, NOW_MS)
            .await
            .expect("admit records");
    }

    /// Whether the record store holds a verified envelope with `hash`.
    async fn holds_verified(&self, hash: &[u8]) -> bool {
        let verified: Option<i64> = sqlx::query_scalar(
            "SELECT verified FROM sync_auth_envelopes WHERE envelope_hash = ? LIMIT 1",
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await
        .expect("read stored envelope");
        verified == Some(1)
    }

    /// Whether the record store holds any envelope with `hash`.
    async fn holds(&self, hash: &[u8]) -> bool {
        let stored: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM sync_auth_envelopes WHERE envelope_hash = ? LIMIT 1")
                .bind(hash)
                .fetch_optional(&self.pool)
                .await
                .expect("read stored envelope");
        stored.is_some()
    }

    /// The envelope hashes held for refetch.
    async fn refetching(&self) -> Vec<Vec<u8>> {
        sqlx::query_scalar("SELECT envelope_hash FROM sync_auth_refetch ORDER BY envelope_hash")
            .fetch_all(&self.pool)
            .await
            .expect("read refetch hashes")
    }

    /// Whether the record exchange would ask peers for `hash`.
    async fn wants_hash(&self, hash: &[u8]) -> bool {
        let mut conn = acquire(&self.pool).await;
        Engine::vault()
            .record_wants(&mut conn, &self.ctx, usize::MAX)
            .await
            .expect("record wants")
            .hashes
            .iter()
            .any(|wanted| wanted.as_slice() == hash)
    }
}

/// The refetch amendment to R7.7 between replicas. Relays hold a copy of the laptop's newest
/// kit record with a bad signature, the honest copy with its ancestry, and the laptop's
/// certificate, one each. The phone takes the bad copy first, so the honest copy its pages bring
/// later is dropped, and the certificate then purges the bad copy. The purged hash stays
/// requested: the relay with the bad copy answers it again and is refused, and the relay with
/// the honest copy, whose page cursor is already past it, answers the request by hash, so the
/// honest record is stored without its pages delivering it again.
#[test]
fn a_purged_record_is_fetched_by_hash_from_an_honest_peer_after_a_bad_copy() {
    block_on(async {
        let (hub_replica, laptop) = forced_away("refetch").await;
        let laptop_writer = laptop.writer.id();
        let rows: Vec<(String, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT role, writer_id, envelope FROM sync_auth_envelopes ORDER BY stored_order",
        )
        .fetch_all(&laptop.pool)
        .await
        .expect("read the laptop's records");
        let own = |writer: &[u8]| writer == laptop_writer.as_bytes().as_slice();
        let certificate = rows
            .iter()
            .find(|(role, writer, _)| role == "certificate" && own(writer))
            .map(|(_, _, envelope)| envelope.clone())
            .expect("the laptop's certificate");
        let honest = rows
            .iter()
            .filter(|(role, writer, _)| role == "record" && own(writer))
            .max_by_key(|(_, _, envelope)| Envelope::decode(envelope).expect("decode").seq)
            .map(|(_, _, envelope)| envelope.clone())
            .expect("the laptop's kit record");
        let mut bad = honest.clone();
        *bad.last_mut().expect("signature") ^= 1;
        let hash = Envelope::decode(&honest).expect("decode").hash().to_vec();
        let without_certificate: Vec<Vec<u8>> = rows
            .iter()
            .filter(|(_, _, envelope)| *envelope != certificate)
            .map(|(_, _, envelope)| envelope.clone())
            .collect();

        let relay_bad = Replica::new("relay-bad-refetch", "device-relay-bad").await;
        relay_bad.hold_records(std::slice::from_ref(&bad)).await;
        let relay_honest = Replica::new("relay-honest-refetch", "device-relay-honest").await;
        relay_honest.hold_records(&without_certificate).await;
        let relay_certificate =
            Replica::new("relay-certificate-refetch", "device-relay-certificate").await;
        relay_certificate
            .hold_records(std::slice::from_ref(&certificate))
            .await;
        assert!(relay_bad.holds(&hash).await && relay_honest.holds(&hash).await);
        let bad_transport = record_hub(&relay_bad, RecordExchange::default());
        let honest_transport = record_hub(&relay_honest, RecordExchange::default());
        let certificate_transport = record_hub(&relay_certificate, RecordExchange::default());

        let phone = Replica::new("phone-refetch", PHONE_DEVICE).await;
        let limits = RecordLimits {
            low: RecordLimits::default().normal,
            ..RecordLimits::default()
        };
        let state = RecordExchange::default().with_limits(limits);
        phone
            .records_with(&bad_transport, &phone_ids(), "relay-bad", &state)
            .await;
        assert!(phone.holds(&hash).await && !phone.holds_verified(&hash).await);
        phone
            .records_with(&honest_transport, &phone_ids(), "relay-honest", &state)
            .await;
        assert!(
            !phone.holds_verified(&hash).await,
            "the honest copy is dropped"
        );
        phone
            .records_with(
                &certificate_transport,
                &phone_ids(),
                "relay-certificate",
                &state,
            )
            .await;
        assert!(phone.records_of(laptop_writer).await > 0);
        assert!(
            !phone.holds(&hash).await,
            "the certificate purges the bad copy"
        );
        assert_eq!(phone.refetching().await, vec![hash.clone()]);
        assert!(phone.wants_hash(&hash).await);

        // The relay with the bad copy is asked first and answers with it again: refused at
        // admission now that the certificate is stored, and the hash stays requested.
        state.connected("relay-bad");
        phone
            .records_with(&bad_transport, &phone_ids(), "relay-bad", &state)
            .await;
        assert!(!phone.holds(&hash).await);
        assert_eq!(phone.refetching().await, vec![hash.clone()]);
        assert!(phone.wants_hash(&hash).await);

        // The honest relay's cursor is past its copy, so only the request by hash brings it,
        // once the backoff the refused answer set is due.
        phone
            .records_with(&honest_transport, &phone_ids(), "relay-honest", &state)
            .await;
        assert!(
            !phone.holds(&hash).await,
            "pages past the cursor do not deliver the honest copy again"
        );
        for _ in 0..MAX_SESSIONS {
            if phone.holds(&hash).await {
                break;
            }
            phone
                .records_with(&honest_transport, &phone_ids(), "relay-honest", &state)
                .await;
        }
        assert!(phone.holds_verified(&hash).await);
        assert!(phone.refetching().await.is_empty());
        assert!(!phone.wants_hash(&hash).await);
        assert!(phone.removed(HUB_DEVICE).await);

        hub_replica.close().await;
        laptop.close().await;
        relay_bad.close().await;
        relay_honest.close().await;
        relay_certificate.close().await;
        phone.close().await;
    });
}
