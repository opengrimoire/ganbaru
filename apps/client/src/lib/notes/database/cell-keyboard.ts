/** The text editing state of the field a database value key press comes from. */
export interface NotesDatabaseCellCaret {
  /** Whether the field is a multi-line text area, where vertical arrows and Shift+Enter edit text. */
  multiline: boolean;
  value: string;
  selectionStart: number | null;
  selectionEnd: number | null;
}

/**
 * Whether a key press belongs to the text field it comes from rather than to moving between values: horizontal
 * arrows that can still move the caret, and in text areas vertical arrows that can still move between lines and
 * Shift+Enter.
 *
 * @param key The pressed key.
 * @param shiftKey Whether Shift was held.
 * @param caret The text field state, or null when the key press does not come from a text field.
 */
export function notesDatabaseCellKeepsKey(key: string, shiftKey: boolean, caret: NotesDatabaseCellCaret | null): boolean {
  if (!caret) return false;
  const start = caret.selectionStart ?? 0;
  const end = caret.selectionEnd ?? start;
  const length = caret.value.length;
  if (key === "ArrowLeft") return start > 0 || end > 0;
  if (key === "ArrowRight") return start < length || end < length;
  if (!caret.multiline) return false;
  if (key === "Enter") return shiftKey;
  if (key === "ArrowUp") return start > 0 || end > 0;
  if (key === "ArrowDown") return start < length || end < length;
  return false;
}

/** The text editing state of a key press target, or null when it is not a text field. */
export function notesDatabaseCellCaret(target: EventTarget | null): NotesDatabaseCellCaret | null {
  if (!(target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) || target.type === "checkbox") return null;
  return {
    multiline: target instanceof HTMLTextAreaElement,
    value: target.value,
    selectionStart: target.selectionStart,
    selectionEnd: target.selectionEnd,
  };
}
