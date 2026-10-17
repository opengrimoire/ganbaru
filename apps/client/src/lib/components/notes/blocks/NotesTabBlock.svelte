<script lang="ts">
  import type { NotesMentionCatalog } from "./mention-data-controller.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesBlockAnchorId } from "$lib/notes/links/block-link";
  import { notesEmbeddedCalloutLayers } from "$lib/notes/block-types/callout-layout";
  import { notesBlockContextMenuPoint } from "$lib/notes/blocks/handle";
  import { blockPlainText, type NotesHeadingBlockType } from "$lib/notes/blocks/factory";
  import type { NotesBlockInsertRequest } from "$lib/notes/blocks/insertion";
  import type { NotesMoveToPageTarget } from "$lib/notes/blocks/move";
  import {
    planNotesKeyboardAction,
    type NotesKeyboardAction,
  } from "$lib/notes/blocks/keyboard";
  import type { NotesMediaAssetChange } from "$lib/notes/blocks/factory";
  import { notesUndoShortcutAction } from "$lib/notes/history/undo-history";
  import type { NotesTemplateBlockStatus } from "$lib/notes/block-types/template";
  import type { NotesButtonBlockStatus } from "$lib/notes/block-types/button";
  import type { NotesUnsupportedConversionTarget } from "$lib/notes/block-types/unsupported";
  import {
    notesTabCanAdd,
    notesTabCanMove,
    notesTabCanRemove,
    notesTabIconOptions,
    type NotesTabMoveDirection,
  } from "$lib/notes/navigation/tabs";
  import type { NotesSlashAction, NotesSlashCommand } from "$lib/notes/editor/slash-commands";
  import type {
    NotesDateMentionTarget,
    NotesObjectMentionTarget,
    NotesPageMentionTarget,
    NotesRichTextAnnotationPatch,
  } from "$lib/notes/rich-text/core";
  import type { NotesTextSelection } from "$lib/notes/editor/selection";
  import type { NotesBlockDropIndicator } from "$lib/notes/blocks/drag";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import type {
    NotesBlockTreeItem,
    NotesBlockType,
    NotesButtonInsertPosition,
    NotesColor,
    NotesIcon,
    NotesPageBreadcrumbItem,
    NotesRichText,
    NotesTabBlockItems,
    NotesTableOfContentsItem,
    NotesTableRowBlock,
  } from "$lib/notes/types";
  import NotesBlockHandle from "./NotesBlockHandle.svelte";
  import NotesBlockRow from "./NotesBlockRow.svelte";
  import NotesCalloutEmbeddedRow from "./NotesCalloutEmbeddedRow.svelte";
  import NotesLazySlashMenu from "./NotesLazySlashMenu.svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";

  let {
    item,
    tabItems,
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
    onUpdateTabLabel,
    onUpdateTabIcon,
    onAddTab,
    onRemoveTab,
    onMoveTab,
    onMoveBlockToTab,
    onSelectPage,
    onFocusBlock,
    onHandleMenuOpenChange,
  }: {
    item: NotesBlockTreeItem;
    tabItems: NotesTabBlockItems[];
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
    onUpdateTabLabel: (labelBlockId: string, label: string) => Promise<void> | void;
    onUpdateTabIcon: (labelBlockId: string, icon: NotesIcon | null) => Promise<void> | void;
    onAddTab: (tabBlockId: string, afterTabIndex: number) => Promise<void> | void;
    onRemoveTab: (tabBlockId: string, labelBlockId: string) => Promise<void> | void;
    onMoveTab: (
      tabBlockId: string,
      labelBlockId: string,
      direction: NotesTabMoveDirection,
    ) => Promise<void> | void;
    onMoveBlockToTab: (blockId: string, labelBlockId: string) => Promise<void> | void;
    onSelectPage: (pageId: string) => void;
    onFocusBlock: (blockId: string, preventScroll?: boolean) => void;
    onHandleMenuOpenChange: (blockId: string, open: boolean) => void;
  } = $props();

  const { t } = getLocalization();
  let focusButton: HTMLButtonElement | null = $state(null);
  let slashOpen = $state(false);
  let activeTabId = $state<string | null>(null);
  let activeLabelDraft = $state("");
  const block = $derived(item.block);
  const tabIconChoices = notesTabIconOptions();
  const activeTab = $derived(
    tabItems.find((tab) => tab.label.id === activeTabId) ?? tabItems[0] ?? null,
  );
  const activeTabIndex = $derived(
    activeTab ? tabItems.findIndex((tab) => tab.label.id === activeTab.label.id) : -1,
  );
  const canAddTab = $derived(notesTabCanAdd(tabItems.length));
  const calloutLayersByTab = $derived(new Map(tabItems.map((tab) => [
    tab.label.id, notesEmbeddedCalloutLayers(tab.items),
  ])));
  const canRemoveTab = $derived(notesTabCanRemove(tabItems.length));

  $effect(() => {
    if (tabItems.length === 0) {
      activeTabId = null;
      return;
    }
    if (!tabItems.some((tab) => tab.label.id === activeTabId)) {
      activeTabId = tabItems[0]?.label.id ?? null;
    }
  });

  $effect(() => {
    activeLabelDraft = activeTab
      ? tabLabel(activeTab, activeTabIndex < 0 ? 0 : activeTabIndex)
      : "";
  });

  $effect(() => {
    const _focusRequestId = focusRequestId;
    if (focusBlockId !== block.id) return;
    void tick().then(() => {
      focusButton?.focus();
    });
  });

  function tabLabel(tab: NotesTabBlockItems, index: number): string {
    return blockPlainText(tab.label).trim() || t("notes.tab", index + 1);
  }

  function activeIconOptionId(): string {
    const icon = activeTab?.label.paragraph.icon ?? null;
    if (!icon) return "none";
    if (icon.type === "emoji") return `emoji:${icon.emoji}`;
    if (icon.type === "icon") return `icon:${icon.icon.name}`;
    return "none";
  }

  function commitActiveLabel(): void {
    if (!activeTab) return;
    const nextLabel = activeLabelDraft.trim();
    if (!nextLabel || nextLabel === tabLabel(activeTab, activeTabIndex)) return;
    void Promise.resolve(onUpdateTabLabel(activeTab.label.id, nextLabel));
  }

  function handleLabelKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter") {
      event.preventDefault();
      commitActiveLabel();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      activeLabelDraft = activeTab
        ? tabLabel(activeTab, activeTabIndex < 0 ? 0 : activeTabIndex)
        : "";
    }
  }

  function selectActiveIcon(optionId: string): void {
    if (!activeTab) return;
    const option = tabIconChoices.find((candidate) => candidate.id === optionId);
    if (!option) return;
    void Promise.resolve(onUpdateTabIcon(activeTab.label.id, option.icon));
  }

  function addTabAfterActive(): void {
    if (!canAddTab) return;
    void Promise.resolve(onAddTab(block.id, Math.max(0, activeTabIndex)));
  }

  function removeActiveTab(): void {
    if (!activeTab || !canRemoveTab) return;
    void Promise.resolve(onRemoveTab(block.id, activeTab.label.id));
  }

  function moveActiveTab(direction: NotesTabMoveDirection): void {
    if (!activeTab || !notesTabCanMove(tabItems, activeTab.label.id, direction)) return;
    void Promise.resolve(onMoveTab(block.id, activeTab.label.id, direction));
  }

  function handleTabTriggerDragOver(event: DragEvent): void {
    if (!draggingBlockId) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  }

  function handleTabTriggerDrop(event: DragEvent, labelBlockId: string): void {
    if (!draggingBlockId) return;
    event.preventDefault();
    event.stopPropagation();
    activeTabId = labelBlockId;
    void Promise.resolve(onMoveBlockToTab(draggingBlockId, labelBlockId));
  }

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

  let contextMenuRequest = $state<{ x: number; y: number; id: number } | null>(null);
  let contextMenuRequestId = 0;

  function openBlockContextMenu(event: MouseEvent): void {
    const point = notesBlockContextMenuPoint(event);
    if (point) contextMenuRequest = { ...point, id: ++contextMenuRequestId };
  }
</script>

<div
  id={notesBlockAnchorId(block.id)}
  class="notes-block-row notes-tab-row group relative"
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
      <section class="notes-tab-layout my-1 min-w-0" aria-label={t("notes.blockType.tab")}>
        {#if tabItems.length === 0}
          <button
            bind:this={focusButton}
            type="button"
            class="min-h-8 w-full rounded px-1 text-left text-[0.8rem] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onclick={() => {
              void Promise.resolve(onAddTab(block.id, 0));
            }}
            onkeydown={handleKeydown}
          >
            {t("notes.emptyTabs")}
          </button>
        {:else}
          <div class="notes-tab-list" role="tablist" aria-label={t("notes.blockType.tab")}>
            {#each tabItems as tab, tabIndex (tab.label.id)}
              {@const selected = activeTab?.label.id === tab.label.id}
              {@const tabIcon = tab.label.paragraph.icon ?? null}
              <button
                type="button"
                role="tab"
                aria-selected={selected}
                class="notes-tab-trigger"
                class:notes-tab-trigger-active={selected}
                aria-label={t("notes.openTabPanel", tabLabel(tab, tabIndex))}
                onclick={() => {
                  activeTabId = tab.label.id;
                  onFocusBlock(tab.label.id);
                }}
                ondragover={handleTabTriggerDragOver}
                ondrop={(event) => handleTabTriggerDrop(event, tab.label.id)}
              >
                {#if tabIcon}
                  <NotesPageIcon icon={tabIcon} size={15} strokeWidth={1.8} class="notes-tab-icon" />
                {/if}
                <span class="min-w-0 truncate">{tabLabel(tab, tabIndex)}</span>
              </button>
            {/each}
          </div>

          {#if activeTab}
            <div class="notes-tab-tools" aria-label={t("notes.tabActions", activeTabIndex + 1)}>
              <input
                class="notes-tab-label-input field"
                type="text"
                value={activeLabelDraft}
                aria-label={t("notes.renameTab", activeTabIndex + 1)}
                oninput={(event) => {
                  activeLabelDraft = event.currentTarget.value;
                }}
                onblur={commitActiveLabel}
                onkeydown={handleLabelKeydown}
              />
              <div class="notes-tab-icon-control">
                <Select
                  inline
                  appearance="quiet"
                  contentAlign="start"
                  class="w-full min-w-0"
                  ariaLabel={t("notes.tabIcon", activeTabIndex + 1)}
                  value={String(activeIconOptionId() ?? "")}
                  options={[...(tabIconChoices).map((option) => ({ value: String(option.id), label: String(option.id === "none" ? t("notes.noTabIcon") : option.label) }))]}
                  onChange={(nextValue) => selectActiveIcon(nextValue)}
                />
              </div>
              <button
                type="button"
                class="notes-tab-tool-button"
                aria-label={t("notes.moveTabLeft", activeTabIndex + 1)}
                disabled={!notesTabCanMove(tabItems, activeTab.label.id, "left")}
                onclick={() => moveActiveTab("left")}
              >
                <ArrowLeft size={14} strokeWidth={2} aria-hidden="true" />
              </button>
              <button
                type="button"
                class="notes-tab-tool-button"
                aria-label={t("notes.moveTabRight", activeTabIndex + 1)}
                disabled={!notesTabCanMove(tabItems, activeTab.label.id, "right")}
                onclick={() => moveActiveTab("right")}
              >
                <ArrowRight size={14} strokeWidth={2} aria-hidden="true" />
              </button>
              <button
                type="button"
                class="notes-tab-tool-button"
                aria-label={t("notes.addTabAfter", activeTabIndex + 1)}
                disabled={!canAddTab}
                onclick={addTabAfterActive}
              >
                <Plus size={14} strokeWidth={2} aria-hidden="true" />
              </button>
              <button
                type="button"
                class="notes-tab-tool-button"
                aria-label={t("notes.removeTab", activeTabIndex + 1)}
                disabled={!canRemoveTab}
                onclick={removeActiveTab}
              >
                <Trash2 size={14} strokeWidth={2} aria-hidden="true" />
              </button>
            </div>
            <div
              class="notes-tab-panel"
              role="tabpanel"
              aria-label={tabLabel(activeTab, tabItems.indexOf(activeTab))}
            >
              {#if activeTab.items.length === 0}
                <button
                  bind:this={focusButton}
                  type="button"
                  class="min-h-8 w-full rounded px-1 text-left text-[0.8rem] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  onkeydown={handleKeydown}
                >
                  {t("notes.emptyTabs")}
                </button>
              {:else}
                <div class="flex min-w-0 flex-col">
                  {#each activeTab.items as tabBlockItem (tabBlockItem.block.id)}
                    <NotesCalloutEmbeddedRow layers={calloutLayersByTab.get(activeTab.label.id)?.get(tabBlockItem.block.id) ?? []}>
                      {#snippet body()}
                        <NotesBlockRow
                          item={tabBlockItem}
                          {breadcrumbItems}
                          {tableOfContentsItems}
                          tableRows={tableRowsForBlock(tabBlockItem.block.id)}
                          previousBlockType={previousBlockTypeForBlock(tabBlockItem.block.id)}
                          isOnlyBlock={isOnlyBlockForBlock(tabBlockItem.block.id)}
                          {focusBlockId}
                          {focusRequestId}
                          {focusSelection}
                          {listOrdinals}
                          {mentionTargets}
                          templateStatus={templateStatusForBlock(tabBlockItem.block.id)}
                          buttonStatus={buttonStatusForBlock(tabBlockItem.block.id)}
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
                          moveTargets={moveTargetsForBlock(tabBlockItem.block.id)}
                          {onMoveToPage}
                          {onDelete}
                          isDragging={draggingBlockId === tabBlockItem.block.id}
                          dropPosition={dropPositionForBlock(tabBlockItem.block.id)}
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
            </div>
          {/if}
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

  .notes-tab-layout {
    container-type: inline-size;
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
    background: var(--primary);
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
    background: var(--primary);
    content: "";
  }

  .notes-block-drop-inside > .notes-block-surface {
    background: hsl(var(--primary) / 0.1);
    box-shadow: inset 0 0 0 2px hsl(var(--primary) / 0.65);
  }

  .notes-tab-list {
    display: flex;
    min-width: 0;
    gap: 0.25rem;
    overflow-x: auto;
    border-bottom: 1px solid var(--border);
  }

  .notes-tab-trigger {
    display: inline-flex;
    max-width: 12rem;
    min-height: 1.9rem;
    min-width: 0;
    flex-shrink: 0;
    align-items: center;
    gap: 0.25rem;
    border-bottom: 2px solid transparent;
    padding: 0 0.55rem;
    color: var(--muted-foreground);
    font-size: calc(0.8rem * var(--type-scale));
    font-weight: 500;
    outline: none;
  }

  .notes-tab-trigger:hover,
  .notes-tab-trigger:focus-visible {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .notes-tab-trigger-active {
    border-bottom-color: var(--primary);
    color: var(--foreground);
  }

  .notes-tab-trigger :global(.notes-tab-icon) {
    max-width: 3rem;
    flex-shrink: 0;
    overflow: hidden;
    color: inherit;
    font-size: calc(0.75rem * var(--type-scale));
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .notes-tab-tools {
    display: grid;
    grid-template-columns: minmax(8rem, 1fr) minmax(6rem, auto) repeat(4, 1.9rem);
    align-items: center;
    gap: 0.35rem;
    min-width: 0;
    padding: 0.4rem 0 0.2rem;
  }

  .notes-tab-label-input {
    min-width: 0;
    height: 1.9rem;
    min-height: 0;
    color: var(--foreground);
    font-size: calc(0.78rem * var(--type-scale));
  }

  .notes-tab-tool-button:focus-visible {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--ring) 65%, transparent);
  }

  .notes-tab-tool-button {
    display: inline-flex;
    width: 1.9rem;
    height: 1.9rem;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    color: var(--muted-foreground);
    outline: none;
  }

  .notes-tab-tool-button:hover:not(:disabled) {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .notes-tab-tool-button:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }

  .notes-tab-panel {
    min-width: 0;
    padding-top: 0.45rem;
  }

  @container (max-width: 28rem) {
    .notes-tab-tools {
      grid-template-columns: minmax(0, 1fr) minmax(5.5rem, auto) repeat(4, 1.75rem);
      gap: 0.25rem;
    }

    .notes-tab-trigger {
      max-width: 9rem;
      padding: 0 0.45rem;
    }

    .notes-tab-tool-button {
      width: 1.75rem;
      height: 1.75rem;
    }
  }

  @container (max-width: 22rem) {
    .notes-tab-tools {
      grid-template-columns: minmax(0, 1fr) repeat(4, 1.75rem);
    }

    .notes-tab-icon-control {
      grid-column: 1 / -1;
      width: 100%;
    }
  }
</style>
