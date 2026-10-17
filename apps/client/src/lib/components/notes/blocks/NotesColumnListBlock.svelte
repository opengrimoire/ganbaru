<script lang="ts">
  import type { NotesMentionCatalog } from "./mention-data-controller.svelte";
  import { tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesBlockAnchorId } from "$lib/notes/links/block-link";
  import { notesEmbeddedCalloutLayers } from "$lib/notes/block-types/callout-layout";
  import { notesBlockContextMenuPoint } from "$lib/notes/blocks/handle";
  import type { NotesMoveToPageTarget } from "$lib/notes/blocks/move";
  import type { NotesBlockInsertRequest } from "$lib/notes/blocks/insertion";
  import {
    planNotesKeyboardAction,
    type NotesKeyboardAction,
  } from "$lib/notes/blocks/keyboard";
  import type { NotesMediaAssetChange } from "$lib/notes/blocks/factory";
  import {
    NOTES_COLUMN_MIN_WIDTH_RATIO,
    notesColumnCanAdd,
    notesColumnCanMove,
    notesColumnCanRemove,
    notesColumnGridTemplate,
    notesColumnWidths,
    type NotesColumnMoveDirection,
  } from "$lib/notes/block-types/column";
  import { notesUndoShortcutAction } from "$lib/notes/history/undo-history";
  import type { NotesTemplateBlockStatus } from "$lib/notes/block-types/template";
  import type { NotesButtonBlockStatus } from "$lib/notes/block-types/button";
  import type { NotesUnsupportedConversionTarget } from "$lib/notes/block-types/unsupported";
  import type { NotesHeadingBlockType } from "$lib/notes/blocks/factory";
  import type { NotesSlashAction, NotesSlashCommand } from "$lib/notes/editor/slash-commands";
  import type {
    NotesDateMentionTarget,
    NotesObjectMentionTarget,
    NotesPageMentionTarget,
    NotesRichTextAnnotationPatch,
  } from "$lib/notes/rich-text/core";
  import type { NotesTextSelection } from "$lib/notes/editor/selection";
  import type { NotesBlockDropIndicator } from "$lib/notes/blocks/drag";
  import type {
    NotesBlockTreeItem,
    NotesBlockType,
    NotesButtonInsertPosition,
    NotesColor,
    NotesColumnBlockItems,
    NotesIcon,
    NotesPageBreadcrumbItem,
    NotesRichText,
    NotesTableOfContentsItem,
    NotesTableRowBlock,
  } from "$lib/notes/types";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import NotesBlockHandle from "./NotesBlockHandle.svelte";
  import NotesBlockRow from "./NotesBlockRow.svelte";
  import NotesCalloutEmbeddedRow from "./NotesCalloutEmbeddedRow.svelte";
  import NotesLazySlashMenu from "./NotesLazySlashMenu.svelte";

  let {
    item,
    columnItems,
    breadcrumbItems,
    tableOfContentsItems,
    tableRowsForBlock,
    previousBlockType,
    previousBlockTypeForBlock,
    isOnlyBlock,
    isOnlyBlockForBlock,
    focusBlockId,
    focusRequestId,
    focusSelection,
    listOrdinals = new Map<string, number>(),
    mentionTargets,
    templateStatusForBlock,
    buttonStatusForBlock,
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
    onCalloutIconChange,
    onCopyLink,
    onDuplicate,
    onUseTemplate,
    onAddTemplateChild,
    onUseButton,
    onAddButtonChild,
    onButtonIconChange,
    onButtonInsertPositionChange,
    onCreateLinkedDatabaseView,
    onConvertUnsupported,
    onComment,
    onMoveUp,
    onMoveDown,
    moveTargets,
    moveTargetsForBlock,
    onMoveToPage,
    onDelete,
    isDragging,
    dropPosition,
    draggingBlockId,
    dropPositionForBlock,
    onDragStart,
    onDragEnd,
    onDragOver,
    onDragLeave,
    onDrop,
    onToggleTodo,
    onToggleOpen,
    onCodeLanguageChange,
    onBookmarkChange,
    onLinkPreviewUrlChange,
    onEmbedUrlChange,
    onEquationExpressionChange,
    onMediaChange,
    onTableCellRichTextChange,
    onAddTableRow,
    onRemoveTableRow,
    onAddTableColumn,
    onRemoveTableColumn,
    onAddColumn,
    onRemoveColumn,
    onMoveColumn,
    onResizeColumn,
    onMoveBlockToColumn,
    onSelectPage,
    onFocusBlock,
    onHandleMenuOpenChange,
  }: {
    item: NotesBlockTreeItem;
    columnItems: NotesColumnBlockItems[];
    breadcrumbItems: NotesPageBreadcrumbItem[];
    tableOfContentsItems: NotesTableOfContentsItem[];
    tableRowsForBlock: (blockId: string) => NotesTableRowBlock[];
    previousBlockType: NotesBlockType | null;
    previousBlockTypeForBlock: (blockId: string) => NotesBlockType | null;
    isOnlyBlock: boolean;
    isOnlyBlockForBlock: (blockId: string) => boolean;
    focusBlockId: string | null;
    focusRequestId: number;
    focusSelection: NotesTextSelection | null;
    listOrdinals?: ReadonlyMap<string, number>;
    mentionTargets: NotesMentionCatalog;
    templateStatusForBlock: (blockId: string) => NotesTemplateBlockStatus;
    buttonStatusForBlock: (blockId: string) => NotesButtonBlockStatus;
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
    onCalloutIconChange: (blockId: string, icon: NotesIcon | null) => Promise<void> | void;
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
    onCreateLinkedDatabaseView: (blockId: string) => Promise<void> | void;
    onConvertUnsupported: (
      blockId: string,
      target: NotesUnsupportedConversionTarget,
    ) => Promise<void> | void;
    onComment: (blockId: string) => void;
    onMoveUp: (blockId: string) => void;
    onMoveDown: (blockId: string) => void;
    moveTargets: NotesMoveToPageTarget[];
    moveTargetsForBlock: (blockId: string) => NotesMoveToPageTarget[];
    onMoveToPage: (blockId: string, pageId: string) => void;
    onDelete: (blockId: string) => void;
    isDragging: boolean;
    dropPosition: NotesBlockDropIndicator | null;
    draggingBlockId: string | null;
    dropPositionForBlock: (blockId: string) => NotesBlockDropIndicator | null;
    onDragStart: (blockId: string, event: DragEvent) => void;
    onDragEnd: () => void;
    onDragOver: (blockId: string, event: DragEvent) => void;
    onDragLeave: (blockId: string, event: DragEvent) => void;
    onDrop: (blockId: string, event: DragEvent) => void;
    onToggleTodo: (blockId: string, checked: boolean) => void;
    onToggleOpen: (blockId: string, open: boolean) => void;
    onCodeLanguageChange: (blockId: string, language: string) => void;
    onBookmarkChange: (blockId: string, url: string, caption: string) => void;
    onLinkPreviewUrlChange: (blockId: string, url: string) => void;
    onEmbedUrlChange: (blockId: string, url: string) => void;
    onEquationExpressionChange: (blockId: string, expression: string) => void;
    onMediaChange: (
      blockId: string,
      url: string,
      caption: string,
      name?: string,
      assetChange?: NotesMediaAssetChange,
    ) => void;
    onTableCellRichTextChange: (
      rowBlockId: string,
      columnIndex: number,
      richText: readonly NotesRichText[],
    ) => Promise<void> | void;
    onAddTableRow: (tableBlockId: string, afterRowIndex: number) => Promise<void> | void;
    onRemoveTableRow: (tableBlockId: string, rowBlockId: string) => Promise<void> | void;
    onAddTableColumn: (tableBlockId: string, afterColumnIndex: number) => Promise<void> | void;
    onRemoveTableColumn: (tableBlockId: string, columnIndex: number) => Promise<void> | void;
    onAddColumn: (columnListBlockId: string, afterColumnIndex: number) => Promise<void> | void;
    onRemoveColumn: (columnListBlockId: string, columnBlockId: string) => Promise<void> | void;
    onMoveColumn: (
      columnListBlockId: string,
      columnBlockId: string,
      direction: NotesColumnMoveDirection,
    ) => Promise<void> | void;
    onResizeColumn: (
      columnListBlockId: string,
      columnBlockId: string,
      widthRatio: number,
    ) => Promise<void> | void;
    onMoveBlockToColumn: (blockId: string, columnBlockId: string) => Promise<void> | void;
    onSelectPage: (pageId: string) => void;
    onFocusBlock: (blockId: string, preventScroll?: boolean) => void;
    onHandleMenuOpenChange: (blockId: string, open: boolean) => void;
  } = $props();

  const { t } = getLocalization();
  let focusButton: HTMLButtonElement | null = $state(null);
  let slashOpen = $state(false);
  let columnDropTargetId = $state<string | null>(null);
  const block = $derived(item.block);
  const columns = $derived(columnItems.map((columnItem) => columnItem.column));
  const calloutLayersByColumn = $derived(new Map(columnItems.map((columnItem) => [
    columnItem.column.id, notesEmbeddedCalloutLayers(columnItem.items),
  ])));
  const columnTemplate = $derived(notesColumnGridTemplate(columns));
  const columnWidths = $derived(notesColumnWidths(columns));
  const canAddColumn = $derived(notesColumnCanAdd(columnItems.length));
  const canRemoveColumn = $derived(notesColumnCanRemove(columnItems.length));
  const minColumnWidthPercent = Math.round(NOTES_COLUMN_MIN_WIDTH_RATIO * 100);
  const maxColumnWidthPercent = $derived(
    Math.round((1 - NOTES_COLUMN_MIN_WIDTH_RATIO * Math.max(0, columnItems.length - 1)) * 100),
  );

  $effect(() => {
    const _focusRequestId = focusRequestId;
    if (focusBlockId !== block.id) return;
    void tick().then(() => {
      focusButton?.focus();
    });
  });

  function handleKeydown(event: KeyboardEvent): void {
    const undoAction = notesUndoShortcutAction(event);
    if (undoAction) {
      event.preventDefault();
      void Promise.resolve(undoAction === "undo" ? onUndo() : onRedo());
      return;
    }
    const action = planNotesKeyboardAction({
      key: event.key,
      shiftKey: event.shiftKey,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      altKey: event.altKey,
      text: "",
      selectionStart: 0,
      selectionEnd: 0,
      blockType: block.type,
      previousBlockType,
      isOnlyBlock,
    });
    if (action.type === "none" || action.type === "insert_newline") return;
    if (action.type === "open_slash_menu") {
      slashOpen = true;
      return;
    }
    if (action.preventDefault) event.preventDefault();
    slashOpen = false;
    onKeyboardAction(block.id, action);
  }

  function selectSlashCommand(command: NotesSlashCommand): void {
    slashOpen = false;
    switch (command.kind) {
      case "block":
        onConvert(block.id, command.blockType, true);
        return;
      case "toggle_heading":
        onConvertToToggleHeading(block.id, command.headingType);
        return;
      case "action":
        runSlashAction(command.action);
        return;
      case "color":
        return;
    }
  }

  function runSlashAction(action: NotesSlashAction): void {
    switch (action) {
      case "copy_link":
        void Promise.resolve(onCopyLink(block.id)).catch((error) => {
          console.warn("copy notes block link failed", error);
        });
        return;
      case "duplicate":
        onDuplicate(block.id);
        return;
      case "move_up":
        onMoveUp(block.id);
        return;
      case "move_down":
        onMoveDown(block.id);
        return;
      case "delete":
        onDelete(block.id);
        return;
    }
  }

  function openTurnIntoMenu(): void {
    slashOpen = true;
  }

  function columnWidthPercent(columnIndex: number): number {
    return Math.round((columnWidths[columnIndex] ?? 1) * 100);
  }

  function handleColumnDropZoneDragOver(event: DragEvent, columnBlockId: string): void {
    if (!draggingBlockId) return;
    event.preventDefault();
    event.stopPropagation();
    columnDropTargetId = columnBlockId;
  }

  function handleColumnDropZoneDragLeave(event: DragEvent, columnBlockId: string): void {
    const target = event.currentTarget;
    const related = event.relatedTarget;
    if (
      target instanceof HTMLElement
      && related instanceof Node
      && target.contains(related)
    ) {
      return;
    }
    if (columnDropTargetId === columnBlockId) columnDropTargetId = null;
  }

  function handleColumnDropZoneDrop(event: DragEvent, columnBlockId: string): void {
    if (!draggingBlockId) return;
    event.preventDefault();
    event.stopPropagation();
    const blockId = draggingBlockId;
    columnDropTargetId = null;
    void Promise.resolve(onMoveBlockToColumn(blockId, columnBlockId)).catch((error) => {
      console.warn("move notes block to column failed", error);
    });
  }

  let contextMenuRequest = $state<{ x: number; y: number; id: number } | null>(null);
  let contextMenuRequestId = 0;

  function openBlockContextMenu(event: MouseEvent): void {
    const point = notesBlockContextMenuPoint(event);
    if (point) contextMenuRequest = { ...point, id: ++contextMenuRequestId };
  }
</script>

<div
  id={notesBlockAnchorId(block.id)}
  class="notes-block-row notes-column-list-row group relative"
  role="group"
  tabindex="-1"
  data-notes-selectable-block-id={block.id}
  class:notes-block-focused={focusBlockId === block.id}
  class:notes-block-dragging={isDragging}
  class:notes-block-drop-before={dropPosition === "before"}
  class:notes-block-drop-after={dropPosition === "after"}
  class:notes-block-drop-inside={dropPosition === "inside"}
  class:notes-block-drop-outdent={dropPosition === "outdent"}
  style={`--notes-depth: ${item.depth}`}
  ondragover={(event) => onDragOver(block.id, event)}
  ondragleave={(event) => onDragLeave(block.id, event)}
  ondrop={(event) => onDrop(block.id, event)}
  oncontextmenu={openBlockContextMenu}
>
  <div
    class="notes-block-surface flex min-w-0 items-start gap-1 rounded-md py-0.5 pr-2 hover:bg-accent/50"
    data-notes-block-selection-zone
  >
    {#if item.depth > 0}
      <div class="notes-block-indent shrink-0"></div>
    {/if}
    <NotesBlockHandle
      {contextMenuRequest}
      onTurnInto={openTurnIntoMenu}
      canSetColor={false}
      currentColor="default"
      onColorSelect={() => undefined}
      onCopyLink={() => onCopyLink(block.id)}
      onDuplicate={() => onDuplicate(block.id)}
      onComment={() => onComment(block.id)}
      onMoveUp={() => onMoveUp(block.id)}
      onMoveDown={() => onMoveDown(block.id)}
      {moveTargets}
      onMoveToPage={(pageId) => onMoveToPage(block.id, pageId)}
      onDelete={() => onDelete(block.id)}
      onMenuOpenChange={(open) => onHandleMenuOpenChange(block.id, open)}
    />

    <div class="relative min-w-0 flex-1">
      <section class="notes-column-layout my-1 min-w-0" aria-label={t("notes.blockType.columns")}>
        <div class="notes-column-toolbar" role="toolbar" aria-label={t("notes.columnLayoutActions")}>
          <button
            type="button"
            class="notes-column-tool-button"
            aria-label={t("notes.addColumn")}
            title={t("notes.addColumn")}
            disabled={!canAddColumn}
            onclick={() => {
              void Promise.resolve(onAddColumn(block.id, Math.max(0, columnItems.length - 1)));
            }}
          >
            <Plus class="size-3.5" aria-hidden="true" />
            <span class="sr-only">{t("notes.addColumn")}</span>
          </button>
        </div>
        {#if columnItems.length === 0}
          <button
            bind:this={focusButton}
            type="button"
            class="min-h-8 w-full rounded px-1 text-left text-[0.8rem] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onkeydown={handleKeydown}
            onclick={() => {
              void Promise.resolve(onAddColumn(block.id, 0));
            }}
          >
            {t("notes.emptyColumns")}
          </button>
        {:else}
          <div class="notes-column-grid" style={`--notes-column-template: ${columnTemplate}`}>
            {#each columnItems as columnItem, columnIndex (columnItem.column.id)}
              <section
                class="notes-column min-w-0"
                aria-label={t("notes.column", columnIndex + 1)}
              >
                <div
                  class="notes-column-header"
                  role="toolbar"
                  aria-label={t("notes.columnActions", columnIndex + 1)}
                >
                  <button
                    type="button"
                    class="notes-column-tool-button"
                    aria-label={t("notes.moveColumnLeft", columnIndex + 1)}
                    title={t("notes.moveColumnLeft", columnIndex + 1)}
                    disabled={!notesColumnCanMove(columns, columnItem.column.id, "left")}
                    onclick={() => {
                      void Promise.resolve(onMoveColumn(block.id, columnItem.column.id, "left"));
                    }}
                  >
                    <ArrowLeft class="size-3.5" aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    class="notes-column-tool-button"
                    aria-label={t("notes.moveColumnRight", columnIndex + 1)}
                    title={t("notes.moveColumnRight", columnIndex + 1)}
                    disabled={!notesColumnCanMove(columns, columnItem.column.id, "right")}
                    onclick={() => {
                      void Promise.resolve(onMoveColumn(block.id, columnItem.column.id, "right"));
                    }}
                  >
                    <ArrowRight class="size-3.5" aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    class="notes-column-tool-button"
                    aria-label={t("notes.addColumnAfter", columnIndex + 1)}
                    title={t("notes.addColumnAfter", columnIndex + 1)}
                    disabled={!canAddColumn}
                    onclick={() => {
                      void Promise.resolve(onAddColumn(block.id, columnIndex));
                    }}
                  >
                    <Plus class="size-3.5" aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    class="notes-column-tool-button notes-column-danger-button"
                    aria-label={t("notes.removeColumn", columnIndex + 1)}
                    title={t("notes.removeColumn", columnIndex + 1)}
                    disabled={!canRemoveColumn}
                    onclick={() => {
                      void Promise.resolve(onRemoveColumn(block.id, columnItem.column.id));
                    }}
                  >
                    <Trash2 class="size-3.5" aria-hidden="true" />
                  </button>
                  <input
                    class="notes-column-width-slider"
                    type="range"
                    min={minColumnWidthPercent}
                    max={maxColumnWidthPercent}
                    step="1"
                    value={columnWidthPercent(columnIndex)}
                    aria-label={t("notes.columnWidth", columnIndex + 1, columnWidthPercent(columnIndex))}
                    onchange={(event) => {
                      onResizeColumn(
                        block.id,
                        columnItem.column.id,
                        event.currentTarget.valueAsNumber / 100,
                      );
                    }}
                  />
                  <span class="notes-column-width-value" aria-hidden="true">
                    {columnWidthPercent(columnIndex)}%
                  </span>
                </div>
                {#if columnItem.items.length === 0}
                  <button
                    bind:this={focusButton}
                    type="button"
                    class="min-h-8 w-full rounded px-1 text-left text-[0.8rem] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    onkeydown={handleKeydown}
                  >
                    {t("notes.emptyColumns")}
                  </button>
                {:else}
                  <div class="flex min-w-0 flex-col">
                    {#each columnItem.items as columnBlockItem (columnBlockItem.block.id)}
                      <NotesCalloutEmbeddedRow layers={calloutLayersByColumn.get(columnItem.column.id)?.get(columnBlockItem.block.id) ?? []}>
                        {#snippet body()}
                          <NotesBlockRow
                            item={columnBlockItem}
                            {breadcrumbItems}
                            {tableOfContentsItems}
                            tableRows={tableRowsForBlock(columnBlockItem.block.id)}
                            previousBlockType={previousBlockTypeForBlock(columnBlockItem.block.id)}
                            isOnlyBlock={isOnlyBlockForBlock(columnBlockItem.block.id)}
                            {focusBlockId}
                            {focusRequestId}
                            {focusSelection}
                            {listOrdinals}
                            {mentionTargets}
                            templateStatus={templateStatusForBlock(columnBlockItem.block.id)}
                            buttonStatus={buttonStatusForBlock(columnBlockItem.block.id)}
                            {onTextInput}
                            {onReplaceRichText}
                            {onInsertPageMention}
                            {onInsertDateMention}
                            {onInsertObjectMention}
                            {onApplyTextLink}
                            {onInsertInlineEquation}
                            {onPastePlainText}
                            {onPasteRichHtml}
                            {onApplyTextAnnotations}
                            {onCreateInlineComment}
                            {onCreateInlineSuggestion}
                            {onKeyboardAction}
                            {onUndo}
                            {onRedo}
                            {onAddBelow}
                            {onConvert}
                            {onConvertToToggleHeading}
                            {onColorChange}
                            {onCalloutIconChange}
                            {onCopyLink}
                            {onDuplicate}
                            {onUseTemplate}
                            {onAddTemplateChild}
                            {onUseButton}
                            {onAddButtonChild}
                            {onButtonIconChange}
                            {onButtonInsertPositionChange}
                            {onCreateLinkedDatabaseView}
                            {onConvertUnsupported}
                            {onComment}
                            {onMoveUp}
                            {onMoveDown}
                            moveTargets={moveTargetsForBlock(columnBlockItem.block.id)}
                            {onMoveToPage}
                            {onDelete}
                            isDragging={draggingBlockId === columnBlockItem.block.id}
                            dropPosition={dropPositionForBlock(columnBlockItem.block.id)}
                            {onDragStart}
                            {onDragEnd}
                            {onDragOver}
                            {onDragLeave}
                            {onDrop}
                            {onToggleTodo}
                            {onToggleOpen}
                            {onCodeLanguageChange}
                            {onBookmarkChange}
                            {onLinkPreviewUrlChange}
                            {onEmbedUrlChange}
                            {onEquationExpressionChange}
                            {onMediaChange}
                            {onTableCellRichTextChange}
                            {onAddTableRow}
                            {onRemoveTableRow}
                            {onAddTableColumn}
                            {onRemoveTableColumn}
                            {onSelectPage}
                            {onFocusBlock}
                            {onHandleMenuOpenChange}
                          />
                        {/snippet}
                      </NotesCalloutEmbeddedRow>
                    {/each}
                  </div>
                {/if}
                {#if draggingBlockId}
                  <div
                    class="notes-column-drop-zone"
                    class:notes-column-drop-zone-active={columnDropTargetId === columnItem.column.id}
                    role="button"
                    tabindex="-1"
                    aria-label={t("notes.dropBlockInColumn", columnIndex + 1)}
                    ondragover={(event) => handleColumnDropZoneDragOver(event, columnItem.column.id)}
                    ondragleave={(event) => handleColumnDropZoneDragLeave(event, columnItem.column.id)}
                    ondrop={(event) => handleColumnDropZoneDrop(event, columnItem.column.id)}
                  >
                    <span class="sr-only">{t("notes.dropBlockInColumn", columnIndex + 1)}</span>
                  </div>
                {/if}
              </section>
            {/each}
          </div>
        {/if}
      </section>

      {#if slashOpen}
        <NotesLazySlashMenu onSelect={selectSlashCommand} />
      {/if}
    </div>
  </div>
</div>

<style>
  .notes-block-indent {
    width: calc(var(--notes-depth) * 1.25rem);
  }

  .notes-column-layout {
    container-type: inline-size;
  }

  .notes-column-toolbar {
    display: flex;
    justify-content: flex-end;
    gap: 0.25rem;
    padding-bottom: 0.25rem;
  }

  .notes-column-header {
    display: grid;
    grid-template-columns: repeat(4, minmax(1.65rem, auto)) minmax(4.5rem, 1fr) auto;
    align-items: center;
    gap: 0.15rem;
    padding-bottom: 0.25rem;
  }

  .notes-column-tool-button {
    display: inline-flex;
    min-height: 1.65rem;
    min-width: 1.65rem;
    align-items: center;
    justify-content: center;
    border-radius: 0.25rem;
    color: hsl(var(--muted-foreground));
    outline: none;
  }

  .notes-column-tool-button:hover,
  .notes-column-tool-button:focus-visible {
    background: hsl(var(--accent));
    color: hsl(var(--accent-foreground));
  }

  .notes-column-tool-button:disabled {
    cursor: not-allowed;
    opacity: 0.38;
  }

  .notes-column-danger-button {
    color: hsl(var(--destructive));
  }

  .notes-column-width-slider {
    min-width: 0;
    accent-color: hsl(var(--primary));
  }

  .notes-column-width-value {
    min-width: 2.25rem;
    color: hsl(var(--muted-foreground));
    font-size: calc(0.733333rem * var(--type-scale));
    text-align: right;
  }

  .notes-block-dragging {
    opacity: 0.45;
  }

  .notes-block-drop-before::before,
  .notes-block-drop-after::after {
    position: absolute;
    left: calc(var(--notes-depth) * 1.25rem);
    right: 0.5rem;
    z-index: 5;
    height: 2px;
    border-radius: 999px;
    background: hsl(var(--primary));
    content: "";
  }

  .notes-block-drop-before::before {
    top: -1px;
  }

  .notes-block-drop-after::after {
    bottom: -1px;
  }

  .notes-block-drop-outdent::after {
    position: absolute;
    left: max(0rem, calc((var(--notes-depth) - 1) * 1.25rem));
    right: 0.5rem;
    bottom: -1px;
    z-index: 5;
    height: 2px;
    border-radius: 999px;
    background: hsl(var(--primary));
    content: "";
  }

  .notes-block-drop-inside > .notes-block-surface {
    background: hsl(var(--primary) / 0.1);
    box-shadow: inset 0 0 0 2px hsl(var(--primary) / 0.65);
  }

  .notes-column-grid {
    display: grid;
    grid-template-columns: var(--notes-column-template);
    gap: 0.75rem;
  }

  .notes-column {
    border-left: 1px solid var(--border);
    padding-left: 0.5rem;
  }

  .notes-column-drop-zone {
    min-height: 1.75rem;
    border: 1px dashed hsl(var(--border));
    border-radius: 0.375rem;
    background: hsl(var(--muted) / 0.16);
    outline: none;
  }

  .notes-column-drop-zone-active {
    border-color: hsl(var(--primary));
    background: hsl(var(--primary) / 0.1);
    box-shadow: inset 0 0 0 1px hsl(var(--primary) / 0.45);
  }

  @container (max-width: 30rem) {
    .notes-column-grid {
      grid-template-columns: 1fr;
    }

    .notes-column-header {
      grid-template-columns: repeat(4, minmax(1.65rem, auto)) minmax(3rem, 1fr) auto;
    }
  }
</style>
