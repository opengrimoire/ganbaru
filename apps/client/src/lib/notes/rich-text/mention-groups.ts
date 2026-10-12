import {
  filterNotesMentionTargets,
  notesMentionTargetKey,
  type NotesDateMentionTarget,
  type NotesMentionTarget,
  type NotesNamedMentionTarget,
} from "./core";

/** Sections of the editor mention menu, in display order. */
export const NOTES_MENTION_GROUP_KINDS = [
  "date",
  "people",
  "pages",
  "projects",
  "calendar",
] as const;

export type NotesMentionGroupKind = (typeof NOTES_MENTION_GROUP_KINDS)[number];

/** Sections filled from named targets, which are filtered and truncated here. */
export type NotesNamedMentionGroupKind = Exclude<NotesMentionGroupKind, "date">;

/**
 * One choice in the mention menu. Targets insert a mention, `invite` opens the invitation panel, and `more` reveals
 * the rest of a truncated section.
 */
export type NotesMentionRow =
  | { kind: "target"; target: NotesMentionTarget }
  | { kind: "invite" }
  | { kind: "more"; group: NotesNamedMentionGroupKind; hiddenCount: number };

export interface NotesMentionGroup {
  kind: NotesMentionGroupKind;
  rows: NotesMentionRow[];
}

/** Rows each named section shows before a "more" row collapses the rest. */
export type NotesMentionGroupLimits = Readonly<Record<NotesNamedMentionGroupKind, number>>;

export const NOTES_MENTION_GROUP_LIMITS: NotesMentionGroupLimits = {
  people: 4,
  pages: 5,
  projects: 3,
  calendar: 3,
};

/** The section a named mention target belongs to. */
export function notesMentionGroupKind(target: NotesNamedMentionTarget): NotesNamedMentionGroupKind {
  switch (target.kind) {
    case "user":
      return "people";
    case "page":
    case "database":
      return "pages";
    case "project":
    case "project_task":
      return "projects";
    case "calendar_event":
      return "calendar";
  }
}

/**
 * Group mention suggestions the way the menu reads them: dates, people, pages, then other app objects.
 *
 * Date suggestions arrive already matched to the query. Named targets are filtered here and each section shows at
 * most its limit unless it was expanded. People always end with the invitation row while the query is empty or a
 * person matches it, so typing a page name never makes the invitation the first choice. Empty sections are dropped.
 *
 * @param input The typed query, its date suggestions, the named catalog, and the sections expanded in this session.
 * @returns The non-empty sections in display order.
 */
export function buildNotesMentionGroups(input: {
  query: string;
  dateTargets: readonly NotesDateMentionTarget[];
  namedTargets: readonly NotesNamedMentionTarget[];
  expanded?: ReadonlySet<NotesMentionGroupKind>;
  canInvite?: boolean;
  limits?: NotesMentionGroupLimits;
}): NotesMentionGroup[] {
  const limits = input.limits ?? NOTES_MENTION_GROUP_LIMITS;
  const byKind = new Map<NotesNamedMentionGroupKind, NotesNamedMentionTarget[]>();
  for (const target of input.namedTargets) {
    const kind = notesMentionGroupKind(target);
    const targets = byKind.get(kind) ?? [];
    targets.push(target);
    byKind.set(kind, targets);
  }
  const groups = NOTES_MENTION_GROUP_KINDS.map((kind): NotesMentionGroup => {
    if (kind === "date") {
      return { kind, rows: input.dateTargets.map((target) => ({ kind: "target", target })) };
    }
    const matches = filterNotesMentionTargets(byKind.get(kind) ?? [], input.query, Number.POSITIVE_INFINITY);
    const shown = input.expanded?.has(kind) ? matches : matches.slice(0, limits[kind]);
    const rows: NotesMentionRow[] = shown.map((target) => ({ kind: "target", target }));
    if (shown.length < matches.length) rows.push({ kind: "more", group: kind, hiddenCount: matches.length - shown.length });
    if (kind === "people" && input.canInvite && (!input.query.trim() || matches.length > 0)) rows.push({ kind: "invite" });
    return { kind, rows };
  });
  return groups.filter((group) => group.rows.length > 0);
}

/** The menu's rows in keyboard order, matching the order the sections render. */
export function flattenNotesMentionGroups(groups: readonly NotesMentionGroup[]): NotesMentionRow[] {
  return groups.flatMap((group) => group.rows);
}

/** A stable identity for a row within one menu session. */
export function notesMentionRowKey(row: NotesMentionRow): string {
  if (row.kind === "target") return notesMentionTargetKey(row.target);
  if (row.kind === "more") return `more:${row.group}`;
  return "invite";
}
