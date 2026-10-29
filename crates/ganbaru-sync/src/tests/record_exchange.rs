//! Record exchange over the authorization record store: pages by class with a cursor the
//! requester keeps, wanted lookups that chase missing parents and certificates, and storage
//! accounting for the backstop and growth alerts.

use ganbaru_sync_contracts::{
    AuthorizationRecord, Content, DeviceKeyPair, DeviceRole, Digest32, Entry, Envelope, Sequencing,
    SetRole, WriterId, WriterKeyPair, sign_certificate,
};

use super::arrival_orders;
use super::authorization::{admin_entry, genesis_of, head, remove_entry, seal_after, view};
use super::block_on;
use super::replica::{
    ANCHOR_DEVICE_ID, Identity, Replica, anchor_admin, anchor_device_key, device_admin, device_id,
    enroll_entry, person, proposal,
};
use crate::{AuthorizationView, RecordClass, RecordCursor, RecordEnvelope, RecordRole};

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

/// Every envelope the record store holds, with its role and whether it is verified, sorted.
async fn store_set(replica: &mut Replica) -> Vec<(String, Vec<u8>, i64)> {
    sqlx::query_as(
        "SELECT role, envelope_hash, verified FROM sync_auth_envelopes
         ORDER BY role, envelope_hash",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
}

/// The stored genesis certificate envelope of `writer`.
async fn genesis_of_writer(replica: &mut Replica, writer: WriterId) -> Vec<u8> {
    sqlx::query_scalar(
        "SELECT envelope FROM sync_auth_envelopes WHERE role = 'certificate' AND writer_id = ?",
    )
    .bind(writer.as_bytes().as_slice())
    .fetch_one(&mut replica.conn)
    .await
    .unwrap()
}

/// A second genesis of the writer `key` signs, re-sealed from `genesis` with a certificate that
/// names the same device and device key and a later creation time, as a reinstalled copy of the
/// installation would seal.
fn resealed_genesis(genesis: &[u8], key: &WriterKeyPair, device: &DeviceKeyPair) -> Vec<u8> {
    let mut operation = Envelope::decode(genesis).unwrap().operation().unwrap();
    let Content::Genesis(signed) = &operation.content else {
        panic!("a genesis operation");
    };
    let mut certificate = signed.certificate.clone();
    certificate.created_at_ms += 1;
    operation.content = Content::Genesis(sign_certificate(&certificate, device).unwrap());
    operation.seal(key).unwrap().into_bytes()
}

/// What a fresh member selects and stores once it admits `envelopes` one at a time in each
/// arrival order of [`arrival_orders`]. Asserts that every order derives the same view, head,
/// and stored set, and returns them.
async fn admitted_in_every_order(
    envelopes: &[Vec<u8>],
    salt: u64,
) -> (AuthorizationView, Digest32, Vec<(String, Vec<u8>, i64)>) {
    let mut agreed = None;
    for order in arrival_orders(envelopes.len()) {
        let mut member = Replica::new(31, salt).await;
        for index in &order {
            let now_ms = member.now_ms;
            member
                .engine
                .admit_records(
                    &mut member.conn,
                    &member.ctx,
                    std::slice::from_ref(&envelopes[*index]),
                    now_ms,
                )
                .await
                .unwrap();
        }
        member.apply_all().await.unwrap();
        let derived = (
            view(&mut member).await,
            head(&mut member).await.2,
            store_set(&mut member).await,
        );
        let first = agreed.get_or_insert_with(|| derived.clone());
        assert_eq!(*first, derived, "arrival order {order:?}");
    }
    agreed.unwrap()
}

/// The anchor with admin 20 enrolled, and admin 20's replica holding its log.
async fn anchor_and_admin(salt: u64) -> (Replica, Replica) {
    let mut anchor = Replica::anchor(0, salt).await;
    anchor.sequence(admin_entry(20)).await.unwrap().unwrap();
    let mut admin = Replica::new(20, salt).await;
    admin.pull(&mut anchor).await;
    (anchor, admin)
}

/// Every envelope `replica` serves on record pages, as bytes.
async fn page_bytes(replica: &mut Replica) -> Vec<Vec<u8>> {
    let (envelopes, _) = all_pages(replica, PAGE_ENVELOPES).await;
    envelopes
        .into_iter()
        .map(|envelope| envelope.bytes)
        .collect()
}

/// R7.6 test 47: the founder, removed by a take, re-seals a second genesis certificate for its
/// old writer and seals a branch from the charter under a new writer certified by its device
/// key. Its old writer stays bound, so the take and the removal keep their authority; head the
/// removal, unfrozen, in every arrival order.
#[test]
fn a_founders_second_genesis_after_its_removal_keeps_the_take_and_the_removal_selected() {
    block_on(async {
        let salt = 86;
        let (mut founder, mut admin) = anchor_and_admin(salt).await;
        let taken = take(&mut admin, 20).await;
        let signed = proposal(
            admin.ctx.space,
            &device_admin(20),
            admin.now_ms,
            remove_entry(ANCHOR_DEVICE_ID),
        );
        admin.sequence_signed(&signed).await.unwrap().unwrap();
        let removal = head(&mut admin).await.2;
        let mut envelopes = page_bytes(&mut admin).await;

        let device = anchor_device_key(&person());
        let founder_writer = founder.writer();
        let genesis = genesis_of_writer(&mut founder, founder_writer).await;
        envelopes.push(resealed_genesis(&genesis, &founder.identity.key, &device));
        let fresh = Identity::new([86; 32], &device, ANCHOR_DEVICE_ID, None);
        let mut previous = genesis_of(&genesis, &fresh);
        envelopes.push(previous.clone());
        let charter = founder
            .engine
            .charter(&mut founder.conn, &founder.ctx)
            .await
            .unwrap()
            .unwrap();
        let mut parent = charter.hash();
        for (revision, index) in [(1, 22), (2, 23), (3, 24)] {
            let record = AuthorizationRecord {
                epoch: 0,
                revision,
                previous_entry_hash: parent,
                anchor_device_id: ANCHOR_DEVICE_ID.to_owned(),
                proposal: proposal(
                    founder.ctx.space,
                    &anchor_admin(&person()),
                    founder.now_ms + revision,
                    admin_entry(index),
                ),
                sequencing: Sequencing::Empty,
            };
            parent = record.hash();
            previous = seal_after(
                &previous,
                &fresh.key,
                Content::Authorization(Box::new(record)),
                revision - 1,
                founder.now_ms,
            );
            envelopes.push(previous.clone());
        }

        let (selected, selected_head, stored) = admitted_in_every_order(&envelopes, salt).await;
        assert_eq!(selected_head, removal);
        assert_ne!(selected_head, taken);
        assert_eq!(selected.anchor_device_id, device_id(20));
        assert_eq!(selected.frozen_at, None);
        assert!(selected.devices.iter().any(
            |device| device.device_id == ANCHOR_DEVICE_ID && device.removed_revision.is_some()
        ));
        assert_eq!(selected, view(&mut admin).await);
        assert!(!stored.is_empty());
        let mut member = Replica::new(31, salt).await;
        admit_bytes(&mut member, &envelopes).await;
        let founder_certificates: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sync_auth_envelopes WHERE role = 'certificate' AND writer_id = ?",
        )
        .bind(founder_writer.as_bytes().as_slice())
        .fetch_one(&mut member.conn)
        .await
        .unwrap();
        assert_eq!(founder_certificates, 2);
    });
}

/// R7.6 test 48: an honest installation re-seals its genesis, and a member's take and later
/// records descend from its entries. Nothing loses its authority: every arrival order selects
/// the head and view of the run without the second genesis, unfrozen, and the twin shows as a
/// forked writer.
#[test]
fn an_honest_second_genesis_changes_no_selection() {
    block_on(async {
        let salt = 87;
        let (mut anchor, mut admin) = anchor_and_admin(salt).await;
        take(&mut admin, 20).await;
        let signed = proposal(
            admin.ctx.space,
            &device_admin(20),
            admin.now_ms,
            enroll_entry(&person(), 21, DeviceRole::Member),
        );
        admin.sequence_signed(&signed).await.unwrap().unwrap();
        let later = head(&mut admin).await.2;
        let single = page_bytes(&mut admin).await;
        let anchor_writer = anchor.writer();
        let genesis = genesis_of_writer(&mut anchor, anchor_writer).await;
        let mut twinned = single.clone();
        twinned.push(resealed_genesis(
            &genesis,
            &anchor.identity.key,
            &anchor_device_key(&person()),
        ));

        let (single_view, single_head, _) = admitted_in_every_order(&single, salt).await;
        let (twinned_view, twinned_head, _) = admitted_in_every_order(&twinned, salt).await;
        assert_eq!(single_head, later);
        assert_eq!(twinned_head, single_head);
        assert_eq!(twinned_view, single_view);
        assert_eq!(twinned_view.frozen_at, None);

        let mut member = Replica::new(31, salt).await;
        admit_bytes(&mut member, &twinned).await;
        member.apply_all().await.unwrap();
        let storage = member
            .engine
            .authorization_storage(&mut member.conn, &member.ctx)
            .await
            .unwrap();
        assert!(
            storage
                .forked_writers
                .iter()
                .any(|forked| forked.writer == anchor.writer())
        );
        assert!(member.evidence().await.is_empty());
    });
}

async fn admit_bytes(into: &mut Replica, envelopes: &[Vec<u8>]) -> crate::RecordAdmission {
    let now_ms = into.now_ms;
    into.engine
        .admit_records(&mut into.conn, &into.ctx, envelopes, now_ms)
        .await
        .unwrap()
}

/// R7.6 test 49: a record of a writer with a bad writer-key signature, stored as pending before
/// the writer's certificate on one replica and offered after it on another, is purged on the
/// first and refused on the second, so both hold the same set, in every arrival order.
#[test]
fn a_pending_record_with_a_bad_signature_is_purged_when_its_certificate_arrives() {
    block_on(async {
        let salt = 88;
        let (_, mut admin) = anchor_and_admin(salt).await;
        take(&mut admin, 20).await;
        let honest = page_bytes(&mut admin).await;
        let admin_writer = admin.writer();
        let certificate = genesis_of_writer(&mut admin, admin_writer).await;
        let (epoch, revision, parent) = head(&mut admin).await;
        let record = AuthorizationRecord {
            epoch,
            revision: revision + 1,
            previous_entry_hash: parent,
            anchor_device_id: device_id(20),
            proposal: proposal(
                admin.ctx.space,
                &device_admin(20),
                admin.now_ms,
                enroll_entry(&person(), 21, DeviceRole::Member),
            ),
            sequencing: Sequencing::Empty,
        };
        let last = admin.own_ops().await.pop().unwrap();
        let mut bad = seal_after(
            &last,
            &admin.identity.key,
            Content::Authorization(Box::new(record)),
            revision,
            admin.now_ms,
        );
        *bad.last_mut().unwrap() ^= 1;
        let bad_hash = Envelope::decode(&bad).unwrap().hash();

        // Replica A stores the record pending, then purges it with the certificate; replica B
        // refuses it once the certificate is stored.
        let mut first = Replica::new(31, salt).await;
        admit_bytes(&mut first, std::slice::from_ref(&bad)).await;
        assert!(
            store_set(&mut first)
                .await
                .iter()
                .any(|(_, hash, verified)| *hash == bad_hash.to_vec() && *verified == 0)
        );
        admit_bytes(&mut first, std::slice::from_ref(&certificate)).await;
        let mut second = Replica::new(32, salt).await;
        admit_bytes(&mut second, std::slice::from_ref(&certificate)).await;
        admit_bytes(&mut second, std::slice::from_ref(&bad)).await;
        let first_set = store_set(&mut first).await;
        assert_eq!(first_set, store_set(&mut second).await);
        assert!(
            first_set
                .iter()
                .all(|(_, hash, _)| *hash != bad_hash.to_vec())
        );

        let mut envelopes = honest;
        envelopes.push(bad);
        let (_, _, stored) = admitted_in_every_order(&envelopes, salt).await;
        assert!(
            stored
                .iter()
                .all(|(_, hash, verified)| *hash != bad_hash.to_vec() && *verified == 1)
        );
    });
}

/// R7.6 tests 21, 25, 26, and 38 at the store: the anchor's writer seals three twins at one
/// sequence above a shared prefix, and an admin's take descends from below them. Every arrival
/// order, including twins before their prefix, stores every twin and agrees: frozen at the last
/// record below the twins without the take, and the take selected and unfrozen with it.
#[test]
fn twins_above_a_take_store_alike_and_freeze_or_select_the_same_in_every_order() {
    block_on(async {
        let salt = 89;
        let (mut anchor, mut admin) = anchor_and_admin(salt).await;
        let (_, below_revision, below_hash) = head(&mut anchor).await;
        let below = anchor.own_ops().await.pop().unwrap();
        let taken = take(&mut admin, 20).await;
        for index in [21, 22] {
            anchor
                .sequence(enroll_entry(&person(), index, DeviceRole::Member))
                .await
                .unwrap()
                .unwrap();
        }
        let twins: Vec<Vec<u8>> = [23_usize, 24]
            .into_iter()
            .map(|index| {
                let record = AuthorizationRecord {
                    epoch: 0,
                    revision: below_revision + 1,
                    previous_entry_hash: below_hash,
                    anchor_device_id: ANCHOR_DEVICE_ID.to_owned(),
                    proposal: proposal(
                        anchor.ctx.space,
                        &anchor_admin(&person()),
                        anchor.now_ms + u64::try_from(index).unwrap(),
                        enroll_entry(&person(), index, DeviceRole::Member),
                    ),
                    sequencing: Sequencing::Empty,
                };
                seal_after(
                    &below,
                    &anchor.identity.key,
                    Content::Authorization(Box::new(record)),
                    below_revision,
                    anchor.now_ms,
                )
            })
            .collect();
        let twin_hashes: Vec<Vec<u8>> = twins
            .iter()
            .map(|bytes| Envelope::decode(bytes).unwrap().hash().to_vec())
            .collect();

        let mut without_take = page_bytes(&mut anchor).await;
        without_take.extend(twins.iter().cloned());
        let (frozen, _, stored) = admitted_in_every_order(&without_take, salt).await;
        assert_eq!(frozen.frozen_at, Some(below_revision));
        assert!(
            twin_hashes
                .iter()
                .all(|hash| stored.iter().any(|(_, stored, _)| stored == hash))
        );
        for index in [21, 22, 23, 24] {
            assert!(
                !frozen
                    .devices
                    .iter()
                    .any(|device| device.device_id == device_id(index))
            );
        }

        let mut with_take = without_take.clone();
        for bytes in page_bytes(&mut admin).await {
            if !with_take.contains(&bytes) {
                with_take.push(bytes);
            }
        }
        let (selected, selected_head, stored) = admitted_in_every_order(&with_take, salt).await;
        assert_eq!(selected_head, taken);
        assert_eq!(selected.anchor_device_id, device_id(20));
        assert_eq!(selected.frozen_at, None);
        assert!(
            twin_hashes
                .iter()
                .all(|hash| stored.iter().any(|(_, stored, _)| stored == hash))
        );

        let one_twin: Vec<Vec<u8>> = with_take
            .iter()
            .filter(|bytes| Envelope::decode(bytes).unwrap().hash().to_vec() != twin_hashes[1])
            .cloned()
            .collect();
        let (fewer, fewer_head, _) = admitted_in_every_order(&one_twin, salt).await;
        assert_eq!(fewer_head, selected_head);
        assert_eq!(fewer, selected);
    });
}
