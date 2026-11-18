//! Digests of what a local acceptance of synced Chat work covers.

use crate::scheduling::{RecurrenceRule, RecurrenceSpec};
use sha2::{Digest, Sha256};

/// Domain of an assignment acceptance digest.
pub const ACCEPTANCE_DOMAIN: &[u8] = b"ganbaru-assignment-acceptance-v2";
/// Domain of a scheduled message consent digest, so neither kind satisfies the other.
pub const SCHEDULED_CONSENT_DOMAIN: &[u8] = b"ganbaru-scheduled-consent-v1";
/// Domain of a repeating scheduled message consent digest.
pub const SCHEDULED_SERIES_CONSENT_DOMAIN: &[u8] = b"ganbaru-scheduled-series-consent-v1";

/// One folder an agent could run in, with the access it would hold there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptanceGrant {
    pub working_folder_id: String,
    /// Stored folder capability the resolved access level grants.
    pub capability: String,
    /// Stored runtime approval the resolved access level runs under in the folder.
    pub runtime_approval: String,
}

/// What would run on this device for an assignment.
#[derive(Clone, Copy, Debug)]
pub struct AcceptanceSubject<'a> {
    pub agent_id: &'a str,
    /// Stored spelling of the effective access level the device resolved for the channel.
    pub access_level: &'a str,
    /// Hex digest of the instructions frozen on the assignment.
    pub instructions_sha256: &'a str,
    /// Hex digest of the model selection frozen on the assignment.
    pub model_selection_sha256: &'a str,
    pub provider_instance_id: &'a str,
    /// Folders sorted by working folder id.
    pub grants: &'a [AcceptanceGrant],
    /// Stored runtime approval in the agent's scratch, empty when the level allows no scratch.
    pub scratch_runtime_approval: &'a str,
}

/// Hex digest of `subject`.
pub fn acceptance_digest(subject: &AcceptanceSubject<'_>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ACCEPTANCE_DOMAIN);
    absorb(&mut hasher, subject.agent_id);
    absorb(&mut hasher, subject.access_level);
    absorb(&mut hasher, subject.instructions_sha256);
    absorb(&mut hasher, subject.model_selection_sha256);
    absorb(&mut hasher, subject.provider_instance_id);
    absorb(&mut hasher, subject.scratch_runtime_approval);
    hasher.update((subject.grants.len() as u64).to_be_bytes());
    for grant in subject.grants {
        absorb(&mut hasher, &grant.working_folder_id);
        absorb(&mut hasher, &grant.capability);
        absorb(&mut hasher, &grant.runtime_approval);
    }
    format!("{:x}", hasher.finalize())
}

/// Hex digest of the stored request `request_data` of `scheduled_message_id`.
pub fn scheduled_consent_digest(scheduled_message_id: &str, request_data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(SCHEDULED_CONSENT_DOMAIN);
    absorb(&mut hasher, scheduled_message_id);
    absorb(&mut hasher, request_data);
    format!("{:x}", hasher.finalize())
}

/// Hex digest of the stored request `request_data` of the repeating scheduled message
/// `scheduled_message_id` together with its recurrence `spec`, so a consent covers one request
/// on one schedule and never satisfies the one-time digest of the same request.
pub fn scheduled_series_consent_digest(
    scheduled_message_id: &str,
    request_data: &str,
    spec: &RecurrenceSpec,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(SCHEDULED_SERIES_CONSENT_DOMAIN);
    absorb(&mut hasher, scheduled_message_id);
    absorb(&mut hasher, request_data);
    absorb(&mut hasher, spec.rule.kind());
    match spec.rule {
        RecurrenceRule::Interval { minutes } => hasher.update(minutes.to_be_bytes()),
        RecurrenceRule::FixedTime {
            local_minute,
            weekdays,
        } => {
            hasher.update(local_minute.to_be_bytes());
            hasher.update([weekdays.mask()]);
        }
    }
    absorb(&mut hasher, &spec.time_zone);
    absorb(&mut hasher, spec.quiet_policy.as_str());
    format!("{:x}", hasher.finalize())
}

/// Adds `value` with its length, so adjacent values cannot trade bytes.
fn absorb(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
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
    fn acceptance_digest_changes_with_access_instructions_model_provider_and_grants() {
        let grants = [grant("folder:a", "read", "ask")];
        let subject = AcceptanceSubject {
            agent_id: "participant:agent",
            access_level: "ask",
            model_selection_sha256: "model:1",
            instructions_sha256: "instructions:1",
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
                agent_id: "participant:other",
                ..subject
            },
            AcceptanceSubject {
                access_level: "own",
                ..subject
            },
            AcceptanceSubject {
                model_selection_sha256: "model:2",
                ..subject
            },
            AcceptanceSubject {
                instructions_sha256: "instructions:2",
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
            agent_id: "ab",
            access_level: "c",
            model_selection_sha256: "",
            instructions_sha256: "",
            provider_instance_id: "",
            grants: &[],
            scratch_runtime_approval: "",
        };
        let joined = AcceptanceSubject {
            agent_id: "a",
            access_level: "bc",
            ..split
        };
        assert_ne!(acceptance_digest(&split), acceptance_digest(&joined));
    }

    #[test]
    fn acceptance_digest_differs_from_a_scheduled_consent_of_the_same_values() {
        let subject = AcceptanceSubject {
            agent_id: "same",
            access_level: "same",
            model_selection_sha256: "",
            instructions_sha256: "",
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

    #[test]
    fn scheduled_series_consent_digest_covers_the_schedule() {
        use crate::scheduling::{QuietPolicy, WeekdaySet};
        let spec = RecurrenceSpec {
            rule: RecurrenceRule::FixedTime {
                local_minute: 540,
                weekdays: WeekdaySet::ALL,
            },
            time_zone: "Europe/Madrid".to_owned(),
            quiet_policy: QuietPolicy::Defer,
        };
        let digest = scheduled_series_consent_digest("message:a", "{}", &spec);
        assert_ne!(digest, scheduled_consent_digest("message:a", "{}"));
        let changed = [
            RecurrenceSpec {
                rule: RecurrenceRule::FixedTime {
                    local_minute: 541,
                    weekdays: WeekdaySet::ALL,
                },
                ..spec.clone()
            },
            RecurrenceSpec {
                rule: RecurrenceRule::Interval { minutes: 540 },
                ..spec.clone()
            },
            RecurrenceSpec {
                time_zone: "Europe/Lisbon".to_owned(),
                ..spec.clone()
            },
            RecurrenceSpec {
                quiet_policy: QuietPolicy::Ignore,
                ..spec.clone()
            },
        ];
        for other in &changed {
            assert_ne!(
                digest,
                scheduled_series_consent_digest("message:a", "{}", other),
                "{other:?}"
            );
        }
        assert_ne!(
            digest,
            scheduled_series_consent_digest("message:a", "{\"changed\":true}", &spec)
        );
    }
}
