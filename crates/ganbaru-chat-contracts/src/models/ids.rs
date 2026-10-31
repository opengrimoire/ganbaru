use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;

/// Byte limit of device-local and provider identifiers.
const MAX_IDENTIFIER_BYTES: usize = 1_024;
/// Byte limit of organizational identifiers, which become replicated row keys and must fit the
/// sync engine's row key bound.
pub const MAX_ORGANIZATIONAL_IDENTIFIER_BYTES: usize = 128;

fn validate_identifier(value: &str, label: &str, max_bytes: usize) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label} is required"));
    }
    if value.len() > max_bytes {
        return Err(format!("{label} exceeds the {max_bytes} byte limit"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} contains a control character"));
    }
    Ok(())
}

macro_rules! chat_identifier {
    ($name:ident, $label:literal) => {
        chat_identifier!($name, $label, MAX_IDENTIFIER_BYTES);
    };
    ($name:ident, $label:literal, $max_bytes:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                validate_identifier(&value, $label, $max_bytes)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

pub use ganbaru_working_folders::{ProjectWorkingFolderId, UtcTimestamp};
chat_identifier!(
    ChatChannelId,
    "Chat channel ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatParticipantId,
    "Chat participant ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatConversationId,
    "Chat conversation ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatConversationItemId,
    "Chat conversation item ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatMessageRevisionId,
    "Chat message revision ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatReplyThreadId,
    "Chat reply thread ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatTeammatePolicyRevisionId,
    "Chat teammate policy revision ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatWorkAssignmentId,
    "Chat work assignment ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(ChatAgentRunId, "Chat agent run ID");
chat_identifier!(ChatThreadId, "Chat thread ID");
chat_identifier!(ChatTurnId, "Chat turn ID");
chat_identifier!(ChatMessageId, "Chat message ID");
chat_identifier!(ChatActivityId, "Chat activity ID");
chat_identifier!(ChatRequestId, "Chat request ID");
chat_identifier!(ChatPlanId, "Chat plan ID");
chat_identifier!(ChatAttachmentId, "Chat attachment ID");
chat_identifier!(ChatEventId, "Chat event ID");
chat_identifier!(ChatCommandId, "Chat command ID");
chat_identifier!(
    ChatScheduledMessageId,
    "Chat scheduled message ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatAccessProfileId,
    "Chat access profile ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatAccessProfileRevisionId,
    "Chat access profile revision ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatMessageReferenceId,
    "Chat message reference ID",
    MAX_ORGANIZATIONAL_IDENTIFIER_BYTES
);
chat_identifier!(
    ChatAuthorizationRevisionId,
    "Chat authorization revision ID"
);
chat_identifier!(ChatScratchScopeId, "Chat scratch scope ID");
chat_identifier!(ChatScratchGenerationId, "Chat scratch generation ID");
chat_identifier!(ChatScratchPromotionId, "Chat scratch promotion ID");
chat_identifier!(ChatScratchCleanupJobId, "Chat scratch cleanup job ID");
chat_identifier!(ChatExecutionEnvironmentId, "Chat execution environment ID");
chat_identifier!(ChatCheckpointId, "Chat checkpoint ID");
chat_identifier!(CredentialReferenceId, "credential reference ID");
chat_identifier!(ProviderFamilyId, "provider family ID");
chat_identifier!(ProviderInstanceId, "provider instance ID");
chat_identifier!(ProviderSessionId, "provider session ID");
chat_identifier!(ProviderThreadId, "provider thread ID");
chat_identifier!(ProviderTurnId, "provider turn ID");
chat_identifier!(ProviderItemId, "provider item ID");
chat_identifier!(ProviderRequestId, "provider request ID");
chat_identifier!(ProviderTaskId, "provider task ID");
chat_identifier!(ContinuationGroupId, "continuation group ID");
chat_identifier!(ModelId, "model ID");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organizational_ids_fit_the_row_key_bound_while_local_ids_keep_theirs() {
        let organizational = "c".repeat(MAX_ORGANIZATIONAL_IDENTIFIER_BYTES);
        assert!(ChatConversationId::new(organizational.clone()).is_ok());
        assert!(ChatConversationId::new(format!("{organizational}c")).is_err());
        assert!(ChatReplyThreadId::new(format!("{organizational}c")).is_err());

        let local = "t".repeat(MAX_IDENTIFIER_BYTES);
        assert!(ChatThreadId::new(local.clone()).is_ok());
        assert!(ChatThreadId::new(format!("{local}t")).is_err());
        assert!(ChatConversationId::new("").is_err());
        assert!(ChatConversationId::new("conversation:\n").is_err());
    }
}
