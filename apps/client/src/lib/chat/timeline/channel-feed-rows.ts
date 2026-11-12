import type { ChatMessageRead } from "$lib/chat/contracts";
import { formatDateTime } from "$lib/i18n/formatters";
import type { AppLocale } from "$lib/i18n/locales";

/** Longest gap between two messages of one author that still renders them as one group. */
export const CHANNEL_MESSAGE_GROUP_WINDOW_MS = 5 * 60_000;

/** A date divider shown before the first message of each local calendar day. */
export interface ChannelFeedDateDividerRow {
  kind: "date-divider";
  id: string;
  label: string;
  first: boolean;
}

/** The divider shown before the first unread message. */
export interface ChannelFeedUnreadDividerRow {
  kind: "unread-divider";
  id: string;
}

/** One channel message with its grouping against the previous message. */
export interface ChannelFeedMessageRow {
  kind: "message";
  id: string;
  message: ChatMessageRead;
  grouped: boolean;
}

export type ChannelFeedRow = ChannelFeedDateDividerRow | ChannelFeedUnreadDividerRow | ChannelFeedMessageRow;

/** Stable row ID of the unread divider. */
export const CHANNEL_FEED_UNREAD_ROW_ID = "unread";

/** Returns the stable feed row ID of a channel message. */
export function channelFeedMessageRowId(messageItemId: string): string {
  return `message:${messageItemId}`;
}

/**
 * Builds the flat feed rows for loaded channel messages, computing dividers and grouping over the full list so a
 * virtual window never changes them.
 *
 * @param messages Loaded channel messages, oldest first.
 * @param unreadStart Index of the first unread message, or `messages.length` when none is unread.
 * @param unreadCount Unread count of the channel; the unread divider shows only when it is positive.
 * @param locale Locale used for the date divider labels.
 * @returns Rows in display order. Date divider IDs are keyed by the local day, so prepending older messages of the
 * same day keeps every existing row ID.
 */
export function buildChannelFeedRows(
  messages: readonly ChatMessageRead[],
  unreadStart: number,
  unreadCount: number,
  locale: AppLocale,
): ChannelFeedRow[] {
  const rows: ChannelFeedRow[] = [];
  let previous: ChatMessageRead | undefined;
  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index];
    if (!message) continue;
    if (index === unreadStart && unreadCount > 0) {
      rows.push({ kind: "unread-divider", id: CHANNEL_FEED_UNREAD_ROW_ID });
    }
    const day = localDayKey(message.createdAt);
    if (!previous || localDayKey(previous.createdAt) !== day) {
      rows.push({
        kind: "date-divider",
        id: `date:${day}`,
        label: formatDateTime(locale, Date.parse(message.createdAt), { dateStyle: "full" }),
        first: index === 0,
      });
    }
    rows.push({
      kind: "message",
      id: channelFeedMessageRowId(message.itemId),
      message,
      grouped: groupsWithPrevious(message, previous),
    });
    previous = message;
  }
  return rows;
}

function groupsWithPrevious(current: ChatMessageRead, previous: ChatMessageRead | undefined): boolean {
  if (!previous || current.author === null || current.author.id !== previous.author?.id) return false;
  return localDayKey(current.createdAt) === localDayKey(previous.createdAt)
    && Date.parse(current.createdAt) - Date.parse(previous.createdAt) < CHANNEL_MESSAGE_GROUP_WINDOW_MS;
}

function localDayKey(timestamp: string): string {
  return new Date(timestamp).toDateString();
}
