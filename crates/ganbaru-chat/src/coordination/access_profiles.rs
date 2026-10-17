//! Built-in access profile recipes and expansion classification.

use ganbaru_chat_contracts::models::{
    ChatAccessProfileBuiltinKey, ChatChannelCapabilities, ChatFolderCapability, ChatHistoryBoundary,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltinAccessProfileDefinition {
    pub id: &'static str,
    pub key: ChatAccessProfileBuiltinKey,
    pub display_name: &'static str,
    pub default_channel_capabilities: ChatChannelCapabilities,
    pub default_history_boundary: ChatHistoryBoundary,
    pub maximum_folder_capability: ChatFolderCapability,
}

/// Returns the immutable built-in profile recipes seeded by the database.
pub fn builtin_access_profiles() -> [BuiltinAccessProfileDefinition; 5] {
    [
        definition(
            "access-profile:conversation-only",
            ChatAccessProfileBuiltinKey::ConversationOnly,
            "Conversation only",
            ChatFolderCapability::None,
        ),
        definition(
            "access-profile:read-only",
            ChatAccessProfileBuiltinKey::ReadOnly,
            "Read only",
            ChatFolderCapability::Read,
        ),
        definition(
            "access-profile:edit-files",
            ChatAccessProfileBuiltinKey::EditFiles,
            "Edit files",
            ChatFolderCapability::Edit,
        ),
        definition(
            "access-profile:build-and-test",
            ChatAccessProfileBuiltinKey::BuildAndTest,
            "Build and test",
            ChatFolderCapability::Execute,
        ),
        definition(
            "access-profile:publish-changes",
            ChatAccessProfileBuiltinKey::PublishChanges,
            "Publish changes",
            ChatFolderCapability::Publish,
        ),
    ]
}

fn definition(
    id: &'static str,
    key: ChatAccessProfileBuiltinKey,
    display_name: &'static str,
    maximum_folder_capability: ChatFolderCapability,
) -> BuiltinAccessProfileDefinition {
    BuiltinAccessProfileDefinition {
        id,
        key,
        display_name,
        default_channel_capabilities: ChatChannelCapabilities {
            read_history: true,
            participate: true,
        },
        default_history_boundary: ChatHistoryBoundary::Entire,
        maximum_folder_capability,
    }
}

/// Returns whether a profile revision grants any broader authority.
pub fn profile_revision_is_expansion(
    current_capabilities: ChatChannelCapabilities,
    current_history: &ChatHistoryBoundary,
    current_folder: ChatFolderCapability,
    proposed_capabilities: ChatChannelCapabilities,
    proposed_history: &ChatHistoryBoundary,
    proposed_folder: ChatFolderCapability,
) -> bool {
    (!current_capabilities.read_history && proposed_capabilities.read_history)
        || (!current_capabilities.participate && proposed_capabilities.participate)
        || super::access::history_boundary_is_expansion(current_history, proposed_history)
        || proposed_folder.rank() > current_folder.rank()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_recipes_without_a_principal() {
        let profiles = builtin_access_profiles();
        assert_eq!(profiles.len(), 5);
        assert!(
            profiles
                .iter()
                .all(|profile| profile.id.starts_with("access-profile:"))
        );
        assert_eq!(
            profiles[0].maximum_folder_capability,
            ChatFolderCapability::None
        );
    }

    #[test]
    fn entire_history_is_an_expansion_from_a_grant_boundary() {
        assert!(profile_revision_is_expansion(
            ChatChannelCapabilities::default(),
            &ChatHistoryBoundary::FromGrant {
                lower_ordinal: Some(12),
            },
            ChatFolderCapability::Read,
            ChatChannelCapabilities::default(),
            &ChatHistoryBoundary::Entire,
            ChatFolderCapability::Read,
        ));
    }
}
