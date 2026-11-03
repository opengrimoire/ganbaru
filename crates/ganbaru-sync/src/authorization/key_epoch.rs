//! How a key epoch record an untrusted source such as a relay serves stands against the stored
//! authorization records.
//!
//! The record is judged by the same rules the walk applies: it stands only where a stored
//! check-valid chain admits it, so records a device signed for itself, or admin roles no chain
//! grants, never make it authentic. A record is forged only when the rules refuse it, or an
//! ancestor of it, at a parent the walk reached; a record whose chain the store does not reach
//! may continue a rotation this replica has not stored yet.

use std::collections::BTreeMap;
use std::sync::Arc;

use ganbaru_sync_contracts::{
    AuthorizationRecord, Content, Digest32, Entry, Envelope, OperationKind, SpaceId,
};
use sqlx::SqliteConnection;

use super::{AuthState, Stored, Walk};
use crate::error::SyncResult;

/// How a key epoch record an untrusted source serves stands against the stored records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEpochStanding {
    /// A stored check-valid chain admits the record, so a real rotation stands behind it.
    Authentic,
    /// The record is provably not a key epoch record of this space and epoch.
    Forged,
    /// The store lacks the chain the record continues, so this replica may be lagging.
    Undecided,
}

/// How `bytes`, a key epoch record some untrusted source serves for `epoch`, stands against the
/// authorization records `space` stores, see [`KeyEpochStanding`].
///
/// The walk runs without vetoes, its most permissive form, so no stored contest makes a real
/// rotation look forged.
pub(crate) async fn key_epoch_standing(
    conn: &mut SqliteConnection,
    space: SpaceId,
    bytes: &[u8],
    epoch: u32,
) -> SyncResult<KeyEpochStanding> {
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(KeyEpochStanding::Forged);
    };
    let Some(record) = key_epoch_record(bytes, space, &stored.charter.hash(), epoch) else {
        return Ok(KeyEpochStanding::Forged);
    };
    let walk = Walk::new(&stored, Arc::default());
    let visits: BTreeMap<Digest32, usize> = walk
        .visits
        .iter()
        .enumerate()
        .map(|(index, visit)| (visit.hash, index))
        .collect();
    if let Some(&visit) = visits.get(&record.previous_entry_hash) {
        return Ok(match walk.state_at(visit).check(&record) {
            Ok(()) => KeyEpochStanding::Authentic,
            Err(_) => KeyEpochStanding::Forged,
        });
    }
    // The parent is a stored entry the walk did not reach: find its nearest ancestor that
    // continues a reached entry, and judge that one, since everything below it stands or falls
    // with it.
    let nodes: BTreeMap<&Digest32, usize> = stored
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (&node.hash, index))
        .collect();
    let mut at = record.previous_entry_hash;
    for _ in 0..stored.nodes.len() {
        let Some(&index) = nodes.get(&at) else {
            break;
        };
        let ancestor = &stored.nodes[index].record;
        if let Some(&visit) = visits.get(&ancestor.previous_entry_hash) {
            return Ok(match walk.state_at(visit).check(ancestor) {
                Ok(()) => KeyEpochStanding::Undecided,
                Err(_) => KeyEpochStanding::Forged,
            });
        }
        at = ancestor.previous_entry_hash;
    }
    Ok(KeyEpochStanding::Undecided)
}

/// The record of `bytes` when it is a kind 4 envelope of `space` whose record opens key epoch
/// `epoch` under `charter` and is signed by a device key.
fn key_epoch_record(
    bytes: &[u8],
    space: SpaceId,
    charter: &Digest32,
    epoch: u32,
) -> Option<AuthorizationRecord> {
    let envelope = Envelope::decode(bytes).ok()?;
    if envelope.space != space
        || OperationKind::from_u8(envelope.kind) != Some(OperationKind::Authorization)
    {
        return None;
    }
    let Content::Authorization(record) = envelope.operation().ok()?.content else {
        return None;
    };
    let proposal = &record.proposal.proposal;
    let Entry::KeyEpoch(key_epoch) = &proposal.entry else {
        return None;
    };
    let fits = key_epoch.key_epoch == epoch
        && proposal.space == space
        && proposal.charter == *charter
        && proposal.device_author().is_some();
    fits.then_some(*record)
}

impl Walk<'_> {
    /// The state after `visit`, replayed from the charter along its chain.
    fn state_at(&self, visit: usize) -> AuthState {
        let mut path = Vec::new();
        let mut at = visit;
        while at != 0 {
            path.push(at);
            at = self.parent(at);
        }
        let mut state = AuthState::from_charter(&self.stored.charter, Arc::clone(&self.vetoes));
        for &step in path.iter().rev() {
            let visit = &self.visits[step];
            if let Some(node) = visit.node {
                state.advance(&self.stored.nodes[node].record, visit.hash);
            }
        }
        state
    }
}
