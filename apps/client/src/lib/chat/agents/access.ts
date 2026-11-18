import type {
  ChatChannelCapabilities,
  ChatFolderCapability,
  ChatHistoryBoundary,
  ChatRuntimeApprovalPolicy,
  ChatAgentChannelAccessInput,
} from "$lib/chat/contracts";

export type ChatChannelCapabilityPreset =
  | "contextSource"
  | "isolatedResponder"
  | "collaborator"
  | "custom";

export type SelectionState = "none" | "some" | "all";

export interface ChatAgentAccessConfirmationImpact {
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

/** File access levels a folder grant toggles, from least to most authority. Each level includes the ones before it. */
export const CHAT_FOLDER_ACCESS_LEVELS = ["read", "edit", "execute", "publish"] as const;
export type ChatFolderAccessLevel = (typeof CHAT_FOLDER_ACCESS_LEVELS)[number];

const LEVEL_BELOW: Record<ChatFolderAccessLevel, ChatFolderCapability> = {
  read: "none",
  edit: "read",
  execute: "edit",
  publish: "execute",
};

/** Returns whether a folder capability includes the given capability level. */
export function folderCapabilityIncludes(
  capability: ChatFolderCapability,
  level: ChatFolderCapability,
): boolean {
  return CAPABILITY_RANK[capability] >= CAPABILITY_RANK[level];
}

/** Returns which file access toggles are on for a folder capability, keyed by level. */
export function folderAccessToggles(
  capability: ChatFolderCapability,
): Record<ChatFolderAccessLevel, boolean> {
  return {
    read: folderCapabilityIncludes(capability, "read"),
    edit: folderCapabilityIncludes(capability, "edit"),
    execute: folderCapabilityIncludes(capability, "execute"),
    publish: folderCapabilityIncludes(capability, "publish"),
  };
}

/**
 * Returns the folder capability after turning one file access toggle on or off.
 *
 * Toggles are cumulative: turning a level on also turns on every lower level, and turning a level off also turns
 * off every higher level. Turning every level off yields `none`.
 */
export function folderCapabilityWithToggle(
  capability: ChatFolderCapability,
  level: ChatFolderAccessLevel,
  enabled: boolean,
): ChatFolderCapability {
  const includes = folderCapabilityIncludes(capability, level);
  if (enabled) return includes ? capability : level;
  return includes ? LEVEL_BELOW[level] : capability;
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
  agentDefault: ChatRuntimeApprovalPolicy,
  channelOverride: ChatRuntimeApprovalPolicy | null,
  folderOverride: ChatRuntimeApprovalPolicy | null,
): { policy: ChatRuntimeApprovalPolicy; source: "agent" | "channel" | "folder" } {
  if (folderOverride) return { policy: folderOverride, source: "folder" };
  if (channelOverride) return { policy: channelOverride, source: "channel" };
  return { policy: agentDefault, source: "agent" };
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
  channels: readonly ChatAgentChannelAccessInput[],
  channelIds: ReadonlySet<string>,
  preset: Exclude<ChatChannelCapabilityPreset, "custom">,
): ChatAgentChannelAccessInput[] {
  return channels.map((channel) => channelIds.has(channel.channelId)
    ? {
        ...channel,
        capabilities: capabilitiesForPreset(preset),
        historyBoundary: { kind: "entire" },
      }
    : channel);
}

/** Creates a stable access snapshot for dirty checks and optimistic replacement. */
export function agentAccessDraftSnapshot(
  channels: readonly ChatAgentChannelAccessInput[],
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
export function agentAccessConfirmationImpact(
  current: readonly ChatAgentChannelAccessInput[],
  proposed: readonly ChatAgentChannelAccessInput[],
): ChatAgentAccessConfirmationImpact {
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
      if (grant.capability === "publish" && !folderCapabilityIncludes(priorCapability, "publish")) {
        publishFolderIds.add(grant.workingFolderId);
      } else if (
        grant.capability === "execute"
        && !folderCapabilityIncludes(priorCapability, "execute")
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
export function agentAccessNeedsConfirmation(
  impact: ChatAgentAccessConfirmationImpact,
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
    : boundary.lowerOrderKey === undefined
      ? { kind: "fromGrant" }
      : { kind: "fromGrant", lowerOrderKey: boundary.lowerOrderKey };
}

/** Returns all configuration errors that must block an atomic access save. */
export function agentAccessDraftErrors(
  channels: readonly ChatAgentChannelAccessInput[],
): string[] {
  const errors: string[] = [];
  const channelIds = new Set<string>();
  for (const channel of channels) {
    if (channelIds.has(channel.channelId)) errors.push(`Duplicate channel ${channel.channelId}`);
    channelIds.add(channel.channelId);
    if (!channel.capabilities.readHistory && channel.historyBoundary.kind !== "entire") {
      errors.push(`Channel ${channel.channelId} has a history boundary without history access`);
    }
    const folderIds = new Set<string>();
    let defaultCount = 0;
    for (const grant of channel.folderGrants) {
      if (folderIds.has(grant.workingFolderId)) {
        errors.push(`Channel ${channel.channelId} grants folder ${grant.workingFolderId} twice`);
      }
      folderIds.add(grant.workingFolderId);
      if (grant.isDefault) defaultCount += 1;
    }
    if (defaultCount > 1) errors.push(`Channel ${channel.channelId} has multiple default folders`);
  }
  return errors;
}

/**
 * Returns the folder a channel membership's work runs in when a post names none, matching the native target order:
 * the default grant, else the channel project's working folder when it is granted, else the only granted folder.
 * Returns `null` when the work runs conversation only. Grants without a capability never become the target.
 */
export function impliedFolderGrantId(
  grants: readonly Pick<ChatAgentChannelAccessInput["folderGrants"][number], "workingFolderId" | "capability" | "isDefault">[],
  projectWorkingFolderId: string | null,
): string | null {
  const candidates = grants.filter((grant) => grant.capability !== "none");
  const implied = candidates.find((grant) => grant.isDefault)
    ?? candidates.find((grant) => grant.workingFolderId === projectWorkingFolderId)
    ?? (candidates.length === 1 ? candidates[0] : undefined);
  return implied?.workingFolderId ?? null;
}
