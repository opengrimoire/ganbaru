//! Which members one sync pass exchanges with, and which of them holds the long poll.
//!
//! A pass prefers the anchor when it is reachable, then members discovered on the LAN, then
//! members the peer book knows, and never runs more exchanges at once than the device kind
//! allows. Only enrolled, not removed, not quarantined members are ever planned.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::DeviceKind;

use super::book::PeerEndpoints;

/// Most exchanges a desktop runs at once in one pass.
pub const DESKTOP_MAX_EXCHANGES: usize = 3;

/// Most exchanges a phone runs at once in one pass.
pub const MOBILE_MAX_EXCHANGES: usize = 2;

/// The most exchanges a device of `kind` runs at once.
pub fn max_exchanges(kind: DeviceKind) -> usize {
    match kind {
        DeviceKind::Desktop => DESKTOP_MAX_EXCHANGES,
        DeviceKind::Mobile => MOBILE_MAX_EXCHANGES,
        // A server only answers: it never dials a member.
        DeviceKind::Server => 0,
    }
}

/// Where a planned peer's endpoints came from, in the order a pass prefers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PeerSource {
    /// The anchor, at the endpoints its pin holds.
    Anchor,
    /// A member that answered LAN discovery during this pass.
    Discovered,
    /// A member at the endpoints the peer book last saw.
    Known,
}

/// One member a pass exchanges with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedPeer {
    /// Handoff device id of the member.
    pub device_id: String,
    /// Endpoints to dial, in order.
    pub endpoints: Vec<String>,
    /// Why the member was chosen.
    pub source: PeerSource,
}

/// The exchanges of one pass and the one that holds the long poll.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PassPlan {
    /// The members to exchange with, at most the device kind's limit, in preference order.
    pub peers: Vec<PlannedPeer>,
    /// The member whose exchange waits for news, when any: the anchor while it is reachable.
    pub long_poll: Option<String>,
}

/// What a pass knows when it plans.
#[derive(Clone, Debug)]
pub struct PlanInput<'a> {
    /// This device's kind, which bounds the concurrent exchanges.
    pub kind: DeviceKind,
    /// This device's handoff id, never planned.
    pub own_device_id: &'a str,
    /// The devices the selected log enrolled and did not remove, minus quarantined ones.
    pub members: &'a BTreeSet<String>,
    /// The anchor's device id and pinned endpoints, when this device has an anchor pin.
    pub anchor: Option<(&'a str, &'a [String])>,
    /// Whether the anchor answered the previous pass; an unreachable anchor keeps the last
    /// slot so a pass notices it is back.
    pub anchor_reachable: bool,
    /// Members that answered LAN discovery, with their endpoints.
    pub discovered: &'a BTreeMap<String, Vec<String>>,
    /// The peer book.
    pub known: &'a PeerEndpoints,
}

/// Plans one pass.
pub fn plan_pass(input: &PlanInput<'_>) -> PassPlan {
    let limit = max_exchanges(input.kind);
    let mut planned: Vec<PlannedPeer> = Vec::new();
    let mut chosen: BTreeSet<&str> = BTreeSet::new();
    let eligible =
        |device_id: &str| device_id != input.own_device_id && input.members.contains(device_id);

    let anchor = input
        .anchor
        .filter(|(device_id, endpoints)| eligible(device_id) && !endpoints.is_empty());
    let mut late_anchor = None;
    if let Some((device_id, endpoints)) = anchor {
        let peer = PlannedPeer {
            device_id: device_id.to_string(),
            endpoints: endpoints.to_vec(),
            source: PeerSource::Anchor,
        };
        chosen.insert(device_id);
        if input.anchor_reachable {
            planned.push(peer);
        } else {
            late_anchor = Some(peer);
        }
    }

    let reserve = usize::from(late_anchor.is_some());
    for (device_id, endpoints) in input.discovered {
        if planned.len() + reserve >= limit {
            break;
        }
        if endpoints.is_empty() || !eligible(device_id) || !chosen.insert(device_id) {
            continue;
        }
        // Discovery answers are unauthenticated, so a spoofed record only adds endpoints to
        // try first and never hides the ones the book already knows.
        let mut dial = endpoints.clone();
        for known in input.known.endpoints(device_id) {
            if !dial.contains(&known) {
                dial.push(known);
            }
        }
        planned.push(PlannedPeer {
            device_id: device_id.clone(),
            endpoints: dial,
            source: PeerSource::Discovered,
        });
    }

    for member in input.known.report() {
        if planned.len() + reserve >= limit {
            break;
        }
        if !eligible(&member.device_id) || chosen.contains(member.device_id.as_str()) {
            continue;
        }
        let endpoints = input.known.endpoints(&member.device_id);
        if let Some(device_id) = input.members.get(&member.device_id) {
            chosen.insert(device_id);
        }
        planned.push(PlannedPeer {
            device_id: member.device_id,
            endpoints,
            source: PeerSource::Known,
        });
    }

    if let Some(peer) = late_anchor
        && planned.len() < limit
    {
        planned.push(peer);
    }

    let long_poll = planned
        .iter()
        .find(|peer| peer.source == PeerSource::Anchor && input.anchor_reachable)
        .or_else(|| {
            planned
                .iter()
                .find(|peer| peer.source != PeerSource::Anchor)
        })
        .map(|peer| peer.device_id.clone());
    PassPlan {
        peers: planned,
        long_poll,
    }
}
