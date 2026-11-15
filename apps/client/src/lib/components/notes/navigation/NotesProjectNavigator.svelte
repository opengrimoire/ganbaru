<script lang="ts">
  import { tick, untrack } from "svelte";
  import BriefcasePlus from "@lucide/svelte/icons/briefcase-plus";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import FileText from "@lucide/svelte/icons/file-text";
  import LayersPlus from "@lucide/svelte/icons/layers-plus";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { isNewItemShortcut } from "$lib/keyboard-shortcuts";
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
    DEFAULT_PROJECT_GROUP_ICON,
    DEFAULT_PROJECT_ICON,
    type Project,
    type ProjectGroup,
  } from "$lib/projects/types";
  import { notesFoldersForProject } from "$lib/notes/navigation/tree";
  import { notesHierarchyChildren } from "$lib/notes/navigation/hierarchy-menu";
  import { notesPageSearchGroups, notesPageSearchQueryIsSearchable } from "$lib/notes/navigation/page-search";
  import { NOTES_PAGE_CHROME_EMOJI_SCALE } from "$lib/notes/pages/icon";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import { notesPagesForProject } from "$lib/notes/project-membership";
  import type { NotesPage } from "$lib/notes/types";
  import type { ProjectNavigatorPanelMode } from "$lib/projects/toolbar";
  import {
    buildProjectLists,
    projectListGroupId,
    projectListKey,
    UNGROUPED_PROJECT_LIST_KEY,
    type ProjectListKey,
  } from "$lib/projects/lists";
  import { isProjectNameTaken, projectNameKey } from "$lib/projects/text";
  import { isProjectNameTakenError } from "$lib/api/projects";
  import { useNotesWorkspace } from "$lib/stores/notes.svelte";
  import { createNotesSearchController } from "$lib/stores/notes/search.svelte";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { openInNewTabIntent, preventMiddleClickAutoscroll, type OpenInNewTabIntent } from "$lib/workspace-tabs/gestures";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import SidebarOrderMenu from "$lib/components/ui/SidebarOrderMenu.svelte";
  import StarredIconBadge from "$lib/components/ui/StarredIconBadge.svelte";
  import { cn, type MaybePromise } from "$lib/utils";
  import { createSidebarOrderDrag } from "$lib/utils/sidebar-order-drag.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import ProjectNameDraftForm from "$lib/components/projects/pickers/ProjectNameDraftForm.svelte";
  import NotesHierarchyPickerPanel from "./NotesHierarchyPickerPanel.svelte";

  /** The groups footer's create buttons, styled like the Notes picker's New folder and New note. */
  const FOOTER_CREATE_BUTTON_CLASS =
    "flex min-h-8 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-floating-item px-1 text-popover-foreground transition-colors hover:bg-accent hover:text-accent-foreground";

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

  /** A group's projects, or the projects outside every group when `group` is `null`. */
  interface NotesPickerProjectList {
    key: ProjectListKey;
    group: ProjectGroup | null;
  }

  const projects = useProjects();
  const tabs = getWorkspaceTabs();
  const preferences = getPreferences();
  const notes = useNotesWorkspace();
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
  let groupIconDraft = $state(DEFAULT_PROJECT_GROUP_ICON);
  let isCreateGroupOpen = $state(false);
  let createProjectListKey = $state<ProjectListKey | null>(null);
  let projectDraftByList = $state<Partial<Record<ProjectListKey, string>>>({});
  let projectIconDraftByList = $state<Partial<Record<ProjectListKey, string>>>({});
  /** Name key the backend refused per list, for a project the snapshot has not caught up with yet. */
  let refusedProjectNameByList = $state<Partial<Record<ProjectListKey, string>>>({});
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
  /** The list of the selected project, or of `selectedGroupId` when no project is selected. */
  const selectedListKey = $derived.by((): ProjectListKey | null => {
    if (selectedProject) return projectListKey(selectedProject.groupId);
    return selectedGroupId ? projectListKey(selectedGroupId) : null;
  });
  const trimmedSearch = $derived(projectSearch.trim());
  const isSearchActive = $derived(trimmedSearch.length > 0);
  const visibleGroups = $derived.by(() => projects.visibleGroups());
  /** Projects outside every group, listed as rows of their own after the groups. */
  const ungroupedProjects = $derived.by(() => projectsInGroup(null));
  /** The search finds notes by title through the Notes index, grouped by the listed projects in sidebar order. */
  const pageSearch = createNotesSearchController();
  const pageSearchQueryIsSearchable = $derived(notesPageSearchQueryIsSearchable(trimmedSearch));
  const pageSearchPending = $derived(
    pageSearchQueryIsSearchable && (searchedPageQuery !== trimmedSearch || pageSearch.loading),
  );
  const pageSearchGroups = $derived(notesPageSearchGroups(
    pageSearch.results,
    buildProjectLists(visibleGroups, projectsInGroup).flatMap((list) => list.projects),
  ));
  const pageSearchRowCount = $derived(
    pageSearchGroups.reduce((count, group) => count + group.pages.length + 1, 0),
  );
  const activeGroup = $derived.by(() => visibleGroups.find((group) => group.id === activeGroupId));
  const activeGroupList = $derived.by((): NotesPickerProjectList | undefined =>
    activeGroup ? { key: projectListKey(activeGroup.id), group: activeGroup } : undefined,
  );
  const directProjectList = $derived.by((): NotesPickerProjectList | undefined => {
    if (selectedListKey === null) return undefined;
    const groupId = projectListGroupId(selectedListKey);
    if (groupId === null) return { key: selectedListKey, group: null };
    const group = projects.groupById(groupId);
    return group ? { key: selectedListKey, group } : undefined;
  });
  const directProjects = $derived.by(() => directProjectList ? projectsOfList(directProjectList) : []);
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
  const projectDragList = $derived(panelMode === "groups" ? activeGroupList : directProjectList);
  /** Groups drag by their row in the custom order. */
  const groupDrag = createSidebarOrderDrag({
    getIds: () => visibleGroups.map((group) => group.id),
    enabled: () => customOrder && !isSearchActive,
    save: (orderedIds) => projects.reorderGroups(orderedIds),
    description: "project groups from the Notes project navigator",
  });
  /** Projects drag by their row in the custom order, in the main panel or the open group subpanel. */
  const projectDrag = createSidebarOrderDrag({
    getIds: () => projectDragList ? projectsOfList(projectDragList).map((project) => project.id) : [],
    enabled: () => customOrder && !isSearchActive && projectDragList !== undefined,
    isStarred: (projectId) => projects.isProjectStarred(projectId),
    save: async (orderedIds) => {
      if (projectDragList) await projects.reorderProjectsInGroup(projectDragList.group?.id ?? null, orderedIds);
    },
    description: "projects from the Notes project navigator",
  });
  /** Projects outside every group drag by their root row in the groups panel, after the groups. */
  const rootProjectDrag = createSidebarOrderDrag({
    getIds: () => ungroupedProjects.map((project) => project.id),
    enabled: () => customOrder && !isSearchActive && panelMode === "groups",
    isStarred: (projectId) => projects.isProjectStarred(projectId),
    save: (orderedIds) => projects.reorderProjectsInGroup(null, orderedIds),
    description: "projects without a group from the Notes project navigator",
  });
  $effect(() => groupDrag.reset);
  $effect(() => projectDrag.reset);
  $effect(() => rootProjectDrag.reset);

  /** A lifted group or project closes the panels it would otherwise open while passing over rows. */
  $effect(() => {
    const groupLifted = groupDrag.draggedId !== null || rootProjectDrag.draggedId !== null;
    const projectLifted = projectDrag.draggedId !== null;
    if (!groupLifted && !projectLifted) return;
    untrack(() => {
      if (groupLifted) closeProjectSubpanel();
      else closeNoteSubpanel();
    });
  });

  function rowDragActive(): boolean {
    return groupDrag.draggedId !== null
      || projectDrag.draggedId !== null
      || rootProjectDrag.draggedId !== null;
  }

  /** Projects of a group, or of no group when `groupId` is `null`. */
  function projectsInGroup(groupId: string | null): Project[] {
    return showInactiveProjects
      ? projects.projectsForGroupIncludingInactive(groupId)
      : projects.projectsForGroup(groupId);
  }

  function projectsOfList(list: NotesPickerProjectList): Project[] {
    return projectsInGroup(list.group?.id ?? null);
  }

  /** Accessible name of a project row, naming its group when it has one. */
  function selectProjectLabel(project: Project, group: ProjectGroup | null): string {
    return group
      ? t("projects.actions.selectProject", project.name, group.name)
      : t("projects.actions.selectUngroupedProject", project.name);
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
      : panelMode === "projects" && directProjectList
        ? directProjects.length
        : visibleGroups.length + ungroupedProjects.length;
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
    const projectCount = activeGroup ? projectsInGroup(activeGroup.id).length : 0;
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
    // The creator for projects outside every group lives in the main panel footer, so it outlives the subpanel.
    if (untrack(() => createProjectListKey) !== UNGROUPED_PROJECT_LIST_KEY) createProjectListKey = null;
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
      if (createProjectListKey !== UNGROUPED_PROJECT_LIST_KEY) createProjectListKey = null;
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

  /** Closes the open group subpanel, then shows a root project's note subpanel in its place. */
  function leaveGroupSubpanelFor(project: Project, target: EventTarget | null): void {
    closeProjectSubpanel();
    showNoteSubpanel(project, target, panelRootElement);
  }

  /**
   * Hovers a project outside every group in the groups panel. An open group subpanel closes now, or after the grace
   * delay while the pointer is heading into it.
   */
  function handleRootProjectPointer(project: Project, event: PointerEvent): void {
    if (rowDragActive()) return;
    if (activeGroupId === null) {
      if (event.type === "pointerenter") handleProjectPointerEnter(project, event, panelRootElement);
      else handleProjectPointerMove(project, event, panelRootElement);
      return;
    }
    const target = event.currentTarget;
    if (isPointerAimingAtProjectSubpanel(projectPickerPointerPoint(event))) {
      projectSubpanelDelay.schedule(() => leaveGroupSubpanelFor(project, target));
      return;
    }
    leaveGroupSubpanelFor(project, target);
  }

  function handleRootProjectFocus(project: Project, target: EventTarget | null): void {
    if (activeGroupId === null) showNoteSubpanel(project, target, panelRootElement);
    else leaveGroupSubpanelFor(project, target);
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

  function openPageInNewTab(pageId: string, projectId: string | null, intent: OpenInNewTabIntent): void {
    tabs.openLocation("item", { kind: "notes", projectId, item: { type: "notes-page", id: pageId } }, intent);
    if (intent.background) return;
    closeNoteSubpanel();
    onClose?.();
  }

  /** Route a search result click to a new tab when the gesture asks for one, else open the page here. */
  function handleSearchedPageClick(event: MouseEvent, page: NotesPage, projectId: string | null): void {
    const intent = tabs.canOpenTabs ? openInNewTabIntent(event) : null;
    if (intent) {
      event.preventDefault();
      openPageInNewTab(page.id, projectId, intent);
      return;
    }
    if (event.type === "click") void selectSearchedPage(page);
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
    await projects.addGroup(name, groupIconDraft);
    groupDraft = "";
    groupIconDraft = DEFAULT_PROJECT_GROUP_ICON;
    isCreateGroupOpen = false;
  }

  /** Opens the creator for a group or for projects outside every group, closing the group creator. */
  function openProjectCreator(listKey: ProjectListKey): void {
    isCreateGroupOpen = false;
    createProjectListKey = listKey;
  }

  function openGroupCreator(): void {
    if (createProjectListKey === UNGROUPED_PROJECT_LIST_KEY) createProjectListKey = null;
    isCreateGroupOpen = true;
  }

  /**
   * Runs the new item shortcut on the groups level: it clears a search, which hides the footer, and opens the creator
   * for a project outside every group. The key stays inside the picker so the Notes view does not also make a note.
   */
  function handlePanelKeydown(event: KeyboardEvent): void {
    if (panelMode !== "groups" || !isNewItemShortcut(event)) return;
    event.preventDefault();
    event.stopPropagation();
    if (createProjectListKey === UNGROUPED_PROJECT_LIST_KEY) return;
    projectSearch = "";
    openProjectCreator(UNGROUPED_PROJECT_LIST_KEY);
  }

  /** Closes the group creator or the ungrouped project creator in the groups footer. */
  function closeGroupsFooterCreator(): void {
    isCreateGroupOpen = false;
    if (createProjectListKey === UNGROUPED_PROJECT_LIST_KEY) createProjectListKey = null;
  }

  function projectDraftError(listKey: ProjectListKey): string | null {
    const draft = projectDraftByList[listKey] ?? "";
    const groupId = projectListGroupId(listKey);
    const taken = isProjectNameTaken(projects.projects, groupId, draft)
      || (projectNameKey(draft) !== "" && refusedProjectNameByList[listKey] === projectNameKey(draft));
    if (!taken) return null;
    return groupId === null
      ? t("projects.navigator.ungroupedProjectNameTaken")
      : t("projects.navigator.projectNameTaken");
  }

  async function submitProject(listKey: ProjectListKey): Promise<void> {
    const name = (projectDraftByList[listKey] ?? "").trim();
    if (!name || projectDraftError(listKey)) return;
    try {
      await projects.addProject(
        projectListGroupId(listKey),
        name,
        projectIconDraftByList[listKey] ?? DEFAULT_PROJECT_ICON,
      );
    } catch (error: unknown) {
      if (!isProjectNameTakenError(error)) throw error;
      refusedProjectNameByList = { ...refusedProjectNameByList, [listKey]: projectNameKey(name) };
      return;
    }
    projectDraftByList = { ...projectDraftByList, [listKey]: "" };
    projectIconDraftByList = { ...projectIconDraftByList, [listKey]: DEFAULT_PROJECT_ICON };
    createProjectListKey = null;
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
      // A root project row outside every group opens its notes from the main panel without a group subpanel.
      if (activeProjectPanelElement !== panelRootElement) closeNoteSubpanel();
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
    const ungroupedCount = ungroupedProjects.length;
    const resultCount = pageSearchRowCount;
    const searchPending = pageSearchPending;
    const directProjectCount = directProjects.length;
    const noteCount = activeProjectRootItems.length;
    const isCreatingGroup = isCreateGroupOpen;
    const creatingProjectListKey = createProjectListKey;
    void maxHeight;
    void hasActiveSearch;
    void mode;
    void groupCount;
    void ungroupedCount;
    void resultCount;
    void searchPending;
    void directProjectCount;
    void noteCount;
    void isCreatingGroup;
    void creatingProjectListKey;
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

{#snippet projectCreator(listKey: ProjectListKey, onCancel: (() => void) | undefined)}
  <ProjectNameDraftForm
    bind:name={() => projectDraftByList[listKey] ?? "", (value) => { projectDraftByList = { ...projectDraftByList, [listKey]: value }; }}
    bind:icon={() => projectIconDraftByList[listKey] ?? DEFAULT_PROJECT_ICON, (value) => { projectIconDraftByList = { ...projectIconDraftByList, [listKey]: value }; }}
    placeholder={t("projects.navigator.projectNamePlaceholder")}
    error={projectDraftError(listKey)}
    {iconStrokeWidth}
    onSubmit={() => void submitProject(listKey)}
    {onCancel}
  />
{/snippet}

{#snippet projectRow(project: Project, group: ProjectGroup | null, sourcePanelElement: HTMLDivElement | null | undefined)}
  {@const rootRow = group === null && panelMode === "groups"}
  <button
    type="button"
    class={cn(
      "menu-item grid grid-cols-[1rem_minmax(0,1fr)_auto]",
      project.status === "active" ? "text-popover-foreground" : "text-popover-foreground/60",
    )}
    aria-expanded={activeProjectId === project.id}
    aria-label={selectProjectLabel(project, group)}
    onpointerdown={(event) => (rootRow ? rootProjectDrag : projectDrag).pointerDown(event, project.id)}
    onpointerenter={(event) => {
      if (rootRow) handleRootProjectPointer(project, event);
      else handleProjectPointerEnter(project, event, sourcePanelElement);
    }}
    onpointermove={(event) => {
      if (rootRow) handleRootProjectPointer(project, event);
      else handleProjectPointerMove(project, event, sourcePanelElement);
    }}
    onpointerleave={handleNoteSubpanelBoundaryLeave}
    onfocus={(event) => {
      if (rootRow) handleRootProjectFocus(project, event.currentTarget);
      else showNoteSubpanel(project, event.currentTarget, sourcePanelElement);
    }}
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

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={panelRootElement}
  data-floating-root
  data-app-shortcuts="ignore"
  onkeydown={handlePanelKeydown}
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
          <Notice class="px-3 py-2" message={t("projects.loadFailed")} copy={projects.loadError} onRetry={() => projects.load()} />
        {:else if isSearchActive && pageSearch.error}
          <Notice class="px-3 py-2" message={t("notes.searchFailed")} copy={pageSearch.error} />
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
                      onmousedown={preventMiddleClickAutoscroll}
                      onclick={(event) => handleSearchedPageClick(event, page, resultGroup.project?.id ?? null)}
                      onauxclick={(event) => handleSearchedPageClick(event, page, resultGroup.project?.id ?? null)}
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
          {#if visibleGroups.length === 0 && ungroupedProjects.length === 0}
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
              {#each ungroupedProjects as project (project.id)}
                <div class="pl-1 pr-1" {@attach rootProjectDrag.item(project.id)}>
                  {@render projectRow(project, null, panelRootElement)}
                </div>
              {/each}
            </div>
          {/if}
        {:else if !directProjectList}
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
                {@render projectRow(project, directProjectList.group, panelRootElement)}
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
          <ProjectNameDraftForm
            bind:name={groupDraft}
            bind:icon={groupIconDraft}
            placeholder={t("projects.navigator.groupNamePlaceholder")}
            {iconStrokeWidth}
            onSubmit={() => void submitGroup()}
            onCancel={closeGroupsFooterCreator}
          />
        {:else if createProjectListKey === UNGROUPED_PROJECT_LIST_KEY}
          {@render projectCreator(UNGROUPED_PROJECT_LIST_KEY, closeGroupsFooterCreator)}
        {:else}
          <div class="flex min-h-8 items-center">
            <button type="button" class={FOOTER_CREATE_BUTTON_CLASS} onclick={openGroupCreator}>
              <LayersPlus size={13} strokeWidth={iconStrokeWidth} class="shrink-0" aria-hidden="true" />
              <span class="truncate">{t("calendar.eventPanel.newGroup")}</span>
            </button>
            <div class="mx-1 h-5 border-l border-border/70" aria-hidden="true"></div>
            <button
              type="button"
              class={FOOTER_CREATE_BUTTON_CLASS}
              aria-keyshortcuts="Control+N Meta+N"
              onclick={() => openProjectCreator(UNGROUPED_PROJECT_LIST_KEY)}
            >
              <BriefcasePlus size={13} strokeWidth={iconStrokeWidth} class="shrink-0" aria-hidden="true" />
              <span class="truncate">{t("calendar.eventPanel.newProject")}</span>
            </button>
          </div>
        {/if}
      {:else if directProjectList}
        {#if createProjectListKey === directProjectList.key}
          {@render projectCreator(directProjectList.key, undefined)}
        {:else}
          <button
            type="button"
            class="menu-item justify-center text-popover-foreground"
            onclick={() => openProjectCreator(directProjectList.key)}
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
  {@const activeGroupProjects = projectsInGroup(activeGroup.id)}
  {@const activeGroupListKey = projectListKey(activeGroup.id)}
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
    data-floating-root
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
      {#if createProjectListKey === activeGroupListKey}
        {@render projectCreator(activeGroupListKey, undefined)}
      {:else}
        <button
          type="button"
          class="menu-item justify-center text-popover-foreground"
          onclick={() => {
            openProjectCreator(activeGroupListKey);
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
    onPageOpenInNewTab={tabs.canOpenTabs ? (pageId, intent) => openPageInNewTab(pageId, activeProjectId, intent) : undefined}
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
