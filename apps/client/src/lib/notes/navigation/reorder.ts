import { placedNextTo, steppedBy, type SidebarDropPlacement } from "$lib/utils/sidebar-order";

/** One row of the flat Notes navigation list, as custom ordering sees it. */
export interface NotesNavigationReorderRow {
  /** Navigation key, `folder:<id>` or `page:<id>`. */
  key: string;
  kind: "folder" | "page";
  depth: number;
  favorite: boolean;
}

/** Where a reordered row goes relative to the row it is dropped on. */
export type NotesNavigationReorderPlacement = SidebarDropPlacement;

/** How a drop on a row resolves in the custom order: next to it, or into it as a move to another folder. */
export type NotesNavigationReorderBand = NotesNavigationReorderPlacement | "into";

/** Share of a folder row's height, at its top and bottom edges, that places a row next to the folder instead of into it. */
const FOLDER_EDGE_RATIO = 0.25;

/**
 * Group the rows that can trade places: those under the same parent, of the same kind, and with the same favorite state,
 * since folders list before notes and favorites before the rest. Each row maps to its group's keys in rendered order.
 */
export function notesNavigationSiblingGroups(
  rows: readonly NotesNavigationReorderRow[],
): Map<string, readonly string[]> {
  const groups = new Map<string, string[]>();
  const siblingsByKey = new Map<string, readonly string[]>();
  const ancestors: string[] = [];
  for (const row of rows) {
    ancestors.length = row.depth;
    const parentKey = ancestors[row.depth - 1] ?? "";
    ancestors[row.depth] = row.key;
    const groupId = JSON.stringify([parentKey, row.kind, row.favorite]);
    const group = groups.get(groupId) ?? [];
    group.push(row.key);
    groups.set(groupId, group);
    siblingsByKey.set(row.key, group);
  }
  return siblingsByKey;
}

/**
 * Resolve a drop at `offsetRatio` (0 at the top of the row, 1 at the bottom). A note row splits in half; a folder row
 * keeps its middle for moving into the folder and uses its edges for placing next to it.
 */
export function notesNavigationReorderBand(kind: "folder" | "page", offsetRatio: number): NotesNavigationReorderBand {
  if (kind === "page") return offsetRatio < 0.5 ? "before" : "after";
  if (offsetRatio < FOLDER_EDGE_RATIO) return "before";
  return offsetRatio > 1 - FOLDER_EDGE_RATIO ? "after" : "into";
}

/**
 * Sibling keys after placing `sourceKey` before or after `targetKey`, or `null` when the two rows cannot trade places
 * or the order would not change.
 */
export function notesNavigationReorderedKeys(
  groups: ReadonlyMap<string, readonly string[]>,
  sourceKey: string,
  targetKey: string,
  placement: NotesNavigationReorderPlacement,
): string[] | null {
  return placedNextTo(groups.get(sourceKey) ?? [], sourceKey, targetKey, placement);
}

/** Sibling keys after moving `key` one place up (`-1`) or down (`1`), or `null` when it is already at that end. */
export function notesNavigationSteppedKeys(
  groups: ReadonlyMap<string, readonly string[]>,
  key: string,
  step: -1 | 1,
): string[] | null {
  return steppedBy(groups.get(key) ?? [], key, step);
}
