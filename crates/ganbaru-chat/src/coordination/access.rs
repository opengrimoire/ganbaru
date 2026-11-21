//! Pure agent access resolution shared by persistence, sync effects, and runtime adapters.
//!
//! A channel's access comes from the channel, else its project, else the app. Each agent then
//! works at that level or the closest narrower behavior its provider can enforce on the device.

use ganbaru_chat_contracts::models::{
    ChatAccessDowngradeReason, ChatAgentAccess, ChatAgentAccessSource, ChatEffectiveAgentAccess,
    ChatFolderCapability, ChatRuntimeApprovalPolicy, ProviderAuthoritySupport,
};
use sha2::{Digest, Sha256};

/// What one agent may do under a requested access level with one provider's enforcement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedAgentAccess {
    pub requested: ChatAgentAccess,
    /// `None` when the provider cannot even reply without a folder.
    pub effective: Option<ChatEffectiveAgentAccess>,
    pub folder_capability: ChatFolderCapability,
    pub runtime_approval: ChatRuntimeApprovalPolicy,
    pub scratch_allowed: bool,
    /// How scratch commands are approved, meaningful only when `scratch_allowed`.
    pub scratch_approval: ChatRuntimeApprovalPolicy,
    /// Why the requested level failed, absent when the agent works as requested.
    pub downgrade_reason: Option<ChatAccessDowngradeReason>,
}

impl ResolvedAgentAccess {
    /// Whether the agent may work in the channel at all.
    pub const fn can_work(&self) -> bool {
        self.effective.is_some()
    }
}

/// The most a level can ever allow, whatever the provider supports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentAccessCeiling {
    pub folder_capability: ChatFolderCapability,
    pub auto_approve: bool,
    pub scratch_allowed: bool,
}

/// Returns the most `level` allows with a provider that enforces everything.
pub const fn access_ceiling(level: ChatAgentAccess) -> AgentAccessCeiling {
    match level {
        ChatAgentAccess::Talk => AgentAccessCeiling {
            folder_capability: ChatFolderCapability::None,
            auto_approve: false,
            scratch_allowed: false,
        },
        ChatAgentAccess::Read => AgentAccessCeiling {
            folder_capability: ChatFolderCapability::Read,
            auto_approve: false,
            scratch_allowed: false,
        },
        ChatAgentAccess::Ask => AgentAccessCeiling {
            folder_capability: ChatFolderCapability::Publish,
            auto_approve: false,
            scratch_allowed: true,
        },
        ChatAgentAccess::Own => AgentAccessCeiling {
            folder_capability: ChatFolderCapability::Publish,
            auto_approve: true,
            scratch_allowed: true,
        },
    }
}

const fn command_capability(support: &ProviderAuthoritySupport) -> ChatFolderCapability {
    if support.classified_publish {
        ChatFolderCapability::Publish
    } else {
        ChatFolderCapability::Execute
    }
}

const fn supports_own(support: &ProviderAuthoritySupport) -> bool {
    support.confined_commands && support.writable_root && support.network_boundary
}

const fn supports_ask(support: &ProviderAuthoritySupport) -> bool {
    support.writable_root && support.network_boundary && support.person_approved_commands
}

const fn supports_edit_only(support: &ProviderAuthoritySupport) -> bool {
    support.writable_root && support.deny_shell && support.network_boundary
}

const fn supports_read(support: &ProviderAuthoritySupport) -> bool {
    support.read_only_root && support.deny_shell && support.network_boundary
}

const fn behavior(
    requested: ChatAgentAccess,
    effective: Option<ChatEffectiveAgentAccess>,
    support: &ProviderAuthoritySupport,
) -> ResolvedAgentAccess {
    let (folder_capability, runtime_approval, scratch_allowed) = match effective {
        Some(ChatEffectiveAgentAccess::Own) => (
            command_capability(support),
            ChatRuntimeApprovalPolicy::AutoApprove,
            true,
        ),
        Some(ChatEffectiveAgentAccess::Ask) => (
            command_capability(support),
            ChatRuntimeApprovalPolicy::Ask,
            true,
        ),
        Some(ChatEffectiveAgentAccess::EditOnly) => (
            ChatFolderCapability::Edit,
            ChatRuntimeApprovalPolicy::Ask,
            false,
        ),
        Some(ChatEffectiveAgentAccess::Read) => (
            ChatFolderCapability::Read,
            ChatRuntimeApprovalPolicy::Ask,
            false,
        ),
        Some(ChatEffectiveAgentAccess::Talk) | None => (
            ChatFolderCapability::None,
            ChatRuntimeApprovalPolicy::Ask,
            false,
        ),
    };
    ResolvedAgentAccess {
        requested,
        effective,
        folder_capability,
        runtime_approval,
        scratch_allowed,
        scratch_approval: runtime_approval,
        downgrade_reason: None,
    }
}

const fn failure_reason(level: ChatAgentAccess) -> ChatAccessDowngradeReason {
    match level {
        ChatAgentAccess::Own => ChatAccessDowngradeReason::CannotConfineCommands,
        ChatAgentAccess::Ask => ChatAccessDowngradeReason::CannotAskBeforeCommands,
        ChatAgentAccess::Read => ChatAccessDowngradeReason::CannotReadOnly,
        ChatAgentAccess::Talk => ChatAccessDowngradeReason::CannotIsolate,
    }
}

/// Resolves what an agent does under `requested` with `support`, falling back through own, ask,
/// edit only, read, and talk to the widest behavior that is no wider than the request.
pub fn resolve_agent_access(
    requested: ChatAgentAccess,
    support: &ProviderAuthoritySupport,
) -> ResolvedAgentAccess {
    let wants = |level: ChatAgentAccess| requested >= level;
    let effective = if wants(ChatAgentAccess::Own) && supports_own(support) {
        Some(ChatEffectiveAgentAccess::Own)
    } else if wants(ChatAgentAccess::Ask) && supports_ask(support) {
        Some(ChatEffectiveAgentAccess::Ask)
    } else if wants(ChatAgentAccess::Ask) && supports_edit_only(support) {
        Some(ChatEffectiveAgentAccess::EditOnly)
    } else if wants(ChatAgentAccess::Read) && supports_read(support) {
        Some(ChatEffectiveAgentAccess::Read)
    } else if support.isolated_conversation {
        Some(ChatEffectiveAgentAccess::Talk)
    } else {
        None
    };
    let mut resolved = behavior(requested, effective, support);
    if effective != Some(ChatEffectiveAgentAccess::from(requested)) {
        resolved.downgrade_reason = Some(failure_reason(requested));
    }
    resolved
}

/// Returns whether frozen authority exceeds what `level` can ever allow.
pub fn exceeds_access(
    level: ChatAgentAccess,
    folder_capability: ChatFolderCapability,
    runtime_approval: ChatRuntimeApprovalPolicy,
    uses_scratch: bool,
) -> bool {
    let ceiling = access_ceiling(level);
    folder_capability.rank() > ceiling.folder_capability.rank()
        || (runtime_approval == ChatRuntimeApprovalPolicy::AutoApprove && !ceiling.auto_approve)
        || (uses_scratch && !ceiling.scratch_allowed)
}

/// Resolves a channel's access: the channel's own value, else its project's, else the app's.
pub fn effective_channel_access(
    channel: Option<ChatAgentAccess>,
    project: Option<ChatAgentAccess>,
    app: ChatAgentAccess,
) -> (ChatAgentAccess, ChatAgentAccessSource) {
    match (channel, project) {
        (Some(level), _) => (level, ChatAgentAccessSource::Channel),
        (None, Some(level)) => (level, ChatAgentAccessSource::Project),
        (None, None) => (app, ChatAgentAccessSource::App),
    }
}

/// Id of the single row of app agent settings.
pub const APP_AGENT_SETTINGS_ID: &str = "vault";

/// Returns an SQL scalar expression for the stored access level of the conversation whose id is
/// `conversation_id`, a column or parameter of the enclosing query, resolved like
/// [`effective_channel_access`]: the channel's own value, else its project's, else the app's.
/// A missing app settings row resolves to talk, so a damaged vault grants nothing.
pub fn effective_access_level_sql(conversation_id: &str) -> String {
    format!(
        "coalesce(
             (SELECT access_channel.agent_access FROM chat_channels access_channel
              WHERE access_channel.conversation_id = {conversation_id}),
             (SELECT access_project.agent_access
              FROM chat_conversations access_conversation
              JOIN chat_project_agent_settings access_project
                ON access_project.project_id = access_conversation.project_id
              WHERE access_conversation.id = {conversation_id}),
             (SELECT access_app.agent_access FROM chat_agent_app_settings access_app
              WHERE access_app.id = '{APP_AGENT_SETTINGS_ID}'),
             '{talk}'
         )",
        talk = ChatAgentAccess::Talk.as_str()
    )
}

/// Heading that introduces the app scope of instructions.
pub const APP_INSTRUCTIONS_HEADING: &str = "## Instructions for every channel";
/// Heading that introduces the project scope of instructions.
pub const PROJECT_INSTRUCTIONS_HEADING: &str = "## Project instructions";
/// Heading that introduces the channel scope of instructions.
pub const CHANNEL_INSTRUCTIONS_HEADING: &str = "## Channel instructions";

/// Joins the app, project, and channel instructions in that order under headings, skipping
/// scopes whose instructions are blank.
pub fn compose_instructions(app: &str, project: &str, channel: &str) -> String {
    [
        (APP_INSTRUCTIONS_HEADING, app),
        (PROJECT_INSTRUCTIONS_HEADING, project),
        (CHANNEL_INSTRUCTIONS_HEADING, channel),
    ]
    .into_iter()
    .filter_map(|(heading, text)| {
        let text = text.trim();
        (!text.is_empty()).then(|| format!("{heading}\n\n{text}"))
    })
    .collect::<Vec<_>>()
    .join("\n\n")
}

/// Returns the lowercase hex SHA-256 digest of composed instructions.
pub fn instructions_digest(instructions: &str) -> String {
    format!("{:x}", Sha256::digest(instructions.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn support_from_bits(bits: u16) -> ProviderAuthoritySupport {
        let bit = |index: u16| bits & (1 << index) != 0;
        ProviderAuthoritySupport {
            isolated_conversation: bit(0),
            internal_host_tools: true,
            deny_shell: bit(1),
            read_only_root: bit(2),
            writable_root: bit(3),
            confined_commands: bit(4),
            network_boundary: bit(5),
            classified_publish: bit(6),
            person_approved_commands: bit(7),
        }
    }

    fn full() -> ProviderAuthoritySupport {
        support_from_bits(u16::MAX)
    }

    #[test]
    fn full_support_works_at_every_requested_level() {
        let cases = [
            (ChatAgentAccess::Talk, ChatFolderCapability::None, false),
            (ChatAgentAccess::Read, ChatFolderCapability::Read, false),
            (ChatAgentAccess::Ask, ChatFolderCapability::Publish, true),
            (ChatAgentAccess::Own, ChatFolderCapability::Publish, true),
        ];
        for (level, folder, scratch) in cases {
            let resolved = resolve_agent_access(level, &full());
            assert_eq!(resolved.effective, Some(level.into()), "{level:?}");
            assert_eq!(resolved.folder_capability, folder, "{level:?}");
            assert_eq!(resolved.scratch_allowed, scratch, "{level:?}");
            assert_eq!(resolved.downgrade_reason, None, "{level:?}");
        }
        assert_eq!(
            resolve_agent_access(ChatAgentAccess::Own, &full()).runtime_approval,
            ChatRuntimeApprovalPolicy::AutoApprove
        );
        assert_eq!(
            resolve_agent_access(ChatAgentAccess::Ask, &full()).scratch_approval,
            ChatRuntimeApprovalPolicy::Ask
        );
    }

    #[test]
    fn unclassified_publish_stops_command_work_at_execute() {
        let support = ProviderAuthoritySupport {
            classified_publish: false,
            ..full()
        };
        for level in [ChatAgentAccess::Ask, ChatAgentAccess::Own] {
            assert_eq!(
                resolve_agent_access(level, &support).folder_capability,
                ChatFolderCapability::Execute
            );
        }
    }

    #[test]
    fn downgrades_name_why_the_requested_level_failed() {
        let cases = [
            (
                "no confinement asks instead",
                ChatAgentAccess::Own,
                ProviderAuthoritySupport {
                    confined_commands: false,
                    ..full()
                },
                Some(ChatEffectiveAgentAccess::Ask),
                ChatAccessDowngradeReason::CannotConfineCommands,
            ),
            (
                "no person approval edits without commands",
                ChatAgentAccess::Ask,
                ProviderAuthoritySupport {
                    person_approved_commands: false,
                    ..full()
                },
                Some(ChatEffectiveAgentAccess::EditOnly),
                ChatAccessDowngradeReason::CannotAskBeforeCommands,
            ),
            (
                "own without confinement or approval edits only",
                ChatAgentAccess::Own,
                ProviderAuthoritySupport {
                    confined_commands: false,
                    person_approved_commands: false,
                    ..full()
                },
                Some(ChatEffectiveAgentAccess::EditOnly),
                ChatAccessDowngradeReason::CannotConfineCommands,
            ),
            (
                "no writable root reads",
                ChatAgentAccess::Ask,
                ProviderAuthoritySupport {
                    writable_root: false,
                    ..full()
                },
                Some(ChatEffectiveAgentAccess::Read),
                ChatAccessDowngradeReason::CannotAskBeforeCommands,
            ),
            (
                "no read only root talks",
                ChatAgentAccess::Read,
                ProviderAuthoritySupport {
                    read_only_root: false,
                    ..full()
                },
                Some(ChatEffectiveAgentAccess::Talk),
                ChatAccessDowngradeReason::CannotReadOnly,
            ),
            (
                "no network boundary talks",
                ChatAgentAccess::Own,
                ProviderAuthoritySupport {
                    network_boundary: false,
                    ..full()
                },
                Some(ChatEffectiveAgentAccess::Talk),
                ChatAccessDowngradeReason::CannotConfineCommands,
            ),
            (
                "no isolation cannot talk",
                ChatAgentAccess::Talk,
                ProviderAuthoritySupport {
                    isolated_conversation: false,
                    ..full()
                },
                None,
                ChatAccessDowngradeReason::CannotIsolate,
            ),
        ];
        for (name, level, support, effective, reason) in cases {
            let resolved = resolve_agent_access(level, &support);
            assert_eq!(resolved.effective, effective, "{name}");
            assert_eq!(resolved.downgrade_reason, Some(reason), "{name}");
        }
    }

    #[test]
    fn edit_only_has_no_commands_and_no_scratch() {
        let resolved = resolve_agent_access(
            ChatAgentAccess::Ask,
            &ProviderAuthoritySupport {
                person_approved_commands: false,
                ..full()
            },
        );
        assert_eq!(resolved.folder_capability, ChatFolderCapability::Edit);
        assert!(!resolved.scratch_allowed);
        assert_eq!(resolved.runtime_approval, ChatRuntimeApprovalPolicy::Ask);
    }

    #[test]
    fn unworkable_agents_hold_no_authority() {
        let resolved = resolve_agent_access(ChatAgentAccess::Own, &support_from_bits(0));
        assert!(!resolved.can_work());
        assert_eq!(resolved.folder_capability, ChatFolderCapability::None);
        assert_eq!(resolved.runtime_approval, ChatRuntimeApprovalPolicy::Ask);
        assert!(!resolved.scratch_allowed);
    }

    #[test]
    fn resolution_never_exceeds_the_request_and_grows_with_it() {
        for bits in 0..256u16 {
            let support = support_from_bits(bits);
            let mut previous: Option<ResolvedAgentAccess> = None;
            for level in ChatAgentAccess::ALL {
                let resolved = resolve_agent_access(level, &support);
                if let Some(effective) = resolved.effective {
                    assert!(effective <= level.into(), "{bits:#b} {level:?}");
                }
                assert!(
                    !exceeds_access(
                        level,
                        resolved.folder_capability,
                        resolved.runtime_approval,
                        resolved.scratch_allowed
                    ),
                    "{bits:#b} {level:?}"
                );
                assert_eq!(
                    resolved.downgrade_reason.is_none(),
                    resolved.effective == Some(level.into()),
                    "{bits:#b} {level:?}"
                );
                if let Some(previous) = previous {
                    assert!(
                        previous.effective <= resolved.effective,
                        "{bits:#b} {level:?}"
                    );
                    assert!(
                        previous.folder_capability.rank() <= resolved.folder_capability.rank(),
                        "{bits:#b} {level:?}"
                    );
                }
                previous = Some(resolved);
            }
        }
    }

    #[test]
    fn exceeds_access_compares_against_the_level_ceiling() {
        use ChatFolderCapability as Folder;
        use ChatRuntimeApprovalPolicy::{Ask, AutoApprove};
        let cases = [
            (ChatAgentAccess::Talk, Folder::Read, Ask, false, true),
            (ChatAgentAccess::Talk, Folder::None, Ask, true, true),
            (ChatAgentAccess::Talk, Folder::None, Ask, false, false),
            (ChatAgentAccess::Read, Folder::Edit, Ask, false, true),
            (ChatAgentAccess::Read, Folder::Read, Ask, false, false),
            (
                ChatAgentAccess::Ask,
                Folder::Publish,
                AutoApprove,
                false,
                true,
            ),
            (ChatAgentAccess::Ask, Folder::Publish, Ask, true, false),
            (
                ChatAgentAccess::Own,
                Folder::Publish,
                AutoApprove,
                true,
                false,
            ),
        ];
        for (level, folder, approval, scratch, expected) in cases {
            assert_eq!(
                exceeds_access(level, folder, approval, scratch),
                expected,
                "{level:?} {folder:?} {approval:?} {scratch}"
            );
        }
    }

    #[test]
    fn channel_access_inherits_from_project_then_app() {
        use ChatAgentAccess::{Ask, Own, Read, Talk};
        assert_eq!(
            effective_channel_access(Some(Talk), Some(Own), Ask),
            (Talk, ChatAgentAccessSource::Channel)
        );
        assert_eq!(
            effective_channel_access(None, Some(Read), Own),
            (Read, ChatAgentAccessSource::Project)
        );
        assert_eq!(
            effective_channel_access(None, None, Own),
            (Own, ChatAgentAccessSource::App)
        );
    }

    #[test]
    fn instructions_join_scopes_in_order_and_skip_blank_ones() {
        assert_eq!(compose_instructions("", " \n", ""), "");
        assert_eq!(
            compose_instructions("Be brief.", "", "Review only.\n"),
            format!(
                "{APP_INSTRUCTIONS_HEADING}\n\nBe brief.\n\n{CHANNEL_INSTRUCTIONS_HEADING}\n\nReview only."
            )
        );
        let all = compose_instructions("a", "b", "c");
        let app = all.find(APP_INSTRUCTIONS_HEADING).unwrap();
        let project = all.find(PROJECT_INSTRUCTIONS_HEADING).unwrap();
        let channel = all.find(CHANNEL_INSTRUCTIONS_HEADING).unwrap();
        assert!(app < project && project < channel);
    }

    #[test]
    fn instructions_digest_is_stable_and_content_sensitive() {
        assert_eq!(
            instructions_digest(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_ne!(instructions_digest("a"), instructions_digest("b"));
    }
}
