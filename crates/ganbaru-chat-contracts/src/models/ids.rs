use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;

const MAX_IDENTIFIER_BYTES: usize = 1_024;

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label} is required"));
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(format!(
            "{label} exceeds the {MAX_IDENTIFIER_BYTES} byte limit"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} contains a control character"));
    }
    Ok(())
}

macro_rules! chat_identifier {
    ($name:ident, $label:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                validate_identifier(&value, $label)?;
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
chat_identifier!(ChatChannelId, "Chat channel ID");
chat_identifier!(ChatParticipantId, "Chat participant ID");
chat_identifier!(ChatConversationId, "Chat conversation ID");
chat_identifier!(ChatConversationItemId, "Chat conversation item ID");
chat_identifier!(ChatMessageRevisionId, "Chat message revision ID");
chat_identifier!(ChatReplyThreadId, "Chat reply thread ID");
chat_identifier!(
    ChatTeammatePolicyRevisionId,
    "Chat teammate policy revision ID"
);
chat_identifier!(ChatWorkAssignmentId, "Chat work assignment ID");
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
chat_identifier!(ChatScheduledMessageId, "Chat scheduled message ID");
chat_identifier!(ChatAccessProfileId, "Chat access profile ID");
chat_identifier!(
    ChatAccessProfileRevisionId,
    "Chat access profile revision ID"
);
chat_identifier!(ChatMessageReferenceId, "Chat message reference ID");
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
