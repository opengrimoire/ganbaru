<script lang="ts">
  import NotesLoadingSkeleton from "$lib/components/notes/NotesLoadingSkeleton.svelte";
  import type { NotesMentionCatalog } from "./mention-data-controller.svelte";
  import { tick } from "svelte";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import type NotesCalloutIconPicker from "./NotesCalloutIconPicker.svelte";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import {
    notesCommentAnchorsForBlock,
    unreadNotesCommentThreadCount,
  } from "$lib/notes/collaboration/comments";
  import { filterNotesDiscussions } from "$lib/notes/collaboration/discussions";
  import {
    notesSuggestionAnchorsForBlock,
  } from "$lib/notes/collaboration/suggestions";
  import type { NotesUnsupportedConversionTarget } from "$lib/notes/block-types/unsupported";
  import {
    blockPlainText,
    headingIsToggleable,
    headingToggleOpen,
    isHeadingBlockType,
    isTextEditableBlock,
    type NotesHeadingBlockType,
  } from "$lib/notes/blocks/factory";
  import { notesBlockAnchorId } from "$lib/notes/links/block-link";
  import { notesBlockContextMenuPoint } from "$lib/notes/blocks/handle";
  import { notesSyncedBlockStatus } from "$lib/notes/block-types/synced";
  import type { NotesTemplateBlockStatus } from "$lib/notes/block-types/template";
  import type { NotesButtonBlockStatus } from "$lib/notes/block-types/button";
  import {
    type NotesDateMentionTarget,
    type NotesObjectMentionTarget,
    type NotesRichTextAnnotationPatch,
    type NotesPageMentionTarget,
  } from "$lib/notes/rich-text/core";
  import type { NotesMoveToPageTarget } from "$lib/notes/blocks/move";
  import type { NotesTextSelection } from "$lib/notes/editor/selection";
  import {
    blockColor,
    canBlockHaveColor,
    notesBlockColorStyle,
  } from "$lib/notes/blocks/color";
  import type { NotesBlockInsertRequest } from "$lib/notes/blocks/insertion";
  import { notesBlockMarker } from "$lib/notes/blocks/editor-ui";
  import { notesCalloutIsOwnTextHidden } from "$lib/notes/block-types/callout-layout";
  import {
    planNotesKeyboardAction,
    type NotesKeyboardAction,
  } from "$lib/notes/blocks/keyboard";
  import type { NotesMediaAssetChange } from "$lib/notes/blocks/factory";
  import { notesUndoShortcutAction } from "$lib/notes/history/undo-history";
  import type { NotesSlashAction, NotesSlashCommand } from "$lib/notes/editor/slash-commands";
  import type { NotesBlockDropIndicator } from "$lib/notes/blocks/drag";
  import type {
    NotesBlockTreeItem,
    NotesBlockType,
    NotesButtonInsertPosition,
    NotesColor,
    NotesIcon,
    NotesPageBreadcrumbItem,
    NotesRichText,
    NotesTableRowBlock,
    NotesTableOfContentsItem,
  } from "$lib/notes/types";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import FileText from "@lucide/svelte/icons/file-text";
  import SmilePlus from "@lucide/svelte/icons/smile-plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import NotesBlockHandle from "./NotesBlockHandle.svelte";
  import NotesTextBlockEditor from "./NotesTextBlockEditor.svelte";
  import {
    loadNotesAdvancedBlock,
    readNotesAdvancedBlock,
    loadNotesTextControl,
    notesBlockRenderFamily,
    retryNotesAdvancedBlock,
    retryNotesTextControl,
    type LoadedNotesAdvancedBlock,
    type LoadedNotesTextControl,
    type NotesAdvancedBlockFamily,
  } from "$lib/components/notes/editor-component-registry";

  let {
    item,
    breadcrumbItems,
    tableOfContentsItems,
    tableRows,
    previousBlockType,
    isOnlyBlock,
    focusBlockId,
    focusRequestId,
    focusSelection,
    listOrdinals = new Map<string, number>(),
    mentionTargets,
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
    onMoveToPage,
    onDelete,
    isDragging,
    dropPosition,
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
    onSelectPage,
    onFocusBlock,
    onHandleMenuOpenChange,
  }: {
    item: NotesBlockTreeItem;
    breadcrumbItems: NotesPageBreadcrumbItem[];
    tableOfContentsItems: NotesTableOfContentsItem[];
    tableRows: NotesTableRowBlock[];
    previousBlockType: NotesBlockType | null;
    isOnlyBlock: boolean;
    focusBlockId: string | null;
    focusRequestId: number;
    focusSelection: NotesTextSelection | null;
    listOrdinals?: ReadonlyMap<string, number>;
    mentionTargets: NotesMentionCatalog;
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
    onMoveToPage: (blockId: string, pageId: string) => void;
    onDelete: (blockId: string) => void;
    isDragging: boolean;
    dropPosition: NotesBlockDropIndicator | null;
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
    onSelectPage: (pageId: string) => void;
    onFocusBlock: (blockId: string, preventScroll?: boolean) => void;
    onHandleMenuOpenChange: (blockId: string, open: boolean) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const notes = getNotesEditor();

  function breadcrumbStatusLabel(crumb: NotesPageBreadcrumbItem): string | null {
    switch (crumb.status) {
      case "archived":
        return t("notes.breadcrumbArchived");
      case "trashed":
        return t("notes.breadcrumbTrashed");
      case "missing":
        return t("notes.breadcrumbMissing");
      case "workspace":
      case "active":
        return null;
    }
  }

  function breadcrumbCanNavigate(crumb: NotesPageBreadcrumbItem): boolean {
    return !!crumb.id && !crumb.current && crumb.status === "active";
  }

  let dividerButton: HTMLButtonElement | null = $state(null);
  let childPageButton: HTMLButtonElement | null = $state(null);
  let breadcrumbButton: HTMLButtonElement | null = $state(null);
  let tableOfContentsButton: HTMLButtonElement | null = $state(null);
  let syncedBlockButton: HTMLButtonElement | null = $state(null);
  let slashOpen = $state(false);
  let advancedBlockLoadState = $state<LazyComponentLoadState<
    NotesAdvancedBlockFamily,
    LoadedNotesAdvancedBlock
  > | null>(null);
  let readyDatabaseIdentity = $state<string | null>(null);
  let slashMenuLoadState = $state<LazyComponentLoadState<
    "slash-menu",
    LoadedNotesTextControl
  > | null>(null);
  const block = $derived(item.block);
  const text = $derived(blockPlainText(block));
  const marker = $derived(notesBlockMarker(
    block.type,
    listOrdinals.get(block.id) ?? 1,
    localization.locale,
  ));
  const blockCommentThreads = $derived(
    notes.commentThreads.filter(
      (thread) => thread.parent.type === "block_id" && thread.parent.block_id === block.id,
    ),
  );
  const blockCommentCount = $derived(blockCommentThreads.length);
  const blockUnreadCommentCount = $derived(unreadNotesCommentThreadCount(blockCommentThreads));

  /** Open the page's discussions on the block's most recently active discussion, or start one when none is left. */
  function openBlockDiscussion(): void {
    const latest = filterNotesDiscussions(blockCommentThreads, "all")[0];
    if (latest) notes.requestDiscussion(latest.id);
    else onComment(block.id);
  }
  const commentAnchors = $derived(notesCommentAnchorsForBlock(notes.commentThreads, block.id, text));
  const suggestionAnchors = $derived(notesSuggestionAnchorsForBlock(notes.suggestions, block.id, text));
  const calloutContainerOnly = $derived(notesCalloutIsOwnTextHidden(
    block,
    notes.childIdsByParentId[block.id]?.length ?? 0,
  ));
  const firstCalloutChild = $derived.by(() => {
    if (block.type !== "callout") return undefined;
    const firstChildId = notes.childIdsByParentId[block.id]?.[0];
    return firstChildId ? notes.blockById(firstChildId) : undefined;
  });
  const firstCalloutChildHeadingClass = $derived(
    firstCalloutChild && isHeadingBlockType(firstCalloutChild.type)
      ? `notes-${firstCalloutChild.type.replaceAll("_", "-")}`
      : "",
  );
  const showTextEditor = $derived(isTextEditableBlock(block.type) && !calloutContainerOnly);
  const currentColor = $derived(blockColor(block));
  const calloutContext = $derived.by(() => {
    let current: NotesBlockTreeItem["block"] | undefined = block;
    const visited = new Set<string>();
    let target: { id: string; color: NotesColor } | null = null;
    let count = 0;
    while (current && !visited.has(current.id)) {
      visited.add(current.id);
      if (current.type === "callout") {
        target ??= { id: current.id, color: blockColor(current) };
        count += 1;
      }
      current = current.parent.type === "block_id" ? notes.blockById(current.parent.block_id) : undefined;
    }
    return { target, nestedLayerCount: Math.max(0, count - 1) };
  });
  const calloutColorTarget = $derived(calloutContext.target);
  const visualDepth = $derived(Math.max(0, item.depth - calloutContext.nestedLayerCount));
  const directCalloutChild = $derived(
    block.parent.type === "block_id"
    && notes.blockById(block.parent.block_id)?.type === "callout",
  );
  const blockSupportsColor = $derived(canBlockHaveColor(block.type));
  const blockSurfaceStyle = $derived(notesBlockColorStyle(block.type === "callout" ? "default" : currentColor));
  let CalloutIconPicker = $state<typeof NotesCalloutIconPicker | null>(null);
  let calloutIconPickerRequested = $state(false);

  function openCalloutIconPicker(): void {
    calloutIconPickerRequested = true;
    if (CalloutIconPicker) return;
    void import("./NotesCalloutIconPicker.svelte")
      .then((module) => { CalloutIconPicker = module.default; })
      .catch((error: unknown) => console.error("load Notes callout icon picker failed", error));
  }
  const isToggleOpen = $derived(block.type !== "toggle" || block.toggle.ganbaru_open !== false);
  const headingToggleable = $derived(isHeadingBlockType(block.type) && headingIsToggleable(block));
  const headingOpen = $derived(!isHeadingBlockType(block.type) || headingToggleOpen(block));
  const childPageTitle = $derived(
    block.type === "child_page" ? block.child_page.title.trim() : "",
  );
  const syncedBlockSourceId = $derived(
    block.type === "synced_block" ? block.synced_block.synced_from?.block_id ?? null : null,
  );
  const syncedBlockStatus = $derived(
    block.type === "synced_block" ? notesSyncedBlockStatus(block.synced_block) : null,
  );
  const advancedBlockFamily = $derived.by((): NotesAdvancedBlockFamily | null => {
    const family = notesBlockRenderFamily(block.type);
    return family === "eager" || family === "column-list" || family === "tab" ? null : family;
  });

  function requestAdvancedBlock(retry = false): void {
    const family = advancedBlockFamily;
    if (!family || (!retry && advancedBlockLoadState?.key === family)) return;
    const component = readNotesAdvancedBlock(family);
    if (component) {
      advancedBlockLoadState = { key: family, status: "ready", requestId: (advancedBlockLoadState?.requestId ?? 0) + 1, component };
      return;
    }
    const loadingState = beginLazyComponentLoad(advancedBlockLoadState, family);
    advancedBlockLoadState = loadingState;
    const request = retry
      ? retryNotesAdvancedBlock(family)
      : loadNotesAdvancedBlock(family);
    void request.then((component) => {
      if (!advancedBlockLoadState) return;
      advancedBlockLoadState = resolveLazyComponentLoad(
        advancedBlockLoadState,
        family,
        loadingState.requestId,
        component,
      );
    }).catch((error: unknown) => {
      if (!advancedBlockLoadState) return;
      advancedBlockLoadState = rejectLazyComponentLoad(
        advancedBlockLoadState,
        family,
        loadingState.requestId,
        error,
      );
      console.error(`load Notes ${family} block failed`, error);
    });
  }

  function requestSlashMenu(retry = false): void {
    if (!retry && slashMenuLoadState?.key === "slash-menu") return;
    const loadingState = beginLazyComponentLoad(slashMenuLoadState, "slash-menu");
    slashMenuLoadState = loadingState;
    const request = retry
      ? retryNotesTextControl("slash-menu")
      : loadNotesTextControl("slash-menu");
    void request.then((component) => {
      if (!slashMenuLoadState) return;
      slashMenuLoadState = resolveLazyComponentLoad(
        slashMenuLoadState,
        "slash-menu",
        loadingState.requestId,
        component,
      );
    }).catch((error: unknown) => {
      if (!slashMenuLoadState) return;
      slashMenuLoadState = rejectLazyComponentLoad(
        slashMenuLoadState,
        "slash-menu",
        loadingState.requestId,
        error,
      );
      console.error("load Notes slash menu failed", error);
    });
  }

  $effect(() => {
    advancedBlockFamily;
    requestAdvancedBlock();
    if (slashOpen) requestSlashMenu();
  });

  $effect(() => {
    const _focusRequestId = focusRequestId;
    if (focusBlockId !== block.id) return;
    if (showTextEditor) return;
    void tick().then(() => {
      if (focusBlockId !== block.id || _focusRequestId !== focusRequestId) return;
      focusControl(childPageButton);
      focusControl(syncedBlockButton);
      focusControl(dividerButton);
      focusControl(breadcrumbButton);
      focusControl(tableOfContentsButton);
    });
  });

  function focusControl(control: HTMLElement | null): void {
    control?.focus({ preventScroll: true });
  }

  function toggleButtonLabel(): string {
    if (block.type === "toggle") return isToggleOpen ? t("notes.closeToggle") : t("notes.openToggle");
    if (headingToggleable) return headingOpen ? t("notes.closeToggle") : t("notes.openToggle");
    return t("notes.toggleBlock");
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (handleUndoRedoKeydown(event)) return;
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter" && headingToggleable) {
      event.preventDefault();
      onToggleOpen(block.id, !headingOpen);
      return;
    }
    const target = event.currentTarget;
    const input = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement
      ? target
      : null;
    const action = planNotesKeyboardAction({
      key: event.key,
      shiftKey: event.shiftKey,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      altKey: event.altKey,
      text,
      selectionStart: input?.selectionStart ?? 0,
      selectionEnd: input?.selectionEnd ?? 0,
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

  function handleUndoRedoKeydown(event: KeyboardEvent): boolean {
    const undoAction = notesUndoShortcutAction(event);
    if (!undoAction) return false;
    event.preventDefault();
    void Promise.resolve(undoAction === "undo" ? onUndo() : onRedo());
    return true;
  }

  function handleChildPageKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter" || event.key === " ") return;
    handleKeydown(event);
  }

  function clearSlashText(): void {
    if (text.startsWith("/")) onTextInput(block.id, "", { start: 0, end: 0 });
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
        if (command.action !== "delete") clearSlashText();
        runSlashAction(command.action);
        return;
      case "color":
        if (!blockSupportsColor) return;
        clearSlashText();
        changeBlockColor(command.color);
        return;
    }
  }

  function changeBlockColor(color: NotesColor): void {
    const targetId = color.endsWith("_background") ? calloutColorTarget?.id : null;
    onColorChange(targetId ?? block.id, color);
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
  class={`notes-block-row group relative ${firstCalloutChildHeadingClass}`}
  role="group"
  tabindex="-1"
  data-notes-selectable-block-id={block.id}
  class:notes-block-focused={focusBlockId === block.id}
  class:notes-block-dragging={isDragging}
  class:notes-block-drop-before={dropPosition === "before"}
  class:notes-block-drop-after={dropPosition === "after"}
  class:notes-block-drop-inside={dropPosition === "inside"}
  class:notes-block-drop-outdent={dropPosition === "outdent"}
  class:notes-callout-container-only={calloutContainerOnly}
  style={`--notes-depth: ${visualDepth}`}
  ondragover={(event) => onDragOver(block.id, event)}
  ondragleave={(event) => onDragLeave(block.id, event)}
  ondrop={(event) => onDrop(block.id, event)}
  oncontextmenu={openBlockContextMenu}
>
  <div
    class={`notes-block-surface flex min-w-0 items-start gap-1 rounded-md py-0.5 pr-2 ${block.type === "child_database" ? "" : "hover:bg-accent/50"}`}
    class:notes-callout-surface={block.type === "callout"}
    class:notes-callout-direct-child={directCalloutChild && block.type !== "callout"}
    data-notes-block-selection-zone={showTextEditor || block.type === "child_database" ? undefined : ""}
    style={blockSurfaceStyle}
  >
    {#if visualDepth > 0}
      <div class="notes-block-indent shrink-0"></div>
    {/if}
    <NotesBlockHandle
      {contextMenuRequest}
      onTurnInto={openTurnIntoMenu}
      canTurnInto={block.type !== "child_database"}
      canSetColor={blockSupportsColor}
      currentColor={currentColor}
      currentBackgroundColor={calloutColorTarget?.color ?? currentColor}
      backgroundOnly={block.type === "callout"}
      onColorSelect={changeBlockColor}
      onCopyLink={() => onCopyLink(block.id)}
      onDuplicate={() => onDuplicate(block.id)}
      onComment={() => onComment(block.id)}
      onOpenComments={openBlockDiscussion}
      commentCount={blockCommentCount}
      unreadCommentCount={blockUnreadCommentCount}
      onMoveUp={() => onMoveUp(block.id)}
      onMoveDown={() => onMoveDown(block.id)}
      {moveTargets}
      onMoveToPage={(pageId) => onMoveToPage(block.id, pageId)}
      onDelete={() => onDelete(block.id)}
      onMenuOpenChange={(open) => onHandleMenuOpenChange(block.id, open)}
    />
    {#if block.type === "to_do"}
      <input
        class="mt-2 size-4 shrink-0 accent-primary"
        type="checkbox"
        checked={block.to_do.checked}
        aria-label={t("notes.todoChecked")}
        onchange={(event) => {
          const target = event.currentTarget;
          onToggleTodo(block.id, target.checked);
        }}
      />
    {:else if block.type === "toggle"}
      <button
        class="mt-1.5 flex size-5 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-label={toggleButtonLabel()}
        aria-expanded={isToggleOpen}
        onclick={() => {
          onToggleOpen(block.id, !isToggleOpen);
        }}
      >
        {#if isToggleOpen}
          <ChevronDown class="size-4" />
        {:else}
          <ChevronRight class="size-4" />
        {/if}
      </button>
    {:else if headingToggleable}
      <button
        class="mt-1.5 flex size-5 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-label={toggleButtonLabel()}
        aria-expanded={headingOpen}
        onclick={() => {
          onToggleOpen(block.id, !headingOpen);
        }}
      >
        {#if headingOpen}
          <ChevronDown class="size-4" />
        {:else}
          <ChevronRight class="size-4" />
        {/if}
      </button>
    {:else if block.type === "callout"}
      {#if CalloutIconPicker}
        <CalloutIconPicker
          icon={block.callout.icon}
          initiallyOpen={calloutIconPickerRequested}
          onChange={(icon) => onCalloutIconChange(block.id, icon)}
        />
      {:else}
        <button
          class="notes-callout-icon flex size-6 shrink-0 items-center justify-center rounded hover:bg-foreground/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          type="button"
          aria-label={t("notes.changeCalloutIcon")}
          onclick={openCalloutIconPicker}
        >
          {#if block.callout.icon}
            <NotesPageIcon icon={block.callout.icon} size={16} class="notes-callout-icon-glyph" />
          {:else}
            <SmilePlus class="notes-callout-icon-glyph size-4 text-muted-foreground" />
          {/if}
        </button>
      {/if}
    {:else if marker}
      <div
        class="notes-editor-body-text min-w-5 shrink-0 select-none whitespace-nowrap py-1 text-right leading-normal text-muted-foreground"
      >
        {marker}
      </div>
    {/if}

    <div class="relative min-w-0 flex-1">
      {#if block.type === "divider"}
        <button
          bind:this={dividerButton}
          class="my-2 h-5 w-full rounded-sm px-1 focus-visible:outline-none"
          aria-label={t("notes.blockType.divider")}
          onkeydown={handleKeydown}
          onclick={() => onConvert(block.id, "paragraph", true)}
        >
          <span class="block border-t border-border"></span>
        </button>
      {:else if block.type === "child_page"}
        <button
          bind:this={childPageButton}
          data-notes-atomic-block={block.id}
          type="button"
          class="my-1 flex min-h-9 w-full min-w-0 items-center gap-2 rounded-md px-1 text-left text-[0.933333rem] font-medium text-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
          aria-label={t("notes.openChildPage", childPageTitle || t("notes.untitled"))}
          onkeydown={handleChildPageKeydown}
          onclick={() => onSelectPage(block.id)}
        >
          <FileText class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span class="min-w-0 truncate">{childPageTitle || t("notes.untitled")}</span>
        </button>
      {:else if block.type === "child_database"}
        {@const databaseIdentity = JSON.stringify([block.id, block.child_database.database_id, block.child_database.data_source_id])}
        {@const databaseCreationPending = notes.isDatabaseCreationPending(block.id)}
        <NotesLoadingSkeleton kind="database" ready={!databaseCreationPending && (readyDatabaseIdentity === databaseIdentity || (advancedBlockLoadState?.key === "child-database" && advancedBlockLoadState.status === "failed"))}>
          {#snippet children()}
            {#key databaseIdentity}
              {#if !databaseCreationPending && advancedBlockLoadState?.status === "ready" && advancedBlockLoadState.component.kind === "child-database"}
                {@const NotesChildDatabaseBlock = advancedBlockLoadState.component.component}
                <NotesChildDatabaseBlock
                  onReady={() => { readyDatabaseIdentity = databaseIdentity; }}
                  onTitleSaved={(databaseId, title) => notes.reconcileDatabaseTitle(block.id, databaseId, title)}
                  onOpenDatabase={notes.openDatabase}
                  onDeleteDatabase={notes.deleteBlock}
                  {block}
                  {focusBlockId}
                  {focusRequestId}
                  onFocusBlock={onFocusBlock}
                  onKeydown={handleKeydown}
                  {onSelectPage}
                  {onCreateLinkedDatabaseView}
                />
              {/if}
            {/key}
          {/snippet}
        </NotesLoadingSkeleton>
      {:else if block.type === "breadcrumb"}
        <nav
          class="my-1 flex min-h-8 min-w-0 items-center gap-1 rounded-md px-1 text-[0.8rem] text-muted-foreground"
          aria-label={t("notes.blockType.breadcrumb")}
        >
          {#each breadcrumbItems as crumb, index}
            {@const statusLabel = breadcrumbStatusLabel(crumb)}
            {#if index > 0}
              <ChevronRight class="size-3.5 shrink-0" aria-hidden="true" />
            {/if}
            {#if breadcrumbCanNavigate(crumb)}
              <button
                type="button"
                class="min-w-0 truncate rounded px-1 py-0.5 text-left hover:bg-accent hover:text-foreground"
                aria-label={t("notes.openBreadcrumbPage", crumb.title)}
                onclick={() => {
                  if (crumb.id) onSelectPage(crumb.id);
                }}
              >
                {crumb.title}
              </button>
            {:else if crumb.current}
              <button
                bind:this={breadcrumbButton}
                type="button"
                class="min-w-0 truncate rounded px-1 py-0.5 text-left text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
                aria-current="page"
                onkeydown={handleKeydown}
              >
                {crumb.title}
              </button>
            {:else}
              <span
                class="inline-flex min-w-0 items-center gap-1 truncate px-1 py-0.5"
                aria-label={statusLabel ? t("notes.breadcrumbUnavailable", crumb.title, statusLabel) : undefined}
              >
                <span class="min-w-0 truncate">{crumb.title}</span>
                {#if statusLabel}
                  <span class="shrink-0 text-[0.7rem] text-muted-foreground">
                    {statusLabel}
                  </span>
                {/if}
              </span>
            {/if}
          {/each}
        </nav>
      {:else if block.type === "table_of_contents"}
        <nav
          class="my-1 min-w-0 rounded-md px-1 py-1 text-[0.866667rem]"
          aria-label={t("notes.blockType.tableOfContents")}
        >
          {#if tableOfContentsItems.length === 0}
            <button
              bind:this={tableOfContentsButton}
              type="button"
              class="min-h-8 w-full rounded px-1 text-left text-[0.8rem] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onkeydown={handleKeydown}
            >
              {t("notes.noHeadingsForTableOfContents")}
            </button>
          {:else}
            <ol class="flex min-w-0 flex-col gap-0.5">
              {#each tableOfContentsItems as heading, index}
                <li
                  class="notes-toc-item min-w-0"
                  style={`--notes-toc-level: ${heading.level}`}
                >
                  {#if index === 0}
                    <button
                      bind:this={tableOfContentsButton}
                      type="button"
                      class="min-h-7 w-full truncate rounded px-1 text-left text-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
                      aria-label={t("notes.openTableOfContentsHeading", heading.title)}
                      onclick={() => onFocusBlock(heading.blockId)}
                    >
                      {heading.title}
                    </button>
                  {:else}
                    <button
                      type="button"
                      class="min-h-7 w-full truncate rounded px-1 text-left text-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
                      aria-label={t("notes.openTableOfContentsHeading", heading.title)}
                      onclick={() => onFocusBlock(heading.blockId)}
                    >
                      {heading.title}
                    </button>
                  {/if}
                </li>
              {/each}
            </ol>
          {/if}
        </nav>
      {:else if block.type === "table"}
        {#if advancedBlockLoadState?.status === "ready" && advancedBlockLoadState.component.kind === "table"}
          {@const NotesTableBlock = advancedBlockLoadState.component.component}
          <NotesTableBlock
          {block}
          {tableRows}
          {previousBlockType}
          {isOnlyBlock}
          {focusBlockId}
          {focusRequestId}
          {onKeyboardAction}
          {onUndo}
          {onRedo}
          {onTableCellRichTextChange}
          {onAddTableRow}
          {onRemoveTableRow}
          {onAddTableColumn}
          {onRemoveTableColumn}
          />
        {/if}
      {:else if block.type === "image" || block.type === "video" || block.type === "audio" || block.type === "file" || block.type === "pdf"}
        {#if advancedBlockLoadState?.status === "ready" && advancedBlockLoadState.component.kind === "media"}
          {@const NotesMediaBlock = advancedBlockLoadState.component.component}
          <NotesMediaBlock
          {block}
          {previousBlockType}
          {isOnlyBlock}
          {focusBlockId}
          {focusRequestId}
          {onKeyboardAction}
          {onUndo}
          {onRedo}
          {onMediaChange}
          />
        {/if}
      {:else if block.type === "synced_block" && syncedBlockStatus}
        <section
          class="my-1 flex min-w-0 items-start gap-2 rounded-md border border-dashed border-border bg-muted/30 p-2"
          aria-label={t("notes.blockType.syncedBlock")}
        >
          <div
            class="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"
            aria-hidden="true"
          >
            <RefreshCw class="size-4" />
          </div>
          <button
            bind:this={syncedBlockButton}
            type="button"
            class="flex min-h-8 min-w-0 flex-1 flex-col gap-0.5 rounded-sm text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onkeydown={handleKeydown}
            onclick={() => onFocusBlock(block.id)}
          >
            <span class="text-[0.866667rem] font-medium text-foreground">
              {#if syncedBlockStatus.role === "original"}
                {t("notes.syncedBlockOriginal")}
              {:else}
                {t("notes.syncedBlockDuplicate")}
              {/if}
            </span>
            <span class="min-w-0 break-all text-[0.8rem] text-muted-foreground">
              {#if syncedBlockSourceId}
                {t("notes.syncedBlockReference", syncedBlockSourceId)}
              {:else}
                {t("notes.syncedBlockOriginalDetail")}
              {/if}
            </span>
            <span class="min-w-0 text-[0.733333rem] text-muted-foreground">
              {#if syncedBlockStatus.role === "original"}
                {t("notes.syncedBlockOriginalPreserved")}
              {:else}
                {t("notes.syncedBlockDuplicatePreserved")}
              {/if}
            </span>
            <span class="min-w-0 text-[0.733333rem] text-muted-foreground">
              {t("notes.syncedBlockFanoutUnavailable")}
            </span>
          </button>
        </section>
      {:else if block.type === "unsupported"}
        {#if advancedBlockLoadState?.status === "ready" && advancedBlockLoadState.component.kind === "unsupported"}
          {@const NotesUnsupportedBlock = advancedBlockLoadState.component.component}
          <NotesUnsupportedBlock
          {block}
          {focusBlockId}
          {focusRequestId}
          onSurfaceKeydown={handleKeydown}
          {onFocusBlock}
          {onConvertUnsupported}
          />
        {/if}
      {:else if block.type === "bookmark" || block.type === "link_preview" || block.type === "embed" || block.type === "equation"}
        {#if advancedBlockLoadState?.status === "ready" && advancedBlockLoadState.component.kind === "card"}
          {@const NotesCardBlock = advancedBlockLoadState.component.component}
          <NotesCardBlock
          {block}
          {previousBlockType}
          {isOnlyBlock}
          {focusBlockId}
          {focusRequestId}
          {onKeyboardAction}
          {onUndo}
          {onRedo}
          {onBookmarkChange}
          {onLinkPreviewUrlChange}
          {onEmbedUrlChange}
          {onEquationExpressionChange}
          />
        {/if}
      {:else if showTextEditor}
        <NotesTextBlockEditor
          indentationDepth={item.depth}
          calloutBackgroundTargetId={calloutColorTarget?.id ?? null}
          calloutBackgroundColor={calloutColorTarget?.color ?? null}
          {block}
          {previousBlockType}
          {isOnlyBlock}
          {focusBlockId}
          {focusRequestId}
          {focusSelection}
          {mentionTargets}
          {commentAnchors}
          {suggestionAnchors}
          {templateStatus}
          {buttonStatus}
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
          {onCopyLink}
          {onDuplicate}
          {onUseTemplate}
          {onAddTemplateChild}
          {onUseButton}
          {onAddButtonChild}
          {onButtonIconChange}
          {onButtonInsertPositionChange}
          {onMoveUp}
          {onMoveDown}
          {onDelete}
          {onToggleOpen}
          {onCodeLanguageChange}
          {onFocusBlock}
        />
      {/if}

      {#if advancedBlockFamily && advancedBlockLoadState?.status === "failed" && advancedBlockLoadState.key === advancedBlockFamily}
        <div class="my-1 rounded-md border border-destructive/40 p-2 text-[0.8rem] text-destructive" role="alert">
          <p>{t("common.viewLoadFailed", block.type)}</p>
          <button class="mt-2 min-h-8 rounded-md border border-border px-2 text-foreground hover:bg-accent" type="button" onclick={() => requestAdvancedBlock(true)}>{t("common.retry")}</button>
        </div>
      {:else if advancedBlockFamily && advancedBlockFamily !== "child-database" && advancedBlockLoadState?.status !== "ready"}
        <NotesLoadingSkeleton kind={advancedBlockFamily === "table" ? "table" : "block"} />
      {/if}

      {#if slashOpen}
        {#if slashMenuLoadState?.status === "ready" && slashMenuLoadState.component.kind === "slash-menu"}
          {@const NotesSlashMenu = slashMenuLoadState.component.component}
          <NotesSlashMenu
            query={text.startsWith("/") ? text.slice(1) : ""}
            canSetColor={blockSupportsColor}
            currentColor={currentColor}
            onSelect={selectSlashCommand}
          />
        {:else if slashMenuLoadState?.status === "failed"}
          <button class="min-h-8 rounded-md border border-border px-2 text-[0.8rem] text-foreground hover:bg-accent" type="button" onclick={() => requestSlashMenu(true)}>{t("common.retry")}</button>
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .notes-block-indent {
    width: calc(var(--notes-depth) * 1.25rem);
  }

  .notes-block-surface {
    min-inline-size: calc(var(--notes-depth) * 1.25rem + 12rem);
    color: var(--notes-block-color, var(--foreground));
    background: var(--notes-block-bg, transparent);
    box-shadow: inset 0 0 0 1px var(--notes-block-border, transparent);
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
    box-shadow:
      inset 0 0 0 2px hsl(var(--primary) / 0.65),
      inset 0 0 0 1px var(--notes-block-border, transparent);
  }

  .notes-callout-surface {
    min-height: 2.25rem;
    padding-block: 0.2rem;
    gap: var(--notes-callout-inset, 0.75rem);
    background: transparent;
    box-shadow: none;
  }

  .notes-callout-direct-child {
    padding-left: var(--notes-callout-inset, 0.75rem);
  }

  .notes-callout-surface :global(.notes-callout-icon) {
    margin-top: calc(0.75rem * var(--font-scale) - 0.5rem);
  }

  .notes-callout-surface :global(.notes-callout-icon-glyph) {
    transform: scale(var(--font-scale));
  }

  .notes-callout-container-only > .notes-block-surface {
    height: 0;
    min-height: 0;
    padding: 0;
    overflow: visible;
  }

  .notes-callout-container-only {
    --notes-callout-first-line-height: calc(1.5rem * var(--font-scale));
  }

  .notes-callout-container-only:where(
    .notes-heading-1,
    .notes-heading-2,
    .notes-heading-3,
    .notes-heading-4,
    .notes-heading-5,
    .notes-heading-6
  ) {
    --notes-callout-first-line-height: calc(var(--notes-heading-size) * var(--font-scale) * 1.3);
  }

  .notes-callout-container-only :global(.notes-callout-icon) {
    position: absolute;
    top: calc(var(--notes-callout-first-line-height) / 2 - 0.375rem);
    left: calc(var(--notes-depth) * 1.25rem);
    margin-top: 0;
    z-index: 2;
  }

  .notes-toc-item {
    padding-left: calc((var(--notes-toc-level) - 1) * 1rem);
  }
</style>
