//! Admission to a member's sync listener, decided from the local selected authorization log.
//!
//! A listener accepts the TLS certificates of devices the selected log enrolled and this
//! installation did not quarantine. A device the log removed is served record exchange only.
//! A non-anchor listener serves only sync requests, so pairing, other handoff, and contact
//! requests are refused there.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_handoff::protocol::ControlMessage;
use ganbaru_sync::AuthorizationView;

/// Why a member listener turned a connection or request away. Each code names the reason
/// without echoing keys or certificates, so it can be logged and sent as is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberRefusal {
    /// The certificate belongs to no device the selected log enrolled.
    Unknown,
    /// The selected log removed the device.
    Removed,
    /// This installation quarantined the device.
    Quarantined,
    /// The request names a different device than the certificate's enrollment.
    DeviceMismatch,
    /// The request names a vault this listener does not serve.
    VaultMismatch,
    /// The request is not a sync request, which is all a non-anchor listener serves.
    NotSync,
    /// The request proposes an authorization change to a listener that is not the anchor, so
    /// only the anchor's signed answer can be final.
    Proposal,
    /// The request asks for proposals held for the anchor, which only the coordinator holds.
    Forward,
    /// The listener cannot read the selected log or quarantine now, so it admits no one until
    /// it can.
    Unavailable,
}

impl MemberRefusal {
    /// The stable error code a refused request is answered with.
    pub fn code(self) -> &'static str {
        match self {
            Self::Unknown => "member_unknown",
            Self::Removed => "member_removed",
            Self::Quarantined => "member_quarantined",
            Self::DeviceMismatch => "member_device_mismatch",
            Self::VaultMismatch => "vault_mismatch",
            Self::NotSync => "sync_only",
            Self::Proposal => "proposals_to_anchor",
            Self::Forward => "forwards_to_coordinator",
            Self::Unavailable => "admission_unavailable",
        }
    }

    /// A short description for logs and error replies.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Unknown => "the certificate belongs to no enrolled device",
            Self::Removed => "the device was removed from the vault",
            Self::Quarantined => "this device quarantined the peer",
            Self::DeviceMismatch => "the request names another device than its certificate",
            Self::VaultMismatch => "this listener serves a different vault",
            Self::NotSync => "this listener serves only sync requests",
            Self::Proposal => "authorization proposals go to the anchor",
            Self::Forward => "held proposals are collected from the coordinator",
            Self::Unavailable => "this listener cannot read its admission now",
        }
    }
}

/// A device the selected log enrolled, by certificate fingerprint.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Enrolled {
    device_id: String,
    removed: bool,
}

/// The admission decisions of one listener for one vault, built from the selected log and the
/// device-local quarantine. Rebuild it whenever either changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberAdmission {
    vault_id: String,
    by_fingerprint: BTreeMap<String, Enrolled>,
    quarantined: BTreeSet<String>,
    anchor_device_id: String,
    takes_proposals: bool,
    answers_forwards: bool,
}

impl MemberAdmission {
    /// Admission for `vault_id` from its selected authorization state and the devices this
    /// installation quarantined. It takes no proposals until [`Self::served_by`] names the
    /// anchor.
    pub fn new(vault_id: &str, view: &AuthorizationView, quarantined: &BTreeSet<String>) -> Self {
        let by_fingerprint = view
            .devices
            .iter()
            .map(|device| {
                (
                    fingerprint_hex(&device.tls_fingerprint),
                    Enrolled {
                        device_id: device.device_id.clone(),
                        removed: device.removed_revision.is_some(),
                    },
                )
            })
            .collect();
        Self {
            vault_id: vault_id.to_string(),
            by_fingerprint,
            quarantined: quarantined.clone(),
            anchor_device_id: view.anchor_device_id.clone(),
            takes_proposals: false,
            answers_forwards: false,
        }
    }

    /// The admission of the listener of `own_device_id`, which takes authorization proposals
    /// exactly when the selected log names it as the anchor.
    #[must_use]
    pub fn served_by(mut self, own_device_id: &str) -> Self {
        self.takes_proposals = self.anchor_device_id == own_device_id;
        self
    }

    /// The admission of a server listener, which hands the proposals it holds to the anchor
    /// because the anchor cannot reach them anywhere else: a server dials no one.
    #[must_use]
    pub fn answering_forwards(mut self) -> Self {
        self.answers_forwards = true;
        self
    }

    /// Whether the listener answers authorization proposals, because it anchors the log.
    pub fn takes_proposals(&self) -> bool {
        self.takes_proposals
    }

    /// The certificate fingerprints a TLS handshake may complete with, in the lowercase hex
    /// form of [`ganbaru_handoff::pairing::certificate_fingerprint`]. A removed device still
    /// completes it, for record exchange only.
    pub fn admitted_fingerprints(&self) -> BTreeSet<String> {
        self.by_fingerprint
            .iter()
            .filter(|(_, enrolled)| !self.quarantined.contains(&enrolled.device_id))
            .map(|(fingerprint, _)| fingerprint.clone())
            .collect()
    }

    /// The enrolled device a certificate with `fingerprint` belongs to, if it may connect.
    pub fn certificate(&self, fingerprint: &str) -> Result<&str, MemberRefusal> {
        let enrolled = self
            .by_fingerprint
            .get(fingerprint)
            .ok_or(MemberRefusal::Unknown)?;
        if enrolled.removed {
            return Err(MemberRefusal::Removed);
        }
        if self.quarantined.contains(&enrolled.device_id) {
            return Err(MemberRefusal::Quarantined);
        }
        Ok(&enrolled.device_id)
    }

    /// Whether a connection whose certificate has `fingerprint` may receive a snapshot copy of
    /// `vault_id` for `device_id`: only a device the selected log enrolled and did not remove,
    /// that this installation did not quarantine, asking for itself in this vault. A removed
    /// device gets no copy, unlike record exchange.
    pub fn bootstrap(
        &self,
        fingerprint: &str,
        vault_id: &str,
        device_id: &str,
    ) -> Result<(), MemberRefusal> {
        let enrolled = self.certificate(fingerprint)?;
        if vault_id != self.vault_id {
            return Err(MemberRefusal::VaultMismatch);
        }
        if device_id != enrolled {
            return Err(MemberRefusal::DeviceMismatch);
        }
        Ok(())
    }

    /// Whether a connection whose certificate has `fingerprint` may have `request` answered:
    /// a sync request for this vault that names the device the certificate enrolled, and only
    /// record exchange when the selected log removed that device. A
    /// proposal is answered only by the anchor's listener, and a forward request never, since
    /// the coordinator holds proposals for an anchor that is not reachable directly. A phone
    /// runs a member listener only while the app is in the foreground.
    pub fn request(
        &self,
        fingerprint: &str,
        request: &ControlMessage,
    ) -> Result<(), MemberRefusal> {
        let (vault_id, device_id) = request.sync_peer().ok_or(MemberRefusal::NotSync)?;
        let enrolled = match self.certificate(fingerprint) {
            Err(MemberRefusal::Removed) if request.is_record_exchange() => {
                let device = self
                    .by_fingerprint
                    .get(fingerprint)
                    .map(|enrolled| enrolled.device_id.as_str())
                    .ok_or(MemberRefusal::Unknown)?;
                if self.quarantined.contains(device) {
                    return Err(MemberRefusal::Quarantined);
                }
                device
            }
            enrolled => enrolled?,
        };
        match request {
            ControlMessage::SyncPropose { .. } if !self.takes_proposals => {
                return Err(MemberRefusal::Proposal);
            }
            ControlMessage::SyncForward { .. } if !self.answers_forwards => {
                return Err(MemberRefusal::Forward);
            }
            _ => {}
        }
        if vault_id != self.vault_id {
            return Err(MemberRefusal::VaultMismatch);
        }
        if device_id != enrolled {
            return Err(MemberRefusal::DeviceMismatch);
        }
        Ok(())
    }
}

/// The lowercase hex form of a SHA-256 certificate fingerprint, as pairing pins it.
pub fn fingerprint_hex(fingerprint: &[u8; 32]) -> String {
    fingerprint
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
