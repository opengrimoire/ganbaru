import { normalizeQuickNoteRuns } from "./rich-text";
import type { QuickNote, QuickNoteTextRun } from "./types";

/** The fields the Quick notes editor writes. */
export interface QuickNoteEditableFields {
  title: string;
  runs: QuickNoteTextRun[];
  color: QuickNote["color"];
  tagId: string | null;
  pinned: boolean;
}

export type QuickNoteEditableField = keyof QuickNoteEditableFields;

const EDITABLE_FIELDS: readonly QuickNoteEditableField[] = ["title", "runs", "color", "tagId", "pinned"];

/** The outcome of replaying local edits on a newer canonical note. */
export type QuickNoteRebase =
  | { kind: "rebased"; fields: QuickNoteEditableFields }
  | { kind: "conflict"; fields: QuickNoteEditableField[] };

function runsEqual(left: readonly QuickNoteTextRun[], right: readonly QuickNoteTextRun[]): boolean {
  const a = normalizeQuickNoteRuns(left);
  const b = normalizeQuickNoteRuns(right);
  return a.length === b.length && a.every((run, index) => {
    const other = b[index];
    return other !== undefined
      && run.content === other.content
      && run.bold === other.bold
      && run.italic === other.italic
      && run.underline === other.underline;
  });
}

function fieldEqual(field: QuickNoteEditableField, left: QuickNoteEditableFields, right: QuickNoteEditableFields): boolean {
  if (field === "runs") return runsEqual(left.runs, right.runs);
  return left[field] === right[field];
}

/** Copies the editable fields of a note. */
export function quickNoteEditableFields(note: QuickNoteEditableFields): QuickNoteEditableFields {
  return {
    title: note.title,
    runs: note.runs.map((run) => ({ ...run })),
    color: note.color,
    tagId: note.tagId,
    pinned: note.pinned,
  };
}

/**
 * Replays the editor's unsaved changes on a canonical note that changed meanwhile.
 *
 * A field the editor did not change takes the canonical value, and a field only the editor
 * changed keeps the local value. A field both sides changed to different values is a conflict,
 * which the caller must not resolve silently.
 *
 * @param base The canonical note the editor started from.
 * @param local The editor's current values.
 * @param canonical The newer canonical note.
 */
export function rebaseQuickNoteEdits(
  base: QuickNoteEditableFields,
  local: QuickNoteEditableFields,
  canonical: QuickNoteEditableFields,
): QuickNoteRebase {
  const fields = quickNoteEditableFields(canonical);
  const conflicts: QuickNoteEditableField[] = [];
  for (const field of EDITABLE_FIELDS) {
    if (fieldEqual(field, local, base) || fieldEqual(field, local, canonical)) continue;
    if (!fieldEqual(field, canonical, base)) {
      conflicts.push(field);
      continue;
    }
    switch (field) {
      case "title": fields.title = local.title; break;
      case "runs": fields.runs = local.runs.map((run) => ({ ...run })); break;
      case "color": fields.color = local.color; break;
      case "tagId": fields.tagId = local.tagId; break;
      case "pinned": fields.pinned = local.pinned; break;
    }
  }
  return conflicts.length > 0 ? { kind: "conflict", fields: conflicts } : { kind: "rebased", fields };
}
