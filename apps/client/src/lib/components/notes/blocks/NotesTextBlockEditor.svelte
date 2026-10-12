<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import type { NotesHeadingBlockType } from "$lib/notes/blocks/factory";
  import type { NotesBlockInsertRequest } from "$lib/notes/blocks/insertion";
  import {
    notesMentionMenuDomId,
    notesRichTextEditorActiveDescendant,
    notesRichTextEditorControls,
    notesRichTextEditorDomId,
    notesRichTextEditorStatusDomId,
    notesSlashMenuDomId,
  } from "$lib/notes/editor/accessibility";
  import { notesRichTextEditorClass } from "$lib/notes/blocks/editor-ui";
  import type { NotesKeyboardAction } from "$lib/notes/blocks/keyboard";
  import { notesMentionChipSelectionAction, type NotesTextSelection } from "$lib/notes/editor/selection";
  import {
    type NotesDateMentionTarget,
    type NotesObjectMentionTarget,
    type NotesPageMentionTarget,
    type NotesRichTextAnnotationPatch,
  } from "$lib/notes/rich-text/core";
  import type { NotesResolvedCommentAnchor } from "$lib/notes/collaboration/comments";
  import type { NotesResolvedSuggestionAnchor } from "$lib/notes/collaboration/suggestions";
  import type {
    NotesBlock,
    NotesBlockType,
    NotesButtonInsertPosition,
    NotesColor,
    NotesIcon,
    NotesRichText,
  } from "$lib/notes/types";
  import type { NotesTemplateBlockStatus } from "$lib/notes/block-types/template";
  import type { NotesButtonBlockStatus } from "$lib/notes/block-types/button";
  import NotesRichTextInline from "$lib/components/notes/rich-text/NotesRichTextInline.svelte";
  import { createNotesTextEditorController } from "./text-editor-runtime.svelte";
  import type { NotesMentionCatalog } from "./mention-data-controller.svelte";
  import { getNotesEditor } from "$lib/components/notes/editor-context";

  let {
    block,
    previousBlockType,
    isOnlyBlock,
    indentationDepth = 0,
    calloutBackgroundTargetId = null,
    calloutBackgroundColor = null,
    focusBlockId,
    focusRequestId,
    focusSelection,
    mentionTargets,
    commentAnchors,
    suggestionAnchors,
    templateStatus,
    buttonStatus,
    onTextInput,
    onReplaceRichText,
    onInsertPageMention,
    onInsertDateMention,
    onInsertObjectMention,
    onApplyTextLink,
    onInsertInlineEquation,
    onPastePlainText,
    onPasteRichHtml,
    onApplyTextAnnotations,
    onCreateInlineComment,
    onCreateInlineSuggestion,
    onKeyboardAction,
    onUndo,
    onRedo,
    onAddBelow,
    onConvert,
    onConvertToToggleHeading,
    onColorChange,
    onCopyLink,
    onDuplicate,
    onUseTemplate,
    onAddTemplateChild,
    onUseButton,
    onAddButtonChild,
    onButtonIconChange,
    onButtonInsertPositionChange,
    onMoveUp,
    onMoveDown,
    onDelete,
    onToggleOpen,
    onCodeLanguageChange,
    onFocusBlock,
  }: {
    block: NotesBlock;
    previousBlockType: NotesBlockType | null;
    isOnlyBlock: boolean;
    indentationDepth?: number;
    focusBlockId: string | null;
    focusRequestId: number;
    focusSelection: NotesTextSelection | null;
    mentionTargets: NotesMentionCatalog;
    commentAnchors: readonly NotesResolvedCommentAnchor[];
    suggestionAnchors: readonly NotesResolvedSuggestionAnchor[];
    templateStatus: NotesTemplateBlockStatus;
    buttonStatus: NotesButtonBlockStatus;
    onTextInput: (
      blockId: string,
      text: string,
      selection: NotesTextSelection | null,
    ) => void;
    onReplaceRichText: (
      blockId: string,
      richText: readonly NotesRichText[],
    ) => Promise<void> | void;
    onInsertPageMention: (
      blockId: string,
      start: number,
      end: number,
      target: NotesPageMentionTarget,
    ) => Promise<void> | void;
    onInsertDateMention: (
      blockId: string,
      start: number,
      end: number,
      target: NotesDateMentionTarget,
    ) => Promise<void> | void;
    onInsertObjectMention: (
      blockId: string,
      start: number,
      end: number,
      target: NotesObjectMentionTarget,
    ) => Promise<void> | void;
    onApplyTextLink: (
      blockId: string,
      start: number,
      end: number,
      url: string | null,
    ) => Promise<void> | void;
    onInsertInlineEquation: (
      blockId: string,
      start: number,
      end: number,
      expression: string,
    ) => Promise<void> | void;
    onPastePlainText: (
      blockId: string,
      start: number,
      end: number,
      plainText: string,
    ) => Promise<boolean> | boolean;
    onPasteRichHtml: (
      blockId: string,
      start: number,
      end: number,
      html: string,
    ) => Promise<boolean> | boolean;
    onApplyTextAnnotations: (
      blockId: string,
      start: number,
      end: number,
      patch: NotesRichTextAnnotationPatch,
    ) => Promise<void> | void;
    onCreateInlineComment: (
      blockId: string,
      start: number,
      end: number,
    ) => Promise<void> | void;
    onCreateInlineSuggestion: (
      blockId: string,
      start: number,
      end: number,
    ) => Promise<void> | void;
    onKeyboardAction: (blockId: string, action: NotesKeyboardAction) => void;
    onUndo: () => Promise<void> | void;
    onRedo: () => Promise<void> | void;
    onAddBelow: (blockId: string, request?: NotesBlockInsertRequest) => void;
    onConvert: (blockId: string, type: NotesBlockType, clearText?: boolean) => void;
    onConvertToToggleHeading: (
      blockId: string,
      type: NotesHeadingBlockType,
      clearText?: boolean,
    ) => void;
    onColorChange: (blockId: string, color: NotesColor) => void;
    calloutBackgroundTargetId?: string | null;
    calloutBackgroundColor?: NotesColor | null;
    onCopyLink: (blockId: string) => Promise<void> | void;
    onDuplicate: (blockId: string) => void;
    onUseTemplate: (blockId: string) => void;
    onAddTemplateChild: (blockId: string) => void;
    onUseButton: (blockId: string) => void;
    onAddButtonChild: (blockId: string) => void;
    onButtonIconChange: (blockId: string, icon: NotesIcon | null) => void;
    onButtonInsertPositionChange: (
      blockId: string,
      position: NotesButtonInsertPosition,
    ) => void;
    onMoveUp: (blockId: string) => void;
    onMoveDown: (blockId: string) => void;
    onDelete: (blockId: string) => void;
    onToggleOpen: (blockId: string, open: boolean) => void;
    onCodeLanguageChange: (blockId: string, language: string) => void;
    onFocusBlock: (blockId: string, preventScroll?: boolean) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const notesEditor = getNotesEditor();
  const controller = createNotesTextEditorController({
    block: () => block,
    previousBlockType: () => previousBlockType,
    isOnlyBlock: () => isOnlyBlock,
    indentationDepth: () => indentationDepth,
    focusBlockId: () => focusBlockId,
    focusRequestId: () => focusRequestId,
    focusSelection: () => focusSelection,
    mentionTargets: () => mentionTargets.read(),
    locale: () => localization.locale,
    timeFormat: () => preferences.calendarTimeFormat,
    translate: t,
    onTextInput: (blockId, value, selection) => onTextInput(blockId, value, selection),
    onReplaceRichText: (blockId, richText) => onReplaceRichText(blockId, richText),
    onInsertPageMention: (blockId, start, end, target) => onInsertPageMention(blockId, start, end, target),
    onInsertDateMention: (blockId, start, end, target) => onInsertDateMention(blockId, start, end, target),
    onInsertObjectMention: (blockId, start, end, target) => onInsertObjectMention(blockId, start, end, target),
    onApplyTextLink: (blockId, start, end, url) => onApplyTextLink(blockId, start, end, url),
    onInsertInlineEquation: (blockId, start, end, expression) => onInsertInlineEquation(blockId, start, end, expression),
    onPastePlainText: (blockId, start, end, value) => onPastePlainText(blockId, start, end, value),
    onPasteRichHtml: (blockId, start, end, html) => onPasteRichHtml(blockId, start, end, html),
    onApplyTextAnnotations: (blockId, start, end, patch) => onApplyTextAnnotations(blockId, start, end, patch),
    onCreateInlineComment: (blockId, start, end) => onCreateInlineComment(blockId, start, end),
    onCreateInlineSuggestion: (blockId, start, end) => onCreateInlineSuggestion(blockId, start, end),
    onKeyboardAction: (blockId, action) => onKeyboardAction(blockId, action),
    onUndo: () => onUndo(),
    onRedo: () => onRedo(),
    onConvert: (blockId, type, clearText) => onConvert(blockId, type, clearText),
    onConvertToToggleHeading: (blockId, type, clearText) => onConvertToToggleHeading(blockId, type, clearText),
    onColorChange: (blockId, color) => onColorChange(
      color.endsWith("_background") && calloutBackgroundTargetId ? calloutBackgroundTargetId : blockId,
      color,
    ),
    onCopyLink: (blockId) => onCopyLink(blockId),
    onDuplicate: (blockId) => onDuplicate(blockId),
    onMoveUp: (blockId) => onMoveUp(blockId),
    onMoveDown: (blockId) => onMoveDown(blockId),
    onDelete: (blockId) => onDelete(blockId),
    onToggleOpen: (blockId, open) => onToggleOpen(blockId, open),
    onFocusBlock: (blockId, preventScroll) => onFocusBlock(blockId, preventScroll),
  });
  const runtime = controller.runtime;
  const text = $derived(controller.text);
  const editableRichText = $derived(controller.editableRichText);
  const canOpenLinkEditor = $derived(controller.canOpenLinkEditor);
  const contextMenuOpen = $derived(controller.contextMenuOpen);
  const contextMenuPoint = $derived(controller.contextMenuPoint);
  const hasTextSelection = $derived(controller.hasTextSelection);
  const canFormatSelection = $derived(controller.canFormatSelection);
  const currentTextAnnotationRange = $derived(controller.currentTextAnnotationRange);
  const controlLoadStates = $derived(runtime.controlLoadStates);
  const mentionOpen = $derived(controller.mentionOpen);
  $effect(() => {
    if (mentionOpen) return mentionTargets.acquire();
  });
  const mentionMatches = $derived(controller.mentionMatches);
  const mentionInvite = $derived(controller.mentionInvite);
  const slashOpen = $derived(controller.slashOpen);
  const slashQuery = $derived(controller.slashQuery);
  const slashActiveDescendant = $derived(controller.slashActiveDescendant);
  const blockSupportsColor = $derived(controller.blockSupportsColor);
  const currentColor = $derived(controller.currentColor);
  const inlineEquationErrorReason = $derived(controller.inlineEquationErrorReason);
  const linkEditorOpen = $derived(controller.linkEditorOpen);
  const linkRange = $derived(controller.linkRange);
  const linkUrlInput = $derived(controller.linkUrlInput);
  const linkError = $derived(controller.linkError);

  const toggleTextAnnotation = controller.toggleTextAnnotation.bind(controller);
  function applyTextColor(color: NotesColor): void {
    if (color.endsWith("_background") && calloutBackgroundTargetId) {
      onColorChange(calloutBackgroundTargetId, color);
      return;
    }
    controller.applyTextColor(color);
  }
  const insertInlineEquationFromSelection = controller.insertInlineEquationFromSelection.bind(controller);
  const createInlineCommentFromSelection = controller.createInlineCommentFromSelection.bind(controller);
  const createInlineSuggestionFromSelection = controller.createInlineSuggestionFromSelection.bind(controller);
  const openLinkEditorFromButton = controller.openLinkEditorFromButton;
  const handleInput = controller.handleInput;
  const handleKeydown = controller.handleKeydown;
  const handleBeforeInput = controller.handleBeforeInput;
  const handleCompositionEnd = controller.handleCompositionEnd;
  const handlePaste = controller.handlePaste;
  const syncTextSelection = controller.syncTextSelection;
  const handleEditorFocus = controller.handleEditorFocus;
  const handleEditorBlur = controller.handleEditorBlur;

  $effect(() => {
    if (block.type === "code") runtime.requestControl("code-language");
  });

  const selectSlashCommand = controller.selectSlashCommand;
  const applyLinkFromEditor = controller.applyLinkFromEditor;
  const removeLinkFromEditor = controller.removeLinkFromEditor;
  const inlineEquationErrorMessage = controller.inlineEquationErrorMessage.bind(controller);
</script>

{#if block.type === "code"}
  <div class="mb-1 flex justify-end">
    {#if controlLoadStates["code-language"]?.status === "ready" && controlLoadStates["code-language"].component.kind === "code-language"}
      {@const Select = controlLoadStates["code-language"].component.component}
      <Select
        inline
        appearance="quiet"
        contentAlign="start"
        class="w-36 min-w-0"
        ariaLabel={t("notes.codeLanguage")}
        value={String(block.code.language ?? "")}
        options={[{ value: "plain text", label: t("notes.codeLanguagePlainText") },
          { value: "typescript", label: "TypeScript" },
          { value: "rust", label: "Rust" },
          { value: "sql", label: "SQL" },
          { value: "bash", label: "Bash" },
          { value: "json", label: "JSON" },
          { value: "markdown", label: "Markdown" }]}
        onChange={(nextValue) => onCodeLanguageChange(block.id, nextValue)}
      />
    {:else if controlLoadStates["code-language"]?.status === "failed"}
      <button type="button" class="h-7 rounded-md px-2 text-[0.8rem] hover:bg-accent" onclick={() => runtime.requestControl("code-language", true)}>{t("common.retry")}</button>
    {:else}
      <span class="h-7 px-2 text-[0.8rem] text-muted-foreground" aria-busy="true">{block.code.language}</span>
    {/if}
  </div>
{:else if block.type === "template"}
  {#if controller.templateControlsOpen && controlLoadStates["template-controls"]?.status === "ready" && controlLoadStates["template-controls"].component.kind === "template-controls"}
    {@const NotesTemplateBlockControls = controlLoadStates["template-controls"].component.component}
    <NotesTemplateBlockControls
    blockId={block.id}
    title={text || t("notes.blockType.template")}
    status={templateStatus}
    {onUseTemplate}
    {onAddTemplateChild}
    />
  {:else}
    <button class="mb-1 min-h-8 rounded-md border border-border px-2 text-[0.8rem] text-foreground hover:bg-accent" type="button" onclick={controller.openTemplateControls}>{controlLoadStates["template-controls"]?.status === "failed" ? t("common.retry") : t("notes.blockType.template")}</button>
  {/if}
{:else if block.type === "button"}
  {#if controller.buttonControlsOpen && controlLoadStates["button-controls"]?.status === "ready" && controlLoadStates["button-controls"].component.kind === "button-controls"}
    {@const NotesButtonBlockControls = controlLoadStates["button-controls"].component.component}
    <NotesButtonBlockControls
    blockId={block.id}
    title={text || t("notes.blockType.button")}
    button={block.button}
    status={buttonStatus}
    {onUseButton}
    {onAddButtonChild}
    {onButtonIconChange}
    {onButtonInsertPositionChange}
    />
  {:else}
    <button class="mb-1 min-h-8 rounded-md border border-border px-2 text-[0.8rem] text-foreground hover:bg-accent" type="button" onclick={controller.openButtonControls}>{controlLoadStates["button-controls"]?.status === "failed" ? t("common.retry") : text || t("notes.blockType.button")}</button>
  {/if}
{/if}
<div
  bind:this={runtime.editor}
  id={notesRichTextEditorDomId(block.id)}
  class={notesRichTextEditorClass(block.type)}
  role="textbox"
  aria-multiline="true"
  aria-label={t("notes.richTextEditorLabel")}
  aria-describedby={mentionOpen || slashOpen
    ? notesRichTextEditorStatusDomId(block.id)
    : undefined}
  aria-controls={notesRichTextEditorControls(block.id, mentionOpen, slashOpen)}
  aria-activedescendant={mentionOpen
    ? notesRichTextEditorActiveDescendant(
      block.id,
      mentionOpen,
      controller.mentionActiveIndex,
      mentionMatches.length,
    )
    : slashActiveDescendant}
  use:notesMentionChipSelectionAction={editableRichText}
  contenteditable="true"
  spellcheck={block.type !== "code"}
  tabindex="0"
  data-notes-block-id={block.id}
  data-notes-link-actions
  data-empty={text === "" && !runtime.compositionActive ? "true" : undefined}
  data-placeholder={text === "" ? t("notes.emptyPagePlaceholder") : undefined}
  oninput={handleInput}
  onkeydowncapture={handleKeydown}
  onbeforeinput={handleBeforeInput}
  oncompositionstart={controller.handleCompositionStart}
  oncompositionend={handleCompositionEnd}
  onpaste={handlePaste}
  oncopy={controller.handleCopy}
  oncut={controller.handleCopy}
  onpointerdown={controller.captureContextMenuSelection}
  oncontextmenu={controller.openContextMenu}
  onkeyup={(event) => syncTextSelection(event.currentTarget)}
  onclick={(event) => { syncTextSelection(event.currentTarget); controller.handleLinkClick(event); }}
  onpointerover={controller.handleLinkPointerOver}
  onpointerout={controller.handleLinkPointerOut}
  onpointerup={(event) => syncTextSelection(event.currentTarget)}
  onmouseup={(event) => syncTextSelection(event.currentTarget)}
  onfocus={handleEditorFocus}
  onblur={handleEditorBlur}
><NotesRichTextInline richText={editableRichText} {commentAnchors} {suggestionAnchors} onEditDateMention={controller.openDateMentionEditor} /></div>
{#if contextMenuOpen && contextMenuPoint}
  {#if controlLoadStates["text-context-menu"]?.status === "ready" && controlLoadStates["text-context-menu"].component.kind === "text-context-menu"}
    {@const NotesTextContextMenu = controlLoadStates["text-context-menu"].component.component}
    <NotesTextContextMenu
      position={contextMenuPoint}
      focusOnOpen={controller.contextMenuFocusOnOpen}
      annotations={currentTextAnnotationRange.annotations}
      blockType={block.type}
      hasSelection={hasTextSelection}
      {canFormatSelection}
      canOpenLink={canOpenLinkEditor}
      onToggleAnnotation={toggleTextAnnotation}
      onColorSelect={applyTextColor}
      canSetCalloutBackground={calloutBackgroundTargetId !== null}
      calloutBackgroundColor={calloutBackgroundColor ?? undefined}
      onCreateEquation={insertInlineEquationFromSelection}
      onCreateComment={createInlineCommentFromSelection}
      onCreateSuggestion={createInlineSuggestionFromSelection}
      onOpenLink={openLinkEditorFromButton}
      onCopyBlockLink={() => onCopyLink(block.id)}
      onConvert={(type) => onConvert(block.id, type)}
      onInsert={(type) => onAddBelow(block.id, { kind: "block", blockType: type })}
      onCut={controller.cutSelectedText}
      onCopy={controller.copySelectedText}
      onPaste={() => controller.pasteFromClipboard()}
      onPastePlainText={() => controller.pasteFromClipboard(true)}
      onClose={controller.closeContextMenu}
    />
  {:else if controlLoadStates["text-context-menu"]?.status === "failed"}
    <button class="surface-floating fixed z-50 min-h-8 px-2" style:left={`${contextMenuPoint.x}px`} style:top={`${contextMenuPoint.y}px`} type="button" onclick={() => runtime.requestControl("text-context-menu", true)}>{t("common.retry")}</button>
  {/if}
{/if}
{#if mentionOpen || slashOpen}
  <p id={notesRichTextEditorStatusDomId(block.id)} class="sr-only" role="status">
    {mentionOpen
      ? t("notes.richTextMentionMenuStatus", mentionMatches.length)
      : t("notes.richTextSlashMenuStatus")}
  </p>
{/if}
{#if inlineEquationErrorReason}
  <p class="mt-1 text-[0.733333rem] text-destructive" aria-live="polite">
    {inlineEquationErrorMessage(inlineEquationErrorReason)}
  </p>
{/if}
{#if linkEditorOpen}
  {#if controlLoadStates["link-editor"]?.status === "ready" && controlLoadStates["link-editor"].component.kind === "link-editor"}
    {@const NotesLinkEditor = controlLoadStates["link-editor"].component.component}
    <NotesLinkEditor
    value={linkUrlInput}
    title={controller.linkTitleInput}
    mode={controller.linkMode}
    destinationTitle={controller.linkDestinationTitle}
    pageTargets={controller.linkPageTargets}
    busy={controller.linkBusy}
    editor={runtime.editor}
    readAnchor={controller.readLinkAnchor}
    error={linkError}
    canRemove={linkRange.url !== null}
    onInput={controller.updateLinkInput}
    onTitleInput={controller.updateLinkTitle}
    onOpen={() => { void controller.openLinkDestination(); }}
    onCopy={() => { void controller.copyLinkDestination(); }}
    onEdit={controller.editLinkDestination}
    onPointerEnter={controller.keepLinkPreviewOpen}
    onPointerLeave={controller.scheduleLinkPreviewClose}
    onApply={() => {
      void applyLinkFromEditor();
    }}
    onRemove={() => {
      void removeLinkFromEditor();
    }}
    onCancel={controller.cancelLinkEditor}
    />
  {:else if controlLoadStates["link-editor"]?.status === "failed"}
    <button class="surface-floating fixed z-60 min-h-8 px-2" type="button" onclick={() => runtime.requestControl("link-editor", true)}>{t("common.retry")}</button>
  {/if}
{/if}

{#if controller.editedDateMention}
  {#if controlLoadStates["date-mention-editor"]?.status === "ready" && controlLoadStates["date-mention-editor"].component.kind === "date-mention-editor"}
    {@const NotesDateMentionEditor = controlLoadStates["date-mention-editor"].component.component}
    <NotesDateMentionEditor anchor={controller.dateMentionAnchor} mention={controller.editedDateMention}
      onChange={controller.changeDateMention} onClose={controller.closeDateMentionEditor} />
  {:else if controlLoadStates["date-mention-editor"]?.status === "failed"}
    <button class="surface-floating fixed z-60 min-h-8 px-2" type="button" onclick={() => runtime.requestControl("date-mention-editor", true)}>{t("common.retry")}</button>
  {/if}
{/if}

{#if mentionOpen || mentionInvite}
  {#if controlLoadStates["mention-menu"]?.status === "ready" && controlLoadStates["mention-menu"].component.kind === "mention-menu"}
    {@const NotesMentionMenu = controlLoadStates["mention-menu"].component.component}
    {@const mention = mentionInvite ?? controller.mentionQuery}
    <NotesMentionMenu
    menuId={notesMentionMenuDomId(block.id)}
    blockId={block.id}
    anchor={runtime.editor}
    anchorOffset={mention?.start ?? 0}
    query={mention?.query ?? ""}
    groups={controller.mentionGroups}
    activeIndex={controller.mentionActiveIndex}
    localUserId={notesEditor.localUser?.id ?? null}
    inviting={mentionInvite !== null}
    onSelect={controller.selectMentionRow}
    onHighlight={(index) => { controller.mentionActiveIndex = index; }}
    onClose={controller.closeMentionMenu}
    onInviteBack={() => { void controller.closeMentionInvite(true); }}
    onInviteDismiss={() => { void controller.closeMentionInvite(false); }}
    />
  {:else if controlLoadStates["mention-menu"]?.status === "failed"}
    <button class="min-h-8 rounded-md border border-border px-2 text-[0.8rem]" type="button" onclick={() => runtime.requestControl("mention-menu", true)}>{t("common.retry")}</button>
  {/if}
{:else if slashOpen}
  {#if controlLoadStates["slash-menu"]?.status === "ready" && controlLoadStates["slash-menu"].component.kind === "slash-menu"}
    {@const NotesSlashMenu = controlLoadStates["slash-menu"].component.component}
    <NotesSlashMenu
    anchor={runtime.editor}
    onClose={() => controller.closeSlashMenu()}
    menuId={notesSlashMenuDomId(block.id)}
    blockId={block.id}
    query={slashQuery}
    activeIndex={controller.slashActiveIndex}
    canSetColor={blockSupportsColor}
    currentColor={currentColor}
    onActiveIndexChange={controller.updateSlashActiveIndex}
    onActiveCommandChange={controller.updateSlashActiveCommand}
    onSelect={selectSlashCommand}
    />
  {:else if controlLoadStates["slash-menu"]?.status === "failed"}
    <button class="min-h-8 rounded-md border border-border px-2 text-[0.8rem]" type="button" onmousedown={(event) => event.preventDefault()} onclick={() => runtime.requestControl("slash-menu", true)}>{t("common.retry")}</button>
  {:else}
    <div class="text-sm text-muted-foreground" role="status">{t("common.loading")}</div>
  {/if}
{/if}

<style>
  .notes-rich-text-editor {
    caret-color: var(--foreground);
  }

  /* The block list marks a page whose body is a lone empty paragraph; floating the hint keeps the caret at the start. */
  :global(.notes-block-list[data-notes-empty-page]) .notes-rich-text-editor[data-empty="true"]::before {
    float: left;
    height: 0;
    color: var(--muted-foreground);
    content: attr(data-placeholder);
    pointer-events: none;
  }

</style>
