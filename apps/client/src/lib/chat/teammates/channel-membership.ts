import type {
  ChatAccessProfileRead,
  ChatAiTeammateRead,
  ChatTeammateAccessRead,
  ChatTeammateChannelAccess,
  ChatTeammateChannelAccessInput,
  ProviderAuthoritySupport,
  ProviderInstanceRead,
  SafetyMode,
} from "$lib/chat/contracts";
import { providerExecutesWithAsk } from "$lib/chat/composer/provider-authority";
import { capabilitiesForPreset } from "$lib/chat/teammates/access";

/** Access facts a channel offers a teammate that joins it. */
export interface ChannelAccessDefaults {
  profiles: readonly ChatAccessProfileRead[];
  /** The channel project's preferred working folder, or null when the project has none. */
  projectWorkingFolderId: string | null;
}

/** What a teammate's provider and policy let the send gate enforce for commands. */
export interface TeammateCommandSupport {
  authority: ProviderAuthoritySupport | null;
  safetyMode: SafetyMode | null;
}

/** Returns the profile used when a teammate joins a channel without explicit access choices. */
export function conversationAccessProfile(
  profiles: readonly ChatAccessProfileRead[],
): ChatAccessProfileRead | null {
  return profiles.find((profile) => profile.builtinKey === "conversationOnly")
    ?? profiles[0]
    ?? null;
}

/** Returns the built-in profile whose ceiling allows running commands in a folder, or null. */
export function executeAccessProfile(
  profiles: readonly ChatAccessProfileRead[],
): ChatAccessProfileRead | null {
  return profiles.find((profile) => (
    profile.builtinKey === "buildAndTest"
      && profile.archivedAt === null
      && profile.latestRevision.maximumFolderCapability === "execute"
  )) ?? null;
}

/** Reads the command support of a teammate from its policy and its provider's last probe. */
export function teammateCommandSupport(
  teammate: Pick<ChatAiTeammateRead, "latestPolicy"> | null,
  providerInstances: readonly ProviderInstanceRead[],
): TeammateCommandSupport {
  const policy = teammate?.latestPolicy ?? null;
  const provider = providerInstances.find((entry) => (
    entry.configuration.instanceId === policy?.providerInstanceId
  ));
  return {
    authority: provider?.lastProbe?.authoritySupport ?? null,
    safetyMode: policy?.safetyMode ?? null,
  };
}

/**
 * Builds the access a teammate gets when it joins a channel.
 *
 * The teammate responds in the channel and works in the channel project's folder at Execute with
 * runtime approval Ask, so every command still reaches the person. When the project has no folder,
 * the Execute profile is missing, or the provider cannot keep commands person-approved, the
 * teammate joins in conversation only instead of with a grant the send gate would refuse.
 */
export function defaultChannelAccessInput(
  channelId: string,
  defaults: ChannelAccessDefaults,
  teammate: TeammateCommandSupport,
): ChatTeammateChannelAccessInput {
  const executeProfile = executeAccessProfile(defaults.profiles);
  const canExecute = defaults.projectWorkingFolderId !== null
    && executeProfile !== null
    && teammate.authority !== null
    && providerExecutesWithAsk(teammate.authority, teammate.safetyMode);
  const profile = canExecute ? executeProfile : conversationAccessProfile(defaults.profiles);
  return {
    channelId,
    accessProfileId: profile?.id ?? "",
    accessProfileRevisionId: profile?.latestRevision.id ?? "",
    capabilities: capabilitiesForPreset("isolatedResponder"),
    historyBoundary: { kind: "entire" },
    runtimeApprovalOverride: null,
    scratchRuntimeApprovalOverride: null,
    folderGrants: canExecute && defaults.projectWorkingFolderId !== null
      ? [{
        workingFolderId: defaults.projectWorkingFolderId,
        capability: "execute",
        isDefault: true,
        runtimeApprovalOverride: "ask",
      }]
      : [],
  };
}

/** Converts persisted channel access into the replace request shape, dropping revoked folder grants. */
export function channelAccessInput(
  access: ChatTeammateChannelAccess,
): ChatTeammateChannelAccessInput {
  return {
    channelId: access.channelId,
    accessProfileId: access.accessProfileId,
    accessProfileRevisionId: access.accessProfileRevisionId,
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

/** Returns the teammate's active channel access as replace request inputs. */
export function activeChannelAccessInputs(
  access: ChatTeammateAccessRead,
): ChatTeammateChannelAccessInput[] {
  return access.channels
    .filter((entry) => entry.removedAt === null)
    .map(channelAccessInput);
}

/** Returns the teammate's access with one channel removed, leaving every other channel untouched. */
export function channelAccessWithoutChannel(
  access: ChatTeammateAccessRead,
  channelId: string,
): ChatTeammateChannelAccessInput[] {
  return activeChannelAccessInputs(access).filter((entry) => entry.channelId !== channelId);
}

/** Returns the teammate's access with a channel added at the default access, or unchanged when already present. */
export function channelAccessWithChannel(
  access: ChatTeammateAccessRead,
  channelId: string,
  defaults: ChannelAccessDefaults,
  teammate: TeammateCommandSupport,
): ChatTeammateChannelAccessInput[] {
  const current = activeChannelAccessInputs(access);
  if (current.some((entry) => entry.channelId === channelId)) return current;
  return [...current, defaultChannelAccessInput(channelId, defaults, teammate)];
}

export interface ChannelMemberChanges {
  addedTeammateIds: string[];
  removedTeammateIds: string[];
}

/** Resolves pending member edits against the current roster so repeated add and remove clicks cancel out. */
export function resolveChannelMemberChanges(
  currentTeammateIds: ReadonlySet<string>,
  selectedTeammateIds: ReadonlySet<string>,
): ChannelMemberChanges {
  return {
    addedTeammateIds: [...selectedTeammateIds].filter((id) => !currentTeammateIds.has(id)),
    removedTeammateIds: [...currentTeammateIds].filter((id) => !selectedTeammateIds.has(id)),
  };
}
