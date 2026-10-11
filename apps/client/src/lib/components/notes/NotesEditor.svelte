<script lang="ts">
  import NotesLoadingSkeleton from "./NotesLoadingSkeleton.svelte";
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import Archive from "@lucide/svelte/icons/archive";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import Copy from "@lucide/svelte/icons/copy";
  import Download from "@lucide/svelte/icons/download";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import History from "@lucide/svelte/icons/history";
  import { NOTES_COVER_DEFAULT_FOCAL_POINT } from "$lib/notes/pages/cover";
  import type { NotesCoverFocalPoint, NotesPageCover as CoverValue } from "$lib/notes/contracts/assets";
  import ImagePlus from "@lucide/svelte/icons/image-plus";
  import Link2 from "@lucide/svelte/icons/link-2";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import MoreHorizontal from "@lucide/svelte/icons/more-horizontal";
  import Pencil from "@lucide/svelte/icons/pencil";
  import PencilLine from "@lucide/svelte/icons/pencil-line";
  import Share2 from "@lucide/svelte/icons/share-2";
  import SmilePlus from "@lucide/svelte/icons/smile-plus";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import {
    notesPageIconAssetUrl,
    pickNotesPageIconImageFile,
    saveNotesPageIconImageDataUrl,
  } from "$lib/api/notes/page-icons";
  import type {
    IconPickerAsset,
    IconPickerUploadAdapter,
  } from "$lib/components/icon-picker/types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { blockPlainText, isTextEditableBlock } from "$lib/notes/blocks/factory";
  import { notesBlockAnchorId } from "$lib/notes/links/block-link";
  import { notesEditorScrollTopForTarget } from "$lib/notes/editor/scroll";
  import {
    notesBackgroundPointerTargetsDocumentEnd,
    notesDocumentEndFocusIsCurrent,
  } from "$lib/notes/editor/focus";
  import { notesTextSelectionFromEditableRoot } from "$lib/notes/editor/selection";
  import {
    formatNotesActivityDate,
    formatNotesActivityTime,
  } from "$lib/notes/pages/activity";
  import { notesLocalUserDisplayName } from "$lib/notes/collaboration/local-user";
  import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";
  import { notesPageMoveTargets } from "$lib/notes/pages/move";
  import {
    notesPageIconFromPickerValue,
    notesPageIconPickerValue,
  } from "$lib/notes/pages/icon-picker";
  import {
    createNotesExternalPageIcon,
    createNotesLocalFilePageIcon,
    type NotesPageIconAssetMetadata,
  } from "$lib/notes/pages/icon";
  import {
    notesFoldersForProject,
    notesPageFolderMoveTargets,
  } from "$lib/notes/navigation/tree";
  import { addNotesWorkspaceBreadcrumb, buildNotesPageBreadcrumb } from "$lib/notes/pages/breadcrumb";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import {
    notesPageProjectId,
    notesPagesForProject,
  } from "$lib/notes/project-membership";
  import { openNotesSuggestionCount } from "$lib/notes/collaboration/suggestions";
  import {
    filterNotesDiscussions,
    notesPageDiscussionsInReadingOrder,
  } from "$lib/notes/collaboration/discussions";
  import { unreadNotesCommentThreadCount } from "$lib/notes/collaboration/comments";
  import type { NotesDiscussionsPanelFocus } from "./pages/comments/NotesDiscussionsPanel.svelte";
  import { buildNotesTableOfContents } from "$lib/notes/block-types/table-of-contents";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import type {
    NotesAgentBridgeExportRequest,
    NotesHtmlExportRequest,
    NotesPage,
    NotesPageIcon as NotesPageIconValue,
    NotesParent,
  } from "$lib/notes/types";
  import { getNotes } from "$lib/stores/notes.svelte";
  import type { NotesEditorStore } from "$lib/stores/notes/editor-session.svelte";
  import { provideNotesEditor } from "./editor-context";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { cn } from "$lib/utils";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import { portal } from "$lib/utils/portal";
  import NotesBlockList from "$lib/components/notes/blocks/NotesBlockList.svelte";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import NotesPeekModeIcon from "$lib/components/notes/pages/NotesPeekModeIcon.svelte";
  import {
    loadNotesEditorPanel,
    readNotesEditorPanel,
    retryNotesEditorPanel,
    type LoadedNotesEditorPanel,
    type NotesEditorPanelKind,
  } from "./editor-component-registry";
  import {
    EMPTY_NOTES_MUSIC_MENTION_CONTEXT,
    type NotesMusicMentionContext,
  } from "$lib/components/notes/blocks/mention-targets";

  type NotesEditorPanel = "links" | "suggestions";

  const FOCUSED_BLOCK_SCROLL_PADDING_PX = 16;
  const PAGE_PANEL_VIEWPORT_MARGIN_PX = 8;
  const PAGE_PANEL_TRIGGER_GAP_PX = 4;
  const PAGE_MENU_WIDTH_PX = FLOATING_WIDTH.sm;
  const PAGE_MENU_MAX_HEIGHT_PX = 640;
  const ACTIVITY_PANEL_WIDTH_PX = FLOATING_WIDTH.lg;
  const PAGE_DETAILS_PANEL_WIDTH_PX = 704;
  const PAGE_DETAILS_PANEL_MAX_HEIGHT_PX = 512;

  let {
    projectId = null,
    openMode = "full",
    onClose,
    onOpenModeChange,
    pageActionsTarget = null,
    musicMentionContext = EMPTY_NOTES_MUSIC_MENTION_CONTEXT,
    editorStore = null,
    active = true,
  }: {
    projectId?: string | null;
    openMode?: NotesPageOpenMode;
    onClose?: () => void;
    onOpenModeChange?: (mode: NotesPageOpenMode) => void;
    pageActionsTarget?: HTMLElement | null;
    musicMentionContext?: NotesMusicMentionContext;
    editorStore?: NotesEditorStore | null;
    active?: boolean;
  } = $props();

  // A pane keeps the same editor session for its entire mounted lifetime.
  const notes = untrack(() => editorStore) ?? getNotes();
  provideNotesEditor(notes);
  const mobileBackStack = getMobileBackStack();
  const mobileLayout = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const preferences = getPreferences();
  const projects = getProjects();
  const fileExportAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "notes.file-export",
  );
  const localization = getLocalization();
  const { t } = localization;
  const noteActionIconStrokeWidth = 1.5;

  let titleDraft = $state("");
  let titleInput: HTMLInputElement | null = $state(null);
  let coverMenuOpen = $state(false);
  let coverPosition = $state<{ pageId: string; cover: Exclude<CoverValue, { type: "design" }>; focal: NotesCoverFocalPoint } | null>(null);
  let coverPositionSaving = $state(false);
  let coverPositionError = $state<string | null>(null);
  let coverStatus = $state<"loading" | "ready" | "error">("loading");
  let repositionButton: HTMLButtonElement | null = $state(null);
  let coverBanner: HTMLDivElement | null = $state(null);
  let coverMenuButton: HTMLButtonElement | null = $state(null);
  let inlineCommentsRequested = $state(false);
  /** Page-scoped so a request from one page never focuses the composer of the next. */
  let inlineCommentFocus = $state<{ pageId: string; request: number } | null>(null);
  let editorRoot: HTMLElement | null = $state(null);
  let discussionsButton: HTMLButtonElement | null = $state(null);
  let discussionsPopoverOpen = $state(false);
  let discussionsPanelOpen = $state(false);
  /** Page-scoped like the inline comment focus, so a reveal never lands on the next page. */
  let discussionsFocus = $state<(NotesDiscussionsPanelFocus & { pageId: string }) | null>(null);
  let shareButton: HTMLButtonElement | null = $state(null);
  let shareOpen = $state(false);
  let pageMenuOpen = $state(false);
  let moveMenuOpen = $state(false);
  let folderMoveMenuOpen = $state(false);
  let activityPanelOpen = $state(false);
  let activityButton: HTMLButtonElement | null = $state(null);
  let pageMenuButton: HTMLButtonElement | null = $state(null);
  let floatingLayoutVersion = $state(0);
  let activePanel = $state<NotesEditorPanel | null>(null);
  let htmlExportOpen = $state(false);
  let agentBridgeExportOpen = $state(false);
  let pageHistoryModalOpen = $state(false);
  let pendingArchivePage = $state<NotesPage | null>(null);
  let pendingTrashPage = $state<NotesPage | null>(null);
  let requestedIconPicker = $state<"action" | "icon" | null>(null);
  let iconPickerPanelAnchor: HTMLElement | null = $state(null);
  let openingIconPickerFromMenu = false;
  let actionIconPickerTrigger: HTMLButtonElement | null = $state(null);
  let pageIconPickerTrigger: HTMLButtonElement | null = $state(null);
  let panelLoadStates = $state<Partial<Record<
    NotesEditorPanelKind,
    LazyComponentLoadState<NotesEditorPanelKind, LoadedNotesEditorPanel>
  >>>({});
  let blockScrollViewport: HTMLDivElement | null = $state(null);
  let activityNowMs = $state(Date.now());
  let activityPanelHideTimer: number | null = null;
  let lastTitlePageId = "";
  let lastPanelPresencePageId = "";
  let lastHandledTitleFocusRequestId = 0;

  const page = $derived(notes.loadedPage);
  const editablePageTitle = $derived(page ? notesPageTitle(page, "") : "");
  const currentPageTitle = $derived(page ? notesPageTitle(page, t("notes.untitled")) : t("notes.untitled"));
  const pageIconLabel = $derived(pageIconScreenReaderText(page?.icon ?? null));
  const effectiveProjectId = $derived(page ? notesPageProjectId(page) ?? projectId : projectId);
  const projectPages = $derived(notesPagesForProject(
    notes.navigationPages,
    effectiveProjectId,
  ));
  const projectFolders = $derived(notesFoldersForProject(notes.folders, effectiveProjectId));
  const moveTargets = $derived(page
    ? notesPageMoveTargets(
        projectPages,
        page.id,
        t("notes.workspace"),
        (candidate) => notesPageTitle(candidate, t("notes.untitled")),
        notes.recentPageIds,
      )
    : []);
  const folderMoveTargets = $derived(page
    ? notesPageFolderMoveTargets(
        projectPages,
        projectFolders,
        page.id,
        t("notes.projectRoot"),
        (candidate) => notesPageTitle(candidate, t("notes.untitled")),
        notes.recentPageIds,
      ).filter((target) => target.kind !== "page")
    : []);
  const breadcrumbItems = $derived(
    notes.pageBreadcrumbItems.length > 0
      ? addNotesWorkspaceBreadcrumb(
          notes.pageBreadcrumbItems,
          t("notes.workspace"),
          t("notes.untitled"),
          t("notes.breadcrumbMissingPage"),
        )
      : buildNotesPageBreadcrumb(page, notes.allPages, t("notes.workspace"), t("notes.untitled")),
  );
  const tableOfContentsItems = $derived(buildNotesTableOfContents(notes.flatBlocks));
  const pageFavorited = $derived(page ? notes.favoritePageIds.includes(page.id) : false);
  const activityNow = $derived(new Date(activityNowMs));
  const activityAuthorName = $derived(notesLocalUserDisplayName(preferences.profileDisplayName));
  const activityTimeLabels = $derived({
    justNow: t("notes.metadataJustNow"),
    minutesAgo: (minutes: number) => t("notes.metadataMinutesAgo", minutes),
    hoursAgo: (hours: number) => t("notes.metadataHoursAgo", hours),
  });
  const editedDateLabel = $derived(page
    ? formatNotesActivityTime(page.last_edited_time, {
        locale: localization.locale,
        now: activityNow,
        labels: activityTimeLabels,
      })
    : "");
  const createdDateLabel = $derived(page
    ? formatNotesActivityDate(page.created_time, localization.locale, activityNow)
    : "");
  const activityPanelId = $derived(page ? `notes-activity-panel-${page.id}` : undefined);
  const editedMetadataLabel = $derived(t("notes.metadataEdited", editedDateLabel));
  const activityEditedByLabel = $derived(t("notes.activityEditedBy", activityAuthorName));
  const activityCreatedByLabel = $derived(t("notes.activityCreatedBy", activityAuthorName));
  const activityPanelLabel = $derived(
    page ? `${activityEditedByLabel}, ${editedDateLabel}` : t("notes.activity"),
  );
  const openSuggestionCount = $derived(openNotesSuggestionCount(notes.suggestions));
  const linksBadgeCount = $derived(notes.backlinks.length + notes.pageAliases.length + notes.unresolvedLinks.length);
  const hasPanelMenuItems = $derived(linksBadgeCount > 0 || openSuggestionCount > 0);
  const inlineCommentsId = $derived(page ? `notes-page-comments-${page.id}` : undefined);
  const pageDiscussionCount = $derived(notesPageDiscussionsInReadingOrder(notes.allCommentThreads).length);
  const inlineCommentsVisible = $derived(inlineCommentsRequested || pageDiscussionCount > 0);
  const isPeekMode = $derived(openMode !== "full");
  const pageCommentThreads = $derived(page
    ? notes.allCommentThreads.filter((thread) => thread.page_id === page.id)
    : []);
  const openPageDiscussions = $derived(filterNotesDiscussions(pageCommentThreads, "open"));
  const unreadPageDiscussionCount = $derived(unreadNotesCommentThreadCount(openPageDiscussions));
  /** Resolved discussions keep the button so the All discussions panel stays reachable. */
  const discussionsButtonVisible = $derived(
    discussionsPanelOpen || pageCommentThreads.some((thread) => thread.comments.length > 0),
  );
  const discussionsButtonLabel = $derived(
    t("notes.discussionsButton", openPageDiscussions.length, unreadPageDiscussionCount),
  );
  const discussionsPanelFocus = $derived<NotesDiscussionsPanelFocus | null>(
    page && discussionsFocus?.pageId === page.id
      ? { discussionId: discussionsFocus.discussionId, reply: discussionsFocus.reply, request: discussionsFocus.request }
      : null,
  );
  const activityPanelStyle = $derived.by(() => {
    void floatingLayoutVersion;
    return floatingPagePanelStyle(activityButton, ACTIVITY_PANEL_WIDTH_PX);
  });
  const pageMenuStyle = $derived.by(() => {
    void floatingLayoutVersion;
    return floatingPagePanelStyle(pageMenuButton, PAGE_MENU_WIDTH_PX);
  });
  const pageDetailsPanelStyle = $derived.by(() => {
    void floatingLayoutVersion;
    return floatingPagePanelStyle(
      pageMenuButton,
      PAGE_DETAILS_PANEL_WIDTH_PX,
      PAGE_DETAILS_PANEL_MAX_HEIGHT_PX,
    );
  });

  /** Place a page popover below its toolbar trigger without clipping at viewport edges. */
  function floatingPagePanelStyle(
    anchor: HTMLElement | null,
    preferredWidth: number,
    preferredMaxHeight = PAGE_MENU_MAX_HEIGHT_PX,
  ): string {
    if (!anchor || typeof window === "undefined") return "visibility:hidden";
    const rect = anchor.getBoundingClientRect();
    const width = Math.max(0, Math.min(
      preferredWidth,
      window.innerWidth - PAGE_PANEL_VIEWPORT_MARGIN_PX * 2,
    ));
    const maxLeft = Math.max(
      PAGE_PANEL_VIEWPORT_MARGIN_PX,
      window.innerWidth - width - PAGE_PANEL_VIEWPORT_MARGIN_PX,
    );
    const left = Math.max(PAGE_PANEL_VIEWPORT_MARGIN_PX, Math.min(rect.right - width, maxLeft));
    const top = Math.min(
      rect.bottom + PAGE_PANEL_TRIGGER_GAP_PX,
      window.innerHeight - PAGE_PANEL_VIEWPORT_MARGIN_PX,
    );
    const maxHeight = Math.max(0, Math.min(
      preferredMaxHeight,
      window.innerHeight - top - PAGE_PANEL_VIEWPORT_MARGIN_PX,
    ));
    return `left:${Math.round(left)}px;top:${Math.round(top)}px;width:${Math.round(width)}px;max-height:${Math.round(maxHeight)}px`;
  }

  $effect(() => {
    if (!activityPanelOpen && !pageMenuOpen && !activePanel && !coverMenuOpen) return;
    const update = () => {
      floatingLayoutVersion += 1;
    };
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  });

  function requestEditorPanel(kind: NotesEditorPanelKind, retry = false): void {
    const current = panelLoadStates[kind] ?? null;
    if (!retry && current?.key === kind) return;
    const loadingState = beginLazyComponentLoad(current, kind);
    const cached = retry ? null : readNotesEditorPanel(kind);
    if (cached) {
      panelLoadStates = { ...panelLoadStates, [kind]: resolveLazyComponentLoad(loadingState, kind, loadingState.requestId, cached) };
      return;
    }
    panelLoadStates = { ...panelLoadStates, [kind]: loadingState };
    const request = retry ? retryNotesEditorPanel(kind) : loadNotesEditorPanel(kind);
    void request.then((component) => {
      const latest = panelLoadStates[kind];
      if (!latest) return;
      panelLoadStates = {
        ...panelLoadStates,
        [kind]: resolveLazyComponentLoad(latest, kind, loadingState.requestId, component),
      };
    }).catch((error: unknown) => {
      const latest = panelLoadStates[kind];
      if (!latest) return;
      panelLoadStates = {
        ...panelLoadStates,
        [kind]: rejectLazyComponentLoad(latest, kind, loadingState.requestId, error),
      };
      console.error(`load Notes ${kind} panel failed`, error);
    });
  }

  /** Toggle the share popover, loading its surface only on first use. */
  function toggleSharePanel(): void {
    shareOpen = !shareOpen;
    if (shareOpen) requestEditorPanel("share");
  }

  function openIconPicker(kind: "action" | "icon", panelAnchor: HTMLElement | null = null): void {
    if (kind === "action") iconPickerPanelAnchor = panelAnchor;
    requestedIconPicker = kind;
    requestEditorPanel("icon-picker", panelLoadStates["icon-picker"]?.status === "failed");
  }

  $effect(() => {
    if (activePanel === "links") {
      requestEditorPanel("backlinks");
      requestEditorPanel("page-links");
    } else if (activePanel === "suggestions") {
      requestEditorPanel("suggestions");
    }
    if (moveMenuOpen || folderMoveMenuOpen) requestEditorPanel("destination-picker");
    if (coverMenuOpen) requestEditorPanel("cover-menu");
    if (page?.cover) requestEditorPanel("page-cover");
    if (page?.parent.type === "data_source_id") requestEditorPanel("row-properties");
    if (inlineCommentsVisible) requestEditorPanel("page-comments");
    if (discussionsPopoverOpen) requestEditorPanel("discussions-popover");
    if (discussionsPanelOpen) requestEditorPanel("discussions-panel");
    if (htmlExportOpen) requestEditorPanel("html-export");
    if (agentBridgeExportOpen) requestEditorPanel("agent-export");
    if (pageHistoryModalOpen) requestEditorPanel("page-history");
    if (pendingArchivePage || pendingTrashPage) requestEditorPanel("confirm-dialog");
    if (requestedIconPicker && panelLoadStates["icon-picker"]?.status === "ready") {
      const kind = requestedIconPicker;
      requestedIconPicker = null;
      void tick().then(() => {
        openingIconPickerFromMenu = kind === "action" && iconPickerPanelAnchor !== null;
        try {
          (kind === "action" ? actionIconPickerTrigger : pageIconPickerTrigger)?.click();
        } finally {
          openingIconPickerFromMenu = false;
        }
      });
    }
  });

  $effect(() => {
    if (!page || page.id === lastTitlePageId) return;
    if (lastTitlePageId) notes.clearPageTitleDraft(lastTitlePageId);
    lastTitlePageId = page.id;
    notes.clearPageTitleDraft(page.id);
    titleDraft = editablePageTitle;
    activePanel = null;
    inlineCommentsRequested = false;
    inlineCommentFocus = null;
    discussionsPopoverOpen = false;
    discussionsPanelOpen = false;
    discussionsFocus = null;
    const discussionRequest = untrack(() => notes.discussionRequest);
    if (discussionRequest.pageId !== page.id) notes.completeDiscussionRequest(discussionRequest.requestId);
    pageMenuOpen = false;
    moveMenuOpen = false;
    folderMoveMenuOpen = false;
    activityPanelOpen = false;
    coverMenuOpen = false;
    pageHistoryModalOpen = false;
    coverPosition = null;
    coverPositionError = null;
    coverPositionSaving = false;
  });

  $effect(() => {
    const request = notes.discussionRequest;
    const discussionId = request.discussionId;
    if (!page || !discussionId || request.pageId !== page.id) return;
    untrack(() => {
      openDiscussionsPanel(discussionId, false);
      notes.completeDiscussionRequest(request.requestId);
    });
  });

  $effect(() => {
    if (notes.activeCommentParent?.type !== "block_id") return;
    untrack(() => {
      discussionsPopoverOpen = false;
      discussionsPanelOpen = true;
    });
  });

  $effect(() => {
    if (!page || page.id !== lastTitlePageId) return;
    const storedTitle = editablePageTitle;
    if (notes.pageTitleDraftForPage(page.id) !== null) return;
    if (typeof document !== "undefined" && document.activeElement === titleInput) return;
    titleDraft = storedTitle;
  });

  $effect(() => {
    const pageId = page?.id ?? null;
    if (!pageId) {
      lastPanelPresencePageId = "";
      return;
    }
    if (pageId === lastPanelPresencePageId) return;
    lastPanelPresencePageId = pageId;
    for (const subsystem of ["links", "comments", "suggestions"] as const) {
      void notes.ensureOptionalSubsystem(subsystem, pageId).catch((error) => {
        console.error(`load Notes ${subsystem} presence failed`, error);
      });
    }
  });

  function handleBlockViewportPointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    const viewport = blockScrollViewport;
    const target = event.target;
    if (!viewport || !(target instanceof Element)) return;
    const rows = Array.from(
      viewport.querySelectorAll<HTMLElement>("[data-notes-selectable-block-id]"),
    );
    const lastRow = rows.at(-1);
    if (!lastRow) return;
    const clickedRow = target.closest<HTMLElement>("[data-notes-selectable-block-id]");
    if (!notesBackgroundPointerTargetsDocumentEnd({
      pointerY: event.clientY,
      lastRowBottom: lastRow.getBoundingClientRect().bottom,
      targetInsideRow: clickedRow !== null && viewport.contains(clickedRow),
    })) {
      return;
    }
    const blockId = lastRow.dataset.notesSelectableBlockId;
    if (!blockId) return;
    const block = notes.blockById(blockId);
    if (!block) return;
    event.preventDefault();
    if (isTextEditableBlock(block.type)) {
      const end = blockPlainText(block).length;
      const editor = Array.from(
        lastRow.querySelectorAll<HTMLElement>(
          "[contenteditable='true'][role='textbox'][data-notes-block-id]",
        ),
      ).find((candidate) => candidate.dataset.notesBlockId === blockId) ?? null;
      const selection = editor ? notesTextSelectionFromEditableRoot(editor) : null;
      const activeBlockId = document.activeElement instanceof HTMLElement
        ? document.activeElement.dataset.notesBlockId ?? null
        : null;
      if (notesDocumentEndFocusIsCurrent({
        activeBlockId,
        lastBlockId: blockId,
        selection,
        textLength: end,
      })) {
        return;
      }
      notes.focusBlock(blockId, { start: end, end });
      return;
    }
    notes.focusBlock(blockId);
  }

  function documentEndPointer(node: HTMLDivElement): { destroy: () => void } {
    node.addEventListener("pointerdown", handleBlockViewportPointerDown);
    return {
      destroy() {
        node.removeEventListener("pointerdown", handleBlockViewportPointerDown);
      },
    };
  }

  /** Restore the retained page's viewport before its virtualized rows settle. */
  function editorViewport(node: HTMLDivElement): void {
    $effect(() => {
      const pageId = page?.id;
      const top = untrack(() => notes.editorScrollTop);
      let cancelled = false;
      void tick().then(() => {
        if (cancelled || page?.id !== pageId) return;
        node.scrollTop = top;
        node.dispatchEvent(new Event("scroll"));
      });
      return () => { cancelled = true; };
    });
  }

  onMount(() => {
    activityNowMs = Date.now();
    const intervalId = window.setInterval(() => {
      activityNowMs = Date.now();
    }, 30_000);
    return () => window.clearInterval(intervalId);
  });

  $effect(() => {
    if (!active) return;
    const titleFocusRequestId = notes.titleFocusRequestId;
    if (titleFocusRequestId === lastHandledTitleFocusRequestId) return;
    if (!page || notes.titleFocusPageId !== page.id) return;
    lastHandledTitleFocusRequestId = titleFocusRequestId;
    focusTitleInput(false);
  });

  let lastScrolledFocusRequestId = 0;
  $effect(() => {
    if (!active) return;
    const focusRequestId = notes.focusRequestId;
    if (focusRequestId === lastScrolledFocusRequestId) return;
    lastScrolledFocusRequestId = focusRequestId;
    const preventScroll = notes.focusPreventScroll;
    const blockId = notes.focusBlockId;
    if (!blockId) return;
    void tick().then(() => {
      const viewport = blockScrollViewport;
      if (!viewport || notes.focusRequestId !== focusRequestId) return;
      const anchor = viewport.querySelector<HTMLElement>(
        `#${CSS.escape(notesBlockAnchorId(blockId))}`,
      );
      if (!anchor) return;
      const viewportRect = viewport.getBoundingClientRect();
      const targetRect = anchor.getBoundingClientRect();
      const nextScrollTop = notesEditorScrollTopForTarget({
        scrollTop: viewport.scrollTop,
        maxScrollTop: viewport.scrollHeight - viewport.clientHeight,
        viewportTop: viewportRect.top,
        viewportBottom: viewportRect.bottom,
        targetTop: targetRect.top,
        targetBottom: targetRect.bottom,
        padding: FOCUSED_BLOCK_SCROLL_PADDING_PX,
        alignment: "nearest",
        preventScroll,
      });
      if (nextScrollTop !== viewport.scrollTop) viewport.scrollTop = nextScrollTop;
    });
  });

  onDestroy(() => {
    clearActivityPanelHideTimer();
    if (activePanel) notes.setPagePanelSubsystemOpen(activePanel, false);
    if (pageHistoryModalOpen) notes.setPagePanelSubsystemOpen("page-history", false);
    if (lastTitlePageId) notes.clearPageTitleDraft(lastTitlePageId);
  });

  async function saveTitle(): Promise<void> {
    if (!page) return;
    if (notes.pageTitleDraftForPage(page.id) === null) {
      titleDraft = editablePageTitle;
      return;
    }
    const title = titleDraft.trim();
    titleDraft = title;
    if (title === editablePageTitle) {
      notes.clearPageTitleDraft(page.id);
      return;
    }
    await notes.renamePage(page.id, title);
    notes.clearPageTitleDraft(page.id);
  }

  function handleTitleInput(event: Event): void {
    if (!page) return;
    const target = event.currentTarget;
    const title = target instanceof HTMLInputElement ? target.value : titleDraft;
    notes.setPageTitleDraft(page.id, title);
  }

  function handleTitleKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter" && !event.isComposing && page && notes.primaryContentReady) {
      event.preventDefault();
      const blockId = notes.ensurePageBody(page.id);
      if (blockId) notes.focusBlock(blockId, { start: 0, end: 0 });
    }
  }

  function actionButtonClass(active = false): string {
    return cn(
      "relative flex shrink-0 items-center justify-center rounded-md px-1.5 text-foreground transition-colors hover:bg-accent",
      mobileLayout ? "h-12 min-w-12" : "h-7 min-w-7",
      active && "bg-accent",
    );
  }

  function menuItemClass(destructive = false): string {
    return cn("menu-item", destructive && "menu-item-destructive");
  }

  $effect(() => {
    if (!mobileLayout || !pageMenuOpen) return;
    return mobileBackStack.activate({ handle: closePageMenu });
  });

  $effect(() => {
    if (!mobileLayout || !moveMenuOpen) return;
    return mobileBackStack.activate({
      handle: () => {
        moveMenuOpen = false;
      },
    });
  });

  $effect(() => {
    if (!mobileLayout || !folderMoveMenuOpen) return;
    return mobileBackStack.activate({
      handle: () => {
        folderMoveMenuOpen = false;
      },
    });
  });

  $effect(() => {
    if (!mobileLayout || !activePanel) return;
    return mobileBackStack.activate({ handle: closeActionPanel });
  });

  $effect(() => {
    if (!mobileLayout || !discussionsPanelOpen) return;
    return mobileBackStack.activate({ handle: closeDiscussionsPanel });
  });

  $effect(() => {
    if (!mobileLayout || !activityPanelOpen) return;
    return mobileBackStack.activate({
      handle: () => {
        activityPanelOpen = false;
      },
    });
  });

  function togglePanel(panel: NotesEditorPanel): void {
    if (activePanel) notes.setPagePanelSubsystemOpen(activePanel, false);
    const nextPanel = activePanel === panel ? null : panel;
    activePanel = nextPanel;
    if (nextPanel) {
      notes.setPagePanelSubsystemOpen(nextPanel, true);
      void notes.ensureOptionalSubsystem(nextPanel).catch((error) => {
        console.error(`load notes ${nextPanel} panel failed`, error);
      });
      if (nextPanel === "links") {
        void notes.reloadLinkResolutionPages().catch((error) => {
          console.error("load notes link destinations failed", error);
        });
      }
    }
    pageMenuOpen = false;
    moveMenuOpen = false;
    folderMoveMenuOpen = false;
  }

  function openPageHistory(): void {
    if (activePanel) notes.setPagePanelSubsystemOpen(activePanel, false);
    notes.setPagePanelSubsystemOpen("page-history", true);
    pageHistoryModalOpen = true;
    void notes.ensureOptionalSubsystem("page-history").catch((error) => {
      console.error("load notes page history failed", error);
    });
    activePanel = null;
    pageMenuOpen = false;
    moveMenuOpen = false;
  }

  function closePageMenu(): void {
    pageMenuOpen = false;
    moveMenuOpen = false;
    folderMoveMenuOpen = false;
  }

  function closeActionPanel(): void {
    if (activePanel) notes.setPagePanelSubsystemOpen(activePanel, false);
    activePanel = null;
  }

  function showActivityPanel(): void {
    clearActivityPanelHideTimer();
    activityPanelOpen = true;
  }

  function hideActivityPanel(): void {
    clearActivityPanelHideTimer();
    activityPanelOpen = false;
  }

  function clearActivityPanelHideTimer(): void {
    if (activityPanelHideTimer === null) return;
    window.clearTimeout(activityPanelHideTimer);
    activityPanelHideTimer = null;
  }

  function scheduleHideActivityPanel(): void {
    clearActivityPanelHideTimer();
    activityPanelHideTimer = window.setTimeout(() => {
      activityPanelOpen = false;
      activityPanelHideTimer = null;
    }, 180);
  }

  function handleActivityFocusOut(event: FocusEvent): void {
    const currentTarget = event.currentTarget;
    if (!(currentTarget instanceof HTMLElement)) return;
    const nextTarget = event.relatedTarget;
    if (nextTarget instanceof Node && currentTarget.contains(nextTarget)) return;
    hideActivityPanel();
  }

  /** Open the cover picker from a trigger that remains mounted after the page menu closes. */
  function openCoverMenu(trigger: HTMLButtonElement): void {
    coverMenuButton = trigger;
    coverMenuOpen = true;
    if (panelLoadStates["cover-menu"]?.status === "failed") {
      requestEditorPanel("cover-menu", true);
    }
    closePageMenu();
    closeActionPanel();
  }

  /** Toggle the cover picker from the active trigger. */
  function toggleCoverMenu(event: MouseEvent): void {
    if (!(event.currentTarget instanceof HTMLButtonElement)) return;
    if (coverMenuOpen) {
      closeCoverMenu();
      return;
    }
    openCoverMenu(event.currentTarget);
  }

  /** Start a local crop draft; pointer movements never write to storage. */
  function startCoverPosition(): void {
    if (!page?.cover || page.cover.type === "design" || coverStatus !== "ready") return;
    coverMenuOpen = false;
    coverPositionError = null;
    coverPositionSaving = false;
    closePageMenu();
    closeActionPanel();
    coverPosition = { pageId: page.id, cover: page.cover, focal: { ...(page.cover.focal_point ?? NOTES_COVER_DEFAULT_FOCAL_POINT) } };
    void tick().then(() => coverBanner?.querySelector<HTMLButtonElement>("[data-cover-drag]")?.focus({ preventScroll: true }));
  }

  /** Discard the unsaved crop and restore the positioning trigger. */
  function cancelCoverPosition(): void {
    if (coverPositionSaving) return;
    coverPosition = null;
    coverPositionError = null;
    void tick().then(() => repositionButton?.focus({ preventScroll: true }));
  }

  /** Persist one confirmed position and keep failed drafts available for retry. */
  async function saveCoverPosition(): Promise<void> {
    const draft = coverPosition;
    if (!draft || coverPositionSaving) return;
    coverPositionSaving = true;
    coverPositionError = null;
    try {
      await notes.updatePageCover(draft.pageId, { ...draft.cover, focal_point: { ...draft.focal } });
      if (coverPosition === draft) {
        coverPosition = null;
        void tick().then(() => repositionButton?.focus({ preventScroll: true }));
      }
    } catch (cause) {
      if (coverPosition === draft) coverPositionError = t("notes.pageCoverSaveFailed", cause instanceof Error ? cause.message : String(cause));
      else console.error("Could not save Notes cover position", cause);
    } finally {
      if (!coverPosition || coverPosition === draft) coverPositionSaving = false;
    }
  }

  $effect(() => {
    if (coverPosition && (coverPosition.pageId !== page?.id || !page.cover || page.cover.type === "design")) {
      coverPosition = null;
      coverPositionError = null;
      coverPositionSaving = false;
    }
  });

  $effect(() => {
    if (!coverPosition) return;
    const handleEscape = (event: KeyboardEvent): void => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopImmediatePropagation();
      cancelCoverPosition();
    };
    document.addEventListener("keydown", handleEscape, true);
    return () => document.removeEventListener("keydown", handleEscape, true);
  });

  $effect(() => {
    if (mobileLayout && coverPosition) return mobileBackStack.activate({ handle: cancelCoverPosition });
  });

  function closeCoverMenu(): void {
    coverMenuOpen = false;
  }

  function prepareIconPicker(toggle: () => void): void {
    coverMenuOpen = false;
    closePageMenu();
    closeActionPanel();
    toggle();
  }

  function notesIconAssetMetadata(asset: IconPickerAsset): NotesPageIconAssetMetadata {
    if (!asset.contentType || asset.byteSize === undefined || !asset.sha256) {
      throw new Error(t("notes.pageIconUploadFailed"));
    }
    return {
      relativePath: asset.relativePath,
      originalName: asset.originalName,
      contentType: asset.contentType,
      byteSize: asset.byteSize,
      sha256: asset.sha256,
    };
  }

  function updatePageIconFromPicker(value: string): void {
    if (!page) return;
    const icon = notesPageIconFromPickerValue(value, projects.customEmojis);
    void notes.updatePageIcon(page.id, icon);
  }

  const notesIconUploadAdapter: IconPickerUploadAdapter = {
    pickImageFile: pickNotesPageIconImageFile,
    saveImageDataUrl: saveNotesPageIconImageDataUrl,
    assetUrl: (asset) => notesPageIconAssetUrl(asset.relativePath),
    selectAsset: (asset) => {
      if (!page) return;
      return notes.updatePageIcon(page.id, createNotesLocalFilePageIcon(notesIconAssetMetadata(asset)));
    },
    selectPickedAssetImmediately: true,
    selectExternalUrl: (url) => {
      if (!page) return;
      return notes.updatePageIcon(page.id, createNotesExternalPageIcon(url));
    },
  };

  function selectOpenMode(mode: NotesPageOpenMode): void {
    onOpenModeChange?.(mode);
  }

  function openAsFullPage(): void {
    selectOpenMode("full");
  }

  function focusTitleInput(selectTitle = true): void {
    closePageMenu();
    void tick().then(() => {
      titleInput?.focus();
      if (selectTitle) {
        titleInput?.select();
        return;
      }
      const offset = titleInput?.value.length ?? 0;
      titleInput?.setSelectionRange(offset, offset);
    });
  }

  function duplicateCurrentPage(): void {
    if (!page) return;
    void notes.duplicatePage(page.id, t("notes.duplicatePageTitle", currentPageTitle));
    closePageMenu();
  }

  function moveToTarget(targetKey: string): void {
    if (!page) return;
    const target = moveTargets.find((candidate) => candidate.key === targetKey);
    if (!target) return;
    closePageMenu();
    void notes.movePage(page.id, target.parent);
  }

  function moveToFolderTarget(targetKey: string): void {
    if (!page) return;
    const target = folderMoveTargets.find((candidate) => candidate.key === targetKey);
    if (!target) return;
    closePageMenu();
    void notes.movePageToFolder(page.id, target.folderId);
  }

  function confirmArchivePage(): void {
    const targetPage = pendingArchivePage;
    pendingArchivePage = null;
    if (targetPage) void notes.archivePage(targetPage.id);
  }

  function confirmTrashPage(): void {
    const targetPage = pendingTrashPage;
    pendingTrashPage = null;
    if (targetPage) void notes.trashPage(targetPage.id).catch((error) => {
      console.error("trash Notes page failed", error);
    });
  }

  function archiveCurrentPage(): void {
    if (!page) return;
    pendingArchivePage = page;
    closePageMenu();
  }

  function trashCurrentPage(): void {
    if (!page) return;
    pendingTrashPage = page;
    closePageMenu();
  }

  function exportHtmlArchive(input: {
    includePageTree: boolean;
    includeComments: boolean;
    includeResolvedComments: boolean;
    includeAssets: boolean;
    includeDatabaseViews: boolean;
  }) {
    const request: Omit<NotesHtmlExportRequest, "page_id"> = {
      include_page_tree: input.includePageTree,
      include_comments: input.includeComments,
      include_resolved_comments: input.includeResolvedComments,
      include_assets: input.includeAssets,
      include_database_views: input.includeDatabaseViews,
    };
    return notes.exportHtmlArchive(request);
  }

  function exportAgentBridge(input: {
    includeDescendants: boolean;
    includeBacklinks: boolean;
    includeDatabaseViews: boolean;
    includeTaskContext: boolean;
    includePageComments: boolean;
    includeResolvedComments: boolean;
    projectIds: string[];
  }) {
    const request: NotesAgentBridgeExportRequest = {
      include_descendants: input.includeDescendants,
      include_backlinks: input.includeBacklinks,
      include_database_views: input.includeDatabaseViews,
      include_task_context: input.includeTaskContext,
      include_page_comments: input.includePageComments,
      include_resolved_comments: input.includeResolvedComments,
      project_ids: input.projectIds,
    };
    return notes.exportAgentBridge(request);
  }

  /** Reveal the page's inline discussion and move focus into its composer. */
  function revealInlineComments(): void {
    if (!page) return;
    inlineCommentsRequested = true;
    inlineCommentFocus = {
      pageId: page.id,
      request: inlineCommentFocus?.pageId === page.id ? inlineCommentFocus.request + 1 : 1,
    };
    coverMenuOpen = false;
    closePageMenu();
  }

  function preloadDiscussions(): void {
    requestEditorPanel("discussions-popover");
    requestEditorPanel("discussions-panel");
  }

  /** Close the All discussions panel from the header button, otherwise open the summary or, on mobile, the panel. */
  function toggleDiscussions(): void {
    if (discussionsPanelOpen) {
      closeDiscussionsPanel();
    } else if (mobileLayout) {
      openDiscussionsPanel(null, false);
    } else {
      discussionsPopoverOpen = !discussionsPopoverOpen;
    }
  }

  /** Open the All discussions panel on one discussion, or move focus into the panel when none is given. */
  function openDiscussionsPanel(discussionId: string | null, reply: boolean): void {
    if (!page) return;
    discussionsPopoverOpen = false;
    discussionsPanelOpen = true;
    discussionsFocus = {
      pageId: page.id,
      discussionId,
      reply,
      request: discussionsFocus?.pageId === page.id ? discussionsFocus.request + 1 : 1,
    };
  }

  /**
   * Close the All discussions panel and drop a block comment that was being started. Focus that was inside the
   * panel returns to the commented block, the header button, or the title, in that order.
   */
  function closeDiscussionsPanel(): void {
    const panel = editorRoot?.querySelector("[data-notes-discussions-panel]");
    const focusWasInside = typeof document !== "undefined" && !!panel?.contains(document.activeElement);
    const pendingParent = notes.activeCommentParent;
    discussionsPanelOpen = false;
    discussionsFocus = null;
    if (pendingParent?.type === "block_id") notes.setActiveCommentParent(null);
    if (!focusWasInside) return;
    if (pendingParent?.type === "block_id" && notes.blockById(pendingParent.block_id)) {
      notes.focusBlock(pendingParent.block_id);
      return;
    }
    void tick().then(() => {
      if (discussionsButton?.isConnected) discussionsButton.focus({ preventScroll: true });
      else if (!mobileLayout) titleInput?.focus({ preventScroll: true });
    });
  }

  function pageIconScreenReaderText(icon: NotesPageIconValue | null): string {
    if (!icon) return t("notes.noPageIcon");
    if (icon.type === "emoji") return icon.emoji;
    if (icon.type === "icon") return icon.icon.name;
    if (icon.type === "custom_emoji") return icon.custom_emoji.name ?? t("notes.pageIconCustomEmoji");
    return icon.type === "external" ? t("notes.pageIconImage") : icon.file.name ?? t("notes.pageIconImage");
  }
</script>

{#if page}
  <section
    bind:this={editorRoot}
    class="notes-editor-root relative flex h-full w-full min-w-0 flex-1 flex-col overflow-hidden"
    data-mobile={mobileLayout || undefined}
  >
    {#if notes.editorSaveError}
      <div role="alert" class="flex items-center gap-2 border-b border-destructive/30 px-4 py-2 text-sm text-destructive">
        <span>{t("notes.editorSaveFailed", notes.editorSaveError)}</span>
        <button type="button" class="shrink-0 rounded-md border px-2 py-1" onclick={() => void notes.retryEditorMutations().catch(() => undefined)}>{t("common.retry")}</button>
      </div>
    {/if}
    {#snippet discussionsTrigger()}
      <button
        bind:this={discussionsButton}
        type="button"
        class={actionButtonClass(discussionsPopoverOpen || discussionsPanelOpen)}
        aria-label={discussionsButtonLabel}
        data-app-tooltip={t("notes.discussions")}
        aria-haspopup="dialog"
        aria-expanded={discussionsPopoverOpen || discussionsPanelOpen}
        data-notes-discussions-button
        onpointerenter={preloadDiscussions}
        onfocus={preloadDiscussions}
        onclick={toggleDiscussions}
      >
        <MessageSquare class="size-4" strokeWidth={noteActionIconStrokeWidth} />
        {#if unreadPageDiscussionCount > 0}
          <span class="absolute top-1 right-1 size-1.5 rounded-full bg-primary" aria-hidden="true"></span>
        {/if}
      </button>
      {#if discussionsPopoverOpen && discussionsButton}
        {#if panelLoadStates["discussions-popover"]?.status === "ready" && panelLoadStates["discussions-popover"].component.kind === "discussions-popover"}
          {@const NotesDiscussionsPopover = panelLoadStates["discussions-popover"].component.component}
          <NotesDiscussionsPopover
            anchor={discussionsButton}
            pageId={page.id}
            now={activityNow}
            timeLabels={activityTimeLabels}
            onClose={() => { discussionsPopoverOpen = false; }}
            onSeeAll={() => openDiscussionsPanel(null, false)}
            onOpenDiscussion={openDiscussionsPanel}
          />
        {:else if panelLoadStates["discussions-popover"]?.status === "failed"}
          <button class="surface-floating fixed inset-0 z-50 m-auto h-10 px-3" type="button" onclick={() => requestEditorPanel("discussions-popover", true)}>{t("common.retry")}</button>
        {/if}
      {/if}
    {/snippet}
    {#if isPeekMode}
      <div class="absolute right-3 top-2 z-40 flex items-center gap-0.5 rounded-md border border-border bg-popover/95 p-0.5 shadow-sm" data-notes-peek-controls>
        {#if discussionsButtonVisible}
          {@render discussionsTrigger()}
        {/if}
        <button
          type="button"
          class={actionButtonClass()}
          aria-label={t("notes.closePeek")}
          data-app-tooltip={t("notes.closePeek")}
          onclick={() => onClose?.()}
        >
          <X class="size-4" strokeWidth={noteActionIconStrokeWidth} />
        </button>
        <button
          type="button"
          class={actionButtonClass()}
          aria-label={t("notes.expandNote")}
          data-app-tooltip={t("notes.expandNote")}
          onclick={openAsFullPage}
        >
          <NotesPeekModeIcon mode="full" class="size-4" strokeWidth={noteActionIconStrokeWidth} />
        </button>
        <button
          type="button"
          class={actionButtonClass()}
          aria-label={openMode === "side" ? t("notes.centerPeek") : t("notes.sidePeek")}
          data-app-tooltip={openMode === "side" ? t("notes.centerPeek") : t("notes.sidePeek")}
          onclick={() => selectOpenMode(openMode === "side" ? "center" : "side")}
        >
          {#if openMode === "side"}
            <NotesPeekModeIcon mode="center" class="size-4" strokeWidth={noteActionIconStrokeWidth} />
          {:else}
            <NotesPeekModeIcon mode="side" class="size-4" strokeWidth={noteActionIconStrokeWidth} />
          {/if}
        </button>
      </div>
    {/if}
    {#if pageActionsTarget}
      <div
        class="relative z-30 flex shrink-0 items-center gap-1"
        data-notes-page-actions
        use:portal={pageActionsTarget}
        use:dismissOnOutside={{ enabled: !!activePanel, onDismiss: (reason) => {
          closeActionPanel();
          if (reason === "escape") pageMenuButton?.focus({ preventScroll: true });
        } }}
      >
        {#if mobileLayout && openMode === "full"}
          <button
            type="button"
            class={actionButtonClass()}
            aria-label={t("notes.showProjectHome")}
            onclick={() => onClose?.()}
          >
            <ChevronLeft class="size-5" strokeWidth={noteActionIconStrokeWidth} />
          </button>
        {/if}
        <div
          class="relative hidden shrink-0 min-[560px]:block"
          role="group"
          aria-label={t("notes.activity")}
          onmouseenter={showActivityPanel}
          onmouseleave={scheduleHideActivityPanel}
          onfocusin={showActivityPanel}
          onfocusout={handleActivityFocusOut}
        >
          <button
            bind:this={activityButton}
            type="button"
            class="flex h-7 max-w-40 items-center rounded-md px-2 text-[0.8rem] text-foreground transition-colors hover:bg-accent focus-visible:bg-accent"
            aria-label={activityPanelLabel}
            aria-expanded={activityPanelOpen}
            aria-controls={activityPanelId}
          >
            <span class="truncate">{editedMetadataLabel}</span>
          </button>
          {#if activityPanelOpen}
            <div
              id={activityPanelId}
              class="surface-floating fixed z-80 overflow-hidden"
              style={activityPanelStyle}
              role="dialog"
              aria-label={t("notes.activity")}
              data-app-floating-surface
            >
              <div class="border-b border-border px-4 py-3 text-panel-detail font-medium text-muted-foreground">
                {t("notes.activity")}
              </div>
              <div class="flex flex-col gap-3 px-4 py-3">
                <div class="flex min-w-0 items-center justify-between gap-4">
                  <span class="min-w-0 truncate text-foreground">{activityEditedByLabel}</span>
                  <span class="shrink-0 text-muted-foreground">{editedDateLabel}</span>
                </div>
                <div class="flex min-w-0 items-center justify-between gap-4">
                  <span class="min-w-0 truncate text-foreground">{activityCreatedByLabel}</span>
                  <span class="shrink-0 text-muted-foreground">{createdDateLabel}</span>
                </div>
              </div>
            </div>
          {/if}
        </div>
        {#if discussionsButtonVisible && !isPeekMode}
          {@render discussionsTrigger()}
        {/if}
        <button
          bind:this={shareButton}
          type="button"
          class={actionButtonClass(shareOpen)}
          aria-label={t("notes.share")}
          data-app-tooltip={t("notes.share")}
          aria-haspopup="dialog"
          aria-expanded={shareOpen}
          data-notes-share-button
          onpointerenter={() => requestEditorPanel("share")}
          onfocus={() => requestEditorPanel("share")}
          onclick={toggleSharePanel}
        >
          <Share2 class="size-4" strokeWidth={noteActionIconStrokeWidth} />
        </button>
        {#if shareOpen}
          {#if panelLoadStates.share?.status === "ready" && panelLoadStates.share.component.kind === "share"}
            {@const NotesSharePopover = panelLoadStates.share.component.component}
            <NotesSharePopover anchor={shareButton} pageTitle={currentPageTitle} {mobileLayout} onClose={() => { shareOpen = false; }} />
          {:else if panelLoadStates.share?.status === "failed"}
            <button class="surface-floating fixed inset-0 z-50 m-auto h-10 px-3" type="button" onclick={() => requestEditorPanel("share", true)}>{t("common.retry")}</button>
          {/if}
        {/if}
        <button
          type="button"
          class={actionButtonClass(pageFavorited)}
          aria-label={pageFavorited ? t("notes.removeFromFavorites") : t("notes.addToFavorites")}
          data-app-tooltip={pageFavorited ? t("notes.removeFromFavorites") : t("notes.addToFavorites")}
          onclick={() => {
            notes.setPageFavorited(page.id, !pageFavorited);
          }}
        >
          <Star class={`size-4 ${pageFavorited ? "fill-current" : ""}`} strokeWidth={noteActionIconStrokeWidth} />
        </button>
        <div
          class="relative"
          use:dismissOnOutside={{ enabled: pageMenuOpen, onDismiss: closePageMenu }}
        >
          <button
            bind:this={pageMenuButton}
            type="button"
            class={actionButtonClass(pageMenuOpen)}
            aria-label={t("notes.pageActions")}
            data-app-tooltip={t("notes.pageActions")}
            aria-expanded={pageMenuOpen}
            onclick={() => {
              pageMenuOpen = !pageMenuOpen;
              closeActionPanel();
              if (pageMenuOpen) coverMenuOpen = false;
              if (!pageMenuOpen) {
                moveMenuOpen = false;
                folderMoveMenuOpen = false;
              }
            }}
          >
            <MoreHorizontal class="size-4" strokeWidth={noteActionIconStrokeWidth} />
          </button>
          {#if pageMenuOpen}
            <div
              class="surface-floating fixed z-80 flex flex-col overflow-hidden"
              style={pageMenuStyle}
              role="menu"
              data-app-floating-surface
            >
              <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto" use:scrollEdgeFadeAction>
                {#if mobileLayout}
                  <button class={menuItemClass()} type="button" role="menuitem" aria-haspopup="dialog" onclick={() => {
                    openIconPicker("action", pageMenuButton);
                    closePageMenu();
                  }}>
                    <SmilePlus class="size-4" strokeWidth={noteActionIconStrokeWidth} />
                    <span>{page.icon ? t("notes.changePageIcon") : t("notes.addPageIcon")}</span>
                  </button>
                  <button class={menuItemClass()} type="button" role="menuitem" aria-haspopup="dialog" disabled={coverPosition !== null} onclick={() => {
                    if (pageMenuButton) openCoverMenu(pageMenuButton);
                  }}>
                    <ImagePlus class="size-4" strokeWidth={noteActionIconStrokeWidth} />
                    <span>{page.cover ? t("notes.changePageCover") : t("notes.addPageCover")}</span>
                  </button>
                  <button class={menuItemClass()} type="button" role="menuitem" onclick={revealInlineComments}>
                    <MessageSquare class="size-4" strokeWidth={noteActionIconStrokeWidth} />
                    <span>{t("notes.addComment")}</span>
                  </button>
                  <div class="menu-separator" role="separator"></div>
                {/if}
                <button class={menuItemClass()} type="button" role="menuitem" onclick={() => focusTitleInput()}>
                  <Pencil class="size-4" />
                  <span>{t("notes.renamePage")}</span>
                </button>
                <button class={menuItemClass()} type="button" role="menuitem" onclick={duplicateCurrentPage}>
                  <Copy class="size-4" />
                  <span>{t("notes.duplicatePage")}</span>
                </button>
                {#if hasPanelMenuItems}
                  <div class="menu-separator" role="separator"></div>
                  {#if linksBadgeCount > 0}
                    <button class={menuItemClass()} type="button" role="menuitem" onclick={() => togglePanel("links")}>
                      <Link2 class="size-4" strokeWidth={noteActionIconStrokeWidth} />
                      <span>{t("notes.noteLinks")}</span>
                      <span class="ml-auto text-panel-detail text-muted-foreground">{linksBadgeCount}</span>
                    </button>
                  {/if}
                  {#if openSuggestionCount > 0}
                    <button class={menuItemClass()} type="button" role="menuitem" onclick={() => togglePanel("suggestions")}>
                      <PencilLine class="size-4" strokeWidth={noteActionIconStrokeWidth} />
                      <span>{t("notes.suggestionsCount", openSuggestionCount)}</span>
                    </button>
                  {/if}
                  <div class="menu-separator" role="separator"></div>
                {/if}
                <button
                  class={menuItemClass()}
                  type="button"
                  role="menuitem"
                  aria-expanded={moveMenuOpen}
                  onclick={() => {
                    moveMenuOpen = !moveMenuOpen;
                    if (moveMenuOpen) {
                      void notes.ensureOptionalSubsystem("destinations").catch((error) => {
                        console.error("load notes move destinations failed", error);
                      });
                    }
                    folderMoveMenuOpen = false;
                  }}
                >
                  <FolderInput class="size-4" />
                  <span>{t("notes.movePageTo")}</span>
                </button>
                {#if moveMenuOpen}
                  <div class="my-1 max-h-72 overflow-auto border-y border-border bg-muted/25 py-1" use:scrollEdgeFadeAction>
                    {#if panelLoadStates["destination-picker"]?.status === "ready" && panelLoadStates["destination-picker"].component.kind === "destination-picker"}
                      {@const NotesDestinationPickerList = panelLoadStates["destination-picker"].component.component}
                      <NotesDestinationPickerList
                      targets={moveTargets}
                      searchLabel={t("notes.moveDestinationSearch")}
                      searchPlaceholder={t("notes.moveDestinationSearchPlaceholder")}
                      recentLabel={t("notes.recentDestinations")}
                      pagesLabel={t("notes.allPages")}
                      emptyLabel={t("notes.noPageMoveTargets")}
                      optionLabel={(target) => t("notes.movePageToTarget", target.title)}
                      onSelect={moveToTarget}
                      onClose={() => {
                        moveMenuOpen = false;
                      }}
                      />
                    {:else if panelLoadStates["destination-picker"]?.status === "failed"}
                      <button class="m-2 min-h-8 rounded-floating-item border border-border px-2 hover:bg-accent" type="button" onclick={() => requestEditorPanel("destination-picker", true)}>{t("common.retry")}</button>
                    {:else}
                      <div class="p-2 text-muted-foreground" aria-busy="true">{t("common.loading")}</div>
                    {/if}
                  </div>
                {/if}
                {#if folderMoveTargets.length > 0}
                  <button
                    class={menuItemClass()}
                    type="button"
                    role="menuitem"
                    aria-expanded={folderMoveMenuOpen}
                    onclick={() => {
                      folderMoveMenuOpen = !folderMoveMenuOpen;
                      if (folderMoveMenuOpen) {
                        void notes.ensureOptionalSubsystem("destinations").catch((error) => {
                          console.error("load notes folder destinations failed", error);
                        });
                      }
                      moveMenuOpen = false;
                    }}
                  >
                    <FolderTree class="size-4" />
                    <span>{t("notes.movePageToFolder")}</span>
                  </button>
                  {#if folderMoveMenuOpen}
                    <div class="my-1 max-h-72 overflow-auto border-y border-border bg-muted/25 py-1" use:scrollEdgeFadeAction>
                      {#if panelLoadStates["destination-picker"]?.status === "ready" && panelLoadStates["destination-picker"].component.kind === "destination-picker"}
                        {@const NotesDestinationPickerList = panelLoadStates["destination-picker"].component.component}
                        <NotesDestinationPickerList
                        targets={folderMoveTargets}
                        searchLabel={t("notes.moveDestinationSearch")}
                        searchPlaceholder={t("notes.moveDestinationSearchPlaceholder")}
                        recentLabel={t("notes.recentDestinations")}
                        pagesLabel={t("notes.folders")}
                        emptyLabel={t("notes.noFolderMoveTargets")}
                        optionLabel={(target) => t("notes.movePageToFolderTarget", target.title)}
                        onSelect={moveToFolderTarget}
                        onClose={() => {
                          folderMoveMenuOpen = false;
                        }}
                        />
                      {:else if panelLoadStates["destination-picker"]?.status === "failed"}
                        <button class="m-2 min-h-8 rounded-floating-item border border-border px-2 hover:bg-accent" type="button" onclick={() => requestEditorPanel("destination-picker", true)}>{t("common.retry")}</button>
                      {:else}
                        <div class="p-2 text-muted-foreground" aria-busy="true">{t("common.loading")}</div>
                      {/if}
                    </div>
                  {/if}
                {/if}
                {#if fileExportAvailable}
                  <button
                    class={menuItemClass()}
                    type="button"
                    role="menuitem"
                    onclick={() => {
                      htmlExportOpen = true;
                      closePageMenu();
                    }}
                  >
                    <Download class="size-4" />
                    <span>{t("notes.htmlExportOpen")}</span>
                  </button>
                  <button
                    class={menuItemClass()}
                    type="button"
                    role="menuitem"
                    onclick={() => {
                      agentBridgeExportOpen = true;
                      closePageMenu();
                    }}
                  >
                    <GitBranch class="size-4" />
                    <span>{t("notes.agentBridgeExportOpen")}</span>
                  </button>
                {/if}
                <button class={menuItemClass()} type="button" role="menuitem" onclick={openPageHistory}>
                  <History class="size-4" />
                  <span>{t("notes.pageHistory")}</span>
                </button>
                <button class={menuItemClass()} type="button" role="menuitem" onclick={archiveCurrentPage}>
                  <Archive class="size-4" />
                  <span>{t("notes.archivePage")}</span>
                </button>
                <button class={menuItemClass(true)} type="button" role="menuitem" onclick={trashCurrentPage}>
                  <Trash2 class="size-4" />
                  <span>{t("notes.moveToTrash")}</span>
                </button>
              </div>
            </div>
          {/if}
        </div>

        {#if activePanel}
          <div
            class="surface-floating fixed z-80 flex min-h-0 flex-col overflow-hidden"
            style={pageDetailsPanelStyle}
            role="dialog"
            aria-label={activePanel === "links" ? t("notes.noteLinks") : t("notes.suggestedEdits")}
            data-app-floating-surface
          >
            {#if activePanel === "links"}
              <div class="min-h-0 flex-1 overflow-auto p-2" use:scrollEdgeFadeAction>
                <div class="flex min-w-0 flex-col gap-2">
                  {#if panelLoadStates.backlinks?.status === "ready" && panelLoadStates.backlinks.component.kind === "backlinks"}
                    {@const NotesBacklinks = panelLoadStates.backlinks.component.component}
                    <NotesBacklinks embedded />
                  {/if}
                  {#if panelLoadStates["page-links"]?.status === "ready" && panelLoadStates["page-links"].component.kind === "page-links"}
                    {@const NotesPageLinks = panelLoadStates["page-links"].component.component}
                    <NotesPageLinks embedded />
                  {/if}
                  {#if panelLoadStates.backlinks?.status === "failed" || panelLoadStates["page-links"]?.status === "failed"}
                    <button class="min-h-8 rounded-floating-item border border-border px-2 hover:bg-accent" type="button" onclick={() => { if (panelLoadStates.backlinks?.status === "failed") requestEditorPanel("backlinks", true); if (panelLoadStates["page-links"]?.status === "failed") requestEditorPanel("page-links", true); }}>{t("common.retry")}</button>
                  {/if}
                </div>
              </div>
            {:else if activePanel === "suggestions"}
              <div class="min-h-0 flex-1 overflow-auto p-2" use:scrollEdgeFadeAction>
                {#if panelLoadStates.suggestions?.status === "ready" && panelLoadStates.suggestions.component.kind === "suggestions"}
                  {@const NotesSuggestions = panelLoadStates.suggestions.component.component}
                  <NotesSuggestions embedded />
                {:else if panelLoadStates.suggestions?.status === "failed"}
                  <button class="min-h-8 rounded-floating-item border border-border px-2 hover:bg-accent" type="button" onclick={() => requestEditorPanel("suggestions", true)}>{t("common.retry")}</button>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}

    {#if notes.pageCreationError}
      <div class="flex shrink-0 items-center justify-between gap-3 border-b border-destructive/30 bg-destructive/10 px-3 py-2 text-[0.8rem] text-destructive" role="alert">
        <span>{t("notes.pageCreationFailed", notes.pageCreationError)}</span>
        <button class="min-h-7 shrink-0 rounded-md border border-destructive/40 px-2 font-medium hover:bg-destructive/10" type="button" onclick={notes.retryPageCreation}>
          {t("common.retry")}
        </button>
      </div>
    {/if}

    <div class="notes-editor-body relative flex min-h-0 flex-1">
    <div
      bind:this={blockScrollViewport}
      data-notes-editor-scroll
      use:editorViewport
      onscroll={(event) => { if (page) notes.rememberEditorScroll(page.id, event.currentTarget.scrollTop); }}
      style="container-type: inline-size;"
      class="min-h-0 min-w-0 flex-1 overflow-x-auto overflow-y-scroll"
      use:documentEndPointer
    >
      {#if page.cover}
        <div bind:this={coverBanner} class="notes-page-banner relative overflow-hidden bg-muted">
          {#if panelLoadStates["page-cover"]?.status === "ready" && panelLoadStates["page-cover"].component.kind === "page-cover"}
            {@const NotesPageCover = panelLoadStates["page-cover"].component.component}
            <NotesPageCover cover={page.cover} unavailableLabel={t("notes.pageCoverUnavailable")}
              focalPoint={coverPosition?.focal} focalLabel={t("notes.pageCoverFocalHint")}
              positioningDisabled={coverPositionSaving}
              onFocalPoint={coverPosition ? (point) => { if (coverPosition && !coverPositionSaving) coverPosition.focal = point; } : undefined}
              onStatus={(status) => { coverStatus = status; }} />
          {:else if panelLoadStates["page-cover"]?.status === "failed"}
            <button class="m-2 min-h-8 rounded-floating-item border border-border bg-popover px-2" type="button" onclick={() => requestEditorPanel("page-cover", true)}>{t("common.retry")}</button>
          {:else}
            <NotesLoadingSkeleton kind="cover" />
          {/if}
          <div
            class="notes-cover-actions absolute right-3 top-3 flex items-center divide-x divide-border overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-sm"
            data-open={coverMenuOpen || coverPosition !== null || undefined}
          >
            {#if coverPosition}
              <button class="min-h-7 px-2 text-[0.733333rem] hover:bg-accent focus-visible:bg-accent disabled:opacity-50" type="button" disabled={coverPositionSaving} onclick={saveCoverPosition}>{t("notes.pageCoverSavePosition")}</button>
              <button class="min-h-7 px-2 text-[0.733333rem] hover:bg-accent focus-visible:bg-accent disabled:opacity-50" type="button" disabled={coverPositionSaving} onclick={cancelCoverPosition}>{t("common.cancel")}</button>
            {:else}
              <button
                class="min-h-7 px-2 text-[0.733333rem] hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
                type="button"
                aria-label={t("notes.changePageCover")}
                aria-haspopup="dialog"
                aria-expanded={coverMenuOpen}
                onclick={(event) => toggleCoverMenu(event)}
              >{t("notes.pageCoverChangeAction")}</button>
              {#if page.cover.type !== "design"}
                <button
                  class="min-h-7 px-2 text-[0.733333rem] hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
                  type="button"
                  aria-label={t("notes.pageCoverReposition")}
                  bind:this={repositionButton}
                  disabled={coverStatus !== "ready"}
                  onclick={startCoverPosition}
                >{t("notes.pageCoverRepositionAction")}</button>
              {/if}
            {/if}
          </div>
          {#if coverPosition}
            <p class="pointer-events-none absolute left-1/2 top-1/2 flex min-h-7 max-w-[calc(100%-1.5rem)] -translate-x-1/2 -translate-y-1/2 items-center rounded-md bg-popover/60 px-2 text-[0.733333rem] text-popover-foreground">{t("notes.pageCoverDragHint")}</p>
          {/if}
        </div>
      {/if}

      {#if coverPositionError}<p class="bg-destructive/10 px-3 py-2 text-[0.8rem] text-destructive" role="alert">{coverPositionError}</p>{/if}

      {#if coverMenuOpen}
        {#if panelLoadStates["cover-menu"]?.status === "ready" && panelLoadStates["cover-menu"].component.kind === "cover-menu"}
          {@const NotesPageCoverMenu = panelLoadStates["cover-menu"].component.component}
          {#key page.id}
            {@const coverPageId = page.id}
            <NotesPageCoverMenu
              cover={page.cover}
              trigger={coverMenuButton}
              onClose={closeCoverMenu}
              onSelect={(cover) => notes.updatePageCover(coverPageId, cover)}
            />
          {/key}
        {:else if panelLoadStates["cover-menu"]?.status === "failed"}
          <button class="min-h-8 rounded-floating-item border border-border px-2 hover:bg-accent" type="button" onclick={() => requestEditorPanel("cover-menu", true)}>{t("common.retry")}</button>
        {/if}
      {/if}

      <div
        class={cn(
          "mx-auto flex w-full max-w-208 flex-col pb-12 pt-3",
          openMode === "side" ? "pl-16 pr-4 sm:pr-8" : "px-4 sm:px-8",
        )}
      >
        <div class="notes-page-title-surface min-w-0 pb-5">
          <div class={cn(
            "notes-page-title-actions -ml-1.5 flex min-h-8 flex-wrap items-center gap-1.5",
            isPeekMode && !page.cover && (discussionsButtonVisible ? "pr-36" : "pr-28"),
          )}>
            {#if panelLoadStates["icon-picker"]?.status === "ready" && panelLoadStates["icon-picker"].component.kind === "icon-picker"}
              {@const IconPicker = panelLoadStates["icon-picker"].component.component}
              <IconPicker
                value={notesPageIconPickerValue(page.icon)}
                ariaLabel={page.icon ? t("notes.changePageIcon") : t("notes.addPageIcon")}
                panelAlign="start"
                panelAnchor={iconPickerPanelAnchor}
                uploadAdapter={notesIconUploadAdapter}
                onChange={updatePageIconFromPicker}
              >
                {#snippet trigger({ open, toggle, panelId })}
                  <button
                    bind:this={actionIconPickerTrigger}
                    class={`inline-flex max-w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-[0.8rem] text-muted-foreground hover:bg-accent hover:text-foreground ${open ? "bg-accent text-foreground" : ""}`}
                    type="button"
                    aria-label={page.icon ? t("notes.changePageIcon") : t("notes.addPageIcon")}
                    aria-haspopup="dialog"
                    aria-expanded={open}
                    aria-controls={panelId}
                    data-notes-icon-picker-open={open ? "true" : undefined}
                    onclick={() => {
                      if (!openingIconPickerFromMenu) iconPickerPanelAnchor = null;
                      prepareIconPicker(toggle);
                    }}
                  >
                    <SmilePlus class="size-3.5" />
                    <span class="truncate">{page.icon ? t("notes.changePageIcon") : t("notes.addPageIcon")}</span>
                  </button>
                {/snippet}
              </IconPicker>
            {:else}
              <button
                class="inline-flex max-w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-[0.8rem] text-muted-foreground hover:bg-accent hover:text-foreground"
                type="button"
                aria-label={page.icon ? t("notes.changePageIcon") : t("notes.addPageIcon")}
                onclick={() => openIconPicker("action")}
              >
                <SmilePlus class="size-3.5" />
                <span class="truncate">{page.icon ? t("notes.changePageIcon") : t("notes.addPageIcon")}</span>
              </button>
            {/if}
            {#if !page.cover}
              <button
                class="inline-flex max-w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-[0.8rem] text-muted-foreground hover:bg-accent hover:text-foreground"
                type="button"
                aria-label={t("notes.addPageCover")}
                aria-haspopup="dialog"
                aria-expanded={coverMenuOpen}
                data-notes-cover-open={coverMenuOpen ? "true" : undefined}
                onclick={(event) => toggleCoverMenu(event)}
              >
                <ImagePlus class="size-3.5" />
                <span class="truncate">{t("notes.addPageCover")}</span>
              </button>
            {/if}
            <button
              class="inline-flex max-w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-[0.8rem] text-muted-foreground hover:bg-accent hover:text-foreground"
              type="button"
              aria-label={t("notes.addComment")}
              aria-controls={inlineCommentsId}
              onclick={revealInlineComments}
            >
              <MessageSquare class="size-3.5" />
              <span class="truncate">{t("notes.addComment")}</span>
            </button>
          </div>

          {#if page.icon}
            <div class="mb-3 inline-flex">
              {#if panelLoadStates["icon-picker"]?.status === "ready" && panelLoadStates["icon-picker"].component.kind === "icon-picker"}
                {@const IconPicker = panelLoadStates["icon-picker"].component.component}
                <IconPicker
                  value={notesPageIconPickerValue(page.icon)}
                  ariaLabel={t("notes.changePageIcon")}
                  panelAlign="start"
                  uploadAdapter={notesIconUploadAdapter}
                  onChange={updatePageIconFromPicker}
                >
                  {#snippet trigger({ open, toggle, panelId })}
                    <button
                      bind:this={pageIconPickerTrigger}
                      class={`flex size-16 items-center justify-center rounded-md text-foreground hover:bg-accent ${open ? "bg-accent" : ""}`}
                      type="button"
                      aria-label={t("notes.changePageIcon")}
                      aria-haspopup="dialog"
                      aria-expanded={open}
                      aria-controls={panelId}
                      data-app-tooltip={t("notes.changePageIcon")}
                      onclick={() => prepareIconPicker(toggle)}
                    >
                      <NotesPageIcon icon={page.icon} size={48} class="shrink-0" />
                      <span class="sr-only">{pageIconLabel}</span>
                    </button>
                  {/snippet}
                </IconPicker>
              {:else}
                <button
                  class="flex size-16 items-center justify-center rounded-md text-foreground hover:bg-accent"
                  type="button"
                  aria-label={t("notes.changePageIcon")}
                  data-app-tooltip={t("notes.changePageIcon")}
                  onclick={() => openIconPicker("icon")}
                >
                  <NotesPageIcon icon={page.icon} size={48} class="shrink-0" />
                  <span class="sr-only">{pageIconLabel}</span>
                </button>
              {/if}
            </div>
          {/if}

          <input
            bind:this={titleInput}
            class="notes-editor-page-title block w-full min-w-0 bg-transparent font-bold leading-[1.2] text-foreground outline-none placeholder:text-muted-foreground"
            aria-label={t("notes.titleInput")}
            bind:value={titleDraft}
            placeholder={t("notes.titlePlaceholder")}
            oninput={handleTitleInput}
            onkeydown={handleTitleKeydown}
            onblur={() => {
              void saveTitle();
            }}
          />
        </div>

        {#if page.parent.type === "data_source_id"}
          {@const rowDataSourceId = page.parent.data_source_id}
          <div class={cn("notes-page-header-extras", inlineCommentsVisible ? "mb-2" : "mb-4 border-b border-border pb-3")}>
            {#if panelLoadStates["row-properties"]?.status === "ready" && panelLoadStates["row-properties"].component.kind === "row-properties"}
              {@const NotesDatabaseRowPageProperties = panelLoadStates["row-properties"].component.component}
              {#key page.id}
                <NotesDatabaseRowPageProperties
                  dataSourceId={rowDataSourceId}
                  pageId={page.id}
                  onOpenPage={(pageId) => {
                    void notes.openPageContextually(pageId).catch((error: unknown) => {
                      console.warn("Open Notes preview failed", error);
                    });
                  }}
                />
              {/key}
            {:else if panelLoadStates["row-properties"]?.status === "failed"}
              <button class="min-h-8 rounded-floating-item border border-border px-2 hover:bg-accent" type="button" onclick={() => requestEditorPanel("row-properties", true)}>{t("common.retry")}</button>
            {/if}
          </div>
        {/if}

        {#if inlineCommentsVisible && inlineCommentsId}
          <div class="notes-page-header-extras mb-4 border-b border-border pb-4">
            {#if panelLoadStates["page-comments"]?.status === "ready" && panelLoadStates["page-comments"].component.kind === "page-comments"}
              {@const NotesPageComments = panelLoadStates["page-comments"].component.component}
              {#key page.id}
                <NotesPageComments
                  id={inlineCommentsId}
                  pageId={page.id}
                  now={activityNow}
                  timeLabels={activityTimeLabels}
                  focusRequest={inlineCommentFocus?.pageId === page.id ? inlineCommentFocus.request : 0}
                  onLeaveEmpty={() => { inlineCommentsRequested = false; }}
                />
              {/key}
            {:else if panelLoadStates["page-comments"]?.status === "failed"}
              <button class="min-h-8 rounded-floating-item border border-border px-2 hover:bg-accent" type="button" onclick={() => requestEditorPanel("page-comments", true)}>{t("common.retry")}</button>
            {/if}
          </div>
        {/if}

        <NotesBlockList
          items={notes.flatBlocks}
          pageId={page.id}
          {breadcrumbItems}
          {tableOfContentsItems}
          onSelectPage={(pageId) => {
            void notes.openPageContextually(pageId).catch((error: unknown) => {
              console.warn("Open Notes preview failed", error);
            });
          }}
          onFocusBlock={(blockId, preventScroll) => {
            notes.focusBlock(blockId, null, preventScroll);
          }}
          scrollViewport={blockScrollViewport}
          {musicMentionContext}
        />
      </div>
    </div>
    {#if discussionsPanelOpen}
      {#if panelLoadStates["discussions-panel"]?.status === "ready" && panelLoadStates["discussions-panel"].component.kind === "discussions-panel"}
        {@const NotesDiscussionsPanel = panelLoadStates["discussions-panel"].component.component}
        <NotesDiscussionsPanel
          pageId={page.id}
          pageTitle={currentPageTitle}
          pageIcon={page.icon}
          now={activityNow}
          timeLabels={activityTimeLabels}
          focus={discussionsPanelFocus}
          mobile={mobileLayout}
          topInset={isPeekMode}
          onClose={closeDiscussionsPanel}
        />
      {:else if panelLoadStates["discussions-panel"]?.status === "failed"}
        <div class="flex w-56 shrink-0 items-start justify-center border-l border-border p-4">
          <button class="min-h-8 rounded-floating-item border border-border px-2 hover:bg-accent" type="button" onclick={() => requestEditorPanel("discussions-panel", true)}>{t("common.retry")}</button>
        </div>
      {/if}
    {/if}
    </div>
  </section>

  {#if htmlExportOpen}
    {#if panelLoadStates["html-export"]?.status === "ready" && panelLoadStates["html-export"].component.kind === "html-export"}
      {@const NotesHtmlExportDialog = panelLoadStates["html-export"].component.component}
      <NotesHtmlExportDialog
      pageTitle={currentPageTitle}
      onExport={exportHtmlArchive}
      onCancel={() => {
        htmlExportOpen = false;
      }}
      />
    {:else if panelLoadStates["html-export"]?.status === "failed"}
      <button class="surface-floating fixed inset-0 z-50 m-auto h-10 px-3" type="button" onclick={() => requestEditorPanel("html-export", true)}>{t("common.retry")}</button>
    {/if}
  {/if}

  {#if agentBridgeExportOpen}
    {#if panelLoadStates["agent-export"]?.status === "ready" && panelLoadStates["agent-export"].component.kind === "agent-export"}
      {@const NotesAgentBridgeExportDialog = panelLoadStates["agent-export"].component.component}
      <NotesAgentBridgeExportDialog
      pageTitle={currentPageTitle}
      onExport={exportAgentBridge}
      onCancel={() => {
        agentBridgeExportOpen = false;
      }}
      />
    {:else if panelLoadStates["agent-export"]?.status === "failed"}
      <button class="surface-floating fixed inset-0 z-50 m-auto h-10 px-3" type="button" onclick={() => requestEditorPanel("agent-export", true)}>{t("common.retry")}</button>
    {/if}
  {/if}

  {#if pageHistoryModalOpen}
    {#if panelLoadStates["page-history"]?.status === "ready" && panelLoadStates["page-history"].component.kind === "page-history"}
      {@const NotesPageVersionHistoryModal = panelLoadStates["page-history"].component.component}
      <NotesPageVersionHistoryModal
      pageId={page.id}
      onClose={() => {
        pageHistoryModalOpen = false;
        notes.setPagePanelSubsystemOpen("page-history", false);
      }}
      />
    {:else if panelLoadStates["page-history"]?.status === "failed"}
      <button class="surface-floating fixed inset-0 z-50 m-auto h-10 px-3" type="button" onclick={() => requestEditorPanel("page-history", true)}>{t("common.retry")}</button>
    {/if}
  {/if}

  {#if pendingArchivePage}
    {#if panelLoadStates["confirm-dialog"]?.status === "ready" && panelLoadStates["confirm-dialog"].component.kind === "confirm-dialog"}
      {@const ConfirmDialog = panelLoadStates["confirm-dialog"].component.component}
      <ConfirmDialog
      title={t("notes.archiveConfirmTitle", notesPageTitle(pendingArchivePage, t("notes.untitled")))}
      message={t("notes.archiveConfirmMessage")}
      confirmLabel={t("notes.archiveConfirm")}
      cancelLabel={t("common.cancel")}
      onConfirm={confirmArchivePage}
      onCancel={() => {
        pendingArchivePage = null;
      }}
      />
    {:else if panelLoadStates["confirm-dialog"]?.status === "failed"}
      <button class="surface-floating fixed inset-0 z-50 m-auto h-10 px-3" type="button" onclick={() => requestEditorPanel("confirm-dialog", true)}>{t("common.retry")}</button>
    {/if}
  {/if}

  {#if pendingTrashPage}
    {#if panelLoadStates["confirm-dialog"]?.status === "ready" && panelLoadStates["confirm-dialog"].component.kind === "confirm-dialog"}
      {@const ConfirmDialog = panelLoadStates["confirm-dialog"].component.component}
      <ConfirmDialog
      title={t("notes.trashConfirmTitle", notesPageTitle(pendingTrashPage, t("notes.untitled")))}
      message={t("notes.trashConfirmMessage")}
      confirmLabel={t("notes.trashConfirm")}
      cancelLabel={t("common.cancel")}
      onConfirm={confirmTrashPage}
      onCancel={() => {
        pendingTrashPage = null;
      }}
      />
    {:else if panelLoadStates["confirm-dialog"]?.status === "failed"}
      <button class="surface-floating fixed inset-0 z-50 m-auto h-10 px-3" type="button" onclick={() => requestEditorPanel("confirm-dialog", true)}>{t("common.retry")}</button>
    {/if}
  {/if}
{:else}
  <div class="flex min-w-0 flex-1 items-center justify-center px-4 text-center text-[0.933333rem] text-muted-foreground">
    {t("notes.emptyEditor")}
  </div>
{/if}

<style>
  .notes-editor-body {
    container: notes-editor-body / inline-size;
  }

  .notes-cover-actions,
  .notes-page-title-actions {
    opacity: 0;
    pointer-events: none;
    --notes-actions-enter-delay: 280ms;
    --notes-actions-enter-duration: 180ms;
    --notes-actions-exit-duration: 90ms;
    transition: opacity var(--notes-actions-exit-duration) ease-out;
  }

  :where(.notes-page-banner:hover) .notes-cover-actions,
  :where(.notes-page-title-surface:hover) .notes-page-title-actions {
    opacity: 1;
    pointer-events: auto;
    transition: opacity var(--notes-actions-enter-duration) ease-out var(--notes-actions-enter-delay);
  }

  .notes-cover-actions:focus-within,
  .notes-cover-actions[data-open="true"],
  .notes-editor-root[data-mobile="true"] .notes-cover-actions,
  :where(.notes-page-title-surface:focus-within) .notes-page-title-actions,
  .notes-page-title-actions:has([data-notes-icon-picker-open="true"], [data-notes-cover-open="true"]),
  .notes-editor-root[data-mobile="true"] .notes-page-title-actions {
    opacity: 1;
    pointer-events: auto;
    transition: none;
  }

  .notes-editor-root[data-mobile="true"] .notes-cover-actions button {
    min-height: 3rem;
  }

  @media (hover: none), (pointer: coarse) {
    .notes-cover-actions,
    .notes-page-title-actions {
      opacity: 1;
      pointer-events: auto;
      transition: none;
    }

    .notes-cover-actions button {
      min-height: 3rem;
    }
  }

  .notes-editor-root[data-mobile="true"] .notes-page-title-actions :global(button) {
    min-height: 3rem;
  }
</style>
