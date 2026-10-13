<script lang="ts">
  import { getContext, tick, untrack } from "svelte";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Search from "@lucide/svelte/icons/search";
  import SquarePen from "@lucide/svelte/icons/square-pen";
  import X from "@lucide/svelte/icons/x";
  import Database from "@lucide/svelte/icons/database";
  import Table2 from "@lucide/svelte/icons/table-2";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import List from "@lucide/svelte/icons/list";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import ChartGantt from "@lucide/svelte/icons/chart-gantt";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import {
    notesHierarchyPickerChildren,
    notesHierarchyPickerTitle,
    notesHierarchyPanelChromeHeight,
    NOTES_HIERARCHY_PANEL_LIST_PADDING,
    NOTES_HIERARCHY_PANEL_ROW_HEIGHT,
    type NotesHierarchyPickerNode,
    type NotesHierarchyParent,
  } from "$lib/notes/navigation/hierarchy-menu";
  import { NOTES_PAGE_CHROME_EMOJI_SCALE } from "$lib/notes/pages/icon";
  import { notesFoldersForProject } from "$lib/notes/navigation/tree";
  import { notesPagesForProject } from "$lib/notes/project-membership";
  import { listNotesDatabaseViews } from "$lib/api/notes";
  import { databaseResource, notesDatabaseSession } from "$lib/notes/database/session.svelte";
  import type { NotesDatabaseView, NotesDatabaseViewKind } from "$lib/notes/types";
  import {
    projectPickerBridgeFrameStyle,
    projectPickerPanelFrameStyle,
    projectPickerSubpanelGeometry,
  } from "$lib/projects/picker-panels";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import { cn, type MaybePromise } from "$lib/utils";
  import { createHoverDelay } from "$lib/utils/menu-aim";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import NotesHierarchyPickerPanel from "./NotesHierarchyPickerPanel.svelte";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";


  interface NotesMobileHierarchyLevel {
    parent: NotesHierarchyParent;
    title: string;
  }

  let {
    projectId,
    parent = { kind: "root" },
    frameStyle,
    className = "fixed",
    zIndexClass = "z-81",
    rootElement = $bindable<HTMLDivElement | undefined>(),
    onPageSelected,
    onLayoutChange = undefined,
    onItemCountChange = undefined,
    onPointerEnter = undefined,
    onPointerLeave = undefined,
    showSearch = true,
    mobileLayout = false,
    title = undefined,
    initialMobileAncestors = [],
    onBack = undefined,
    onClose = undefined,
  }: {
    projectId: string | null;
    parent?: NotesHierarchyParent;
    frameStyle: string;
    className?: string;
    zIndexClass?: string;
    rootElement?: HTMLDivElement | undefined;
    onPageSelected: () => MaybePromise<void>;
    onLayoutChange?: () => void;
    onItemCountChange?: (count: number) => void;
    onPointerEnter?: (event: PointerEvent) => void;
    onPointerLeave?: (event: PointerEvent) => void;
    showSearch?: boolean;
    mobileLayout?: boolean;
    title?: string;
    initialMobileAncestors?: NotesMobileHierarchyLevel[];
    onBack?: () => void;
    onClose?: () => void;
  } = $props();

  const notes = getNotesEditor();
  const { t } = getLocalization();
  const activateNestedMobileBack = getContext<((handle: () => void) => () => void) | undefined>(
    "ganbaru-mobile-nested-back",
  );
  const iconSize = $derived(mobileLayout ? 18 : 13);
  const iconStrokeWidth = 1.6;
  const panelGapPx = 4;
  const panelListPadding = NOTES_HIERARCHY_PANEL_LIST_PADDING;
  const panelRowHeight = NOTES_HIERARCHY_PANEL_ROW_HEIGHT;
  const viewIcons = { table: Table2, board: Columns3, gallery: LayoutGrid, list: List, calendar: CalendarDays, timeline: ChartGantt };

  let search = $state("");
  let isCreatingFolder = $state(false);
  let folderDraft = $state("");
  let folderCreationError = $state<string | null>(null);
  let folderInput = $state<HTMLInputElement | null>(null);
  let activeNodeKey = $state<string | null>(null);
  let activeNodeAnchor = $state<HTMLElement | null>(null);
  const childDelay = createHoverDelay();
  $effect(() => () => childDelay.cancel());
  let childPanelElement = $state<HTMLDivElement | undefined>();
  let childBridgeElement = $state<HTMLDivElement | undefined>();
  let childPanelStyle = $state("");
  let childBridgeStyle = $state("");
  let scrollElement = $state<HTMLElement | undefined>();
  let views = $state<NotesDatabaseView[]>([]);
  let isLoadingViews = $state(false);
  let viewError = $state<string | null>(null);
  let navigationError = $state<string | null>(null);
  let viewRetryCount = $state(0);
  let childMeasuredItemCount = $state<number | null>(null);
  let mobileParent = $state<NotesHierarchyParent>(untrack(() => parent));
  let mobileTitle = $state(untrack(() => title ?? t("notes.noteNavigatorLabel")));
  let mobileAncestors = $state<NotesMobileHierarchyLevel[]>(untrack(() => [...initialMobileAncestors]));
  const requestedPageIds = new Set<string>();

  const effectiveParent = $derived(mobileLayout ? mobileParent : parent);

  const projectPages = $derived.by(() => notesPagesForProject(notes.allPages, projectId));
  const projectFolders = $derived.by(() => notesFoldersForProject(notes.folders, projectId));
  const normalizedSearch = $derived(showSearch ? search.trim().toLocaleLowerCase() : "");
  const allItems = $derived(notesHierarchyPickerChildren(
    projectPages,
    projectFolders,
    notes.navigationDatabases,
    effectiveParent,
    t("notes.untitled"),
    notes.sidebarPageIdsWithChildren,
    views,
  ));
  const items = $derived(allItems.filter((item) => {
    if (!normalizedSearch) return true;
    const title = notesHierarchyPickerTitle(item, t("notes.untitled"));
    return title.toLocaleLowerCase().includes(normalizedSearch);
  }));
  const activeNode = $derived(items.find((item) => item.key === activeNodeKey) ?? null);
  const childParent = $derived.by((): NotesHierarchyParent | null => {
    if (!activeNode?.hasChildren) return null;
    return nodeParent(activeNode);
  });
  const childItemCount = $derived(childMeasuredItemCount ?? (childParent
    ? notesHierarchyPickerChildren(
        projectPages,
        projectFolders,
        notes.navigationDatabases,
        childParent,
        t("notes.untitled"),
        notes.sidebarPageIdsWithChildren,
      ).length
    : 0));

  function currentBounds() {
    const margin = 8;
    const viewportBounds = {
      left: margin,
      right: window.innerWidth - margin,
      top: margin,
      bottom: window.innerHeight - margin,
    };
    const bounds = rootElement?.closest(".notes-view-root")?.getBoundingClientRect();
    if (!bounds) return viewportBounds;
    return {
      left: Math.max(bounds.left, viewportBounds.left),
      right: Math.min(bounds.right, viewportBounds.right),
      top: Math.max(bounds.top, viewportBounds.top),
      bottom: Math.min(bounds.bottom, viewportBounds.bottom),
    };
  }

  function updateChildGeometry(): void {
    if (!activeNodeAnchor || !rootElement || !childParent) return;
    const geometry = projectPickerSubpanelGeometry({
      anchorRect: activeNodeAnchor.getBoundingClientRect(),
      panelRect: rootElement.getBoundingClientRect(),
      bounds: currentBounds(),
      gap: panelGapPx,
      footerHeight: notesHierarchyPanelChromeHeight(childParent, false),
      panelHeight: childPanelElement?.getBoundingClientRect().height || undefined,
      projectCount: Math.max(1, childItemCount),
      visibleRows: null,
      listPadding: panelListPadding,
      rowHeight: panelRowHeight,
    });
    childPanelStyle = projectPickerPanelFrameStyle(geometry.panel);
    childBridgeStyle = projectPickerBridgeFrameStyle(geometry.bridge);
  }

  function activateNode(node: NotesHierarchyPickerNode, target: EventTarget | null): void {
    childDelay.cancel();
    if (!node.hasChildren) {
      activeNodeKey = null;
      activeNodeAnchor = null;
      return;
    }
    if (activeNodeKey !== node.key) childMeasuredItemCount = null;
    activeNodeKey = node.key;
    activeNodeAnchor = target instanceof HTMLElement ? target : null;
    if (node.kind === "page" && !requestedPageIds.has(node.page.id)) {
      requestedPageIds.add(node.page.id);
      void notes.loadNavigationChildren(node.page.id).catch((error: unknown) => {
        requestedPageIds.delete(node.page.id);
        console.error("load Notes hierarchy children failed", error);
      });
    }
    updateChildGeometry();
    void tick().then(updateChildGeometry);
  }

  function nodeParent(node: NotesHierarchyPickerNode): NotesHierarchyParent {
    if (node.kind === "folder") return { kind: "folder", id: node.folder.id };
    if (node.kind === "page") return { kind: "page", id: node.page.id };
    return { kind: "database", id: node.database.id };
  }

  function nodeTitle(node: NotesHierarchyPickerNode): string {
    return notesHierarchyPickerTitle(node, t("notes.untitled"));
  }

  function openMobileChildren(node: NotesHierarchyPickerNode): void {
    if (!node.hasChildren && node.kind !== "folder") return;
    if (node.kind === "page" && !requestedPageIds.has(node.page.id)) {
      requestedPageIds.add(node.page.id);
      void notes.loadNavigationChildren(node.page.id).catch((error: unknown) => {
        requestedPageIds.delete(node.page.id);
        console.error("load Notes hierarchy children failed", error);
      });
    }
    mobileAncestors = [
      ...mobileAncestors,
      { parent: effectiveParent, title: mobileTitle },
    ];
    mobileParent = nodeParent(node);
    mobileTitle = nodeTitle(node);
    search = "";
  }

  function handleMobileBack(): void {
    if (isCreatingFolder) {
      isCreatingFolder = false;
      folderDraft = "";
      folderCreationError = null;
      return;
    }
    if (normalizedSearch) {
      search = "";
      return;
    }
    const previous = mobileAncestors[mobileAncestors.length - 1];
    if (!previous) {
      onBack?.();
      return;
    }
    mobileAncestors = mobileAncestors.slice(0, -1);
    mobileParent = previous.parent;
    mobileTitle = previous.title;
    search = "";
  }

  function boundaryContains(target: EventTarget | null): boolean {
    if (!(target instanceof Node)) return false;
    return Boolean(
      rootElement?.contains(target)
      || activeNodeAnchor?.contains(target)
      || childBridgeElement?.contains(target)
      || childPanelElement?.contains(target),
    );
  }

  function handleBoundaryLeave(event: PointerEvent): void {
    if (boundaryContains(event.relatedTarget)) return;
    childDelay.schedule(() => {
      activeNodeKey = null;
      activeNodeAnchor = null;
    });
    onPointerLeave?.(event);
  }

  function handleBoundaryEnter(event: PointerEvent): void {
    childDelay.cancel();
    onPointerEnter?.(event);
  }

  async function selectPage(pageId: string): Promise<void> {
    await notes.selectPage(pageId, { openMode: "full" });
    await onPageSelected();
  }

  /** Open a database shell or preselect a saved view without adding a view destination. */
  async function selectNode(node: NotesHierarchyPickerNode): Promise<void> {
    navigationError = null;
    try {
      if (node.kind === "page") await selectPage(node.page.id);
      else if (node.kind === "database" || node.kind === "view") {
        const opened = await notes.openDatabase({ pageId: node.database.page_id, blockId: node.database.id }, {
          followSource: false,
          ...(node.kind === "view" ? { viewId: node.view.id } : {}),
        });
        if (opened) await onPageSelected();
        else navigationError = t("notes.databaseUnavailable");
      }
    } catch (error: unknown) {
      navigationError = error instanceof Error ? error.message : String(error);
    }
  }

  async function createPage(): Promise<void> {
    if (effectiveParent.kind === "database") return;
    if (effectiveParent.kind === "page") {
      await notes.createSubpage(effectiveParent.id, "", { openMode: "full" });
    } else {
      await notes.createPage("", {
        projectId,
        folderId: effectiveParent.kind === "folder" ? effectiveParent.id : null,
        openMode: "full",
      });
    }
    await onPageSelected();
  }

  function nextFolderName(): string {
    const parentFolderId = effectiveParent.kind === "folder" ? effectiveParent.id : null;
    const baseName = t("notes.defaultFolderName");
    const siblingNames = new Set(
      projectFolders
        .filter((folder) => folder.parent_folder_id === parentFolderId)
        .map((folder) => folder.name.trim().toLocaleLowerCase()),
    );
    if (!siblingNames.has(baseName.toLocaleLowerCase())) return baseName;
    let suffix = 2;
    while (siblingNames.has(`${baseName} ${suffix}`.toLocaleLowerCase())) suffix += 1;
    return `${baseName} ${suffix}`;
  }

  function beginFolderCreation(): void {
    folderDraft = nextFolderName();
    folderCreationError = null;
    isCreatingFolder = true;
    void tick().then(() => {
      folderInput?.focus();
      folderInput?.select();
    });
  }

  async function createFolder(): Promise<void> {
    if (!projectId || (effectiveParent.kind !== "root" && effectiveParent.kind !== "folder")) return;
    const name = folderDraft.trim();
    if (!name) return;
    folderCreationError = null;
    try {
      await notes.createFolder(
        projectId,
        name,
        effectiveParent.kind === "folder" ? effectiveParent.id : null,
      );
      isCreatingFolder = false;
      folderDraft = "";
    } catch (error) {
      folderCreationError = error instanceof Error ? error.message : String(error);
    }
  }

  $effect(() => {
    const database = effectiveParent.kind === "database"
      ? notes.navigationDatabases.find((item) => item.id === effectiveParent.id)
      : null;
    notesDatabaseSession.revision;
    viewRetryCount;
    views = [];
    viewError = null;
    isLoadingViews = false;
    if (!database) return;
    const resource = databaseResource("views", database.data_source_id, { databaseId: database.id });
    const cached = untrack(() => notesDatabaseSession.read(resource));
    views = cached ?? [];
    isLoadingViews = cached === null;
    let cancelled = false;
    void untrack(() => notesDatabaseSession.load(resource, () => listNotesDatabaseViews(database.id))).then((loaded) => {
      if (!cancelled) views = loaded;
    }).catch((error: unknown) => {
      if (!cancelled) viewError = error instanceof Error ? error.message : String(error);
    }).finally(() => { if (!cancelled) isLoadingViews = false; });
    return () => { cancelled = true; };
  });

  $effect(() => {
    if (!normalizedSearch) return;
    activeNodeKey = null;
    activeNodeAnchor = null;
  });

  $effect(() => {
    if (
      !mobileLayout
      || (!onBack && mobileAncestors.length === 0 && !isCreatingFolder && !normalizedSearch)
    ) return;
    return activateNestedMobileBack?.(handleMobileBack);
  });

  $effect(() => {
    const itemCount = items.length;
    onItemCountChange?.(itemCount);
    childItemCount;
    requestAnimationFrame(() => {
      onLayoutChange?.();
      updateChildGeometry();
    });
  });

  $effect(() => {
    if (mobileLayout || !rootElement || !onLayoutChange) return;
    const observer = new ResizeObserver(() => onLayoutChange?.());
    observer.observe(rootElement);
    return () => observer.disconnect();
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={rootElement}
  class={cn(
    "project-picker-panel surface-floating flex min-h-0 flex-col overflow-hidden",
    mobileLayout && "h-full rounded-2xl",
    className,
    zIndexClass,
  )}
  style={mobileLayout ? frameStyle : `${frameStyle}; height: auto`}
  onpointerenter={handleBoundaryEnter}
  onpointerleave={handleBoundaryLeave}
>
  {#if mobileLayout || showSearch}
  <div class={cn("shrink-0", mobileLayout ? "border-b border-border/70 p-2" : "px-1.5 pt-1.5")}>
    {#if mobileLayout}
      <div class="flex min-h-14 items-center gap-1">
        {#if onBack || mobileAncestors.length > 0}
          <button
            type="button"
            class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
            aria-label={mobileAncestors.length > 0 ? t("notes.backToPages") : t("projects.navigator.backToProjects")}
            onclick={handleMobileBack}
          >
            <ChevronLeft size={22} strokeWidth={iconStrokeWidth} aria-hidden="true" />
          </button>
        {/if}
        <h2 class="min-w-0 flex-1 truncate px-2 text-base font-semibold">{mobileTitle}</h2>
        {#if onClose}
          <button
            type="button"
            data-project-picker-initial-focus="true"
            class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
            aria-label={t("common.close")}
            onclick={onClose}
          >
            <X size={22} strokeWidth={iconStrokeWidth} aria-hidden="true" />
          </button>
        {/if}
      </div>
    {/if}
    {#if showSearch}
    <div class={cn(
      "field flex items-center gap-1.5",
      mobileLayout && "min-h-12 rounded-xl px-3",
    )}>
      <Search size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0 text-popover-foreground/60" />
      <input
        bind:value={search}
        placeholder={t(effectiveParent.kind === "database" ? "notes.databaseSearchViews" : "notes.searchPlaceholder")}
        aria-label={t(effectiveParent.kind === "database" ? "notes.databaseSearchViews" : "notes.searchLabel")}
        class={cn(
          "field-bare text-popover-foreground",
          mobileLayout && "h-12 text-base",
        )}
      />
      {#if mobileLayout && normalizedSearch}
        <button
          type="button"
          class="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl text-popover-foreground/60 active:bg-accent"
          aria-label={t("notes.clearSearch")}
          onclick={() => { search = ""; }}
        >
          <X size={18} strokeWidth={iconStrokeWidth} aria-hidden="true" />
        </button>
      {/if}
    </div>
    {/if}
  </div>
  {/if}
  <div class="relative flex min-h-0 flex-auto flex-col">
    <div bind:this={scrollElement} use:scrollEdgeFadeAction class={cn("hide-scrollbar min-h-0 flex-auto overflow-y-auto", mobileLayout ? "overscroll-contain px-2 py-2" : "surface-floating-body")}>
      {#if navigationError || viewError}
        <Notice class="px-3 py-2" message={navigationError ?? viewError ?? ""} onRetry={viewError ? () => { viewRetryCount += 1; } : undefined} />
      {:else if isLoadingViews || (notes.loading && projectPages.length === 0 && projectFolders.length === 0)}
        <Loading layout="rows" rows={3} label={t("notes.loading")} />
      {:else if notes.loadError}
        <Notice class="px-3 py-2" message={t("notes.loadFailed", notes.loadError)} copy={notes.loadError} onRetry={() => notes.load()} />
      {:else if items.length === 0}
        <div class="px-3 py-2 text-popover-foreground/60">
          {normalizedSearch ? t("notes.noSearchResults") : t(effectiveParent.kind === "database" ? "notes.databaseNoViews" : "notes.noPages")}
        </div>
      {:else}
        <div class="grid">
          {#each items as item (item.key)}
            {@const title = nodeTitle(item)}
            {#if mobileLayout}
              <div class={cn("flex min-h-12 items-stretch rounded-floating-item", activeNodeKey === item.key && "bg-accent text-accent-foreground")}>
                <button
                  type="button"
                  class="flex min-w-0 flex-1 items-center gap-2 rounded-floating-item px-3 text-left text-sm text-popover-foreground active:bg-accent"
                  aria-label={title}
                  onclick={() => {
                    if (item.kind === "folder") openMobileChildren(item);
                    else void selectNode(item);
                  }}
                >
                  {#if item.kind === "folder"}
                    <Folder size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
                  {:else if item.kind === "database"}
                    <Database size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
                  {:else if item.kind === "view"}
                    {@const Icon = viewIcons[item.view.type as NotesDatabaseViewKind]}
                    <Icon size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
                  {:else if item.page.icon}
                    <NotesPageIcon
                      icon={item.page.icon}
                      size={iconSize}
                      strokeWidth={iconStrokeWidth}
                      emojiScale={NOTES_PAGE_CHROME_EMOJI_SCALE}
                      class="shrink-0"
                    />
                  {:else}
                    <FileText size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
                  {/if}
                  <span class="min-w-0 flex-1 truncate">{title}</span>
                  {#if item.kind === "folder"}
                    <ChevronRight size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0 text-popover-foreground/45" />
                  {/if}
                </button>
                {#if item.kind !== "folder" && item.hasChildren}
                  <button
                    type="button"
                    class="flex w-12 shrink-0 items-center justify-center rounded-floating-item text-popover-foreground/60 active:bg-accent"
                    aria-label={title}
                    onclick={() => openMobileChildren(item)}
                  >
                    <ChevronRight size={iconSize} strokeWidth={iconStrokeWidth} aria-hidden="true" />
                  </button>
                {/if}
              </div>
            {:else}
              <button
                type="button"
                class="menu-item text-popover-foreground"
                aria-expanded={item.hasChildren ? activeNodeKey === item.key : undefined}
                aria-label={title}
                onpointerenter={(event) => activateNode(item, event.currentTarget)}
                onfocus={(event) => activateNode(item, event.currentTarget)}
                onclick={(event) => {
                  if (item.kind === "folder") activateNode(item, event.currentTarget);
                  else void selectNode(item);
                }}
              >
                {#if item.kind === "folder"}
                  <Folder size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
                {:else if item.kind === "database"}
                  <Database size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
                {:else if item.kind === "view"}
                  {@const Icon = viewIcons[item.view.type as NotesDatabaseViewKind]}
                  <Icon size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
                {:else if item.page.icon}
                  <NotesPageIcon
                    icon={item.page.icon}
                    size={iconSize}
                    strokeWidth={iconStrokeWidth}
                    emojiScale={NOTES_PAGE_CHROME_EMOJI_SCALE}
                    class="shrink-0"
                  />
                {:else}
                  <FileText size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
                {/if}
                <span class="min-w-0 flex-1 truncate">{title}</span>
                {#if item.hasChildren}
                  <ChevronRight size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0 text-popover-foreground/45" />
                {/if}
              </button>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
    {#if !mobileLayout}
      <CalendarScrollbar
        scrollContainer={scrollElement}
        stickyTop={4}
        stickyBottom={4}
        wheelPassthrough
      />
    {/if}
  </div>

  {#if effectiveParent.kind !== "database"}
  <div class={cn("relative z-10 shrink-0 bg-popover", mobileLayout ? "p-2" : "p-1.5")}>
    <div class="pointer-events-none absolute left-1.5 right-1.5 top-0 border-t border-border/70"></div>
    {#if isCreatingFolder && effectiveParent.kind !== "page"}
      <form class={cn("flex", mobileLayout ? "gap-2" : "gap-1")} onsubmit={(event) => { event.preventDefault(); void createFolder(); }}>
        <input
          bind:this={folderInput}
          bind:value={folderDraft}
          placeholder={t("notes.defaultFolderName")}
          aria-label={t("notes.newFolder")}
          class={cn(
            "field min-w-0 flex-1 text-popover-foreground",
            mobileLayout && "min-h-12 rounded-xl px-3 text-base",
          )}
        />
        <button type="submit" class={cn("bg-primary font-medium text-primary-foreground", mobileLayout ? "min-h-12 rounded-xl px-4 text-sm" : "min-h-8 rounded-floating-item px-2 text-panel-detail")}>
          {t("common.save")}
        </button>
        {#if mobileLayout}
          <button
            type="button"
            class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
            aria-label={t("common.cancel")}
            onclick={() => {
              isCreatingFolder = false;
              folderDraft = "";
              folderCreationError = null;
            }}
          >
            <X size={18} strokeWidth={iconStrokeWidth} aria-hidden="true" />
          </button>
        {/if}
      </form>
    {:else}
      <div class={cn("flex items-center", mobileLayout ? "min-h-12" : "min-h-8")}>
        {#if effectiveParent.kind !== "page"}
          <button
            type="button"
            class={cn("flex min-w-0 flex-1 items-center justify-center gap-1.5 px-1 text-popover-foreground transition-colors hover:bg-accent hover:text-accent-foreground", mobileLayout ? "min-h-12 rounded-xl text-sm active:bg-accent" : "min-h-8 rounded-floating-item")}
            onclick={beginFolderCreation}
          >
            <FolderPlus size={iconSize} strokeWidth={iconStrokeWidth} />
            <span class="truncate">{t("notes.newFolder")}</span>
          </button>
          <div class="mx-1 h-5 border-l border-border/70" aria-hidden="true"></div>
        {/if}
        <button
          type="button"
          class={cn("flex min-w-0 flex-1 items-center justify-center gap-1.5 px-1 text-popover-foreground transition-colors hover:bg-accent hover:text-accent-foreground", mobileLayout ? "min-h-12 rounded-xl text-sm active:bg-accent" : "min-h-8 rounded-floating-item")}
          aria-label={t("notes.newPage")}
          aria-keyshortcuts="Control+N Meta+N"
          onclick={() => { void createPage(); }}
        >
          <SquarePen size={iconSize} strokeWidth={iconStrokeWidth} />
          <span class="truncate">{t("notes.newPage")}</span>
        </button>
      </div>
    {/if}
    {#if folderCreationError}
      <Notice class="px-1 pt-1 text-panel-detail" message={t("notes.folderActionFailed", folderCreationError)} copy={folderCreationError} />
    {/if}
  </div>
  {/if}
</div>

{#if !mobileLayout && childParent && activeNodeAnchor}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={childBridgeElement}
    aria-hidden="true"
    class={cn("fixed bg-transparent", zIndexClass)}
    style={childBridgeStyle}
    onpointerenter={handleBoundaryEnter}
    onpointerleave={handleBoundaryLeave}
  ></div>
  <NotesHierarchyPickerPanel
    bind:rootElement={childPanelElement}
    {projectId}
    parent={childParent}
    showSearch={false}
    frameStyle={childPanelStyle}
    {zIndexClass}
    {onPageSelected}
    onItemCountChange={(count) => { childMeasuredItemCount = count; }}
    onLayoutChange={updateChildGeometry}
    onPointerEnter={handleBoundaryEnter}
    onPointerLeave={handleBoundaryLeave}
  />
{/if}

<style>
  .project-picker-panel {
    --cal-scrollbar-thumb: color-mix(in srgb, var(--popover-foreground) 18%, var(--popover));
    --cal-scrollbar-thumb-hover: color-mix(in srgb, var(--popover-foreground) 36%, var(--popover));
  }
</style>
