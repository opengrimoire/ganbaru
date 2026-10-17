import { readInteger, readNullableString, readRecord, readStringArray } from "$lib/notes/validation/readers";

export const NOTES_DATABASE_ROW_MAX_DEPTH = 32;
export const NOTES_DATABASE_MAX_COLLAPSED_ROWS = 500;

export interface NotesDatabaseRowHierarchyMetadata {
  parent_row_page_id: string | null;
  ancestor_row_page_ids: string[];
  depth: number;
  child_count: number;
}

export type NotesDatabaseRowHierarchy = Record<string, NotesDatabaseRowHierarchyMetadata>;

/** Validate bounded canonical hierarchy metadata from the native query boundary. */
export function parseNotesDatabaseRowHierarchy(value: unknown): NotesDatabaseRowHierarchy {
  if (value === undefined) return {};
  const rows = readRecord(value, "database row hierarchy");
  const result: NotesDatabaseRowHierarchy = {};
  for (const [rowId, raw] of Object.entries(rows)) {
    const row = readRecord(raw, `database row hierarchy.${rowId}`);
    const depth = readInteger(row.depth, "database row depth");
    const childCount = readInteger(row.child_count, "database row child count");
    const ancestors = readStringArray(row.ancestor_row_page_ids, "database row ancestors");
    const parent = readNullableString(row.parent_row_page_id, "database row parent");
    if (!rowId || depth < 0 || depth > NOTES_DATABASE_ROW_MAX_DEPTH || childCount < 0
      || ancestors.length !== depth || new Set(ancestors).size !== ancestors.length
      || ancestors.includes(rowId) || (ancestors[0] ?? null) !== parent) {
      throw new Error("Database row hierarchy is inconsistent");
    }
    Object.defineProperty(result, rowId, { value: { parent_row_page_id: parent, ancestor_row_page_ids: ancestors, depth, child_count: childCount }, enumerable: true, writable: true, configurable: true });
  }
  return result;
}

/** A rendered table line: a row, or the add row that closes an expanded row's visible sub-items. */
export type NotesDatabaseHierarchyItem<Row> = { type: "row"; row: Row } | { type: "addSubitem"; parent: Row };

/** Arrange loaded descendants below their closest loaded ancestor, preserving sibling query order. */
export function notesDatabaseHierarchyRows<Row extends { id: string }>(
  rows: readonly Row[], hierarchy: NotesDatabaseRowHierarchy, collapsedIds: readonly string[],
): Row[] {
  return notesDatabaseHierarchyItems(rows, hierarchy, collapsedIds, () => false)
    .flatMap((item) => item.type === "row" ? [item.row] : []);
}

/**
 * Arrange rows like {@link notesDatabaseHierarchyRows} and close each row that `expanded` accepts with an add row after
 * its visible sub-items, while the row is shallow enough to take another level.
 */
export function notesDatabaseHierarchyItems<Row extends { id: string }>(
  rows: readonly Row[], hierarchy: NotesDatabaseRowHierarchy, collapsedIds: readonly string[], expanded: (row: Row) => boolean,
): NotesDatabaseHierarchyItem<Row>[] {
  const collapsed = new Set(collapsedIds);
  const visible = rows.filter((row) => !hierarchy[row.id]?.ancestor_row_page_ids.some((id) => collapsed.has(id)));
  const loadedIds = new Set(visible.map((row) => row.id));
  const children = new Map<string | null, Row[]>();
  for (const row of visible) {
    const parent = hierarchy[row.id]?.ancestor_row_page_ids.find((id) => loadedIds.has(id)) ?? null;
    const siblings = children.get(parent) ?? [];
    siblings.push(row);
    children.set(parent, siblings);
  }
  const result: NotesDatabaseHierarchyItem<Row>[] = [];
  const visited = new Set<string>();
  function append(parent: string | null): void {
    for (const row of children.get(parent) ?? []) {
      if (visited.has(row.id)) continue;
      visited.add(row.id);
      result.push({ type: "row", row });
      append(row.id);
      if ((hierarchy[row.id]?.depth ?? 0) < NOTES_DATABASE_ROW_MAX_DEPTH && expanded(row)) result.push({ type: "addSubitem", parent: row });
    }
  }
  append(null);
  // Defend against malformed local metadata without dropping otherwise visible rows.
  for (const row of visible) if (!visited.has(row.id)) result.push({ type: "row", row });
  return result;
}

/** The given rows without those whose ancestor is also given, so a subtree action runs once per subtree. */
export function notesDatabaseTopmostRows<Row extends { id: string }>(rows: readonly Row[], hierarchy: NotesDatabaseRowHierarchy): Row[] {
  const ids = new Set(rows.map((row) => row.id));
  return rows.filter((row) => !hierarchy[row.id]?.ancestor_row_page_ids.some((id) => ids.has(id)));
}

/** Toggle one persisted collapse identity without exceeding the bounded saved-view contract. */
export function notesDatabaseCollapsedRows(ids: readonly string[], rowId: string): string[] {
  if (ids.includes(rowId)) return ids.filter((id) => id !== rowId);
  if (ids.length >= NOTES_DATABASE_MAX_COLLAPSED_ROWS) return [...ids];
  return [...ids, rowId];
}
