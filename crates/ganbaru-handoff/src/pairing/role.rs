//! The link role of this device and its one transition owner.
//!
//! A device is unlinked, a coordinator with linked devices of its own, a member that follows one
//! coordinator, or a member its coordinator revoked. Each role carries only the progress it can
//! hold, so a revoked device has no coordinator pin and a member never serves its own copy.

use super::{CoordinatorPin, PendingAcknowledgement, StoredOutgoingTransfer, UnlinkReason};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum LinkRole {
    /// Linked to nothing. `reason` records why it last unlinked itself, until the next link.
    Unlinked { reason: Option<UnlinkReason> },
    /// Links at least one device of its own and serves one vault copy at a time.
    Coordinator {
        outgoing: Option<StoredOutgoingTransfer>,
        /// The last copy a receiver activated, kept so a repeated acknowledgement is answered.
        completed: Option<PendingAcknowledgement>,
    },
    /// Follows one pinned coordinator.
    Member {
        coordinator: CoordinatorPin,
        copy: MemberCopy,
    },
    /// Its coordinator revoked this device, which must link again.
    Revoked,
}

/// Whether a member holds its own copy of the linked vault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MemberCopy {
    /// The linked vault has not been activated here yet.
    Required,
    /// A copy is active here. `pending_acknowledgement` names the coordinator copy whose
    /// activation the coordinator has not acknowledged yet.
    Ready {
        pending_acknowledgement: Option<PendingAcknowledgement>,
    },
}

/// A requested change of the link role.
#[derive(Clone, Debug)]
pub(super) enum LinkEvent {
    /// A device of its own was linked.
    PeerLinked,
    /// The last device of its own was unlinked.
    PeersEmptied,
    /// Every link ends because this device unlinked itself.
    UnlinkAll { reason: Option<UnlinkReason> },
    /// The device unlinked its coordinator.
    CoordinatorRemoved { device_id: String },
    /// The pinned coordinator answered that it revoked this device.
    CoordinatorRevoked,
    /// A coordinator linked this device and presented `pin`.
    CoordinatorPinned { pin: CoordinatorPin },
    /// A received copy was activated here and awaits its acknowledgement.
    CopyActivated {
        acknowledgement: PendingAcknowledgement,
    },
    /// The coordinator acknowledged the activation of `transfer_id`.
    CopyAcknowledged { transfer_id: String },
    /// The coordinator prepared a copy for one of its devices.
    OutgoingPrepared { transfer: StoredOutgoingTransfer },
    /// The coordinator released the copy `transfer_id` without an activation.
    OutgoingReleased { transfer_id: String },
    /// The receiver of the outgoing copy reported its activation.
    OutgoingActivated { completed: PendingAcknowledgement },
}

pub(super) const ACTIVE_HANDOFF_ERROR: &str = "finish or retry the active handoff before unlinking";

impl LinkRole {
    pub(super) fn coordinator(&self) -> Option<&CoordinatorPin> {
        match self {
            Self::Member { coordinator, .. } => Some(coordinator),
            _ => None,
        }
    }

    pub(super) fn coordinator_mut(&mut self) -> Option<&mut CoordinatorPin> {
        match self {
            Self::Member { coordinator, .. } => Some(coordinator),
            _ => None,
        }
    }

    pub(super) fn copy_ready(&self) -> bool {
        matches!(
            self,
            Self::Member {
                copy: MemberCopy::Ready { .. },
                ..
            }
        )
    }

    pub(super) fn pending_acknowledgement(&self) -> Option<&PendingAcknowledgement> {
        match self {
            Self::Member {
                copy:
                    MemberCopy::Ready {
                        pending_acknowledgement,
                    },
                ..
            } => pending_acknowledgement.as_ref(),
            _ => None,
        }
    }

    pub(super) fn outgoing(&self) -> Option<&StoredOutgoingTransfer> {
        match self {
            Self::Coordinator { outgoing, .. } => outgoing.as_ref(),
            _ => None,
        }
    }

    pub(super) fn completed(&self) -> Option<&PendingAcknowledgement> {
        match self {
            Self::Coordinator { completed, .. } => completed.as_ref(),
            _ => None,
        }
    }

    /// Whether a handoff in flight must finish before any link can end.
    pub(super) fn has_active_handoff(&self) -> bool {
        self.pending_acknowledgement().is_some() || self.outgoing().is_some()
    }
}

/// Returns the role `event` leads to from `role`, or why the move is refused.
pub(super) fn transition(role: &LinkRole, event: LinkEvent) -> Result<LinkRole, String> {
    use LinkEvent as Event;
    use LinkRole as Role;
    match (role, event) {
        (Role::Member { .. }, Event::PeerLinked) => {
            Err("a device linked to a coordinator cannot link devices of its own".to_string())
        }
        (Role::Coordinator { .. }, Event::PeerLinked) => Ok(role.clone()),
        (Role::Unlinked { .. } | Role::Revoked, Event::PeerLinked) => Ok(Role::Coordinator {
            outgoing: None,
            completed: None,
        }),
        (Role::Coordinator { outgoing: Some(_), .. }, Event::PeersEmptied) => {
            Err(ACTIVE_HANDOFF_ERROR.to_string())
        }
        (Role::Coordinator { .. }, Event::PeersEmptied) => Ok(Role::Unlinked { reason: None }),
        (_, Event::PeersEmptied) => Err("this device links no devices of its own".to_string()),
        (_, Event::UnlinkAll { .. }) if role.has_active_handoff() => {
            Err(ACTIVE_HANDOFF_ERROR.to_string())
        }
        (_, Event::UnlinkAll { reason }) => Ok(Role::Unlinked { reason }),
        (_, Event::CoordinatorRemoved { .. }) if role.has_active_handoff() => {
            Err(ACTIVE_HANDOFF_ERROR.to_string())
        }
        (Role::Member { coordinator, .. }, Event::CoordinatorRemoved { device_id })
            if coordinator.device_id == device_id =>
        {
            Ok(Role::Unlinked { reason: None })
        }
        (_, Event::CoordinatorRemoved { .. }) => {
            Err("linked device was not found".to_string())
        }
        (Role::Member { .. }, Event::CoordinatorRevoked) => Ok(Role::Revoked),
        (_, Event::CoordinatorRevoked) => {
            Err("coordinator changed before revocation could be applied".to_string())
        }
        (Role::Unlinked { .. } | Role::Revoked, Event::CoordinatorPinned { pin }) => {
            Ok(Role::Member {
                coordinator: pin,
                copy: MemberCopy::Required,
            })
        }
        (Role::Member { coordinator, copy }, Event::CoordinatorPinned { mut pin })
            if coordinator.device_id == pin.device_id
                && coordinator.vault_id == pin.vault_id
                && coordinator.certificate_fingerprint == pin.certificate_fingerprint =>
        {
            // Relinking to the same coordinator and vault keeps its copy and what its exchanges
            // reported.
            pin.reported_endpoints = coordinator.reported_endpoints.clone();
            Ok(Role::Member {
                coordinator: pin,
                copy: copy.clone(),
            })
        }
        (Role::Member { .. }, Event::CoordinatorPinned { .. }) => Err(
            "this device is already linked to another coordinator; unlink it before linking to a different coordinator"
                .to_string(),
        ),
        (Role::Coordinator { .. }, Event::CoordinatorPinned { .. }) => Err(
            "unlink coordinated devices before linking this device to another coordinator"
                .to_string(),
        ),
        (Role::Member { coordinator, .. }, Event::CopyActivated { acknowledgement }) => {
            Ok(Role::Member {
                coordinator: coordinator.clone(),
                copy: MemberCopy::Ready {
                    pending_acknowledgement: Some(acknowledgement),
                },
            })
        }
        (_, Event::CopyActivated { .. }) => {
            Err("this device is not linked to a coordinator".to_string())
        }
        (
            Role::Member {
                coordinator,
                copy:
                    MemberCopy::Ready {
                        pending_acknowledgement: Some(pending),
                    },
            },
            Event::CopyAcknowledged { transfer_id },
        ) => {
            if pending.transfer_id != transfer_id {
                return Err("pending activation acknowledgement does not match".to_string());
            }
            Ok(Role::Member {
                coordinator: coordinator.clone(),
                copy: MemberCopy::Ready {
                    pending_acknowledgement: None,
                },
            })
        }
        (_, Event::CopyAcknowledged { .. }) => Ok(role.clone()),
        (
            Role::Coordinator {
                outgoing: None,
                completed,
            },
            Event::OutgoingPrepared { transfer },
        ) => Ok(Role::Coordinator {
            outgoing: Some(transfer),
            completed: completed.clone(),
        }),
        (Role::Coordinator { .. }, Event::OutgoingPrepared { .. }) => {
            Err("the coordinator is already serving a vault copy".to_string())
        }
        (_, Event::OutgoingPrepared { .. }) => {
            Err("only a coordinator with linked devices serves a vault copy".to_string())
        }
        (
            Role::Coordinator {
                outgoing: Some(transfer),
                completed,
            },
            Event::OutgoingReleased { transfer_id },
        ) => {
            if transfer.metadata.transfer_id != transfer_id {
                return Err("outgoing transfer identity does not match".to_string());
            }
            Ok(Role::Coordinator {
                outgoing: None,
                completed: completed.clone(),
            })
        }
        (_, Event::OutgoingReleased { .. }) => Ok(role.clone()),
        (Role::Coordinator { outgoing, .. }, Event::OutgoingActivated { completed }) => {
            if outgoing
                .as_ref()
                .is_some_and(|transfer| transfer.metadata.transfer_id != completed.transfer_id)
            {
                return Err("completed activation does not match outgoing transfer".to_string());
            }
            Ok(Role::Coordinator {
                outgoing: None,
                completed: Some(completed),
            })
        }
        (_, Event::OutgoingActivated { .. }) => {
            Err("only a coordinator with linked devices completes a vault copy".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BundleMetadata, PROTOCOL_VERSION};

    fn pin(device_id: &str) -> CoordinatorPin {
        CoordinatorPin {
            device_id: device_id.to_string(),
            device_label: None,
            endpoint: "192.168.1.20:43821".to_string(),
            vault_id: "vault-1".to_string(),
            certificate_fingerprint: "f".repeat(64),
            last_endpoint: None,
            reported_endpoints: Vec::new(),
            charter: None,
        }
    }

    fn acknowledgement(transfer_id: &str) -> PendingAcknowledgement {
        PendingAcknowledgement {
            vault_id: "vault-1".to_string(),
            device_id: "device-phone".to_string(),
            transfer_id: transfer_id.to_string(),
        }
    }

    fn transfer(transfer_id: &str) -> StoredOutgoingTransfer {
        StoredOutgoingTransfer {
            metadata: BundleMetadata {
                protocol_version: PROTOCOL_VERSION,
                compatibility: crate::protocol::test_compatibility(),
                vault_id: "vault-1".to_string(),
                device_id: "device-phone".to_string(),
                transfer_id: transfer_id.to_string(),
                archive_bytes: 42,
                archive_sha256: "a".repeat(64),
                derived_state_stale: false,
            },
            expires_at_ms: 1_000,
        }
    }

    fn member(copy: MemberCopy) -> LinkRole {
        LinkRole::Member {
            coordinator: pin("device-desktop"),
            copy,
        }
    }

    fn coordinator(outgoing: Option<&str>, completed: Option<&str>) -> LinkRole {
        LinkRole::Coordinator {
            outgoing: outgoing.map(transfer),
            completed: completed.map(acknowledgement),
        }
    }

    fn ready(pending: Option<&str>) -> MemberCopy {
        MemberCopy::Ready {
            pending_acknowledgement: pending.map(acknowledgement),
        }
    }

    /// One value of every role and progress combination the type can hold.
    fn every_role() -> Vec<LinkRole> {
        vec![
            LinkRole::Unlinked { reason: None },
            LinkRole::Unlinked {
                reason: Some(UnlinkReason::KeyProtectionLost),
            },
            coordinator(None, None),
            coordinator(Some("transfer-1"), None),
            coordinator(None, Some("transfer-0")),
            coordinator(Some("transfer-1"), Some("transfer-0")),
            member(MemberCopy::Required),
            member(ready(None)),
            member(ready(Some("transfer-1"))),
            LinkRole::Revoked,
        ]
    }

    fn every_event() -> Vec<LinkEvent> {
        vec![
            LinkEvent::PeerLinked,
            LinkEvent::PeersEmptied,
            LinkEvent::UnlinkAll { reason: None },
            LinkEvent::UnlinkAll {
                reason: Some(UnlinkReason::KeyProtectionLost),
            },
            LinkEvent::CoordinatorRemoved {
                device_id: "device-desktop".to_string(),
            },
            LinkEvent::CoordinatorRemoved {
                device_id: "device-other".to_string(),
            },
            LinkEvent::CoordinatorRevoked,
            LinkEvent::CoordinatorPinned {
                pin: pin("device-desktop"),
            },
            LinkEvent::CoordinatorPinned {
                pin: pin("device-other"),
            },
            LinkEvent::CopyActivated {
                acknowledgement: acknowledgement("transfer-2"),
            },
            LinkEvent::CopyAcknowledged {
                transfer_id: "transfer-1".to_string(),
            },
            LinkEvent::CopyAcknowledged {
                transfer_id: "transfer-9".to_string(),
            },
            LinkEvent::OutgoingPrepared {
                transfer: transfer("transfer-2"),
            },
            LinkEvent::OutgoingReleased {
                transfer_id: "transfer-1".to_string(),
            },
            LinkEvent::OutgoingReleased {
                transfer_id: "transfer-9".to_string(),
            },
            LinkEvent::OutgoingActivated {
                completed: acknowledgement("transfer-1"),
            },
            LinkEvent::OutgoingActivated {
                completed: acknowledgement("transfer-9"),
            },
        ]
    }

    /// Whether `role` may follow `event`, stated from the rules instead of the implementation.
    fn expected(role: &LinkRole, event: &LinkEvent) -> Option<LinkRole> {
        use LinkEvent as Event;
        use LinkRole as Role;
        let active = role.has_active_handoff();
        match event {
            Event::PeerLinked => match role {
                Role::Member { .. } => None,
                Role::Coordinator { .. } => Some(role.clone()),
                _ => Some(coordinator(None, None)),
            },
            Event::PeersEmptied => match role {
                Role::Coordinator { outgoing: None, .. } => Some(Role::Unlinked { reason: None }),
                _ => None,
            },
            Event::UnlinkAll { reason } => (!active).then_some(Role::Unlinked { reason: *reason }),
            Event::CoordinatorRemoved { device_id } => match role {
                Role::Member { coordinator, .. }
                    if !active && &coordinator.device_id == device_id =>
                {
                    Some(Role::Unlinked { reason: None })
                }
                _ => None,
            },
            Event::CoordinatorRevoked => {
                matches!(role, Role::Member { .. }).then_some(Role::Revoked)
            }
            Event::CoordinatorPinned { pin: next } => match role {
                Role::Unlinked { .. } | Role::Revoked => Some(Role::Member {
                    coordinator: next.clone(),
                    copy: MemberCopy::Required,
                }),
                Role::Member { coordinator, copy } if coordinator.device_id == next.device_id => {
                    Some(Role::Member {
                        coordinator: next.clone(),
                        copy: copy.clone(),
                    })
                }
                _ => None,
            },
            Event::CopyActivated { acknowledgement } => match role {
                Role::Member { coordinator, .. } => Some(Role::Member {
                    coordinator: coordinator.clone(),
                    copy: MemberCopy::Ready {
                        pending_acknowledgement: Some(acknowledgement.clone()),
                    },
                }),
                _ => None,
            },
            Event::CopyAcknowledged { transfer_id } => match role.pending_acknowledgement() {
                Some(pending) if &pending.transfer_id == transfer_id => Some(member(ready(None))),
                Some(_) => None,
                None => Some(role.clone()),
            },
            Event::OutgoingPrepared { transfer: next } => match role {
                Role::Coordinator {
                    outgoing: None,
                    completed,
                } => Some(Role::Coordinator {
                    outgoing: Some(next.clone()),
                    completed: completed.clone(),
                }),
                _ => None,
            },
            Event::OutgoingReleased { transfer_id } => match role {
                Role::Coordinator {
                    outgoing: Some(current),
                    completed,
                } => (&current.metadata.transfer_id == transfer_id).then(|| Role::Coordinator {
                    outgoing: None,
                    completed: completed.clone(),
                }),
                _ => Some(role.clone()),
            },
            Event::OutgoingActivated { completed } => match role {
                Role::Coordinator { outgoing, .. }
                    if outgoing.as_ref().is_none_or(|current| {
                        current.metadata.transfer_id == completed.transfer_id
                    }) =>
                {
                    Some(Role::Coordinator {
                        outgoing: None,
                        completed: Some(completed.clone()),
                    })
                }
                _ => None,
            },
        }
    }

    #[test]
    fn every_role_and_event_pair_follows_the_link_rules() {
        for role in every_role() {
            for event in every_event() {
                let outcome = transition(&role, event.clone()).ok();
                assert_eq!(outcome, expected(&role, &event), "{role:?} after {event:?}");
            }
        }
    }

    #[test]
    fn no_link_ends_while_a_handoff_is_in_flight() {
        for role in [
            coordinator(Some("transfer-1"), None),
            member(ready(Some("transfer-1"))),
        ] {
            for event in [
                LinkEvent::UnlinkAll { reason: None },
                LinkEvent::CoordinatorRemoved {
                    device_id: "device-desktop".to_string(),
                },
                LinkEvent::PeersEmptied,
            ] {
                assert!(transition(&role, event).is_err(), "{role:?}");
            }
        }
    }

    #[test]
    fn relinking_the_same_coordinator_keeps_the_copy_and_reported_contact() {
        let mut reported = pin("device-desktop");
        reported.reported_endpoints = vec!["192.168.1.21:43821".to_string()];
        let role = LinkRole::Member {
            coordinator: reported,
            copy: ready(None),
        };
        let relinked = transition(
            &role,
            LinkEvent::CoordinatorPinned {
                pin: pin("device-desktop"),
            },
        )
        .expect("relink");
        assert!(relinked.copy_ready());
        assert_eq!(
            relinked.coordinator().expect("pin").reported_endpoints,
            vec!["192.168.1.21:43821".to_string()]
        );
    }

    #[test]
    fn a_revoked_member_must_take_a_new_copy() {
        let revoked = transition(
            &member(ready(Some("transfer-1"))),
            LinkEvent::CoordinatorRevoked,
        )
        .expect("revoke");
        assert_eq!(revoked, LinkRole::Revoked);
        let relinked = transition(
            &revoked,
            LinkEvent::CoordinatorPinned {
                pin: pin("device-other"),
            },
        )
        .expect("link again");
        assert!(!relinked.copy_ready());
    }
}
