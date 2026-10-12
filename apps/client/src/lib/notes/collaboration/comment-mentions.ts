import {
  filterNotesMentionTargets,
  type NotesDateMentionTarget,
  type NotesMentionTarget,
  type NotesNamedMentionTarget,
} from "$lib/notes/rich-text/core";

/** Sections of the comment composer's mention menu, in display order. */
export type NotesCommentMentionGroupKind = "date" | "people" | "pages";

export interface NotesCommentMentionGroup {
  kind: NotesCommentMentionGroupKind;
  targets: NotesMentionTarget[];
}

export interface NotesCommentMentionLimits {
  /** Dates suggested before the person types anything after the at sign. */
  idleDates: number;
  /** Dates suggested once the query narrows the choice. */
  queryDates: number;
  people: number;
  pages: number;
}

export const NOTES_COMMENT_MENTION_LIMITS: NotesCommentMentionLimits = {
  idleDates: 2,
  queryDates: 3,
  people: 4,
  pages: 6,
};

/**
 * Group mention suggestions for a comment the way a discussion reads them: dates, people, then pages and databases.
 * Date suggestions arrive already matched to the query; named targets are filtered here. Empty groups are dropped so
 * the menu never shows a heading without choices.
 */
export function buildNotesCommentMentionGroups(input: {
  query: string;
  dateTargets: readonly NotesDateMentionTarget[];
  namedTargets: readonly NotesNamedMentionTarget[];
  limits?: NotesCommentMentionLimits;
}): NotesCommentMentionGroup[] {
  const limits = input.limits ?? NOTES_COMMENT_MENTION_LIMITS;
  const dateLimit = input.query.trim() ? limits.queryDates : limits.idleDates;
  const people = input.namedTargets.filter((target) => target.kind === "user");
  const pages = input.namedTargets.filter((target) => target.kind === "page" || target.kind === "database");
  const groups: NotesCommentMentionGroup[] = [
    { kind: "date", targets: input.dateTargets.slice(0, dateLimit) },
    { kind: "people", targets: filterNotesMentionTargets(people, input.query, limits.people) },
    { kind: "pages", targets: filterNotesMentionTargets(pages, input.query, limits.pages) },
  ];
  return groups.filter((group) => group.targets.length > 0);
}

/** The menu's choices in keyboard order, matching the order the groups render. */
export function flattenNotesCommentMentionGroups(
  groups: readonly NotesCommentMentionGroup[],
): NotesMentionTarget[] {
  return groups.flatMap((group) => group.targets);
}
