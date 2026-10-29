//! Record exchange over the authorization record store: pages by class with a cursor the
//! requester keeps, wanted lookups that chase missing parents and certificates, and storage
//! accounting for the backstop and growth alerts.

use ganbaru_sync_contracts::{DeviceRole, Digest32, Entry, SetRole};

use super::authorization::{admin_entry, head, view};
use super::block_on;
use super::replica::{ANCHOR_DEVICE_ID, Replica, device_admin, device_id, enroll_entry, person};
use crate::{RecordClass, RecordCursor, RecordEnvelope, RecordRole};

const PAGE_BYTES: usize = 600 * 1024;
const PAGE_ENVELOPES: usize = 256;
const MAX_ROUNDS: usize = 64;

/// A hub anchor that sequenced admins 20, 21, and 22.
async fn anchor_with_admins(seed: u64) -> Replica {
    let mut hub = Replica::anchor(0, seed).await;
    for index in [20, 21, 22] {
        hub.sequence(admin_entry(index)).await.unwrap().unwrap();
    }
    hub
}

/// Every envelope `from` serves, paging until it reports nothing more.
async fn all_pages(from: &mut Replica, max_envelopes: usize) -> (Vec<RecordEnvelope>, usize) {
    let mut cursor = RecordCursor::default();
    let mut envelopes = Vec::new();
    let mut pages = 0;
    for _ in 0..MAX_ROUNDS {
        let page = from
            .engine
            .record_page(
                &mut from.conn,
                &from.ctx,
                &cursor,
                PAGE_BYTES,
                max_envelopes,
            )
            .await
            .unwrap();
        pages += 1;
        cursor = page.cursor;
        envelopes.extend(page.envelopes);
        if !page.more {
            return (envelopes, pages);
        }
    }
    panic!("paging did not end");
}

async fn admit(into: &mut Replica, envelopes: &[RecordEnvelope]) -> crate::RecordAdmission {
    let bytes: Vec<Vec<u8>> = envelopes
        .iter()
        .map(|envelope| envelope.bytes.clone())
        .collect();
    let now_ms = into.now_ms;
    into.engine
        .admit_records(&mut into.conn, &into.ctx, &bytes, now_ms)
        .await
        .unwrap()
}

#[test]
fn pages_carry_the_whole_store_once_and_select_the_same_head_without_operations() {
    block_on(async {
        let mut hub = anchor_with_admins(80).await;
        let mut phone = Replica::new(30, 80).await;
        let (envelopes, pages) = all_pages(&mut hub, 1).await;
        assert!(pages > 1, "one envelope per page takes several pages");
        assert!(
            envelopes
                .iter()
                .any(|envelope| envelope.role == RecordRole::Record)
        );
        assert!(
            envelopes
                .iter()
                .any(|envelope| envelope.role == RecordRole::Certificate)
        );
        let (again, _) = all_pages(&mut hub, PAGE_ENVELOPES).await;
        assert_eq!(again.len(), envelopes.len());

        let admission = admit(&mut phone, &envelopes).await;
        // The shared vault image already holds the charter's envelopes; the admins' are new.
        assert!(admission.admitted >= 3, "{admission:?}");
        assert!(admission.missing.is_empty());
        let (held, _) = all_pages(&mut phone, PAGE_ENVELOPES).await;
        assert_eq!(held.len(), envelopes.len());
        phone.apply_all().await.unwrap();
        assert_eq!(view(&mut phone).await, view(&mut hub).await);
        // A second delivery stores nothing new.
        assert_eq!(admit(&mut phone, &envelopes).await.admitted, 0);
    });
}

#[test]
fn a_page_past_the_cursor_holds_only_what_was_stored_since() {
    block_on(async {
        let mut hub = Replica::anchor(0, 81).await;
        hub.sequence(admin_entry(20)).await.unwrap().unwrap();
        let mut cursor = RecordCursor::default();
        let first = hub
            .engine
            .record_page(&mut hub.conn, &hub.ctx, &cursor, PAGE_BYTES, PAGE_ENVELOPES)
            .await
            .unwrap();
        assert!(!first.more);
        cursor = first.cursor;
        hub.sequence(admin_entry(21)).await.unwrap().unwrap();
        let next = hub
            .engine
            .record_page(&mut hub.conn, &hub.ctx, &cursor, PAGE_BYTES, PAGE_ENVELOPES)
            .await
            .unwrap();
        assert_eq!(next.envelopes.len(), 1);
        assert_eq!(next.envelopes[0].role, RecordRole::Record);
        assert!(
            first
                .envelopes
                .iter()
                .all(|envelope| envelope.bytes != next.envelopes[0].bytes)
        );
    });
}

#[test]
fn a_record_without_its_parent_wants_the_parent_and_certificate_until_the_chain_closes() {
    block_on(async {
        let mut hub = anchor_with_admins(82).await;
        let mut phone = Replica::new(31, 82).await;
        let (envelopes, _) = all_pages(&mut hub, PAGE_ENVELOPES).await;
        let last = envelopes
            .iter()
            .rev()
            .find(|envelope| envelope.role == RecordRole::Record)
            .expect("a record")
            .clone();
        let mut missing = admit(&mut phone, std::slice::from_ref(&last)).await.missing;
        assert!(
            !missing.is_empty(),
            "the newest record alone lacks its ancestry"
        );
        let mut rounds = 0;
        while !missing.is_empty() {
            rounds += 1;
            assert!(
                rounds <= MAX_ROUNDS,
                "wanted lookups did not close the chain"
            );
            let mut found = hub
                .engine
                .records_by_hash(&mut hub.conn, &hub.ctx, &missing.hashes)
                .await
                .unwrap();
            found.extend(
                hub.engine
                    .certificates_of(&mut hub.conn, &hub.ctx, &missing.writers)
                    .await
                    .unwrap(),
            );
            assert!(
                found
                    .iter()
                    .all(|envelope| envelope.class == RecordClass::Wanted)
            );
            assert!(!found.is_empty(), "the hub holds every wanted entry");
            missing = admit(&mut phone, &found).await.missing;
        }
        assert!(rounds > 1, "the chain closes one parent at a time");
        let wants = phone
            .engine
            .record_wants(&mut phone.conn, &phone.ctx, PAGE_ENVELOPES)
            .await
            .unwrap();
        assert!(wants.is_empty());
        phone.apply_all().await.unwrap();
        assert_eq!(view(&mut phone).await, view(&mut hub).await);
    });
}

#[test]
fn storage_counts_bytes_by_certified_device_and_names_no_fork_in_an_honest_store() {
    block_on(async {
        let mut hub = anchor_with_admins(83).await;
        let storage = hub
            .engine
            .authorization_storage(&mut hub.conn, &hub.ctx)
            .await
            .unwrap();
        let (envelopes, _) = all_pages(&mut hub, PAGE_ENVELOPES).await;
        assert_eq!(storage.envelopes, envelopes.len() as u64);
        assert_eq!(
            storage.bytes,
            envelopes
                .iter()
                .map(|envelope| envelope.bytes.len() as u64)
                .sum::<u64>()
        );
        let anchor = storage
            .by_device
            .iter()
            .find(|(device, _)| device == ANCHOR_DEVICE_ID)
            .expect("the anchor carries its records");
        assert!(anchor.1 > 0 && anchor.1 <= storage.bytes);
        assert!(storage.forked_writers.is_empty());
    });
}

/// Brings `into` every record `from` stores by record exchange alone, chasing what the pages
/// leave missing through wanted lookups, then applies what `into` stores.
async fn exchange(into: &mut Replica, from: &mut Replica) {
    let (envelopes, _) = all_pages(from, PAGE_ENVELOPES).await;
    let mut missing = admit(into, &envelopes).await.missing;
    for _ in 0..MAX_ROUNDS {
        if missing.is_empty() {
            break;
        }
        let mut found = from
            .engine
            .records_by_hash(&mut from.conn, &from.ctx, &missing.hashes)
            .await
            .unwrap();
        found.extend(
            from.engine
                .certificates_of(&mut from.conn, &from.ctx, &missing.writers)
                .await
                .unwrap(),
        );
        if found.is_empty() {
            break;
        }
        missing = admit(into, &found).await.missing;
    }
    into.apply_all().await.unwrap();
}

/// The envelope bytes `replica` stores, sorted.
async fn stored(replica: &mut Replica) -> Vec<Vec<u8>> {
    let (envelopes, _) = all_pages(replica, PAGE_ENVELOPES).await;
    let mut bytes: Vec<Vec<u8>> = envelopes
        .into_iter()
        .map(|envelope| envelope.bytes)
        .collect();
    bytes.sort();
    bytes.dedup();
    bytes
}

async fn take(replica: &mut Replica, index: usize) -> Digest32 {
    let signed = replica.handover_proposal(&device_admin(index)).await;
    replica.seal_handover(&signed, None).await.unwrap().unwrap();
    head(replica).await.2
}

/// R7.6 test 18 with every lane withheld: two admins take the anchor, the contest freezes, and
/// one contestant's yield resolves it on the hub, the other contestant, and a member that see
/// only record pages.
#[test]
fn two_takes_resolve_by_a_yield_that_travels_by_record_pages_alone() {
    block_on(async {
        let mut hub = anchor_with_admins(84).await;
        hub.sequence(enroll_entry(&person(), 30, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        let mut first = Replica::new(20, 84).await;
        let mut second = Replica::new(21, 84).await;
        let mut member = Replica::new(30, 84).await;
        for replica in [&mut first, &mut second, &mut member] {
            exchange(replica, &mut hub).await;
        }
        let point = view(&mut hub).await.revision;
        let to_first = take(&mut first, 20).await;
        take(&mut second, 21).await;
        exchange(&mut hub, &mut first).await;
        exchange(&mut hub, &mut second).await;
        assert_eq!(view(&mut hub).await.frozen_at, Some(point));
        exchange(&mut second, &mut hub).await;
        exchange(&mut member, &mut hub).await;
        assert_eq!(view(&mut member).await.frozen_at, Some(point));

        second
            .seal_yield(&device_admin(21), &to_first)
            .await
            .unwrap()
            .unwrap();
        exchange(&mut hub, &mut second).await;
        let resolved = view(&mut hub).await;
        assert_eq!(
            (resolved.anchor_device_id.as_str(), resolved.frozen_at),
            (device_id(20).as_str(), None)
        );
        for replica in [&mut first, &mut member] {
            exchange(replica, &mut hub).await;
            assert_eq!(view(replica).await, resolved);
        }
        assert_eq!(stored(&mut member).await, stored(&mut hub).await);
    });
}

/// Entries the Q2 sequence chains past the hub's admins. The design's figure is 100,000; each
/// entry here is sequenced and rederived on the hub in a debug build, so the chain is shorter.
const Q2_CHAIN: usize = 2_000;

/// R7.6 test 30 (Q2): one replica receives the anchor's chain ascending and the other
/// descending, with a take between; both store the same envelopes, want nothing, and select
/// the same head.
#[test]
fn a_chain_received_ascending_or_descending_beside_a_take_stores_and_selects_the_same() {
    block_on(async {
        let mut hub = anchor_with_admins(85).await;
        hub.sequence(enroll_entry(&person(), 30, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        let mut first = Replica::new(20, 85).await;
        exchange(&mut first, &mut hub).await;
        for index in 0..Q2_CHAIN {
            let role = if index % 2 == 0 {
                DeviceRole::Admin(device_admin(30).public_key())
            } else {
                DeviceRole::Member
            };
            hub.sequence(Entry::SetRole(SetRole::Device {
                device_id: device_id(30),
                role,
            }))
            .await
            .unwrap()
            .unwrap();
        }
        take(&mut first, 20).await;
        let (chain, _) = all_pages(&mut hub, PAGE_ENVELOPES).await;
        let chain_set: Vec<Vec<u8>> = chain
            .iter()
            .map(|envelope| envelope.bytes.clone())
            .collect();
        let (taken, _) = all_pages(&mut first, PAGE_ENVELOPES).await;
        let taken: Vec<RecordEnvelope> = taken
            .into_iter()
            .filter(|envelope| !chain_set.contains(&envelope.bytes))
            .collect();
        assert!(!taken.is_empty());

        let mut ascending = Replica::new(31, 85).await;
        let mut descending = Replica::new(32, 85).await;
        let half = chain.len() / 2;
        for (replica, reverse) in [(&mut ascending, false), (&mut descending, true)] {
            let mut order = chain.clone();
            if reverse {
                order.reverse();
            }
            order.splice(half..half, taken.iter().cloned());
            for page in order.chunks(16) {
                admit(replica, page).await;
            }
        }
        exchange(&mut ascending, &mut descending).await;
        exchange(&mut descending, &mut ascending).await;
        for replica in [&mut ascending, &mut descending] {
            let wants = replica
                .engine
                .record_wants(&mut replica.conn, &replica.ctx, PAGE_ENVELOPES)
                .await
                .unwrap();
            assert!(wants.is_empty(), "{wants:?}");
        }
        assert_eq!(stored(&mut ascending).await, stored(&mut descending).await);
        let selected = view(&mut ascending).await;
        assert_eq!(selected, view(&mut descending).await);
        assert_eq!(selected.anchor_device_id, device_id(20));
    });
}
