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
    ProjectChatIntegration,
    ProjectSection,
    ProjectStatus,
    ProjectTask,
    ProjectViewId,
  } from "$lib/projects/types";
  import {
    projectCalendarCreateDefaults as buildProjectCalendarCreateDefaults,
    projectEventDurationMinutesInDateRange,
  } from "$lib/projects/scheduling/schedule";
  import {
    pickProjectTaskModalLayout,
  } from "$lib/projects/toolbar";
  import ProjectEmptyState from "./ProjectEmptyState.svelte";
  import ProjectWorkspaceHeader from "./ProjectWorkspaceHeader.svelte";
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
  let showInactiveProjects = $state(false);
  let projectCalendarViewMode = $state<CalendarViewMode>("week");
  let projectCalendarViewModeInitialized = false;

  $effect(() => {
    if (projectCalendarViewModeInitialized) return;
    projectCalendarViewModeInitialized = true;
    projectCalendarViewMode = mobileLayout ? "day" : "week";
  });
  const taskQuery = new ProjectTaskQueryController({ projects, calendar, translate: t });
  const routeLoad = new ProjectRouteLoadController(projects);
  const routeUi = new ProjectRouteUiController({
    setActiveView: (view) => { projects.activeView = view; },
  });


  const toolbarLoadState = $derived(routeLoad.optionalState("toolbar"));
  const bulkActionsLoadState = $derived(routeLoad.optionalState("bulk-actions"));
  const taskFinderLoadState = $derived(routeLoad.optionalState("task-finder"));

  const selectedProject = $derived(projects.selectedProject);
  const selectedGroup = $derived(projects.selectedGroup);
  const selectedProjectId = $derived(selectedProject?.id ?? null);
  const toolbarDataReady = $derived.by(() => {
    const projectId = selectedProjectId;
    return Boolean(
      projectId
      && projects.taskViewPage?.projectId === projectId
      && projects.isProjectOptionalDataLoaded("saved_views", projectId),
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
  const savedTaskViews = $derived(taskQuery.savedViews);
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
  });

  let viewDataFailure = $state<{ projectId: string; view: ProjectViewId } | null>(null);
  const viewDataFailed = $derived(
    viewDataFailure?.projectId === selectedProjectId && viewDataFailure?.view === projects.activeView,
  );

  function requestViewData(projectId: string, view: ProjectViewId): void {
    viewDataFailure = null;
    void projects.ensureProjectViewData(projectId, view).catch((error) => {
      viewDataFailure = { projectId, view };
      console.error(`load optional Project ${view} data failed`, error);
    });
  }

  $effect(() => {
    const view = projects.activeView;
    if (!selectedProject || !selectedGroup) return;
    requestViewData(selectedProject.id, view);
  });

  $effect(() => {
    projects.activeView;
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
    taskQuery.repairDisappearingFields();
  });

  function terminalStatus(): ProjectStatus | undefined {
    return statuses.find((status) => status.terminal);
  }

  function firstOpenStatus(): ProjectStatus | undefined {
    return statuses.find((status) => status.name.toLowerCase() === "to do")
      ?? statuses.find((status) => !status.terminal);
  }

  function clearTaskFilters(): void {
    taskQuery.reset();
  }

  function revealCreatedTask(task: ProjectTask | undefined): void {
    if (!task) return;
    projects.activeView = "list";
    taskQuery.reset();
    routeUi.selectedTaskId = task.id;
    routeUi.clearTaskSelection();
    taskQuery.showArchivedTasks = false;
    taskQuery.showInactiveSections = false;
  }


  async function toggleSectionCollapsed(section: ProjectSection): Promise<void> {
    await projects.updateSection(section, { collapsed: !section.collapsed });
  }

  function thisWeekScheduledMinutes(): number {
    return projectEventDurationMinutesInDateRange(allProjectEvents, todayDate, taskQuery.weekEnd);
  }

</script>

<svelte:window onkeydown={(event) => routeUi.handleWindowKeydown(event)} />

<svelte:document
  onselectstart={(event) => routeUi.handleDocumentSelectStart(event)}
  onselectionchange={() => routeUi.handleDocumentSelectionChange()}
/>

<div
  bind:this={routeUi.rootElement}
  class="projects-view-root relative flex h-full min-h-0 overflow-hidden text-foreground"
  style="background-color: var(--cal-bg);"
  data-first-use-shell="projects"
>
  <section class="flex min-w-0 flex-1 flex-col">
    {#if selectedProject && selectedGroup}
      <header class="flex shrink-0 flex-col" style="background-color: var(--cal-header-bg);">
        <ProjectWorkspaceHeader
          {selectedProject}
          {selectedGroup}
          {selectedProjectId}
          {showInactiveProjects}
          projectToolbarPanel={routeUi.toolbarPanel}
          {taskGroupingActive}
          taskFiltersActive={taskFilterControlsActive}
          {taskCustomizeActive}
          {mobileLayout}
          {projectChat}
          onShowInactiveProjectsChange={(value) => {
            showInactiveProjects = value;
          }}
          onProjectSelected={() => {
            routeUi.closeTaskDetail();
            routeUi.closeToolbarImmediately();
          }}
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
              {savedTaskViews}
              {taskListColumnControls}
              {archivedProjectTaskCount}
              {inactiveSectionCount}
              {taskFiltersActive}
              savedViewSaving={taskQuery.savedViewSaving}
              savedViewError={taskQuery.savedViewError}
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
              bind:savedViewNameDraft={taskQuery.savedViewNameDraft}
              onClose={() => {
                routeUi.requestToolbarClose();
              }}
              onProjectSettingsDirtyChange={(dirty) => {
                routeUi.settingsDirty = dirty;
              }}
              onRevealInactive={() => {
                showInactiveProjects = true;
              }}
              onClearTaskFilters={clearTaskFilters}
              onSaveCurrentTaskView={() => { void taskQuery.saveCurrentView(); }}
              onApplyTaskView={(view) => {
                void taskQuery.applySavedView(view).finally(() => routeUi.clearTaskSelection());
              }}
              onDeleteSavedTaskView={(view) => { void taskQuery.deleteSavedView(view); }}
              onToggleTaskListColumn={(column) => { void taskQuery.toggleColumn(column); }}
            />
          {:else if routeLoad.toolbarDataError || toolbarLoadState?.status === "failed"}
            <div class="flex min-h-9 items-center justify-center gap-2 border-t border-border px-3 text-xs text-muted-foreground" role="alert">
              <span>{t("common.viewLoadFailed", t("projects.header.projectSettings"))}</span>
              <button
                type="button"
                class="font-medium text-foreground underline-offset-2 hover:underline"
                onclick={() => {
                  if (routeLoad.toolbarDataError) {
                    const projectId = selectedProjectId;
                    void routeLoad.requestToolbarData(projectId, () => (
                      routeUi.toolbarPanel !== null && selectedProjectId === projectId
                    ));
                  } else routeLoad.requestOptional("toolbar", true);
                }}
              >
                {t("common.retry")}
              </button>
            </div>
          {:else}
            <span class="sr-only" aria-busy="true">{t("common.loading")}</span>
          {/if}
        {/if}
        {#if routeUi.selectedTaskIds.length > 0}
          {#if bulkActionsLoadState?.status === "ready" && bulkActionsLoadState.component.kind === "bulk-actions"}
            {@const ProjectBulkActionController = bulkActionsLoadState.component.component}
            <ProjectBulkActionController
              {selectedProject}
              {priorities}
              {selectedTasks}
              {selectableTasks}
              {selectedActiveTaskCount}
              {selectedArchivedTaskCount}
              terminalStatus={terminalStatus()}
              firstOpenStatus={firstOpenStatus()}
              bind:selectedTaskIds={routeUi.selectedTaskIds}
              bind:showArchivedTasks={taskQuery.showArchivedTasks}
            />
          {:else if bulkActionsLoadState?.status === "failed"}
            <div class="flex min-h-9 items-center justify-center gap-2 border-t border-border px-3 text-xs text-muted-foreground" role="alert">
              <span>{t("common.viewLoadFailed", t("collections.selection.selected", routeUi.selectedTaskIds.length))}</span>
              <button
                type="button"
                class="font-medium text-foreground underline-offset-2 hover:underline"
                onclick={() => routeLoad.requestOptional("bulk-actions", true)}
              >
                {t("common.retry")}
              </button>
            </div>
          {:else}
            <span class="sr-only" aria-busy="true">{t("common.loading")}</span>
          {/if}
        {/if}
      </header>

    {:else}
      <header
        data-projects-shell-header
        class="flex h-11 shrink-0 items-center border-b border-border px-4 text-sm font-semibold"
        style="background-color: var(--cal-header-bg);"
      >
        {t("titleBar.tab.projects")}
      </header>
    {/if}

    <div
      data-projects-content-frame
      class="relative min-h-0 flex-1"
      style="background-color: var(--cal-bg);"
    >
      {#if selectedProject && selectedGroup}
          {#if projects.activeView === "list"}
            {@const ProjectListView = viewComponents?.list ?? null}
            {#if ProjectListView && projects.isProjectViewDataLoaded(selectedProjectId, "list")}
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
            {:else if viewDataFailed}
              <div class="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground" role="alert">
                <span>{t("common.viewLoadFailed", t("projects.tabs.list"))}</span>
                <button
                  type="button"
                  class="font-medium text-foreground underline-offset-2 hover:underline"
                  onclick={() => { if (selectedProjectId) requestViewData(selectedProjectId, "list"); }}
                >
                  {t("common.retry")}
                </button>
              </div>
            {:else}
              <div class="flex h-full items-center justify-center text-sm text-muted-foreground" aria-busy="true">
                {t("common.loading")}
              </div>
            {/if}
          {:else if projects.activeView === "kanban" && viewComponents?.kanban}
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
          {:else if projects.activeView === "calendar" && viewComponents?.calendar}
            {@const CalendarView = viewComponents.calendar}
            {#if CalendarView}
            <div class="h-full min-h-112 overflow-hidden">
              <CalendarView
                eventFilter={(event) => taskQuery.eventMatches(event)}
                createDefaults={projectCalendarCreateDefaults}
                initialViewMode={projectCalendarViewMode}
                onViewModeChange={(mode) => {
                  projectCalendarViewMode = mode;
                }}
                {mobileLayout}
              />
            </div>
            {/if}
          {:else if projects.activeView === "gantt" && viewComponents?.gantt}
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
          {:else if projects.activeView === "dashboard" && viewComponents?.dashboard}
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
            <div class="flex h-full items-center justify-center text-sm text-muted-foreground" aria-busy="true">
              {t("common.loading")}
            </div>
          {/if}
      {:else}
        <ProjectEmptyState
          {selectedProjectId}
          bind:showInactiveProjects
          {mobileLayout}
          onProjectSelected={() => {
            routeUi.closeTaskDetail();
            routeUi.closeToolbarImmediately();
          }}
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
      <div class="absolute inset-x-3 top-3 z-80 flex min-h-10 items-center justify-center gap-2 surface-floating px-3 text-muted-foreground" role="alert">
        <span>{t("common.viewLoadFailed", t("projects.finder.label"))}</span>
        <button
          type="button"
          class="font-medium text-foreground underline-offset-2 hover:underline"
          onclick={() => routeLoad.requestOptional("task-finder", true)}
        >
          {t("common.retry")}
        </button>
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
      <div class="absolute inset-0 z-80 surface-backdrop flex items-center justify-center p-4" role="alert">
        <div class="flex min-h-32 w-full max-w-sm flex-col items-center justify-center gap-3 surface-dialog p-4 text-center text-sm text-muted-foreground">
          <p>{t("common.viewLoadFailed", t("projects.detail.title"))}</p>
          <div class="flex gap-2">
            <button
              type="button"
              class="min-h-9 rounded-md border border-border bg-background px-3 font-medium text-foreground hover:bg-accent"
              onclick={() => { void routeLoad.requestTaskDetailData(selectedProjectId, routeUi.selectedTaskId); }}
            >
              {t("common.retry")}
            </button>
            <button
              type="button"
              class="min-h-9 rounded-md px-3 font-medium text-foreground hover:bg-accent"
              onclick={() => {
                routeUi.closeTaskDetail();
                routeLoad.invalidateTaskDetailData();
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      </div>
    {:else}
      <div class="absolute inset-0 z-80 surface-backdrop flex items-center justify-center p-4" aria-busy="true">
        <div class="surface-dialog px-4 py-3 text-sm text-muted-foreground">
          {t("common.loading")}
        </div>
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
