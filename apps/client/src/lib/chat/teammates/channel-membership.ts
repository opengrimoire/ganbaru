import type {
  ChatAccessProfileRead,
  ChatTeammateAccessRead,
  ChatTeammateChannelAccess,
  ChatTeammateChannelAccessInput,
} from "$lib/chat/contracts";
import { capabilitiesForPreset } from "$lib/chat/teammates/access";

/** Returns the profile used when a teammate joins a channel without explicit access choices. */
export function conversationAccessProfile(
  profiles: readonly ChatAccessProfileRead[],
): ChatAccessProfileRead | null {
  return profiles.find((profile) => profile.builtinKey === "conversationOnly")
    ?? profiles[0]
    ?? null;
}

/** Builds the narrowest channel access: respond in the channel, no history, no folders. */
export function defaultChannelAccessInput(
  channelId: string,
  profile: ChatAccessProfileRead | null,
): ChatTeammateChannelAccessInput {
  return {
    channelId,
    accessProfileId: profile?.id ?? "",
    accessProfileRevision: profile?.latestRevision.revision ?? 0,
    capabilities: capabilitiesForPreset("isolatedResponder"),
    historyBoundary: { kind: "entire" },
    runtimeApprovalOverride: null,
    scratchRuntimeApprovalOverride: null,
    folderGrants: [],
  };
}

/** Converts persisted channel access into the replace request shape, dropping revoked folder grants. */
export function channelAccessInput(
  access: ChatTeammateChannelAccess,
): ChatTeammateChannelAccessInput {
  return {
    channelId: access.channelId,
    accessProfileId: access.accessProfileId,
    accessProfileRevision: access.accessProfileRevision,
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
  profile: ChatAccessProfileRead | null,
): ChatTeammateChannelAccessInput[] {
  const current = activeChannelAccessInputs(access);
  if (current.some((entry) => entry.channelId === channelId)) return current;
  return [...current, defaultChannelAccessInput(channelId, profile)];
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
