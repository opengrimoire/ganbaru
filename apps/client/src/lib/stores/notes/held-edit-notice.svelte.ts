/**
 * Raised by a flush while a block edit waits for the user to choose, so navigation, tab moves, and window close keep
 * the page and its draft open instead of leaving the held edit behind. It lives beside the app notice, outside the
 * Notes stores, so the window and tab shells can recognize it without loading Notes.
 */
export class NotesHeldEditError extends Error {
  constructor(
    readonly blockIds: readonly string[],
    /** The pages that hold the edits, in the order their edits were held. */
    readonly pageIds: readonly string[],
  ) {
    super(`Notes block edits wait for a choice: ${blockIds.join(", ")}`);
    this.name = "NotesHeldEditError";
  }
}

/**
 * What the app tells the user about held Notes edits outside the editor: a flush refused because an edit waits on
 * `pageId`, or held text whose page was removed, kept so it can still be copied.
 */
export type NotesHeldEditNotice =
  | { kind: "held"; pageId: string | null }
  | { kind: "removed"; text: string };

/**
 * The notice for a failed save before closing a window, moving a tab, or hiding the app.
 *
 * @param error The failure the flush raised.
 * @returns The held notice naming the first page that holds an edit, or `null` for any other failure.
 */
export function notesHeldEditNoticeFromFailure(error: unknown): NotesHeldEditNotice | null {
  if (!(error instanceof NotesHeldEditError)) return null;
  return { kind: "held", pageId: error.pageIds[0] ?? null };
}

/**
 * The notice to show when `next` arrives while `current` is shown. Removed text is never replaced: a later refusal
 * keeps it, and later removed text is appended to it, since dismissing that notice is the only way to lose the text.
 */
export function mergeNotesHeldEditNotice(
  current: NotesHeldEditNotice | null,
  next: NotesHeldEditNotice,
): NotesHeldEditNotice {
  if (current?.kind !== "removed") return next;
  if (next.kind !== "removed") return current;
  return { kind: "removed", text: `${current.text}\n\n${next.text}` };
}

/**
 * The notice for held edits whose page was removed elsewhere.
 *
 * @param texts The plain text of each held edit.
 * @returns The removed notice with the texts joined by blank lines, or `null` when every text is blank.
 */
export function notesRemovedHeldEditNotice(texts: readonly string[]): NotesHeldEditNotice | null {
  const kept = texts.filter((text) => text.trim().length > 0);
  return kept.length > 0 ? { kind: "removed", text: kept.join("\n\n") } : null;
}

let current = $state<NotesHeldEditNotice | null>(null);

/**
 * The app-wide notice about held Notes edits, shown by a toast in each window so a refused close, tab move, or page
 * removal is never silent while the Notes view is not on screen.
 */
export const notesHeldEditNotice = {
  get current(): NotesHeldEditNotice | null { return current; },
  /** Show `notice`, keeping any removed text shown now. */
  show(notice: NotesHeldEditNotice): void { current = mergeNotesHeldEditNotice(current, notice); },
  dismiss(): void { current = null; },
};

/**
 * Show the held notice when `error` is a flush refused by a held Notes edit.
 *
 * @returns Whether `error` was a held edit refusal, so callers report other failures as before.
 */
export function reportNotesHeldEditFailure(error: unknown): boolean {
  const notice = notesHeldEditNoticeFromFailure(error);
  if (!notice) return false;
  notesHeldEditNotice.show(notice);
  return true;
}
