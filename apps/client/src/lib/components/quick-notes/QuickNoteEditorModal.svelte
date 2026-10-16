<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import Bold from "@lucide/svelte/icons/bold";
  import Italic from "@lucide/svelte/icons/italic";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import Redo2 from "@lucide/svelte/icons/redo-2";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Underline from "@lucide/svelte/icons/underline";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import {
    createQuickNote,
    getQuickNote,
    updateQuickNote,
    QuickNoteWriteError,
  } from "$lib/api/quick-notes";
  import { FALLBACK_COLOR_INDEX } from "$lib/calendar/types";
  import {
    notesPlainTextFromEditableRoot,
    notesTextSelectionFromEditableRoot,
    restoreNotesEditableSelection,
  } from "$lib/notes/editor/selection";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getQuickNoteColor } from "$lib/quick-notes/colors";
  import { registerQuickNotesFlusher } from "$lib/quick-notes/persistence";
  import {
    createQuickNoteHistory,
    quickNoteHistoryShortcutAction,
    recordQuickNoteHistory,
    stepQuickNoteHistory,
    type QuickNoteHistoryKind,
    type QuickNoteHistorySnapshot,
  } from "$lib/quick-notes/history";
  import {
    applyQuickNoteBeforeInput,
    normalizeQuickNoteRuns,
    quickNoteBodyWithinLimit,
    quickNoteFormattingForSelection,
    quickNotePlainText,
    quickNoteRunsFromHtml,
    reconcileQuickNotePlainText,
    replaceQuickNoteRange,
    toggleQuickNoteFormatting,
    type QuickNoteSelection,
  } from "$lib/quick-notes/rich-text";
  import {
    EMPTY_QUICK_NOTE_FORMATTING,
    QUICK_NOTE_BODY_MAX_CHARS,
    QUICK_NOTE_TITLE_MAX_CHARS,
    type QuickNote,
    type QuickNoteConflictResolved,
    type QuickNoteTag,
    type QuickNoteFormatting,
    type QuickNoteFormattingName,
    type QuickNoteTextRun,
  } from "$lib/quick-notes/types";
  import {
    quickNoteEditableFields,
    rebaseQuickNoteEdits,
    type QuickNoteEditableFields,
  } from "$lib/quick-notes/rebase";
  import { listenForQuickNotesChanges, publishQuickNotesChanged } from "$lib/quick-notes/window-sync";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import type { Theme } from "$lib/themes";
  import QuickNoteColorPicker from "./QuickNoteColorPicker.svelte";
  import QuickNoteConflictBanner from "./QuickNoteConflictBanner.svelte";
  import QuickNoteRichText from "./QuickNoteRichText.svelte";
  import QuickNoteTagPicker from "./QuickNoteTagPicker.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { OPENING_MAY_FOCUS_TEXT } from "$lib/mobile/keyboard";

  let {
    note,
    tags,
    defaultTagId,
    theme,
    onClose,
    onSaved,
    onArchive,
    onUnarchive,
    onTrash,
    onRestore,
    onDelete,
    mobileLayout = false,
    obscured = false,
  }: {
    note: QuickNote | null;
    tags: readonly QuickNoteTag[];
    defaultTagId: string | null;
    theme: Theme;
    onClose: () => void;
    onSaved: (note: QuickNote) => void;
    onArchive: (note: QuickNote) => void;
    onUnarchive: (note: QuickNote) => void;
    onTrash: (note: QuickNote) => void;
    onRestore: (note: QuickNote) => void;
    onDelete: (note: QuickNote) => void;
    mobileLayout?: boolean;
    obscured?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const initialNote = untrack(() => note);
  const initialTagId = untrack(() => defaultTagId);
  const initialId = initialNote?.id ?? crypto.randomUUID();
  let id = $state(initialId);
  let title = $state(initialNote?.title ?? "");
  let runs = $state<QuickNoteTextRun[]>(initialNote?.runs.map((run) => ({ ...run })) ?? []);
  let color = $state<QuickNote["color"]>(initialNote?.color ?? FALLBACK_COLOR_INDEX);
  let tagId = $state(initialNote?.tagId ?? initialTagId);
  let pinned = $state(initialNote?.pinned ?? false);
  let persisted = $state<QuickNote | null>(initialNote);
  let revision = $state(initialNote?.revision ?? 0);
  let selection = $state<QuickNoteSelection>({ start: 0, end: 0 });
  let typingFormatting = $state<QuickNoteFormatting>({ ...EMPTY_QUICK_NOTE_FORMATTING });
  let status = $state<"idle" | "saving" | "saved" | "failed" | "conflict">(initialNote ? "saved" : "idle");
  let limitError = $state("");
  let dirtyVersion = 0;
  let savedVersion = 0;
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let saveChain: Promise<void> = Promise.resolve();
  let transitionPending = $state(false);
  let dialog = $state<HTMLDivElement | null>(null);
  let editor = $state<HTMLDivElement | null>(null);
  let titleInput = $state<HTMLInputElement | null>(null);
  let history = $state(createQuickNoteHistory());
  let composing = false;
  let compositionSnapshot: QuickNoteHistorySnapshot | null = null;
  let nativeInputSnapshot: QuickNoteHistorySnapshot | null = null;
  const readOnly = $derived(persisted?.trashedAt !== null && persisted !== null);
  const colors = $derived(getQuickNoteColor(color, theme));
  const modalStyle = $derived(`--quick-editor-bg: ${colors.bg}; --quick-editor-fg: ${colors.text};`);
  const selectedFormatting = $derived(selection.start === selection.end
    ? typingFormatting
    : quickNoteFormattingForSelection(runs, selection));
  const toolbarButton = $derived(mobileLayout
    ? "flex size-12 shrink-0 items-center justify-center rounded-xl transition-colors active:bg-black/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-current disabled:opacity-35 dark:active:bg-white/10"
    : "flex size-8 items-center justify-center rounded-md transition-colors hover:bg-black/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-current disabled:opacity-35 dark:hover:bg-white/10");
  const overlayStyle = $derived(mobileLayout
    ? "left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left);"
    : "");
  const dialogStyle = $derived(mobileLayout
    ? `${modalStyle} width: 100%; height: 100%; max-width: 48rem;`
    : `${modalStyle} height: min(560px, calc(100dvh - 1rem));`);

  function meaningful(): boolean {
    return title.trim().length > 0 || quickNotePlainText(runs).trim().length > 0;
  }

  function markDirty(): void {
    dirtyVersion += 1;
    status = "idle";
    scheduleSave();
  }

  function scheduleSave(): void {
    if (readOnly) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = null;
      void flush();
    }, 250);
  }

  function applyCanonical(saved: QuickNote): void {
    persisted = saved;
    revision = saved.revision;
    id = saved.id;
    onSaved(saved);
    publishQuickNotesChanged();
  }

  function currentFields(): QuickNoteEditableFields {
    return quickNoteEditableFields({ title, runs, color, tagId, pinned });
  }

  /** Shows canonical or rebased values, keeping the caret inside the new body. */
  function applyFields(fields: QuickNoteEditableFields): void {
    title = fields.title;
    color = fields.color;
    tagId = fields.tagId;
    pinned = fields.pinned;
    if (runsEqual(runs, fields.runs)) return;
    runs = fields.runs;
    const length = quickNotePlainText(runs).length;
    const nextSelection = { start: Math.min(selection.start, length), end: Math.min(selection.end, length) };
    selection = nextSelection;
    if (editor?.contains(document.activeElement)) void restoreEditorSelection(nextSelection);
  }

  /**
   * Replays unsaved edits on the newer canonical note after a revision conflict.
   *
   * @returns Whether the edits were rebased and can be saved again.
   */
  async function rebaseOnCanonical(): Promise<boolean> {
    const base = persisted;
    if (!base) return false;
    let fresh: QuickNote;
    try {
      fresh = await getQuickNote(base.id);
    } catch (error: unknown) {
      console.warn("quick note could not be reloaded for rebase", error);
      return false;
    }
    if (fresh.trashedAt !== null || persisted !== base) return false;
    const result = rebaseQuickNoteEdits(base, currentFields(), fresh);
    if (result.kind === "conflict") return false;
    applyFields(result.fields);
    persisted = fresh;
    revision = fresh.revision;
    onSaved(fresh);
    return true;
  }

  /** Shows changes from another window or device while the editor has nothing unsaved. */
  async function refreshFromCanonical(): Promise<void> {
    const shown = persisted;
    if (!shown || saveTimer || savedVersion !== dirtyVersion || status === "saving" || composing) return;
    const expectedVersion = dirtyVersion;
    let fresh: QuickNote;
    try {
      fresh = await getQuickNote(shown.id);
    } catch (error: unknown) {
      console.warn("quick note could not be refreshed", error);
      return;
    }
    if (persisted !== shown || dirtyVersion !== expectedVersion || savedVersion !== dirtyVersion || saveTimer) return;
    if (fresh.revision === shown.revision && fresh.hasConflict === shown.hasConflict) return;
    applyFields(quickNoteEditableFields(fresh));
    persisted = fresh;
    revision = fresh.revision;
    if (status === "conflict") status = "saved";
  }

  function conflictResolved(result: QuickNoteConflictResolved): void {
    const base = persisted;
    const rebased = base
      ? rebaseQuickNoteEdits(base, currentFields(), result.note)
      : { kind: "rebased" as const, fields: quickNoteEditableFields(result.note) };
    // A field edited here while resolving keeps the newer local value for the next save.
    if (rebased.kind === "rebased") applyFields(rebased.fields);
    applyCanonical(result.note);
    if (result.copy) onSaved(result.copy);
  }

  async function persistCurrent(targetVersion: number, allowRebase = true): Promise<void> {
    if (!meaningful() && persisted === null) {
      savedVersion = targetVersion;
      status = "idle";
      return;
    }
    if (title.length > QUICK_NOTE_TITLE_MAX_CHARS || !quickNoteBodyWithinLimit(runs)) return;
    status = "saving";
    try {
      const saved = persisted === null
        ? await createQuickNote({ id, title, runs: normalizeQuickNoteRuns(runs), color, tagId, pinned })
        : await updateQuickNote({
            id,
            expectedRevision: revision,
            title,
            runs: normalizeQuickNoteRuns(runs),
            color,
            tagId,
            pinned,
          });
      applyCanonical(saved);
      savedVersion = targetVersion;
      status = dirtyVersion === targetVersion ? "saved" : "idle";
      if (dirtyVersion !== targetVersion) scheduleSave();
    } catch (error: unknown) {
      const revisionConflict = error instanceof QuickNoteWriteError && error.code === "revision_conflict";
      if (revisionConflict && allowRebase && await rebaseOnCanonical()) {
        await persistCurrent(targetVersion, false);
        return;
      }
      status = revisionConflict ? "conflict" : "failed";
      throw error;
    }
  }

  async function flush(): Promise<void> {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
    }
    if (savedVersion === dirtyVersion) {
      await saveChain;
      return;
    }
    const targetVersion = dirtyVersion;
    saveChain = saveChain.catch(() => undefined).then(() => persistCurrent(targetVersion));
    await saveChain;
  }

  function snapshot(
    snapshotRuns: readonly QuickNoteTextRun[] = runs,
    snapshotSelection: QuickNoteSelection = selection,
    snapshotFormatting: QuickNoteFormatting = typingFormatting,
  ): QuickNoteHistorySnapshot {
    return {
      runs: snapshotRuns.map((run) => ({ ...run })),
      selection: { ...snapshotSelection },
      typingFormatting: { ...snapshotFormatting },
    };
  }

  function runsEqual(left: readonly QuickNoteTextRun[], right: readonly QuickNoteTextRun[]): boolean {
    return left.length === right.length && left.every((run, index) => {
      const other = right[index];
      return other !== undefined
        && run.content === other.content
        && run.bold === other.bold
        && run.italic === other.italic
        && run.underline === other.underline;
    });
  }

  async function restoreEditorSelection(nextSelection: QuickNoteSelection): Promise<void> {
    await tick();
    if (editor) restoreNotesEditableSelection(editor, nextSelection);
  }

  async function applyRuns(
    nextRuns: QuickNoteTextRun[],
    nextSelection: QuickNoteSelection,
    options: {
      before?: QuickNoteHistorySnapshot;
      historyKind?: QuickNoteHistoryKind;
      typingFormatting?: QuickNoteFormatting;
    } = {},
  ): Promise<void> {
    if (!quickNoteBodyWithinLimit(nextRuns)) {
      limitError = t("quickNotes.editor.bodyLimit", QUICK_NOTE_BODY_MAX_CHARS);
      return;
    }
    const normalized = normalizeQuickNoteRuns(nextRuns);
    const nextFormatting = options.typingFormatting
      ?? quickNoteFormattingForSelection(normalized, nextSelection);
    if (runsEqual(runs, normalized)) {
      runs = normalized;
      selection = nextSelection;
      typingFormatting = nextFormatting;
      await restoreEditorSelection(nextSelection);
      return;
    }
    limitError = "";
    if (options.before && options.historyKind) {
      history = recordQuickNoteHistory(
        history,
        options.before,
        snapshot(normalized, nextSelection, nextFormatting),
        options.historyKind,
      );
    }
    runs = normalized;
    selection = nextSelection;
    typingFormatting = nextFormatting;
    markDirty();
    await restoreEditorSelection(nextSelection);
  }

  function syncSelection(): void {
    if (!editor) return;
    const next = notesTextSelectionFromEditableRoot(editor);
    if (!next) return;
    const moved = next.start !== selection.start || next.end !== selection.end;
    selection = next;
    if (moved || next.start !== next.end) {
      typingFormatting = quickNoteFormattingForSelection(runs, next);
    }
  }

  function historyKindForInput(inputType: string): QuickNoteHistoryKind {
    if (inputType === "insertParagraph" || inputType === "insertLineBreak") return "break";
    if (inputType.startsWith("delete")) return "delete";
    if (inputType.startsWith("insert")) return "insert";
    return "replace";
  }

  function handleBeforeInput(event: InputEvent): void {
    if (readOnly || composing || event.isComposing) return;
    if (!editor) return;
    if (event.inputType === "historyUndo" || event.inputType === "historyRedo") {
      event.preventDefault();
      if (event.inputType === "historyUndo") undo(); else redo();
      return;
    }
    const currentSelection = notesTextSelectionFromEditableRoot(editor) ?? selection;
    const before = snapshot(runs, currentSelection);
    const plan = applyQuickNoteBeforeInput(
      runs,
      currentSelection,
      event.inputType,
      event.data,
      currentSelection.start === currentSelection.end
        ? typingFormatting
        : quickNoteFormattingForSelection(runs, currentSelection),
    );
    if (!plan) {
      nativeInputSnapshot = before;
      return;
    }
    event.preventDefault();
    nativeInputSnapshot = null;
    void applyRuns(plan.runs, plan.selection, {
      before,
      historyKind: historyKindForInput(event.inputType),
    });
  }

  function formattingShortcut(event: KeyboardEvent): QuickNoteFormattingName | null {
    if (!(event.ctrlKey || event.metaKey) || event.altKey) return null;
    const key = event.key.toLowerCase();
    if (key === "b") return "bold";
    if (key === "i") return "italic";
    if (key === "u") return "underline";
    return null;
  }

  function toggleFormatting(name: QuickNoteFormattingName): void {
    if (readOnly) return;
    syncSelection();
    if (selection.start === selection.end) {
      typingFormatting = { ...typingFormatting, [name]: !typingFormatting[name] };
      return;
    }
    const before = snapshot();
    const next = toggleQuickNoteFormatting(runs, selection, name);
    void applyRuns(next, selection, { before, historyKind: "format" });
  }

  function undo(): void {
    const result = stepQuickNoteHistory(history, "undo");
    if (!result) return;
    history = result.state;
    void applyRuns(
      result.snapshot.runs,
      result.snapshot.selection,
      { typingFormatting: result.snapshot.typingFormatting },
    );
  }

  function redo(): void {
    const result = stepQuickNoteHistory(history, "redo");
    if (!result) return;
    history = result.state;
    void applyRuns(
      result.snapshot.runs,
      result.snapshot.selection,
      { typingFormatting: result.snapshot.typingFormatting },
    );
  }

  function handleEditorKeydown(event: KeyboardEvent): void {
    const formatting = formattingShortcut(event);
    if (formatting) {
      event.preventDefault();
      toggleFormatting(formatting);
      return;
    }
    const historyAction = quickNoteHistoryShortcutAction(event);
    if (historyAction) {
      event.preventDefault();
      if (historyAction === "undo") undo(); else redo();
    }
  }

  function handlePaste(event: ClipboardEvent): void {
    if (readOnly || !editor) return;
    event.preventDefault();
    syncSelection();
    const html = event.clipboardData?.getData("text/html") ?? "";
    const plainText = event.clipboardData?.getData("text/plain") ?? "";
    const replacement = html.trim()
      ? quickNoteRunsFromHtml(html)
      : plainText ? [{ content: plainText.replace(/\r\n?/gu, "\n"), ...typingFormatting }] : [];
    const next = replaceQuickNoteRange(runs, selection, replacement);
    const cursor = Math.min(selection.start, selection.end) + quickNotePlainText(replacement).length;
    const before = snapshot();
    void applyRuns(next, { start: cursor, end: cursor }, { before, historyKind: "paste" });
  }

  function handleCompositionStart(): void {
    syncSelection();
    compositionSnapshot = snapshot();
    composing = true;
    if (!editor) return;
    const documentSelection = editor.ownerDocument.getSelection();
    const anchorNode = documentSelection?.anchorNode;
    const anchorElement = anchorNode instanceof Element ? anchorNode : anchorNode?.parentElement;
    const sentinel = anchorElement?.closest<HTMLElement>("[data-notes-editor-sentinel]");
    if (!sentinel || !editor.contains(sentinel) || !sentinel.parentNode) return;
    const parent = sentinel.parentNode;
    const index = Array.from(parent.childNodes).indexOf(sentinel);
    sentinel.remove();
    const range = editor.ownerDocument.createRange();
    range.setStart(parent, Math.max(0, index));
    range.collapse(true);
    documentSelection?.removeAllRanges();
    documentSelection?.addRange(range);
  }

  function handleCompositionEnd(): void {
    composing = false;
    if (!editor) return;
    const next = reconcileQuickNotePlainText(runs, notesPlainTextFromEditableRoot(editor));
    const nextSelection = notesTextSelectionFromEditableRoot(editor) ?? selection;
    const before = compositionSnapshot ?? snapshot();
    compositionSnapshot = null;
    void applyRuns(next, nextSelection, { before, historyKind: "insert" });
  }

  function handleInput(event: Event): void {
    const eventIsComposing = event instanceof InputEvent && event.isComposing;
    if (composing || eventIsComposing || !editor) return;
    const nextSelection = notesTextSelectionFromEditableRoot(editor) ?? selection;
    const next = reconcileQuickNotePlainText(runs, notesPlainTextFromEditableRoot(editor));
    const before = nativeInputSnapshot ?? snapshot();
    nativeInputSnapshot = null;
    void applyRuns(next, nextSelection, { before, historyKind: "replace" });
  }

  function updateTitle(next: string): void {
    title = next;
    limitError = "";
    markDirty();
  }

  function updateColor(next: QuickNote["color"]): void {
    color = next;
    markDirty();
  }

  function updateTag(next: string | null): void {
    tagId = next;
    markDirty();
  }

  function updatePinned(next: boolean): void {
    pinned = next;
    markDirty();
  }

  async function requestClose(): Promise<void> {
    if (transitionPending) return;
    transitionPending = true;
    try {
      await flush();
      onClose();
    } catch {
      // The visible save error keeps the editor open.
    } finally {
      transitionPending = false;
    }
  }

  async function requestLifecycle(action: (saved: QuickNote) => void): Promise<void> {
    if (transitionPending) return;
    transitionPending = true;
    try {
      await flush();
      if (persisted) action(persisted);
      else onClose();
    } catch {
      // The visible save error keeps the editor open.
    } finally {
      transitionPending = false;
    }
  }

  async function reloadConflict(): Promise<void> {
    if (!persisted) return;
    const fresh = await getQuickNote(persisted.id);
    persisted = fresh;
    revision = fresh.revision;
    title = fresh.title;
    runs = fresh.runs.map((run) => ({ ...run }));
    color = fresh.color;
    tagId = fresh.tagId;
    pinned = fresh.pinned;
    dirtyVersion += 1;
    savedVersion = dirtyVersion;
    status = "saved";
    onSaved(fresh);
  }

  async function saveAsCopy(): Promise<void> {
    id = crypto.randomUUID();
    persisted = null;
    revision = 0;
    pinned = false;
    dirtyVersion += 1;
    status = "idle";
    await flush();
  }

  function handleDialogKeydown(event: KeyboardEvent): void {
    if (obscured) return;
    const target = event.target instanceof Element ? event.target : null;
    const targetDialog = target?.closest("[role='dialog']") ?? null;
    if (targetDialog !== null && targetDialog !== dialog) return;
    if (event.key === "Tab" && dialog) {
      const focusable = [...dialog.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), [contenteditable='true'], [tabindex]:not([tabindex='-1'])",
      )].filter((element) => element.offsetParent !== null);
      const first = focusable[0];
      const last = focusable.at(-1);
      const active = document.activeElement;
      if (first && last && (active === dialog || !dialog.contains(active))) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (first && last && event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (first && last && !event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
      return;
    }
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    void requestClose();
  }

  onMount(() => {
    const returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const restoreTriggerFocus = returnFocus?.matches(":focus-visible") ?? false;
    const returnContainer = returnFocus?.closest<HTMLElement>("[role='dialog']") ?? null;
    const unregister = registerQuickNotesFlusher(flush);
    const deactivateMobileBack = mobileLayout
      ? mobileBackStack.activate({ handle: () => { void requestClose(); } })
      : () => undefined;
    window.addEventListener("keydown", handleDialogKeydown, true);
    // A new note starts typing; opening an existing one on a phone focuses the dialog so the keyboard stays closed.
    const focusText = note === null || OPENING_MAY_FOCUS_TEXT;
    void tick().then(() => (focusText ? titleInput ?? editor ?? dialog : dialog)?.focus());
    let mounted = true;
    let stopChanges: (() => void) | null = null;
    void listenForQuickNotesChanges(() => { void refreshFromCanonical(); })
      .then((stop) => {
        if (mounted) stopChanges = stop;
        else stop();
      })
      .catch((error: unknown) => console.warn("quick note changes could not be observed", error));
    return () => {
      mounted = false;
      stopChanges?.();
      unregister();
      deactivateMobileBack();
      if (saveTimer) clearTimeout(saveTimer);
      window.removeEventListener("keydown", handleDialogKeydown, true);
      queueMicrotask(() => {
        if (restoreTriggerFocus) returnFocus?.focus();
        else returnContainer?.focus({ preventScroll: true });
      });
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class={mobileLayout ? "surface-backdrop fixed z-80 flex items-center justify-center" : "surface-backdrop fixed inset-0 z-80 flex items-center justify-center p-2 sm:p-4"}
  style={overlayStyle}
  onclick={(event) => { if (event.target === event.currentTarget) void requestClose(); }}
>
  <div
    bind:this={dialog}
    class={mobileLayout ? "quick-note-editor flex min-h-0 flex-col overflow-hidden outline-none" : "quick-note-editor surface-dialog flex min-h-0 w-[min(720px,calc(100vw-1rem))] flex-col overflow-hidden outline-none"}
    style={dialogStyle}
    role="dialog"
    aria-modal="true"
    aria-label={note ? t("quickNotes.editor.editTitle") : t("quickNotes.editor.newTitle")}
    aria-hidden={obscured ? "true" : undefined}
    inert={obscured}
    tabindex="-1"
    data-app-shortcuts="ignore"
  >
    <div class={mobileLayout ? "flex min-h-0 flex-1 flex-col overflow-hidden px-4 pt-3" : "flex min-h-0 flex-1 flex-col overflow-hidden px-4 pt-4 sm:px-5 sm:pt-5"}>
      <div class="flex items-start gap-2">
        <input
          bind:this={titleInput}
          type="text"
          value={title}
          maxlength={QUICK_NOTE_TITLE_MAX_CHARS}
          readonly={readOnly}
          class={`min-w-0 flex-1 bg-transparent text-[1.05rem] font-semibold outline-none placeholder:opacity-55 ${mobileLayout ? "min-h-12" : ""}`}
          placeholder={t("quickNotes.editor.titlePlaceholder")}
          aria-label={t("quickNotes.editor.titlePlaceholder")}
          oninput={(event) => updateTitle(event.currentTarget.value)}
          onkeydown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              editor?.focus();
            }
          }}
        />
        {#if !readOnly}
          <button class={toolbarButton} type="button" aria-label={pinned ? t("quickNotes.action.unpin") : t("quickNotes.action.pin")} title={pinned ? t("quickNotes.action.unpin") : t("quickNotes.action.pin")} onclick={() => updatePinned(!pinned)}>
            {#if pinned}<PinOff class="size-4" strokeWidth={1.5} />{:else}<Pin class="size-4" strokeWidth={1.5} />{/if}
          </button>
        {/if}
      </div>
      <div
        bind:this={editor}
        use:scrollEdgeFadeAction
        data-selectable-content
        class="quick-note-content relative my-3 min-h-0 flex-1 overflow-y-auto rounded-md py-1 text-[0.92rem] leading-relaxed outline-none"
        class:cursor-default={readOnly}
        role="textbox"
        aria-multiline="true"
        aria-label={t("quickNotes.editor.bodyLabel")}
        aria-placeholder={t("quickNotes.editor.bodyPlaceholder")}
        contenteditable={!readOnly}
        spellcheck={!readOnly}
        tabindex="0"
        data-placeholder={t("quickNotes.editor.bodyPlaceholder")}
        data-empty={quickNotePlainText(runs).length === 0 ? "true" : undefined}
        onbeforeinput={handleBeforeInput}
        oninput={handleInput}
        onkeydown={handleEditorKeydown}
        onpaste={handlePaste}
        oncompositionstart={handleCompositionStart}
        oncompositionend={handleCompositionEnd}
        onkeyup={syncSelection}
        onpointerup={syncSelection}
        onclick={syncSelection}
      ><QuickNoteRichText {runs} /></div>
      {#if limitError}<p class="pb-2 text-xs text-destructive" role="alert">{limitError}</p>{/if}
      {#if persisted?.hasConflict && !readOnly}
        <QuickNoteConflictBanner
          noteId={persisted.id}
          revision={persisted.revision}
          onResolved={conflictResolved}
          onStale={() => void refreshFromCanonical()}
          {mobileLayout}
        />
      {/if}
      {#if status === "conflict"}
        <div class="mb-2 text-xs" data-quick-note-notice>
          <Notice message={t("quickNotes.editor.conflict")} tone="warning" copy={false}>
            {#snippet actions()}
              <button class="notice-action-text" type="button" onclick={() => void reloadConflict()}>{t("quickNotes.editor.reload")}</button>
              <button class="notice-action-text" type="button" onclick={() => void saveAsCopy()}>{t("quickNotes.editor.saveCopy")}</button>
            {/snippet}
          </Notice>
        </div>
      {:else if status === "failed"}
        <div class="mb-2 text-xs" data-quick-note-notice>
          <Notice message={t("quickNotes.editor.failed")} retryLabel={t("quickNotes.editor.retry")} copy={false} onRetry={flush} />
        </div>
      {/if}
    </div>

    <footer class={mobileLayout ? "flex shrink-0 flex-col border-t border-current/10 px-2 pb-2" : "flex min-h-12 shrink-0 flex-wrap items-center gap-0.5 border-t border-current/10 px-2.5 py-2 sm:px-4"}>
      <div class={mobileLayout ? "flex min-h-12 w-full min-w-0 items-center gap-0.5 overflow-x-auto" : "contents"}>
      {#if !readOnly}
        <button class={`${toolbarButton} ${selectedFormatting.bold ? "bg-black/10 dark:bg-white/10" : ""}`} type="button" aria-label={t("quickNotes.formatting.bold")} title={t("quickNotes.formatting.bold")} aria-pressed={selectedFormatting.bold} onpointerdown={(event) => event.preventDefault()} onclick={() => toggleFormatting("bold")}><Bold class="size-4" strokeWidth={1.5} /></button>
        <button class={`${toolbarButton} ${selectedFormatting.italic ? "bg-black/10 dark:bg-white/10" : ""}`} type="button" aria-label={t("quickNotes.formatting.italic")} title={t("quickNotes.formatting.italic")} aria-pressed={selectedFormatting.italic} onpointerdown={(event) => event.preventDefault()} onclick={() => toggleFormatting("italic")}><Italic class="size-4" strokeWidth={1.5} /></button>
        <button class={`${toolbarButton} ${selectedFormatting.underline ? "bg-black/10 dark:bg-white/10" : ""}`} type="button" aria-label={t("quickNotes.formatting.underline")} title={t("quickNotes.formatting.underline")} aria-pressed={selectedFormatting.underline} onpointerdown={(event) => event.preventDefault()} onclick={() => toggleFormatting("underline")}><Underline class="size-4" strokeWidth={1.5} /></button>
        <QuickNoteColorPicker {color} {theme} onSelect={updateColor} buttonClass={toolbarButton} {mobileLayout} />
        <QuickNoteTagPicker {tagId} {tags} onSelect={updateTag} buttonClass={toolbarButton} {mobileLayout} />
        <button class={toolbarButton} type="button" disabled={history.undo.length === 0} aria-label={t("quickNotes.formatting.undo")} title={t("quickNotes.formatting.undo")} onclick={undo}><Undo2 class="size-4" strokeWidth={1.5} /></button>
        <button class={toolbarButton} type="button" disabled={history.redo.length === 0} aria-label={t("quickNotes.formatting.redo")} title={t("quickNotes.formatting.redo")} onclick={redo}><Redo2 class="size-4" strokeWidth={1.5} /></button>
        <span class="mx-1 h-5 w-px bg-current/15"></span>
        {#if persisted?.archived}
          <button class={toolbarButton} type="button" disabled={transitionPending} aria-label={t("quickNotes.action.unarchive")} title={t("quickNotes.action.unarchive")} onclick={() => void requestLifecycle(onUnarchive)}><ArchiveRestore class="size-4" strokeWidth={1.5} /></button>
        {:else}
          <button class={toolbarButton} type="button" disabled={transitionPending} aria-label={t("quickNotes.action.archive")} title={t("quickNotes.action.archive")} onclick={() => void requestLifecycle(onArchive)}><Archive class="size-4" strokeWidth={1.5} /></button>
        {/if}
        <button class={toolbarButton} type="button" disabled={transitionPending} aria-label={t("quickNotes.action.trash")} title={t("quickNotes.action.trash")} onclick={() => void requestLifecycle(onTrash)}><Trash2 class="size-4" strokeWidth={1.5} /></button>
      {:else if persisted}
        <button class={toolbarButton} type="button" aria-label={t("quickNotes.action.restore")} title={t("quickNotes.action.restore")} onclick={() => onRestore(persisted!)}><RotateCcw class="size-4" strokeWidth={1.5} /></button>
        <button class={toolbarButton} type="button" aria-label={t("quickNotes.action.deletePermanently")} title={t("quickNotes.action.deletePermanently")} onclick={() => onDelete(persisted!)}><Trash2 class="size-4" strokeWidth={1.5} /></button>
      {/if}
      </div>
      <div class={mobileLayout ? "flex min-h-12 w-full items-center" : "contents"}>
      <button class={mobileLayout ? "ml-auto min-h-12 rounded-xl px-4 text-sm font-medium active:bg-black/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-current disabled:opacity-40 dark:active:bg-white/10" : "ml-auto rounded-md px-3 py-1.5 text-sm font-medium hover:bg-black/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-current disabled:opacity-40 dark:hover:bg-white/10"} type="button" disabled={transitionPending} onclick={() => void requestClose()}>{t("quickNotes.editor.close")}</button>
      </div>
    </footer>
  </div>
</div>

<style>
  .quick-note-editor {
    background: var(--quick-editor-bg);
    color: var(--quick-editor-fg);
  }

  .quick-note-content {
    caret-color: var(--quick-editor-fg);
  }

  /* Notices sit on the note color, so they follow its contrasting text instead of the app foreground. */
  .quick-note-editor :global([data-quick-note-notice]) {
    --foreground: var(--quick-editor-fg);
    --muted-foreground: color-mix(in srgb, var(--quick-editor-fg) 64%, transparent);
    --accent: color-mix(in srgb, var(--quick-editor-fg) 10%, transparent);
  }

  .quick-note-content :global([data-notes-editor-sentinel="empty-line"]) {
    caret-color: var(--quick-editor-fg);
  }

  .quick-note-content[data-empty="true"]::before {
    color: color-mix(in srgb, currentColor 52%, transparent);
    content: attr(data-placeholder);
    left: 0.125rem;
    pointer-events: none;
    position: absolute;
    top: 0.25rem;
  }
</style>
