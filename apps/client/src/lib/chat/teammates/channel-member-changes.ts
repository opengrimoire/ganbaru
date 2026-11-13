import * as chatApi from "$lib/api/chat";
import type { ChatChannelRead } from "$lib/chat/contracts";
import {
  channelAccessWithChannel,
  channelAccessWithoutChannel,
  resolveChannelMemberChanges,
  teammateCommandSupport,
  type TeammateCommandSupport,
} from "./channel-membership";
import { preferredProjectWorkingFolder } from "$lib/chat/workspace/working-folder-selection";
import type { getChat } from "$lib/stores/chat.svelte";

/** The channel whose members change, with what a joining teammate's default access needs. */
export interface ChannelMemberChangeContext {
  channel: Pick<ChatChannelRead, "id">;
  /** The channel project's preferred working folder, or null when the project has none. */
  projectWorkingFolderId: string | null;
  teammateSupport: (teammateId: string) => TeammateCommandSupport;
}

/**
 * Persist the difference between a channel's current teammate members and a chosen set by rewriting each affected
 * teammate's access. Returns whether anything changed so callers know to reload the channel.
 */
export async function applyChannelMemberChanges(
  currentTeammateIds: ReadonlySet<string>,
  selectedTeammateIds: ReadonlySet<string>,
  context: ChannelMemberChangeContext,
): Promise<boolean> {
  const changes = resolveChannelMemberChanges(currentTeammateIds, selectedTeammateIds);
  if (changes.addedTeammateIds.length === 0 && changes.removedTeammateIds.length === 0) return false;
  const channelId = context.channel.id;
  const profiles = changes.addedTeammateIds.length > 0 ? await chatApi.listChatAccessProfiles() : [];
  const defaults = { profiles, projectWorkingFolderId: context.projectWorkingFolderId };
  for (const teammateId of changes.addedTeammateIds) {
    const access = await chatApi.readChatTeammateAccess(teammateId);
    await chatApi.replaceChatTeammateAccess({
      teammateId,
      expectedAccessRevision: access.accessRevision,
      channels: channelAccessWithChannel(access, channelId, defaults, context.teammateSupport(teammateId)),
    });
  }
  for (const teammateId of changes.removedTeammateIds) {
    const access = await chatApi.readChatTeammateAccess(teammateId);
    await chatApi.replaceChatTeammateAccess({
      teammateId,
      expectedAccessRevision: access.accessRevision,
      channels: channelAccessWithoutChannel(access, channelId),
    });
  }
  return true;
}

/** Builds the member change context for a channel from the loaded Chat store. */
export function channelMemberChangeContext(
  chat: Pick<ReturnType<typeof getChat>, "workingFolders" | "teammates" | "settings">,
  channel: Pick<ChatChannelRead, "id" | "projectId">,
): ChannelMemberChangeContext {
  const providerInstances = chat.settings?.providerInstances ?? [];
  return {
    channel,
    projectWorkingFolderId: preferredProjectWorkingFolder(chat.workingFolders, channel.projectId, null)
      ?.workingFolder.id ?? null,
    teammateSupport: (teammateId) => teammateCommandSupport(
      chat.teammates.find((entry) => entry.participant.id === teammateId) ?? null,
      providerInstances,
    ),
  };
}
