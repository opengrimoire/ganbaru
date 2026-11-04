import type { NotesPage } from "$lib/notes/types";
import type { NotesDataSourceSchemaPropertyDraft } from "./data-source-schema";
import { NOTES_DATABASE_ROW_MAX_DEPTH, type NotesDatabaseRowHierarchyMetadata } from "./row-hierarchy";
import { notesDatabasePropertyCell, notesDatabaseTableCellText, type NotesDatabaseTableColumn } from "./table";
import { moveNotesDatabaseView, stepNotesDatabaseView, type NotesDatabaseViewDropPlacement } from "./view-order";

/** The schema fields that place one property on row pages. */
export type NotesDatabaseRowPagePlacement = Pick<NotesDataSourceSchemaPropertyDraft, "id" | "type" | "pageVisibility" | "pagePosition">;

/** One property shown below a row page title, and whether it folds under "N more properties". */
export interface NotesDatabaseRowPageEntry<Property extends NotesDatabaseRowPagePlacement> {
  property: Property;
  collapsed: boolean;
}

/**
 * Order the properties a row page lists below its title: properties with an explicit page position first, by
 * position, then the rest in the fallback order they arrive in. The title is left out because the page heading edits
 * it. Equal positions keep their fallback order.
 *
 * @param properties Source properties in fallback order, such as the default table view's column order.
 */
export function orderNotesDatabaseRowPageProperties<Property extends NotesDatabaseRowPagePlacement>(
  properties: readonly Property[],
): Property[] {
  const listed = properties.filter((property) => property.type !== "title");
  const positioned = listed
    .map((property, index) => ({ property, index }))
    .filter((entry): entry is { property: Property & { pagePosition: number }; index: number } => entry.property.pagePosition !== null)
    .sort((left, right) => left.property.pagePosition - right.property.pagePosition || left.index - right.index)
    .map(({ property }) => property);
  return [...positioned, ...listed.filter((property) => property.pagePosition === null)];
}

/**
 * Mark which ordered properties fold under "N more properties": those set to always hide, and those set to hide when
 * empty that have no value on this row.
 *
 * @param ordered Properties in row page order.
 * @param isEmpty Whether the row has no value for a property.
 */
export function notesDatabaseRowPageEntries<Property extends NotesDatabaseRowPagePlacement>(
  ordered: readonly Property[],
  isEmpty: (property: Property) => boolean,
): NotesDatabaseRowPageEntry<Property>[] {
  return ordered.map((property) => ({
    property,
    collapsed: property.pageVisibility === "always_hide" || (property.pageVisibility === "hide_when_empty" && isEmpty(property)),
  }));
}

/**
 * Whether a row has no value for one property, for "Hide when empty". Unchecked checkboxes, blank text, no options,
 * no relations, no date, and computed values with no result count as empty. Buttons and timestamps always have
 * something to show, so they never count as empty. Neither does a value stored under another type than the
 * property's, which waits for the user to convert or discard it.
 *
 * @param page Row page with hydrated property values.
 * @param column Property as a table column.
 */
export function notesDatabaseRowPageValueIsEmpty(page: NotesPage, column: NotesDatabaseTableColumn): boolean {
  if (notesDatabasePropertyCell(page, column).kind === "mismatch") return false;
  switch (column.type) {
    case "button":
    case "created_time":
    case "last_edited_time":
      return false;
    case "checkbox":
      return notesDatabaseTableCellText(page, column) !== "true";
    default:
      return notesDatabaseTableCellText(page, column).trim() === "";
  }
}

/**
 * Give every listed property its index as its page position, so a reorder or an appended property saves one
 * complete order. The title and properties missing from the order keep their saved position.
 *
 * @param drafts Saved source drafts.
 * @param orderedIds Non-title property ids in their new row page order.
 * @returns New drafts with the positions applied.
 */
export function withNotesDatabaseRowPagePositions<Property extends NotesDatabaseRowPagePlacement>(
  drafts: readonly Property[],
  orderedIds: readonly string[],
): Property[] {
  const positions = new Map(orderedIds.map((id, index) => [id, index]));
  return drafts.map((property) => {
    const position = property.type === "title" ? undefined : positions.get(property.id);
    return position === undefined || position === property.pagePosition ? property : { ...property, pagePosition: position };
  });
}

/**
 * Move one row page property next to another, such as after a drag.
 *
 * @param orderedIds Every non-title property id in row page order.
 * @param movedId Property being moved.
 * @param targetId Property it is dropped on.
 * @param placement Whether it lands before or after the target.
 * @returns The new order, or null when the order would not change.
 */
export function moveNotesDatabaseRowPageProperty(
  orderedIds: readonly string[],
  movedId: string,
  targetId: string,
  placement: NotesDatabaseViewDropPlacement,
): string[] | null {
  return moveNotesDatabaseView(orderedIds, movedId, targetId, placement);
}

/**
 * Move one row page property a single step among the properties on screen, keeping folded ones in their places.
 *
 * @param orderedIds Every non-title property id in row page order.
 * @param visibleIds Properties on screen, in row page order.
 * @param movedId Property being moved.
 * @param direction -1 to move up, 1 to move down.
 * @returns The new order, or null when the property is already at that end.
 */
export function stepNotesDatabaseRowPageProperty(
  orderedIds: readonly string[],
  visibleIds: readonly string[],
  movedId: string,
  direction: -1 | 1,
): string[] | null {
  return stepNotesDatabaseView(orderedIds, visibleIds, movedId, direction);
}

/** A row page's place in its source hierarchy, as its Relations section shows it. */
export interface NotesDatabaseRowRelatives {
  rowId: string;
  hierarchy: NotesDatabaseRowHierarchyMetadata;
  childIds: readonly string[];
}

/** Whether a row is shallow enough to take another level of sub-items. */
export function notesDatabaseRowCanAddSubitems(hierarchy: NotesDatabaseRowHierarchyMetadata): boolean {
  return hierarchy.depth + 1 <= NOTES_DATABASE_ROW_MAX_DEPTH;
}

/**
 * Rows a row's sub-item picker leaves out: the row itself, its ancestors (which would make a cycle), and its direct
 * sub-items. A candidate with a deep subtree of its own can still exceed the depth limit, which the native save
 * reports.
 */
export function notesDatabaseSubitemExclusions(row: NotesDatabaseRowRelatives): string[] {
  return [...new Set([row.rowId, ...row.hierarchy.ancestor_row_page_ids, ...row.childIds])];
}

/**
 * Rows a row's parent item picker leaves out: the row itself, its current parent, and its direct sub-items. Deeper
 * descendants are not loaded with the row page, so the native save rejects them as cycles.
 */
export function notesDatabaseParentExclusions(row: NotesDatabaseRowRelatives): string[] {
  const parentId = row.hierarchy.parent_row_page_id;
  return [...new Set([row.rowId, ...(parentId ? [parentId] : []), ...row.childIds])];
}

/**
 * Filter picker candidates by a search query, matching every word of the query anywhere in the title, ignoring case
 * and accents.
 *
 * @param rows Candidates in display order.
 * @param query Text typed in the picker.
 * @param titleOf Title shown for a candidate.
 */
export function notesDatabaseRowSearch<Row>(rows: readonly Row[], query: string, titleOf: (row: Row) => string): Row[] {
  const words = foldSearchText(query).split(/\s+/u).filter(Boolean);
  if (words.length === 0) return [...rows];
  return rows.filter((row) => {
    const title = foldSearchText(titleOf(row));
    return words.every((word) => title.includes(word));
  });
}

function foldSearchText(value: string): string {
  return value.normalize("NFD").replace(/\p{Mn}/gu, "").toLocaleLowerCase();
}
