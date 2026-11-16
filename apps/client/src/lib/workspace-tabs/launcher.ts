import { isNewItemShortcut, menuDigitIndex, type KeyboardModifierState } from "$lib/keyboard-shortcuts";
import type { View } from "$lib/navigation";
import { filterProjectLists, type ProjectList } from "$lib/projects/lists";
import type { ProjectGroup } from "$lib/projects/types";

/** One place the new-tab launcher has drilled into, with the label its breadcrumb shows. */
export type TabLauncherStep =
  | { type: "kind"; kind: View; label: string }
  | { type: "group"; groupId: string; label: string }
  | { type: "project"; projectId: string; label: string }
  | { type: "page"; pageId: string; label: string }
  | { type: "folder"; folderId: string; label: string };

/** The parent whose Notes children a pages level lists. */
export type TabLauncherPagesParent =
  | { kind: "root" }
  | { kind: "page"; id: string }
  | { kind: "folder"; id: string };

/** What the launcher lists for a path: the kinds, a kind's projects, a project's channels, or a level of its pages. */
export type TabLauncherLevel =
  | { type: "kinds" }
  | { type: "projects"; kind: View; groupId: string | null }
  | { type: "channels"; projectId: string }
  | { type: "pages"; projectId: string; parent: TabLauncherPagesParent };

/**
 * Whether picking a kind drills into its projects instead of opening it. Calendar has no projects, and Projects,
 * Notes, and Chat pick a project first.
 */
export function tabLauncherKindDrills(kind: View): boolean {
  return kind !== "calendar";
}

/** The level a launcher path shows. A path that names no kind shows the kinds. */
export function tabLauncherLevel(path: readonly TabLauncherStep[]): TabLauncherLevel {
  const kindStep = path.find((step) => step.type === "kind");
  if (!kindStep) return { type: "kinds" };
  const kind = kindStep.kind;
  const projectStep = path.find((step) => step.type === "project");
  if (!projectStep || kind === "projects" || kind === "calendar") {
    const groupStep = path.findLast((step) => step.type === "group");
    return { type: "projects", kind, groupId: groupStep?.groupId ?? null };
  }
  if (kind === "chat") return { type: "channels", projectId: projectStep.projectId };
  const last = path.at(-1);
  if (last?.type === "page") return { type: "pages", projectId: projectStep.projectId, parent: { kind: "page", id: last.pageId } };
  if (last?.type === "folder") return { type: "pages", projectId: projectStep.projectId, parent: { kind: "folder", id: last.folderId } };
  return { type: "pages", projectId: projectStep.projectId, parent: { kind: "root" } };
}

/** What the projects level shows: group rows to drill into, then project lists, each with or without a section label. */
export interface TabLauncherProjectRows<TProject> {
  groups: ProjectGroup[];
  sections: { list: ProjectList<TProject>; labeled: boolean }[];
}

/**
 * Choose the rows of a projects level, mirroring the groups mode of the project pickers. At the top, every group comes
 * first, empty ones included so a new group can take its first project, and the projects outside every group follow;
 * inside a group, its projects. A search looks through every project, whatever group is open, and labels each
 * matching list with its group.
 */
export function tabLauncherProjectRows<TProject extends { name: string }>(
  lists: readonly ProjectList<TProject>[],
  groupId: string | null,
  query: string,
): TabLauncherProjectRows<TProject> {
  if (query.trim()) {
    return { groups: [], sections: filterProjectLists(lists, query).map((list) => ({ list, labeled: true })) };
  }
  if (groupId !== null) {
    const list = lists.find((candidate) => candidate.group?.id === groupId);
    return { groups: [], sections: list ? [{ list, labeled: false }] : [] };
  }
  const groups = lists.flatMap((list) => (list.group ? [list.group] : []));
  const ungrouped = lists.filter((list) => list.group === null && list.projects.length > 0);
  return { groups, sections: ungrouped.map((list) => ({ list, labeled: false })) };
}

/** What a level's create actions make, in the order its footer shows them. */
export type TabLauncherCreateKind = "group" | "project" | "folder" | "page" | "channel";

/**
 * The create actions a level's footer offers, the primary one marked. The primary action is what the new item
 * shortcut runs: a project on a projects level (a group only at the top), a note on a pages level (a folder only
 * outside a note), and a channel on a channels level.
 */
export function tabLauncherCreateKinds(level: TabLauncherLevel): { kind: TabLauncherCreateKind; primary: boolean }[] {
  switch (level.type) {
    case "kinds":
      return [];
    case "projects":
      return level.groupId === null
        ? [{ kind: "group", primary: false }, { kind: "project", primary: true }]
        : [{ kind: "project", primary: true }];
    case "channels":
      return [{ kind: "channel", primary: true }];
    case "pages":
      return level.parent.kind === "page"
        ? [{ kind: "page", primary: true }]
        : [{ kind: "folder", primary: false }, { kind: "page", primary: true }];
  }
}

/** What the keyboard needs to know about one launcher row. */
export interface TabLauncherRowShape {
  /** Rows that take a digit: the kinds, groups, projects, channels, pages, and folders, but not "open here" rows, open tabs, or create actions. */
  numbered: boolean;
  /** Whether ArrowRight shows what is inside the row instead of leaving the key to the search caret. */
  canDrill: boolean;
  /**
   * A footer create action, listed after every other row so the arrows reach it last. `primary` marks the one the
   * new item shortcut runs, which an otherwise empty level highlights.
   */
  create?: { primary: boolean };
}

/** The highest row number a digit key can pick: 1 to 9, then 0 for the tenth. */
export const TAB_LAUNCHER_MAX_NUMBER = 10;

/**
 * The digit key that picks a numbered row, which is 0 for the tenth row.
 *
 * @param number The row's 1-based number.
 * @returns The key to press.
 */
export function tabLauncherNumberKey(number: number): string {
  return String(number % 10);
}

/**
 * The 1-based number each row shows, or `null`. Numbered rows count from 1 in order up to 10, and only while digits
 * pick rows: always on a level without search, and only while the search is empty otherwise.
 */
export function tabLauncherRowNumbers(rows: readonly TabLauncherRowShape[], digitsPick: boolean): (number | null)[] {
  let next = 1;
  return rows.map((row) => {
    if (!digitsPick || !row.numbered || next > TAB_LAUNCHER_MAX_NUMBER) return null;
    return next++;
  });
}

/** Whether digit keys pick rows instead of typing into the search. */
export function tabLauncherDigitsPick(query: string, searchable: boolean): boolean {
  return !searchable || query.length === 0;
}

/**
 * The index of the highlighted row. A remembered row keeps the highlight while it is still listed. Otherwise an
 * empty search highlights the first row, which is the "open here" row where a level has one, or the primary create
 * action when the level lists nothing else; a search highlights its first result. Returns -1 when nothing is listed.
 */
export function tabLauncherHighlightIndex<TRow extends TabLauncherRowShape & { key: string }>(
  rows: readonly TRow[],
  highlightKey: string | null,
  query: string,
): number {
  if (rows.length === 0) return -1;
  if (highlightKey !== null) {
    const index = rows.findIndex((row) => row.key === highlightKey);
    if (index >= 0) return index;
  }
  if (!query) {
    if (!rows[0]?.create) return 0;
    const primary = rows.findIndex((row) => row.create?.primary);
    return primary >= 0 ? primary : 0;
  }
  const firstResult = rows.findIndex((row) => row.numbered);
  return firstResult >= 0 ? firstResult : 0;
}

/** A key the launcher handles, from a keydown event. */
export type TabLauncherKeyInput = KeyboardModifierState & Pick<KeyboardEvent, "key" | "code" | "isComposing">;

/** The launcher state a key acts on. */
export interface TabLauncherKeyContext {
  query: string;
  /** Whether this level has a search field. The kinds level does not. */
  searchable: boolean;
  /** Whether the launcher shows the kinds, its first level. */
  atKinds: boolean;
  rows: readonly TabLauncherRowShape[];
  /** The highlighted row index, or -1. */
  highlight: number;
  /** Whether the search caret sits at the end of the text with nothing selected. */
  caretAtEnd: boolean;
  /** Whether the new item shortcut runs the level's primary create action. */
  createShortcut?: boolean;
}

/** What a key does in the launcher. */
export type TabLauncherKeyAction =
  | { type: "highlight"; index: number }
  | { type: "pick"; index: number }
  | { type: "drill"; index: number }
  | { type: "back" }
  | { type: "clear-search" }
  | { type: "close" }
  | { type: "create" };

function hasCommandModifier(input: KeyboardModifierState): boolean {
  return input.ctrlKey || input.altKey || input.metaKey;
}

/**
 * Decide what a key does, or `null` to leave it to the browser, so it types into the search or moves its caret.
 * Escape clears a search, then goes back a level, then closes. Arrows move the highlight and Enter picks it. A digit
 * picks a numbered row only while digits pick rows, and a digit without a row types instead. ArrowRight shows what is
 * inside a row that has more inside, and ArrowLeft or Backspace in an empty search go back a level. Create actions are
 * plain rows to the keyboard: the arrows, Home, and End reach them and Enter runs them, but no digit does. The new
 * item shortcut runs the primary create action where the level has one.
 */
export function tabLauncherKeyAction(input: TabLauncherKeyInput, context: TabLauncherKeyContext): TabLauncherKeyAction | null {
  if (input.isComposing) return null;
  const { query, searchable, atKinds, rows, highlight } = context;
  const emptyQuery = query.length === 0;
  const plain = !hasCommandModifier(input) && !input.shiftKey;
  if (context.createShortcut && isNewItemShortcut(input)) return { type: "create" };
  switch (input.key) {
    case "Escape":
      if (!emptyQuery) return { type: "clear-search" };
      return atKinds ? { type: "close" } : { type: "back" };
    case "ArrowDown":
    case "ArrowUp": {
      if (hasCommandModifier(input) || rows.length === 0) return null;
      const step = input.key === "ArrowDown" ? 1 : -1;
      const from = highlight < 0 ? (step > 0 ? -1 : 0) : highlight;
      return { type: "highlight", index: (from + step + rows.length) % rows.length };
    }
    case "Home":
    case "End":
      if (!plain || rows.length === 0 || (searchable && !emptyQuery)) return null;
      return { type: "highlight", index: input.key === "Home" ? 0 : rows.length - 1 };
    case "Enter":
      if (hasCommandModifier(input) || highlight < 0 || highlight >= rows.length) return null;
      return { type: "pick", index: highlight };
    case "ArrowRight":
      if (!plain || highlight < 0 || !rows[highlight]?.canDrill) return null;
      if (searchable && !emptyQuery && !context.caretAtEnd) return null;
      return { type: "drill", index: highlight };
    case "ArrowLeft":
    case "Backspace":
      if (!plain || atKinds || !emptyQuery) return null;
      return { type: "back" };
  }
  if (!tabLauncherDigitsPick(query, searchable)) return null;
  const digit = menuDigitIndex(input);
  if (digit === null) return null;
  const numbers = tabLauncherRowNumbers(rows, true);
  const index = numbers.indexOf(digit + 1);
  return index >= 0 ? { type: "pick", index } : null;
}
