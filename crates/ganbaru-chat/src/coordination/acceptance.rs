//! Digests of what a local acceptance of synced Chat work covers.

/// Domain of an assignment acceptance digest.
pub const ACCEPTANCE_DOMAIN: &[u8] = b"ganbaru-assignment-acceptance-v1";
/// Domain of a scheduled message consent digest, so neither kind satisfies the other.
pub const SCHEDULED_CONSENT_DOMAIN: &[u8] = b"ganbaru-scheduled-consent-v1";

/// One folder a teammate could run in, with the access it would hold there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptanceGrant {
    pub working_folder_id: String,
    pub capability: String,
    /// Effective runtime approval in the folder.
    pub runtime_approval: String,
}

/// What would run on this device for an assignment.
#[derive(Clone, Copy, Debug)]
pub struct AcceptanceSubject<'a> {
    pub teammate_id: &'a str,
    pub policy_revision_id: &'a str,
    pub instructions: &'a str,
    pub provider_instance_id: &'a str,
    /// Grants sorted by working folder id.
    pub grants: &'a [AcceptanceGrant],
    /// Effective runtime approval in the teammate's scratch.
    pub scratch_runtime_approval: &'a str,
}

/// Hex digest of `subject`.
pub fn acceptance_digest(_subject: &AcceptanceSubject<'_>) -> String {
    String::new()
}

/// Hex digest of the stored request `request_data` of `scheduled_message_id`.
pub fn scheduled_consent_digest(_scheduled_message_id: &str, _request_data: &str) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(folder: &str, capability: &str, approval: &str) -> AcceptanceGrant {
        AcceptanceGrant {
            working_folder_id: folder.to_owned(),
            capability: capability.to_owned(),
            runtime_approval: approval.to_owned(),
        }
    }

    #[test]
    fn acceptance_digest_changes_with_policy_instructions_provider_and_grants() {
        let grants = [grant("folder:a", "read", "ask")];
        let subject = AcceptanceSubject {
            teammate_id: "participant:agent",
            policy_revision_id: "policy:1",
            instructions: "Review the code",
            provider_instance_id: "provider:a",
            grants: &grants,
            scratch_runtime_approval: "ask",
        };
        let digest = acceptance_digest(&subject);
        assert_eq!(digest.len(), 64, "a hex SHA-256 digest");
        assert_eq!(digest, acceptance_digest(&subject), "the digest is stable");

        let widened_capability = [grant("folder:a", "execute", "ask")];
        let widened_approval = [grant("folder:a", "read", "auto_approve")];
        let added_folder = [
            grant("folder:a", "read", "ask"),
            grant("folder:b", "read", "ask"),
        ];
        let changed = [
            AcceptanceSubject {
                teammate_id: "participant:other",
                ..subject
            },
            AcceptanceSubject {
                policy_revision_id: "policy:2",
                ..subject
            },
            AcceptanceSubject {
                instructions: "Delete everything",
                ..subject
            },
            AcceptanceSubject {
                provider_instance_id: "provider:b",
                ..subject
            },
            AcceptanceSubject {
                grants: &widened_capability,
                ..subject
            },
            AcceptanceSubject {
                grants: &widened_approval,
                ..subject
            },
            AcceptanceSubject {
                grants: &added_folder,
                ..subject
            },
            AcceptanceSubject {
                grants: &[],
                ..subject
            },
            AcceptanceSubject {
                scratch_runtime_approval: "auto_approve",
                ..subject
            },
        ];
        for other in &changed {
            assert_ne!(digest, acceptance_digest(other), "{other:?}");
        }
    }

    #[test]
    fn acceptance_digest_keeps_field_boundaries() {
        let split = AcceptanceSubject {
            teammate_id: "ab",
            policy_revision_id: "c",
            instructions: "",
            provider_instance_id: "",
            grants: &[],
            scratch_runtime_approval: "",
        };
        let joined = AcceptanceSubject {
            teammate_id: "a",
            policy_revision_id: "bc",
            ..split
        };
        assert_ne!(acceptance_digest(&split), acceptance_digest(&joined));
    }

    #[test]
    fn acceptance_digest_differs_from_a_scheduled_consent_of_the_same_values() {
        let subject = AcceptanceSubject {
            teammate_id: "same",
            policy_revision_id: "same",
            instructions: "",
            provider_instance_id: "",
            grants: &[],
            scratch_runtime_approval: "",
        };
        assert_ne!(
            acceptance_digest(&subject),
            scheduled_consent_digest("same", "same")
        );
        assert_ne!(
            scheduled_consent_digest("message:a", "{}"),
            scheduled_consent_digest("message:a", "{\"changed\":true}")
        );
    }
}
