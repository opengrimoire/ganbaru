//! Pure organizational access evaluation shared by persistence and runtime adapters.

use ganbaru_chat_contracts::models::{
    ChatChannelCapabilities, ChatFolderCapability, ChatHistoryBoundary, ChatRuntimeApprovalPolicy,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatAccessDenial {
    RequesterCannotReadSource,
    DestinationAudienceCannotReadSource,
    TeammateCannotReadSource,
    TeammateCannotParticipate,
    FolderCapabilityExceeded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatAuthorityEvaluationInput {
    pub requester_can_read_source: bool,
    pub destination_audience_can_read_source: bool,
    pub destination_capabilities: ChatChannelCapabilities,
    pub source_read_history: bool,
    pub requested_folder_capability: ChatFolderCapability,
    pub granted_folder_capability: ChatFolderCapability,
    pub profile_folder_ceiling: ChatFolderCapability,
    pub provider_folder_ceiling: ChatFolderCapability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatAuthorityEvaluation {
    pub allowed: bool,
    pub effective_folder_capability: ChatFolderCapability,
    pub denials: Vec<ChatAccessDenial>,
}

/// Intersects organizational authority without consulting provider-specific state.
pub fn evaluate_authority(input: &ChatAuthorityEvaluationInput) -> ChatAuthorityEvaluation {
    let effective_folder_capability = input
        .granted_folder_capability
        .intersect(input.profile_folder_ceiling)
        .intersect(input.provider_folder_ceiling);
    let mut denials = Vec::new();
    if !input.requester_can_read_source {
        denials.push(ChatAccessDenial::RequesterCannotReadSource);
    }
    if !input.destination_audience_can_read_source {
        denials.push(ChatAccessDenial::DestinationAudienceCannotReadSource);
    }
    if !input.source_read_history {
        denials.push(ChatAccessDenial::TeammateCannotReadSource);
    }
    if !input.destination_capabilities.participate {
        denials.push(ChatAccessDenial::TeammateCannotParticipate);
    }
    if input.requested_folder_capability.rank() > effective_folder_capability.rank() {
        denials.push(ChatAccessDenial::FolderCapabilityExceeded);
    }
    ChatAuthorityEvaluation {
        allowed: denials.is_empty(),
        effective_folder_capability,
        denials,
    }
}

/// Applies the most local runtime approval override.
pub fn resolve_runtime_approval(
    teammate_default: ChatRuntimeApprovalPolicy,
    channel_override: Option<ChatRuntimeApprovalPolicy>,
    folder_override: Option<ChatRuntimeApprovalPolicy>,
) -> ChatRuntimeApprovalPolicy {
    folder_override
        .or(channel_override)
        .unwrap_or(teammate_default)
}

/// Returns whether the proposed history boundary expands stored visibility.
pub fn history_boundary_is_expansion(
    current: &ChatHistoryBoundary,
    proposed: &ChatHistoryBoundary,
) -> bool {
    matches!(
        (current, proposed),
        (
            ChatHistoryBoundary::FromGrant { .. },
            ChatHistoryBoundary::Entire
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_is_the_narrowest_folder_ceiling() {
        let result = evaluate_authority(&ChatAuthorityEvaluationInput {
            requester_can_read_source: true,
            destination_audience_can_read_source: true,
            destination_capabilities: ChatChannelCapabilities {
                read_history: true,
                participate: true,
            },
            source_read_history: true,
            requested_folder_capability: ChatFolderCapability::Execute,
            granted_folder_capability: ChatFolderCapability::Publish,
            profile_folder_ceiling: ChatFolderCapability::Execute,
            provider_folder_ceiling: ChatFolderCapability::Edit,
        });

        assert_eq!(
            result.effective_folder_capability,
            ChatFolderCapability::Edit
        );
        assert_eq!(
            result.denials,
            vec![ChatAccessDenial::FolderCapabilityExceeded]
        );
        assert!(!result.allowed);
    }

    #[test]
    fn strict_channel_disclosure_requires_every_intersection() {
        let result = evaluate_authority(&ChatAuthorityEvaluationInput {
            requester_can_read_source: true,
            destination_audience_can_read_source: false,
            destination_capabilities: ChatChannelCapabilities {
                read_history: false,
                participate: true,
            },
            source_read_history: true,
            requested_folder_capability: ChatFolderCapability::None,
            granted_folder_capability: ChatFolderCapability::None,
            profile_folder_ceiling: ChatFolderCapability::None,
            provider_folder_ceiling: ChatFolderCapability::None,
        });

        assert_eq!(
            result.denials,
            vec![ChatAccessDenial::DestinationAudienceCannotReadSource]
        );
    }

    #[test]
    fn folder_override_wins_without_granting_folder_authority() {
        assert_eq!(
            resolve_runtime_approval(
                ChatRuntimeApprovalPolicy::Ask,
                Some(ChatRuntimeApprovalPolicy::AutoApprove),
                Some(ChatRuntimeApprovalPolicy::Unattended),
            ),
            ChatRuntimeApprovalPolicy::Unattended
        );
    }
}
