import * as chatApi from "$lib/api/chat";
import {
  channelAccessWithChannel,
  channelAccessWithoutChannel,
  conversationAccessProfile,
  resolveChannelMemberChanges,
} from "./channel-membership";

/**
 * Persist the difference between a channel's current teammate members and a chosen set by rewriting each affected
 * teammate's access. Returns whether anything changed so callers know to reload the channel.
 */
export async function applyChannelMemberChanges(
  currentTeammateIds: ReadonlySet<string>,
  selectedTeammateIds: ReadonlySet<string>,
  channelId: string,
): Promise<boolean> {
  const changes = resolveChannelMemberChanges(currentTeammateIds, selectedTeammateIds);
  if (changes.addedTeammateIds.length === 0 && changes.removedTeammateIds.length === 0) return false;
  const profile = changes.addedTeammateIds.length > 0
    ? conversationAccessProfile(await chatApi.listChatAccessProfiles())
    : null;
  for (const teammateId of changes.addedTeammateIds) {
    const access = await chatApi.readChatTeammateAccess(teammateId);
    await chatApi.replaceChatTeammateAccess({
      teammateId,
      expectedAccessRevision: access.accessRevision,
      teammateDefaultRuntimeApproval: access.teammateDefaultRuntimeApproval,
      channels: channelAccessWithChannel(access, channelId, profile),
    });
  }
  for (const teammateId of changes.removedTeammateIds) {
    const access = await chatApi.readChatTeammateAccess(teammateId);
    await chatApi.replaceChatTeammateAccess({
      teammateId,
      expectedAccessRevision: access.accessRevision,
      teammateDefaultRuntimeApproval: access.teammateDefaultRuntimeApproval,
      channels: channelAccessWithoutChannel(access, channelId),
    });
  }
  return true;
}
