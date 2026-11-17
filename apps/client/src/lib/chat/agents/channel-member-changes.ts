import * as chatApi from "$lib/api/chat";
import type { ChatChannelRead } from "$lib/chat/contracts";
import {
  channelAccessWithChannel,
  channelAccessWithoutChannel,
  resolveChannelMemberChanges,
  agentCommandSupport,
  type AgentCommandSupport,
} from "./channel-membership";
import { preferredProjectWorkingFolder } from "$lib/chat/workspace/working-folder-selection";
import type { getChat } from "$lib/stores/chat.svelte";

/** The channel whose members change, with what a joining agent's default access needs. */
export interface ChannelMemberChangeContext {
  channel: Pick<ChatChannelRead, "id">;
  /** The channel project's preferred working folder, or null when the project has none. */
  projectWorkingFolderId: string | null;
  agentSupport: (agentId: string) => AgentCommandSupport;
}

/**
 * Persist the difference between a channel's current agent members and a chosen set by rewriting each affected
 * agent's access. Returns whether anything changed so callers know to reload the channel.
 */
export async function applyChannelMemberChanges(
  currentAgentIds: ReadonlySet<string>,
  selectedAgentIds: ReadonlySet<string>,
  context: ChannelMemberChangeContext,
): Promise<boolean> {
  const changes = resolveChannelMemberChanges(currentAgentIds, selectedAgentIds);
  if (changes.addedAgentIds.length === 0 && changes.removedAgentIds.length === 0) return false;
  const channelId = context.channel.id;
  const profiles = changes.addedAgentIds.length > 0 ? await chatApi.listChatAccessProfiles() : [];
  const defaults = { profiles, projectWorkingFolderId: context.projectWorkingFolderId };
  for (const agentId of changes.addedAgentIds) {
    const access = await chatApi.readChatAgentAccess(agentId);
    await chatApi.replaceChatAgentAccess({
      agentId,
      expectedAccessRevision: access.accessRevision,
      channels: channelAccessWithChannel(access, channelId, defaults, context.agentSupport(agentId)),
    });
  }
  for (const agentId of changes.removedAgentIds) {
    const access = await chatApi.readChatAgentAccess(agentId);
    await chatApi.replaceChatAgentAccess({
      agentId,
      expectedAccessRevision: access.accessRevision,
      channels: channelAccessWithoutChannel(access, channelId),
    });
  }
  return true;
}

/** Builds the member change context for a channel from the loaded Chat store. */
export function channelMemberChangeContext(
  chat: Pick<ReturnType<typeof getChat>, "workingFolders" | "agents" | "settings">,
  channel: Pick<ChatChannelRead, "id" | "projectId">,
): ChannelMemberChangeContext {
  const providerInstances = chat.settings?.providerInstances ?? [];
  return {
    channel,
    projectWorkingFolderId: preferredProjectWorkingFolder(chat.workingFolders, channel.projectId, null)
      ?.workingFolder.id ?? null,
    agentSupport: (agentId) => agentCommandSupport(
      chat.agents.find((entry) => entry.participant.id === agentId) ?? null,
      providerInstances,
    ),
  };
}
