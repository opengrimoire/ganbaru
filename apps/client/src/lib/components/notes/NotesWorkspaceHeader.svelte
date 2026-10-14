<script lang="ts">
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import Database from "@lucide/svelte/icons/database";
  import Folder from "@lucide/svelte/icons/folder";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import {
    COMPACT_IDENTITY_EMOJI_SCALE,
    COMPACT_IDENTITY_ICON_SIZE,
    COMPACT_IDENTITY_ICON_STROKE_WIDTH,
  } from "$lib/icon-sizing";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { NOTES_PAGE_CHROME_EMOJI_SCALE } from "$lib/notes/pages/icon";
  import {
    notesHierarchyNodeParent,
    notesHierarchyPath,
    type NotesHierarchyNode,
    type NotesHierarchyParent,
  } from "$lib/notes/navigation/hierarchy-menu";
  import { notesFoldersForProject } from "$lib/notes/navigation/tree";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import { notesPagesForProject } from "$lib/notes/project-membership";
  import type { NotesPage } from "$lib/notes/types";
  import {
    projectNavigatorPanelGeometry,
    type ProjectNavigatorPanelMode,
  } from "$lib/projects/toolbar";
  import type { Project, ProjectGroup } from "$lib/projects/types";
  import { getNotes } from "$lib/stores/notes.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { cn } from "$lib/utils";
  import { createHoverMenu } from "$lib/utils/menu-aim";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import WorkspaceBreadcrumbTerminalIcon from "$lib/components/ui/WorkspaceBreadcrumbTerminalIcon.svelte";
  import {
    loadNotesOptionalComponent,
    retryNotesOptionalComponent,
    type LoadedNotesOptionalComponent,
  } from "./component-registry";
  import NotesHierarchyPickerPanel from "$lib/components/notes/navigation/NotesHierarchyPickerPanel.svelte";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import NotesProjectNavigator from "$lib/components/notes/navigation/NotesProjectNavigator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import MobileContextSwitcher from "$lib/components/mobile/MobileContextSwitcher.svelte";

  type NotesNavigatorMode = ProjectNavigatorPanelMode | "notes";

  let {
    mobileLayout = false,
    selectedProject,
    selectedGroup,
    selectedProjectId,
    selectedPage,
    creationFolderId,
    showInactiveProjects,
    onShowInactiveProjectsChange,
    onProjectSelected,
    onShowHome,
    onOpenExplorer = undefined,
    projectSettingsOpen,
    onToggleProjectSettings,
    pageActionsTarget = $bindable<HTMLElement | null>(null),
  }: {
    mobileLayout?: boolean;
    selectedProject: Project | undefined;
    selectedGroup: ProjectGroup | undefined;
    selectedProjectId: string | null;
    selectedPage: NotesPage | null;
    creationFolderId: string | null;
    showInactiveProjects: boolean;
    onShowInactiveProjectsChange: (value: boolean) => void;
    onProjectSelected: () => void;
    onShowHome: () => void;
    /** Opens the phone explorer panel over an open note. */
    onOpenExplorer?: () => void;
    projectSettingsOpen: boolean;
    onToggleProjectSettings: () => void;
    pageActionsTarget?: HTMLElement | null;
  } = $props();

  const notes = getNotes();
  const projects = getProjects();
  const viewport = getViewport();
  const { t } = getLocalization();
  const identityIconSize = COMPACT_IDENTITY_ICON_SIZE;
  const identityIconStrokeWidth = COMPACT_IDENTITY_ICON_STROKE_WIDTH;
  const projectIdentityEmojiScale = COMPACT_IDENTITY_EMOJI_SCALE;
  const newPageShortcut = $derived(formatShortcut("Mod + N"));
  const newPageTitle = $derived(`${t("notes.newPage")} (${newPageShortcut})`);

  let navigatorOpen = $state(false);
  let navigatorMode = $state<NotesNavigatorMode>("groups");
  let notesNavigatorParent = $state<NotesHierarchyParent>({ kind: "root" });
  let notesNavigatorTitle = $state("");
  let notesNavigatorAncestors = $state<Array<{
    parent: NotesHierarchyParent;
    title: string;
  }>>([]);
  let notesNavigatorSourceKey = $state("root");
  let notesHeaderElement = $state<HTMLDivElement | null>(null);
  let notesIdentityElement = $state<HTMLDivElement | null>(null);
  let navigatorAnchorElement = $state<HTMLButtonElement | null>(null);
  let groupTriggerElement = $state<HTMLButtonElement | null>(null);
  let projectTriggerElement = $state<HTMLButtonElement | null>(null);
  let noteTriggerElement = $state<HTMLButtonElement | null>(null);
  let contextSwitcherElement = $state<HTMLButtonElement | null>(null);
  let navigatorPanelElement = $state<HTMLDivElement | null>(null);
  let navigatorPanelStyle = $state("");
  let navigatorPanelMaxHeight = $state(0);
  const navigatorHover = createHoverMenu({
    isOpen: () => navigatorOpen,
    close: () => {
      navigatorOpen = false;
    },
  });
  $effect(() => {
    if (!navigatorOpen) navigatorHover.cancel();
  });
  $effect(() => () => navigatorHover.cancel());
  let mobileProjectPickerLoadState = $state<LazyComponentLoadState<
    "mobile-project-picker",
    LoadedNotesOptionalComponent
  > | null>(null);
  interface NavigatorBounds {
    left: number;
    right: number;
    top: number;
    bottom: number;
  }

  const selectedPageTitle = $derived.by(() => {
    if (notes.viewMode === "archive") return t("notes.archive");
    if (notes.viewMode === "trash") return t("notes.trash");
    if (!selectedPage) return null;
    const draftTitle = notes.pageTitleDraftForPage(selectedPage.id);
    if (draftTitle !== null) {
      const trimmedDraftTitle = draftTitle.trim();
      return trimmedDraftTitle.length > 0 ? trimmedDraftTitle : t("notes.untitled");
    }
    return notesPageTitle(selectedPage, t("notes.untitled"));
  });
  const selectedPageId = $derived(selectedPage?.id ?? null);
  const selectedDatabaseBlock = $derived(notes.selectedDatabaseBlock);
  const databaseSelected = $derived(notes.viewMode === "pages" && notes.selectedDatabaseBlockId != null);
  /** The mobile action row only carries Back and page actions, so it collapses on the project explorer. */
  const mobileActionRowVisible = $derived(mobileLayout && (selectedPageTitle !== null || databaseSelected));
  const selectedDatabaseTitle = $derived(selectedDatabaseBlock?.child_database.title.trim() || t("notes.untitled"));
  const selectedProjectPages = $derived.by(() => notesPagesForProject(
    selectedPage ? [...notes.navigationPages.filter((page) => page.id !== selectedPage.id), selectedPage] : notes.navigationPages,
    selectedProjectId,
  ));
  const selectedProjectFolders = $derived.by(() => notesFoldersForProject(
    notes.folders,
    selectedProjectId,
  ));
  const selectedPagePath = $derived(notesHierarchyPath(
    selectedPageId,
    selectedProjectPages,
    selectedProjectFolders,
    notes.sidebarPageIdsWithChildren,
  ));
  const showSelectedPagePath = $derived(selectedPagePath.length > 0);

  function toolbarIconButtonClass(active = false, open = false, primary = false): string {
    return cn(
      "flex shrink-0 items-center justify-center rounded-md transition-colors",
      mobileLayout ? "h-12 w-(--mobile-chrome-column)" : "h-7 w-7",
      primary
        ? "bg-primary text-primary-foreground hover:bg-primary/90"
        : "hover:bg-accent",
      !primary && "text-foreground",
      !primary && (active || open) && "bg-accent",
    );
  }

  function inlineNewPageButtonClass(): string {
    return cn(
      "flex shrink-0 items-center justify-center rounded-md text-foreground transition-colors",
      mobileLayout ? "h-12 w-(--mobile-chrome-column)" : "h-7 w-7",
      "hover:bg-accent",
    );
  }

  function navigatorBounds(): NavigatorBounds {
    const boundsElement = notesHeaderElement?.closest(".notes-view-root");
    const rect = boundsElement?.getBoundingClientRect();
    if (rect) {
      return {
        left: rect.left,
        right: rect.right,
        top: rect.top,
        bottom: rect.bottom,
      };
    }

    return {
      left: 0,
      right: viewport.width,
      top: 0,
      bottom: viewport.height,
    };
  }

  function refreshNavigatorPanelGeometry(): void {
    if (mobileLayout) {
      navigatorPanelStyle = "";
      navigatorPanelMaxHeight = 0;
      return;
    }
    if (!navigatorOpen || !navigatorAnchorElement) return;
    const rect = navigatorAnchorElement.getBoundingClientRect();
    const bounds = navigatorBounds();
    const geometry = projectNavigatorPanelGeometry({
      anchorLeft: rect.left,
      anchorBottom: rect.bottom,
      viewportWidth: viewport.width,
      viewportHeight: viewport.height,
      boundsLeft: bounds.left,
      boundsRight: bounds.right,
      boundsTop: bounds.top,
      boundsBottom: bounds.bottom,
    });
    navigatorPanelStyle = [
      `left: ${Math.round(geometry.left)}px`,
      `top: ${Math.round(geometry.top)}px`,
      `width: ${Math.round(geometry.width)}px`,
    ].join("; ");
    navigatorPanelMaxHeight = geometry.height;
  }

  function triggerForMode(mode: NotesNavigatorMode): HTMLButtonElement | null {
    if (mode === "groups") return groupTriggerElement;
    if (mode === "projects") return projectTriggerElement;
    return noteTriggerElement ?? projectTriggerElement;
  }

  function openNavigator(
    mode: NotesNavigatorMode,
    anchor: HTMLButtonElement | null = triggerForMode(mode),
  ): void {
    navigatorMode = mode;
    navigatorAnchorElement = anchor;
    navigatorOpen = true;
    if (mobileLayout) requestMobileProjectPicker();
    refreshNavigatorPanelGeometry();
    requestAnimationFrame(refreshNavigatorPanelGeometry);
  }

  function openHierarchyNavigator(
    node: NotesHierarchyNode,
    anchor: EventTarget | null,
  ): void {
    notesNavigatorParent = notesHierarchyNodeParent(node);
    const nodeIndex = selectedPagePath.findIndex((candidate) => candidate.key === node.key);
    const precedingNodes = nodeIndex < 0 ? [] : selectedPagePath.slice(0, nodeIndex);
    notesNavigatorTitle = precedingNodes.length > 0
      ? hierarchyNodeTitle(precedingNodes[precedingNodes.length - 1])
      : selectedProject?.name ?? t("notes.noteNavigatorLabel");
    notesNavigatorAncestors = precedingNodes.length === 0
      ? []
      : [
          {
            parent: { kind: "root" },
            title: selectedProject?.name ?? t("notes.noteNavigatorLabel"),
          },
          ...precedingNodes.slice(0, -1).map((ancestor) => ({
            parent: ancestor.kind === "folder"
              ? { kind: "folder" as const, id: ancestor.folder.id }
              : { kind: "page" as const, id: ancestor.page.id },
            title: hierarchyNodeTitle(ancestor),
          })),
        ];
    notesNavigatorSourceKey = node.key;
    openNavigator("notes", anchor instanceof HTMLButtonElement ? anchor : null);
  }

  /** Offer the selected database's saved views without adding a view breadcrumb. */
  function openDatabaseNavigator(anchor: EventTarget | null, toggle = false): void {
    const id = notes.selectedDatabaseBlockId;
    if (!id) return;
    const key = `database:${id}`;
    if (toggle) {
      const triggerPanelOpen = navigatorOpen && navigatorMode === "notes" && notesNavigatorSourceKey === key;
      if (navigatorHover.claimClick(triggerPanelOpen)) return;
      if (triggerPanelOpen) {
        navigatorOpen = false;
        return;
      }
    }
    notesNavigatorParent = { kind: "database", id };
    notesNavigatorTitle = selectedDatabaseTitle;
    notesNavigatorAncestors = [
      { parent: { kind: "root" }, title: selectedProject?.name ?? t("notes.noteNavigatorLabel") },
      ...selectedPagePath.map((node) => ({
        parent: node.kind === "folder" ? { kind: "folder" as const, id: node.folder.id } : { kind: "page" as const, id: node.page.id },
        title: hierarchyNodeTitle(node),
      })),
    ];
    notesNavigatorSourceKey = key;
    openNavigator("notes", anchor instanceof HTMLButtonElement ? anchor : null);
  }

  function hierarchyNodeTitle(node: NotesHierarchyNode): string {
    return node.kind === "folder"
      ? node.folder.name
      : notesPageTitle(node.page, t("notes.untitled"));
  }

  function toggleHierarchyNavigator(
    node: NotesHierarchyNode,
    anchor: EventTarget | null,
  ): void {
    const triggerPanelOpen = navigatorOpen && navigatorMode === "notes" && notesNavigatorSourceKey === node.key;
    if (navigatorHover.claimClick(triggerPanelOpen)) return;
    if (triggerPanelOpen) {
      navigatorOpen = false;
      return;
    }
    openHierarchyNavigator(node, anchor);
  }

  /** Return a database breadcrumb's note ancestor to its existing editor pane. */
  function openNoteAncestor(node: NotesHierarchyNode, anchor: EventTarget | null): void {
    if (!databaseSelected || node.kind !== "page") {
      toggleHierarchyNavigator(node, anchor);
      return;
    }
    navigatorOpen = false;
    if (node.page.id === selectedPageId) {
      notes.closeDatabase();
      return;
    }
    void notes.selectPage(node.page.id).catch((error: unknown) => {
      console.warn("Open Notes breadcrumb ancestor failed", error);
    });
  }

  function toggleNavigator(mode: NotesNavigatorMode): void {
    const triggerPanelOpen = navigatorOpen && navigatorMode === mode;
    if (navigatorHover.claimClick(triggerPanelOpen)) return;
    if (triggerPanelOpen) {
      navigatorOpen = false;
      return;
    }
    openNavigator(mode);
  }

  /** Opens the phone navigator where the user is: the open note's siblings, or the project list from project home. */
  function toggleMobileContextNavigator(): void {
    if (navigatorOpen) {
      navigatorOpen = false;
      return;
    }
    const currentNode = selectedPagePath.at(-1);
    if (currentNode) {
      openHierarchyNavigator(currentNode, contextSwitcherElement);
    } else if (selectedProject) {
      openNavigator("projects", contextSwitcherElement);
    } else {
      notesNavigatorParent = { kind: "root" };
      notesNavigatorTitle = t("notes.noteNavigatorLabel");
      notesNavigatorAncestors = [];
      notesNavigatorSourceKey = "root";
      openNavigator("notes", contextSwitcherElement);
    }
  }

  function closeMobileDatabase(): void {
    navigatorOpen = false;
    notes.closeDatabase();
  }

  function handleProjectTriggerClick(): void {
    if (selectedPageTitle) {
      navigatorOpen = false;
      onShowHome();
      return;
    }
    toggleNavigator("projects");
  }

  function handleWindowPointerDown(event: PointerEvent): void {
    if (mobileLayout) return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (
      navigatorOpen
      && !notesIdentityElement?.contains(target)
      && !navigatorPanelElement?.contains(target)
    ) {
      navigatorOpen = false;
    }
  }

  function createPage(): void {
    navigatorOpen = false;
    void notes.createPage("", {
      projectId: selectedProjectId,
      folderId: creationFolderId,
      openMode: "full",
    });
  }

  async function openMobileProject(project: Project): Promise<void> {
    await projects.selectProject(project.id);
    onProjectSelected();
    notesNavigatorParent = { kind: "root" };
    notesNavigatorTitle = project.name;
    notesNavigatorAncestors = [];
    navigatorMode = "notes";
  }

  function requestMobileProjectPicker(): void {
    if (
      mobileProjectPickerLoadState?.status === "loading"
      || mobileProjectPickerLoadState?.status === "ready"
    ) return;
    const shouldRetry = mobileProjectPickerLoadState?.status === "failed";
    const loadingState = beginLazyComponentLoad(
      mobileProjectPickerLoadState,
      "mobile-project-picker",
    );
    mobileProjectPickerLoadState = loadingState;
    const request = shouldRetry
      ? retryNotesOptionalComponent("mobile-project-picker")
      : loadNotesOptionalComponent("mobile-project-picker");
    void request.then((component) => {
      if (!mobileProjectPickerLoadState) return;
      mobileProjectPickerLoadState = resolveLazyComponentLoad(
        mobileProjectPickerLoadState,
        "mobile-project-picker",
        loadingState.requestId,
        component,
      );
    }).catch((error: unknown) => {
      if (!mobileProjectPickerLoadState) return;
      mobileProjectPickerLoadState = rejectLazyComponentLoad(
        mobileProjectPickerLoadState,
        "mobile-project-picker",
        loadingState.requestId,
        error,
      );
      console.error("load Notes mobile project picker failed", error);
    });
  }

  $effect(() => {
    if (!navigatorOpen) return;
    const viewportWidth = viewport.width;
    const viewportHeight = viewport.height;
    void viewportWidth;
    void viewportHeight;
    requestAnimationFrame(refreshNavigatorPanelGeometry);
  });
</script>

{#snippet mobileHeader()}
  <MobileContextSwitcher
    bind:triggerElement={contextSwitcherElement}
    title={selectedProject?.name ?? t("notes.title")}
    expanded={navigatorOpen}
    onclick={toggleMobileContextNavigator}
  >
    {#snippet icon()}
      {#if selectedProject}
        <ProjectIcon
          name={selectedProject.icon}
          size={identityIconSize}
          strokeWidth={identityIconStrokeWidth}
          emojiScale={projectIdentityEmojiScale}
        />
      {:else}
        <NotesPageIcon
          icon={null}
          size={identityIconSize}
          strokeWidth={identityIconStrokeWidth}
          emojiScale={NOTES_PAGE_CHROME_EMOJI_SCALE}
        />
      {/if}
    {/snippet}
  </MobileContextSwitcher>
  <div class="flex h-12 min-w-0 max-w-full items-center gap-0 text-identity font-medium">
    {#if (selectedPageTitle || databaseSelected) && onOpenExplorer}
      <button
        type="button"
        class={toolbarIconButtonClass()}
        aria-label={t("notes.expandExplorerSidebar")}
        title={t("notes.expandExplorerSidebar")}
        data-notes-mobile-sidebar-button
        onclick={() => {
          navigatorOpen = false;
          onOpenExplorer();
        }}
      >
        <PanelLeft size={16} strokeWidth={1.75} />
      </button>
    {/if}
    {#if databaseSelected}
      <button
        type="button"
        class={toolbarIconButtonClass()}
        aria-label={t("common.back")}
        data-notes-mobile-back
        onclick={closeMobileDatabase}
      >
        <ChevronLeft class="size-5" strokeWidth={identityIconStrokeWidth} />
      </button>
    {/if}
    {#if databaseSelected}
      {@render databaseBreadcrumb()}
    {/if}
  </div>
{/snippet}

{#snippet databaseBreadcrumb()}
  {#if !mobileLayout}<span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>{/if}
  <button
    type="button"
    class={cn("flex min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left hover:bg-accent", mobileLayout ? "h-12" : "h-7 shrink-0",
      navigatorOpen && navigatorMode === "notes" && notesNavigatorSourceKey === `database:${notes.selectedDatabaseBlockId}` && "bg-accent")}
    aria-current="page"
    aria-label={selectedDatabaseTitle}
    aria-expanded={navigatorOpen && navigatorMode === "notes" && notesNavigatorSourceKey === `database:${notes.selectedDatabaseBlockId}`}
    data-notes-database-breadcrumb
    onpointerenter={(event) => {
      const anchor = event.currentTarget;
      if (!mobileLayout) navigatorHover.enterTrigger(event, () => openDatabaseNavigator(anchor));
    }}
    onpointerleave={navigatorHover.leave}
    onclick={(event) => openDatabaseNavigator(event.currentTarget, true)}
  >
    <Database size={identityIconSize} strokeWidth={identityIconStrokeWidth} class="shrink-0" />
    <span class={cn("truncate text-foreground", mobileLayout ? "min-w-0" : "max-w-48")}>{selectedDatabaseTitle}</span>
    <WorkspaceBreadcrumbTerminalIcon kind="chevron" context="notes" class="shrink-0 text-muted-foreground" />
  </button>
{/snippet}

{#snippet selectedPagePathBreadcrumb()}
  {#each selectedPagePath as node, nodeIndex (node.key)}
    {@const pathTitle = node.kind === "folder"
      ? node.folder.name
      : node.page.id === selectedPageId && selectedPageTitle
        ? selectedPageTitle
        : notesPageTitle(node.page, t("notes.untitled"))}
    <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
    <button
      type="button"
      class={cn(
        "flex min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left hover:bg-accent",
        databaseSelected && "shrink-0",
        "h-7",
        navigatorOpen
          && navigatorMode === "notes"
          && notesNavigatorSourceKey === node.key
          && "bg-accent",
      )}
      aria-label={pathTitle}
      aria-expanded={navigatorOpen && navigatorMode === "notes" && notesNavigatorSourceKey === node.key}
      onpointerenter={(event) => {
        const anchor = event.currentTarget;
        navigatorHover.enterTrigger(event, () => openHierarchyNavigator(node, anchor));
      }}
      onpointerleave={navigatorHover.leave}
      onclick={(event) => openNoteAncestor(node, event.currentTarget)}
    >
      {#if node.kind === "folder"}
        <Folder
          size={identityIconSize}
          strokeWidth={identityIconStrokeWidth}
          class="shrink-0"
        />
      {:else}
        <NotesPageIcon
          icon={node.page.icon}
          size={identityIconSize}
          strokeWidth={identityIconStrokeWidth}
          emojiScale={NOTES_PAGE_CHROME_EMOJI_SCALE}
          class="shrink-0"
        />
      {/if}
      <span class="min-w-0 truncate text-foreground">{pathTitle}</span>
      {#if !databaseSelected && nodeIndex === selectedPagePath.length - 1}
        <WorkspaceBreadcrumbTerminalIcon kind="chevron" context="notes" class="shrink-0 text-muted-foreground" />
      {/if}
    </button>
  {/each}
  {#if databaseSelected}
    {@render databaseBreadcrumb()}
  {:else}
  <button
    type="button"
    class={inlineNewPageButtonClass()}
    aria-label={t("notes.newPage")}
    aria-keyshortcuts="Control+N Meta+N"
    title={newPageTitle}
    onclick={createPage}
  >
    <WorkspaceBreadcrumbTerminalIcon kind="plus" />
  </button>
  {/if}
{/snippet}

<svelte:window onpointerdown={handleWindowPointerDown} />

<div
  bind:this={notesHeaderElement}
  class={cn(
    "flex shrink-0 items-center",
    mobileLayout ? "gap-0 overflow-hidden px-(--mobile-chrome-inset)" : "gap-1 overflow-x-auto px-3",
  )}
  style={`height: ${mobileLayout && !mobileActionRowVisible ? "0" : "var(--toolbar-h)"}; background-color: var(--cal-bg);`}
  data-notes-mobile-action-row={mobileLayout ? (mobileActionRowVisible ? "visible" : "collapsed") : undefined}
  onscroll={refreshNavigatorPanelGeometry}
  data-notes-workspace-header
>
  <div
    bind:this={notesIdentityElement}
    class={cn(
      "relative",
      mobileLayout || databaseSelected
        ? "min-w-0 flex-1 overflow-hidden"
        : "min-w-36 min-[760px]:max-w-xl",
    )}
  >
    {#if mobileLayout}
      {@render mobileHeader()}
    {:else}
    <div class={cn(
      "flex h-7 min-w-0 max-w-full items-center gap-0.5 text-identity font-medium",
      databaseSelected ? "overflow-x-auto" : "overflow-hidden",
    )}>
      {#if selectedProject && selectedGroup}
        <button
          bind:this={groupTriggerElement}
          type="button"
          class={cn(
            "flex min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left hover:bg-accent",
            databaseSelected && "shrink-0",
            "h-7",
            navigatorOpen && navigatorMode === "groups" && "bg-accent",
          )}
          aria-label={t("projects.navigator.open")}
          aria-expanded={navigatorOpen && navigatorMode === "groups"}
          onpointerenter={(event) => {
            navigatorHover.enterTrigger(event, () => openNavigator("groups"));
          }}
          onpointerleave={navigatorHover.leave}
          onclick={() => toggleNavigator("groups")}
        >
          <ProjectIcon
            name={selectedGroup.icon}
            size={identityIconSize}
            strokeWidth={identityIconStrokeWidth}
            emojiScale={projectIdentityEmojiScale}
            class="shrink-0"
          />
          <span class="min-w-0 truncate text-foreground">{selectedGroup.name}</span>
        </button>
        <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
        <button
          bind:this={projectTriggerElement}
          type="button"
          class={cn(
            "flex min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left hover:bg-accent",
            databaseSelected && "shrink-0",
            "h-7",
            navigatorOpen && navigatorMode === "projects" && "bg-accent",
          )}
          aria-label={selectedPageTitle ? t("notes.showProjectHome") : t("projects.navigator.open")}
          aria-expanded={navigatorOpen && navigatorMode === "projects"}
          onpointerenter={(event) => {
            navigatorHover.enterTrigger(event, () => openNavigator("projects"));
          }}
          onpointerleave={navigatorHover.leave}
          onclick={handleProjectTriggerClick}
        >
          <ProjectIcon
            name={selectedProject.icon}
            size={identityIconSize}
            strokeWidth={identityIconStrokeWidth}
            emojiScale={projectIdentityEmojiScale}
            class="shrink-0"
          />
          <span class="min-w-0 truncate text-foreground">{selectedProject.name}</span>
          {#if !showSelectedPagePath}
            <WorkspaceBreadcrumbTerminalIcon kind="chevron" context="notes" class="shrink-0 text-muted-foreground" />
          {/if}
        </button>
        {#if !showSelectedPagePath}
          <button
            type="button"
            class={inlineNewPageButtonClass()}
            aria-label={t("notes.newPage")}
            aria-keyshortcuts="Control+N Meta+N"
            title={newPageTitle}
            onclick={createPage}
          >
            <WorkspaceBreadcrumbTerminalIcon kind="plus" />
          </button>
        {/if}
        {#if showSelectedPagePath}
          {@render selectedPagePathBreadcrumb()}
        {/if}
      {:else}
        <button
          bind:this={noteTriggerElement}
          type="button"
          class={cn(
            "flex min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left hover:bg-accent",
            "h-7",
            navigatorOpen && navigatorMode === "notes" && "bg-accent",
          )}
          aria-label={t("notes.openNoteNavigator")}
          aria-expanded={navigatorOpen && navigatorMode === "notes"}
          onclick={() => toggleNavigator("notes")}
        >
          <NotesPageIcon
            icon={selectedPage?.icon ?? null}
            size={identityIconSize}
            strokeWidth={identityIconStrokeWidth}
            emojiScale={NOTES_PAGE_CHROME_EMOJI_SCALE}
            class="shrink-0"
          />
          <span class="min-w-0 truncate text-foreground">{selectedPageTitle ?? t("notes.title")}</span>
          <WorkspaceBreadcrumbTerminalIcon kind="chevron" class="shrink-0 text-muted-foreground" />
        </button>
        {#if databaseSelected}
          {@render databaseBreadcrumb()}
        {:else}
        <button
          type="button"
          class={inlineNewPageButtonClass()}
          aria-label={t("notes.newPage")}
          aria-keyshortcuts="Control+N Meta+N"
          title={newPageTitle}
          onclick={createPage}
        >
          <WorkspaceBreadcrumbTerminalIcon kind="plus" />
        </button>
        {/if}
      {/if}
    </div>
    {/if}
    {#if navigatorOpen}
      {#if mobileLayout}
        {#if mobileProjectPickerLoadState?.status === "ready" && mobileProjectPickerLoadState.component.kind === "mobile-project-picker"}
          {@const ProjectPickerMobileDialog = mobileProjectPickerLoadState.component.dialog}
          {@const ProjectPickerPanels = mobileProjectPickerLoadState.component.panels}
          <ProjectPickerMobileDialog
            label={navigatorMode === "notes" ? t("notes.noteNavigatorLabel") : t("projects.navigator.pickerLabel")}
            closeLabel={t("common.close")}
            onClose={() => { navigatorOpen = false; }}
          >
            {#if navigatorMode === "notes"}
              <NotesHierarchyPickerPanel
                projectId={selectedProjectId}
                parent={notesNavigatorParent}
                frameStyle="height:100%;max-height:100%"
                className="relative"
                zIndexClass=""
                mobileLayout
                title={notesNavigatorTitle || selectedProject?.name}
                initialMobileAncestors={notesNavigatorAncestors}
                onBack={() => { navigatorMode = "projects"; }}
                onClose={() => { navigatorOpen = false; }}
                onPageSelected={() => { navigatorOpen = false; }}
              />
            {:else}
              <ProjectPickerPanels
                {selectedProjectId}
                selectedGroupId={selectedGroup?.id ?? null}
                mode="groups"
                {showInactiveProjects}
                showInactiveToggle
                showOrderMenu
                showLifecycleBadges
                {onShowInactiveProjectsChange}
                onProjectSelected={openMobileProject}
                onProjectDrilldown={openMobileProject}
                mobileLayout
                initialMobileGroupId={navigatorMode === "projects" ? selectedGroup?.id ?? null : null}
                onClose={() => { navigatorOpen = false; }}
              />
            {/if}
          </ProjectPickerMobileDialog>
        {:else if mobileProjectPickerLoadState?.status === "failed"}
          <Notice
            layout="floating"
            message={t("common.viewLoadFailed", navigatorMode === "notes" ? t("notes.noteNavigatorLabel") : t("projects.navigator.pickerLabel"))}
            copy={false}
            onRetry={requestMobileProjectPicker}
            onDismiss={() => { navigatorOpen = false; }}
          />
        {/if}
      {:else}
        <div
          bind:this={navigatorPanelElement}
          class="fixed z-80"
          style={navigatorPanelStyle}
          role="dialog"
          tabindex="-1"
          aria-label={navigatorMode === "notes" ? t("notes.noteNavigatorLabel") : t("projects.navigator.pickerLabel")}
          onpointerenter={navigatorHover.enterPanel}
          onpointerleave={navigatorHover.leave}
          onpointerdown={navigatorHover.pin}
          onkeydown={navigatorHover.pin}
        >
          {#if navigatorMode === "notes"}
            <NotesHierarchyPickerPanel
              projectId={selectedProjectId}
              parent={notesNavigatorParent}
              frameStyle={`width: 100%; max-height: ${navigatorPanelMaxHeight}px;`}
              className="relative"
              zIndexClass=""
              onPageSelected={() => {
                navigatorOpen = false;
              }}
            />
          {:else}
            <NotesProjectNavigator
              {selectedProjectId}
              selectedGroupId={selectedGroup?.id ?? null}
              {showInactiveProjects}
              panelMode={navigatorMode}
              panelMaxHeight={navigatorPanelMaxHeight}
              onShowInactiveProjectsChange={onShowInactiveProjectsChange}
              onProjectSelected={() => {
                navigatorOpen = false;
                onProjectSelected();
              }}
              onPageSelected={() => {
                navigatorOpen = false;
              }}
            />
          {/if}
        </div>
      {/if}
    {/if}
  </div>
  {#if !mobileLayout}<div class="flex-1"></div>{/if}
  <div class={cn("flex shrink-0 items-center", mobileLayout ? "gap-0" : "gap-1")}>
    <div bind:this={pageActionsTarget} class={cn("flex shrink-0 items-center", mobileLayout ? "gap-0" : "gap-1")} data-notes-page-actions-slot></div>
    {#if selectedProject && !mobileLayout}
      <button
        type="button"
        data-notes-toolbar-trigger="settings"
        class={toolbarIconButtonClass(false, projectSettingsOpen)}
        aria-label={t("notes.projectSettingsTitle", selectedProject.name)}
        title={t("notes.projectSettingsTitle", selectedProject.name)}
        aria-expanded={projectSettingsOpen}
        onclick={onToggleProjectSettings}
      >
        <Settings2 size={14} strokeWidth={1.75} />
      </button>
    {/if}
  </div>
</div>
