import type {
  ChatRuntimeApprovalPolicy,
  ChatTeammateChannelAccessInput,
  ModelOptionSelection,
  SafetyMode,
  VersionedJson,
} from "$lib/chat/contracts";
import { teammateAccessDraftSnapshot } from "./access";

export interface ChatTeammateStudioProfileDraft {
  displayName: string;
  role: string;
  instructions: string;
  providerId: string;
  safetyMode: SafetyMode;
  modelId: string;
  providerManagedModel: boolean;
  modelOptions: ModelOptionSelection[];
  effort: string | null;
  speed: string | null;
  providerOptions: VersionedJson;
}

export interface ChatTeammateStudioDraft {
  profile: ChatTeammateStudioProfileDraft;
  teammateDefaultRuntimeApproval: ChatRuntimeApprovalPolicy;
  channels: ChatTeammateChannelAccessInput[];
}

export interface ChatTeammateConflictComparison {
  identityChanged: boolean;
  policyChanged: boolean;
  runtimeApprovalChanged: boolean;
  addedChannelCount: number;
  removedChannelCount: number;
  changedChannelCount: number;
}

/** Returns a detached full studio draft suitable for conflict retention. */
export function cloneChatTeammateStudioDraft(
  draft: ChatTeammateStudioDraft,
): ChatTeammateStudioDraft {
  return cloneJsonValue(draft);
}

/** Compares a retained local draft with the latest durable teammate state. */
export function compareChatTeammateStudioDrafts(
  local: ChatTeammateStudioDraft,
  durable: ChatTeammateStudioDraft,
): ChatTeammateConflictComparison {
  const localChannels = channelMap(local.channels);
  const durableChannels = channelMap(durable.channels);
  let addedChannelCount = 0;
  let removedChannelCount = 0;
  let changedChannelCount = 0;
  for (const channelId of new Set([...localChannels.keys(), ...durableChannels.keys()])) {
    const localChannel = localChannels.get(channelId);
    const durableChannel = durableChannels.get(channelId);
    if (localChannel && !durableChannel) addedChannelCount += 1;
    else if (!localChannel && durableChannel) removedChannelCount += 1;
    else if (localChannel && durableChannel && !channelEqual(localChannel, durableChannel)) {
      changedChannelCount += 1;
    }
  }
  return {
    identityChanged: !identityEqual(local.profile, durable.profile),
    policyChanged: !policyEqual(local.profile, durable.profile),
    runtimeApprovalChanged: local.teammateDefaultRuntimeApproval
      !== durable.teammateDefaultRuntimeApproval,
    addedChannelCount,
    removedChannelCount,
    changedChannelCount,
  };
}

/** Replays local changes from a baseline over the latest durable teammate state. */
export function rebaseChatTeammateStudioDraft(
  baseline: ChatTeammateStudioDraft,
  local: ChatTeammateStudioDraft,
  durable: ChatTeammateStudioDraft,
): ChatTeammateStudioDraft {
  const baselineChannels = channelMap(baseline.channels);
  const localChannels = channelMap(local.channels);
  const durableChannels = channelMap(durable.channels);
  const channels: ChatTeammateChannelAccessInput[] = [];
  const orderedChannelIds = [
    ...durable.channels.map((channel) => channel.channelId),
    ...local.channels
      .map((channel) => channel.channelId)
      .filter((channelId) => !durableChannels.has(channelId)),
  ];
  for (const channelId of orderedChannelIds) {
    const baselineChannel = baselineChannels.get(channelId);
    const localChannel = localChannels.get(channelId);
    const durableChannel = durableChannels.get(channelId);
    const locallyChanged = presenceChanged(baselineChannel, localChannel)
      || Boolean(baselineChannel && localChannel && !channelEqual(baselineChannel, localChannel));
    const selected = locallyChanged ? localChannel : durableChannel;
    if (selected) channels.push(cloneJsonValue(selected));
  }
  return {
    profile: {
      displayName: chooseField(baseline.profile.displayName, local.profile.displayName, durable.profile.displayName, trimmedEqual),
      role: chooseField(baseline.profile.role, local.profile.role, durable.profile.role, trimmedEqual),
      instructions: chooseField(baseline.profile.instructions, local.profile.instructions, durable.profile.instructions, trimmedEqual),
      providerId: chooseField(baseline.profile.providerId, local.profile.providerId, durable.profile.providerId),
      safetyMode: chooseField(
        baseline.profile.safetyMode,
        local.profile.safetyMode,
        durable.profile.safetyMode,
      ),
      modelId: chooseField(baseline.profile.modelId, local.profile.modelId, durable.profile.modelId),
      providerManagedModel: chooseField(
        baseline.profile.providerManagedModel,
        local.profile.providerManagedModel,
        durable.profile.providerManagedModel,
      ),
      modelOptions: cloneJsonValue(chooseField(
        baseline.profile.modelOptions,
        local.profile.modelOptions,
        durable.profile.modelOptions,
        modelOptionsEqual,
      )),
      effort: chooseField(baseline.profile.effort, local.profile.effort, durable.profile.effort),
      speed: chooseField(baseline.profile.speed, local.profile.speed, durable.profile.speed),
      providerOptions: cloneJsonValue(chooseField(
        baseline.profile.providerOptions,
        local.profile.providerOptions,
        durable.profile.providerOptions,
        versionedJsonEqual,
      )),
    },
    teammateDefaultRuntimeApproval: chooseField(
      baseline.teammateDefaultRuntimeApproval,
      local.teammateDefaultRuntimeApproval,
      durable.teammateDefaultRuntimeApproval,
    ),
    channels,
  };
}

function channelMap(
  channels: readonly ChatTeammateChannelAccessInput[],
): Map<string, ChatTeammateChannelAccessInput> {
  return new Map(channels.map((channel) => [channel.channelId, channel]));
}

function channelEqual(
  left: ChatTeammateChannelAccessInput,
  right: ChatTeammateChannelAccessInput,
): boolean {
  return teammateAccessDraftSnapshot([left]) === teammateAccessDraftSnapshot([right]);
}

function identityEqual(
  left: ChatTeammateStudioProfileDraft,
  right: ChatTeammateStudioProfileDraft,
): boolean {
  return trimmedEqual(left.displayName, right.displayName)
    && trimmedEqual(left.role, right.role)
    && trimmedEqual(left.instructions, right.instructions);
}

function policyEqual(
  left: ChatTeammateStudioProfileDraft,
  right: ChatTeammateStudioProfileDraft,
): boolean {
  return left.providerId === right.providerId
    && left.safetyMode === right.safetyMode
    && left.modelId === right.modelId
    && left.providerManagedModel === right.providerManagedModel
    && modelOptionsEqual(left.modelOptions, right.modelOptions)
    && left.effort === right.effort
    && left.speed === right.speed
    && versionedJsonEqual(left.providerOptions, right.providerOptions);
}

function modelOptionsEqual(
  left: readonly ModelOptionSelection[],
  right: readonly ModelOptionSelection[],
): boolean {
  const sort = (options: readonly ModelOptionSelection[]) => (
    [...options].sort((a, b) => a.key.localeCompare(b.key))
  );
  return JSON.stringify(sort(left)) === JSON.stringify(sort(right));
}

function versionedJsonEqual(left: VersionedJson, right: VersionedJson): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function presenceChanged<T>(baseline: T | undefined, local: T | undefined): boolean {
  return Boolean(baseline) !== Boolean(local);
}

function trimmedEqual(left: string, right: string): boolean {
  return left.trim() === right.trim();
}

function chooseField<T>(
  baseline: T,
  local: T,
  durable: T,
  equal: (left: T, right: T) => boolean = Object.is,
): T {
  return equal(local, baseline) ? durable : local;
}

function cloneJsonValue<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}
