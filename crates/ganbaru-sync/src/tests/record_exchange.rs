//! Record exchange over the authorization record store: pages by class with a cursor the
//! requester keeps, wanted lookups that chase missing parents and certificates, and storage
//! accounting for the backstop and growth alerts.

use super::authorization::{admin_entry, view};
use super::block_on;
use super::replica::{ANCHOR_DEVICE_ID, Replica};
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
