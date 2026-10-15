<script lang="ts">
  import { tick, untrack } from "svelte";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import FileText from "@lucide/svelte/icons/file-text";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ProjectLifecycleIcon from "$lib/components/projects/ProjectLifecycleIcon.svelte";
  import {
    createHoverDelay,
    isPointerAimingAtSubmenu,
    SUBMENU_AIM_TOLERANCES,
    type MenuAimPoint,
  } from "$lib/utils/menu-aim";
  import { verticalBorderWidth } from "$lib/utils/anchored-panel";
  import {
    projectPickerBridgeFrameStyle,
    projectPickerMenuAimRect,
    projectPickerPanelEstimatedListHeight,
    projectPickerPanelFrameStyle,
    projectPickerPanelHeight,
    projectPickerPointerPoint,
    projectPickerScrollState,
    projectPickerSubpanelAimOrigin,
    projectPickerSubpanelGeometry,
    projectPickerSubpanelSide,
  } from "$lib/projects/picker-panels";
  import {
    PROJECT_TEMPLATE_IDS,
    type Project,
    type ProjectGroup,
    type ProjectTemplateId,
  } from "$lib/projects/types";
  import { notesFoldersForProject } from "$lib/notes/navigation/tree";
  import { notesHierarchyChildren } from "$lib/notes/navigation/hierarchy-menu";
  import { notesPageSearchGroups, notesPageSearchQueryIsSearchable } from "$lib/notes/navigation/page-search";
  import { NOTES_PAGE_CHROME_EMOJI_SCALE } from "$lib/notes/pages/icon";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import { notesPagesForProject } from "$lib/notes/project-membership";
  import type { NotesPage } from "$lib/notes/types";
  import type { ProjectNavigatorPanelMode } from "$lib/projects/toolbar";
  import { getNotes } from "$lib/stores/notes.svelte";
  import { createNotesSearchController } from "$lib/stores/notes/search.svelte";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { openInNewTabIntent, preventMiddleClickAutoscroll } from "$lib/workspace-tabs/gestures";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import SidebarOrderMenu from "$lib/components/ui/SidebarOrderMenu.svelte";
  import StarredIconBadge from "$lib/components/ui/StarredIconBadge.svelte";
  import { cn, type MaybePromise } from "$lib/utils";
  import { createSidebarOrderDrag } from "$lib/utils/sidebar-order-drag.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import NotesHierarchyPickerPanel from "./NotesHierarchyPickerPanel.svelte";


  let {
    selectedProjectId = null,
    selectedGroupId = null,
    panelMode = "groups",
    panelMaxHeight = null,
    showInactiveProjects = false,
    onShowInactiveProjectsChange,
    onProjectSelected,
    onPageSelected,
    onClose = undefined,
  }: {
    selectedProjectId?: string | null;
    selectedGroupId?: string | null;
    panelMode?: ProjectNavigatorPanelMode;
    panelMaxHeight?: number | null;
    showInactiveProjects?: boolean;
    onShowInactiveProjectsChange: (value: boolean) => void;
    onProjectSelected: () => MaybePromise<void>;
    onPageSelected: () => MaybePromise<void>;
    onClose?: () => void;
  } = $props();

  const projects = getProjects();
  const tabs = getWorkspaceTabs();
  const preferences = getPreferences();
  const notes = getNotes();
  const { t } = getLocalization();
  const iconStrokeWidth = 1.6;
  const iconSize = 13;
  const emojiScale = 0.94;
  const panelFallbackHeaderHeightPx = 40;
  const panelFallbackFooterHeightPx = 44;
  const panelListPaddingPx = 6;
  const panelRowHeightRem = 2;
  const panelFallbackRowHeightPx = 30;
  const subpanelListPaddingPx = 8;
  const subpanelFallbackFooterHeightPx = 44;
  const subpanelRowHeightPx = 32;
  const subpanelGapPx = 4;
  const zIndexClass = "z-81";
  const pageSearchDebounceMs = 200;
  const pageSearchPageSize = 30;

  let projectSearch = $state("");
  let searchedPageQuery = $state("");
  let groupDraft = $state("");
  let isCreateGroupOpen = $state(false);
  let createProjectGroupId = $state<string | null>(null);
  let projectDraftByGroup = $state<Record<string, string>>({});
  let projectTemplateDraftByGroup = $state<Record<string, ProjectTemplateId>>({});
  let activeGroupId = $state<string | null>(null);
  let activeGroupAnchorElement = $state<HTMLElement | null>(null);
  let activeProjectId = $state<string | null>(null);
  let activeProjectAnchorElement = $state<HTMLElement | null>(null);
  let activeProjectPanelElement = $state<HTMLDivElement | null>(null);
  let panelRootElement = $state<HTMLDivElement | null>(null);
  let panelHeaderElement = $state<HTMLDivElement | undefined>();
  let panelFooterElement = $state<HTMLDivElement | undefined>();
  let panelStyle = $state("");
  let groupScrollElement = $state<HTMLElement | undefined>();
  let groupScrollContentElement = $state<HTMLElement | undefined>();
  let isGroupScrollable = $state(false);
  let groupCanScrollUp = $state(false);
  let groupCanScrollDown = $state(false);
  let groupScrollStateFrame: number | null = null;
  let projectSubpanelElement = $state<HTMLDivElement | undefined>();
  let projectSubpanelBridgeElement = $state<HTMLDivElement | undefined>();
  let projectSubpanelFooterElement = $state<HTMLDivElement | undefined>();
  let projectSubpanelStyle = $state("");
  let projectSubpanelBridgeStyle = $state("");
  let projectScrollElement = $state<HTMLElement | undefined>();
  let projectScrollContentElement = $state<HTMLElement | undefined>();
  let isProjectScrollable = $state(false);
  let projectCanScrollUp = $state(false);
  let projectCanScrollDown = $state(false);
  let projectScrollStateFrame: number | null = null;
  let noteSubpanelElement = $state<HTMLDivElement | undefined>();
  let noteSubpanelBridgeElement = $state<HTMLDivElement | undefined>();
  let noteSubpanelStyle = $state("");
  let noteSubpanelBridgeStyle = $state("");
  const projectSubpanelDelay = createHoverDelay();
  const noteSubpanelDelay = createHoverDelay();
  $effect(() => () => {
    projectSubpanelDelay.cancel();
    noteSubpanelDelay.cancel();
  });

  const selectedProject = $derived(projects.projectById(selectedProjectId));
  const selectedGroup = $derived(projects.groupById(selectedProject?.groupId ?? selectedGroupId));
  const trimmedSearch = $derived(projectSearch.trim());
  const isSearchActive = $derived(trimmedSearch.length > 0);
  const visibleGroups = $derived.by(() => projects.visibleGroups());
  /** The search finds notes by title through the Notes index, grouped by the listed projects in sidebar order. */
  const pageSearch = createNotesSearchController();
  const pageSearchQueryIsSearchable = $derived(notesPageSearchQueryIsSearchable(trimmedSearch));
  const pageSearchPending = $derived(
    pageSearchQueryIsSearchable && (searchedPageQuery !== trimmedSearch || pageSearch.loading),
  );
  const pageSearchGroups = $derived(notesPageSearchGroups(
    pageSearch.results,
    visibleGroups.flatMap((group) => projectsInGroup(group)),
  ));
  const pageSearchRowCount = $derived(
    pageSearchGroups.reduce((count, group) => count + group.pages.length + 1, 0),
  );
  const activeGroup = $derived.by(() => visibleGroups.find((group) => group.id === activeGroupId));
  const directProjectGroup = $derived.by(() => selectedGroup);
  const directProjects = $derived.by(() => directProjectGroup ? projectsInGroup(directProjectGroup) : []);
  const activeProjectPages = $derived.by(() => notesPagesForProject(notes.allPages, activeProjectId));
  const activeProjectFolders = $derived.by(() => notesFoldersForProject(notes.folders, activeProjectId));
  const activeProjectRootItems = $derived(notesHierarchyChildren(
    activeProjectPages,
    activeProjectFolders,
    { kind: "root" },
    t("notes.untitled"),
    notes.sidebarPageIdsWithChildren,
  ));

  const customOrder = $derived(preferences.projectsSidebarOrder === "custom");
  const projectDragGroup = $derived(panelMode === "groups" ? activeGroup : directProjectGroup);
  /** Groups drag by their row in the custom order. */
  const groupDrag = createSidebarOrderDrag({
    getIds: () => visibleGroups.map((group) => group.id),
    enabled: () => customOrder && !isSearchActive,
    save: (orderedIds) => projects.reorderGroups(orderedIds),
    description: "project groups from the Notes project navigator",
  });
  /** Projects drag by their row in the custom order, in the main panel or the open group subpanel. */
  const projectDrag = createSidebarOrderDrag({
    getIds: () => projectDragGroup ? projectsInGroup(projectDragGroup).map((project) => project.id) : [],
    enabled: () => customOrder && !isSearchActive && projectDragGroup !== undefined,
    isStarred: (projectId) => projects.isProjectStarred(projectId),
    save: async (orderedIds) => {
      if (projectDragGroup) await projects.reorderProjectsInGroup(projectDragGroup.id, orderedIds);
    },
    description: "projects from the Notes project navigator",
  });
  $effect(() => groupDrag.reset);
  $effect(() => projectDrag.reset);

  /** A lifted group or project closes the panels it would otherwise open while passing over rows. */
  $effect(() => {
    const groupLifted = groupDrag.draggedId !== null;
    const projectLifted = projectDrag.draggedId !== null;
    if (!groupLifted && !projectLifted) return;
    untrack(() => {
      if (groupLifted) closeProjectSubpanel();
      else closeNoteSubpanel();
    });
  });

  function rowDragActive(): boolean {
    return groupDrag.draggedId !== null || projectDrag.draggedId !== null;
  }

  function projectsInGroup(group: ProjectGroup): Project[] {
    return showInactiveProjects
      ? projects.projectsForGroupIncludingInactive(group.id)
      : projects.projectsForGroup(group.id);
  }

  function projectTemplateLabel(templateId: ProjectTemplateId): string {
    if (templateId === "software") return t("projects.templates.software");
    if (templateId === "course") return t("projects.templates.course");
    if (templateId === "routine") return t("projects.templates.routine");
    if (templateId === "reading") return t("projects.templates.reading");
    if (templateId === "chores") return t("projects.templates.chores");
    return t("projects.templates.blank");
  }

  function rowHeight(): number {
    const rootFontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
    return Number.isFinite(rootFontSize)
      ? rootFontSize * panelRowHeightRem
      : panelFallbackRowHeightPx;
  }

  function cssPixelValue(value: string): number {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : 0;
  }

  function scrollAreaVerticalPadding(element: HTMLElement | undefined, fallback: number): number {
    if (!element) return fallback;
    const style = getComputedStyle(element);
    return cssPixelValue(style.paddingTop) + cssPixelValue(style.paddingBottom);
  }

  function mainListEstimatedHeight(): number {
    const itemCount = isSearchActive
      ? Math.max(1, pageSearchRowCount)
      : panelMode === "projects" && directProjectGroup
        ? projectsInGroup(directProjectGroup).length
        : visibleGroups.length;
    return projectPickerPanelEstimatedListHeight({
      itemCount,
      visibleRows: null,
      listPadding: panelListPaddingPx,
      rowHeight: rowHeight(),
    });
  }

  function updatePanelStyle(): void {
    const headerHeight = panelHeaderElement?.offsetHeight ?? panelFallbackHeaderHeightPx;
    const footerHeight = verticalBorderWidth(panelRootElement) + (isSearchActive
      ? 0
      : panelFooterElement?.offsetHeight ?? panelFallbackFooterHeightPx);
    const measuredListHeight = groupScrollContentElement
      ? groupScrollContentElement.scrollHeight + scrollAreaVerticalPadding(
        groupScrollElement,
        panelListPaddingPx,
      )
      : undefined;
    const listHeight = measuredListHeight ?? mainListEstimatedHeight();
    const panelHeight = projectPickerPanelHeight({
      headerHeight,
      footerHeight,
      listHeight,
      maxHeight: panelMaxHeight,
      visibleRows: null,
      listPadding: panelListPaddingPx,
      rowHeight: rowHeight(),
    });
    panelStyle = [
      `height: ${panelHeight}px`,
      `max-height: ${panelHeight}px`,
    ].join("; ");
  }

  function currentPanelBounds() {
    const margin = 8;
    const viewportBounds = {
      left: margin,
      right: window.innerWidth - margin,
      top: margin,
      bottom: window.innerHeight - margin,
    };
    const rect = panelRootElement?.closest(".notes-view-root")?.getBoundingClientRect();
    if (!rect) return viewportBounds;
    return {
      left: Math.max(rect.left, viewportBounds.left),
      right: Math.min(rect.right, viewportBounds.right),
      top: Math.max(rect.top, viewportBounds.top),
      bottom: Math.min(rect.bottom, viewportBounds.bottom),
    };
  }

  function isPointerAimingAtProjectSubpanel(point: MenuAimPoint): boolean {
    if (!activeGroupAnchorElement || !projectSubpanelElement) return false;
    const anchorRect = activeGroupAnchorElement.getBoundingClientRect();
    const subpanelRect = projectSubpanelElement.getBoundingClientRect();
    const side = projectPickerSubpanelSide(anchorRect, subpanelRect);
    return isPointerAimingAtSubmenu({
      origin: projectPickerSubpanelAimOrigin(anchorRect),
      point,
      submenu: projectPickerMenuAimRect(subpanelRect),
      side,
      ...SUBMENU_AIM_TOLERANCES,
    });
  }

  function isPointerAimingAtNoteSubpanel(point: MenuAimPoint): boolean {
    if (!activeProjectAnchorElement || !noteSubpanelElement) return false;
    const anchorRect = activeProjectAnchorElement.getBoundingClientRect();
    const subpanelRect = noteSubpanelElement.getBoundingClientRect();
    const side = projectPickerSubpanelSide(anchorRect, subpanelRect);
    return isPointerAimingAtSubmenu({
      origin: projectPickerSubpanelAimOrigin(anchorRect),
      point,
      submenu: projectPickerMenuAimRect(subpanelRect),
      side,
      ...SUBMENU_AIM_TOLERANCES,
    });
  }

  function updateProjectSubpanelGeometry(): void {
    if (!activeGroupAnchorElement || !panelRootElement) return;
    const anchorRect = activeGroupAnchorElement.getBoundingClientRect();
    const panelRect = panelRootElement.getBoundingClientRect();
    const bounds = currentPanelBounds();
    const footerHeight = verticalBorderWidth(projectSubpanelElement)
      + (projectSubpanelFooterElement?.offsetHeight ?? subpanelFallbackFooterHeightPx);
    const projectCount = activeGroup ? projectsInGroup(activeGroup).length : 0;
    const measuredListHeight = projectScrollContentElement
      ? projectScrollContentElement.scrollHeight + scrollAreaVerticalPadding(
        projectScrollElement,
        subpanelListPaddingPx,
      )
      : undefined;
    const geometry = projectPickerSubpanelGeometry({
      anchorRect,
      panelRect,
      bounds,
      gap: subpanelGapPx,
      footerHeight,
      projectCount,
      visibleRows: null,
      listHeight: measuredListHeight,
      listPadding: subpanelListPaddingPx,
      rowHeight: subpanelRowHeightPx,
    });

    projectSubpanelStyle = projectPickerPanelFrameStyle(geometry.panel);
    projectSubpanelBridgeStyle = projectPickerBridgeFrameStyle(geometry.bridge);
  }

  function updateNoteSubpanelGeometry(): void {
    if (!activeProjectAnchorElement || !activeProjectPanelElement) return;
    const anchorRect = activeProjectAnchorElement.getBoundingClientRect();
    const panelRect = activeProjectPanelElement.getBoundingClientRect();
    const bounds = currentPanelBounds();
    const geometry = projectPickerSubpanelGeometry({
      anchorRect,
      panelRect,
      bounds,
      gap: subpanelGapPx,
      footerHeight: subpanelFallbackFooterHeightPx,
      panelHeight: noteSubpanelElement?.getBoundingClientRect().height || undefined,
      projectCount: Math.max(1, activeProjectRootItems.length),
      visibleRows: null,
      listPadding: subpanelListPaddingPx,
      rowHeight: subpanelRowHeightPx,
    });

    noteSubpanelStyle = projectPickerPanelFrameStyle(geometry.panel);
    noteSubpanelBridgeStyle = projectPickerBridgeFrameStyle(geometry.bridge);
  }

  function refreshGroupScrollState(): void {
    groupScrollStateFrame = null;
    const element = groupScrollElement;
    if (!element) {
      isGroupScrollable = false;
      groupCanScrollUp = false;
      groupCanScrollDown = false;
      return;
    }
    const state = projectPickerScrollState(element);
    isGroupScrollable = state.scrollable;
    groupCanScrollUp = state.canScrollUp;
    groupCanScrollDown = state.canScrollDown;
  }

  function requestGroupScrollStateRefresh(): void {
    if (groupScrollStateFrame !== null) cancelAnimationFrame(groupScrollStateFrame);
    groupScrollStateFrame = requestAnimationFrame(refreshGroupScrollState);
  }

  function refreshProjectScrollState(): void {
    projectScrollStateFrame = null;
    const element = projectScrollElement;
    if (!element) {
      isProjectScrollable = false;
      projectCanScrollUp = false;
      projectCanScrollDown = false;
      return;
    }
    const state = projectPickerScrollState(element);
    isProjectScrollable = state.scrollable;
    projectCanScrollUp = state.canScrollUp;
    projectCanScrollDown = state.canScrollDown;
  }

  function requestProjectScrollStateRefresh(): void {
    if (projectScrollStateFrame !== null) cancelAnimationFrame(projectScrollStateFrame);
    projectScrollStateFrame = requestAnimationFrame(refreshProjectScrollState);
  }

  function handleGroupScroll(): void {
    refreshGroupScrollState();
  }

  function handleProjectScroll(): void {
    refreshProjectScrollState();
  }

  function closeNoteSubpanel(): void {
    noteSubpanelDelay.cancel();
    activeProjectId = null;
    activeProjectAnchorElement = null;
    activeProjectPanelElement = null;
    noteSubpanelStyle = "";
    noteSubpanelBridgeStyle = "";
  }

  function closeProjectSubpanel(): void {
    projectSubpanelDelay.cancel();
    activeGroupId = null;
    activeGroupAnchorElement = null;
    createProjectGroupId = null;
    projectSubpanelFooterElement = undefined;
    projectSubpanelStyle = "";
    projectSubpanelBridgeStyle = "";
    closeNoteSubpanel();
  }

  function showProjectSubpanel(
    group: ProjectGroup,
    target: EventTarget | null,
  ): void {
    projectSubpanelDelay.cancel();
    if (activeGroupId !== group.id) {
      createProjectGroupId = null;
      closeNoteSubpanel();
    }
    activeGroupId = group.id;
    activeGroupAnchorElement = target instanceof HTMLElement ? target : null;
    updateProjectSubpanelGeometry();
    void tick().then(updateProjectSubpanelGeometry);
  }

  function showNoteSubpanel(
    project: Project,
    target: EventTarget | null,
    sourcePanelElement: HTMLDivElement | null | undefined,
  ): void {
    noteSubpanelDelay.cancel();
    activeProjectId = project.id;
    activeProjectAnchorElement = target instanceof HTMLElement ? target : null;
    activeProjectPanelElement = sourcePanelElement ?? null;
    updateNoteSubpanelGeometry();
    void tick().then(updateNoteSubpanelGeometry);
  }

  /**
   * Shows a group's project subpanel, deferring the switch while the pointer
   * travels toward the open subpanel so a diagonal path does not swap it.
   */
  function hoverGroup(group: ProjectGroup, event: PointerEvent): void {
    const target = event.currentTarget;
    if (activeGroupId && isPointerAimingAtProjectSubpanel(projectPickerPointerPoint(event))) {
      projectSubpanelDelay.schedule(() => showProjectSubpanel(group, target));
      return;
    }
    showProjectSubpanel(group, target);
  }

  /**
   * Shows a project's note subpanel, deferring the switch while the pointer
   * travels toward the open subpanel so a diagonal path does not swap it.
   */
  function hoverProject(
    project: Project,
    event: PointerEvent,
    sourcePanelElement: HTMLDivElement | null | undefined,
  ): void {
    const target = event.currentTarget;
    if (activeProjectId && isPointerAimingAtNoteSubpanel(projectPickerPointerPoint(event))) {
      noteSubpanelDelay.schedule(() => showNoteSubpanel(project, target, sourcePanelElement));
      return;
    }
    showNoteSubpanel(project, target, sourcePanelElement);
  }

  function handleGroupPointerEnter(group: ProjectGroup, event: PointerEvent): void {
    if (rowDragActive()) return;
    if (activeGroupId === group.id) {
      projectSubpanelDelay.cancel();
      return;
    }
    hoverGroup(group, event);
  }

  function handleGroupPointerMove(group: ProjectGroup, event: PointerEvent): void {
    if (rowDragActive() || activeGroupId === group.id) return;
    hoverGroup(group, event);
  }

  function handleProjectPointerEnter(
    project: Project,
    event: PointerEvent,
    sourcePanelElement: HTMLDivElement | null | undefined,
  ): void {
    if (rowDragActive()) return;
    if (activeProjectId === project.id) {
      noteSubpanelDelay.cancel();
      return;
    }
    hoverProject(project, event, sourcePanelElement);
  }

  function handleProjectPointerMove(
    project: Project,
    event: PointerEvent,
    sourcePanelElement: HTMLDivElement | null | undefined,
  ): void {
    if (rowDragActive() || activeProjectId === project.id) return;
    hoverProject(project, event, sourcePanelElement);
  }

  function isProjectSubpanelBoundaryTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Node)) return false;
    return Boolean(
      activeGroupAnchorElement?.contains(target)
      || projectSubpanelBridgeElement?.contains(target)
      || projectSubpanelElement?.contains(target)
      || noteSubpanelBridgeElement?.contains(target)
      || noteSubpanelElement?.contains(target),
    );
  }

  function isNoteSubpanelBoundaryTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Node)) return false;
    return Boolean(
      activeProjectAnchorElement?.contains(target)
      || activeProjectPanelElement?.contains(target)
      || noteSubpanelBridgeElement?.contains(target)
      || noteSubpanelElement?.contains(target),
    );
  }

  function handleProjectSubpanelBoundaryLeave(event: PointerEvent): void {
    if (isProjectSubpanelBoundaryTarget(event.relatedTarget)) return;
    projectSubpanelDelay.schedule(closeProjectSubpanel);
  }

  function handleNoteSubpanelBoundaryLeave(event: PointerEvent): void {
    if (isNoteSubpanelBoundaryTarget(event.relatedTarget)) return;
    noteSubpanelDelay.schedule(closeNoteSubpanel);
  }

  function handleNoteSubpanelBoundaryEnter(): void {
    noteSubpanelDelay.cancel();
    projectSubpanelDelay.cancel();
  }

  async function selectProject(project: Project): Promise<void> {
    await projects.selectProject(project.id);
    await onProjectSelected();
    projectSearch = "";
  }

  function handleProjectRowClick(event: MouseEvent, project: Project): void {
    const intent = tabs.canOpenTabs ? openInNewTabIntent(event) : null;
    if (intent) {
      event.preventDefault();
      closeNoteSubpanel();
      tabs.openLocation("project", { kind: "notes", projectId: project.id, item: null }, intent);
      if (!intent.background) onClose?.();
      return;
    }
    if (event.type === "click") void selectProject(project);
  }

  async function selectSearchedPage(page: NotesPage): Promise<void> {
    await notes.selectPage(page.id, { openMode: "full" });
    await onPageSelected();
    projectSearch = "";
  }

  /** Searches after typing pauses; a query the index cannot run clears the results instead. */
  $effect(() => {
    const query = trimmedSearch;
    if (!pageSearchQueryIsSearchable) {
      searchedPageQuery = "";
      void pageSearch.search("", pageSearchPageSize, false);
      return;
    }
    const timer = setTimeout(() => {
      searchedPageQuery = query;
      void pageSearch.search(query, pageSearchPageSize, false);
    }, pageSearchDebounceMs);
    return () => clearTimeout(timer);
  });

  async function submitGroup(): Promise<void> {
    const name = groupDraft.trim();
    if (!name) return;
    await projects.addGroup(name);
    groupDraft = "";
    isCreateGroupOpen = false;
  }

  async function submitProject(groupId: string): Promise<void> {
    const name = (projectDraftByGroup[groupId] ?? "").trim();
    if (!name) return;
    const templateId = projectTemplateDraftByGroup[groupId] ?? "blank";
    await projects.addProject(groupId, name, templateId);
    projectDraftByGroup = { ...projectDraftByGroup, [groupId]: "" };
    projectTemplateDraftByGroup = { ...projectTemplateDraftByGroup, [groupId]: "blank" };
    createProjectGroupId = null;
    await onProjectSelected();
  }

  $effect(() => {
    if (isSearchActive) {
      closeProjectSubpanel();
      closeNoteSubpanel();
      return;
    }
    if (panelMode === "projects") {
      closeProjectSubpanel();
      return;
    }
    if (!activeGroupId) {
      closeNoteSubpanel();
      return;
    }
    if (activeProjectPanelElement === panelRootElement) {
      closeNoteSubpanel();
    }
    if (!visibleGroups.some((group) => group.id === activeGroupId)) {
      closeProjectSubpanel();
    }
  });

  $effect(() => {
    const maxHeight = panelMaxHeight;
    const hasActiveSearch = isSearchActive;
    const mode = panelMode;
    const groupCount = visibleGroups.length;
    const resultCount = pageSearchRowCount;
    const searchPending = pageSearchPending;
    const directProjectCount = directProjectGroup ? projectsInGroup(directProjectGroup).length : 0;
    const noteCount = activeProjectRootItems.length;
    const isCreatingGroup = isCreateGroupOpen;
    const creatingProjectGroupId = createProjectGroupId;
    void maxHeight;
    void hasActiveSearch;
    void mode;
    void groupCount;
    void resultCount;
    void searchPending;
    void directProjectCount;
    void noteCount;
    void isCreatingGroup;
    void creatingProjectGroupId;
    requestAnimationFrame(() => {
      updatePanelStyle();
      requestGroupScrollStateRefresh();
      updateProjectSubpanelGeometry();
      updateNoteSubpanelGeometry();
    });
  });

  $effect(() => {
    const scrollElement = groupScrollElement;
    if (!scrollElement) return;
    const resizeObserver = new ResizeObserver(() => {
      updatePanelStyle();
      requestGroupScrollStateRefresh();
      updateProjectSubpanelGeometry();
      updateNoteSubpanelGeometry();
    });
    resizeObserver.observe(scrollElement);
    if (panelHeaderElement) resizeObserver.observe(panelHeaderElement);
    if (panelFooterElement) resizeObserver.observe(panelFooterElement);
    if (groupScrollContentElement) resizeObserver.observe(groupScrollContentElement);
    updatePanelStyle();
    requestGroupScrollStateRefresh();
    return () => {
      resizeObserver.disconnect();
      if (groupScrollStateFrame !== null) {
        cancelAnimationFrame(groupScrollStateFrame);
        groupScrollStateFrame = null;
      }
    };
  });

  $effect(() => {
    const scrollElement = projectScrollElement;
    if (!scrollElement) return;
    const resizeObserver = new ResizeObserver(() => {
      requestProjectScrollStateRefresh();
      updateProjectSubpanelGeometry();
      updateNoteSubpanelGeometry();
    });
    resizeObserver.observe(scrollElement);
    if (projectScrollContentElement) resizeObserver.observe(projectScrollContentElement);
    if (projectSubpanelFooterElement) resizeObserver.observe(projectSubpanelFooterElement);
    requestProjectScrollStateRefresh();
    return () => {
      resizeObserver.disconnect();
      if (projectScrollStateFrame !== null) {
        cancelAnimationFrame(projectScrollStateFrame);
        projectScrollStateFrame = null;
      }
    };
  });
</script>

{#snippet projectRow(project: Project, group: ProjectGroup, sourcePanelElement: HTMLDivElement | null | undefined)}
  <button
    type="button"
    class={cn(
      "menu-item grid grid-cols-[1rem_minmax(0,1fr)_auto]",
      project.status === "active" ? "text-popover-foreground" : "text-popover-foreground/60",
    )}
    aria-expanded={activeProjectId === project.id}
    aria-label={t("projects.actions.selectProject", project.name, group.name)}
    onpointerdown={(event) => projectDrag.pointerDown(event, project.id)}
    onpointerenter={(event) => handleProjectPointerEnter(project, event, sourcePanelElement)}
    onpointermove={(event) => handleProjectPointerMove(project, event, sourcePanelElement)}
    onpointerleave={handleNoteSubpanelBoundaryLeave}
    onfocus={(event) => showNoteSubpanel(project, event.currentTarget, sourcePanelElement)}
    onmousedown={preventMiddleClickAutoscroll}
    onclick={(event) => handleProjectRowClick(event, project)}
    onauxclick={(event) => handleProjectRowClick(event, project)}
  >
    <StarredIconBadge starred={projects.isProjectStarred(project.id)}><ProjectIcon name={project.icon} size={iconSize} strokeWidth={iconStrokeWidth} emojiScale={emojiScale} /></StarredIconBadge>
    <span class="min-w-0 truncate">{project.name}</span>
    <span class="flex min-w-0 items-center justify-end gap-1">
      {#if project.status !== "active"}
        <ProjectLifecycleIcon status={project.status} size={iconSize} strokeWidth={iconStrokeWidth} class="text-popover-foreground/60" />
      {/if}
      <ChevronRight size={13} strokeWidth={iconStrokeWidth} class="shrink-0 text-popover-foreground/60" />
    </span>
  </button>
{/snippet}

<div
  bind:this={panelRootElement}
  class="project-picker-panel surface-floating flex min-h-0 w-full flex-col overflow-hidden"
  style={panelStyle}
>
  <div bind:this={panelHeaderElement} class="px-1.5 pb-0.5 pt-1.5">
    <div class="field flex items-center gap-1.5 pr-1">
      <Search size={13} strokeWidth={iconStrokeWidth} class="shrink-0 text-popover-foreground/60" />
      <input
        bind:value={projectSearch}
        placeholder={t("notes.searchPlaceholder")}
        aria-label={t("notes.searchLabel")}
        class="field-bare text-popover-foreground"
      />
      <div class="flex shrink-0 items-center">
        <SidebarOrderMenu
          order={preferences.projectsSidebarOrder}
          label={t("projects.navigator.sortGroupsAndProjects")}
          buttonClass="flex size-6 shrink-0 items-center justify-center rounded-floating-item text-popover-foreground/60 hover:bg-accent hover:text-accent-foreground"
          activeClass="bg-accent text-accent-foreground"
          iconSize={13}
          {iconStrokeWidth}
          menuZIndexClass="z-90"
          onSelect={(order) => { if (order) preferences.setProjectsSidebarOrder(order); }}
        />
        <button
          type="button"
          class="flex size-6 shrink-0 items-center justify-center rounded-floating-item text-popover-foreground/60 hover:bg-accent hover:text-accent-foreground"
          aria-label={showInactiveProjects ? t("projects.navigator.hideInactive") : t("projects.navigator.showInactive")}
          title={showInactiveProjects ? t("projects.navigator.hideInactive") : t("projects.navigator.showInactive")}
          onclick={() => {
            onShowInactiveProjectsChange(!showInactiveProjects);
          }}
        >
          {#if showInactiveProjects}
            <EyeOff size={13} strokeWidth={iconStrokeWidth} />
          {:else}
            <Eye size={13} strokeWidth={iconStrokeWidth} />
          {/if}
        </button>
      </div>
    </div>
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="relative min-h-0 flex-1" onpointerleave={handleNoteSubpanelBoundaryLeave}>
    <div
      bind:this={groupScrollElement}
      class={cn(
        "project-picker-scroll-area hide-scrollbar h-full min-h-0 overflow-y-auto pb-1 pt-0.5",
        isGroupScrollable && "pr-2",
        isGroupScrollable && groupCanScrollUp && groupCanScrollDown && "project-picker-scroll-both",
        isGroupScrollable && groupCanScrollUp && !groupCanScrollDown && "project-picker-scroll-top",
        isGroupScrollable && !groupCanScrollUp && groupCanScrollDown && "project-picker-scroll-bottom",
      )}
      onscroll={handleGroupScroll}
    >
      <div bind:this={groupScrollContentElement}>
        {#if projects.loading && !projects.loaded}
          <Loading layout="rows" rows={3} label={t("projects.loading")} />
        {:else if projects.loadError}
          <Notice class="px-3 py-2" message={t("projects.loadFailed", projects.loadError)} copy={projects.loadError} onRetry={() => projects.load()} />
        {:else if isSearchActive && pageSearch.error}
          <Notice class="px-3 py-2" message={t("notes.searchFailed", pageSearch.error)} copy={pageSearch.error} />
        {:else if isSearchActive && pageSearchGroups.length > 0}
          <div class="grid px-1">
            {#each pageSearchGroups as resultGroup (resultGroup.project?.id ?? "")}
              <div class="px-1 pb-1">
                <div class="menu-label">
                  {resultGroup.project?.name ?? t("notes.searchNoProject")}
                </div>
                <div class="grid">
                  {#each resultGroup.pages as page (page.id)}
                    <button
                      type="button"
                      class={cn(
                        "menu-item text-popover-foreground",
                        notes.selectedPageId === page.id && "font-medium",
                      )}
                      aria-current={notes.selectedPageId === page.id ? "page" : undefined}
                      onclick={() => { void selectSearchedPage(page); }}
                    >
                      <StarredIconBadge starred={notes.favoritePageIds.includes(page.id)}>
                        {#if page.icon}
                          <NotesPageIcon
                            icon={page.icon}
                            size={iconSize}
                            strokeWidth={iconStrokeWidth}
                            emojiScale={NOTES_PAGE_CHROME_EMOJI_SCALE}
                            class="shrink-0"
                          />
                        {:else}
                          <FileText size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
                        {/if}
                      </StarredIconBadge>
                      <span class="min-w-0 flex-1 truncate">{notesPageTitle(page, t("notes.untitled"))}</span>
                    </button>
                  {/each}
                </div>
              </div>
            {/each}
          </div>
        {:else if isSearchActive && pageSearchPending}
          <Loading layout="rows" rows={3} label={t("notes.searching")} />
        {:else if isSearchActive}
          <div class="px-3 py-2 text-popover-foreground/60">
            {t("notes.noSearchResults")}
          </div>
        {:else if panelMode === "groups"}
          {#if visibleGroups.length === 0}
            <div class="px-3 py-2 text-popover-foreground/60">
              {t("calendar.eventPanel.noProjectsFound")}
            </div>
          {:else}
            <div class="grid pl-1">
              {#each visibleGroups as group (group.id)}
                <div class="pl-1 pr-1" {@attach groupDrag.item(group.id)}>
                  <button
                    type="button"
                    class="menu-item grid grid-cols-[1.25rem_minmax(0,1fr)_1rem] text-popover-foreground"
                    onpointerdown={(event) => groupDrag.pointerDown(event, group.id)}
                    aria-expanded={activeGroupId === group.id}
                    onpointerenter={(event) => handleGroupPointerEnter(group, event)}
                    onpointermove={(event) => handleGroupPointerMove(group, event)}
                    onpointerleave={handleProjectSubpanelBoundaryLeave}
                    onfocus={(event) => showProjectSubpanel(group, event.currentTarget)}
                    onclick={(event) => showProjectSubpanel(group, event.currentTarget)}
                  >
                    <ProjectIcon name={group.icon} size={iconSize} strokeWidth={iconStrokeWidth} emojiScale={emojiScale} class="shrink-0" />
                    <span class="truncate font-medium">{group.name}</span>
                    <ChevronRight size={13} strokeWidth={iconStrokeWidth} class="justify-self-end text-popover-foreground/60" />
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        {:else if !directProjectGroup}
          <div class="px-3 py-2 text-popover-foreground/60">
            {t("projects.navigator.empty")}
          </div>
        {:else if directProjects.length === 0}
          <div class="px-3 py-2 text-popover-foreground/60">
            {t("calendar.eventPanel.noProjectsFound")}
          </div>
        {:else}
          <div class="grid px-1">
            {#each directProjects as project (project.id)}
              <div class="px-1" {@attach projectDrag.item(project.id)}>
                {@render projectRow(project, directProjectGroup, panelRootElement)}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
    <CalendarScrollbar
      scrollContainer={groupScrollElement}
      stickyTop={4}
      stickyBottom={4}
      wheelPassthrough
    />
  </div>

  {#if !isSearchActive}
    <div bind:this={panelFooterElement} class="relative z-10 shrink-0 bg-popover p-1.5">
      <div class="pointer-events-none absolute left-1.5 right-1.5 top-0 border-t border-border/70"></div>
      {#if panelMode === "groups"}
        {#if isCreateGroupOpen}
          <form class="flex gap-1" onsubmit={(event) => { event.preventDefault(); void submitGroup(); }}>
            <input
              bind:value={groupDraft}
              placeholder={t("projects.navigator.groupNamePlaceholder")}
              class="field min-w-0 flex-1 text-popover-foreground"
            />
            <button type="submit" class="min-h-(--panel-row-height) rounded-floating-item bg-primary px-2 text-panel-detail font-medium text-primary-foreground">
              {t("common.save")}
            </button>
          </form>
        {:else}
          <button
            type="button"
            class="menu-item justify-center text-popover-foreground"
            onclick={() => { isCreateGroupOpen = true; }}
          >
            <Plus size={13} strokeWidth={iconStrokeWidth} />
            <span>{t("calendar.eventPanel.createGroup")}</span>
          </button>
        {/if}
      {:else if directProjectGroup}
        {#if createProjectGroupId === directProjectGroup.id}
          <form class="grid gap-1" onsubmit={(event) => { event.preventDefault(); void submitProject(directProjectGroup.id); }}>
            <div class="flex gap-1">
              <input
                value={projectDraftByGroup[directProjectGroup.id] ?? ""}
                oninput={(event) => {
                  projectDraftByGroup = {
                    ...projectDraftByGroup,
                    [directProjectGroup.id]: event.currentTarget.value,
                  };
                }}
                placeholder={t("projects.navigator.projectNamePlaceholder")}
                class="field min-w-0 flex-1 text-popover-foreground"
              />
              <button type="submit" class="min-h-(--panel-row-height) rounded-floating-item bg-primary px-2 text-panel-detail font-medium text-primary-foreground">
                {t("common.save")}
              </button>
            </div>
            <div class="flex flex-wrap gap-1" aria-label={t("projects.navigator.projectTemplate")}>
              {#each PROJECT_TEMPLATE_IDS as templateId}
                <button
                  type="button"
                  class={cn(
                    "min-h-6 rounded-floating-item border px-1.5 text-panel-detail",
                    (projectTemplateDraftByGroup[directProjectGroup.id] ?? "blank") === templateId
                      ? "border-primary/60 bg-primary/10 text-primary"
                      : "border-border bg-transparent text-popover-foreground/60 hover:bg-accent hover:text-accent-foreground",
                  )}
                  onclick={() => {
                    projectTemplateDraftByGroup = {
                      ...projectTemplateDraftByGroup,
                      [directProjectGroup.id]: templateId,
                    };
                  }}
                >
                  {projectTemplateLabel(templateId)}
                </button>
              {/each}
            </div>
          </form>
        {:else}
          <button
            type="button"
            class="menu-item justify-center text-popover-foreground"
            onclick={() => {
              createProjectGroupId = directProjectGroup.id;
            }}
          >
            <Plus size={13} strokeWidth={iconStrokeWidth} />
            <span>{t("calendar.eventPanel.createProject")}</span>
          </button>
        {/if}
      {/if}
    </div>
  {/if}
</div>

{#if panelMode === "groups" && !isSearchActive && activeGroup && activeGroupAnchorElement}
  {@const activeGroupProjects = projectsInGroup(activeGroup)}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={projectSubpanelBridgeElement}
    aria-hidden="true"
    class={cn("fixed bg-transparent", zIndexClass)}
    style={projectSubpanelBridgeStyle}
    onpointerenter={projectSubpanelDelay.cancel}
    onpointerleave={handleProjectSubpanelBoundaryLeave}
  ></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={projectSubpanelElement}
    class={cn("project-picker-panel surface-floating fixed flex min-h-0 flex-col overflow-hidden", zIndexClass)}
    style={projectSubpanelStyle}
    onpointerenter={projectSubpanelDelay.cancel}
    onpointerleave={handleProjectSubpanelBoundaryLeave}
  >
    <div class="relative min-h-0 flex-1">
      <div
        bind:this={projectScrollElement}
        class={cn(
          "project-picker-scroll-area hide-scrollbar h-full min-h-0 overflow-y-auto py-1",
          isProjectScrollable ? "px-1 pr-2" : "px-1",
          isProjectScrollable && projectCanScrollUp && projectCanScrollDown && "project-picker-scroll-both",
          isProjectScrollable && projectCanScrollUp && !projectCanScrollDown && "project-picker-scroll-top",
          isProjectScrollable && !projectCanScrollUp && projectCanScrollDown && "project-picker-scroll-bottom",
        )}
        onscroll={handleProjectScroll}
      >
        <div bind:this={projectScrollContentElement}>
          {#if activeGroupProjects.length === 0}
            <div class="px-3 py-2 text-popover-foreground/60">
              {t("calendar.eventPanel.noProjectsFound")}
            </div>
          {:else}
            <div class="grid">
              {#each activeGroupProjects as project (project.id)}
                <div {@attach projectDrag.item(project.id)}>
                  {@render projectRow(project, activeGroup, projectSubpanelElement)}
                </div>
              {/each}
            </div>
          {/if}
        </div>
      </div>
      <CalendarScrollbar
        scrollContainer={projectScrollElement}
        stickyTop={4}
        stickyBottom={4}
        wheelPassthrough
      />
    </div>

    <div bind:this={projectSubpanelFooterElement} class="relative z-10 shrink-0 bg-popover p-1.5">
      <div class="pointer-events-none absolute left-1.5 right-1.5 top-0 border-t border-border/70"></div>
      {#if createProjectGroupId === activeGroup.id}
        <form class="grid gap-1" onsubmit={(event) => { event.preventDefault(); void submitProject(activeGroup.id); }}>
          <div class="flex gap-1">
            <input
              value={projectDraftByGroup[activeGroup.id] ?? ""}
              oninput={(event) => {
                projectDraftByGroup = {
                  ...projectDraftByGroup,
                  [activeGroup.id]: event.currentTarget.value,
                };
              }}
              placeholder={t("projects.navigator.projectNamePlaceholder")}
              class="field min-w-0 flex-1 text-popover-foreground"
            />
            <button type="submit" class="min-h-(--panel-row-height) rounded-floating-item bg-primary px-2 text-panel-detail font-medium text-primary-foreground">
              {t("common.save")}
            </button>
          </div>
          <div class="flex flex-wrap gap-1" aria-label={t("projects.navigator.projectTemplate")}>
            {#each PROJECT_TEMPLATE_IDS as templateId}
              <button
                type="button"
                class={cn(
                  "min-h-6 rounded-floating-item border px-1.5 text-panel-detail",
                  (projectTemplateDraftByGroup[activeGroup.id] ?? "blank") === templateId
                    ? "border-primary/60 bg-primary/10 text-primary"
                    : "border-border bg-transparent text-popover-foreground/60 hover:bg-accent hover:text-accent-foreground",
                )}
                onclick={() => {
                  projectTemplateDraftByGroup = {
                    ...projectTemplateDraftByGroup,
                    [activeGroup.id]: templateId,
                  };
                }}
              >
                {projectTemplateLabel(templateId)}
              </button>
            {/each}
          </div>
        </form>
      {:else}
        <button
          type="button"
          class="menu-item justify-center text-popover-foreground"
          onclick={() => {
            createProjectGroupId = activeGroup.id;
            void tick().then(updateProjectSubpanelGeometry);
          }}
        >
          <Plus size={13} strokeWidth={iconStrokeWidth} />
          <span>{t("calendar.eventPanel.createProject")}</span>
        </button>
      {/if}
    </div>
  </div>
{/if}

{#if activeProjectId && activeProjectAnchorElement}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={noteSubpanelBridgeElement}
    aria-hidden="true"
    class={cn("fixed bg-transparent", zIndexClass)}
    style={noteSubpanelBridgeStyle}
    onpointerenter={handleNoteSubpanelBoundaryEnter}
    onpointerleave={handleNoteSubpanelBoundaryLeave}
  ></div>
  <NotesHierarchyPickerPanel
    bind:rootElement={noteSubpanelElement}
    projectId={activeProjectId}
    parent={{ kind: "root" }}
    showSearch={false}
    frameStyle={noteSubpanelStyle}
    className="fixed"
    {zIndexClass}
    onLayoutChange={updateNoteSubpanelGeometry}
    onPointerEnter={handleNoteSubpanelBoundaryEnter}
    onPointerLeave={handleNoteSubpanelBoundaryLeave}
    onPageSelected={async () => {
      closeNoteSubpanel();
      await onPageSelected();
    }}
  />
{/if}

<style>
  .project-picker-panel {
    --cal-scrollbar-thumb: color-mix(in srgb, var(--popover-foreground) 18%, var(--popover));
    --cal-scrollbar-thumb-hover: color-mix(in srgb, var(--popover-foreground) 36%, var(--popover));
  }

  .project-picker-scroll-area {
    --project-picker-scroll-fade: 28px;

    transition: -webkit-mask-image 120ms ease, mask-image 120ms ease;
  }

  .project-picker-scroll-top {
    -webkit-mask-image: linear-gradient(to bottom, transparent, black var(--project-picker-scroll-fade), black);
    mask-image: linear-gradient(to bottom, transparent, black var(--project-picker-scroll-fade), black);
  }

  .project-picker-scroll-bottom {
    -webkit-mask-image: linear-gradient(to top, transparent, black var(--project-picker-scroll-fade), black);
    mask-image: linear-gradient(to top, transparent, black var(--project-picker-scroll-fade), black);
  }

  .project-picker-scroll-both {
    -webkit-mask-image:
      linear-gradient(
        to bottom,
        transparent,
        black var(--project-picker-scroll-fade),
        black calc(100% - var(--project-picker-scroll-fade)),
        transparent
      );
    mask-image:
      linear-gradient(
        to bottom,
        transparent,
        black var(--project-picker-scroll-fade),
        black calc(100% - var(--project-picker-scroll-fade)),
        transparent
      );
  }
</style>
