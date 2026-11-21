import type { ChatMessageRead } from "$lib/chat/contracts";
import { formatDateTime } from "$lib/i18n/formatters";
import type { AppLocale } from "$lib/i18n/locales";
import type { TimelineVirtualItem } from "./virtualization";

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
 * Returns the feed row ID of the date divider shown before a channel message. Messages follow their order keys, not
 * their writer clocks, so one local day can start more than once in a feed; keying the divider by the message it
 * precedes keeps every row ID unique.
 */
export function channelFeedDateDividerRowId(messageItemId: string): string {
  return `date:${messageItemId}`;
}

/**
 * Returns the row that keeps its place on screen when older messages are prepended: the first message row that reaches
 * below the viewport top. Dividers are skipped because a prepend can move, remove, or replace them.
 *
 * @param items Rows of the current virtual window, in display order.
 * @param viewportTop Scroll offset of the viewport top within the row list.
 * @returns The message row ID, or `null` when the window holds no message row below the viewport top.
 */
export function channelFeedScrollAnchorRowId(
  items: readonly TimelineVirtualItem<ChannelFeedRow>[],
  viewportTop: number,
): string | null {
  return items.find((item) => item.row.kind === "message" && item.top + item.height > viewportTop)?.row.id ?? null;
}

/**
 * Builds the flat feed rows for loaded channel messages, computing dividers and grouping over the full list so a
 * virtual window never changes them.
 *
 * @param messages Loaded channel messages, oldest first.
 * @param unreadStart Index of the first unread message, or `messages.length` when none is unread.
 * @param unreadCount Unread count of the channel; the unread divider shows only when it is positive.
 * @param locale Locale used for the date divider labels.
 * @returns Rows in display order with unique row IDs. Prepending older messages keeps every message row ID.
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
        id: channelFeedDateDividerRowId(message.itemId),
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
