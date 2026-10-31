<script lang="ts">
  import { pickNotesFileAsset } from "$lib/api/notes/file-assets";
  import { notesMentionData } from "$lib/components/notes/blocks/mention-data-controller.svelte";
  import { buildNotesMentionTargets } from "$lib/components/notes/blocks/mention-targets";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import NotesRichTextInline from "$lib/components/notes/rich-text/NotesRichTextInline.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { notesCommentAttachmentFromAsset } from "$lib/notes/collaboration/comment-attachments";
  import {
    buildNotesCommentMentionGroups,
    flattenNotesCommentMentionGroups,
    NOTES_COMMENT_MENTION_LIMITS,
    type NotesCommentMentionGroup,
  } from "$lib/notes/collaboration/comment-mentions";
  import { isNotesCommentTooLargeMessage, notesCommentIsEmpty } from "$lib/notes/collaboration/comments";
  import {
    notesPlainTextFromEditableRoot,
    notesTextSelectionFromEditableRoot,
    restoreNotesEditableSelection,
    type NotesTextSelection,
  } from "$lib/notes/editor/selection";
  import { buildNotesPageLink } from "$lib/notes/links/block-link";
  import { planNotesControlledTextEdit } from "$lib/notes/rich-text/controlled-text-input";
  import {
    buildDateMentionTargets,
    detectPageMentionQuery,
    insertDateMentionRichText,
    insertObjectMentionRichText,
    insertPageMentionRichText,
    replacePlainTextPreservingRichText,
    richTextPlainText,
    type NotesMentionQuery,
    type NotesMentionTarget,
  } from "$lib/notes/rich-text/core";
  import type { NotesRichText } from "$lib/notes/types";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { Temporal } from "@js-temporal/polyfill";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import AtSign from "@lucide/svelte/icons/at-sign";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import { tick, untrack } from "svelte";
  import NotesCommentAttachment from "./NotesCommentAttachment.svelte";
  import NotesCommentAvatar from "./NotesCommentAvatar.svelte";
  import NotesCommentMentionMenu from "./NotesCommentMentionMenu.svelte";

  type CommentAttachment = Record<string, unknown>;

  /**
   * Rich comment input shared by new comments, replies, and edits: mentions grouped by Date, People, and Link to page,
   * file attachments, Enter to send, and Shift+Enter for a new line. The attach, mention, and send tools appear while
   * the composer has focus, a draft, or an edit in progress.
   */
  let {
    label,
    placeholder,
    initialRichText = [],
    initialAttachments = [],
    mode = "create",
    showAvatar = true,
    menuPlacement = "below",
    disabled = false,
    onSubmit,
    onCancel,
    onLeaveEmpty,
  }: {
    /** Accessible name of the text field. */
    label: string;
    placeholder: string;
    initialRichText?: readonly NotesRichText[];
    initialAttachments?: readonly CommentAttachment[];
    /** An edit keeps its draft after saving and offers Cancel; a new comment clears once posted. */
    mode?: "create" | "edit";
    showAvatar?: boolean;
    menuPlacement?: "below" | "above";
    disabled?: boolean;
    /** Post the draft. Resolve false when nothing was posted so the draft stays. */
    onSubmit: (richText: NotesRichText[], attachments: CommentAttachment[]) => Promise<boolean>;
    onCancel?: () => void;
    /** Focus left the composer while it held nothing to post. */
    onLeaveEmpty?: () => void;
  } = $props();

  const notes = getNotesEditor();
  const localization = getLocalization();
  const preferences = getPreferences();
  const { t } = localization;
  const uid = $props.id();
  const menuId = `${uid}-mentions`;
  const filePickerAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "storage.native-file-picker");

  let root = $state<HTMLDivElement | null>(null);
  let editor = $state<HTMLDivElement | null>(null);
  let richText = $state.raw<NotesRichText[]>(untrack(() => [...initialRichText]));
  let attachments = $state.raw<CommentAttachment[]>(untrack(() => [...initialAttachments]));
  let composing = $state(false);
  let mentionQuery = $state<NotesMentionQuery | null>(null);
  let mentionActiveIndex = $state(0);
  let submitting = $state(false);
  let attaching = $state(false);
  let errorMessage = $state<string | null>(null);
  let lastSelection: NotesTextSelection | null = null;
  /** The at sign whose menu was dismissed; typing after it does not reopen the menu. */
  let dismissedMentionStart: number | null = null;

  const plainText = $derived(richTextPlainText(richText));
  const empty = $derived(notesCommentIsEmpty(richText, attachments));
  const mentionGroups = $derived<NotesCommentMentionGroup[]>(mentionQuery ? mentionGroupsFor(mentionQuery.query) : []);
  const mentionTargets = $derived(flattenNotesCommentMentionGroups(mentionGroups));
  const activeMentionIndex = $derived(mentionActiveIndex < mentionTargets.length ? mentionActiveIndex : 0);
  const mentionOpen = $derived(mentionQuery !== null);
  /** Keeps the tools visible without focus, for example while the file picker is open or a draft waits. */
  const active = $derived(mode === "edit" || !empty || attaching || submitting || mentionOpen || errorMessage !== null);

  $effect(() => {
    if (!mentionOpen) return;
    return notesMentionData.acquire();
  });

  /** Focus the text field with the caret at the end of the draft. */
  export function focus(): void {
    void placeCaret({ start: plainText.length, end: plainText.length }, true);
  }

  function mentionOptionId(index: number): string {
    return `${uid}-mention-${index}`;
  }

  function mentionGroupsFor(query: string): NotesCommentMentionGroup[] {
    const dateTargets = buildDateMentionTargets(query, {
      today: Temporal.Now.plainDateISO(),
      locale: localization.locale,
      labels: {
        today: t("notes.dateMentionToday"),
        tomorrow: t("notes.dateMentionTomorrow"),
        yesterday: t("notes.dateMentionYesterday"),
        nextWeek: t("notes.dateMentionNextWeek"),
        date: t("notes.dateMention"),
        reminder: t("notes.reminderMention"),
        remindTitle: (dateLabel: string) => t("notes.remindOnDate", dateLabel),
        atTime: (dateLabel: string, time: string) => t("notes.dateMentionAtTime", dateLabel, time),
      },
      limit: NOTES_COMMENT_MENTION_LIMITS.queryDates,
      timeFormat: preferences.calendarTimeFormat,
    }).filter((target) => !target.reminder);
    const namedTargets = buildNotesMentionTargets({
      pages: notes.allPages,
      localUser: notes.localUser,
      dataSources: notesMentionData.dataSources,
      dataSourceRowPages: notesMentionData.rowPages,
      projects: [],
      tasks: [],
      calendarEvents: [],
      translate: t,
    });
    return buildNotesCommentMentionGroups({ query, dateTargets, namedTargets });
  }

  function readEditor(): { text: string; selection: NotesTextSelection } | null {
    if (!editor) return null;
    const text = notesPlainTextFromEditableRoot(editor);
    const selection = notesTextSelectionFromEditableRoot(editor) ?? { start: text.length, end: text.length };
    return { text, selection };
  }

  function updateMentionQuery(text: string, selection: NotesTextSelection): void {
    const query = detectPageMentionQuery(text, selection.start, selection.end);
    if (!query) {
      dismissedMentionStart = null;
      mentionQuery = null;
      return;
    }
    if (query.start === dismissedMentionStart) {
      mentionQuery = null;
      return;
    }
    if (mentionQuery?.start !== query.start) mentionActiveIndex = 0;
    mentionQuery = query;
  }

  function commitText(text: string, selection: NotesTextSelection): void {
    richText = replacePlainTextPreservingRichText(richText, text);
    lastSelection = selection;
    errorMessage = null;
    updateMentionQuery(text, selection);
    void placeCaret(selection, false);
  }

  async function placeCaret(selection: NotesTextSelection, takeFocus: boolean): Promise<void> {
    await tick();
    if (!editor) return;
    if (document.activeElement !== editor) {
      if (!takeFocus) return;
      editor.focus({ preventScroll: true });
    }
    restoreNotesEditableSelection(editor, selection);
    lastSelection = selection;
  }

  function syncSelection(): void {
    const state = readEditor();
    if (!state) return;
    lastSelection = state.selection;
    updateMentionQuery(state.text, state.selection);
  }

  function handleBeforeInput(event: InputEvent): void {
    if (event.isComposing || composing) return;
    if (event.inputType.startsWith("history") || event.inputType.startsWith("format")) {
      event.preventDefault();
      return;
    }
    const state = readEditor();
    if (!state) return;
    const edit = planNotesControlledTextEdit({
      inputType: event.inputType,
      data: event.data,
      text: state.text,
      selectionStart: state.selection.start,
      selectionEnd: state.selection.end,
    });
    if (!edit) return;
    event.preventDefault();
    commitText(edit.text, edit.selection);
  }

  function handleInput(event: Event): void {
    if (composing || (event instanceof InputEvent && event.isComposing)) {
      mentionQuery = null;
      return;
    }
    const state = readEditor();
    if (state) commitText(state.text, state.selection);
  }

  function handleCompositionEnd(): void {
    composing = false;
    const state = readEditor();
    if (state) commitText(state.text, state.selection);
  }

  function handlePaste(event: ClipboardEvent): void {
    event.preventDefault();
    const pasted = (event.clipboardData?.getData("text/plain") ?? "").replace(/\r\n?/gu, "\n");
    const state = readEditor();
    if (!pasted || !state) return;
    const { start, end } = state.selection;
    const cursor = start + pasted.length;
    commitText(`${state.text.slice(0, start)}${pasted}${state.text.slice(end)}`, { start: cursor, end: cursor });
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (composing || event.isComposing || event.keyCode === 229) return;
    if (mentionOpen && routeMentionKey(event)) return;
    if (event.key === "Enter" && !event.shiftKey && !event.altKey) {
      event.preventDefault();
      void submit();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (onCancel && (mode === "edit" || empty)) onCancel();
      else editor?.blur();
    }
  }

  function routeMentionKey(event: KeyboardEvent): boolean {
    const count = mentionTargets.length;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (count > 0) mentionActiveIndex = (activeMentionIndex + (event.key === "ArrowDown" ? 1 : count - 1)) % count;
      return true;
    }
    if ((event.key === "Enter" || event.key === "Tab") && count > 0) {
      event.preventDefault();
      selectMention(mentionTargets[activeMentionIndex]);
      return true;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismissedMentionStart = mentionQuery?.start ?? null;
      mentionQuery = null;
      return true;
    }
    return false;
  }

  function selectMention(target: NotesMentionTarget): void {
    const query = mentionQuery;
    if (!query) return;
    if (target.kind === "page") {
      richText = insertPageMentionRichText(
        richText,
        query.start,
        query.end,
        target.id,
        target.title,
        buildNotesPageLink(window.location.href, { pageId: target.id }),
      );
    } else if (target.kind === "date") {
      richText = insertDateMentionRichText(richText, query.start, query.end, target.date, target.title);
    } else {
      richText = insertObjectMentionRichText(richText, query.start, query.end, target);
    }
    mentionQuery = null;
    mentionActiveIndex = 0;
    const cursor = query.start + target.title.length;
    void placeCaret({ start: cursor, end: cursor }, true);
  }

  /** Type an at sign at the caret, with a space before it when it would otherwise join a word. */
  function startMention(): void {
    const text = plainText;
    const selection = lastSelection ?? { start: text.length, end: text.length };
    const before = text.slice(0, selection.start);
    const trigger = before && !/[\s([{:;,]$/u.test(before) ? " @" : "@";
    const cursor = selection.start + trigger.length;
    const next = `${before}${trigger}${text.slice(selection.end)}`;
    richText = replacePlainTextPreservingRichText(richText, next);
    dismissedMentionStart = null;
    mentionActiveIndex = 0;
    mentionQuery = { start: cursor - 1, end: cursor, query: "" };
    void placeCaret({ start: cursor, end: cursor }, true);
  }

  async function attachFile(): Promise<void> {
    if (attaching || disabled || !filePickerAvailable) return;
    attaching = true;
    errorMessage = null;
    try {
      const asset = await pickNotesFileAsset("file");
      if (asset) attachments = [...attachments, notesCommentAttachmentFromAsset(asset)];
    } catch (error) {
      errorMessage = t("notes.commentAttachFailed", error instanceof Error ? error.message : String(error));
    } finally {
      attaching = false;
      const end = plainText.length;
      void placeCaret(lastSelection ?? { start: end, end }, true);
    }
  }

  function removeAttachment(index: number): void {
    attachments = attachments.filter((_, itemIndex) => itemIndex !== index);
    focus();
  }

  async function submit(): Promise<void> {
    if (submitting || disabled || empty) return;
    submitting = true;
    errorMessage = null;
    mentionQuery = null;
    try {
      const posted = await onSubmit(richText, attachments);
      if (posted && mode === "create") {
        richText = [];
        attachments = [];
        lastSelection = null;
        void placeCaret({ start: 0, end: 0 }, false);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      errorMessage = isNotesCommentTooLargeMessage(message)
        ? t("notes.commentTooLarge")
        : t("notes.commentSendFailed", message);
    } finally {
      submitting = false;
    }
  }

  function handleEditorBlur(): void {
    mentionQuery = null;
  }

  function handleFocusOut(event: FocusEvent): void {
    const next = event.relatedTarget;
    if (next instanceof Node && root?.contains(next)) return;
    if (attaching || submitting || !empty) return;
    onLeaveEmpty?.();
  }
</script>

<div bind:this={root} class="comment-composer flex min-w-0 items-start gap-2" data-active={active || undefined} onfocusout={handleFocusOut}>
  {#if showAvatar}
    <span class="mt-1.5 flex shrink-0"><NotesCommentAvatar own /></span>
  {/if}
  <div class="relative min-w-0 flex-1">
    <div class="flex min-w-0 flex-wrap items-end gap-x-1 gap-y-1.5">
      <div class="relative min-w-40 flex-1 py-1">
        {#if !plainText && !composing}
          <span class="pointer-events-none absolute inset-x-0 top-1 truncate text-muted-foreground" aria-hidden="true">{placeholder}</span>
        {/if}
        <div
          bind:this={editor}
          class="notes-rich-text-editor comment-editor block max-h-60 min-h-6 cursor-text overflow-y-auto whitespace-pre-wrap wrap-break-word outline-none"
          role="textbox"
          aria-multiline="true"
          aria-label={label}
          aria-placeholder={placeholder}
          aria-controls={mentionOpen ? menuId : undefined}
          aria-activedescendant={mentionOpen && mentionTargets.length > 0 ? mentionOptionId(activeMentionIndex) : undefined}
          aria-disabled={disabled || undefined}
          contenteditable={disabled ? "false" : "true"}
          spellcheck="true"
          tabindex="0"
          onbeforeinput={handleBeforeInput}
          oninput={handleInput}
          onkeydown={handleKeydown}
          onkeyup={syncSelection}
          onpointerup={syncSelection}
          oncompositionstart={() => { composing = true; mentionQuery = null; }}
          oncompositionend={handleCompositionEnd}
          onpaste={handlePaste}
          ondrop={(event) => event.preventDefault()}
          onblur={handleEditorBlur}
        ><NotesRichTextInline {richText} /></div>
      </div>
      <div class="composer-tools ml-auto flex shrink-0 items-center gap-0.5">
        {#if mode === "edit" && onCancel}
          <button type="button" class="composer-text-button" onclick={onCancel}>{t("common.cancel")}</button>
        {/if}
        {#if filePickerAvailable}
          <button
            type="button"
            class="composer-tool"
            aria-label={t("notes.commentAttachFile")}
            title={t("notes.commentAttachFile")}
            disabled={disabled || attaching}
            aria-busy={attaching || undefined}
            onpointerdown={(event) => event.preventDefault()}
            onclick={() => void attachFile()}
          ><Paperclip class="size-4" aria-hidden="true" /></button>
        {/if}
        <button
          type="button"
          class="composer-tool"
          aria-label={t("notes.commentMention")}
          title={t("notes.commentMention")}
          disabled={disabled}
          onpointerdown={(event) => event.preventDefault()}
          onclick={startMention}
        ><AtSign class="size-4" aria-hidden="true" /></button>
        <button
          type="button"
          class="composer-send"
          data-ready={!empty || undefined}
          aria-label={mode === "edit" ? t("notes.commentSave") : t("notes.commentSend")}
          title={mode === "edit" ? t("notes.commentSave") : t("notes.commentSend")}
          disabled={disabled || submitting || empty}
          aria-busy={submitting || undefined}
          onpointerdown={(event) => event.preventDefault()}
          onclick={() => void submit()}
        ><ArrowUp class="size-3.5" strokeWidth={2.5} aria-hidden="true" /></button>
      </div>
    </div>
    {#if attachments.length > 0}
      <div class="mt-2 flex flex-wrap gap-2">
        {#each attachments as attachment, index (index)}
          <NotesCommentAttachment {attachment} variant="compact" onRemove={() => removeAttachment(index)} />
        {/each}
      </div>
    {/if}
    {#if errorMessage}
      <Notice class="mt-1 text-panel-detail" message={errorMessage} />
    {/if}
    {#if mentionOpen}
      <NotesCommentMentionMenu
        {menuId}
        optionId={mentionOptionId}
        groups={mentionGroups}
        activeIndex={activeMentionIndex}
        localUserId={notes.localUser?.id ?? null}
        placement={menuPlacement}
        onSelect={selectMention}
        onHighlight={(index) => { mentionActiveIndex = index; }}
      />
    {/if}
  </div>
</div>

<style>
  .comment-composer:not(:focus-within, [data-active]) .composer-tools {
    display: none;
  }
  .composer-tool,
  .composer-send {
    display: grid;
    flex: 0 0 auto;
    place-items: center;
  }
  .composer-tool {
    width: 1.75rem;
    height: 1.75rem;
    border-radius: var(--floating-item-radius);
    color: var(--muted-foreground);
  }
  .composer-tool:hover:not(:disabled) {
    background-color: var(--accent);
    color: var(--foreground);
  }
  .composer-tool:disabled {
    opacity: 0.5;
  }
  .composer-send {
    width: 1.5rem;
    height: 1.5rem;
    margin-left: 0.125rem;
    border-radius: 999px;
    background-color: var(--muted);
    color: var(--muted-foreground);
    transition: background-color 120ms ease, color 120ms ease;
  }
  .composer-send[data-ready] {
    background-color: var(--primary);
    color: var(--primary-foreground);
  }
  .composer-send:disabled:not([data-ready]) {
    cursor: default;
  }
  .composer-text-button {
    min-height: 1.75rem;
    padding: 0 0.5rem;
    border-radius: var(--floating-item-radius);
    color: var(--muted-foreground);
    font-size: var(--panel-detail-font-size);
  }
  .composer-text-button:hover {
    background-color: var(--accent);
    color: var(--foreground);
  }
  @media (pointer: coarse) {
    .composer-tool,
    .composer-send,
    .composer-text-button {
      min-width: 2.75rem;
      min-height: 2.75rem;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .composer-send {
      transition: none;
    }
  }
</style>
