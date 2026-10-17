export interface InlineRenameHistory {
  entries: readonly string[];
  index: number;
  lastInputKind: InlineRenameInputKind | null;
  lastInputAt: number | null;
}

export type InlineRenameHistoryAction = "undo" | "redo";
export type InlineRenameInputKind = "insert" | "delete" | "replace";

const INLINE_RENAME_COALESCE_WINDOW_MS = 1_000;

/** Create a rename history with the original value as its first entry. */
export function createInlineRenameHistory(initialValue: string): InlineRenameHistory {
  return { entries: [initialValue], index: 0, lastInputKind: null, lastInputAt: null };
}

/** Record a value while grouping adjacent typing or deletion into natural undo steps. */
export function recordInlineRenameValue(
  history: InlineRenameHistory,
  value: string,
  inputKind: InlineRenameInputKind,
  inputAt: number,
): InlineRenameHistory {
  if (history.entries[history.index] === value) return history;
  const canCoalesce = inputKind !== "replace"
    && history.lastInputKind === inputKind
    && history.lastInputAt !== null
    && inputAt - history.lastInputAt <= INLINE_RENAME_COALESCE_WINDOW_MS
    && history.index === history.entries.length - 1;
  if (canCoalesce) {
    const entries = [...history.entries];
    entries[history.index] = value;
    return { ...history, entries, lastInputAt: inputAt };
  }
  return {
    entries: [...history.entries.slice(0, history.index + 1), value],
    index: history.index + 1,
    lastInputKind: inputKind,
    lastInputAt: inputAt,
  };
}

/** Move through rename history without passing either boundary. */
export function stepInlineRenameHistory(
  history: InlineRenameHistory,
  action: InlineRenameHistoryAction,
): InlineRenameHistory {
  const index = action === "undo"
    ? Math.max(0, history.index - 1)
    : Math.min(history.entries.length - 1, history.index + 1);
  return index === history.index
    ? history
    : { ...history, index, lastInputKind: null, lastInputAt: null };
}

/** Classify browser input events for rename history coalescing. */
export function inlineRenameInputKind(inputType: string): InlineRenameInputKind {
  if (inputType.startsWith("insert")) return "insert";
  if (inputType.startsWith("delete")) return "delete";
  return "replace";
}

/** Resolve platform text-history shortcuts used by inline rename fields. */
export function inlineRenameHistoryAction(
  event: Pick<KeyboardEvent, "altKey" | "ctrlKey" | "key" | "metaKey" | "shiftKey">,
): InlineRenameHistoryAction | null {
  if (!(event.ctrlKey || event.metaKey) || event.altKey) return null;
  const key = event.key.toLowerCase();
  if (key === "z") return event.shiftKey ? "redo" : "undo";
  if (key === "y" && !event.shiftKey) return "redo";
  return null;
}

/** Return the value at the active rename history position. */
export function inlineRenameHistoryValue(history: InlineRenameHistory): string {
  return history.entries[history.index] ?? "";
}
