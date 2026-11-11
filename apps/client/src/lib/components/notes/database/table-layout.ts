import { COLLECTION_ROW_INSET_REM } from "$lib/components/collections/row-expansion";

/** Minimum width of the trailing track, which only holds the add property button (`size-9`). */
export const NOTES_TABLE_ADD_PROPERTY_TRACK_MIN = "2.25rem";

/** Leading track for the selection checkbox, in rem. */
export const NOTES_TABLE_SELECTION_TRACK_REM = 1.5;

/** Leading track for the expand chevron (`size-7`), in rem. */
export const NOTES_TABLE_EXPAND_TRACK_REM = 1.75;

export const NOTES_TABLE_SELECTION_TRACK = `${NOTES_TABLE_SELECTION_TRACK_REM}rem`;
export const NOTES_TABLE_EXPAND_TRACK = `${NOTES_TABLE_EXPAND_TRACK_REM}rem`;

/**
 * The selection and expand tracks sit outside the table: row fills and dividers start after them, and the table hangs
 * them into the margin so its content lines up with the database title and toolbar.
 */
export const NOTES_TABLE_ROW_GUTTER = `calc(${NOTES_TABLE_SELECTION_TRACK} + ${NOTES_TABLE_EXPAND_TRACK})`;

/** Where table content starts inside a row, after its inset and leading gutter, in rem. */
export const NOTES_TABLE_ROW_CONTENT_START_REM = COLLECTION_ROW_INSET_REM + NOTES_TABLE_SELECTION_TRACK_REM + NOTES_TABLE_EXPAND_TRACK_REM;

export const NOTES_TABLE_ROW_CONTENT_START = `${NOTES_TABLE_ROW_CONTENT_START_REM}rem`;

/**
 * Grid template of a database table row: the leading gutter tracks, one track per column width, and the trailing add
 * property track that absorbs the remaining width.
 *
 * @param columnWidths Rendered column widths in pixels, in display order.
 */
export function notesTableRowTemplate(columnWidths: readonly number[]): string {
  const columns = columnWidths.map((width) => `${width}px`).join(" ");
  return `${NOTES_TABLE_SELECTION_TRACK} ${NOTES_TABLE_EXPAND_TRACK} ${columns} minmax(${NOTES_TABLE_ADD_PROPERTY_TRACK_MIN}, 1fr)`;
}

/**
 * Left margin that hangs a table's leading gutter into the room left of its container, so the table content lines up
 * with the surrounding text when there is room and moves right only by what is missing.
 *
 * @param gutterRoom Pixels available left of the container before an ancestor clips, from `collectionGutterRoom`.
 */
export function notesTableGutterMargin(gutterRoom: number): string {
  return `calc(-1 * min(${gutterRoom}px, ${NOTES_TABLE_ROW_CONTENT_START}))`;
}
