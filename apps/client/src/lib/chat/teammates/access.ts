import type {
  ChatChannelCapabilities,
  ChatFolderCapability,
  ChatHistoryBoundary,
  ChatRuntimeApprovalPolicy,
  ChatTeammateChannelAccessInput,
} from "$lib/chat/contracts";

export type ChatChannelCapabilityPreset =
  | "contextSource"
  | "isolatedResponder"
  | "collaborator"
  | "custom";

export type SelectionState = "none" | "some" | "all";

export interface ChatTeammateAccessConfirmationImpact {
  historyChannelIds: string[];
  entireHistoryChannelIds: string[];
  executeFolderIds: string[];
  publishFolderIds: string[];
  removedChannelIds: string[];
}

const CAPABILITY_RANK: Record<ChatFolderCapability, number> = {
  none: 0,
  read: 1,
  edit: 2,
  execute: 3,
  publish: 4,
};

/** Returns whether a requested folder capability fits inside a profile ceiling. */
export function folderCapabilityFits(
  requested: ChatFolderCapability,
  ceiling: ChatFolderCapability,
): boolean {
  return CAPABILITY_RANK[requested] <= CAPABILITY_RANK[ceiling];
}

/** Returns a deterministic preset name for independent channel capabilities. */
export function channelCapabilityPreset(
  capabilities: ChatChannelCapabilities,
): ChatChannelCapabilityPreset {
  if (capabilities.readHistory && capabilities.participate) return "collaborator";
  if (capabilities.readHistory) return "contextSource";
  if (capabilities.participate) return "isolatedResponder";
  return "custom";
}

/** Returns the capability pair represented by a quick preset. */
export function capabilitiesForPreset(
  preset: Exclude<ChatChannelCapabilityPreset, "custom">,
): ChatChannelCapabilities {
  if (preset === "contextSource") return { readHistory: true, participate: false };
  if (preset === "isolatedResponder") return { readHistory: false, participate: true };
  return { readHistory: true, participate: true };
}

/** Resolves an inherited runtime approval without letting an absent override widen it. */
export function resolveRuntimeApproval(
  teammateDefault: ChatRuntimeApprovalPolicy,
  channelOverride: ChatRuntimeApprovalPolicy | null,
  folderOverride: ChatRuntimeApprovalPolicy | null,
): { policy: ChatRuntimeApprovalPolicy; source: "teammate" | "channel" | "folder" } {
  if (folderOverride) return { policy: folderOverride, source: "folder" };
  if (channelOverride) return { policy: channelOverride, source: "channel" };
  return { policy: teammateDefault, source: "teammate" };
}

/** Calculates the native tri-state value for a bounded group of selectable channels. */
export function selectionState(ids: readonly string[], selectedIds: ReadonlySet<string>): SelectionState {
  if (ids.length === 0 || ids.every((id) => !selectedIds.has(id))) return "none";
  if (ids.every((id) => selectedIds.has(id))) return "all";
  return "some";
}

/** Updates a bounded selection. Future channels are intentionally unaffected. */
export function toggleSelectionGroup(
  ids: readonly string[],
  selectedIds: ReadonlySet<string>,
  selected: boolean,
): Set<string> {
  const next = new Set(selectedIds);
  for (const id of ids) {
    if (selected) next.add(id);
    else next.delete(id);
  }
  return next;
}

/** Applies one channel behavior to an exact, already selected scope. */
export function applyChannelPresetToScope(
  channels: readonly ChatTeammateChannelAccessInput[],
  channelIds: ReadonlySet<string>,
  preset: Exclude<ChatChannelCapabilityPreset, "custom">,
): ChatTeammateChannelAccessInput[] {
  return channels.map((channel) => channelIds.has(channel.channelId)
    ? {
        ...channel,
        capabilities: capabilitiesForPreset(preset),
        historyBoundary: { kind: "entire" },
      }
    : channel);
}

/** Applies one profile ceiling to an exact scope and removes grants above it. */
export function applyAccessProfileToScope(
  channels: readonly ChatTeammateChannelAccessInput[],
  channelIds: ReadonlySet<string>,
  profile: {
    id: string;
    revision: number;
    maximumFolderCapability: ChatFolderCapability;
  },
): ChatTeammateChannelAccessInput[] {
  return channels.map((channel) => channelIds.has(channel.channelId)
    ? {
        ...channel,
        accessProfileId: profile.id,
        accessProfileRevision: profile.revision,
        folderGrants: channel.folderGrants.filter((grant) => folderCapabilityFits(
          grant.capability,
          profile.maximumFolderCapability,
        )),
      }
    : channel);
}

/** Creates a stable access snapshot for dirty checks and optimistic replacement. */
export function teammateAccessDraftSnapshot(
  channels: readonly ChatTeammateChannelAccessInput[],
): string {
  return JSON.stringify(
    [...channels]
      .map((channel) => ({
        ...channel,
        capabilities: {
          readHistory: channel.capabilities.readHistory,
          participate: channel.capabilities.participate,
        },
        historyBoundary: normalizeHistoryBoundary(channel.historyBoundary),
        folderGrants: [...channel.folderGrants]
          .map((grant) => ({ ...grant }))
          .sort((left, right) => left.workingFolderId.localeCompare(right.workingFolderId)),
      }))
      .sort((left, right) => left.channelId.localeCompare(right.channelId)),
  );
}

/**
 * Returns only access changes that benefit from a separate human confirmation.
 * Exact responder membership and participation changes remain ordinary draft edits.
 */
export function teammateAccessConfirmationImpact(
  current: readonly ChatTeammateChannelAccessInput[],
  proposed: readonly ChatTeammateChannelAccessInput[],
): ChatTeammateAccessConfirmationImpact {
  const currentByChannel = new Map(current.map((channel) => [channel.channelId, channel]));
  const proposedByChannel = new Map(proposed.map((channel) => [channel.channelId, channel]));
  const historyChannelIds = new Set<string>();
  const entireHistoryChannelIds = new Set<string>();
  const executeFolderIds = new Set<string>();
  const publishFolderIds = new Set<string>();

  for (const channel of proposed) {
    const prior = currentByChannel.get(channel.channelId);
    if (channel.capabilities.readHistory && !prior?.capabilities.readHistory) {
      if (channel.historyBoundary.kind === "entire") {
        entireHistoryChannelIds.add(channel.channelId);
      } else {
        historyChannelIds.add(channel.channelId);
      }
    } else if (
      channel.capabilities.readHistory
      && prior?.capabilities.readHistory
      && prior.historyBoundary.kind === "fromGrant"
      && channel.historyBoundary.kind === "entire"
    ) {
      entireHistoryChannelIds.add(channel.channelId);
    }

    const priorGrants = new Map(
      prior?.folderGrants.map((grant) => [grant.workingFolderId, grant]) ?? [],
    );
    for (const grant of channel.folderGrants) {
      const priorCapability = priorGrants.get(grant.workingFolderId)?.capability ?? "none";
      if (grant.capability === "publish" && !folderCapabilityFits("publish", priorCapability)) {
        publishFolderIds.add(grant.workingFolderId);
      } else if (
        grant.capability === "execute"
        && !folderCapabilityFits("execute", priorCapability)
      ) {
        executeFolderIds.add(grant.workingFolderId);
      }
    }
  }

  return {
    historyChannelIds: [...historyChannelIds],
    entireHistoryChannelIds: [...entireHistoryChannelIds],
    executeFolderIds: [...executeFolderIds],
    publishFolderIds: [...publishFolderIds],
    removedChannelIds: current
      .filter((channel) => !proposedByChannel.has(channel.channelId))
      .map((channel) => channel.channelId),
  };
}

/** Returns whether the impact contains a change that deserves separate confirmation. */
export function teammateAccessNeedsConfirmation(
  impact: ChatTeammateAccessConfirmationImpact,
): boolean {
  return impact.historyChannelIds.length > 0
    || impact.entireHistoryChannelIds.length > 0
    || impact.executeFolderIds.length > 0
    || impact.publishFolderIds.length > 0
    || impact.removedChannelIds.length > 0;
}

function normalizeHistoryBoundary(boundary: ChatHistoryBoundary): ChatHistoryBoundary {
  return boundary.kind === "entire"
    ? { kind: "entire" }
    : boundary.lowerOrdinal === undefined
      ? { kind: "fromGrant" }
      : { kind: "fromGrant", lowerOrdinal: boundary.lowerOrdinal };
}

/** Returns all configuration errors that must block an atomic access save. */
export function teammateAccessDraftErrors(
  channels: readonly ChatTeammateChannelAccessInput[],
  profileCeilings: ReadonlyMap<string, ChatFolderCapability>,
): string[] {
  const errors: string[] = [];
  const channelIds = new Set<string>();
  for (const channel of channels) {
    if (channelIds.has(channel.channelId)) errors.push(`Duplicate channel ${channel.channelId}`);
    channelIds.add(channel.channelId);
    if (!channel.capabilities.readHistory && channel.historyBoundary.kind !== "entire") {
      errors.push(`Channel ${channel.channelId} has a history boundary without history access`);
    }
    const ceiling = profileCeilings.get(channel.accessProfileId);
    if (!ceiling) errors.push(`Channel ${channel.channelId} uses an unavailable access profile`);
    const folderIds = new Set<string>();
    let defaultCount = 0;
    for (const grant of channel.folderGrants) {
      if (folderIds.has(grant.workingFolderId)) {
        errors.push(`Channel ${channel.channelId} grants folder ${grant.workingFolderId} twice`);
      }
      folderIds.add(grant.workingFolderId);
      if (grant.isDefault) defaultCount += 1;
      if (ceiling && !folderCapabilityFits(grant.capability, ceiling)) {
        errors.push(`Folder ${grant.workingFolderId} exceeds its access profile`);
      }
    }
    if (defaultCount > 1) errors.push(`Channel ${channel.channelId} has multiple default folders`);
  }
  return errors;
}
