import { Temporal } from "@js-temporal/polyfill";
import type { NotesCommentThread } from "$lib/notes/types";

/** Which discussions the All discussions panel lists. */
export type NotesDiscussionFilter = "open" | "resolved" | "all";

export const NOTES_DISCUSSION_FILTERS: readonly NotesDiscussionFilter[] = ["open", "resolved", "all"];

/** The longest quote a discussion header shows before the layout clamps it further. */
const NOTES_DISCUSSION_QUOTE_LENGTH = 280;

export interface NotesDiscussionDayGroup {
  /** ISO calendar date in the viewer's time zone. */
  day: string;
  /** Set for the two days people name instead of reading a date. */
  relative: "today" | "yesterday" | null;
  threads: NotesCommentThread[];
}

function timestamp(value: string | null | undefined): number {
  if (!value) return Number.NEGATIVE_INFINITY;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : Number.NEGATIVE_INFINITY;
}

/** The last moment someone posted, edited, or resolved something in the thread, in epoch milliseconds. */
export function notesDiscussionLastActivity(thread: NotesCommentThread): number {
  let latest = Math.max(timestamp(thread.created_time), timestamp(thread.resolved_at));
  for (const comment of thread.comments) {
    latest = Math.max(latest, timestamp(comment.created_time), timestamp(comment.last_edited_time));
  }
  return latest;
}

/** Threads that match the filter and still have a visible comment, most recently active first. */
export function filterNotesDiscussions(
  threads: readonly NotesCommentThread[],
  filter: NotesDiscussionFilter,
): NotesCommentThread[] {
  return threads
    .filter((thread) => thread.comments.length > 0 && (filter === "all" || thread.status === filter))
    .map((thread) => ({ thread, activity: notesDiscussionLastActivity(thread) }))
    .sort((left, right) => right.activity - left.activity || left.thread.id.localeCompare(right.thread.id))
    .map(({ thread }) => thread);
}

/**
 * Group threads under the calendar day of their last activity in the given time zone, keeping the incoming order
 * inside each day. Threads without a readable timestamp are left out rather than filed under an invented day.
 */
export function groupNotesDiscussionsByDay(
  threads: readonly NotesCommentThread[],
  options: { today: Temporal.PlainDate; timeZone: string },
): NotesDiscussionDayGroup[] {
  const yesterday = options.today.subtract({ days: 1 });
  const groups = new Map<string, NotesDiscussionDayGroup>();
  for (const thread of threads) {
    const activity = notesDiscussionLastActivity(thread);
    if (!Number.isFinite(activity)) continue;
    const day = Temporal.Instant.fromEpochMilliseconds(activity)
      .toZonedDateTimeISO(options.timeZone)
      .toPlainDate();
    const key = day.toString();
    let group = groups.get(key);
    if (!group) {
      const relative = day.equals(options.today) ? "today" : day.equals(yesterday) ? "yesterday" : null;
      group = { day: key, relative, threads: [] };
      groups.set(key, group);
    }
    group.threads.push(thread);
  }
  return [...groups.values()].sort((left, right) => Temporal.PlainDate.compare(right.day, left.day));
}

/** Discussions on the page itself, oldest first, the order they read in under the title. */
export function notesPageDiscussionsInReadingOrder(
  threads: readonly NotesCommentThread[],
): NotesCommentThread[] {
  return threads
    .filter((thread) => thread.parent.type === "page_id" && thread.status === "open" && thread.comments.length > 0)
    .sort((left, right) => timestamp(left.created_time) - timestamp(right.created_time)
      || left.id.localeCompare(right.id));
}

/**
 * The text a block discussion refers to: the commented selection when there is one, otherwise the block's text.
 * Page discussions and blocks without readable text have no quote.
 */
export function notesDiscussionQuote(thread: NotesCommentThread, blockText: string | null): string | null {
  if (thread.parent.type === "page_id") return null;
  return notesDiscussionQuoteText(thread.anchor?.text || blockText || "");
}

/** Collapse whitespace and clamp quoted text for a discussion header, or return null when nothing readable is left. */
export function notesDiscussionQuoteText(source: string): string | null {
  const text = source.replace(/\s+/gu, " ").trim();
  if (!text) return null;
  return text.length > NOTES_DISCUSSION_QUOTE_LENGTH
    ? `${text.slice(0, NOTES_DISCUSSION_QUOTE_LENGTH - 1).trimEnd()}…`
    : text;
}
