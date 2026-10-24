import type { NotesPage } from "$lib/notes/types";

/** The identity of a data source property a row cell reads. */
export interface NotesDatabasePropertyCellColumn {
  id: string;
  type: string;
}

/**
 * The state of one row cell read from a page's composed properties.
 *
 * A value written before a concurrent property type change keeps its own type and reads as a
 * mismatch: it is shown neither as the column's type nor dropped, so it can be converted or
 * discarded explicitly.
 */
export type NotesDatabasePropertyCell =
  | { kind: "empty" }
  | { kind: "value"; property: Record<string, unknown>; payload: unknown }
  | { kind: "mismatch"; property: Record<string, unknown>; storedType: string; payload: unknown };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Read a row cell by the property's stable id and compare its stored type with the column type.
 *
 * @param page Row page whose composed properties are read.
 * @param column Property id and current type from the data source schema.
 * @returns Empty when the row has no well-formed value for the property, the value when its type
 * matches, or the mismatch with the stored type otherwise.
 */
export function notesDatabasePropertyCell(
  page: Pick<NotesPage, "properties">,
  column: NotesDatabasePropertyCellColumn,
): NotesDatabasePropertyCell {
  for (const property of Object.values(page.properties)) {
    if (!isRecord(property) || property.id !== column.id) continue;
    const storedType = property.type;
    if (typeof storedType !== "string" || !storedType) return { kind: "empty" };
    const payload = property[storedType] ?? null;
    return storedType === column.type
      ? { kind: "value", property, payload }
      : { kind: "mismatch", property, storedType, payload };
  }
  return { kind: "empty" };
}

/**
 * Read the stored property object only when its type matches the column.
 *
 * @param page Row page whose composed properties are read.
 * @param column Property id and current type from the data source schema.
 * @returns The property object, or null when it is absent or mismatched.
 */
export function notesDatabaseMatchingProperty(
  page: Pick<NotesPage, "properties">,
  column: NotesDatabasePropertyCellColumn,
): Record<string, unknown> | null {
  const cell = notesDatabasePropertyCell(page, column);
  return cell.kind === "value" ? cell.property : null;
}

/**
 * Read the typed payload only when the stored type matches the column.
 *
 * @param page Row page whose composed properties are read.
 * @param column Property id and current type from the data source schema.
 * @returns The payload under the column type, or null when absent or mismatched.
 */
export function notesDatabaseMatchingPayload(
  page: Pick<NotesPage, "properties">,
  column: NotesDatabasePropertyCellColumn,
): unknown {
  const cell = notesDatabasePropertyCell(page, column);
  return cell.kind === "value" ? cell.payload : null;
}
