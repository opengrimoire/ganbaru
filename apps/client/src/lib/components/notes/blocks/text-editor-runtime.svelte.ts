import { notesClipboardPasteHtml, readNotesClipboard } from "$lib/notes/clipboard/paste";
import { notesClipboardContent, setNotesClipboardData, writeNotesClipboard } from "$lib/notes/clipboard/export";
import { Temporal } from "@js-temporal/polyfill";
import { tick } from "svelte";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { CalendarTimeFormat } from "$lib/stores/preference-options";
import { blockColor, canBlockHaveColor } from "$lib/notes/blocks/color";
import {
  blockEditableRichText,
  blockPlainText,
  blockTextAnnotationsForSelection,
  blockTextLinkRangeForSelection,
  headingIsToggleable,
  headingToggleOpen,
  isHeadingBlockType,
  type NotesHeadingBlockType,
} from "$lib/notes/blocks/factory";
import { normalizeNotesClipboardPlainText, shouldHandleNotesPlainTextPaste } from "$lib/notes/clipboard/blocks";
import { isNotesTabKey, planNotesKeyboardAction, type NotesKeyboardAction } from "$lib/notes/blocks/keyboard";
import { shouldDeferNotesCompositionInput, shouldLetNativeCompositionHandleKeydown } from "$lib/notes/rich-text/composition";
import { planNotesControlledTextEdit } from "$lib/notes/rich-text/controlled-text-input";
import {
  normalizeNotesSelectionForAtomicRanges,
  notesAtomicRangeEndingAt,
  notesAtomicRangeStartingAt,
  notesAtomicTextRanges,
  snapNotesDirectionalSelection,
  type NotesAtomicTextRange,
} from "$lib/notes/rich-text/atomic-ranges";
import { notesSlashMenuItemDomId } from "$lib/notes/editor/accessibility";
import {
  beginLazyComponentLoad,
  rejectLazyComponentLoad,
  resolveLazyComponentLoad,
  type LazyComponentLoadState,
} from "$lib/lazy-component-loader";
import {
  clampNotesTextSelection,
  notesEditableSelectionViewportRect,
  notesEditableOffsetFromDomPoint,
  notesPlainTextFromEditableRoot,
  notesDirectionalSelectionFromEditableRoot,
  notesTextSelectionFromEditableRoot,
  planNotesSelectionReconciliation,
  restoreNotesEditableSelection,
  setNotesEditableDirectionalSelection,
  type NotesTextSelection,
} from "$lib/notes/editor/selection";
import { normalizeNotesTextLinkUrl, notesPastedTextLinkUrl, notesLinkTarget, openNotesTextLink, type NotesLinkOpener } from "$lib/notes/links/navigation";
import { writeTextToClipboard } from "$lib/utils/clipboard";
import type { NotesFloatingPanelRect } from "$lib/notes/editor/floating-panel";
import { notesTextContextMenuSelectionAtPoint } from "$lib/notes/editor/text-context-menu";
import {
  buildDateMentionTargets,
  buildIdleDateMentionTargets,
  detectPageMentionQuery,
  editRichTextLink,
  isNotesDateMentionRichText,
  planRichTextEquationConversion,
  replacePlainTextPreservingRichText,
  richTextPlainText,
  richTextAnnotationTogglePatch,
  richTextColorPatch,
  type NotesDateMentionTarget,
  type NotesInlineEquationConversionError,
  type NotesInlineEquationConversionPlan,
  type NotesMentionQuery,
  type NotesMentionTarget,
  type NotesNamedMentionTarget,
  type NotesObjectMentionTarget,
  type NotesPageMentionTarget,
  type NotesRichTextAnnotationName,
  type NotesRichTextAnnotationPatch,
} from "$lib/notes/rich-text/core";
import { planNotesMarkdownInlineShortcutConversion } from "$lib/notes/rich-text/markdown";
import {
  buildNotesMentionGroups,
  flattenNotesMentionGroups,
  type NotesMentionGroup,
  type NotesMentionGroupKind,
  type NotesMentionRow,
} from "$lib/notes/rich-text/mention-groups";
import {
  notesRichTextEquationShortcutRequested,
  notesRichTextFormattingShortcutAnnotationName,
  notesRichTextLinkShortcutRequested,
} from "$lib/notes/rich-text/shortcuts";
import {
  nextNotesSlashActiveIndex,
  notesSlashCommandKey,
  notesSlashInputSessionFromText,
  recordRecentNotesSlashCommandKey,
  type NotesSlashAction,
  type NotesSlashCommand,
} from "$lib/notes/editor/slash-commands";
import type {
  NotesBlock,
  NotesBlockType,
  NotesColor,
  NotesDateMentionRichText,
  NotesRichText,
} from "$lib/notes/types";
import { notesUndoShortcutAction } from "$lib/notes/history/undo-history";
import {
  loadNotesTextControl,
  retryNotesTextControl,
  type LoadedNotesTextControl,
  type NotesTextControlKind,
} from "$lib/components/notes/editor-component-registry";

export interface NotesTextEditorRuntimeSource {
  block: () => NotesBlock;
  editableRichText: () => readonly NotesRichText[];
  focusBlockId: () => string | null;
  focusRequestId: () => number;
  focusSelection: () => NotesTextSelection | null;
  contextMenuOpen: () => boolean;
  linkEditorOpen: () => boolean;
  mentionOpen: () => boolean;
  slashOpen: () => boolean;
  templateControlsOpen: () => boolean;
  buttonControlsOpen: () => boolean;
  dateMentionEditorOpen: () => boolean;
}

export interface NotesTextEditorControllerSource {
  block: () => NotesBlock;
  previousBlockType: () => NotesBlockType | null;
  isOnlyBlock: () => boolean;
  indentationDepth: () => number;
  focusBlockId: () => string | null;
  focusRequestId: () => number;
  focusSelection: () => NotesTextSelection | null;
  mentionTargets: () => NotesNamedMentionTarget[];
  locale: () => string;
  /** The user's clock format, used for times typed into date mentions. */
  timeFormat: () => CalendarTimeFormat;
  translate: Translate;
  onTextInput: (blockId: string, text: string, selection: NotesTextSelection | null) => void;
  onReplaceRichText: (blockId: string, richText: readonly NotesRichText[]) => Promise<void> | void;
  onInsertPageMention: (blockId: string, start: number, end: number, target: NotesPageMentionTarget) => Promise<void> | void;
  onInsertDateMention: (blockId: string, start: number, end: number, target: NotesDateMentionTarget) => Promise<void> | void;
  onInsertObjectMention: (blockId: string, start: number, end: number, target: NotesObjectMentionTarget) => Promise<void> | void;
  onApplyTextLink: (blockId: string, start: number, end: number, url: string | null) => Promise<void> | void;
  /** Opens local links through the Notes navigation of the editor this text belongs to. */
  notes: NotesLinkOpener;
  onInsertInlineEquation: (blockId: string, start: number, end: number, expression: string) => Promise<void> | void;
  onPastePlainText: (blockId: string, start: number, end: number, plainText: string) => Promise<boolean> | boolean;
  onPasteRichHtml: (blockId: string, start: number, end: number, html: string) => Promise<boolean> | boolean;
  onApplyTextAnnotations: (blockId: string, start: number, end: number, patch: NotesRichTextAnnotationPatch) => Promise<void> | void;
  onCreateInlineComment: (blockId: string, start: number, end: number) => Promise<void> | void;
  onCreateInlineSuggestion: (blockId: string, start: number, end: number) => Promise<void> | void;
  onKeyboardAction: (blockId: string, action: NotesKeyboardAction) => void;
  onUndo: () => Promise<void> | void;
  onRedo: () => Promise<void> | void;
  onConvert: (blockId: string, type: NotesBlockType, clearText?: boolean) => void;
  onConvertToToggleHeading: (blockId: string, type: NotesHeadingBlockType, clearText?: boolean) => void;
  onColorChange: (blockId: string, color: NotesColor) => void;
  onCopyLink: (blockId: string) => Promise<void> | void;
  onDuplicate: (blockId: string) => void;
  onMoveUp: (blockId: string) => void;
  onMoveDown: (blockId: string) => void;
  onDelete: (blockId: string) => void;
  onToggleOpen: (blockId: string, open: boolean) => void;
  onFocusBlock: (blockId: string, preventScroll?: boolean) => void;
}

export interface NotesTextEditorVisibleControls {
  textContextMenu: boolean;
  linkEditor: boolean;
  mentionMenu: boolean;
  slashMenu: boolean;
  templateControls: boolean;
  buttonControls: boolean;
  dateMentionEditor: boolean;
}

export interface NotesTextInputMenuState {
  slashOpen: boolean;
  mentionQuery: NotesMentionQuery | null;
}

/** Plans mutually exclusive slash and mention menus after a controlled text edit. */
export function planNotesTextInputMenuState(
  value: string,
  selection: NotesTextSelection | null,
  slashWasOpen: boolean,
  canUseMentions: boolean,
  previousText?: string,
): NotesTextInputMenuState {
  const slashOpen = canUseMentions && notesSlashInputSessionFromText(value, slashWasOpen, previousText).open;
  return {
    slashOpen,
    mentionQuery: slashOpen || !canUseMentions || !selection
      ? null
      : detectPageMentionQuery(value, selection.start, selection.end),
  };
}

/** Returns the independently lazy editor controls required by current UI state. */
export function requestedNotesTextControls(
  visible: NotesTextEditorVisibleControls,
): NotesTextControlKind[] {
  const controls: NotesTextControlKind[] = [];
  if (visible.textContextMenu) controls.push("text-context-menu");
  if (visible.linkEditor) controls.push("link-editor");
  if (visible.mentionMenu) controls.push("mention-menu");
  if (visible.slashMenu) controls.push("slash-menu");
  if (visible.templateControls) controls.push("template-controls");
  if (visible.buttonControls) controls.push("button-controls");
  if (visible.dateMentionEditor) controls.push("date-mention-editor");
  return controls;
}

/**
 * Owns the DOM-sensitive lifecycle shared by every Notes text control.
 *
 * `NotesTextEditorController` owns command routing while this runtime owns
 * selection reconciliation, composition state, and lazy control loading.
 * Keeping those concerns together prevents delayed component loads from
 * stealing focus or restoring a stale selection.
 */
export class NotesTextEditorRuntime {
  editor: HTMLDivElement | null = $state(null);
  textSelection = $state<NotesTextSelection>({ start: 0, end: 0 });
  compositionActive = $state(false);
  controlLoadStates = $state<Partial<Record<
    NotesTextControlKind,
    LazyComponentLoadState<NotesTextControlKind, LoadedNotesTextControl>
  >>>({});

  #hasTextSelection = false;
  #appliedFocusRequestId = -1;

  constructor(private readonly source: NotesTextEditorRuntimeSource) {
    $effect(() => {
      const requested = requestedNotesTextControls({
        textContextMenu: source.contextMenuOpen(),
        linkEditor: source.linkEditorOpen(),
        mentionMenu: source.mentionOpen(),
        slashMenu: source.slashOpen(),
        templateControls: source.templateControlsOpen(),
        buttonControls: source.buttonControlsOpen(),
        dateMentionEditor: source.dateMentionEditorOpen(),
      });
      for (const kind of requested) this.requestControl(kind);
    });

    $effect(() => {
      const block = source.block();
      const requestedFocusId = source.focusRequestId();
      const focusRequestedForEditor = source.focusBlockId() === block.id;
      const requestedSelection = source.focusSelection();
      void block.type;
      void block.last_edited_time;
      void source.editableRichText();
      let cancelled = false;
      void tick().then(() => {
        if (cancelled || !this.editor || this.compositionActive || requestedFocusId !== source.focusRequestId()) return;
        if (this.editor.closest("[data-notes-document-selection]")) {
          this.#appliedFocusRequestId = requestedFocusId;
          return;
        }
        const length = notesPlainTextFromEditableRoot(this.editor).length;
        const plan = planNotesSelectionReconciliation({
          focusRequestIsNew: requestedFocusId !== this.#appliedFocusRequestId,
          focusRequestedForEditor,
          requestedSelection,
          currentSelection: this.#hasTextSelection ? this.textSelection : null,
          textLength: length,
          editorActive: document.activeElement === this.editor,
        });
        this.#appliedFocusRequestId = requestedFocusId;
        if (!plan) return;
        if (plan.focusEditor) this.editor.focus({ preventScroll: true });
        this.restoreTrackedSelection(plan.selection);
      });
      return () => { cancelled = true; };
    });

  }

  requestControl(kind: NotesTextControlKind, retry = false): void {
    const current = this.controlLoadStates[kind] ?? null;
    if (!retry && current?.key === kind) return;
    const loadingState = beginLazyComponentLoad(current, kind);
    this.controlLoadStates = { ...this.controlLoadStates, [kind]: loadingState };
    const request = retry ? retryNotesTextControl(kind) : loadNotesTextControl(kind);
    void request.then((component) => {
      const latest = this.controlLoadStates[kind];
      if (!latest) return;
      this.controlLoadStates = {
        ...this.controlLoadStates,
        [kind]: resolveLazyComponentLoad(latest, kind, loadingState.requestId, component),
      };
    }).catch((error: unknown) => {
      const latest = this.controlLoadStates[kind];
      if (!latest) return;
      this.controlLoadStates = {
        ...this.controlLoadStates,
        [kind]: rejectLazyComponentLoad(latest, kind, loadingState.requestId, error),
      };
      console.error(`load Notes ${kind} control failed`, error);
    });
  }

  setTrackedSelection(selection: NotesTextSelection): void {
    this.textSelection = selection;
    this.#hasTextSelection = true;
  }

  restoreTrackedSelection(selection: NotesTextSelection): void {
    if (!this.editor) return;
    const safeSelection = clampNotesTextSelection(
      selection,
      notesPlainTextFromEditableRoot(this.editor).length,
    );
    restoreNotesEditableSelection(this.editor, safeSelection);
    this.setTrackedSelection(safeSelection);
  }

  async focusEditorWithSelection(start: number, end: number): Promise<void> {
    await tick();
    this.editor?.focus();
    if (!this.editor) {
      this.setTrackedSelection({ start, end });
      return;
    }
    this.restoreTrackedSelection({ start, end });
  }
}

export function createNotesTextEditorRuntime(
  source: NotesTextEditorRuntimeSource,
): NotesTextEditorRuntime {
  return new NotesTextEditorRuntime(source);
}

const NOTES_LINK_HOVER_CLOSE_DELAY_MS = 150;

/** Owns editing state and command routing for one text block on top of its DOM runtime. */
export class NotesTextEditorController {
  readonly runtime: NotesTextEditorRuntime;
  #rightClickSelection: NotesTextSelection | null = null;
  #shouldPasteAsPlainText = false;

  slashOpen = $state(false);
  #shouldResumeSlashAfterComposition = false;
  slashActiveIndex = $state(0);
  slashActiveCommand = $state<NotesSlashCommand | null>(null);
  slashItemCount = $state(0);
  mentionQuery = $state<NotesMentionQuery | null>(null);
  mentionActiveIndex = $state(0);
  /** Sections revealed through their "more" row, tied to the at sign that opened them. */
  mentionExpanded = $state<{ start: number; groups: ReadonlySet<NotesMentionGroupKind> } | null>(null);
  /** The typed mention range an open invitation panel would replace; it outlives editor blur. */
  mentionInvite = $state<NotesMentionQuery | null>(null);
  linkEditorOpen = $state(false);
  linkRange = $state({ start: 0, end: 0, url: null as string | null });
  linkUrlInput = $state("");
  linkTitleInput = $state("");
  linkMode = $state<"preview" | "edit">("edit");
  linkBusy = $state(false);
  #linkAnchorElement: HTMLElement | null = null;
  #linkAnchorRange: Range | null = null;
  #linkSourceText = "";
  #linkHoverCloseTimer: ReturnType<typeof setTimeout> | null = null;
  linkError = $state<string | null>(null);
  contextMenuOpen = $state(false);
  contextMenuPoint = $state<{ x: number; y: number } | null>(null);
  contextMenuFocusOnOpen = $state(false);
  inlineEquationErrorReason = $state<NotesInlineEquationConversionError | null>(null);
  templateControlsOpen = $state(false);
  buttonControlsOpen = $state(false);
  /** The date mention whose editor is open, by its index in the block's rich text, and the chip it opened from. */
  dateMentionEdit = $state<{ index: number; anchor: HTMLElement } | null>(null);

  constructor(private readonly source: NotesTextEditorControllerSource) {
    this.runtime = createNotesTextEditorRuntime({
      block: source.block,
      editableRichText: () => this.editableRichText,
      focusBlockId: source.focusBlockId,
      focusRequestId: source.focusRequestId,
      focusSelection: source.focusSelection,
      contextMenuOpen: () => this.contextMenuOpen,
      linkEditorOpen: () => this.linkEditorOpen,
      mentionOpen: () => this.mentionOpen || this.mentionInvite !== null,
      slashOpen: () => this.slashOpen,
      templateControlsOpen: () => this.templateControlsOpen,
      buttonControlsOpen: () => this.buttonControlsOpen,
      dateMentionEditorOpen: () => this.dateMentionEdit !== null,
    });

    $effect(() => {
      if (this.mentionActiveIndex >= this.mentionMatches.length) this.mentionActiveIndex = 0;
    });

    $effect(() => {
      void this.slashQuery;
      void this.slashOpen;
      this.slashActiveIndex = 0;
    });

    $effect(() => () => this.keepLinkPreviewOpen());
  }

  get block(): NotesBlock { return this.source.block(); }
  get text(): string { return blockPlainText(this.block); }
  get editableRichText(): readonly NotesRichText[] { return blockEditableRichText(this.block); }
  get textSelection(): NotesTextSelection { return this.runtime.textSelection; }
  get canUseMentions(): boolean { return this.block.type !== "code"; }
  get canUseLinks(): boolean { return this.canUseMentions; }
  get canUseInlineFormatting(): boolean { return this.canUseMentions; }
  get currentTextAnnotationRange() {
    return blockTextAnnotationsForSelection(this.block, this.textSelection.start, this.textSelection.end);
  }
  get currentTextLinkRange() {
    return blockTextLinkRangeForSelection(this.block, this.textSelection.start, this.textSelection.end);
  }
  get hasTextSelection(): boolean {
    return this.textSelection.start !== this.textSelection.end;
  }
  get canFormatSelection(): boolean { return this.canUseInlineFormatting && this.hasTextSelection; }
  get canOpenLinkEditor(): boolean {
    return this.canUseLinks && (
      this.textSelection.start !== this.textSelection.end
      || this.currentTextLinkRange.url !== null
      || this.linkEditorOpen
    );
  }
  get linkPageTargets(): NotesPageMentionTarget[] {
    return this.linkEditorOpen ? this.source.mentionTargets().filter((target) => target.kind === "page") : [];
  }
  get linkDestinationTitle(): string {
    const target = notesLinkTarget(this.linkUrlInput, typeof window === "undefined" ? undefined : window.location.href);
    if (!target) return this.linkUrlInput;
    return target.blockId ? this.linkTitleInput
      : this.linkPageTargets.find((page) => page.id === target.pageId)?.title || this.linkTitleInput;
  }

  /** Re-read the original link or selection geometry as the note scrolls. */
  readLinkAnchor = (): NotesFloatingPanelRect | null => {
    return this.#linkAnchorElement?.isConnected ? this.#linkAnchorElement.getBoundingClientRect()
      : this.#linkAnchorRange?.getBoundingClientRect?.() ?? this.runtime.editor?.getBoundingClientRect() ?? null;
  };
  get currentColor(): NotesColor { return blockColor(this.block); }
  get blockSupportsColor(): boolean { return canBlockHaveColor(this.block.type); }
  get headingToggleable(): boolean {
    return isHeadingBlockType(this.block.type) && headingIsToggleable(this.block);
  }
  get headingOpen(): boolean {
    return !isHeadingBlockType(this.block.type) || headingToggleOpen(this.block);
  }
  get slashInputSession() { return notesSlashInputSessionFromText(this.text, this.slashOpen); }
  get slashQuery(): string { return this.slashInputSession.query; }
  get slashActiveDescendant(): string | undefined {
    return this.slashOpen && this.slashItemCount > 0
      ? notesSlashMenuItemDomId(this.block.id, this.slashActiveIndex)
      : undefined;
  }
  get mentionOpen(): boolean { return this.mentionQuery !== null && this.canUseMentions; }
  get mentionGroups(): NotesMentionGroup[] {
    const mention = this.mentionQuery;
    if (!mention || !this.canUseMentions) return [];
    const t = this.source.translate;
    const today = Temporal.Now.plainDateISO();
    const labels = {
      today: t("notes.dateMentionToday"),
      tomorrow: t("notes.dateMentionTomorrow"),
      yesterday: t("notes.dateMentionYesterday"),
      nextWeek: t("notes.dateMentionNextWeek"),
      date: t("notes.dateMention"),
      reminder: t("notes.reminderMention"),
      remindTitle: (dateLabel: string) => t("notes.remindOnDate", dateLabel),
      atTime: (dateLabel: string, time: string) => t("notes.dateMentionAtTime", dateLabel, time),
    };
    const dateTargets = mention.query.trim()
      ? buildDateMentionTargets(mention.query, {
          today,
          locale: this.source.locale(),
          labels,
          limit: 4,
          timeFormat: this.source.timeFormat(),
        })
      : buildIdleDateMentionTargets({ today, labels });
    return buildNotesMentionGroups({
      query: mention.query,
      dateTargets,
      namedTargets: this.source.mentionTargets(),
      expanded: this.mentionExpanded?.start === mention.start ? this.mentionExpanded.groups : undefined,
      canInvite: true,
    });
  }
  /** Menu rows in keyboard order, including the invitation and "more" rows. */
  get mentionMatches(): NotesMentionRow[] { return flattenNotesMentionGroups(this.mentionGroups); }

  closeCompositionSensitiveMenus(): void {
    this.slashOpen = false;
    this.slashActiveIndex = 0;
    this.slashActiveCommand = null;
    this.slashItemCount = 0;
    this.mentionQuery = null;
    this.mentionActiveIndex = 0;
  }

  closeSlashMenu(): void {
    this.#shouldResumeSlashAfterComposition = false;
    this.slashOpen = false;
    this.slashActiveIndex = 0;
    this.slashActiveCommand = null;
    this.slashItemCount = 0;
  }

  openTemplateControls = (): void => {
    this.templateControlsOpen = true;
    if (this.runtime.controlLoadStates["template-controls"]?.status === "failed") {
      this.runtime.requestControl("template-controls", true);
    }
  };

  openButtonControls = (): void => {
    this.buttonControlsOpen = true;
    if (this.runtime.controlLoadStates["button-controls"]?.status === "failed") {
      this.runtime.requestControl("button-controls", true);
    }
  };

  /** The date mention being edited, while the rich text still has a date mention at the edited index. */
  get editedDateMention(): NotesDateMentionRichText | null {
    const edit = this.dateMentionEdit;
    const item = edit ? this.editableRichText[edit.index] : undefined;
    return isNotesDateMentionRichText(item) ? item : null;
  }

  /** The chip the date editor follows, or the editing surface once a re-render has replaced the chip. */
  get dateMentionAnchor(): HTMLElement | undefined {
    const anchor = this.dateMentionEdit?.anchor;
    return anchor?.isConnected ? anchor : this.runtime.editor ?? anchor;
  }

  openDateMentionEditor = (item: NotesDateMentionRichText, anchor: HTMLElement): void => {
    const index = this.editableRichText.indexOf(item);
    if (index < 0 || !this.canUseMentions) return;
    this.cancelLinkEditor();
    this.closeCompositionSensitiveMenus();
    this.mentionInvite = null;
    this.contextMenuOpen = false;
    this.dateMentionEdit = { index, anchor };
    if (this.runtime.controlLoadStates["date-mention-editor"]?.status === "failed") {
      this.runtime.requestControl("date-mention-editor", true);
    }
  };

  /** Replace the edited date mention with its next value, or remove it when the next value is null. */
  changeDateMention = (next: NotesDateMentionRichText | null): void => {
    const edit = this.dateMentionEdit;
    if (!edit || !this.editedDateMention) return;
    const richText = [...this.editableRichText];
    if (next) richText[edit.index] = next;
    else richText.splice(edit.index, 1);
    const saved = Promise.resolve(this.source.onReplaceRichText(this.block.id, richText))
      .catch((error: unknown) => console.warn("Notes date mention edit failed", error));
    if (next) return;
    this.dateMentionEdit = null;
    const cursor = richTextPlainText(richText.slice(0, edit.index)).length;
    void saved.then(() => this.runtime.focusEditorWithSelection(cursor, cursor));
  };

  /** Close the date editor; a keyboard close returns the caret to just after the chip. */
  closeDateMentionEditor = (refocus: boolean): void => {
    const edit = this.dateMentionEdit;
    if (!edit) return;
    this.dateMentionEdit = null;
    if (!refocus) return;
    const cursor = richTextPlainText(this.editableRichText.slice(0, edit.index + 1)).length;
    void this.runtime.focusEditorWithSelection(cursor, cursor);
  };

  handleCompositionStart = (): void => {
    this.cancelLinkEditor();
    this.#shouldResumeSlashAfterComposition = this.slashOpen;
    this.runtime.compositionActive = true;
    this.closeCompositionSensitiveMenus();
  };

  updateLinkInput = (value: string): void => {
    this.linkUrlInput = value;
    this.linkError = null;
  };

  updateLinkTitle = (value: string): void => {
    this.linkTitleInput = value;
    this.linkError = null;
  };

  cancelLinkEditor = (restoreFocus = false): void => {
    this.keepLinkPreviewOpen();
    this.linkEditorOpen = false;
    this.#linkAnchorElement = null;
    this.#linkAnchorRange = null;
    if (restoreFocus) void this.runtime.focusEditorWithSelection(this.linkRange.start, this.linkRange.end);
  };

  updateSlashActiveIndex = (index: number): void => {
    this.slashActiveIndex = index;
  };

  updateSlashActiveCommand = (command: NotesSlashCommand | null, itemCount: number): void => {
    this.slashActiveCommand = command;
    this.slashItemCount = itemCount;
  };

  /** Apply a formatting patch, widening the range so it never splits a mention. */
  async applyTextAnnotationsToRange(
    rangeStart: number,
    rangeEnd: number,
    patch: NotesRichTextAnnotationPatch,
  ): Promise<void> {
    const { start, end } = normalizeNotesSelectionForAtomicRanges(
      this.atomicRangesFor(this.text),
      { start: rangeStart, end: rangeEnd },
    );
    if (start === end) return;
    this.inlineEquationErrorReason = null;
    await Promise.resolve(this.source.onApplyTextAnnotations(this.block.id, start, end, patch));
    await this.runtime.focusEditorWithSelection(start, end);
  }

  inlineEquationErrorMessage(reason: NotesInlineEquationConversionError): string {
    return reason === "selection_required"
      ? this.source.translate("notes.inlineEquationSelectionRequired")
      : this.source.translate("notes.inlineEquationInvalid");
  }

  async applyInlineEquationConversion(plan: NotesInlineEquationConversionPlan): Promise<void> {
    if (plan.type === "error") {
      this.inlineEquationErrorReason = plan.reason;
      this.linkEditorOpen = false;
      this.slashOpen = false;
      this.mentionQuery = null;
      await this.runtime.focusEditorWithSelection(plan.start, plan.end);
      return;
    }
    this.inlineEquationErrorReason = null;
    await Promise.resolve(this.source.onInsertInlineEquation(
      this.block.id,
      plan.start,
      plan.end,
      plan.expression,
    ));
    await this.runtime.focusEditorWithSelection(plan.cursor, plan.cursor);
  }

  insertInlineEquationFromSelection(): void {
    void this.applyInlineEquationConversion(planRichTextEquationConversion(
      this.text,
      this.currentTextAnnotationRange.start,
      this.currentTextAnnotationRange.end,
    ));
  }

  insertInlineEquationFromEditor(target: EventTarget | null): boolean {
    if (!this.canUseInlineFormatting || !(target instanceof HTMLElement)) return false;
    this.syncTextSelection(target);
    const selection = notesTextSelectionFromEditableRoot(target);
    if (!selection) return false;
    void this.applyInlineEquationConversion(
      planRichTextEquationConversion(this.text, selection.start, selection.end),
    );
    return true;
  }

  createInlineCommentFromSelection(): void {
    if (!this.canFormatSelection) return;
    const { start, end } = this.textSelection;
    this.slashOpen = false;
    this.mentionQuery = null;
    void Promise.resolve(this.source.onCreateInlineComment(this.block.id, start, end))
      .then(() => this.runtime.focusEditorWithSelection(start, end));
  }

  createInlineSuggestionFromSelection(): void {
    if (!this.canFormatSelection) return;
    const { start, end } = this.textSelection;
    this.slashOpen = false;
    this.mentionQuery = null;
    void Promise.resolve(this.source.onCreateInlineSuggestion(this.block.id, start, end))
      .then(() => this.runtime.focusEditorWithSelection(start, end));
  }

  toggleTextAnnotation(name: NotesRichTextAnnotationName): void {
    const range = this.currentTextAnnotationRange;
    void this.applyTextAnnotationsToRange(
      range.start,
      range.end,
      richTextAnnotationTogglePatch(range.annotations, name),
    );
  }

  applyTextColor(color: NotesColor): void {
    const range = this.currentTextAnnotationRange;
    void this.applyTextAnnotationsToRange(range.start, range.end, richTextColorPatch(color));
  }

  applyAnnotationShortcut(event: KeyboardEvent): boolean {
    const name = notesRichTextFormattingShortcutAnnotationName(event);
    if (!name || !this.canUseInlineFormatting || !(event.currentTarget instanceof HTMLElement)) {
      return false;
    }
    const selection = notesTextSelectionFromEditableRoot(event.currentTarget);
    if (!selection || selection.start === selection.end) return false;
    const range = blockTextAnnotationsForSelection(this.block, selection.start, selection.end);
    void this.applyTextAnnotationsToRange(
      selection.start,
      selection.end,
      richTextAnnotationTogglePatch(range.annotations, name),
    );
    this.syncTextSelection(event.currentTarget);
    event.preventDefault();
    return true;
  }

  handleKeydown = (event: KeyboardEvent): void => {
    if (shouldLetNativeCompositionHandleKeydown({
      active: this.runtime.compositionActive,
      eventIsComposing: event.isComposing,
      key: event.key,
    })) return;
    if (this.linkEditorOpen && event.key === "Escape") {
      event.preventDefault();
      this.cancelLinkEditor(true);
      return;
    }
    this.#shouldPasteAsPlainText = (event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === "v";
    const undoAction = notesUndoShortcutAction(event);
    if (undoAction) {
      event.preventDefault();
      void Promise.resolve(undoAction === "undo" ? this.source.onUndo() : this.source.onRedo());
      return;
    }
    if (notesRichTextLinkShortcutRequested(event)) {
      if (this.openLinkEditorFromEditor(event.currentTarget)) event.preventDefault();
      return;
    }
    if (notesRichTextEquationShortcutRequested(event)) {
      if (this.insertInlineEquationFromEditor(event.currentTarget)) event.preventDefault();
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter" && this.headingToggleable) {
      event.preventDefault();
      this.source.onToggleOpen(this.block.id, !this.headingOpen);
      return;
    }
    if (this.applyAnnotationShortcut(event)) return;
    if (this.routeMentionKey(event) || this.routeSlashKey(event)) return;
    if (this.moveAcrossAtomicRange(event)) return;
    const target = event.currentTarget;
    const selection = target instanceof HTMLElement ? notesTextSelectionFromEditableRoot(target) : null;
    const currentText = target instanceof HTMLElement ? notesPlainTextFromEditableRoot(target) : this.text;
    const selectionStart = selection?.start ?? currentText.length;
    const selectionEnd = selection?.end ?? selectionStart;
    const action = planNotesKeyboardAction({
      key: event.key,
      code: event.code,
      shiftKey: event.shiftKey,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      altKey: event.altKey,
      text: currentText,
      selectionStart,
      selectionEnd,
      blockType: this.block.type,
      previousBlockType: this.source.previousBlockType(),
      isOnlyBlock: this.source.isOnlyBlock(),
      indentationDepth: this.source.indentationDepth(),
    });
    if (action.type === "none") return;
    if (action.type === "replace_text") {
      event.preventDefault();
      this.commitPlainTextValue(action.text, action.selection);
      return;
    }
    if (action.type === "insert_newline") {
      event.preventDefault();
      const edit = planNotesControlledTextEdit({
        inputType: "insertLineBreak",
        data: null,
        text: currentText,
        selectionStart,
        selectionEnd,
        atomicRanges: this.atomicRangesFor(currentText),
      });
      if (edit) this.commitPlainTextValue(edit.text, edit.selection);
      return;
    }
    if (action.type === "open_slash_menu") {
      this.slashOpen = true;
      this.mentionQuery = null;
      return;
    }
    if (action.preventDefault) event.preventDefault();
    this.slashOpen = false;
    this.mentionQuery = null;
    this.source.onKeyboardAction(this.block.id,
      action.type === "nest" || action.type === "outdent"
        ? { ...action, selection: { start: selectionStart, end: selectionEnd } }
        : action);
  };

  private routeMentionKey(event: KeyboardEvent): boolean {
    if (!this.mentionOpen) return false;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const count = this.mentionMatches.length;
      this.mentionActiveIndex = count === 0
        ? 0
        : (this.mentionActiveIndex + (event.key === "ArrowDown" ? 1 : count - 1)) % count;
      return true;
    }
    if (event.key === "Enter" || isNotesTabKey(event)) {
      const row = this.mentionMatches[this.mentionActiveIndex];
      if (row) {
        event.preventDefault();
        this.selectMentionRow(row);
      }
      return true;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      this.mentionQuery = null;
      this.mentionActiveIndex = 0;
      return true;
    }
    return false;
  }

  private routeSlashKey(event: KeyboardEvent): boolean {
    if (!this.slashOpen) return false;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      this.slashActiveIndex = nextNotesSlashActiveIndex(
        this.slashActiveIndex,
        this.slashItemCount,
        event.key === "ArrowDown" ? "next" : "previous",
      );
      return true;
    }
    if (event.key === "Enter" || isNotesTabKey(event)) {
      event.preventDefault();
      if (this.slashActiveCommand) this.selectSlashCommand(this.slashActiveCommand);
      return true;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      this.closeSlashMenu();
      return true;
    }
    return false;
  }

  handleBeforeInput = (event: InputEvent): void => {
    if (event.isComposing || this.runtime.compositionActive) return;
    this.cancelLinkEditor();
    if (event.inputType === "historyUndo" || event.inputType === "historyRedo") {
      event.preventDefault();
      void Promise.resolve(event.inputType === "historyUndo" ? this.source.onUndo() : this.source.onRedo());
      return;
    }
    if (!(event.currentTarget instanceof HTMLElement)) return;
    const currentText = notesPlainTextFromEditableRoot(event.currentTarget);
    const selection = notesTextSelectionFromEditableRoot(event.currentTarget);
    const selectionStart = selection?.start ?? currentText.length;
    const selectionEnd = selection?.end ?? selectionStart;
    const action: NotesKeyboardAction = event.inputType === "insertParagraph" || event.inputType === "deleteContentBackward"
      ? planNotesKeyboardAction({
        key: event.inputType === "deleteContentBackward" ? "Backspace" : "Enter",
        shiftKey: false,
        ctrlKey: false,
        metaKey: false,
        altKey: false,
        text: currentText,
        selectionStart,
        selectionEnd,
        blockType: this.block.type,
        previousBlockType: this.source.previousBlockType(),
        isOnlyBlock: this.source.isOnlyBlock(),
        indentationDepth: this.source.indentationDepth(),
      }) : { type: "none" };
    if (event.inputType === "deleteContentBackward" && action.type !== "none") {
      event.preventDefault();
      this.slashOpen = false;
      this.mentionQuery = null;
      this.source.onKeyboardAction(this.block.id, action);
      return;
    }
    if (event.inputType !== "insertParagraph") {
      const edit = planNotesControlledTextEdit({
        inputType: event.inputType,
        data: event.data,
        text: currentText,
        selectionStart,
        selectionEnd,
        atomicRanges: this.atomicRangesFor(currentText),
      });
      if (!edit) return;
      event.preventDefault();
      this.commitPlainTextValue(edit.text, edit.selection);
      return;
    }
    if (this.slashOpen) {
      event.preventDefault();
      if (this.slashActiveCommand) this.selectSlashCommand(this.slashActiveCommand);
      return;
    }
    if (this.mentionOpen) return;

    if (action.type === "insert_newline") {
      const edit = planNotesControlledTextEdit({
        inputType: event.inputType,
        data: event.data,
        text: currentText,
        selectionStart,
        selectionEnd,
        atomicRanges: this.atomicRangesFor(currentText),
      });
      if (!edit) return;
      event.preventDefault();
      this.commitPlainTextValue(edit.text, edit.selection);
      return;
    }
    if (action.type === "none" || action.type === "open_slash_menu") return;
    event.preventDefault();
    this.slashOpen = false;
    this.mentionQuery = null;
    this.source.onKeyboardAction(this.block.id, action);
  };

  updateMentionQueryFromText(plainText: string, selection: NotesTextSelection | null): void {
    if (!this.canUseMentions || !selection) {
      this.mentionQuery = null;
      this.mentionActiveIndex = 0;
      return;
    }
    this.mentionQuery = detectPageMentionQuery(plainText, selection.start, selection.end);
    this.mentionActiveIndex = 0;
  }

  /** Track the native selection after snapping any end that falls inside a mention to its edge. */
  syncTextSelection = (target: EventTarget | null): void => {
    if (!(target instanceof HTMLElement)) return;
    this.snapEditableSelectionToAtomicRanges(target);
    const selection = notesTextSelectionFromEditableRoot(target);
    if (!selection) return;
    this.runtime.setTrackedSelection(selection);
  };

  /** Mention spans in plain-text offsets, empty when the editable text has drifted from the model. */
  private atomicRangesFor(text: string): NotesAtomicTextRange[] {
    const richText = this.editableRichText;
    return richTextPlainText(richText) === text ? notesAtomicTextRanges(richText) : [];
  }

  private snapEditableSelectionToAtomicRanges(root: HTMLElement): void {
    if (this.runtime.compositionActive) return;
    const selection = notesDirectionalSelectionFromEditableRoot(root);
    if (!selection) return;
    const ranges = this.atomicRangesFor(notesPlainTextFromEditableRoot(root));
    const snapped = snapNotesDirectionalSelection(ranges, selection);
    if (snapped.anchor === selection.anchor && snapped.focus === selection.focus) return;
    setNotesEditableDirectionalSelection(root, snapped);
  }

  /** Step the caret, or the selection focus with Shift, over a whole mention next to it. */
  private moveAcrossAtomicRange(event: KeyboardEvent): boolean {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return false;
    if (event.ctrlKey || event.altKey || event.metaKey || !(event.currentTarget instanceof HTMLElement)) return false;
    const root = event.currentTarget;
    const selection = notesDirectionalSelectionFromEditableRoot(root);
    if (!selection || (!event.shiftKey && selection.anchor !== selection.focus)) return false;
    const ranges = this.atomicRangesFor(notesPlainTextFromEditableRoot(root));
    const range = event.key === "ArrowLeft"
      ? notesAtomicRangeEndingAt(ranges, selection.focus)
      : notesAtomicRangeStartingAt(ranges, selection.focus);
    if (!range) return false;
    const focus = event.key === "ArrowLeft" ? range.start : range.end;
    event.preventDefault();
    setNotesEditableDirectionalSelection(root, { anchor: event.shiftKey ? selection.anchor : focus, focus });
    this.syncTextSelection(root);
    return true;
  }

  captureContextMenuSelection = (event: PointerEvent): void => {
    if (event.button !== 2 || !(event.currentTarget instanceof HTMLElement)) return;
    this.#rightClickSelection = null;
    const editor = event.currentTarget;
    const selection = editor.ownerDocument.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount === 0) return;
    if (!selection.anchorNode || !selection.focusNode) return;
    if (!editor.contains(selection.anchorNode) || !editor.contains(selection.focusNode)) return;
    const rects = Array.from(selection.getRangeAt(0).getClientRects()).map((rect) => ({
      left: rect.left,
      right: rect.right,
      top: rect.top,
      bottom: rect.bottom,
    }));
    if (notesTextContextMenuSelectionAtPoint(rects, { x: event.clientX, y: event.clientY })) {
      this.#rightClickSelection = notesTextSelectionFromEditableRoot(editor);
    }
  };

  openContextMenu = (event: MouseEvent): void => {
    if (!(event.currentTarget instanceof HTMLElement)) return;
    event.preventDefault();
    event.stopPropagation();
    const preservedSelection = this.#rightClickSelection;
    this.#rightClickSelection = null;
    this.syncTextSelection(event.currentTarget);
    if (preservedSelection) this.runtime.restoreTrackedSelection(preservedSelection);
    const isKeyboardInvoked = event.clientX === 0 && event.clientY === 0;
    this.contextMenuFocusOnOpen = isKeyboardInvoked;
    if (isKeyboardInvoked) {
      const selection = event.currentTarget.ownerDocument.getSelection();
      const caretRect = selection?.rangeCount && event.currentTarget.contains(selection.anchorNode)
        ? selection.getRangeAt(0).getBoundingClientRect()
        : null;
      const anchor = notesEditableSelectionViewportRect(event.currentTarget)
        ?? (caretRect && (caretRect.width > 0 || caretRect.height > 0)
          ? caretRect
          : event.currentTarget.getBoundingClientRect());
      this.contextMenuPoint = { x: anchor.left, y: anchor.bottom };
    } else {
      this.contextMenuPoint = { x: event.clientX, y: event.clientY };
    }
    this.contextMenuOpen = true;
    this.slashOpen = false;
    this.mentionQuery = null;
  };

  closeContextMenu = (restoreFocus = false): void => {
    this.contextMenuOpen = false;
    if (restoreFocus) {
      const { start, end } = this.textSelection;
      void this.runtime.focusEditorWithSelection(start, end);
    }
  };

  /** Copy model semantics even when the context menu has moved native focus. */
  copySelectedText = async (): Promise<void> => {
    const { start, end } = this.textSelection;
    if (start === end) return;
    await writeNotesClipboard(notesClipboardContent([{ block: this.block, start, end }]));
  };

  cutSelectedText = async (): Promise<void> => {
    const { start, end } = this.textSelection;
    if (start === end) return;
    const blockId = this.block.id;
    const originalText = this.text;
    await this.copySelectedText();
    if (this.block.id !== blockId || this.text !== originalText
      || this.textSelection.start !== start || this.textSelection.end !== end) return;
    await this.#deleteCopiedText(start, end);
  };

  /** Override native DOM copying, whose classes do not carry external semantics. */
  handleCopy = (event: ClipboardEvent): void => {
    const editor = this.runtime.editor;
    const domSelection = editor ? notesTextSelectionFromEditableRoot(editor) : null;
    const selection = domSelection
      ? normalizeNotesSelectionForAtomicRanges(this.atomicRangesFor(this.text), domSelection)
      : null;
    if (!selection || selection.start === selection.end || event.defaultPrevented) return;
    event.preventDefault();
    try {
      if (!event.clipboardData) throw new Error("Notes clipboard is unavailable");
      setNotesClipboardData(event.clipboardData, notesClipboardContent([{
        block: this.block, start: selection.start, end: selection.end,
      }]));
      if (event.type === "cut") {
        void this.#deleteCopiedText(selection.start, selection.end)
          .catch((error: unknown) => console.warn("Notes clipboard cut failed", error));
      }
    } catch (error) {
      console.warn("Notes clipboard copy failed", error);
    }
  };

  async #deleteCopiedText(start: number, end: number): Promise<void> {
    const edit = planNotesControlledTextEdit({
      inputType: "deleteByCut", data: null, text: this.text,
      selectionStart: start, selectionEnd: end, atomicRanges: this.atomicRangesFor(this.text),
    });
    if (!edit) return;
    this.source.onTextInput(this.block.id, edit.text, edit.selection);
    await this.runtime.focusEditorWithSelection(edit.selection.start, edit.selection.end);
  }

  pasteFromClipboard = async (plainOnly = false): Promise<void> => {
    if (!navigator.clipboard) throw new Error("Notes text clipboard is unavailable");
    const { start, end } = this.textSelection;
    const blockId = this.block.id;
    const originalText = this.text;
    const clipboard = plainOnly
      ? { plainText: await navigator.clipboard.readText(), html: "" }
      : await readNotesClipboard();
    if (this.block.id !== blockId || this.text !== originalText
      || this.textSelection.start !== start || this.textSelection.end !== end) return;
    const pastedUrl = !plainOnly && this.canUseLinks && start !== end
      ? notesPastedTextLinkUrl(clipboard.plainText, window.location.href) : null;
    if (pastedUrl) {
      await this.source.onApplyTextLink(blockId, start, end, pastedUrl);
      await this.runtime.focusEditorWithSelection(start, end);
      return;
    }
    const html = plainOnly || this.block.type === "code" ? "" : notesClipboardPasteHtml(clipboard.plainText, clipboard.html);
    if (html.trim() && await Promise.resolve(this.source.onPasteRichHtml(this.block.id, start, end, html))) return;
    const plainText = normalizeNotesClipboardPlainText(clipboard.plainText);
    if (!plainText) return;
    const handled = !plainOnly && await Promise.resolve(this.source.onPastePlainText(
      this.block.id,
      start,
      end,
      plainText,
    ));
    if (handled) return;
    const edit = planNotesControlledTextEdit({
      inputType: "insertText",
      data: plainText,
      text: this.text,
      selectionStart: start,
      selectionEnd: end,
      atomicRanges: this.atomicRangesFor(this.text),
    });
    if (!edit) return;
    this.source.onTextInput(this.block.id, edit.text, edit.selection);
    await this.runtime.focusEditorWithSelection(edit.selection.start, edit.selection.end);
  };

  openLinkEditorFromEditor(target: EventTarget | null): boolean {
    if (!this.canUseLinks || !(target instanceof HTMLElement)) return false;
    this.syncTextSelection(target);
    const selection = notesTextSelectionFromEditableRoot(target);
    if (!selection) return false;
    const range = blockTextLinkRangeForSelection(this.block, selection.start, selection.end);
    if (range.start === range.end && !range.url) return false;
    this.keepLinkPreviewOpen();
    this.linkRange = range;
    this.linkUrlInput = range.url ?? "";
    this.linkTitleInput = this.text.slice(range.start, range.end);
    this.linkMode = "edit";
    this.#linkSourceText = this.text;
    this.#linkAnchorElement = null;
    this.#linkAnchorRange = target.ownerDocument.getSelection()?.getRangeAt(0).cloneRange() ?? null;
    this.linkError = null;
    this.inlineEquationErrorReason = null;
    this.linkEditorOpen = true;
    this.slashOpen = false;
    this.mentionQuery = null;
    return true;
  }

  openLinkEditorFromButton = (): void => {
    if (this.runtime.editor) this.openLinkEditorFromEditor(this.runtime.editor);
  };

  /** Read a link's text range without changing the caret or native selection. */
  private prepareLinkTarget(target: EventTarget | null): boolean {
    const editor = this.runtime.editor;
    if (!(target instanceof Element) || !editor) return false;
    const link = target.closest<HTMLElement>("[data-notes-link-url]");
    const url = link?.dataset.notesLinkUrl;
    if (!link || !url || !editor.contains(link) || !this.canUseLinks) return false;
    const offset = notesEditableOffsetFromDomPoint(editor, link, 0);
    if (offset === null) return false;
    this.keepLinkPreviewOpen();
    this.linkRange = blockTextLinkRangeForSelection(this.block, offset + 1, offset + 1);
    this.linkUrlInput = url;
    this.linkTitleInput = this.text.slice(this.linkRange.start, this.linkRange.end) || link.textContent || "";
    this.#linkSourceText = this.text;
    this.#linkAnchorElement = link;
    this.#linkAnchorRange = null;
    this.linkError = null;
    this.linkMode = "preview";
    return true;
  }

  /** Show floating actions on hover without interrupting dragging, touch, or an open edit form. */
  handleLinkPointerOver = (event: PointerEvent): void => {
    if (event.pointerType === "touch" || event.buttons || this.linkBusy || this.contextMenuOpen || (this.linkEditorOpen && this.linkMode === "edit")) return;
    if (this.linkEditorOpen && event.target instanceof Node && this.#linkAnchorElement?.contains(event.target)) {
      this.keepLinkPreviewOpen();
      return;
    }
    if (this.prepareLinkTarget(event.target)) this.linkEditorOpen = true;
  };

  handleLinkPointerOut = (event: PointerEvent): void => {
    if (!this.#linkAnchorElement?.contains(event.target instanceof Node ? event.target : null)) return;
    if (event.relatedTarget instanceof Node && this.#linkAnchorElement.contains(event.relatedTarget)) return;
    this.scheduleLinkPreviewClose();
  };

  keepLinkPreviewOpen = (): void => {
    if (this.#linkHoverCloseTimer !== null) clearTimeout(this.#linkHoverCloseTimer);
    this.#linkHoverCloseTimer = null;
  };

  /** Allow the pointer to cross the small gap between the link and its portaled preview. */
  scheduleLinkPreviewClose = (): void => {
    this.keepLinkPreviewOpen();
    if (!this.linkEditorOpen || this.linkMode !== "preview" || this.linkBusy || this.linkError) return;
    this.#linkHoverCloseTimer = setTimeout(() => this.cancelLinkEditor(), NOTES_LINK_HOVER_CLOSE_DELAY_MS);
  };

  /** Open links directly while leaving text selection gestures intact. */
  handleLinkClick = (event: MouseEvent): void => {
    const editor = this.runtime.editor;
    if (!(event.target instanceof Element) || !editor || event.shiftKey || event.altKey) return;
    if (!event.target.closest("[data-notes-link-url]")) return;
    event.preventDefault();
    if (!event.ctrlKey && !event.metaKey && !editor.ownerDocument.getSelection()?.isCollapsed) return;
    if (this.linkBusy || !this.prepareLinkTarget(event.target)) return;
    this.linkEditorOpen = false;
    this.contextMenuOpen = false;
    this.slashOpen = false;
    this.mentionQuery = null;
    void this.openLinkDestination();
  };

  editLinkDestination = (): void => { this.keepLinkPreviewOpen(); this.linkMode = "edit"; };

  openLinkDestination = async (): Promise<void> => {
    if (this.linkBusy) return;
    this.linkBusy = true;
    try {
      await openNotesTextLink(this.linkUrlInput, this.source.notes);
      this.cancelLinkEditor();
    } catch (error: unknown) {
      console.warn("Notes link navigation failed", error);
      this.linkError = this.source.translate("notes.linkOpenFailed");
      this.linkEditorOpen = true;
    } finally { this.linkBusy = false; }
  };

  copyLinkDestination = async (): Promise<void> => {
    try {
      await writeTextToClipboard(this.linkUrlInput);
      this.cancelLinkEditor(true);
    } catch (error: unknown) {
      console.warn("Notes link copy failed", error);
      this.linkError = this.source.translate("notes.linkCopyFailed");
    }
  };

  applyLinkFromEditor = async (): Promise<void> => {
    if (this.linkBusy) return;
    const normalizedUrl = normalizeNotesTextLinkUrl(this.linkUrlInput, window.location.href);
    if (!normalizedUrl) {
      this.linkError = this.source.translate("notes.linkUrlInvalid");
      return;
    }
    this.linkError = null;
    const { start, end } = this.linkRange;
    if (this.text !== this.#linkSourceText) { this.linkError = this.source.translate("notes.linkSelectionChanged"); return; }
    if (!this.linkTitleInput.trim()) { this.linkError = this.source.translate("notes.linkTitleRequired"); return; }
    this.linkBusy = true;
    try {
      if (this.linkTitleInput === this.text.slice(start, end)) {
        await this.source.onApplyTextLink(this.block.id, start, end, normalizedUrl);
      } else {
        await this.source.onReplaceRichText(this.block.id, editRichTextLink(this.editableRichText, start, end, this.linkTitleInput, normalizedUrl));
      }
      this.cancelLinkEditor();
      await this.runtime.focusEditorWithSelection(start, start + this.linkTitleInput.length);
    } catch (error: unknown) {
      console.warn("Notes link edit failed", error);
      this.linkError = this.source.translate("notes.linkSaveFailed");
    } finally { this.linkBusy = false; }
  };

  removeLinkFromEditor = async (): Promise<void> => {
    if (this.linkBusy) return;
    this.linkError = null;
    const { start, end } = this.linkRange;
    if (this.text !== this.#linkSourceText) { this.linkError = this.source.translate("notes.linkSelectionChanged"); return; }
    this.linkBusy = true;
    try {
      await this.source.onApplyTextLink(this.block.id, start, end, null);
      this.cancelLinkEditor();
      await this.runtime.focusEditorWithSelection(start, end);
    } catch (error: unknown) {
      console.warn("Notes link removal failed", error);
      this.linkError = this.source.translate("notes.linkSaveFailed");
    } finally { this.linkBusy = false; }
  };

  commitPlainTextValue(value: string, selection: NotesTextSelection | null): void {
    this.inlineEquationErrorReason = null;
    if (selection) this.runtime.setTrackedSelection(selection);
    if (this.canUseInlineFormatting && selection) {
      const nextRichText = replacePlainTextPreservingRichText(this.editableRichText, value);
      const shortcutPlan = planNotesMarkdownInlineShortcutConversion(
        nextRichText,
        selection.start,
        selection.end,
      );
      if (shortcutPlan) {
        this.closeCompositionSensitiveMenus();
        void Promise.resolve(this.source.onReplaceRichText(this.block.id, shortcutPlan.richText))
          .then(() => this.runtime.focusEditorWithSelection(shortcutPlan.cursor, shortcutPlan.cursor))
          .catch((error: unknown) => console.warn("notes inline markdown shortcut failed", error));
        return;
      }
    }
    const menuState = planNotesTextInputMenuState(
      value,
      selection,
      this.slashOpen,
      this.canUseMentions,
      this.text,
    );
    this.slashOpen = menuState.slashOpen;
    this.mentionQuery = menuState.mentionQuery;
    this.mentionActiveIndex = 0;
    this.source.onTextInput(this.block.id, value, selection);
  }

  commitRichTextInput(target: HTMLElement): void {
    this.syncTextSelection(target);
    this.commitPlainTextValue(
      notesPlainTextFromEditableRoot(target),
      notesTextSelectionFromEditableRoot(target),
    );
  }

  handleInput = (event: Event): void => {
    if (!(event.currentTarget instanceof HTMLElement)) return;
    const eventIsComposing = event instanceof InputEvent && event.isComposing;
    if (shouldDeferNotesCompositionInput({
      active: this.runtime.compositionActive,
      eventIsComposing,
    })) {
      this.closeCompositionSensitiveMenus();
      return;
    }
    this.commitRichTextInput(event.currentTarget);
  };

  handleCompositionEnd = (event: CompositionEvent): void => {
    this.runtime.compositionActive = false;
    const shouldResumeSlash = this.#shouldResumeSlashAfterComposition;
    this.#shouldResumeSlashAfterComposition = false;
    if (event.currentTarget instanceof HTMLElement && !event.currentTarget.closest("[data-notes-document-selection]")) {
      this.slashOpen = shouldResumeSlash;
      this.commitRichTextInput(event.currentTarget);
    }
  };

  handlePaste = async (event: ClipboardEvent): Promise<void> => {
    if (!(event.currentTarget instanceof HTMLElement)) return;
    this.cancelLinkEditor();
    const plainOnly = this.#shouldPasteAsPlainText;
    this.#shouldPasteAsPlainText = false;
    const clipboardText = event.clipboardData?.getData("text/plain") ?? "";
    const selection = notesTextSelectionFromEditableRoot(event.currentTarget);
    if (!selection) return;
    if (plainOnly) {
      event.preventDefault();
      const plainText = normalizeNotesClipboardPlainText(clipboardText);
      if (!plainText) return;
      const nextText = `${this.text.slice(0, selection.start)}${plainText}${this.text.slice(selection.end)}`;
      const cursor = selection.start + plainText.length;
      this.source.onTextInput(this.block.id, nextText, { start: cursor, end: cursor });
      await this.runtime.focusEditorWithSelection(cursor, cursor);
      return;
    }
    const pastedUrl = this.canUseLinks && selection.start !== selection.end
      ? notesPastedTextLinkUrl(clipboardText, window.location.href) : null;
    if (pastedUrl) {
      event.preventDefault();
      await this.source.onApplyTextLink(this.block.id, selection.start, selection.end, pastedUrl);
      await this.runtime.focusEditorWithSelection(selection.start, selection.end);
      return;
    }
    const html = this.block.type === "code" ? "" : notesClipboardPasteHtml(clipboardText, event.clipboardData?.getData("text/html") ?? "");
    if (html.trim() && this.block.type !== "code") {
      event.preventDefault();
      const handled = await Promise.resolve(
        this.source.onPasteRichHtml(this.block.id, selection.start, selection.end, html),
      );
      if (handled) return;
    }
    const plainText = normalizeNotesClipboardPlainText(
      clipboardText,
    );
    if (!plainText) {
      event.preventDefault();
      return;
    }
    if (!shouldHandleNotesPlainTextPaste({
      currentBlockType: this.block.type,
      currentText: this.text,
      selectionStart: selection.start,
      plainText,
    })) {
      event.preventDefault();
      const nextText = `${this.text.slice(0, selection.start)}${plainText}${this.text.slice(selection.end)}`;
      const cursor = selection.start + plainText.length;
      this.source.onTextInput(this.block.id, nextText, { start: cursor, end: cursor });
      await this.runtime.focusEditorWithSelection(cursor, cursor);
      return;
    }
    event.preventDefault();
    const handled = await Promise.resolve(this.source.onPastePlainText(
      this.block.id,
      selection.start,
      selection.end,
      plainText,
    ));
    if (handled) return;
    const nextText = `${this.text.slice(0, selection.start)}${plainText}${this.text.slice(selection.end)}`;
    const cursor = selection.start + plainText.length;
    this.source.onTextInput(this.block.id, nextText, { start: cursor, end: cursor });
    await this.runtime.focusEditorWithSelection(cursor, cursor);
  };

  handleEditorBlur = (): void => {
    this.#shouldResumeSlashAfterComposition = false;
    this.runtime.compositionActive = false;
    this.slashOpen = false;
    window.setTimeout(() => {
      this.mentionQuery = null;
      this.mentionActiveIndex = 0;
    }, 120);
  };

  handleEditorFocus = (): void => {
    this.mentionInvite = null;
    if (this.source.focusBlockId() !== this.block.id) this.source.onFocusBlock(this.block.id, true);
    this.runtime.requestControl("text-context-menu");
    if (this.canUseMentions) this.runtime.requestControl("slash-menu");
  };

  /** Insert a target, reveal a truncated section in place, or open the invitation panel. */
  selectMentionRow = (row: NotesMentionRow): void => {
    if (row.kind === "target") {
      void this.selectMention(row.target);
      return;
    }
    const mention = this.mentionQuery;
    if (!mention) return;
    if (row.kind === "more") {
      const groups = this.mentionExpanded?.start === mention.start ? this.mentionExpanded.groups : new Set<NotesMentionGroupKind>();
      this.mentionExpanded = { start: mention.start, groups: new Set([...groups, row.group]) };
      return;
    }
    this.mentionInvite = mention;
    this.mentionQuery = null;
    this.mentionActiveIndex = 0;
  };

  closeMentionMenu = (): void => {
    this.mentionQuery = null;
    this.mentionActiveIndex = 0;
  };

  /** Close the invitation panel, optionally returning to the mention menu at the typed at sign. */
  closeMentionInvite = async (returnToMenu: boolean): Promise<void> => {
    const mention = this.mentionInvite;
    this.mentionInvite = null;
    if (!returnToMenu || !mention) return;
    await this.runtime.focusEditorWithSelection(mention.end, mention.end);
    if (this.text.slice(mention.start, mention.end) === `@${mention.query}`) {
      this.mentionQuery = mention;
      this.mentionActiveIndex = 0;
    }
  };

  selectMention = async (target: NotesMentionTarget): Promise<void> => {
    if (!this.mentionQuery) return;
    const range = this.mentionQuery;
    this.mentionQuery = null;
    this.mentionActiveIndex = 0;
    if (target.kind === "page") {
      await Promise.resolve(this.source.onInsertPageMention(this.block.id, range.start, range.end, target));
    } else if (target.kind === "date") {
      await Promise.resolve(this.source.onInsertDateMention(this.block.id, range.start, range.end, target));
    } else {
      await Promise.resolve(this.source.onInsertObjectMention(this.block.id, range.start, range.end, target));
    }
    const inserted = notesAtomicRangeStartingAt(notesAtomicTextRanges(this.editableRichText), range.start);
    const cursor = inserted?.end ?? range.start + target.title.length;
    await this.runtime.focusEditorWithSelection(cursor, cursor);
  };

  clearSlashText(): void {
    if (this.text.startsWith("/")) {
      this.source.onTextInput(this.block.id, "", { start: 0, end: 0 });
    }
  }

  selectSlashCommand = (command: NotesSlashCommand): void => {
    const shouldClearTypedSlashText = this.slashInputSession.open;
    this.closeSlashMenu();
    recordRecentNotesSlashCommandKey(notesSlashCommandKey(command));
    switch (command.kind) {
      case "block":
        this.source.onConvert(this.block.id, command.blockType, shouldClearTypedSlashText);
        return;
      case "toggle_heading":
        this.source.onConvertToToggleHeading(this.block.id, command.headingType, shouldClearTypedSlashText);
        return;
      case "action":
        if (command.action !== "delete" && shouldClearTypedSlashText) this.clearSlashText();
        this.runSlashAction(command.action);
        return;
      case "color":
        if (!this.blockSupportsColor) return;
        if (shouldClearTypedSlashText) this.clearSlashText();
        this.source.onColorChange(this.block.id, command.color);
    }
  };

  runSlashAction(action: NotesSlashAction): void {
    switch (action) {
      case "copy_link":
        void Promise.resolve(this.source.onCopyLink(this.block.id))
          .catch((error: unknown) => console.warn("copy notes block link failed", error));
        return;
      case "duplicate": this.source.onDuplicate(this.block.id); return;
      case "move_up": this.source.onMoveUp(this.block.id); return;
      case "move_down": this.source.onMoveDown(this.block.id); return;
      case "delete": this.source.onDelete(this.block.id); return;
    }
  }
}

export function createNotesTextEditorController(
  source: NotesTextEditorControllerSource,
): NotesTextEditorController {
  return new NotesTextEditorController(source);
}
