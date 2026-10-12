<script lang="ts">
  import { tick, untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalTimezone } from "$lib/calendar/timezone-utils";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { buildNotesBlockLink, buildNotesPageLink } from "$lib/notes/links/block-link";
  import { normalizeNotesSelectableBlockIds } from "$lib/notes/blocks/selection";
  import { notesTemplateBlockStatus } from "$lib/notes/block-types/template";
  import { notesButtonBlockStatus } from "$lib/notes/block-types/button";
  import type { NotesUnsupportedConversionTarget } from "$lib/notes/block-types/unsupported";
  import {
    blockPlainText,
    isTextEditableBlock,
    type NotesHeadingBlockType,
  } from "$lib/notes/blocks/factory";
  import type { NotesBlockInsertRequest } from "$lib/notes/blocks/insertion";
  import {
    type NotesTextSelection,
  } from "$lib/notes/editor/selection";
  import type {
    NotesDateMentionTarget,
    NotesNamedMentionTarget,
    NotesObjectMentionTarget,
    NotesPageMentionTarget,
    NotesRichTextAnnotationPatch,
  } from "$lib/notes/rich-text/core";
  import type { NotesKeyboardAction } from "$lib/notes/blocks/keyboard";
  import {
    notesMoveToPageTargets,
    type NotesMoveToPageTarget,
  } from "$lib/notes/blocks/move";
  import type {
    NotesBlock,
    NotesBlockTreeItem,
    NotesBlockType,
    NotesButtonInsertPosition,
    NotesIcon,
    NotesPageBreadcrumbItem,
    NotesRichText,
    NotesTableOfContentsItem,
  } from "$lib/notes/types";
  import { isNotesPageBodyEmpty, notesNumberedListOrdinals } from "$lib/notes/blocks/editor-ui";
  import { notesCalloutLayers, notesCalloutIsOwnTextHidden } from "$lib/notes/block-types/callout-layout";
  import NotesBlockRow from "./NotesBlockRow.svelte";
  import type NotesSelectionContextMenu from "./NotesSelectionContextMenu.svelte";
  import type NotesDatabasePastePrompt from "$lib/components/notes/database/NotesDatabasePastePrompt.svelte";
  import { createNotesDocumentSelectionController } from "./document-selection-controller.svelte";
  import NotesVirtualBlock from "./NotesVirtualBlock.svelte";
  import NotesVisibleBlockRenderer from "./NotesVisibleBlockRenderer.svelte";
  import { createNotesBlockDragController } from "./block-drag-controller.svelte";
  import { createNotesBlockHandleController } from "./handle-controller.svelte";
  import { createNotesBlockVirtualizer } from "./virtualizer.svelte";
  import { createNotesBlockNavigationController } from "./navigation-controller";
  import {
    createNotesBlockSelectionController,
  } from "./selection-controller.svelte";
  import type {
    NotesBlockDragBindings,
    NotesBlockRenderActions,
    NotesBlockRenderLookups,
    NotesBlockRenderState,
    NotesColumnRenderActions,
    NotesTabRenderActions,
  } from "./render-contracts";
  import { createNotesStructuralBlockLoader } from "./structural-block-loader.svelte";
  import { buildNotesMentionTargets } from "./mention-targets";
  import { notesMentionScope } from "./mention-scope-controller.svelte";
  import { notesCalendarEventPreviewWhen, notesMentionScopeEventTimes } from "$lib/notes/rich-text/mention-preview";
  import { notesMentionData, type NotesMentionCatalog } from "./mention-data-controller.svelte";

  let {
    items,
    pageId,
    breadcrumbItems,
    tableOfContentsItems,
    onSelectPage,
    onFocusBlock,
    scrollViewport,
  }: {
    items: NotesBlockTreeItem[];
    pageId: string;
    breadcrumbItems: NotesPageBreadcrumbItem[];
    tableOfContentsItems: NotesTableOfContentsItem[];
    onSelectPage: (pageId: string) => void;
    onFocusBlock: (blockId: string, preventScroll?: boolean) => void;
    scrollViewport: HTMLDivElement | null;
  } = $props();

  const notes = getNotesEditor();
  const preferences = getPreferences();
  const projects = getProjects();
  const localization = getLocalization();
  const { t } = localization;
  let blockListElement: HTMLDivElement | null = $state(null);
  let DatabasePastePrompt = $state<typeof NotesDatabasePastePrompt | null>(null);
  let pastePromptLoading = $state(false);
  let pastePromptError = $state<string | null>(null);
  let pastePromptAnchor = $state<HTMLElement | null>(null);
  const databasePastePrompt = $derived(notes.databasePaste.prompt?.pageId === pageId ? notes.databasePaste.prompt : null);
  /** Loads the database paste prompt on first use. A retry rejects when the load fails again. */
  function loadPastePrompt(retry = false): Promise<void> {
    pastePromptLoading = true;
    return import("$lib/components/notes/database/NotesDatabasePastePrompt.svelte").then((module) => {
      DatabasePastePrompt = module.default;
      pastePromptError = null;
    }).catch((caught: unknown) => {
      pastePromptError = caught instanceof Error ? caught.message : String(caught);
      if (retry) throw caught;
    }).finally(() => { pastePromptLoading = false; });
  }
  $effect(() => {
    if (!databasePastePrompt || DatabasePastePrompt || pastePromptLoading || pastePromptError) return;
    void loadPastePrompt();
  });
  const blockHandle = createNotesBlockHandleController();
  const blockSelectionController = createNotesBlockSelectionController({
    undo: notes.undoNotesEdit,
    redo: notes.redoNotesEdit,
    readPageId: () => pageId,
    readListElement: () => blockListElement,
    readRenderedBlockIds: renderedSelectableBlockIds,
    readTreeState: currentTreeState,
    hydrateSubtrees: async (ids) => {
      const subtree = notes.outlineSubtreeIds(ids);
      await notes.hydrateBlockRange(subtree);
      return subtree;
    },
    blockIdFromEvent: (event) => navigation.blockIdFromEvent(event),
    targetIsEditable: (target) => navigation.targetIsEditable(target),
    targetIsSelectionZone: (target) => navigation.targetIsSelectionZone(target),
    focusTextEditorAtEnd: (id) => navigation.focusTextEditorAtEnd(id),
    focusRow: (id, preventScroll) => navigation.focusRow(id, preventScroll),
    handleNavigationKeydown: (event, id) => navigation.handleKeydown(event, id),
    pasteBlocks: notes.pasteBlockSelection,
    duplicateBlocks: notes.duplicateBlockSelection,
    moveBlocks: notes.moveBlockSelection,
    deleteBlocks: notes.deleteBlockSelection,
  });
  function isHiddenCalloutLabel(blockId: string): boolean {
    const block = notes.blockById(blockId);
    return !!block && notesCalloutIsOwnTextHidden(
      block,
      notes.childIdsByParentId[blockId]?.length ?? 0,
    );
  }
  const documentSelection = createNotesDocumentSelectionController({
    readIds: () => notes.flatBlockOutlines.map((item) => item.outline.id),
    readPageId: () => pageId,
    readBlock: notes.blockById,
    isHiddenCalloutLabel,
    outlineSubtreeIds: notes.outlineSubtreeIds,
    hydrate: notes.hydrateBlockRange,
    replace: notes.replaceDocumentRange,
    format: notes.formatDocumentRange,
    link: notes.linkDocumentRange,
    indent: notes.indentBlockSelection,
    focus: (point, preventScroll) => notes.focusBlock(point.blockId, { start: point.offset, end: point.offset }, preventScroll),
    restoreFocusAfterEdit: () => { if (notes.focusBlockId) notes.focusBlock(notes.focusBlockId, notes.focusSelection); },
    clearBlockSelection: () => blockSelectionController.setSelection(null),
    undo: notes.undoNotesEdit,
    redo: notes.redoNotesEdit,
  });
  const documentSelectionDelegation = documentSelection.delegation;
  let appliedSelectionRestore = untrack(() => notes.documentSelectionRestore);
  // Set selection ownership before child editors reconcile their focus requests.
  $effect.pre(() => {
    const restore = notes.documentSelectionRestore;
    const currentPageId = pageId;
    if (!restore || restore === appliedSelectionRestore) return;
    appliedSelectionRestore = restore;
    if (restore.pageId !== currentPageId) return;
    untrack(() => {
      const selection = restore.selection;
      if (selection) void documentSelection.run(() => documentSelection.select(selection));
      else documentSelection.clear();
    });
  });

  let blockSelectionMenu = $state<{ x: number; y: number } | null>(null);
  let SelectionContextMenu = $state<typeof NotesSelectionContextMenu | null>(null);
  let selectionMenuLoading = $state(false);
  let selectionMenuError = $state<string | null>(null);
  $effect(() => {
    if (!documentSelection.menu && !blockSelectionMenu) { selectionMenuError = null; return; }
    if (SelectionContextMenu || selectionMenuLoading || selectionMenuError) return;
    selectionMenuLoading = true;
    void import("./NotesSelectionContextMenu.svelte")
      .then((module) => { SelectionContextMenu = module.default; })
      .catch((reason: unknown) => { selectionMenuError = reason instanceof Error ? reason.message : String(reason); })
      .finally(() => { selectionMenuLoading = false; });
  });
  function selectionContextMenu(node: HTMLDivElement) {
    const open = (event: MouseEvent) => {
      if (!blockSelectionController.selection || event.defaultPrevented) return;
      event.preventDefault(); event.stopPropagation();
      blockSelectionMenu = { x: event.clientX, y: event.clientY };
    };
    node.addEventListener("contextmenu", open, true);
    return { destroy: () => node.removeEventListener("contextmenu", open, true) };
  }
  $effect(() => { void pageId; documentSelection.clear(); blockSelectionMenu = null; });
  const blockSelection = $derived(blockSelectionController.selection);
  const selectionClipboard = $derived(blockSelectionController.clipboard);
  const selectionActionError = $derived(blockSelectionController.error);
  const canMoveSelectionUp = $derived(blockSelectionController.canMoveUp);
  const canMoveSelectionDown = $derived(blockSelectionController.canMoveDown);
  const mentionTargets: NotesMentionCatalog = {
    read: buildMentionTargets,
    acquire: () => {
      notesMentionScope.request(pageId);
      return notesMentionData.acquire();
    },
  };
  const listOrdinals = $derived(notesNumberedListOrdinals(notes.flatBlockOutlines.map(({ outline }) => ({
    indent: outline.ganbaru_indent ?? 0,
    id: outline.id,
    type: outline.type,
    parentId: outline.parent.type === "page_id" ? outline.parent.page_id : outline.parent.block_id,
  }))));
  const renderState: NotesBlockRenderState = $derived({
    breadcrumbItems,
    tableOfContentsItems,
    listOrdinals,
    focusBlockId: notes.focusBlockId,
    focusRequestId: notes.focusRequestId,
    focusSelection: notes.focusSelection,
    mentionTargets,
  });
  const structuralBlockLoader = createNotesStructuralBlockLoader();
  const columnListLoadState = $derived(structuralBlockLoader.stateFor("column-list"));
  const tabLoadState = $derived(structuralBlockLoader.stateFor("tab"));
  const blockDrag = createNotesBlockDragController({
    readTreeState: currentTreeState,
    dropBlock: (sourceBlockId, targetBlockId, intent) => (
      notes.dropBlockOnBlock(sourceBlockId, targetBlockId, intent)
    ),
  });
  const virtualizer = createNotesBlockVirtualizer({
    readScrollViewport: () => scrollViewport,
    readListElement: () => blockListElement,
    readOutlines: () => notes.flatBlockOutlines,
    readItems: () => items,
    readPinnedBlockIds: () => [
      ...documentSelection.pinnedIds,
      blockDrag.draggingBlockId,
      blockHandle.openMenuBlockId,
      notes.focusBlockId,
      databasePastePrompt?.blockId ?? null,
      ...(blockSelection?.selectedBlockIds ?? []),
    ].filter((blockId): blockId is string => blockId !== null),
    readFocusRequest: () => ({
      blockId: notes.focusBlockId,
      requestId: notes.focusRequestId,
      preventScroll: notes.focusPreventScroll,
    }),
    hydrateBlockRange: notes.hydrateBlockRange,
  });
  const visibleRange = $derived(virtualizer.visibleRange);
  const visibleOutlines = $derived(virtualizer.visibleOutlines);
  const calloutLayersById = $derived(notesCalloutLayers(notes.flatBlockOutlines, notes.blockById));
  const hydratedItemsById = $derived(virtualizer.hydratedItemsById);
  const emptyPageBody = $derived(isNotesPageBodyEmpty(items));
  const measureVirtualBlock = virtualizer.measureBlock;
  $effect(() => {
    const blockId = databasePastePrompt?.blockId;
    const list = blockListElement;
    void visibleOutlines;
    void hydratedItemsById;
    if (!blockId || !list) {
      pastePromptAnchor = null;
      return;
    }
    const previousAnchor = untrack(() => pastePromptAnchor);
    if (!previousAnchor?.isConnected || previousAnchor.dataset.notesSelectableBlockId !== blockId) pastePromptAnchor = null;
    let cancelled = false;
    void tick().then(() => {
      if (!cancelled) {
        pastePromptAnchor = list.querySelector<HTMLElement>(`[data-notes-selectable-block-id="${blockId}"]`);
      }
    });
    return () => { cancelled = true; };
  });
  $effect(() => {
    void visibleOutlines;
    void hydratedItemsById;
    void tick().then(documentSelection.repaint);
  });

  $effect(() => {
    if (items.some((item) => item.block.type === "column_list")) {
      structuralBlockLoader.request("column-list");
    }
    if (items.some((item) => item.block.type === "tab")) structuralBlockLoader.request("tab");
  });

  $effect(() => {
    if (!projects.loaded && !projects.loading) {
      void projects.ensureLoaded()
        .catch((error) => console.warn("notes mention project targets failed", error));
    }
  });

  function buildMentionTargets(): NotesNamedMentionTarget[] {
    const scope = notesMentionScope.read(pageId);
    const renderZone = getLocalTimezone();
    const whenOptions = {
      locale: localization.locale,
      timeFormat: preferences.calendarTimeFormat,
      labels: {
        allDay: t("calendar.notification.allDay"),
        allDayOn: (date: string) => t("calendar.notification.allDayOn", date),
        datedTimeRange: (date: string, range: string) => t("calendar.notification.datedTimeRange", date, range),
      },
    };
    return buildNotesMentionTargets({
      pages: notes.allPages,
      localUser: notes.localUser,
      dataSources: notesMentionData.dataSources,
      dataSourceRowPages: notesMentionData.rowPages,
      folders: notes.folders,
      projectGroups: projects.groups,
      projects: projects.projects,
      tasks: scope.tasks,
      calendarEvents: scope.events,
      calendarEventWhen: (event) => notesCalendarEventPreviewWhen(notesMentionScopeEventTimes(event, renderZone), whenOptions),
      translate: t,
    });
  }

  $effect(() => {
    const _pageId = pageId;
    const _itemCount = items.length;
    const _selection = blockSelection;
    void tick().then(() => {
      blockSelectionController.pruneToRendered();
    });
  });

  function renderedSelectableBlockIds(): string[] {
    if (!blockListElement) return items.map((item) => item.block.id);
    return normalizeNotesSelectableBlockIds(
      Array.from(blockListElement.querySelectorAll<HTMLElement>("[data-notes-selectable-block-id]"))
        .map((element) => element.dataset.notesSelectableBlockId ?? ""),
    );
  }

  function currentTreeState() {
    return {
      blocksById: notes.blocksById,
      childIdsByParentId: notes.childIdsByParentId,
    };
  }

  function templateStatusForBlock(blockId: string) {
    return notesTemplateBlockStatus(currentTreeState(), blockId);
  }

  function buttonStatusForBlock(blockId: string) {
    return notesButtonBlockStatus(currentTreeState(), blockId);
  }

  const navigation = createNotesBlockNavigationController({
    readListElement: () => blockListElement,
    readRenderedBlockIds: renderedSelectableBlockIds,
    readBlock: notes.blockById,
    isHiddenCalloutLabel,
    requestFocus: notes.focusBlock,
    insertParagraphAdjacent: (id, direction) => {
      void blockSelectionController.run(() => notes.insertParagraphAdjacent(id, direction));
    },
  });
  $effect(() => { void pageId; navigation.resetVerticalGoal(); });

  const selectableBlockIdFromEvent = navigation.blockIdFromEvent;
  const eventTargetIsEditable = navigation.targetIsEditable;
  const focusSelectedBlockRow = navigation.focusRow;
  const focusTextEditorForBlock = navigation.focusTextEditorAtEnd;
  const handleDocumentNavigationKeydown = navigation.handleKeydown;

  function updateBlockHandleMenuOpen(blockId: string, open: boolean): void {
    blockHandle.setMenuOpen(blockId, open);
  }

  const blockSelectionDelegation = blockSelectionController.delegation;
  function handleKeyboardAction(blockId: string, action: NotesKeyboardAction): void {
    if (action.type === "create_sibling") {
      void notes.createSiblingAfter(blockId);
      return;
    }
    if (action.type === "split_text_block") {
      void splitTextBlockFromKeyboardAction(blockId, action);
      return;
    }
    if (action.type === "convert_to_paragraph") {
      void notes.convertBlock(blockId, "paragraph", true);
      return;
    }
    if (action.type === "remove_block_format") {
      void notes.convertBlock(blockId, "paragraph", false, { start: 0, end: 0 });
      return;
    }
    if (action.type === "apply_text_shortcut") {
      void notes.convertBlock(blockId, action.blockType, true);
      return;
    }
    if (action.type === "toggle_block_open") {
      const block = notes.blockById(blockId);
      if (block?.type === "toggle") {
        void notes.updateToggleOpen(blockId, block.toggle.ganbaru_open === false);
      }
      return;
    }
    if (action.type === "delete_block") {
      void notes.deleteBlock(blockId);
      return;
    }
    if (action.type === "merge_with_previous") {
      void notes.mergeBlockWithPrevious(blockId);
      return;
    }
    if (action.type === "nest") {
      void notes.nestBlock(blockId, action.selection);
      return;
    }
    if (action.type === "outdent") {
      void notes.outdentBlock(blockId, action.selection);
      return;
    }
    if (action.type === "move_up") {
      void notes.moveBlockUp(blockId);
      return;
    }
    if (action.type === "move_down") {
      void notes.moveBlockDown(blockId);
    }
  }

  async function splitTextBlockFromKeyboardAction(
    blockId: string,
    action: Extract<NotesKeyboardAction, { type: "split_text_block" }>,
  ): Promise<void> {
    await notes.updateBlockText(blockId, action.text);
    await notes.splitTextBlockAtSelection(blockId, action.selectionStart, action.selectionEnd);
  }

  function handleConvert(blockId: string, type: NotesBlockType, clearText = false): void {
    void notes.convertBlock(blockId, type, clearText)
      .catch((error: unknown) => console.warn("Notes block conversion failed", error));
  }

  function convertToToggleHeading(blockId: string, type: NotesHeadingBlockType, clearText = false): void {
    void notes.convertBlockToToggleHeading(blockId, type, clearText);
  }

  function moveTargetsForBlock(block: NotesBlock): NotesMoveToPageTarget[] {
    return notesMoveToPageTargets(notes.allPages, block, pageId, t("notes.untitled"), {
      recentPageIds: notes.recentPageIds,
      excludedPageIds: loadedChildPageIdsInSubtree(block.id),
    });
  }

  function moveTargetsForBlockId(blockId: string): NotesMoveToPageTarget[] {
    const block = notes.blockById(blockId);
    return block ? moveTargetsForBlock(block) : [];
  }

  function loadedChildPageIdsInSubtree(blockId: string): string[] {
    const result: string[] = [];
    const seen = new Set<string>();
    const queue = [blockId];
    while (queue.length > 0) {
      const currentBlockId = queue.shift();
      if (!currentBlockId || seen.has(currentBlockId)) continue;
      seen.add(currentBlockId);
      const block = notes.blockById(currentBlockId);
      if (block?.type === "child_page") result.push(block.id);
      queue.push(...(notes.childIdsByParentId[currentBlockId] ?? []));
    }
    return result;
  }

  async function copyBlockLink(blockId: string): Promise<void> {
    const link = buildNotesBlockLink(window.location.href, { pageId, blockId });
    await navigator.clipboard.writeText(link);
  }

  async function insertPageMention(
    blockId: string,
    start: number,
    end: number,
    target: NotesPageMentionTarget,
  ): Promise<void> {
    await notes.insertPageMention(
      blockId,
      start,
      end,
      target.id,
      target.title,
      buildNotesPageLink(window.location.href, { pageId: target.id }),
    );
  }

  async function insertDateMention(
    blockId: string,
    start: number,
    end: number,
    target: NotesDateMentionTarget,
  ): Promise<void> {
    await notes.insertDateMention(blockId, start, end, target.date, target.title);
  }

  async function insertObjectMention(
    blockId: string,
    start: number,
    end: number,
    target: NotesObjectMentionTarget,
  ): Promise<void> {
    await notes.insertObjectMention(blockId, start, end, target);
  }

  async function applyTextAnnotations(
    blockId: string,
    start: number,
    end: number,
    patch: NotesRichTextAnnotationPatch,
  ): Promise<void> {
    await notes.updateBlockTextAnnotations(blockId, start, end, patch);
  }

  async function applyTextLink(
    blockId: string,
    start: number,
    end: number,
    url: string | null,
  ): Promise<void> {
    await notes.updateBlockTextLink(blockId, start, end, url);
  }

  async function insertInlineEquation(
    blockId: string,
    start: number,
    end: number,
    expression: string,
  ): Promise<void> {
    await notes.insertInlineEquation(blockId, start, end, expression);
  }

  function pastePlainText(
    blockId: string,
    start: number,
    end: number,
    plainText: string,
  ): Promise<boolean> {
    return notes.pastePlainTextIntoBlock(blockId, start, end, plainText);
  }

  function pasteRichHtml(
    blockId: string,
    start: number,
    end: number,
    html: string,
  ): Promise<boolean> {
    return notes.pasteRichHtmlIntoBlock(blockId, start, end, html);
  }

  function replaceBlockRichText(
    blockId: string,
    richText: readonly NotesRichText[],
  ): void {
    void notes.updateBlockRichText(blockId, richText);
  }

  function replaceTableCellRichText(
    rowBlockId: string,
    columnIndex: number,
    richText: readonly NotesRichText[],
  ): void {
    void notes.updateTableCellRichText(rowBlockId, columnIndex, richText);
  }

  function addTableRow(tableBlockId: string, afterRowIndex: number): Promise<void> {
    return notes.addTableRow(tableBlockId, afterRowIndex);
  }

  function removeTableRow(tableBlockId: string, rowBlockId: string): Promise<void> {
    return notes.removeTableRow(tableBlockId, rowBlockId);
  }

  function addTableColumn(tableBlockId: string, afterColumnIndex: number): Promise<void> {
    return notes.addTableColumn(tableBlockId, afterColumnIndex);
  }

  function removeTableColumn(tableBlockId: string, columnIndex: number): Promise<void> {
    return notes.removeTableColumn(tableBlockId, columnIndex);
  }

  function addColumn(columnListBlockId: string, afterColumnIndex: number): Promise<void> {
    return notes.addColumn(columnListBlockId, afterColumnIndex);
  }

  function removeColumn(columnListBlockId: string, columnBlockId: string): Promise<void> {
    return notes.removeColumn(columnListBlockId, columnBlockId);
  }

  function moveColumn(
    columnListBlockId: string,
    columnBlockId: string,
    direction: "left" | "right",
  ): Promise<void> {
    return notes.moveColumn(columnListBlockId, columnBlockId, direction);
  }

  function resizeColumn(
    columnListBlockId: string,
    columnBlockId: string,
    widthRatio: number,
  ): Promise<void> {
    return notes.resizeColumn(columnListBlockId, columnBlockId, widthRatio);
  }

  function moveBlockToColumn(blockId: string, columnBlockId: string): Promise<void> {
    return notes.moveBlockToColumn(blockId, columnBlockId);
  }

  function updateTabLabel(labelBlockId: string, label: string): Promise<void> {
    return notes.updateTabLabel(labelBlockId, label);
  }

  function updateTabIcon(labelBlockId: string, icon: NotesIcon | null): Promise<void> {
    return notes.updateTabIcon(labelBlockId, icon);
  }

  function addTab(tabBlockId: string, afterTabIndex: number): Promise<void> {
    return notes.addTab(tabBlockId, afterTabIndex);
  }

  function removeTab(tabBlockId: string, labelBlockId: string): Promise<void> {
    return notes.removeTab(tabBlockId, labelBlockId);
  }

  function moveTab(
    tabBlockId: string,
    labelBlockId: string,
    direction: "left" | "right",
  ): Promise<void> {
    return notes.moveTab(tabBlockId, labelBlockId, direction);
  }

  function moveBlockToTab(blockId: string, labelBlockId: string): Promise<void> {
    return notes.moveBlockToTab(blockId, labelBlockId);
  }

  function undoNotesEdit(): void {
    void notes.undoNotesEdit();
  }

  function redoNotesEdit(): void {
    void notes.redoNotesEdit();
  }

  const draggingBlockId = $derived(blockDrag.draggingBlockId);
  const handleBlockDragStart = blockDrag.start;
  const handleBlockDragEnd = blockDrag.end;
  const handleBlockDragOver = blockDrag.over;
  const handleBlockDragLeave = blockDrag.leave;
  const handleBlockDrop = blockDrag.drop;
  const dropPositionForBlock = blockDrag.dropPositionForBlock;
  const renderActions: NotesBlockRenderActions = {
    onTextInput: (id, text, selection) => void notes.updateBlockText(id, text, selection),
    onReplaceRichText: replaceBlockRichText,
    onInsertPageMention: insertPageMention, onInsertDateMention: insertDateMention,
    onInsertObjectMention: insertObjectMention, onApplyTextLink: applyTextLink,
    onInsertInlineEquation: insertInlineEquation, onPastePlainText: pastePlainText,
    onPasteRichHtml: pasteRichHtml, onApplyTextAnnotations: applyTextAnnotations,
    onCreateInlineComment: notes.startInlineComment, onCreateInlineSuggestion: notes.startInlineSuggestion,
    onKeyboardAction: handleKeyboardAction, onUndo: undoNotesEdit, onRedo: redoNotesEdit,
    onAddBelow: (id, request) => void notes.createSiblingAfter(id, request), onConvert: handleConvert,
    onConvertToToggleHeading: convertToToggleHeading, onColorChange: (id, color) => void notes.updateBlockColor(id, color),
    onCalloutIconChange: (id, icon) => void notes.updateCalloutIcon(id, icon),
    onCopyLink: copyBlockLink, onDuplicate: (id) => void notes.duplicateBlock(id),
    onUseTemplate: (id) => void notes.useTemplateBlock(id), onAddTemplateChild: (id) => void notes.addTemplateChild(id),
    onUseButton: (id) => void notes.useButtonBlock(id), onAddButtonChild: (id) => void notes.addButtonChild(id),
    onButtonIconChange: (id, icon) => void notes.updateButtonIcon(id, icon),
    onButtonInsertPositionChange: (id, position) => void notes.updateButtonInsertPosition(id, position),
    onCreateLinkedDatabaseView: notes.createLinkedDatabaseViewAfter, onConvertUnsupported: notes.convertUnsupportedBlock,
    onComment: (id) => void notes.startBlockComment(id), onMoveUp: (id) => void notes.moveBlockUp(id),
    onMoveDown: (id) => void notes.moveBlockDown(id), onMoveToPage: (id, page) => void notes.moveBlockToPage(id, page),
    onDelete: (id) => void notes.deleteBlock(id), onToggleTodo: (id, checked) => void notes.toggleTodo(id, checked),
    onToggleOpen: (id, open) => void notes.updateToggleOpen(id, open),
    onCodeLanguageChange: (id, language) => void notes.updateCodeLanguage(id, language),
    onBookmarkChange: (id, url, caption) => void notes.updateBookmark(id, url, caption),
    onLinkPreviewUrlChange: (id, url) => void notes.updateLinkPreviewUrl(id, url),
    onEmbedUrlChange: (id, url) => void notes.updateEmbedUrl(id, url),
    onEquationExpressionChange: (id, expression) => void notes.updateEquationExpression(id, expression),
    onMediaChange: (id, url, caption, name, change) => void notes.updateMedia(id, url, caption, name, change),
    onTableCellRichTextChange: replaceTableCellRichText,
    onAddTableRow: addTableRow, onRemoveTableRow: removeTableRow,
    onAddTableColumn: addTableColumn, onRemoveTableColumn: removeTableColumn,
    onSelectPage: (id) => onSelectPage(id), onFocusBlock: (id, preventScroll) => onFocusBlock(id, preventScroll),
    onHandleMenuOpenChange: updateBlockHandleMenuOpen,
  };
  const renderLookups: NotesBlockRenderLookups = {
    tableRowsForBlock: notes.tableRowsForBlock, previousBlockTypeForBlock: notes.previousBlockType,
    isOnlyBlockForBlock: notes.isOnlyBlock, moveTargetsForBlock: moveTargetsForBlockId,
    templateStatusForBlock, buttonStatusForBlock,
  };
  const columnRenderActions: NotesColumnRenderActions = { onAddColumn: addColumn, onRemoveColumn: removeColumn, onMoveColumn: moveColumn, onResizeColumn: resizeColumn, onMoveBlockToColumn: moveBlockToColumn };
  const tabRenderActions: NotesTabRenderActions = { onUpdateTabLabel: updateTabLabel, onUpdateTabIcon: updateTabIcon, onAddTab: addTab, onRemoveTab: removeTab, onMoveTab: moveTab, onMoveBlockToTab: moveBlockToTab };
  const dragBindings: NotesBlockDragBindings = $derived({ draggingBlockId, dropPositionForBlock, onDragStart: handleBlockDragStart, onDragEnd: handleBlockDragEnd, onDragOver: handleBlockDragOver, onDragLeave: handleBlockDragLeave, onDrop: handleBlockDrop });
</script>

<div
  use:documentSelectionDelegation
  use:selectionContextMenu
  use:blockSelectionDelegation
  bind:this={blockListElement}
  onpointerdowncapture={navigation.resetVerticalGoal}
  onkeyupcapture={(event) => { if (event.key === "Shift") navigation.resetVerticalGoal(); }}
  class="notes-block-list relative flex min-w-0 flex-col pb-8"
  data-notes-empty-page={emptyPageBody || undefined}
  role="group"
  aria-label={t("notes.blockList")}
>
  {#if documentSelection.menu && SelectionContextMenu}
    <SelectionContextMenu position={documentSelection.menu} onClose={() => documentSelection.closeMenu()}
      actions={[
        { label: t("notes.copySelection"), run: () => void documentSelection.run(() => documentSelection.copy()) },
        { label: t("notes.cutSelection"), run: () => void documentSelection.run(() => documentSelection.copy(true)) },
        { label: t("notes.pasteSelection"), run: () => void documentSelection.run(documentSelection.paste) },
        { label: t("notes.deleteSelection"), run: () => void documentSelection.run(() => documentSelection.replace("")) },
        { label: t("notes.bold"), run: () => void documentSelection.run(() => documentSelection.format("bold")) },
        { label: t("notes.italic"), run: () => void documentSelection.run(() => documentSelection.format("italic")) },
        { label: t("notes.underline"), run: () => void documentSelection.run(() => documentSelection.format("underline")) },
      ]} />
  {:else if blockSelection && blockSelectionMenu && SelectionContextMenu}
    <SelectionContextMenu position={blockSelectionMenu} onClose={() => { blockSelectionMenu = null; if (blockSelection) navigation.focusRow(blockSelection.focusBlockId); }}
      actions={[
        { label: t("notes.copySelection"), run: () => void blockSelectionController.run(() => blockSelectionController.copy("copy")) },
        { label: t("notes.cutSelection"), run: () => void blockSelectionController.run(() => blockSelectionController.copy("cut")) },
        { label: t("notes.pasteSelection"), disabled: !selectionClipboard, run: () => void blockSelectionController.run(() => blockSelectionController.paste(blockSelection?.focusBlockId ?? notes.focusBlockId)) },
        { label: t("notes.duplicateSelection"), run: () => void blockSelectionController.run(blockSelectionController.duplicate) },
        { label: t("notes.moveSelectionUp"), disabled: !canMoveSelectionUp, run: () => void blockSelectionController.run(() => blockSelectionController.move("up")) },
        { label: t("notes.moveSelectionDown"), disabled: !canMoveSelectionDown, run: () => void blockSelectionController.run(() => blockSelectionController.move("down")) },
        { label: t("notes.deleteSelection"), run: () => void blockSelectionController.run(blockSelectionController.remove) },
      ]} />
  {/if}
  {#if documentSelection.error || selectionActionError || selectionMenuError}
    {@const selectionError = documentSelection.error ?? selectionActionError ?? selectionMenuError ?? ""}
    <Notice class="text-sm" role="status" message={`${t("notes.selectionActionFailed")} ${selectionError}`} copy={selectionError} />
  {/if}
  {#if visibleRange.topHeight > 0}
    <div aria-hidden="true" style:height={`${visibleRange.topHeight}px`}></div>
  {/if}
  {#each visibleOutlines as outlineItem (outlineItem.outline.id)}
    {@const item = hydratedItemsById.get(outlineItem.outline.id)}
    <NotesVirtualBlock
      {item}
      blockId={outlineItem.outline.id}
      retainedHeight={outlineItem.outline.retained_height}
      calloutLayers={calloutLayersById.get(outlineItem.outline.id) ?? []}
      measure={measureVirtualBlock}
    >
      {#snippet children(item)}
      <NotesVisibleBlockRenderer
        {item}
        state={renderState}
        actions={renderActions}
        drag={dragBindings}
        lookups={renderLookups}
        columnActions={columnRenderActions}
        tabActions={tabRenderActions}
        columnItems={notes.columnItemsForBlock(item.block.id)}
        tabItems={notes.tabItemsForBlock(item.block.id)}
        tableRows={notes.tableRowsForBlock(item.block.id)}
        previousBlockType={notes.previousBlockType(item.block.id)}
        isOnlyBlock={notes.isOnlyBlock(item.block.id)}
        moveTargets={moveTargetsForBlock(item.block)}
        templateStatus={templateStatusForBlock(item.block.id)}
        buttonStatus={buttonStatusForBlock(item.block.id)}
        columnComponent={columnListLoadState?.status === "ready" && columnListLoadState.component.kind === "column-list" ? columnListLoadState.component.component : null}
        tabComponent={tabLoadState?.status === "ready" && tabLoadState.component.kind === "tab" ? tabLoadState.component.component : null}
        columnFailed={columnListLoadState?.status === "failed"}
        tabFailed={tabLoadState?.status === "failed"}
        retryStructural={(kind) => structuralBlockLoader.request(kind, true)}
      />
      {/snippet}
    </NotesVirtualBlock>
  {/each}
  {#if visibleRange.bottomHeight > 0}
    <div aria-hidden="true" style:height={`${visibleRange.bottomHeight}px`}></div>
  {/if}
</div>

{#if databasePastePrompt && DatabasePastePrompt}
  {#key databasePastePrompt.blockId}
    <DatabasePastePrompt controller={notes.databasePaste} anchor={pastePromptAnchor} />
  {/key}
{:else if databasePastePrompt && pastePromptError}
  <Notice layout="floating" class="text-panel" message={t("notes.databasePasteFailed", pastePromptError)} copy={pastePromptError} onRetry={() => loadPastePrompt(true)} onDismiss={() => notes.databasePaste.dismiss()} />
{/if}

<style>
  :global(.notes-block-list[data-notes-document-selection]:not([data-notes-document-selection-composing]) [contenteditable='true'][data-notes-block-id]) {
    caret-color: transparent;
  }

  :global(.notes-block-row[data-notes-block-selected] > .notes-block-surface) {
    user-select: none;
    background: var(--selection-background);
    box-shadow: inset 0 0 0 1px var(--primary);
  }

  :global(.notes-block-row[data-notes-block-selected]:focus-visible > .notes-block-surface) {
    outline: 2px solid var(--ring);
    outline-offset: 1px;
  }

</style>
