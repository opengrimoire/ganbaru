//! Record floods between replicas, under limits scaled down through configuration: a removed
//! anchor that keeps sequencing is pulled within the low budget, kit records lead what a peer
//! serves, and the storage backstop stops sync while its kit reserve still takes a kit forced
//! handover and the ancestors it wants. A record purged with a bad copy is fetched again by
//! hash from an honest peer. Kit records a removed peer seals again under its own writers never
//! take kit class, at its own historical parents either. Planted wants and a busy peer do not
//! starve an honest want, and a held peer admits a data fork pair from one page or from two,
//! from a run of first halves longer than a hold, and when a push follows a pulled page. The
//! self-takes of an admin the stolen anchor enrolled spend one kit slot per kit proposal, and a
//! former anchor's rewrap at an older parent does not take the slot from the head's record.

use super::handover::{LAPTOP_DEVICE, enroll_mobile, laptop_ids, sequence_here};
use super::{
    DirectTransport, HUB_DEVICE, PHONE_DEVICE, Replica, block_on, contact_source, device_key, done,
    hub, hub_keys, hub_recovery_seed, open_hub, phone_ids, test_contact,
};
use crate::authority::{self, Enrollment, ForcedHandover};
use crate::client::{ClientSession, Exchanged, MAX_EXCHANGE_BYTES, PeerIds, SyncTransport};
use crate::hub::SyncHub;
use crate::records::{
    self, BackstopState, RecordBudget, RecordClock, RecordExchange, RecordLimits, RecordSession,
    RecordsExchanged,
};
use crate::tests::{NOW_MS, VAULT_ID, acquire};
use crate::wire::ops_to_wire;
use ganbaru_handoff::protocol::{
    ControlMessage, MAX_SYNC_RECORDS, PROTOCOL_VERSION, SYNC_PAGE_BYTES,
};
use ganbaru_sync::{Engine, RecordClass, SyncError};
use ganbaru_sync_contracts::bounds::{MAX_BLOB_FIELD_BYTES, MAX_OPERATION_BYTES};
use ganbaru_sync_contracts::{
    AdminKeyPair, AuthorizationRecord, Change, ChangeAction, Content, DeviceKeyPair, DeviceKind,
    Digest32, Entry, Envelope, Field, GroupId, GroupValue, Operation, RowKey, Sequencing, SetRole,
    TableId, Value, WriterId, WriterKeyPair, Yield, sign_certificate,
};
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
    async fn records_with<T: SyncTransport>(
        &self,
        transport: &T,
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

    /// Bytes of the largest stored envelope of `writer`.
    async fn largest_record_of(&self, writer: WriterId) -> usize {
        let bytes: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(length(envelope)), 0) FROM sync_auth_envelopes WHERE writer_id = ?",
        )
        .bind(writer.as_bytes().as_slice())
        .fetch_one(&self.pool)
        .await
        .expect("measure record envelopes");
        usize::try_from(bytes).expect("an envelope size")
    }

    /// Stored envelopes of every writer but `writer`, in store order.
    async fn records_besides(&self, writer: WriterId) -> Vec<Vec<u8>> {
        sqlx::query_scalar(
            "SELECT envelope FROM sync_auth_envelopes WHERE writer_id != ? ORDER BY stored_order",
        )
        .bind(writer.as_bytes().as_slice())
        .fetch_all(&self.pool)
        .await
        .expect("read record envelopes")
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
    forced_away_removing(label, &[]).await
}

/// [`forced_away`], with the hub enrolling the devices `others` before the laptop and the kit
/// removing them with the hub.
async fn forced_away_removing(label: &str, others: &[&str]) -> (Replica, Replica) {
    let mut hub_replica = Replica::new(&format!("hub-{label}"), HUB_DEVICE).await;
    let mut laptop = Replica::new(&format!("laptop-{label}"), LAPTOP_DEVICE).await;
    let charter = hub_replica.charter().await;
    let space = hub_replica.ctx.space;
    for (index, device_id) in others.iter().enumerate() {
        let seed = 90 + u8::try_from(index).expect("few devices");
        sequence_here(
            &mut hub_replica,
            &enroll_mobile(space, charter, device_id, seed),
        )
        .await;
    }
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
    let recovery = authority::recovery_admin(&view, &hub_recovery_seed().kit_code()).expect("kit");
    let laptop_admin = AdminKeyPair::from_seed(&[32; 32]).expect("admin seed");
    let forced = ForcedHandover {
        signer: &recovery,
        promote: Some(laptop_admin.public_key()),
        point: None,
        removals: std::iter::once(HUB_DEVICE)
            .chain(others.iter().copied())
            .map(str::to_string)
            .collect(),
        recovery: None,
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

/// Limits whose low budget lets one envelope of at most `bytes` that is not a kit envelope
/// through per window. Overspend carries into later windows, so `bytes` is the largest
/// envelope expected for the budget to renew each window.
fn one_low_envelope(bytes: usize) -> RecordLimits {
    let one = RecordBudget {
        bytes,
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
        let limits = one_low_envelope(hub_replica.largest_record_of(hub_writer).await);

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
/// anchor. The member already holds the log up to the handover, so the handover binds to the
/// laptop the log enrolled. The operation lanes stop with `AuthorizationStorageFull`, the kit
/// reserve still takes the kit forced handover with the certificate it wants, so the removal
/// selects, and nothing of the flood is stored. A reserve too small for that bound kit record
/// stops record exchange fully. Once the bounds are raised, full sync runs again.
#[test]
fn the_backstop_stops_sync_and_its_kit_reserve_still_takes_a_kit_removal() {
    block_on(async {
        let (hub_replica, laptop) = forced_away("flood-backstop").await;
        let hub_writer = hub_replica.writer.id();
        let laptop_writer = laptop.writer.id();
        let flooded = hub_replica.records_of(hub_writer).await;
        let log = laptop.records_besides(laptop_writer).await;
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

        // A reserve too small for the bound kit record stops record exchange fully.
        let stopped_phone = Replica::new("phone-flood-stopped", PHONE_DEVICE).await;
        stopped_phone.hold_records(&log).await;
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
        phone.hold_records(&log).await;
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
            .record_wants(&mut conn, &self.ctx, usize::MAX, &BTreeSet::new())
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

        // The honest relay's cursor is past its copy, so only the request by hash brings it. The
        // refused answer backed the hash off with the relay that refused it only, so the next
        // session with the honest relay asks for it.
        assert!(state.backed_off("relay-bad").contains(&hash));
        assert!(!state.backed_off("relay-honest").contains(&hash));
        phone
            .records_with(&honest_transport, &phone_ids(), "relay-honest", &state)
            .await;
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

impl Replica {
    /// How many hashes the record exchange would ask peers for.
    async fn wanted_hashes(&self) -> usize {
        let mut conn = acquire(&self.pool).await;
        Engine::vault()
            .record_wants(&mut conn, &self.ctx, usize::MAX, &BTreeSet::new())
            .await
            .expect("record wants")
            .hashes
            .len()
    }

    /// The certificate of `writer` and its records in sequence order.
    async fn own_records(&self, writer: WriterId) -> (Vec<u8>, Vec<Vec<u8>>) {
        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT role, envelope FROM sync_auth_envelopes WHERE writer_id = ? ORDER BY seq",
        )
        .bind(writer.as_bytes().as_slice())
        .fetch_all(&self.pool)
        .await
        .expect("read own records");
        let certificate = rows
            .iter()
            .find(|(role, _)| role == "certificate")
            .map(|(_, envelope)| envelope.clone())
            .expect("a certificate");
        let records = rows
            .into_iter()
            .filter(|(role, _)| role == "record")
            .map(|(_, envelope)| envelope)
            .collect();
        (certificate, records)
    }
}

/// How a rewrap differs from the kit records it copies.
struct Rewrap {
    /// Seed of the throwaway writer key.
    writer_seed: u8,
    /// The device id its certificate names.
    device_id: &'static str,
    /// The device key that signs its certificate.
    device: DeviceKeyPair,
    /// Changes to the fields the kit does not sign of each record but a handover, whose
    /// unsigned fields its signed entry fixes.
    edit: fn(&mut AuthorizationRecord),
}

/// `records` sealed again under the throwaway writer `rewrap` names, after a genesis re-sealed
/// from `genesis` with a certificate the rewrap's device key signs. Every kit-signed proposal
/// stays as the kit signed it.
fn rewrapped(genesis: &[u8], records: &[Vec<u8>], rewrap: &Rewrap) -> Vec<Vec<u8>> {
    let key = WriterKeyPair::from_seed(&[rewrap.writer_seed; 32]).expect("writer seed");
    let writer = WriterId::for_public_key(&key.public_key());
    let mut operation = Envelope::decode(genesis)
        .expect("decode")
        .operation()
        .expect("operation");
    let Content::Genesis(signed) = &operation.content else {
        panic!("a genesis operation");
    };
    let mut certificate = signed.certificate.clone();
    certificate.writer_key = key.public_key();
    certificate.device_id = rewrap.device_id.to_string();
    certificate.device_key = rewrap.device.public_key();
    operation.writer = writer;
    operation.content =
        Content::Genesis(sign_certificate(&certificate, &rewrap.device).expect("certificate"));
    let mut sealed = vec![operation.seal(&key).expect("seal").into_bytes()];
    for record in records {
        let previous = Envelope::decode(sealed.last().expect("a genesis")).expect("decode");
        let (previous_hash, previous_seq) = (previous.hash(), previous.seq);
        let mut operation = Envelope::decode(record)
            .expect("decode")
            .operation()
            .expect("operation");
        let Content::Authorization(mut copied) = operation.content else {
            panic!("an authorization record");
        };
        if !matches!(copied.proposal.proposal.entry, Entry::Handover(_)) {
            (rewrap.edit)(&mut copied);
        }
        operation.header.authorization_revision = copied.revision - 1;
        operation.writer = writer;
        operation.seq = previous_seq + 1;
        operation.previous_hash = previous_hash;
        operation.content = Content::Authorization(copied);
        sealed.push(operation.seal(&key).expect("seal").into_bytes());
    }
    sealed
}

/// The H1 amendment to R7.6 tests 45 and 50: the kit removed the hub, which serves and pushes the
/// kit records again under throwaway writers its own device key certifies, as itself, as the
/// laptop, with a fresh device key, and with forged anchor ids and revisions. None of them takes
/// kit class, so none draws on the kit reserve and the backstop never stops, before or after the
/// honest kit records are stored. The honest forced handover from the laptop still lands past
/// the backstop bound.
#[test]
fn rewraps_of_kit_records_by_a_removed_peer_never_take_kit_class() {
    block_on(async {
        let (hub_replica, laptop) = forced_away("rewrap").await;
        let laptop_writer = laptop.writer.id();
        let log = laptop.records_besides(laptop_writer).await;
        let (genesis, kit) = laptop.own_records(laptop_writer).await;
        let chains: Vec<Vec<Vec<u8>>> = [
            Rewrap {
                writer_seed: 70,
                device_id: HUB_DEVICE,
                device: device_key(HUB_DEVICE),
                edit: |_| {},
            },
            Rewrap {
                writer_seed: 71,
                device_id: HUB_DEVICE,
                device: device_key(HUB_DEVICE),
                edit: |record| record.anchor_device_id = HUB_DEVICE.to_string(),
            },
            Rewrap {
                writer_seed: 72,
                device_id: LAPTOP_DEVICE,
                device: device_key(HUB_DEVICE),
                edit: |record| record.anchor_device_id = LAPTOP_DEVICE.to_string(),
            },
            Rewrap {
                writer_seed: 73,
                device_id: HUB_DEVICE,
                device: device_key("device-rewrap-fresh"),
                edit: |record| record.anchor_device_id = "device-rewrap-fresh".to_string(),
            },
            Rewrap {
                writer_seed: 74,
                device_id: HUB_DEVICE,
                device: device_key(HUB_DEVICE),
                edit: |record| record.revision += 1,
            },
        ]
        .iter()
        .map(|rewrap| rewrapped(&genesis, &kit, rewrap))
        .collect();
        let rewraps = chains.concat();
        let rewrap_writers: BTreeSet<WriterId> = rewraps
            .iter()
            .map(|envelope| Envelope::decode(envelope).expect("decode").writer)
            .collect();

        let phone = Replica::new("phone-rewrap", PHONE_DEVICE).await;
        phone.hold_records(&log).await;
        {
            let mut conn = acquire(&phone.pool).await;
            let classes = Engine::vault()
                .record_classes(&mut conn, &phone.ctx, &rewraps)
                .await
                .expect("classes");
            assert!(!classes.contains(&RecordClass::Kit), "{classes:?}");
            let honest: Vec<Vec<u8>> = std::iter::once(genesis.clone()).chain(kit).collect();
            let classes = Engine::vault()
                .record_classes(&mut conn, &phone.ctx, &honest)
                .await
                .expect("classes");
            assert!(classes.contains(&RecordClass::Kit), "{classes:?}");
        }

        let relay = Replica::new("relay-rewrap", "device-relay-rewrap").await;
        relay.hold_records(&log).await;
        relay.hold_records(&rewraps).await;
        for writer in &rewrap_writers {
            assert!(
                relay.records_of(*writer).await > 0,
                "the relay serves {writer:?}"
            );
        }
        let rogue = record_hub(&relay, RecordExchange::default());
        let state = RecordExchange::default().with_limits(kit_reserve_only(1024 * 1024));
        // The hub serves the rewraps from its first page, and pushes them chain by chain.
        let pushed = || async {
            state.connected(HUB_DEVICE);
            for _ in 0..3 {
                phone
                    .sync_with(&rogue, &phone_ids(), HUB_DEVICE, &state)
                    .await;
            }
            let engine = Engine::vault();
            for chain in &chains {
                let mut conn = acquire(&phone.pool).await;
                let push = ControlMessage::SyncRecordPush {
                    protocol_version: PROTOCOL_VERSION,
                    vault_id: VAULT_ID.to_string(),
                    device_id: HUB_DEVICE.to_string(),
                    records: ops_to_wire(chain),
                };
                // The backstop keeps nothing of them, whatever the answer.
                let _ = records::respond(&state, &engine, &mut conn, &phone.ctx, HUB_DEVICE, push)
                    .await;
            }
        };
        let rewraps_held = || async {
            let mut held = 0;
            for writer in &rewrap_writers {
                held += phone.records_of(*writer).await;
            }
            held
        };
        pushed().await;
        assert_eq!(state.kit_spent(), 0);
        assert_eq!(state.backstop_state(), Some(BackstopState::Reserve));
        assert_eq!(rewraps_held().await, 0);
        assert!(!phone.removed(HUB_DEVICE).await);

        let honest = record_hub(&laptop, RecordExchange::default());
        phone
            .sync_with(&honest, &phone_ids(), LAPTOP_DEVICE, &state)
            .await;
        assert!(phone.removed(HUB_DEVICE).await);
        let spent = state.kit_spent();
        assert!(spent > 0);
        assert_eq!(state.backstop_state(), Some(BackstopState::Reserve));

        // Once the honest records are stored, a rewrap carries an entry the store holds.
        pushed().await;
        assert_eq!(state.kit_spent(), spent);
        assert_eq!(state.backstop_state(), Some(BackstopState::Reserve));
        assert_eq!(rewraps_held().await, 0);
        hub_replica.close().await;
        laptop.close().await;
        relay.close().await;
        phone.close().await;
    });
}

/// Wanted requests a member with planted wants sends one peer per session.
const PLANTED_ROUNDS: usize = 4;
/// Wanted keys one request of a member with planted wants asks for, half of them at most
/// priority keys.
const PLANTED_PER_REQUEST: usize = 16;
/// Wanted entries a planted chain names that no peer holds: more than one session asks, so the
/// rotation through the wanted set carries over sessions.
const PLANTED: usize = (PLANTED_ROUNDS + 2) * PLANTED_PER_REQUEST;
/// Refetch hashes no peer holds that a member keeps beside the planted wants.
const PLANTED_REFETCH: usize = 2 * PLANTED_PER_REQUEST;
/// Yields a key that is no admin signs at the selected head, more than a request asks for.
const PLANTED_YIELDS: usize = 4 * PLANTED_PER_REQUEST;
/// Bytes a member with planted wants serves one peer per window, so each session pushes it a
/// page of the planted chain at most and spends its time asking.
const PLANTED_SERVE_BYTES: usize = 64 * 1024;
/// Envelopes a test hands the record store at once while it plants records.
const PLANT_BATCH: usize = 8192;

/// A writer key from `seed` and its genesis, re-sealed from the genesis `template` with a
/// certificate naming `device_id` that `device` signs.
fn throwaway_writer(
    template: &[u8],
    seed: u8,
    device_id: &str,
    device: &DeviceKeyPair,
) -> (WriterKeyPair, Vec<u8>) {
    let key = WriterKeyPair::from_seed(&[seed; 32]).expect("writer seed");
    let mut operation = operation_of(template);
    let Content::Genesis(signed) = &operation.content else {
        panic!("a genesis operation");
    };
    let mut certificate = signed.certificate.clone();
    certificate.writer_key = key.public_key();
    certificate.device_id = device_id.to_string();
    certificate.device_key = device.public_key();
    operation.writer = WriterId::for_public_key(&key.public_key());
    operation.content =
        Content::Genesis(sign_certificate(&certificate, device).expect("certificate"));
    let genesis = operation.seal(&key).expect("seal").into_bytes();
    (key, genesis)
}

/// `operation` sealed by `key` right after the envelope `previous` of the same writer. An
/// authorization record is sealed against the revision before its own.
fn seal_next(previous: &[u8], key: &WriterKeyPair, mut operation: Operation) -> Vec<u8> {
    let previous = Envelope::decode(previous).expect("decode");
    operation.writer = previous.writer;
    operation.seq = previous.seq + 1;
    operation.previous_hash = previous.hash();
    if let Content::Authorization(record) = &operation.content {
        operation.header.authorization_revision = record.revision - 1;
    }
    operation.seal(key).expect("seal").into_bytes()
}

/// The operation an envelope carries.
fn operation_of(envelope: &[u8]) -> Operation {
    Envelope::decode(envelope)
        .expect("decode")
        .operation()
        .expect("operation")
}

/// The authorization record an envelope carries, if it carries one.
fn authorization_record(envelope: &[u8]) -> Option<AuthorizationRecord> {
    match operation_of(envelope).content {
        Content::Authorization(record) => Some(*record),
        _ => None,
    }
}

/// The hash of an envelope.
fn envelope_hash(envelope: &[u8]) -> Vec<u8> {
    Envelope::decode(envelope).expect("decode").hash().to_vec()
}

/// A low-sorting 32-byte key: `prefix` in its second byte and `index` in its last four.
fn low_key(prefix: u8, index: usize) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[1] = prefix;
    key[28..].copy_from_slice(&u32::try_from(index).expect("a small index").to_be_bytes());
    key
}

/// A chain of [`PLANTED`] copies of `record` under a throwaway writer the hub's device key
/// certifies, each naming a parent entry no peer holds whose hash sorts below every real one.
fn planted(genesis: &[u8], record: &[u8]) -> Vec<Vec<u8>> {
    let (key, genesis) = throwaway_writer(genesis, 80, HUB_DEVICE, &device_key(HUB_DEVICE));
    let mut sealed = vec![genesis];
    for index in 0..PLANTED {
        let mut operation = operation_of(record);
        let Content::Authorization(copied) = &mut operation.content else {
            panic!("an authorization record");
        };
        copied.previous_entry_hash = low_key(0, index + 1);
        sealed.push(seal_next(
            sealed.last().expect("a genesis"),
            &key,
            operation,
        ));
    }
    sealed
}

/// [`PLANTED_YIELDS`] yields at the record `head` that `signer` signs, under a throwaway
/// writer the hub's device key certifies, each giving way to a different handover.
fn planted_yields(
    genesis: &[u8],
    head: &[u8],
    signer: &AdminKeyPair,
    charter: Digest32,
) -> Vec<Vec<u8>> {
    let parent = authorization_record(head).expect("an authorization record");
    let space = operation_of(head).space;
    let (key, genesis) = throwaway_writer(genesis, 82, HUB_DEVICE, &device_key(HUB_DEVICE));
    let mut sealed = vec![genesis];
    for index in 0..PLANTED_YIELDS {
        let proposal = authority::propose(
            space,
            charter,
            signer,
            Entry::Yield(Yield {
                handover_hash: low_key(2, index),
            }),
            NOW_MS,
        )
        .expect("sign a yield");
        let mut operation = operation_of(head);
        operation.content = Content::Authorization(Box::new(AuthorizationRecord {
            epoch: parent.epoch,
            revision: parent.revision + 1,
            previous_entry_hash: parent.hash(),
            anchor_device_id: parent.anchor_device_id.clone(),
            proposal,
            sequencing: Sequencing::Empty,
        }));
        sealed.push(seal_next(
            sealed.last().expect("a genesis"),
            &key,
            operation,
        ));
    }
    sealed
}

/// A peer whose record exchange answers every request busy.
struct BusyTransport;

impl SyncTransport for BusyTransport {
    async fn request(&self, _message: ControlMessage) -> Result<ControlMessage, String> {
        Ok(ControlMessage::Error {
            code: records::RECORDS_BUSY.to_string(),
            message: "record exchange is busy".to_string(),
            retryable: true,
        })
    }
}

impl Replica {
    /// Admits `envelopes` into the record store in batches of [`PLANT_BATCH`].
    async fn plant_records(&self, envelopes: &[Vec<u8>]) {
        for batch in envelopes.chunks(PLANT_BATCH) {
            self.hold_records(batch).await;
        }
    }

    /// Rows of the wanted set and refetch hashes.
    async fn wanted_rows(&self) -> usize {
        let rows: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM sync_auth_wanted)
                  + (SELECT COUNT(*) FROM sync_auth_refetch)",
        )
        .fetch_one(&self.pool)
        .await
        .expect("count wanted rows");
        usize::try_from(rows).expect("a count")
    }

    /// Holds `hash` for refetch.
    async fn hold_refetch(&self, hash: &[u8]) {
        sqlx::query("INSERT INTO sync_auth_refetch (space_id, envelope_hash) VALUES (?, ?)")
            .bind(self.ctx.space.as_bytes().as_slice())
            .bind(hash)
            .execute(&self.pool)
            .await
            .expect("hold a refetch hash");
    }
}

/// The D-M1 amendment to R7.7: a member holds a chain naming [`PLANTED`] parent entries no peer
/// holds, more than one session asks for, [`PLANTED_REFETCH`] refetch hashes no peer holds, and
/// [`PLANTED_YIELDS`] yields a key that is no admin signs, all sorting below every real key. It
/// loses an envelope of the anchor's chain and holds the hash of another for refetch. An
/// admission window passes before each round, in which it dials a peer that answers every
/// request busy, which resets that peer's backoff only, and then the anchor. Without a restart,
/// the sessions with the anchor ask for both and store them within the sessions the rotation
/// bounds: the wanted rows over the hashes a session asks for at least, plus one. The request
/// size and count come from the limits, scaled down with the planted counts.
#[test]
fn planted_wants_do_not_starve_an_honest_want_or_a_refetch_hash() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-planted", HUB_DEVICE).await;
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        for index in 0..FLOOD {
            let seed = 60 + u8::try_from(index).expect("small flood");
            sequence_here(
                &mut hub_replica,
                &enroll_mobile(space, charter, &format!("device-planted-{index}"), seed),
            )
            .await;
        }
        let hub_writer = hub_replica.writer.id();
        let (genesis, records) = hub_replica.own_records(hub_writer).await;
        assert_eq!(records.len(), FLOOD);
        let transport = record_hub(&hub_replica, RecordExchange::default());
        let phone = Replica::new("phone-planted", PHONE_DEVICE).await;
        let limits = RecordLimits {
            normal: RecordBudget {
                bytes: PLANTED_SERVE_BYTES,
                ..RecordLimits::default().normal
            },
            wanted_rounds: PLANTED_ROUNDS,
            wants_per_request: PLANTED_PER_REQUEST,
            ..RecordLimits::default()
        };
        let clock = ManualClock::new();
        let state = RecordExchange::default()
            .with_limits(limits)
            .with_clock(clock.clock());
        phone
            .records_with(&transport, &phone_ids(), HUB_DEVICE, &state)
            .await;
        let held = hub_replica.records_of(hub_writer).await;
        assert_eq!(phone.records_of(hub_writer).await, held);

        phone.plant_records(&planted(&genesis, &records[0])).await;
        let planted_wants: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sync_auth_wanted WHERE kind = 'entry'")
                .fetch_one(&phone.pool)
                .await
                .expect("count wanted entries");
        assert!(planted_wants >= i64::try_from(PLANTED).expect("a count"));
        let rogue = AdminKeyPair::from_seed(&[81; 32]).expect("admin seed");
        let head = records.last().expect("a record");
        phone
            .plant_records(&planted_yields(&genesis, head, &rogue, charter))
            .await;
        for index in 0..PLANTED_REFETCH {
            phone.hold_refetch(&low_key(1, index)).await;
        }
        let wanted = phone.wanted_hashes().await;
        let (lost, refetched) = (envelope_hash(&records[2]), envelope_hash(&records[4]));
        for gone in [&lost, &refetched] {
            sqlx::query("DELETE FROM sync_auth_envelopes WHERE envelope_hash = ?")
                .bind(gone.as_slice())
                .execute(&phone.pool)
                .await
                .expect("lose an envelope");
        }
        phone.hold_refetch(&refetched).await;
        // Each lost envelope is wanted by its entry, which the next record of the chain names,
        // and the refetch hash by itself.
        assert_eq!(phone.wanted_hashes().await, wanted + 3);
        assert!(phone.wants_hash(&refetched).await);

        let per_session = PLANTED_ROUNDS * (PLANTED_PER_REQUEST / 2);
        let bound = phone.wanted_rows().await.div_ceil(per_session) + 1;
        assert!(bound > 2, "the wanted set fits in {bound} sessions");
        let mut sessions = 0;
        while !(phone.holds_verified(&lost).await && phone.holds_verified(&refetched).await) {
            assert!(sessions < bound, "still wanted after {sessions} sessions");
            sessions += 1;
            clock.advance(limits.window);
            phone
                .records_with(&BusyTransport, &phone_ids(), "device-busy", &state)
                .await;
            phone
                .records_with(&transport, &phone_ids(), HUB_DEVICE, &state)
                .await;
        }
        assert_eq!(phone.records_of(hub_writer).await, held);
        assert_eq!(phone.refetching().await.len(), PLANTED_REFETCH);
        hub_replica.close().await;
        phone.close().await;
    });
}

/// Envelopes per page the held peer of the M2 test pushes.
const PUSHED_PAGE: usize = 2;

/// The M2 amendment to R7.6: the hub the kit removed pushes its flood to the new anchor in
/// full pages every window, past a low budget of one envelope per window. A page that starts
/// in budget is taken whole and its overspend is debt later windows repay, so after each window
/// the anchor holds no more than one envelope per window that passed and one page.
#[test]
fn a_held_peer_pushing_full_pages_stays_within_its_budget_over_windows() {
    block_on(async {
        let (hub_replica, laptop) = forced_away("flood-push").await;
        let hub_writer = hub_replica.writer.id();
        let before = laptop.records_of(hub_writer).await;
        let (_, records) = hub_replica.own_records(hub_writer).await;
        let hash = |bytes: &[u8]| Envelope::decode(bytes).expect("decode").hash().to_vec();
        let mut flood = Vec::new();
        for record in records {
            if !laptop.holds(&hash(&record)).await {
                flood.push(record);
            }
        }
        assert_eq!(flood.len(), FLOOD);
        let clock = ManualClock::new();
        let limits = one_low_envelope(hub_replica.largest_record_of(hub_writer).await);
        let state = RecordExchange::default()
            .with_limits(limits)
            .with_clock(clock.clock());
        let engine = Engine::vault();

        let mut windows = 0;
        loop {
            windows += 1;
            assert!(windows <= 4 * FLOOD, "the flood never finished moving");
            for page in flood.chunks(PUSHED_PAGE) {
                if laptop.holds(&hash(page.last().expect("a page"))).await {
                    continue;
                }
                let mut conn = acquire(&laptop.pool).await;
                let push = ControlMessage::SyncRecordPush {
                    protocol_version: PROTOCOL_VERSION,
                    vault_id: VAULT_ID.to_string(),
                    device_id: HUB_DEVICE.to_string(),
                    records: ops_to_wire(page),
                };
                // A spent window answers busy; the pusher tries every page it has not
                // delivered again.
                let _ = records::respond(&state, &engine, &mut conn, &laptop.ctx, HUB_DEVICE, push)
                    .await;
            }
            let taken =
                usize::try_from(laptop.records_of(hub_writer).await - before).expect("a count");
            assert!(
                taken <= windows + PUSHED_PAGE,
                "{taken} after {windows} windows"
            );
            if taken == FLOOD {
                break;
            }
            clock.advance(limits.window);
        }
        assert!(windows > FLOOD / PUSHED_PAGE, "{windows} windows");
        assert!(laptop.removed(HUB_DEVICE).await);
        hub_replica.close().await;
        laptop.close().await;
    });
}

/// A device the hub enrolls before the laptop, which the kit removes with the hub.
const THIRD_DEVICE: &str = "device-third";
/// Throwaway writers the hub's device key certifies for rewraps of one kit removal.
const REWRAP_WRITERS: u8 = 24;

/// The D-H1 amendment to R7.6 tests 45 and 50: the kit removed the hub and a third device. The
/// hub's key rewraps the kit removal of the third device at the parent where the hub enrolled
/// it, with that parent's revision and epoch and the hub as anchor, under [`REWRAP_WRITERS`]
/// throwaway writers. Each writer seals a twin at the same sequence, which keeps its rewrap from
/// being a node, and each rewrap a fresh discovery key, which makes its entry new. No kit
/// handover opened that epoch instance and it is not the selected head, so no rewrap takes kit
/// class, whether a relay stored it already or the hub pushes it past the backstop: the kit
/// reserve stays whole and the backstop never stops. The honest kit removal sealed in the kit
/// epoch still lands past the bound.
#[test]
fn rewraps_of_a_kit_removal_at_the_old_anchors_parents_never_take_kit_class() {
    block_on(async {
        let (hub_replica, laptop) = forced_away_removing("rewrap-third", &[THIRD_DEVICE]).await;
        let laptop_writer = laptop.writer.id();
        let log = laptop.records_besides(laptop_writer).await;
        let (genesis, kit) = laptop.own_records(laptop_writer).await;
        let removal = kit
            .iter()
            .find(|envelope| {
                authorization_record(envelope).is_some_and(|record| {
                    matches!(&record.proposal.proposal.entry,
                        Entry::Remove(remove) if remove.device_id == THIRD_DEVICE)
                })
            })
            .expect("the kit removal of the third device");
        let enrollment = log
            .iter()
            .filter_map(|envelope| authorization_record(envelope))
            .find(|record| {
                matches!(&record.proposal.proposal.entry,
                    Entry::Enroll(enroll) if enroll.device_id == THIRD_DEVICE)
            })
            .expect("the third device's enrollment");
        assert_eq!(enrollment.anchor_device_id, HUB_DEVICE);
        let rewrap = |seed: u8, twin: u8| {
            let mut operation = operation_of(removal);
            let Content::Authorization(record) = &mut operation.content else {
                panic!("an authorization record");
            };
            record.previous_entry_hash = enrollment.hash();
            record.revision = enrollment.revision + 1;
            record.epoch = enrollment.epoch;
            record.anchor_device_id = HUB_DEVICE.to_string();
            let Sequencing::Removal { discovery_key, .. } = &mut record.sequencing else {
                panic!("a removal");
            };
            discovery_key[0] = seed;
            discovery_key[1] = twin;
            operation
        };
        let hub_device = device_key(HUB_DEVICE);
        let chains: Vec<Vec<Vec<u8>>> = (0..REWRAP_WRITERS)
            .map(|index| {
                let seed = 100 + index;
                let (key, genesis) = throwaway_writer(&genesis, seed, HUB_DEVICE, &hub_device);
                let first = seal_next(&genesis, &key, rewrap(seed, 0));
                let twin = seal_next(&genesis, &key, rewrap(seed, 1));
                vec![genesis, first, twin]
            })
            .collect();
        let writer_of = |chain: &[Vec<u8>]| Envelope::decode(&chain[0]).expect("decode").writer;
        let (stored, pushed) = chains.split_at(chains.len() / 2);

        let phone = Replica::new("phone-rewrap-third", PHONE_DEVICE).await;
        phone.hold_records(&log).await;
        let no_kit_class = |envelopes: Vec<Vec<u8>>| {
            let phone = &phone;
            async move {
                let mut conn = acquire(&phone.pool).await;
                let classes = Engine::vault()
                    .record_classes(&mut conn, &phone.ctx, &envelopes)
                    .await
                    .expect("classes");
                assert!(!classes.contains(&RecordClass::Kit), "{classes:?}");
            }
        };
        no_kit_class(chains.concat()).await;
        // A relay stores each rewrap, then its twin.
        for chain in stored {
            phone.hold_records(&chain[..2]).await;
            phone.hold_records(&chain[2..]).await;
            assert_eq!(phone.records_of(writer_of(chain)).await, 3);
            no_kit_class(chains.concat()).await;
        }

        let state = RecordExchange::default().with_limits(kit_reserve_only(1024 * 1024));
        let engine = Engine::vault();
        for chain in pushed {
            for page in [&chain[..2], &chain[2..]] {
                let mut conn = acquire(&phone.pool).await;
                let push = ControlMessage::SyncRecordPush {
                    protocol_version: PROTOCOL_VERSION,
                    vault_id: VAULT_ID.to_string(),
                    device_id: HUB_DEVICE.to_string(),
                    records: ops_to_wire(page),
                };
                // The backstop keeps nothing of them, whatever the answer.
                let _ = records::respond(&state, &engine, &mut conn, &phone.ctx, HUB_DEVICE, push)
                    .await;
            }
            assert_eq!(phone.records_of(writer_of(chain)).await, 0);
        }
        assert_eq!(state.kit_spent(), 0);
        assert_ne!(state.backstop_state(), Some(BackstopState::Stopped));
        assert!(!phone.removed(THIRD_DEVICE).await);

        let honest = record_hub(&laptop, RecordExchange::default());
        phone
            .sync_with(&honest, &phone_ids(), LAPTOP_DEVICE, &state)
            .await;
        assert!(phone.removed(HUB_DEVICE).await);
        assert!(phone.removed(THIRD_DEVICE).await);
        assert!(state.kit_spent() > 0);
        assert_eq!(state.backstop_state(), Some(BackstopState::Reserve));
        hub_replica.close().await;
        laptop.close().await;
        phone.close().await;
    });
}

/// A device a record enrolls after its writer forked its data chain.
const FORK_ENROLLED: &str = "device-fork-enrolled";

/// The D-M2 amendment to R7.7: a writer the hub's device key certifies seals two data operations
/// at one sequence, then a record after one of them that enrolls a device at the selected head.
/// A member holds all of it, so the record is not a node there. A peer the log does not enroll
/// pulls the member's pages, which serve the data pair in one page: it admits both halves, and
/// its walk and record store match the member's.
#[test]
fn a_held_peer_takes_a_data_fork_pair_from_one_page_and_walks_as_a_member_does() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-data-fork", HUB_DEVICE).await;
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        for index in 0..2u8 {
            sequence_here(
                &mut hub_replica,
                &enroll_mobile(
                    space,
                    charter,
                    &format!("device-data-fork-{index}"),
                    60 + index,
                ),
            )
            .await;
        }
        let hub_writer = hub_replica.writer.id();
        let (genesis, records) = hub_replica.own_records(hub_writer).await;
        let head_envelope = records.last().expect("a record");
        let head = authorization_record(head_envelope).expect("an authorization record");
        let (key, fork_genesis) =
            throwaway_writer(&genesis, 83, HUB_DEVICE, &device_key(HUB_DEVICE));
        let data = |content: Content| {
            let mut operation = operation_of(&fork_genesis);
            operation.header.authorization_revision = head.revision;
            operation.content = content;
            seal_next(&fork_genesis, &key, operation)
        };
        let changes = data(Content::Changes(Vec::new()));
        let acknowledgement = data(Content::Acknowledgement);
        let mut operation = operation_of(head_envelope);
        let Content::Authorization(record) = &mut operation.content else {
            panic!("an authorization record");
        };
        record.revision = head.revision + 1;
        record.previous_entry_hash = head.hash();
        record.proposal = enroll_mobile(space, charter, FORK_ENROLLED, 96);
        record.sequencing = Sequencing::Empty;
        let enrolled = seal_next(&changes, &key, operation);
        let fork_writer = Envelope::decode(&fork_genesis).expect("decode").writer;
        let fork = vec![
            fork_genesis,
            changes.clone(),
            acknowledgement.clone(),
            enrolled.clone(),
        ];

        let member = Replica::new("member-data-fork", "device-member-data-fork").await;
        member
            .hold_records(&hub_replica.records_besides(fork_writer).await)
            .await;
        member.hold_records(&fork).await;
        for envelope in &fork {
            assert!(member.holds(&envelope_hash(envelope)).await);
        }

        let phone = Replica::new("phone-data-fork", PHONE_DEVICE).await;
        let transport = record_hub(&member, RecordExchange::default());
        let limits = RecordLimits {
            low: RecordLimits::default().normal,
            ..RecordLimits::default()
        };
        let state = RecordExchange::default().with_limits(limits);
        phone
            .records_with(&transport, &phone_ids(), "device-member-data-fork", &state)
            .await;
        for envelope in &fork {
            assert!(phone.holds(&envelope_hash(envelope)).await);
        }

        let walk = |replica: &Replica| {
            let (pool, ctx) = (replica.pool.clone(), replica.ctx.clone());
            async move {
                let mut conn = acquire(&pool).await;
                let engine = Engine::vault();
                let view = engine
                    .authorization(&mut conn, &ctx)
                    .await
                    .expect("authorization")
                    .expect("charter");
                let storage = engine
                    .authorization_storage(&mut conn, &ctx)
                    .await
                    .expect("authorization storage");
                (view, storage)
            }
        };
        let (member_view, member_storage) = walk(&member).await;
        let (phone_view, phone_storage) = walk(&phone).await;
        assert!(
            member_storage
                .forked_writers
                .iter()
                .any(|forked| forked.writer == fork_writer)
        );
        assert!(
            !member_view
                .devices
                .iter()
                .any(|device| device.device_id == FORK_ENROLLED),
            "a record past a fork is not a node"
        );
        assert_eq!(phone_view, member_view);
        assert_eq!(phone_storage, member_storage);
        hub_replica.close().await;
        member.close().await;
        phone.close().await;
    });
}

/// An admin desktop the stolen anchor enrolls, which then takes the anchor from itself.
const PUPPET_DEVICE: &str = "device-puppet";
/// Self-takes the puppet seals, each the selected head of the replica that stores it.
const SELF_TAKES: u8 = 6;

/// The D2-M1 amendment to R7.6 tests 45 and 50: the stolen anchor enrolls an admin puppet,
/// which takes the anchor and then takes it from itself [`SELF_TAKES`] times, each take a new
/// epoch instance. A phone without the kit handover stores each take as its selected head, and
/// the puppet's key rewraps the kit removal of the hub at that head under a throwaway writer.
/// Outside the instances a kit handover opened, a kit proposal spends one kit slot in total, so
/// one rewrap at most takes kit class: the kit reserve keeps nearly all its room, the backstop
/// never stops, and the honest kit handover still lands past the bound.
#[test]
fn self_takes_of_a_puppet_admin_spend_one_kit_slot_per_kit_proposal() {
    block_on(async {
        let (mut hub_replica, laptop) = forced_away_removing("self-take", &[]).await;
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        let puppet_admin = AdminKeyPair::from_seed(&[44; 32]).expect("admin seed");
        for entry in [
            authority::enroll_entry(Enrollment {
                device_id: PUPPET_DEVICE.to_string(),
                device_key: device_key(PUPPET_DEVICE).public_key(),
                tls_fingerprint: [44; 32],
                label: "Puppet".to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            authority::promote_entry(PUPPET_DEVICE, puppet_admin.public_key()),
        ] {
            let proposal = authority::propose(space, charter, &hub_keys().admin, entry, NOW_MS)
                .expect("sign the puppet's entry");
            sequence_here(&mut hub_replica, &proposal).await;
        }
        let laptop_writer = laptop.writer.id();
        let log = hub_replica.records_besides(laptop_writer).await;
        let (laptop_genesis, kit) = laptop.own_records(laptop_writer).await;
        let removal = kit
            .iter()
            .find(|envelope| {
                authorization_record(envelope).is_some_and(|record| {
                    matches!(&record.proposal.proposal.entry,
                        Entry::Remove(remove) if remove.device_id == HUB_DEVICE)
                })
            })
            .expect("the kit removal of the hub");

        let mut puppet = Replica::new("puppet-self-take", PUPPET_DEVICE).await;
        puppet.hold_records(&log).await;
        let phone = Replica::new("phone-self-take", PHONE_DEVICE).await;
        phone.hold_records(&log).await;
        let state = RecordExchange::default().with_limits(kit_reserve_only(1024 * 1024));
        let engine = Engine::vault();
        let puppet_device = device_key(PUPPET_DEVICE);
        let puppet_writer = puppet.writer.id();
        let mut kit_rewraps = 0;
        let mut largest_rewrap = 0;
        for round in 0..SELF_TAKES {
            let mut conn = acquire(&puppet.pool).await;
            authority::seal_handover_here(
                &puppet.engine,
                &mut conn,
                &puppet.ctx,
                &puppet_admin,
                &mut puppet.writer.local(),
                None,
                NOW_MS,
            )
            .await
            .expect("seal a take")
            .expect("the puppet takes the anchor");
            drop(conn);
            let seq = puppet.stored().await.get(&puppet_writer);
            puppet.writer.committed(seq).expect("record commit");
            let (certificate, takes) = puppet.own_records(puppet_writer).await;
            let take = takes.last().expect("a take");
            let take_record = authorization_record(take).expect("an authorization record");
            assert_eq!(take_record.anchor_device_id, PUPPET_DEVICE);
            phone.hold_records(&[certificate, take.clone()]).await;

            let mut operation = operation_of(removal);
            let Content::Authorization(record) = &mut operation.content else {
                panic!("an authorization record");
            };
            record.previous_entry_hash = take_record.hash();
            record.revision = take_record.revision + 1;
            record.epoch = take_record.epoch;
            record.anchor_device_id = PUPPET_DEVICE.to_string();
            let Sequencing::Removal { discovery_key, .. } = &mut record.sequencing else {
                panic!("a removal");
            };
            discovery_key[0] = round;
            let (key, genesis) =
                throwaway_writer(&laptop_genesis, 120 + round, PUPPET_DEVICE, &puppet_device);
            let rewrap = vec![genesis.clone(), seal_next(&genesis, &key, operation)];
            largest_rewrap = largest_rewrap.max(rewrap.iter().map(Vec::len).sum::<usize>());
            let classes = {
                let mut conn = acquire(&phone.pool).await;
                engine
                    .record_classes(&mut conn, &phone.ctx, &rewrap)
                    .await
                    .expect("classes")
            };
            if classes[1] == RecordClass::Kit {
                kit_rewraps += 1;
            }
            let mut conn = acquire(&phone.pool).await;
            let push = ControlMessage::SyncRecordPush {
                protocol_version: PROTOCOL_VERSION,
                vault_id: VAULT_ID.to_string(),
                device_id: PUPPET_DEVICE.to_string(),
                records: ops_to_wire(&rewrap),
            };
            // Past the backstop only a kit envelope is stored, whatever the answer.
            let _ =
                records::respond(&state, &engine, &mut conn, &phone.ctx, PUPPET_DEVICE, push).await;
            assert_ne!(state.backstop_state(), Some(BackstopState::Stopped));
        }
        assert_eq!(kit_rewraps, 1, "the head allowance takes one slot in total");
        assert!(
            state.kit_spent() <= u64::try_from(largest_rewrap).expect("a small rewrap"),
            "the kit reserve spent {} bytes",
            state.kit_spent()
        );

        let honest = record_hub(&laptop, RecordExchange::default());
        phone
            .sync_with(&honest, &phone_ids(), LAPTOP_DEVICE, &state)
            .await;
        for envelope in &kit {
            assert!(phone.holds_verified(&envelope_hash(envelope)).await);
        }
        assert_eq!(state.backstop_state(), Some(BackstopState::Reserve));
        hub_replica.close().await;
        laptop.close().await;
        puppet.close().await;
        phone.close().await;
    });
}

/// A data operation of `writer`'s chain after `previous`, at the selected head's revision,
/// whose one change carries `bytes` bytes filled with `fill`, in blob fields within their bound.
fn large_changes(
    previous: &[u8],
    key: &WriterKeyPair,
    revision: u64,
    bytes: usize,
    fill: u8,
) -> Vec<u8> {
    let table = TableId(0);
    let group = GroupId::new(0).expect("a group id");
    let fields = vec![fill; bytes]
        .chunks(MAX_BLOB_FIELD_BYTES)
        .map(|chunk| Field::Blob(chunk.to_vec()))
        .collect();
    let value = Value::new(fields).expect("blobs in bounds");
    let mut operation = operation_of(previous);
    operation.header.authorization_revision = revision;
    operation.content = Content::Changes(vec![Change {
        table,
        row: RowKey::new("large-row").expect("a row key"),
        action: ChangeAction::Create,
        groups: vec![GroupValue::new(table, group, value)],
        replaced_by: None,
        cause: None,
    }]);
    seal_next(previous, key, operation)
}

/// The D2-M2 amendment to R7.7: the two halves of a data fork pair are each over half a page,
/// so no page carries both. A member holds the pair and a record that continues from one half.
/// A peer the log does not enroll pulls the member's pages and a relay takes them as pushed
/// pages: each keeps the half one page ends with until the next page brings the other, admits
/// the pair, and walks as the member does.
#[test]
fn halves_of_a_data_fork_pair_on_two_pages_reach_a_held_peer_and_a_relay() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-split-fork", HUB_DEVICE).await;
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        sequence_here(
            &mut hub_replica,
            &enroll_mobile(space, charter, "device-split-fork", 64),
        )
        .await;
        let hub_writer = hub_replica.writer.id();
        let (genesis, records) = hub_replica.own_records(hub_writer).await;
        let head_envelope = records.last().expect("a record");
        let head = authorization_record(head_envelope).expect("an authorization record");
        let (key, fork_genesis) =
            throwaway_writer(&genesis, 84, HUB_DEVICE, &device_key(HUB_DEVICE));
        let half = SYNC_PAGE_BYTES / 2 + 1024;
        let first = large_changes(&fork_genesis, &key, head.revision, half, 1);
        let second = large_changes(&fork_genesis, &key, head.revision, half, 2);
        assert!(first.len() + second.len() > SYNC_PAGE_BYTES);
        let mut operation = operation_of(head_envelope);
        let Content::Authorization(record) = &mut operation.content else {
            panic!("an authorization record");
        };
        record.revision = head.revision + 1;
        record.previous_entry_hash = head.hash();
        record.proposal = enroll_mobile(space, charter, FORK_ENROLLED, 97);
        record.sequencing = Sequencing::Empty;
        let enrolled = seal_next(&first, &key, operation);
        let fork_writer = Envelope::decode(&fork_genesis).expect("decode").writer;
        let fork = vec![fork_genesis, first, second, enrolled];

        let member = Replica::new("member-split-fork", "device-member-split-fork").await;
        member
            .hold_records(&hub_replica.records_besides(fork_writer).await)
            .await;
        member.hold_records(&fork).await;
        for envelope in &fork {
            assert!(member.holds(&envelope_hash(envelope)).await);
        }
        let limits = RecordLimits {
            low: RecordLimits::default().normal,
            ..RecordLimits::default()
        };

        let phone = Replica::new("phone-split-fork", PHONE_DEVICE).await;
        let transport = record_hub(&member, RecordExchange::default());
        let state = RecordExchange::default().with_limits(limits);
        phone
            .records_with(&transport, &phone_ids(), "device-member-split-fork", &state)
            .await;

        let relay = Replica::new("relay-split-fork", "device-relay-split-fork").await;
        let relay_state = RecordExchange::default().with_limits(limits);
        member
            .push_pages_to(&relay, "device-member-split-fork", &relay_state)
            .await;

        let (member_view, member_storage) = member.walk().await;
        assert!(
            member_storage
                .forked_writers
                .iter()
                .any(|forked| forked.writer == fork_writer)
        );
        for peer in [&phone, &relay] {
            for envelope in &fork {
                assert!(peer.holds_verified(&envelope_hash(envelope)).await);
            }
            assert_eq!(
                peer.walk().await,
                (member_view.clone(), member_storage.clone())
            );
        }
        hub_replica.close().await;
        member.close().await;
        phone.close().await;
        relay.close().await;
    });
}

impl Replica {
    /// The authorization view and record storage this replica walks.
    async fn walk(
        &self,
    ) -> (
        ganbaru_sync::AuthorizationView,
        ganbaru_sync::AuthorizationStorage,
    ) {
        let mut conn = acquire(&self.pool).await;
        let engine = Engine::vault();
        let view = engine
            .authorization(&mut conn, &self.ctx)
            .await
            .expect("authorization")
            .expect("charter");
        let storage = engine
            .authorization_storage(&mut conn, &self.ctx)
            .await
            .expect("authorization storage");
        (view, storage)
    }

    /// Pushes every page of this replica's record store to `target`, as the device `peer`
    /// whose exchange state there is `state`.
    async fn push_pages_to(&self, target: &Replica, peer: &str, state: &RecordExchange) {
        let engine = Engine::vault();
        let mut cursor = ganbaru_sync::RecordCursor::default();
        loop {
            let page = {
                let mut conn = acquire(&self.pool).await;
                engine
                    .record_page(
                        &mut conn,
                        &self.ctx,
                        &cursor,
                        SYNC_PAGE_BYTES,
                        MAX_SYNC_RECORDS,
                    )
                    .await
                    .expect("a record page")
            };
            if page.envelopes.is_empty() {
                break;
            }
            let bytes: Vec<Vec<u8>> = page.envelopes.into_iter().map(|e| e.bytes).collect();
            assert!(bytes.iter().map(Vec::len).sum::<usize>() <= SYNC_PAGE_BYTES);
            target.pushed_by(state, peer, &bytes).await;
            cursor = page.cursor;
        }
    }

    /// Answers a push of `envelopes` from the device `peer` whose exchange state here is
    /// `state`.
    async fn pushed_by(&self, state: &RecordExchange, peer: &str, envelopes: &[Vec<u8>]) {
        let mut conn = acquire(&self.pool).await;
        let push = ControlMessage::SyncRecordPush {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: peer.to_string(),
            records: ops_to_wire(envelopes),
        };
        records::respond(state, &Engine::vault(), &mut conn, &self.ctx, peer, push)
            .await
            .expect("the push is taken");
    }
}

/// Throwaway writers in the crossing layout of the D3-M1 amendment.
const CROSSING_WRITERS: u8 = 26;
/// Bytes of the large first half of each crossing pair, so the member's hold keeps the first
/// halves of two pushed pages.
const CROSSING_HALF: usize = 56 * 1024;

/// The D3-M1 amendment to R7.7: an attacker seals data fork pairs at one sequence for
/// [`CROSSING_WRITERS`] throwaway writers, each with a large first half and a small second
/// one, and pushes a member the certificates and pages of first halves, the last page with
/// every second half. The run of first halves is longer than a hold, yet the member stores
/// each pair's halves next to each other, so a page boundary splits one pair at most. A peer
/// the log does not enroll pulls the member's pages and a relay takes them as pushed pages:
/// every pair reaches both, and both walk as the member does.
#[test]
fn a_run_of_first_halves_longer_than_a_hold_still_pairs_on_a_held_peer_and_a_relay() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-crossing", HUB_DEVICE).await;
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        sequence_here(
            &mut hub_replica,
            &enroll_mobile(space, charter, "device-crossing", 65),
        )
        .await;
        let hub_writer = hub_replica.writer.id();
        let (genesis, records) = hub_replica.own_records(hub_writer).await;
        let head = authorization_record(records.last().expect("a record"))
            .expect("an authorization record");
        let hub_device = device_key(HUB_DEVICE);
        let mut geneses = Vec::new();
        let mut firsts = Vec::new();
        let mut seconds = Vec::new();
        for index in 0..CROSSING_WRITERS {
            let (key, writer_genesis) =
                throwaway_writer(&genesis, 100 + index, HUB_DEVICE, &hub_device);
            firsts.push(large_changes(
                &writer_genesis,
                &key,
                head.revision,
                CROSSING_HALF,
                1,
            ));
            seconds.push(large_changes(&writer_genesis, &key, head.revision, 16, 2));
            geneses.push(writer_genesis);
        }
        let first_writer = Envelope::decode(&geneses[0]).expect("decode").writer;
        let per_page = (SYNC_PAGE_BYTES - 64 * 1024) / firsts[0].len();
        let tail = firsts.len() - per_page;
        let held: usize = firsts[..tail].iter().map(Vec::len).sum();
        assert!(
            held <= 2 * MAX_OPERATION_BYTES,
            "the member holds every first half until the last push"
        );
        let mut pushes: Vec<Vec<Vec<u8>>> =
            firsts[..tail].chunks(per_page).map(<[_]>::to_vec).collect();
        pushes[0].splice(0..0, geneses.iter().cloned());
        let mut last = firsts[tail..].to_vec();
        last.extend(seconds.iter().cloned());
        pushes.push(last);
        for push in &pushes {
            assert!(push.iter().map(Vec::len).sum::<usize>() <= SYNC_PAGE_BYTES);
        }
        let run: usize = firsts.iter().map(Vec::len).sum();
        assert!(
            run > 2 * MAX_OPERATION_BYTES,
            "the run of first halves is longer than a hold"
        );

        let limits = RecordLimits {
            low: RecordLimits::default().normal,
            ..RecordLimits::default()
        };
        let member = Replica::new("member-crossing", "device-member-crossing").await;
        member
            .hold_records(&hub_replica.records_besides(first_writer).await)
            .await;
        let member_state = RecordExchange::default().with_limits(limits);
        for push in &pushes {
            member
                .pushed_by(&member_state, "device-attacker", push)
                .await;
        }
        let pairs: Vec<&Vec<u8>> = firsts.iter().chain(&seconds).collect();
        for envelope in &pairs {
            assert!(member.holds_verified(&envelope_hash(envelope)).await);
        }

        let phone = Replica::new("phone-crossing", PHONE_DEVICE).await;
        let transport = record_hub(&member, RecordExchange::default());
        let state = RecordExchange::default().with_limits(limits);
        phone
            .records_with(&transport, &phone_ids(), "device-member-crossing", &state)
            .await;
        let relay = Replica::new("relay-crossing", "device-relay-crossing").await;
        let relay_state = RecordExchange::default().with_limits(limits);
        member
            .push_pages_to(&relay, "device-member-crossing", &relay_state)
            .await;

        let walked = member.walk().await;
        assert_eq!(walked.1.forked_writers.len(), usize::from(CROSSING_WRITERS));
        for peer in [&phone, &relay] {
            for envelope in &pairs {
                assert!(peer.holds_verified(&envelope_hash(envelope)).await);
            }
            assert_eq!(peer.walk().await, walked);
        }
        hub_replica.close().await;
        member.close().await;
        phone.close().await;
        relay.close().await;
    });
}

/// A peer that serves queued record pages, finds nothing it is asked for, and takes every push.
#[derive(Default)]
struct ScriptedPages(Mutex<Vec<Vec<Vec<u8>>>>);

impl ScriptedPages {
    fn serve(&self, page: Vec<Vec<u8>>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(page);
    }
}

impl SyncTransport for ScriptedPages {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        Ok(match message {
            ControlMessage::SyncRecords { cursor, .. } => {
                let mut pages = self.0.lock().unwrap_or_else(PoisonError::into_inner);
                let records = if pages.is_empty() {
                    Vec::new()
                } else {
                    pages.remove(0)
                };
                ControlMessage::SyncRecordPage {
                    records: ops_to_wire(&records),
                    cursor,
                    more: false,
                }
            }
            ControlMessage::SyncRecordGet { .. } => ControlMessage::SyncRecordFound {
                records: Vec::new(),
            },
            ControlMessage::SyncRecordPush { .. } => ControlMessage::SyncRecordPushResult {
                admitted: 0,
                missing_hashes: Vec::new(),
                missing_writers: Vec::new(),
            },
            _ => ControlMessage::Error {
                code: "unexpected".to_string(),
                message: "not a record exchange request".to_string(),
                retryable: false,
            },
        })
    }
}

/// The D3-M1 amendment to R7.7: a page this device pulls from a peer ends with the large first
/// half of a data fork pair, and a page the same peer pushes ends with the first half of
/// another pair behind a lone data envelope, together more than one hold. Each stream keeps
/// its own hold, so the push does not evict the pulled half, and the next page of each stream
/// pairs its half.
#[test]
fn a_push_from_a_peer_does_not_evict_the_half_its_pulled_pages_left() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-streams", HUB_DEVICE).await;
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        sequence_here(
            &mut hub_replica,
            &enroll_mobile(space, charter, "device-streams-enrolled", 66),
        )
        .await;
        let hub_writer = hub_replica.writer.id();
        let (genesis, records) = hub_replica.own_records(hub_writer).await;
        let head = authorization_record(records.last().expect("a record"))
            .expect("an authorization record");
        let hub_device = device_key(HUB_DEVICE);
        let pulled_half = MAX_OPERATION_BYTES - 16 * 1024;
        let pushed_half = (SYNC_PAGE_BYTES - 16 * 1024) / 2;
        let mut geneses = Vec::new();
        let mut pairs = Vec::new();
        let mut lone = Vec::new();
        for (index, first) in [pulled_half, pushed_half, pushed_half]
            .into_iter()
            .enumerate()
        {
            let seed = 112 + u8::try_from(index).expect("few writers");
            let (key, writer_genesis) = throwaway_writer(&genesis, seed, HUB_DEVICE, &hub_device);
            let large = large_changes(&writer_genesis, &key, head.revision, first, 1);
            if index < 2 {
                let small = large_changes(&writer_genesis, &key, head.revision, 16, 2);
                pairs.push([large, small]);
            } else {
                lone = large;
            }
            geneses.push(writer_genesis);
        }
        let [pulled_first, pulled_second] = pairs[0].clone();
        let [pushed_first, pushed_second] = pairs[1].clone();
        let pushed_page = vec![lone, pushed_first];
        assert!(pushed_page.iter().map(Vec::len).sum::<usize>() <= SYNC_PAGE_BYTES);
        assert!(
            pulled_first.len() + pushed_page.iter().map(Vec::len).sum::<usize>()
                > 2 * MAX_OPERATION_BYTES,
            "one hold cannot keep both streams' halves"
        );
        let first_writer = Envelope::decode(&geneses[0]).expect("decode").writer;

        let phone = Replica::new("phone-streams", PHONE_DEVICE).await;
        phone
            .hold_records(&hub_replica.records_besides(first_writer).await)
            .await;
        phone.hold_records(&geneses).await;
        let limits = RecordLimits {
            low: RecordLimits::default().normal,
            ..RecordLimits::default()
        };
        let state = RecordExchange::default().with_limits(limits);
        let peer = "device-streams";
        let transport = ScriptedPages::default();
        transport.serve(vec![pulled_first]);
        phone
            .records_with(&transport, &phone_ids(), peer, &state)
            .await;
        phone.pushed_by(&state, peer, &pushed_page).await;
        transport.serve(vec![pulled_second]);
        phone
            .records_with(&transport, &phone_ids(), peer, &state)
            .await;
        phone.pushed_by(&state, peer, &[pushed_second]).await;
        for envelope in pairs.iter().flatten() {
            assert!(phone.holds_verified(&envelope_hash(envelope)).await);
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

/// The D3-L1 amendment to R7.6: the puppet admin takes the anchor, and a phone without the kit
/// handover stores the take as its selected head. The kit's promotion of the laptop arrives
/// rewrapped at that head under a writer the puppet certifies, and in the same set, behind it,
/// the stolen hub key's rewrap at the older parent the take continues from. The single slot
/// the proposal has outside kit instances goes to the first record that binds there, so the
/// rewrap at the head keeps kit class and the older one does not take it.
#[test]
fn a_former_anchors_rewrap_at_an_older_parent_does_not_take_the_heads_kit_slot() {
    block_on(async {
        let (mut hub_replica, laptop) = forced_away_removing("front-run", &[]).await;
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        let puppet_admin = AdminKeyPair::from_seed(&[45; 32]).expect("admin seed");
        for entry in [
            authority::enroll_entry(Enrollment {
                device_id: PUPPET_DEVICE.to_string(),
                device_key: device_key(PUPPET_DEVICE).public_key(),
                tls_fingerprint: [45; 32],
                label: "Puppet".to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            authority::promote_entry(PUPPET_DEVICE, puppet_admin.public_key()),
        ] {
            let proposal = authority::propose(space, charter, &hub_keys().admin, entry, NOW_MS)
                .expect("sign the puppet's entry");
            sequence_here(&mut hub_replica, &proposal).await;
        }
        let laptop_writer = laptop.writer.id();
        let log = hub_replica.records_besides(laptop_writer).await;
        let (laptop_genesis, kit) = laptop.own_records(laptop_writer).await;
        let promotion = kit
            .iter()
            .find(|envelope| {
                authorization_record(envelope).is_some_and(|record| {
                    matches!(&record.proposal.proposal.entry,
                        Entry::SetRole(SetRole::Device { device_id, .. }) if device_id == LAPTOP_DEVICE)
                })
            })
            .expect("the kit promotion of the laptop");

        let mut puppet = Replica::new("puppet-front-run", PUPPET_DEVICE).await;
        puppet.hold_records(&log).await;
        let mut conn = acquire(&puppet.pool).await;
        authority::seal_handover_here(
            &puppet.engine,
            &mut conn,
            &puppet.ctx,
            &puppet_admin,
            &mut puppet.writer.local(),
            None,
            NOW_MS,
        )
        .await
        .expect("seal a take")
        .expect("the puppet takes the anchor");
        drop(conn);
        let puppet_writer = puppet.writer.id();
        let (certificate, takes) = puppet.own_records(puppet_writer).await;
        let take = takes.last().expect("a take");
        let take_record = authorization_record(take).expect("an authorization record");
        let older = log
            .iter()
            .filter_map(|envelope| authorization_record(envelope))
            .find(|record| record.hash() == take_record.previous_entry_hash)
            .expect("the record the take continues from");
        assert_eq!(older.anchor_device_id, HUB_DEVICE);
        let phone = Replica::new("phone-front-run", PHONE_DEVICE).await;
        phone.hold_records(&log).await;
        phone.hold_records(&[certificate, take.clone()]).await;

        let rewrap = |parent: &AuthorizationRecord, seed: u8, device_id: &str| {
            let mut operation = operation_of(promotion);
            let Content::Authorization(record) = &mut operation.content else {
                panic!("an authorization record");
            };
            record.previous_entry_hash = parent.hash();
            record.revision = parent.revision + 1;
            record.epoch = parent.epoch;
            record.anchor_device_id = parent.anchor_device_id.clone();
            let (key, genesis) =
                throwaway_writer(&laptop_genesis, seed, device_id, &device_key(device_id));
            let sealed = seal_next(&genesis, &key, operation);
            [genesis, sealed]
        };
        let mut set = rewrap(&take_record, 130, PUPPET_DEVICE).to_vec();
        set.extend(rewrap(&older, 131, HUB_DEVICE));
        let classes = {
            let mut conn = acquire(&phone.pool).await;
            Engine::vault()
                .record_classes(&mut conn, &phone.ctx, &set)
                .await
                .expect("classes")
        };
        assert_eq!(classes[1], RecordClass::Kit, "the rewrap at the head");
        assert_ne!(
            classes[3],
            RecordClass::Kit,
            "the rewrap at the older parent"
        );
        hub_replica.close().await;
        laptop.close().await;
        puppet.close().await;
        phone.close().await;
    });
}
