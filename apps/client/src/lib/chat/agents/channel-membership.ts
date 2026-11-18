import type {
  ChatAiAgentRead,
  ChatAgentAccessRead,
  ChatAgentChannelAccess,
  ChatAgentChannelAccessInput,
  ProviderAuthoritySupport,
  ProviderInstanceRead,
  SafetyMode,
} from "$lib/chat/contracts";
import { providerExecutesWithAsk } from "$lib/chat/composer/provider-authority";
import { capabilitiesForPreset } from "$lib/chat/agents/access";

/** Access facts a channel offers an agent that joins it. */
export interface ChannelAccessDefaults {
  /** The channel project's preferred working folder, or null when the project has none. */
  projectWorkingFolderId: string | null;
}

/** What an agent's provider and policy let the send gate enforce for commands. */
export interface AgentCommandSupport {
  authority: ProviderAuthoritySupport | null;
  safetyMode: SafetyMode | null;
}

/** Reads the command support of an agent from its policy and its provider's last probe. */
export function agentCommandSupport(
  agent: Pick<ChatAiAgentRead, "latestPolicy"> | null,
  providerInstances: readonly ProviderInstanceRead[],
): AgentCommandSupport {
  const policy = agent?.latestPolicy ?? null;
  const provider = providerInstances.find((entry) => (
    entry.configuration.instanceId === policy?.providerInstanceId
  ));
  return {
    authority: provider?.lastProbe?.authoritySupport ?? null,
    safetyMode: policy?.safetyMode ?? null,
  };
}

/**
 * Builds the access an agent gets when it joins a channel.
 *
 * The agent reads the channel history from the moment it joins, takes assignments, and works in the channel project's folder at Execute
 * with runtime approval Ask, so every command still reaches the person. When the project has no folder or the
 * provider cannot keep commands person-approved, the agent joins without file access instead of with a grant the
 * send gate would refuse.
 */
export function defaultChannelAccessInput(
  channelId: string,
  defaults: ChannelAccessDefaults,
  agent: AgentCommandSupport,
): ChatAgentChannelAccessInput {
  const projectWorkingFolderId = defaults.projectWorkingFolderId;
  const canExecute = projectWorkingFolderId !== null
    && agent.authority !== null
    && providerExecutesWithAsk(agent.authority, agent.safetyMode);
  return {
    channelId,
    capabilities: capabilitiesForPreset("collaborator"),
    historyBoundary: { kind: "fromGrant" },
    runtimeApprovalOverride: null,
    scratchRuntimeApprovalOverride: null,
    folderGrants: canExecute
      ? [{
        workingFolderId: projectWorkingFolderId,
        capability: "execute",
        isDefault: true,
        runtimeApprovalOverride: "ask",
      }]
      : [],
  };
}

/** Converts persisted channel access into the replace request shape, dropping revoked folder grants. */
export function channelAccessInput(
  access: ChatAgentChannelAccess,
): ChatAgentChannelAccessInput {
  return {
    channelId: access.channelId,
    capabilities: { ...access.capabilities },
    historyBoundary: access.historyBoundary.kind === "entire"
      ? { kind: "entire" }
      : { kind: "fromGrant" },
    runtimeApprovalOverride: access.runtimeApprovalOverride,
    scratchRuntimeApprovalOverride: access.scratchRuntimeApprovalOverride,
    folderGrants: access.folderGrants
      .filter((grant) => grant.revokedAt === null)
      .map((grant) => ({
        workingFolderId: grant.workingFolderId,
        capability: grant.capability,
        isDefault: grant.isDefault,
        runtimeApprovalOverride: grant.runtimeApprovalOverride,
      })),
  };
}

/** Returns the agent's active channel access as replace request inputs. */
export function activeChannelAccessInputs(
  access: ChatAgentAccessRead,
): ChatAgentChannelAccessInput[] {
  return access.channels
    .filter((entry) => entry.removedAt === null)
    .map(channelAccessInput);
}

/** Returns the agent's access with one channel removed, leaving every other channel untouched. */
export function channelAccessWithoutChannel(
  access: ChatAgentAccessRead,
  channelId: string,
): ChatAgentChannelAccessInput[] {
  return activeChannelAccessInputs(access).filter((entry) => entry.channelId !== channelId);
}

/** Returns the agent's access with a channel added at the default access, or unchanged when already present. */
export function channelAccessWithChannel(
  access: ChatAgentAccessRead,
  channelId: string,
  defaults: ChannelAccessDefaults,
  agent: AgentCommandSupport,
): ChatAgentChannelAccessInput[] {
  const current = activeChannelAccessInputs(access);
  if (current.some((entry) => entry.channelId === channelId)) return current;
  return [...current, defaultChannelAccessInput(channelId, defaults, agent)];
}

export interface ChannelMemberChanges {
  addedAgentIds: string[];
  removedAgentIds: string[];
}

/** Resolves pending member edits against the current roster so repeated add and remove clicks cancel out. */
export function resolveChannelMemberChanges(
  currentAgentIds: ReadonlySet<string>,
  selectedAgentIds: ReadonlySet<string>,
): ChannelMemberChanges {
  return {
    addedAgentIds: [...selectedAgentIds].filter((id) => !currentAgentIds.has(id)),
    removedAgentIds: [...currentAgentIds].filter((id) => !selectedAgentIds.has(id)),
  };
}
