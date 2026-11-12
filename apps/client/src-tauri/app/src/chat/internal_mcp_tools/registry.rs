//! The closed set of internal host tools and the class that decides how each one is offered,
//! limited, and audited.

use super::delegation::{MAX_DELEGATION_DEPTH, MAX_OPEN_CHILDREN_PER_ASSIGNMENT};
use crate::chat::internal_mcp::InternalMcpRunScope;
use ganbaru_chat_contracts::models::ChatFolderCapability;

/// One internal MCP host tool. [`HostTool::name`] and [`HostTool::from_name`] are the only
/// mapping between a tool and its wire name.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum HostTool {
    ListReferencedChannels,
    SearchChannelMessages,
    ReadChannelMessages,
    ListAuthorizedRoots,
    SearchWorkspacePaths,
    ReadWorkspaceFile,
    WriteWorkspaceFile,
    CreateWorkspaceFile,
    PatchWorkspaceFile,
    DeleteWorkspaceFile,
    ReadReplyPointer,
    DelegateAssignment,
    Ask,
    Report,
    RequestSecret,
    SecretStatus,
}

impl HostTool {
    /// Every host tool, in the order the tool list presents them.
    pub(crate) const ALL: [Self; 16] = [
        Self::ListReferencedChannels,
        Self::SearchChannelMessages,
        Self::ReadChannelMessages,
        Self::ListAuthorizedRoots,
        Self::SearchWorkspacePaths,
        Self::ReadWorkspaceFile,
        Self::WriteWorkspaceFile,
        Self::CreateWorkspaceFile,
        Self::PatchWorkspaceFile,
        Self::DeleteWorkspaceFile,
        Self::ReadReplyPointer,
        Self::DelegateAssignment,
        Self::Ask,
        Self::Report,
        Self::RequestSecret,
        Self::SecretStatus,
    ];

    /// The wire name a provider calls the tool by.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::ListReferencedChannels => "chat_list_referenced_channels",
            Self::SearchChannelMessages => "chat_search_channel_messages",
            Self::ReadChannelMessages => "chat_read_channel_messages",
            Self::ListAuthorizedRoots => "chat_list_authorized_roots",
            Self::SearchWorkspacePaths => "chat_search_workspace_paths",
            Self::ReadWorkspaceFile => "chat_read_workspace_file",
            Self::WriteWorkspaceFile => "chat_write_workspace_file",
            Self::CreateWorkspaceFile => "chat_create_workspace_file",
            Self::PatchWorkspaceFile => "chat_patch_workspace_file",
            Self::DeleteWorkspaceFile => "chat_delete_workspace_file",
            Self::ReadReplyPointer => "chat_read_reply_pointer",
            Self::DelegateAssignment => "chat_delegate_assignment",
            Self::Ask => "chat_ask",
            Self::Report => "chat_report",
            Self::RequestSecret => "chat_request_secret",
            Self::SecretStatus => "chat_secret_status",
        }
    }

    /// The tool a wire name names, or `None` for any other name.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    pub(crate) const fn class(self) -> HostToolClass {
        match self {
            Self::ListReferencedChannels
            | Self::SearchChannelMessages
            | Self::ReadChannelMessages
            | Self::ListAuthorizedRoots
            | Self::SearchWorkspacePaths
            | Self::ReadWorkspaceFile
            | Self::ReadReplyPointer => HostToolClass::Read,
            Self::WriteWorkspaceFile | Self::CreateWorkspaceFile | Self::PatchWorkspaceFile => {
                HostToolClass::Edit
            }
            Self::DeleteWorkspaceFile => HostToolClass::Destructive,
            Self::DelegateAssignment => HostToolClass::Delegate,
            Self::Ask => HostToolClass::Ask,
            Self::Report => HostToolClass::Report,
            Self::RequestSecret | Self::SecretStatus => HostToolClass::Secret,
        }
    }

    /// Whether the host has a handler for the tool. A tool without one is never offered and
    /// every call to it is denied.
    pub(super) const fn implemented(self) -> bool {
        match self {
            Self::ListReferencedChannels
            | Self::SearchChannelMessages
            | Self::ReadChannelMessages
            | Self::ListAuthorizedRoots
            | Self::SearchWorkspacePaths
            | Self::ReadWorkspaceFile
            | Self::WriteWorkspaceFile
            | Self::CreateWorkspaceFile
            | Self::PatchWorkspaceFile
            | Self::DeleteWorkspaceFile
            | Self::DelegateAssignment => true,
            Self::ReadReplyPointer
            | Self::Ask
            | Self::Report
            | Self::RequestSecret
            | Self::SecretStatus => false,
        }
    }

    /// Whether the run's tool list carries the tool. A call to a tool the run is not offered
    /// is denied even when the provider names it.
    pub(crate) fn offered(self, scope: &InternalMcpRunScope) -> bool {
        self.implemented() && self.class().offered(scope)
    }

    /// Whether an organizational run calls the tool without a provider approval prompt. Only
    /// implemented tools of the read, ask, and report classes qualify, and those classes are
    /// offered to every run, so the list does not depend on the run's scope.
    pub(crate) const fn pre_approved(self) -> bool {
        self.implemented() && self.class().pre_approved()
    }
}

/// How a host tool affects the vault, which decides whether it is offered, how many calls one
/// authorization revision may make, and how an unknown outcome is audited.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum HostToolClass {
    /// Reads authorized channel and folder context.
    Read,
    /// Writes, creates, or patches an authorized file.
    Edit,
    /// Deletes an authorized file.
    Destructive,
    /// Creates a delegated child assignment.
    Delegate,
    /// Asks the person who assigned the work.
    Ask,
    /// Posts a plan, replan, or result report.
    Report,
    /// Requests a secret through the private card.
    Secret,
    /// Proposes a scheduled message. No tool is registered in this class.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the class is reserved and no tool is registered in it"
        )
    )]
    Schedule,
}

impl HostToolClass {
    /// Every class, in the order the audit column's CHECK lists them.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 8] = [
        Self::Read,
        Self::Edit,
        Self::Destructive,
        Self::Delegate,
        Self::Ask,
        Self::Report,
        Self::Secret,
        Self::Schedule,
    ];

    /// The value stored in `chat_host_tool_invocations.tool_class`.
    pub(crate) const fn wire(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Destructive => "destructive",
            Self::Delegate => "delegate",
            Self::Ask => "ask",
            Self::Report => "report",
            Self::Secret => "secret",
            Self::Schedule => "schedule",
        }
    }

    /// Calls one authorization revision may make in this class. Each class has its own limit,
    /// so a teammate that spent its reads can still ask or report.
    pub(crate) const fn call_limit(self) -> u32 {
        match self {
            Self::Read | Self::Edit => 20,
            Self::Destructive | Self::Report => 8,
            Self::Delegate => MAX_OPEN_CHILDREN_PER_ASSIGNMENT,
            Self::Ask => 3,
            Self::Secret => 6,
            Self::Schedule => 2,
        }
    }

    /// Whether a call can change state outside the reply, so the audit reserves the full
    /// response budget before it runs and records a failed call as an unknown outcome.
    pub(crate) const fn mutates_outside_reply(self) -> bool {
        matches!(
            self,
            Self::Edit | Self::Destructive | Self::Delegate | Self::Secret
        )
    }

    /// Whether a call can run without a provider approval prompt, because it only reads
    /// authorized context, asks the assigner, or reports. Every other class keeps the approval
    /// the run's frozen approval policy gives it.
    pub(crate) const fn pre_approved(self) -> bool {
        matches!(self, Self::Read | Self::Ask | Self::Report)
    }

    /// Whether the run's tool list carries tools of this class. Edit and destructive tools need
    /// a folder authorized at edit rank or higher. Delegation needs a run below the depth limit
    /// in a channel with another participating teammate. No schedule tool is offered.
    fn offered(self, scope: &InternalMcpRunScope) -> bool {
        match self {
            Self::Read | Self::Ask | Self::Report | Self::Secret => true,
            Self::Edit | Self::Destructive => scope
                .folder_sources
                .iter()
                .any(|source| source.capability.rank() >= ChatFolderCapability::Edit.rank()),
            Self::Delegate => {
                scope.delegation_depth < MAX_DELEGATION_DEPTH && scope.delegation_target_available
            }
            Self::Schedule => false,
        }
    }
}
