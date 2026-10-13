<script lang="ts">
  import { onMount } from "svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getCalendar } from "$lib/stores/calendar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import type { CalendarEvent, CalendarViewMode } from "$lib/calendar/types";
  import type {
    Project,
    ProjectChatIntegration,
    ProjectSection,
    ProjectTask,
    ProjectView,
    ProjectViewId,
  } from "$lib/projects/types";
  import {
    projectCalendarCreateDefaults as buildProjectCalendarCreateDefaults,
    projectEventDurationMinutesInDateRange,
  } from "$lib/projects/scheduling/schedule";
  import {
    pickProjectTaskModalLayout,
    projectToolbarPanelsForView,
    type ProjectToolbarPanel,
  } from "$lib/projects/toolbar";
  import { projectPlaceShortcut } from "$lib/projects/places";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ProjectEmptyState from "./ProjectEmptyState.svelte";
  import ProjectWorkspaceHeader from "./ProjectWorkspaceHeader.svelte";
  import ProjectsSidebar from "./sidebar/ProjectsSidebar.svelte";
  import { projectViewLabel } from "./sidebar/project-view-presentation";
  import ProjectTaskDetailPanel from "./task-detail/ProjectTaskDetailPanel.svelte";
  import type { ProjectViewComponents } from "./view-components";
  import { ProjectTaskQueryController } from "./task-query-controller.svelte";
  import { ProjectRouteLoadController } from "./route-load-controller.svelte";
  import { ProjectRouteUiController } from "./route-ui-controller.svelte";

  let {
    mobileLayout = false,
    viewComponents = null,
    projectChat = null,
  }: {
    mobileLayout?: boolean;
    viewComponents?: ProjectViewComponents | null;
    projectChat?: ProjectChatIntegration | null;
  } = $props();

  const projects = getProjects();
  const mobileBackStack = getMobileBackStack();
  const calendar = getCalendar();
  const preferences = getPreferences();
  const viewport = getViewport();
  const { t } = getLocalization();
  let projectViewControlsTarget = $state<HTMLElement | null>(null);
  const taskQuery = new ProjectTaskQueryController({ projects, calendar, translate: t });
  const projectCalendarViewMode = $derived<CalendarViewMode>(taskQuery.calendarMode ?? (mobileLayout ? "day" : "week"));
  const routeLoad = new ProjectRouteLoadController(projects);
  const routeUi = new ProjectRouteUiController();


  const toolbarLoadState = $derived(routeLoad.optionalState("toolbar"));
  const bulkActionsLoadState = $derived(routeLoad.optionalState("bulk-actions"));
  const taskFinderLoadState = $derived(routeLoad.optionalState("task-finder"));

  const selectedProject = $derived(projects.selectedProject);
  const selectedGroup = $derived(projects.selectedGroup);
  const selectedProjectId = $derived(selectedProject?.id ?? null);
  const sidebarDocked = $derived(!mobileLayout && Boolean(selectedProject && selectedGroup));
  const sidebarViews = $derived(projects.viewsForProject(selectedProjectId));
  /** Chat opens from the sidebar when the project has a Chat view, so the header keeps its shortcut only otherwise. */
  const chatIsPlace = $derived(sidebarDocked && sidebarViews.some((view) => view.viewType === "chat"));
  const toolbarDataReady = $derived.by(() => {
    const projectId = selectedProjectId;
    return Boolean(
      projectId
      && projects.taskViewPage?.projectId === projectId
      && projects.isProjectOptionalDataLoaded("view_preferences", projectId),
    );
  });
  const taskDetailDataReady = $derived.by(() => {
    if (routeUi.taskDraftProjectId) return true;
    return Boolean(
      routeUi.selectedTaskId
      && projects.taskById(routeUi.selectedTaskId)?.detailLoaded,
    );
  });

  $effect(() => {
    if (!selectedProjectId || projects.isProjectDataLoaded(selectedProjectId)) return;
    void projects.ensureProjectData(selectedProjectId).catch((error) => {
      console.error("load selected project data failed", error);
    });
  });
  const inactiveSectionCount = $derived(taskQuery.inactiveSectionCount);
  const sections = $derived(taskQuery.sections);
  const statuses = $derived(taskQuery.statuses);
  const priorities = $derived(taskQuery.priorities);
  const archivedProjectTaskCount = $derived(taskQuery.archivedTaskCount);
  const allProjectTasks = $derived(taskQuery.allTasks);
  const projectTags = $derived(taskQuery.projectTags);
  const projectCustomFields = $derived(taskQuery.customFields);
  const todayDate = $derived(taskQuery.today);
  const scheduledTaskIds = $derived(taskQuery.scheduledTaskIds);
  const allProjectEvents = $derived(taskQuery.allEvents);

  function projectCalendarCreateDefaults(input: {
    start: string;
    end: string;
    allDay?: boolean;
  }): Partial<CalendarEvent> {
    return buildProjectCalendarCreateDefaults({
      project: selectedProject,
      ...input,
      globalIdleDefaults: {
        idlePauseEnabled: preferences.focusIdlePauseOnEventCreate,
        idleThresholdMinutes: preferences.focusIdleThresholdMinutes,
      },
    });
  }
  const tasks = $derived(taskQuery.tasks);
  const listTaskGroups = $derived(taskQuery.listGroups);
  const selectableTasks = $derived.by(() => tasks.filter((task) => !task.parentTaskId));
  const selectedTaskIdSet = $derived.by(() => new Set(routeUi.selectedTaskIds));
  const selectedTasks = $derived.by(() => allProjectTasks.filter((task) => selectedTaskIdSet.has(task.id)));
  const selectedArchivedTaskCount = $derived(selectedTasks.filter((task) => Boolean(task.archivedAt)).length);
  const selectedActiveTaskCount = $derived(selectedTasks.length - selectedArchivedTaskCount);
  const matchingTaskCount = $derived(taskQuery.matchingTaskCount);
  const taskFiltersActive = $derived(taskQuery.filtersActive);
  const taskFilterControlsActive = $derived(taskQuery.filterControlsActive);
  const taskGroupingActive = $derived(taskQuery.groupingActive);
  const taskCustomizeActive = $derived(taskQuery.customizeActive);
  const scheduledThisWeekMinutes = $derived.by(() => thisWeekScheduledMinutes());
  const selectedTask = $derived.by(() =>
    routeUi.selectedTaskId ? allProjectTasks.find((task) => task.id === routeUi.selectedTaskId) : undefined
  );
  const taskListColumnControls = $derived(taskQuery.columnControls);
  const taskDetailModalLayout = $derived(pickProjectTaskModalLayout({
    viewportWidth: viewport.width,
    viewportHeight: viewport.height,
  }));

  $effect(() => {
    if (!mobileLayout || taskDetailDataReady) return;
    if (routeUi.selectedTaskId === null && routeUi.taskDraftProjectId === null) return;
    return mobileBackStack.activate({
      handle: () => {
        routeUi.closeTaskDetail();
        routeLoad.invalidateTaskDetailData();
      },
    });
  });

  $effect(() => {
    if (!mobileLayout || !routeUi.taskFinderOpen) return;
    return mobileBackStack.activate({
      handle: () => {
        routeUi.closeFinder();
      },
    });
  });

  $effect(() => {
    if (!mobileLayout || routeUi.toolbarPanel === null) return;
    return mobileBackStack.activate({
      handle: () => {
        routeUi.requestToolbarClose();
      },
    });
  });

  onMount(() => {
    void projects.ensureLoaded().catch((error) => {
      console.error("load projects failed", error);
    });
    return () => taskQuery.dispose();
  });

  let viewDataFailure = $state<{ projectId: string; view: ProjectViewId } | null>(null);
  const viewDataFailed = $derived(
    viewDataFailure?.projectId === selectedProjectId && viewDataFailure?.view === projects.activeTaskView,
  );

  function requestViewData(projectId: string, view: ProjectViewId): void {
    viewDataFailure = null;
    void projects.ensureProjectViewData(projectId, view).catch((error) => {
      viewDataFailure = { projectId, view };
      console.error(`load optional Project ${view} data failed`, error);
    });
  }

  $effect(() => {
    const view = projects.activeTaskView;
    if (!selectedProject || !selectedGroup || !view) return;
    requestViewData(selectedProject.id, view);
  });

  $effect(() => {
    taskQuery.syncViewQuery();
  });

  $effect(() => {
    taskQuery.persistViewQuery();
  });

  $effect(() => {
    projects.activeTaskView;
    taskQuery.queryReady;
    taskQuery.filterState;
    taskQuery.sections;
    taskQuery.allEvents;
    taskQuery.loadCurrent(routeUi.selectedTaskIds, routeUi.selectedTaskId);
  });

  $effect(() => {
    if (!routeUi.toolbarPanel) return;
    routeLoad.requestOptional("toolbar");
    const projectId = selectedProjectId;
    void routeLoad.requestToolbarData(projectId, () => (
      routeUi.toolbarPanel !== null && selectedProjectId === projectId
    ));
  });

  $effect(() => {
    const panel = routeUi.toolbarPanel;
    if (!panel) return;
    const view = projects.activeTaskView;
    const available: readonly ProjectToolbarPanel[] = view ? projectToolbarPanelsForView(view) : ["settings"];
    if (!available.includes(panel)) routeUi.closeToolbarImmediately();
  });

  $effect(() => {
    if (routeUi.selectedTaskIds.length > 0) {
      routeLoad.requestOptional("bulk-actions");
    }
  });

  $effect(() => {
    if (!mobileLayout && (routeUi.taskFinderOpen || taskQuery.search.trim())) {
      routeLoad.requestOptional("task-finder");
    }
  });

  $effect(() => {
    if (routeUi.taskDraftProjectId && routeUi.taskDraftProjectId !== selectedProjectId) {
      routeUi.taskDraftProjectId = null;
    }
  });

  $effect(() => {
    if (!routeUi.selectedTaskId) return;
    void routeLoad.requestTaskDetailData(selectedProjectId, routeUi.selectedTaskId);
  });

  $effect(() => {
    selectedProjectId;
    projects.viewPreferences;
    projectCustomFields;
    taskQuery.syncListPreferences();
  });

  $effect(() => {
    if (routeUi.selectedTaskId && !selectedTask) {
      routeUi.selectedTaskId = null;
      routeLoad.invalidateTaskDetailData();
    }
  });

  $effect(() => {
    sections;
    projectTags;
    projectCustomFields;
    taskQuery.queryReady;
    projects.taskViewPage;
    taskQuery.repairDisappearingFields();
  });

  function selectView(view: ProjectView): void {
    routeUi.runAfterSettingsClose(() => {
      projects.selectView(view);
    });
  }

  /** Switches project views with the Ctrl and digit shortcut, even while typing; the app shell handles hosted views. */
  function handlePlaceShortcut(event: KeyboardEvent): void {
    if (event.defaultPrevented || !projects.selectedProject) return;
    const view = projectPlaceShortcut(event, sidebarViews);
    if (!view) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    selectView(view);
  }

  function handleProjectSelected(): void {
    routeUi.closeTaskDetail();
    routeUi.closeToolbarImmediately();
  }

  /** Selects a project from its sidebar menu and opens its settings, asking first when other settings are unsaved. */
  function openProjectSettings(project: Project): void {
    if (project.id === projects.selectedProjectId && routeUi.toolbarPanel === "settings") return;
    routeUi.runAfterSettingsClose(() => {
      void (async () => {
        try {
          if (project.id !== projects.selectedProjectId) await projects.selectProject(project.id);
          routeUi.closeTaskDetail();
          routeUi.setToolbarPanel("settings");
        } catch (error: unknown) {
          console.error("Failed to open project settings from the sidebar", error);
        }
      })();
    });
  }

  function clearTaskFilters(): void {
    taskQuery.reset();
  }

  /** Return the localized name of a toolbar panel, used when the panel cannot open. */
  function toolbarPanelLabel(panel: ProjectToolbarPanel): string {
    if (panel === "settings") return t("projects.header.projectSettings");
    if (panel === "filters") return t("projects.filters.title");
    if (panel === "sort") return t("projects.toolbar.sort");
    if (panel === "customize") return t("projects.toolbar.customize");
    return t("projects.toolbar.group");
  }

  /** Retry whichever part of the toolbar panel failed: its project data or its lazy component. */
  function retryToolbarPanel(): Promise<void> | void {
    if (!routeLoad.toolbarDataError) {
      routeLoad.requestOptional("toolbar", true);
      return;
    }
    const projectId = selectedProjectId;
    return routeLoad.requestToolbarData(projectId, () => (
      routeUi.toolbarPanel !== null && selectedProjectId === projectId
    ));
  }

  /** Opens a task created from the list without touching the view's saved query, even when its filters hide the task. */
  function revealCreatedTask(task: ProjectTask | undefined): void {
    if (!task) return;
    routeUi.selectedTaskId = task.id;
    routeUi.clearTaskSelection();
  }


  async function toggleSectionCollapsed(section: ProjectSection): Promise<void> {
    await projects.updateSection(section, { collapsed: !section.collapsed });
  }

  function thisWeekScheduledMinutes(): number {
    return projectEventDurationMinutesInDateRange(allProjectEvents, todayDate, taskQuery.weekEnd);
  }

</script>

<svelte:window
  onkeydowncapture={handlePlaceShortcut}
  onkeydown={(event) => routeUi.handleWindowKeydown(event)}
/>

<svelte:document
  onselectstart={(event) => routeUi.handleDocumentSelectStart(event)}
  onselectionchange={() => routeUi.handleDocumentSelectionChange()}
/>

<div
  bind:this={routeUi.rootElement}
  class="projects-view-root relative flex h-full min-h-0 overflow-hidden text-foreground"
  class:explorer-chrome={sidebarDocked}
  style:background-color={sidebarDocked ? undefined : "var(--cal-bg)"}
  data-first-use-shell="projects"
>
  {#if sidebarDocked}
    <ProjectsSidebar
      onSelectView={selectView}
      onProjectSelected={handleProjectSelected}
      onOpenProjectSettings={openProjectSettings}
    />
  {/if}
  <section
    class="flex min-w-0 flex-1 flex-col overflow-hidden"
    class:surface-content={sidebarDocked}
    style="background-color: var(--cal-bg);"
  >
    {#if selectedProject && selectedGroup}
      <header class="flex shrink-0 flex-col" style="background-color: var(--cal-bg);">
        <ProjectWorkspaceHeader
          {selectedProject}
          {selectedGroup}
          {selectedProjectId}
          showInactiveProjects={projects.showInactiveProjects}
          projectToolbarPanel={routeUi.toolbarPanel}
          {taskGroupingActive}
          taskFiltersActive={taskFilterControlsActive}
          {taskCustomizeActive}
          {mobileLayout}
          projectChat={chatIsPlace ? null : projectChat}
          bind:viewControlsTarget={projectViewControlsTarget}
          onShowInactiveProjectsChange={(value) => {
            projects.showInactiveProjects = value;
          }}
          onProjectSelected={handleProjectSelected}
          onToggleToolbarPanel={(panel) => routeUi.toggleToolbarPanel(panel)}
          onCreateTask={() => { if (selectedProjectId) routeUi.openTaskDraft(selectedProjectId); }}
        />
        {#if routeUi.toolbarPanel}
          {#if toolbarDataReady && toolbarLoadState?.status === "ready" && toolbarLoadState.component.kind === "toolbar"}
            {@const ProjectToolbarPanels = toolbarLoadState.component.component}
            <ProjectToolbarPanels
              {taskQuery}
              {mobileLayout}
              panel={routeUi.toolbarPanel}
              projectId={selectedProjectId}
              {sections}
              {priorities}
              {projectTags}
              {projectCustomFields}
              {taskListColumnControls}
              {archivedProjectTaskCount}
              {inactiveSectionCount}
              {taskFiltersActive}
              listColumnsSaving={taskQuery.listColumnsSaving || taskQuery.propertySaving}
              listColumnsError={taskQuery.listColumnsError}
              bind:taskStatusFilter={taskQuery.statusFilter}
              bind:taskSectionFilter={taskQuery.sectionFilter}
              bind:taskPriorityFilter={taskQuery.priorityFilter}
              bind:taskDueFilter={taskQuery.dueFilter}
              bind:taskDueRangeStart={taskQuery.dueRangeStart}
              bind:taskDueRangeEnd={taskQuery.dueRangeEnd}
              bind:taskScheduleFilter={taskQuery.scheduleFilter}
              bind:taskDependencyFilter={taskQuery.dependencyFilter}
              bind:taskTagFilter={taskQuery.tagFilter}
              bind:taskCustomFieldFilters={taskQuery.customFieldFilters}
              bind:taskGroupBy={taskQuery.groupBy}
              bind:taskSortMode={taskQuery.sortMode}
              bind:taskSortDirection={taskQuery.sortDirection}
              bind:showArchivedTasks={taskQuery.showArchivedTasks}
              bind:showInactiveSections={taskQuery.showInactiveSections}
              onClose={() => {
                routeUi.requestToolbarClose();
              }}
              onProjectSettingsDirtyChange={(dirty) => {
                routeUi.settingsDirty = dirty;
              }}
              onRevealInactive={() => {
                projects.showInactiveProjects = true;
              }}
              onClearTaskFilters={clearTaskFilters}
              onToggleTaskListColumn={(column) => { void taskQuery.toggleColumn(column); }}
            />
          {:else if routeLoad.toolbarDataError || toolbarLoadState?.status === "failed"}
            {@const panelLabel = toolbarPanelLabel(routeUi.toolbarPanel)}
            <CollectionSettings
              label={panelLabel}
              anchor={document.querySelector<HTMLElement>(`[data-project-toolbar-trigger="${routeUi.toolbarPanel}"]`)}
              onClose={() => routeUi.requestToolbarClose()}
            >
              <Notice
                class="px-1.5 py-1"
                message={t("common.viewLoadFailed", panelLabel)}
                copy={routeLoad.toolbarDataError ?? false}
                onRetry={retryToolbarPanel}
              />
            </CollectionSettings>
          {:else}
            <span class="sr-only" aria-busy="true">{t("common.loading")}</span>
          {/if}
        {/if}
      </header>

    {:else}
      <header
        data-projects-shell-header
        class="flex shrink-0 items-center px-4 text-sm font-semibold"
        style="height: var(--toolbar-h); background-color: var(--cal-bg);"
      >
        {t("titleBar.tab.projects")}
      </header>
    {/if}

    <div
      data-projects-content-frame
      data-collection-selection-scope
      class="relative min-h-0 flex-1"
      style="background-color: var(--cal-bg);"
    >
      {#if selectedProject && selectedGroup}
        {#if routeUi.selectedTaskIds.length > 0}
          <!-- The selection toolbar floats over the bottom of the header so selecting tasks never shifts them. -->
          <div class="pointer-events-none absolute right-3 bottom-full left-3 z-30 mb-1.5 flex flex-col items-start gap-1 *:pointer-events-auto">
          {#if bulkActionsLoadState?.status === "ready" && bulkActionsLoadState.component.kind === "bulk-actions"}
            {@const ProjectBulkActionController = bulkActionsLoadState.component.component}
            <ProjectBulkActionController
              {selectedProject}
              {statuses}
              {priorities}
              {selectedTasks}
              {selectableTasks}
              {selectedActiveTaskCount}
              {selectedArchivedTaskCount}
              bind:selectedTaskIds={routeUi.selectedTaskIds}
              bind:showArchivedTasks={taskQuery.showArchivedTasks}
            />
          {:else if bulkActionsLoadState?.status === "failed"}
            <Notice
              class="surface-floating px-2.5 py-1.5"
              message={t("common.viewLoadFailed", t("collections.selection.selected", routeUi.selectedTaskIds.length))}
              copy={false}
              onRetry={() => routeLoad.requestOptional("bulk-actions", true)}
              onDismiss={() => routeUi.clearTaskSelection()}
            />
          {:else}
            <span class="sr-only" aria-busy="true">{t("common.loading")}</span>
          {/if}
          </div>
        {/if}
          {#if projects.activeTaskView === null}
            <p class="flex h-full items-center justify-center p-4 text-center text-sm text-muted-foreground">
              {t("projects.sidebar.noViews")}
            </p>
          {:else if !taskQuery.queryReady}
            {#if viewDataFailed}
              <Notice
                layout="fill"
                class="text-sm"
                message={t("common.viewLoadFailed", projects.activeProjectView ? projectViewLabel(t, projects.activeProjectView) : "")}
                copy={false}
                onRetry={() => {
                  if (selectedProjectId && projects.activeTaskView) requestViewData(selectedProjectId, projects.activeTaskView);
                }}
              />
            {:else}
              <Loading layout="fill" />
            {/if}
          {:else}
          {#key projects.activeViewId}
          {#if projects.activeTaskView === "list"}
            {@const ProjectListView = viewComponents?.list ?? null}
            {#if ProjectListView}
            <ProjectListView
              {taskQuery}
              {mobileLayout}
              {selectedProjectId}
              {sections}
              {statuses}
              {priorities}
              {tasks}
              {allProjectTasks}
              {listTaskGroups}
              taskGroupBy={taskQuery.groupBy}
              taskSortMode={taskQuery.sortMode}
              taskSortDirection={taskQuery.sortDirection}
              taskListColumns={taskQuery.listColumns}
              taskListColumnWidths={taskQuery.listColumnWidths}
              {projectCustomFields}
              selectedTaskId={routeUi.selectedTaskId}
              selectedTaskIds={routeUi.selectedTaskIds}
              showArchivedTasks={taskQuery.showArchivedTasks}
              onOpenTask={(task) => routeUi.openTask(task)}
              onSelectedTaskIdsChange={(taskIds) => {
                routeUi.selectedTaskIds = taskIds;
              }}
              onRevealTask={revealCreatedTask}
              onTaskListColumnWidthsChange={(widths, options) => taskQuery.updateColumnWidths(widths, options)}
              onNeedMore={() => taskQuery.loadNextList(routeUi.selectedTaskIds)}
            />
            {:else}
              <Loading layout="fill" />
            {/if}
          {:else if projects.activeTaskView === "kanban" && viewComponents?.kanban}
            {@const ProjectKanbanView = viewComponents.kanban}
            {#if ProjectKanbanView}
            <ProjectKanbanView
              {mobileLayout}
              {tasks}
              {statuses}
              {priorities}
              selectedTaskIds={routeUi.selectedTaskIds}
              taskSortMode={taskQuery.sortMode}
              taskSortDirection={taskQuery.sortDirection}
              onOpenTask={(task) => routeUi.openTask(task)}
              onToggleTaskSelection={(task) => routeUi.toggleTaskSelection(task)}
              columnCounts={projects.taskViewPage?.columnCounts ?? []}
              onNeedMore={() => taskQuery.loadNextKanban(routeUi.selectedTaskIds)}
            />
            {/if}
          {:else if projects.activeTaskView === "calendar" && viewComponents?.calendar}
            {@const CalendarView = viewComponents.calendar}
            {#if CalendarView}
            <div class="h-full min-h-112 overflow-hidden">
              <CalendarView
                eventFilter={(event) => taskQuery.eventMatches(event)}
                createDefaults={projectCalendarCreateDefaults}
                initialViewMode={projectCalendarViewMode}
                onViewModeChange={(mode) => {
                  taskQuery.calendarMode = mode;
                }}
                {mobileLayout}
                headerTarget={projectViewControlsTarget}
              />
            </div>
            {/if}
          {:else if projects.activeTaskView === "gantt" && viewComponents?.gantt}
            {@const ProjectGanttView = viewComponents.gantt}
            {#if ProjectGanttView}
            <ProjectGanttView
              tasks={tasks}
              statuses={statuses}
              sections={sections}
              todayDate={todayDate}
              onOpenTask={(task) => routeUi.openTask(task)}
              onToggleSectionCollapsed={(section) => {
                void toggleSectionCollapsed(section);
              }}
            />
            {/if}
          {:else if projects.activeTaskView === "dashboard" && viewComponents?.dashboard}
            {@const ProjectDashboardView = viewComponents.dashboard}
            {#if ProjectDashboardView}
            <ProjectDashboardView
              {mobileLayout}
              projectId={selectedProjectId}
              {tasks}
              {statuses}
              {priorities}
              {todayDate}
              {scheduledTaskIds}
              {scheduledThisWeekMinutes}
              aggregates={projects.taskViewPage?.aggregates}
              onOpenTask={(task) => routeUi.openTask(task)}
            />
            {/if}
          {:else}
            <Loading layout="fill" />
          {/if}
          {/key}
          {/if}
      {:else}
        <ProjectEmptyState
          {selectedProjectId}
          bind:showInactiveProjects={projects.showInactiveProjects}
          {mobileLayout}
          onProjectSelected={handleProjectSelected}
        />
      {/if}
    </div>
  </section>

  {#if !mobileLayout && selectedProject && (routeUi.taskFinderOpen || taskQuery.search.trim().length > 0)}
    {#if taskFinderLoadState?.status === "ready" && taskFinderLoadState.component.kind === "task-finder"}
      {@const ProjectTaskFinder = taskFinderLoadState.component.component}
      <ProjectTaskFinder
        taskSearch={taskQuery.search}
        {matchingTaskCount}
        totalTaskCount={projects.taskViewPage?.totalCount ?? allProjectTasks.length}
        focusRequestId={routeUi.taskFinderFocusRequestId}
        onTaskSearchChange={(value) => {
          taskQuery.search = value;
        }}
        onClose={() => routeUi.closeFinder()}
        onClearAndClose={() => routeUi.closeOrClearFinder(taskQuery.search, () => { taskQuery.search = ""; })}
      />
    {:else if taskFinderLoadState?.status === "failed"}
      <div class="pointer-events-none absolute inset-x-0 bottom-4 z-40 flex justify-center px-3">
        <Notice
          class="pointer-events-auto surface-floating w-[min(32rem,100%)] px-3 py-2.5"
          message={t("common.viewLoadFailed", t("projects.finder.label"))}
          copy={false}
          onRetry={() => routeLoad.requestOptional("task-finder", true)}
          onDismiss={() => routeUi.closeOrClearFinder(taskQuery.search, () => { taskQuery.search = ""; })}
        />
      </div>
    {:else}
      <span class="sr-only" aria-busy="true">{t("common.loading")}</span>
    {/if}
  {/if}

  {#if routeUi.selectedTaskId || routeUi.taskDraftProjectId}
    {#if taskDetailDataReady}
      <ProjectTaskDetailPanel
        taskId={routeUi.selectedTaskId}
        draftProjectId={routeUi.taskDraftProjectId}
        onCreated={(task) => routeUi.completeTaskDraft(task)}
        layout={taskDetailModalLayout}
        showArchivedTasks={taskQuery.showArchivedTasks}
        showInactiveSections={taskQuery.showInactiveSections}
        onClose={() => {
          routeUi.closeTaskDetail();
          routeLoad.invalidateTaskDetailData();
        }}
        onOpenTask={(taskId) => {
          routeUi.selectedTaskId = taskId;
        }}
        onShowArchivedTasks={() => {
          taskQuery.showArchivedTasks = true;
        }}
      />
    {:else if routeLoad.taskDetailDataError}
      <div class="absolute inset-0 z-80 surface-backdrop flex items-center justify-center p-4">
        <Notice
          class="surface-dialog max-w-sm px-4 py-3 text-sm"
          message={t("common.viewLoadFailed", t("projects.detail.title"))}
          copy={routeLoad.taskDetailDataError}
          onRetry={() => routeLoad.requestTaskDetailData(selectedProjectId, routeUi.selectedTaskId)}
          onDismiss={() => {
            routeUi.closeTaskDetail();
            routeLoad.invalidateTaskDetailData();
          }}
        />
      </div>
    {:else}
      <div class="absolute inset-0 z-80 surface-backdrop flex items-center justify-center p-4">
        <Loading layout="fill" />
      </div>
    {/if}
  {/if}

  {#if routeUi.discardConfirmOpen}
    <ConfirmDialog
      title={t("calendar.view.discardUnsavedTitle")}
      message={t("calendar.view.changesLost")}
      confirmLabel={t("calendar.view.discard")}
      cancelLabel={t("common.cancel")}
      onConfirm={() => routeUi.confirmDiscard()}
      onCancel={() => routeUi.cancelDiscard()}
    />
  {/if}
</div>

<style>
  :global(.projects-view-root),
  :global(.projects-view-root *) {
    user-select: none;
  }

  :global(.projects-view-root input),
  :global(.projects-view-root textarea),
  :global(.projects-view-root [contenteditable="true"]),
  :global(.projects-view-root [contenteditable="true"] *) {
    user-select: text;
  }
</style>
