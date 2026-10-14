import type { ChatChannelRead } from "$lib/chat/contracts";

/** Channels of one project that match a channel search, in the project's channel order. */
export interface ChatChannelSearchGroup<TProject extends { id: string }> {
  project: TProject;
  channels: ChatChannelRead[];
}

/** Normalize a channel search query for case-insensitive matching. */
export function normalizeChatChannelSearch(query: string): string {
  return query.trim().toLocaleLowerCase();
}

/** Whether a channel name or topic contains an already normalized query; an empty query matches every channel. */
export function chatChannelMatchesSearch(
  channel: Pick<ChatChannelRead, "name" | "topic">,
  normalizedQuery: string,
): boolean {
  return !normalizedQuery
    || channel.name.toLocaleLowerCase().includes(normalizedQuery)
    || channel.topic.toLocaleLowerCase().includes(normalizedQuery);
}

/**
 * Search channels across projects, keeping the given project order and each project's channel order and leaving
 * out projects without a match.
 */
export function chatChannelSearchGroups<TProject extends { id: string }>(
  projects: readonly TProject[],
  channelsForProject: (projectId: string) => readonly ChatChannelRead[],
  query: string,
): ChatChannelSearchGroup<TProject>[] {
  const normalizedQuery = normalizeChatChannelSearch(query);
  if (!normalizedQuery) return [];
  return projects.flatMap((project) => {
    const channels = channelsForProject(project.id)
      .filter((channel) => chatChannelMatchesSearch(channel, normalizedQuery));
    return channels.length > 0 ? [{ project, channels }] : [];
  });
}

/** Channels a picker lists for one project, split so only active channels take part in reordering. */
export interface ChatChannelPickerRows {
  active: ChatChannelRead[];
  archived: ChatChannelRead[];
}

/**
 * Filter a picker's channels by an already normalized query, listing archived channels after the active ones only
 * when requested and never repeating a channel that is also active.
 */
export function chatChannelPickerRows(
  active: readonly ChatChannelRead[],
  archived: readonly ChatChannelRead[],
  showArchived: boolean,
  normalizedQuery: string,
): ChatChannelPickerRows {
  const activeRows = active.filter((channel) => chatChannelMatchesSearch(channel, normalizedQuery));
  if (!showArchived) return { active: activeRows, archived: [] };
  const activeIds = new Set(active.map((channel) => channel.id));
  return {
    active: activeRows,
    archived: archived.filter((channel) => !activeIds.has(channel.id) && chatChannelMatchesSearch(channel, normalizedQuery)),
  };
}
