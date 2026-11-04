//! Seeded simulation of delivery through a relay: installations exchange operations only as
//! sealed relay records, one per writer slot, which the relay hands out with drops, duplicates,
//! and reordering, and each installation opens with the epoch keys it holds. Key epoch entries
//! reach the relay beside the records, and the relay takes a record only under its current
//! epoch, as only the relay access key of that epoch may write.
//!
//! The anchor removes one member, which seals a new key epoch. A member that has not yet opened
//! its envelope of that epoch refuses the newer records until it does, and the removed member
//! opens none of the records sealed after its removal, so nothing written after it reaches its
//! store.

use std::collections::BTreeMap;
use std::sync::Arc;

use ganbaru_sync_contracts::{
    Content, EncryptionKeyPair, Entry, Envelope, KeyEpoch, RelayError, RelayRecord, Seq,
    SignedCertificate, VersionVector, WriterId,
};

use super::super::block_on;
use super::super::replica::{Replica, anchor_device_key, device_id, device_key, person};
use super::super::space_keys::{
    TestKeys, keyed_anchor, publish_member, start_epochs, stored_keys, synced_member,
};
use super::{Rng, SeedGuard};
use crate::{RelayKeys, SpaceKeyHolder};

const MEMBERS: usize = 3;
const LAGGING: usize = 2;
const REMOVED: usize = 3;
const SEEDS: u64 = 4;
const LOSSY_ROUNDS: usize = 6;
const DROP_PERCENT: u64 = 20;
const DUPLICATE_PERCENT: u64 = 10;
const NOTES: &str = "SELECT count(*) FROM quick_notes";

/// One installation with the device encryption key that opens its envelopes and the epoch keys
/// it opened.
struct Installation {
    replica: Replica,
    keys: Arc<TestKeys>,
    epochs: RelayKeys,
    notes: usize,
}

impl Installation {
    async fn new(mut replica: Replica, keys: Arc<TestKeys>) -> Self {
        let stored = stored_keys(&mut replica).await;
        let epochs = RelayKeys::open(replica.ctx.space, &stored, keys.as_ref());
        Self {
            replica,
            keys,
            epochs,
            notes: 0,
        }
    }

    /// Opens this installation's envelope of every key epoch entry the relay carries that it
    /// does not hold yet.
    fn refresh(&mut self, relay: &Relay) {
        let space = self.replica.ctx.space;
        let own = self.keys.encryption_key();
        for entry in &relay.epochs {
            if self.epochs.key(entry.key_epoch).is_some() {
                continue;
            }
            if let Some(key) = entry
                .envelope_for(&own)
                .and_then(|envelope| self.keys.open(&space, entry.key_epoch, envelope))
            {
                self.epochs.insert(key);
            }
        }
    }

    /// Writes one Quick note and seals it.
    async fn edit(&mut self) {
        self.notes += 1;
        let sql = format!(
            "INSERT INTO quick_notes (id, order_key) VALUES ('relay-{}-{}', 'a{}');",
            self.replica.index, self.notes, self.notes
        );
        assert!(self.replica.try_exec(&sql).await);
        self.replica.seal().await.unwrap();
    }
}

/// A blind store of sealed records keyed by writer slot, with the key epoch entries it carries.
#[derive(Default)]
struct Relay {
    records: BTreeMap<(WriterId, Seq), Vec<u8>>,
    epochs: Vec<KeyEpoch>,
}

impl Relay {
    fn current_epoch(&self) -> u32 {
        self.epochs.last().map_or(0, |entry| entry.key_epoch)
    }

    /// Takes every key epoch entry `anchor` sequenced that the relay lacks.
    async fn publish_epochs(&mut self, anchor: &mut Replica) {
        for bytes in anchor.ops_since(&VersionVector::new()).await {
            let Ok(operation) = Envelope::decode(&bytes).unwrap().operation() else {
                continue;
            };
            let Content::Authorization(record) = operation.content else {
                continue;
            };
            if let Entry::KeyEpoch(entry) = record.proposal.proposal.entry
                && entry.key_epoch > self.current_epoch()
            {
                self.epochs.push(entry);
            }
        }
    }

    /// Seals every stored operation of `from` whose slot the relay lacks under the newest epoch
    /// `from` holds. Returns how many records the relay took.
    async fn push(&mut self, from: &mut Installation) -> usize {
        let mut taken = 0;
        for bytes in from.replica.ops_since(&VersionVector::new()).await {
            let envelope = Envelope::decode(&bytes).unwrap();
            let slot = (envelope.writer, envelope.seq);
            if self.records.contains_key(&slot) {
                continue;
            }
            let certificate = certificate(&mut from.replica, envelope.writer).await;
            let record = from.epochs.seal_record(&bytes, &certificate).unwrap();
            if record.header().epoch != self.current_epoch() {
                continue;
            }
            self.records.insert(slot, record.encode());
            taken += 1;
        }
        taken
    }

    /// Hands the records to `to`, dropping, duplicating, and reordering them when `lossy`, and
    /// stores and applies what it opens. Returns each refusal with the epoch of its record.
    async fn pull(
        &self,
        to: &mut Installation,
        rng: &mut Rng,
        lossy: bool,
    ) -> Vec<(u32, RelayError)> {
        let mut delivered: Vec<&[u8]> = Vec::new();
        for bytes in self.records.values() {
            if lossy && rng.chance(DROP_PERCENT) {
                continue;
            }
            delivered.push(bytes);
            if lossy && rng.chance(DUPLICATE_PERCENT) {
                delivered.push(bytes);
            }
        }
        for index in (1..delivered.len()).rev() {
            delivered.swap(index, rng.below(index + 1));
        }
        let mut operations = Vec::new();
        let mut refusals = Vec::new();
        for bytes in delivered {
            match to.epochs.open_record(bytes) {
                Ok(opened) => operations.push(opened.operation),
                Err(error) => {
                    let epoch = RelayRecord::decode(bytes).unwrap().header().epoch;
                    refusals.push((epoch, error));
                }
            }
        }
        to.replica.receive(&operations).await;
        to.replica.apply_all().await.unwrap();
        refusals
    }
}

/// The signed certificate `replica` stores for `writer`.
async fn certificate(replica: &mut Replica, writer: WriterId) -> SignedCertificate {
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT certificate FROM sync_writers WHERE space_id = ? AND writer_id = ?",
    )
    .bind(replica.ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .fetch_one(&mut replica.conn)
    .await
    .unwrap();
    SignedCertificate::decode(&bytes).unwrap()
}

/// Pushes and pulls through the relay for every installation in `active`, lossy at first and
/// then lossless until nothing new reaches the relay. Returns every refusal by installation.
async fn exchange(
    relay: &mut Relay,
    installations: &mut [Installation],
    active: &[usize],
    rng: &mut Rng,
) -> BTreeMap<usize, Vec<(u32, RelayError)>> {
    let mut refusals: BTreeMap<usize, Vec<(u32, RelayError)>> = BTreeMap::new();
    for round in 0.. {
        assert!(round < 64, "relay delivery never settled");
        let lossy = round < LOSSY_ROUNDS;
        let mut taken = 0;
        for &index in active {
            taken += relay.push(&mut installations[index]).await;
        }
        for &index in active {
            let refused = relay.pull(&mut installations[index], rng, lossy).await;
            refusals.entry(index).or_default().extend(refused);
        }
        if !lossy && taken == 0 {
            return refusals;
        }
    }
    unreachable!()
}

async fn notes(installation: &mut Installation) -> i64 {
    installation.replica.count(NOTES).await
}

fn run_seed(seed: u64) {
    let _guard = SeedGuard(seed);
    block_on(async {
        let mut rng = Rng(seed);
        let salt = 40 + seed;
        let anchor_keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(salt, anchor_keys.clone()).await;
        let member_keys: Vec<Arc<TestKeys>> = (1..=MEMBERS)
            .map(|index| TestKeys::new(device_key(&person(), index)))
            .collect();
        for (offset, keys) in member_keys.iter().enumerate() {
            publish_member(&mut anchor, offset + 1, keys).await;
        }
        start_epochs(&mut anchor, &EncryptionKeyPair::generate().unwrap()).await;
        let mut installations = Vec::new();
        for (offset, keys) in member_keys.iter().enumerate() {
            let member = synced_member(offset + 1, salt, &mut anchor).await;
            installations.push((member, keys.clone()));
        }
        let mut installations: Vec<Installation> = {
            let mut all = vec![Installation::new(anchor, anchor_keys).await];
            for (member, keys) in installations {
                all.push(Installation::new(member, keys).await);
            }
            all
        };
        let mut relay = Relay::default();
        relay.publish_epochs(&mut installations[0].replica).await;
        assert_eq!(relay.current_epoch(), 1);
        for installation in &mut installations {
            assert_eq!(installation.epochs.newest_epoch(), Some(1));
        }

        let everyone: Vec<usize> = (0..=MEMBERS).collect();
        for index in &everyone {
            for _ in 0..=rng.below(3) {
                installations[*index].edit().await;
            }
        }
        let refused = exchange(&mut relay, &mut installations, &everyone, &mut rng).await;
        assert!(refused.values().all(Vec::is_empty), "{refused:?}");
        let before_removal = notes(&mut installations[0]).await;
        for installation in &mut installations {
            assert_eq!(notes(installation).await, before_removal);
        }

        let anchor = &mut installations[0];
        anchor.replica.remove(&device_id(REMOVED)).await.unwrap();
        relay.publish_epochs(&mut anchor.replica).await;
        assert_eq!(relay.current_epoch(), 2);
        anchor.refresh(&relay);
        installations[1].refresh(&relay);
        installations[REMOVED].refresh(&relay);
        assert_eq!(installations[0].epochs.newest_epoch(), Some(2));
        assert_eq!(installations[1].epochs.newest_epoch(), Some(2));
        assert_eq!(installations[LAGGING].epochs.newest_epoch(), Some(1));
        assert_eq!(installations[REMOVED].epochs.newest_epoch(), Some(1));

        for index in [0, 1, LAGGING, REMOVED] {
            installations[index].edit().await;
        }
        let removed_before = installations[REMOVED].replica.stored_vector().await;
        let lagging_before = installations[LAGGING].replica.stored_vector().await;
        let refused = exchange(&mut relay, &mut installations, &everyone, &mut rng).await;
        for index in [LAGGING, REMOVED] {
            let refusals = &refused[&index];
            assert!(!refusals.is_empty());
            assert!(
                refusals
                    .iter()
                    .all(|(epoch, error)| *epoch == 2
                        && matches!(error, RelayError::UnknownEpoch(2))),
                "{refusals:?}"
            );
        }
        assert_eq!(
            installations[REMOVED].replica.stored_vector().await,
            removed_before
        );
        assert_eq!(
            installations[LAGGING].replica.stored_vector().await,
            lagging_before
        );

        installations[LAGGING].refresh(&relay);
        installations[REMOVED].refresh(&relay);
        assert_eq!(installations[LAGGING].epochs.newest_epoch(), Some(2));
        assert_eq!(installations[REMOVED].epochs.newest_epoch(), Some(1));
        let kept = [0, 1, LAGGING];
        installations[LAGGING].edit().await;
        let refused = exchange(&mut relay, &mut installations, &everyone, &mut rng).await;
        assert!(
            kept.iter().all(|index| refused[index].is_empty()),
            "{refused:?}"
        );
        let after_removal = notes(&mut installations[0]).await;
        assert_eq!(after_removal, before_removal + 4);
        for index in kept {
            assert_eq!(notes(&mut installations[index]).await, after_removal);
        }
        assert_eq!(
            installations[REMOVED].replica.stored_vector().await,
            removed_before
        );
        assert_eq!(notes(&mut installations[REMOVED]).await, before_removal + 1);

        let sealed_after_removal: Vec<&Vec<u8>> = relay
            .records
            .values()
            .filter(|bytes| RelayRecord::decode(bytes).unwrap().header().epoch >= 2)
            .collect();
        assert!(!sealed_after_removal.is_empty());
        for bytes in sealed_after_removal {
            assert!(matches!(
                installations[REMOVED].epochs.open_record(bytes),
                Err(RelayError::UnknownEpoch(2))
            ));
        }
    });
}

#[test]
fn relay_delivery_opens_records_only_with_held_epoch_keys() {
    for seed in 0..SEEDS {
        run_seed(seed);
    }
}
