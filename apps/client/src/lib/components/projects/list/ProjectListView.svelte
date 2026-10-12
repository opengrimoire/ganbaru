<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getCalendar } from "$lib/stores/calendar.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { getEventColor } from "$lib/calendar/utils";
  import { collectionRowContentStart } from "$lib/components/collections/row-expansion";
  import { projectListFrozenOffsets, projectListRowColor } from "$lib/projects/list/presentation";
  import { setProjectListTableContext } from "./table-context";
  import type { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";
  import ProjectListCalculationFooter from "./ProjectListCalculationFooter.svelte";
  import { projectPriorityDisplayLabel } from "$lib/projects/display";
  import {
    projectCustomFieldDisplayText,
    projectCustomFieldUsesOptions,
  } from "$lib/projects/custom-fields";
  import {
    projectTaskListGridMinWidth,
    projectTaskListGridTemplate,
    projectTaskListGutterWidth,
    projectTaskListLeadingGridTemplate,
    type ProjectTaskListColumnWidths,
    type ProjectTaskListGridInput,
    type ProjectTaskListResizableColumn,
  } from "$lib/projects/list/view";
  import {
    projectListDueDateEditPlan,
    projectListStartDateEditPlan,
  } from "$lib/projects/list/date-edit";
  import {
    projectListGroupTaskCreateTarget,
    projectListGroupTaskDraftKey,
    projectListSectionTaskCreateTarget,
  } from "$lib/projects/list/quick-add";
  import {
    type ProjectCustomField,
    type ProjectCustomFieldOption,
    type ProjectCustomFieldValue,
    type ProjectCustomFieldValueUpdate,
    type ProjectTag,
    type ProjectPriority,
    type ProjectPriorityConfig,
    type ProjectSection,
    type ProjectStatus,
    type ProjectTask,
    type ProjectTaskGroupMode,
    type ProjectTaskListColumn,
    type ProjectTaskSortDirection,
    type ProjectTaskSortMode,
  } from "$lib/projects/types";
  import type { ProjectTaskListGroup } from "$lib/projects/tasks/view";
  import ProjectListColumnHeaders from "./ProjectListColumnHeaders.svelte";
  import ProjectListGroupHeader from "./ProjectListGroupHeader.svelte";
  import ProjectListScrollbars from "./ProjectListScrollbars.svelte";
  import ProjectListSectionAddRow from "./ProjectListSectionAddRow.svelte";
  import ProjectListSectionBlock from "./ProjectListSectionBlock.svelte";
  import ProjectListTaskAddRow from "./ProjectListTaskAddRow.svelte";
  import ProjectListTaskRows from "./ProjectListTaskRows.svelte";
  import { ProjectListRowDragController } from "./row-drag-controller.svelte";
  import { ProjectListInteractionController } from "./interaction-controller.svelte";
  import { ProjectListQuickAddController } from "./quick-add-controller.svelte";
  import { ProjectListViewportController } from "./viewport-controller.svelte";
  import { createProjectListColumnReorder } from "./column-reorder";

  let {
    mobileLayout = false,
    taskQuery,
    selectedProjectId,
    sections,
    statuses,
    priorities,
    tasks,
    allProjectTasks,
    listTaskGroups,
    taskGroupBy,
    taskSortMode,
    taskSortDirection,
    taskListColumns,
    taskListColumnWidths,
    projectCustomFields,
    selectedTaskId,
    selectedTaskIds,
    showArchivedTasks,
    onOpenTask,
    onSelectedTaskIdsChange,
    onRevealTask,
    onTaskListColumnWidthsChange,
    onNeedMore,
  }: {
    mobileLayout?: boolean;
    taskQuery?: ProjectTaskQueryController;
    selectedProjectId: string | null;
    sections: ProjectSection[];
    statuses: ProjectStatus[];
    priorities: ProjectPriorityConfig[];
    tasks: ProjectTask[];
    allProjectTasks: ProjectTask[];
    listTaskGroups: ProjectTaskListGroup[];
    taskGroupBy: ProjectTaskGroupMode;
    taskSortMode: ProjectTaskSortMode;
    taskSortDirection: ProjectTaskSortDirection;
    taskListColumns: ProjectTaskListColumn[];
    taskListColumnWidths: ProjectTaskListColumnWidths;
    projectCustomFields: ProjectCustomField[];
    selectedTaskId: string | null;
    selectedTaskIds: string[];
    showArchivedTasks: boolean;
    onOpenTask: (task: ProjectTask) => void;
    onSelectedTaskIdsChange: (taskIds: string[]) => void;
    onRevealTask: (task: ProjectTask | undefined) => void;
    onTaskListColumnWidthsChange: (widths: ProjectTaskListColumnWidths, options?: { persist?: boolean }) => Promise<void>;
    onNeedMore: () => void;
  } = $props();

  const projects = getProjects();
  const calendar = getCalendar();
  const theme = getTheme();
  const { t } = getLocalization();
  const tableQuery = untrack(() => taskQuery);

  let sectionNameDrafts = $state<Record<string, string>>({});
  let listViewportWidth = $state(0);
  let listViewportWidthRem = $state<number | undefined>(undefined);
  const interaction = new ProjectListInteractionController({
    getSelectedTaskIds: () => selectedTaskIds,
    selectedTaskIdsChanged: (taskIds) => onSelectedTaskIdsChange(taskIds),
  });
  const quickAdd = new ProjectListQuickAddController({
    projects,
    getSelectedProjectId: () => selectedProjectId,
    getGroupBy: () => taskGroupBy,
    getStatuses: () => statuses,
    getPriorities: () => priorities,
    revealTask: (task) => onRevealTask(task),
    selectProjectFirstMessage: () => t("projects.tasks.selectProjectFirst"),
    createFailedMessage: (error) => t(
      "projects.tasks.createFailed",
      error instanceof Error ? error.message : String(error),
    ),
  });
  const drag = new ProjectListRowDragController({
    getTasks: () => tasks,
    getAllTasks: () => allProjectTasks,
    getSections: () => sections,
    getGroupBy: () => taskGroupBy,
    getSortMode: () => taskSortMode,
    getSortDirection: () => taskSortDirection,
    updateTask: (task, patch) => projects.updateTask(task, patch),
    updateSection: (section, patch) => projects.updateSection(section, patch),
  });
  const viewport = new ProjectListViewportController({
    getColumnWidths: () => taskListColumnWidths,
    getGridInput: taskListGridInputFor,
    persistColumnWidths: (widths) => onTaskListColumnWidthsChange(widths, { persist: true }),
  });
  const columnReorder = tableQuery ? createProjectListColumnReorder(tableQuery, {
    resizing: () => viewport.resizeGesture !== null,
    scroller: () => viewport.container ? { element: viewport.container, scrollTo: viewport.setHorizontalScroll } : null,
  }) : undefined;
  onDestroy(() => columnReorder?.destroy());

  function taskListGridInputFor(columnWidths: ProjectTaskListColumnWidths): ProjectTaskListGridInput {
    return {
      columns: taskListColumns,
      columnWidths,
      tasks,
      statuses,
      customFields: projectCustomFields,
      nameLabel: t("projects.list.name"),
      sectionLabels: sections.map((section) => section.name),
      groupLabels: listTaskGroups.map((group) => taskListGroupTitle(group.value)),
      columnLabel: taskListColumnLabel,
      priorityLabel: (priority: ProjectPriority) => projectPriorityDisplayLabel(priority, priorities, t),
      estimateLabel,
      customFieldDisplayValue,
      scheduledLabel,
    };
  }
  const taskListGridInput = $derived(taskListGridInputFor(viewport.effectiveColumnWidths));
  const taskListGridTemplate = $derived(projectTaskListGridTemplate(taskListGridInput));
  const taskListGridMinWidth = $derived(projectTaskListGridMinWidth(taskListGridInput));
  const taskListLeadingGridTemplate = $derived(projectTaskListLeadingGridTemplate(taskListGridInput));
  const taskListContentStart = collectionRowContentStart(projectTaskListGutterWidth());
  const listRangeDateColumnsVisible = $derived(taskListColumns.includes("start") && taskListColumns.includes("due"));
  const frozenOffsets: Partial<Record<ProjectTaskListResizableColumn | "selection" | "expand", number>> = $derived(taskQuery ? projectListFrozenOffsets(taskQuery.listPresentation, taskListGridInput, listViewportWidthRem) : {});
  $effect(() => {
    sections.length;
    listTaskGroups.length;
    const container = viewport.container;
    if (!container || listViewportWidth <= 0) return;
    const content = container.firstElementChild;
    const row = container.querySelector<HTMLElement>(".collection-row");
    const rootFontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
    if (!(content instanceof HTMLElement) || !row || !Number.isFinite(rootFontSize) || rootFontSize <= 0) return;
    const contentStyle = getComputedStyle(content);
    const rowStyle = getComputedStyle(row);
    const padding = [contentStyle.paddingLeft, contentStyle.paddingRight, rowStyle.paddingLeft, rowStyle.paddingRight]
      .reduce((sum, value) => sum + (Number.parseFloat(value) || 0), 0);
    listViewportWidthRem = Math.max(0, listViewportWidth - padding) / rootFontSize;
  });
  if (tableQuery) setProjectListTableContext({
    query: tableQuery,
    gutter: projectTaskListGutterWidth(),
    cellClass: (column) => [
      frozenOffsets[column] !== undefined ? "project-list-frozen-cell" : "",
      column === "selection" || column === "expand" ? "project-list-gutter-cell" : "",
      column !== "selection" && column !== "expand" && tableQuery.listPresentation.wrappedColumns.includes(column) ? "project-list-wrap-cell" : "",
    ].join(" "),
    cellStyle: (column) => frozenOffsets[column] === undefined ? "" : `left: calc(var(--project-list-content-inset) + ${frozenOffsets[column]}rem);`,
    rowStyle: (task, selected) => {
      const color = projectListRowColor(task, tableQuery.listPresentation.colorRules);
      const background = selected ? "color-mix(in srgb, var(--accent) 40%, var(--cal-bg))"
        : color === undefined ? "var(--cal-bg)" : `color-mix(in srgb, ${getEventColor(color, theme.current).bg} 15%, var(--cal-bg))`;
      return `--project-list-row-bg: ${background}; background: linear-gradient(to right, var(--cal-bg) ${taskListContentStart}, ${background} ${taskListContentStart});`;
    },
    columnReorder,
  });

  function handleHorizontalKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && quickAdd.hasActiveDraft()) {
      event.preventDefault();
      event.stopPropagation();
      quickAdd.cancelActiveDrafts();
      blurTarget(event.target);
      return;
    }
    viewport.handleHorizontalKeydown(event);
  }

  function eventTargetElement(target: EventTarget | null): Element | null {
    if (target instanceof Element) return target;
    if (target instanceof Node) return target.parentElement;
    return null;
  }

  function blurTarget(target: EventTarget | null): void {
    const targetElement = eventTargetElement(target);
    if (targetElement instanceof HTMLElement) {
      targetElement.blur();
      return;
    }
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur();
    }
  }

  function handleProjectWindowPointerDown(event: PointerEvent): void {
    const target = event.target;
    if (!(target instanceof Node)) return;
    const targetElement = eventTargetElement(target);
    if (targetElement) {
      quickAdd.cancelForOutsideTarget(targetElement);
      interaction.handleOutsidePointerTarget(targetElement);
    }
  }

  function taskListGroupTitle(value: string): string {
    if (taskGroupBy === "status") {
      return statuses.find((status) => status.id === value)?.name ?? t("projects.grouping.missingStatus");
    }
    if (taskGroupBy === "priority") {
      return projectPriorityDisplayLabel(value, priorities, t);
    }
    if (taskGroupBy === "due") {
      if (value === "overdue") return t("projects.filters.overdue");
      if (value === "today") return t("projects.filters.today");
      if (value === "week") return t("projects.filters.thisWeek");
      if (value === "later") return t("projects.grouping.laterDue");
      if (value === "earlier") return t("projects.grouping.earlierDue");
      return t("projects.filters.noDueDate");
    }
    if (value === "scheduled") return t("projects.filters.scheduled");
    if (value === "unscheduled") return t("projects.filters.unscheduled");
    return value;
  }

  function taskListColumnLabel(column: ProjectTaskListColumn): string {
    const customFieldId = column.startsWith("custom:") ? column.slice("custom:".length) : undefined;
    if (customFieldId) {
      return projectCustomFields.find((field) => field.id === customFieldId)?.name
        ?? t("projects.columns.customField");
    }
    if (column === "priority") return t("projects.columns.priority");
    if (column === "estimate") return t("projects.columns.estimate");
    if (column === "start") return t("projects.columns.start");
    if (column === "due") return t("projects.columns.due");
    if (column === "scheduled") return t("projects.columns.scheduled");
    if (column === "dependencies") return t("projects.columns.dependencies");
    if (column === "assignee") return t("projects.columns.assignee");
    if (column === "reviewer") return t("projects.columns.reviewer");
    return t("projects.columns.status");
  }

  function subtasksForTask(parent: ProjectTask): ProjectTask[] {
    return showArchivedTasks
      ? projects.subtasksForTaskIncludingArchived(parent.id)
      : projects.subtasksForTask(parent.id);
  }

  function statusForTask(task: ProjectTask): ProjectStatus | undefined {
    return projects.statusById(task.statusId);
  }

  function blockedByDependencies(task: ProjectTask) {
    return projects.dependenciesBlockingTask(task.id);
  }

  function blocksDependencies(task: ProjectTask) {
    return projects.dependenciesBlockedByTask(task.id);
  }

  function tagsForTask(task: ProjectTask): ProjectTag[] {
    return projects.tagsForTask(task.id);
  }

  function visibleTaskTags(task: ProjectTask): ProjectTag[] {
    return tagsForTask(task).slice(0, 3);
  }

  function hiddenTaskTagCount(task: ProjectTask): number {
    return Math.max(0, tagsForTask(task).length - visibleTaskTags(task).length);
  }

  function customFieldOptions(field: ProjectCustomField): ProjectCustomFieldOption[] {
    return projects.customFieldOptionsForField(field.id);
  }

  function customFieldDisplayValue(task: ProjectTask, field: ProjectCustomField): string | undefined {
    const value = projects.customFieldValueForTask(task.id, field.id);
    const displayText = projectCustomFieldDisplayText(value, field);
    if (displayText !== undefined && field.fieldType !== "checkbox") return displayText;
    if (field.fieldType === "checkbox") {
      if (value?.checkboxValue === undefined) return undefined;
      return value.checkboxValue
        ? t("projects.customFields.checked")
        : t("projects.customFields.unchecked");
    }
    if (!projectCustomFieldUsesOptions(field.fieldType)) return undefined;
    const optionNames = projects.customFieldOptionValuesForTask(task.id, field.id)
      .map((option) => option.name);
    return optionNames.length > 0 ? optionNames.join(", ") : undefined;
  }

  function customFieldValue(task: ProjectTask, field: ProjectCustomField): ProjectCustomFieldValue | undefined {
    return projects.customFieldValueForTask(task.id, field.id);
  }

  function customFieldOptionValues(task: ProjectTask, field: ProjectCustomField): ProjectCustomFieldOption[] {
    return projects.customFieldOptionValuesForTask(task.id, field.id);
  }

  async function saveTaskCustomFieldValueFromList(
    task: ProjectTask,
    field: ProjectCustomField,
    value: Omit<ProjectCustomFieldValueUpdate, "taskId" | "fieldId">,
  ): Promise<void> {
    if (task.archivedAt) return;
    await projects.saveCustomFieldValue({
      taskId: task.id,
      fieldId: field.id,
      ...value,
    });
  }

  function sectionForTask(task: ProjectTask): ProjectSection | undefined {
    return sections.find((section) => section.id === task.sectionId);
  }

  async function setTaskStatusFromList(task: ProjectTask, status: ProjectStatus): Promise<void> {
    if (task.archivedAt || task.statusId === status.id) {
      interaction.statusMenuTaskId = null;
      return;
    }
    await projects.setTasksStatus([task], status.id);
    interaction.statusMenuTaskId = null;
  }

  async function setTaskPriorityFromList(task: ProjectTask, priority: ProjectPriority): Promise<void> {
    if (task.archivedAt || task.priority === priority) {
      interaction.priorityMenuTaskId = null;
      return;
    }
    await projects.setTaskPriority(task, priority);
    interaction.priorityMenuTaskId = null;
  }

  async function setTaskStartDateFromList(task: ProjectTask, startDate: string | undefined): Promise<void> {
    const plan = projectListStartDateEditPlan({
      task,
      selectedDate: startDate,
      rangeDateColumnsVisible: listRangeDateColumnsVisible,
    });
    if (plan.patch) {
      await projects.updateTask(task, plan.patch);
    }
    interaction.startDateMenuTaskId = null;
    interaction.dueDateMenuTaskId = plan.nextOpenMenu === "due" ? task.id : null;
  }

  async function setTaskDueDateFromList(task: ProjectTask, dueDate: string | undefined): Promise<void> {
    const plan = projectListDueDateEditPlan({
      task,
      selectedDate: dueDate,
      rangeDateColumnsVisible: listRangeDateColumnsVisible,
    });
    if (plan.patch) {
      await projects.updateTask(task, plan.patch);
    }
    interaction.dueDateMenuTaskId = null;
    interaction.startDateMenuTaskId = plan.nextOpenMenu === "start" ? task.id : null;
  }

  async function setTaskStartTimeFromList(task: ProjectTask, startTime: string | undefined): Promise<void> {
    if (task.archivedAt) {
      interaction.startDateMenuTaskId = null;
      return;
    }
    if (!task.startDate || task.startTime === startTime) return;
    await projects.updateTask(task, { startTime });
  }

  async function setTaskDueTimeFromList(task: ProjectTask, dueTime: string | undefined): Promise<void> {
    if (task.archivedAt) {
      interaction.dueDateMenuTaskId = null;
      return;
    }
    if (!task.dueDate || task.dueTime === dueTime) return;
    await projects.updateTask(task, { dueTime });
  }

  async function renameTaskFromList(task: ProjectTask, title: string): Promise<void> {
    const nextTitle = title.trim();
    if (task.archivedAt || !nextTitle || nextTitle === task.title) return;
    await projects.updateTask(task, { title: nextTitle });
  }

  function sectionNameDraft(section: ProjectSection): string {
    return sectionNameDrafts[section.id] ?? section.name;
  }

  function sectionDraftDirty(section: ProjectSection): boolean {
    return sectionNameDraft(section) !== section.name;
  }

  function sectionDraftSaveable(section: ProjectSection): boolean {
    return sectionDraftDirty(section)
      && sectionNameDraft(section).trim().length > 0
      && !section.hiddenAt
      && !section.archivedAt;
  }

  async function saveSection(section: ProjectSection): Promise<void> {
    const name = sectionNameDraft(section).trim();
    if (!name) return;
    await projects.updateSection(section, { name });
    sectionNameDrafts = {
      ...sectionNameDrafts,
      [section.id]: name,
    };
  }

  async function hideSection(section: ProjectSection): Promise<void> {
    await projects.hideSection(section);
  }

  async function archiveSection(section: ProjectSection): Promise<void> {
    await projects.archiveSection(section);
  }

  async function restoreSection(section: ProjectSection): Promise<void> {
    await projects.restoreSection(section);
  }

  async function toggleSectionCollapsed(section: ProjectSection): Promise<void> {
    await projects.updateSection(section, { collapsed: !section.collapsed });
  }

  function openTaskDetail(task: ProjectTask): void {
    drag.openTask(task, onOpenTask);
  }

  function estimateLabel(minutes: number): string {
    return t("projects.list.estimateMinutes", minutes);
  }

  function scheduledLinksForTask(taskId: string) {
    return projects.eventLinksForTask(taskId).filter((link) => link.linkKind === "scheduled");
  }

  function scheduledLabel(taskId: string): string | null {
    const count = scheduledLinksForTask(taskId).length;
    const starts = projects.eventLinksForTask(taskId)
      .filter((link) => link.linkKind === "scheduled")
      .map((link) => calendar.sourceEvents.find((event) => event.id === link.eventId)?.start)
      .filter((start): start is string => Boolean(start))
      .sort((a, b) => a.localeCompare(b));
    const now = new Date();
    const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")} ${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
    const nextStart = starts.find((start) => start >= date) ?? starts[0];
    if (nextStart) return t("projects.schedule.nextScheduled", nextStart.slice(0, 16), count);
    return count > 0 ? t("projects.schedule.scheduledCount", count) : null;
  }

  function handleScroll(event: Event): void {
    const target = event.currentTarget as HTMLElement;
    if (target.scrollHeight - target.scrollTop - target.clientHeight < 600) onNeedMore();
  }
</script>

<svelte:window
  onkeydown={handleHorizontalKeydown}
  onpointerdown={handleProjectWindowPointerDown}
  onpointermove={viewport.handleResizePointerMove}
  onpointerup={(event) => viewport.finishResize(event, true)}
  onpointercancel={(event) => viewport.finishResize(event, false)}
/>

<div
  bind:this={viewport.container}
  bind:clientWidth={listViewportWidth}
  class="project-list-scroll h-full min-h-0 overflow-auto overscroll-contain"
  style={`--project-list-divider-left: ${taskListContentStart};`}
  data-mobile-layout={mobileLayout}
  onscroll={handleScroll}
>
  <div class="flex min-h-full flex-col gap-5 p-3">
    {#if viewport.resizeError}
      <p role="alert" class="text-sm text-destructive">{viewport.resizeError}</p>
    {/if}
    {#if taskQuery?.loadError}
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <p role="alert" class="text-destructive">{t("projects.tasks.loadFailed", taskQuery.loadError)}</p>
        <button class="min-h-8 rounded px-2 hover:bg-accent" disabled={taskQuery.loading} onclick={() => taskQuery?.retryCurrent(selectedTaskIds, selectedTaskId)}>{t("common.retry")}</button>
      </div>
    {/if}
    {#if taskQuery?.presentationError}<p role="alert" class="text-xs text-destructive">{taskQuery.presentationError}</p>{/if}
    {#if taskQuery?.propertyError}<p role="alert" class="text-xs text-destructive">{taskQuery.propertyError}</p>{/if}
    {#if taskQuery?.listColumnsError}<p role="alert" class="text-xs text-destructive">{taskQuery.listColumnsError}</p>{/if}
    {#if taskGroupBy === "section"}
      {#each sections as section (section.id)}
        {@const sectionTasks = drag.tasksForSection(section)}
        <ProjectListSectionBlock
            {section}
            {sectionTasks}
            gridTemplate={taskListGridTemplate}
            gridMinWidth={taskListGridMinWidth}
            leadingGridTemplate={taskListLeadingGridTemplate}
            {taskListColumns}
            {taskListColumnLabel}
            {statuses}
            {priorities}
            {projectCustomFields}
            {selectedTaskId}
            statusMenuTaskId={interaction.statusMenuTaskId}
            priorityMenuTaskId={interaction.priorityMenuTaskId}
            startDateMenuTaskId={interaction.startDateMenuTaskId}
            dueDateMenuTaskId={interaction.dueDateMenuTaskId}
            draggingTaskId={drag.draggingTaskId}
            dropPendingTaskId={drag.dropPendingTaskId}
            sectionDragOver={drag.dragOverSectionId === section.id}
            sectionDragging={drag.draggingSectionId === section.id}
            sectionDropPending={drag.sectionDropPendingId === section.id}
            sectionDropMarkerBefore={drag.sectionMarkerVisible(section, "before")}
            sectionDropMarkerAfter={drag.sectionMarkerVisible(section, "after")}
            taskSectionDropMarkerVisible={drag.dragOverSectionId === section.id && drag.dragOverPosition === "section"}
            theme={theme.current}
            allTasksSelected={interaction.allTasksSelected(sectionTasks)}
            partiallySelected={interaction.someTasksSelected(sectionTasks) && !interaction.allTasksSelected(sectionTasks)}
            canDragSection={drag.canStartSection(section)}
            sectionDraft={sectionNameDraft(section)}
            sectionDraftDirty={sectionDraftDirty(section)}
            sectionDraftSaveable={sectionDraftSaveable(section)}
            sectionOptionsMenuOpen={interaction.sectionOptionsMenuId === section.id}
            taskDraft={quickAdd.sectionTaskDrafts[section.id] ?? ""}
            taskAddLabel={t("projects.list.addTaskInSection", section.name)}
            taskAddActive={quickAdd.activeSectionTaskDraftInputId === section.id}
            taskAddPending={quickAdd.pendingTarget !== null}
            taskAddError={quickAdd.errorFor(projectListSectionTaskCreateTarget(section.id))}
            {statusForTask}
            {subtasksForTask}
            {scheduledLabel}
            {visibleTaskTags}
            {hiddenTaskTagCount}
            {blockedByDependencies}
            {blocksDependencies}
            taskSelected={(task) => interaction.taskSelected(task)}
            {estimateLabel}
            {customFieldDisplayValue}
            {customFieldOptions}
            {customFieldValue}
            {customFieldOptionValues}
            canStartTaskDrag={drag.canStartTask}
            taskDropMarkerVisible={(task, position) => drag.taskMarkerVisible(section, task, position)}
            onSectionDragOver={(event) => drag.handleSectionGroupDragOver(event, section)}
            onSectionDrop={(event) => { void drag.dropSectionOrTask(event, section); }}
            onToggleSectionSelection={() => interaction.toggleTaskGroupSelection(sectionTasks)}
            onToggleSectionCollapsed={() => toggleSectionCollapsed(section)}
            onSectionDraftChange={(value) => {
              sectionNameDrafts = {
                ...sectionNameDrafts,
                [section.id]: value,
              };
            }}
            onSaveSection={() => saveSection(section)}
            onToggleSectionOptionsMenu={() => {
              interaction.sectionOptionsMenuId = interaction.sectionOptionsMenuId === section.id ? null : section.id;
            }}
            onRestoreSection={() => {
              interaction.sectionOptionsMenuId = null;
              return restoreSection(section);
            }}
            onHideSection={() => {
              interaction.sectionOptionsMenuId = null;
              return hideSection(section);
            }}
            onArchiveSection={() => {
              interaction.sectionOptionsMenuId = null;
              return archiveSection(section);
            }}
            onSectionPointerDown={(event) => drag.handleSectionPointerDown(event, section)}
            onSectionPointerUp={drag.clearSectionGesture}
            onSectionPointerCancel={drag.clearSectionGesture}
            onSectionDragStart={(event) => drag.handleSectionDragStart(event, section)}
            onSectionDragEnd={drag.handleSectionDragEnd}
            onResizePointerDown={viewport.startResize}
            onResizeDoubleClick={viewport.handleResizeDoubleClick}
            onResizeKeydown={viewport.handleResizeKeydown}
            onSaveCustomFieldValue={saveTaskCustomFieldValueFromList}
            onToggleTaskSelection={interaction.toggleTaskSelection}
            onOpenTask={openTaskDetail}
            onTaskPointerDown={(event, task) => drag.handleTaskPointerDown(event, task)}
            onTaskPointerUp={drag.clearTaskGesture}
            onTaskPointerCancel={drag.clearTaskGesture}
            onTaskDragStart={(event, task) => drag.handleTaskDragStart(event, task)}
            onTaskDragEnd={drag.handleTaskDragEnd}
            onTaskDragOver={(event, task) => drag.handleTaskDragOver(event, section, task)}
            onTaskDrop={(event, task) => { void drag.dropTask(event, section, task); }}
            onToggleStatusMenu={(task) => interaction.toggleTaskMenu("status", task.id)}
            onSetStatus={(task, nextStatus) => { void setTaskStatusFromList(task, nextStatus); }}
            onTogglePriorityMenu={(task) => interaction.toggleTaskMenu("priority", task.id)}
            onSetPriority={(task, priority) => { void setTaskPriorityFromList(task, priority); }}
            onToggleStartDateMenu={(task) => interaction.toggleTaskMenu("start", task.id)}
            onCloseStartDateMenu={() => interaction.closeTaskMenu("start")}
            onSetStartDate={(task, startDate) => { void setTaskStartDateFromList(task, startDate); }}
            onClearStartDate={(task) => { void setTaskStartDateFromList(task, undefined); }}
            onSetStartTime={(task, startTime) => { void setTaskStartTimeFromList(task, startTime); }}
            onClearStartTime={(task) => { void setTaskStartTimeFromList(task, undefined); }}
            onToggleDueDateMenu={(task) => interaction.toggleTaskMenu("due", task.id)}
            onCloseDueDateMenu={() => interaction.closeTaskMenu("due")}
            onSetDueDate={(task, dueDate) => { void setTaskDueDateFromList(task, dueDate); }}
            onClearDueDate={(task) => { void setTaskDueDateFromList(task, undefined); }}
            onSetDueTime={(task, dueTime) => { void setTaskDueTimeFromList(task, dueTime); }}
            onClearDueTime={(task) => { void setTaskDueTimeFromList(task, undefined); }}
            onRenameTask={(task, title) => { void renameTaskFromList(task, title); }}
            taskExpanded={(task, hasSubtasks) => interaction.taskExpanded(task, hasSubtasks)}
            onToggleTaskExpanded={(task, hasSubtasks) => interaction.toggleTaskExpanded(task, hasSubtasks)}
            subtaskQuickAdd={quickAdd}
            onTaskDraftChange={(value) => {
                quickAdd.sectionTaskDrafts = {
                  ...quickAdd.sectionTaskDrafts,
                  [section.id]: value,
                };
            }}
            onTaskAddActiveChange={(active) => {
                if (active) {
                  quickAdd.activeSectionTaskDraftInputId = section.id;
                } else if (quickAdd.activeSectionTaskDraftInputId === section.id) {
                  quickAdd.activeSectionTaskDraftInputId = null;
                }
            }}
            onSubmitTask={() => quickAdd.submitSectionTask(section.id)}
        />
      {/each}
      <ProjectListSectionAddRow
        gridTemplate={taskListGridTemplate}
        gridMinWidth={taskListGridMinWidth}
        leadingGridTemplate={taskListLeadingGridTemplate}
        label={t("projects.header.addSection")}
        draft={quickAdd.sectionDraft}
        active={quickAdd.sectionDraftInputActive}
        onDraftChange={(value) => {
          quickAdd.sectionDraft = value;
        }}
        onActiveChange={(active) => {
          quickAdd.sectionDraftInputActive = active;
        }}
        onSubmit={() => quickAdd.submitSection()}
      />
    {:else}
      {#each listTaskGroups as group (group.id)}
        {@const groupKey = projectListGroupTaskDraftKey(taskGroupBy, group)}
        {@const groupTitle = taskListGroupTitle(group.value)}
        {@const groupQuickAddPlan = quickAdd.groupPlan(group)}
        {@const groupCreateTarget = projectListGroupTaskCreateTarget(groupKey)}
        <section
          class="flex flex-col gap-0"
          style={`min-width: max(100%, ${taskListGridMinWidth});`}
        >
          <ProjectListGroupHeader
            title={groupTitle}
            gridTemplate={taskListGridTemplate}
            gridMinWidth={taskListGridMinWidth}
            leadingGridTemplate={taskListLeadingGridTemplate}
            taskCount={group.tasks.length}
            allSelected={interaction.allTasksSelected(group.tasks)}
            partiallySelected={interaction.someTasksSelected(group.tasks) && !interaction.allTasksSelected(group.tasks)}
            onToggleSelection={() => interaction.toggleTaskGroupSelection(group.tasks)}
          />
          <ProjectListColumnHeaders
            mode="group"
            rowKey={`group:${groupKey}`}
            gridTemplate={taskListGridTemplate}
            gridMinWidth={taskListGridMinWidth}
            {taskListColumns}
            {taskListColumnLabel}
            onResizePointerDown={viewport.startResize}
            onResizeDoubleClick={viewport.handleResizeDoubleClick}
            onResizeKeydown={viewport.handleResizeKeydown}
          />
          <ProjectListTaskRows
            tasks={group.tasks}
            {statuses}
            {priorities}
            {taskListColumns}
            {projectCustomFields}
            {selectedTaskId}
            statusMenuTaskId={interaction.statusMenuTaskId}
            priorityMenuTaskId={interaction.priorityMenuTaskId}
            startDateMenuTaskId={interaction.startDateMenuTaskId}
            dueDateMenuTaskId={interaction.dueDateMenuTaskId}
            gridTemplate={taskListGridTemplate}
            gridMinWidth={taskListGridMinWidth}
            theme={theme.current}
            {statusForTask}
            {subtasksForTask}
            {scheduledLabel}
            {visibleTaskTags}
            {hiddenTaskTagCount}
            {blockedByDependencies}
            {blocksDependencies}
            taskSelected={(task) => interaction.taskSelected(task)}
            sectionNameForTask={(task) => sectionForTask(task)?.name}
            {estimateLabel}
            {customFieldDisplayValue}
            {customFieldOptions}
            {customFieldValue}
            {customFieldOptionValues}
            onSaveCustomFieldValue={saveTaskCustomFieldValueFromList}
            onToggleTaskSelection={interaction.toggleTaskSelection}
            onOpenTask={openTaskDetail}
            onToggleStatusMenu={(task) => interaction.toggleTaskMenu("status", task.id)}
            onSetStatus={(task, nextStatus) => { void setTaskStatusFromList(task, nextStatus); }}
            onTogglePriorityMenu={(task) => interaction.toggleTaskMenu("priority", task.id)}
            onSetPriority={(task, priority) => { void setTaskPriorityFromList(task, priority); }}
            onToggleStartDateMenu={(task) => interaction.toggleTaskMenu("start", task.id)}
            onCloseStartDateMenu={() => interaction.closeTaskMenu("start")}
            onSetStartDate={(task, startDate) => { void setTaskStartDateFromList(task, startDate); }}
            onClearStartDate={(task) => { void setTaskStartDateFromList(task, undefined); }}
            onSetStartTime={(task, startTime) => { void setTaskStartTimeFromList(task, startTime); }}
            onClearStartTime={(task) => { void setTaskStartTimeFromList(task, undefined); }}
            onToggleDueDateMenu={(task) => interaction.toggleTaskMenu("due", task.id)}
            onCloseDueDateMenu={() => interaction.closeTaskMenu("due")}
            onSetDueDate={(task, dueDate) => { void setTaskDueDateFromList(task, dueDate); }}
            onClearDueDate={(task) => { void setTaskDueDateFromList(task, undefined); }}
            onSetDueTime={(task, dueTime) => { void setTaskDueTimeFromList(task, dueTime); }}
            onClearDueTime={(task) => { void setTaskDueTimeFromList(task, undefined); }}
            onRenameTask={(task, title) => { void renameTaskFromList(task, title); }}
            taskExpanded={(task, hasSubtasks) => interaction.taskExpanded(task, hasSubtasks)}
            onToggleTaskExpanded={(task, hasSubtasks) => interaction.toggleTaskExpanded(task, hasSubtasks)}
            subtaskQuickAdd={quickAdd}
          />
            {#if groupQuickAddPlan.enabled}
              <ProjectListTaskAddRow
                mode="group"
                rowId={groupKey}
                gridTemplate={taskListGridTemplate}
                gridMinWidth={taskListGridMinWidth}
                label={t("projects.list.addTaskInSection", groupTitle)}
                draft={quickAdd.groupTaskDrafts[groupKey] ?? ""}
                active={quickAdd.activeGroupTaskDraftInputId === groupKey}
                pending={quickAdd.pendingTarget !== null}
                error={quickAdd.errorFor(groupCreateTarget)}
                onDraftChange={(value) => {
                  quickAdd.groupTaskDrafts = {
                    ...quickAdd.groupTaskDrafts,
                    [groupKey]: value,
                  };
                }}
                onActiveChange={(active) => {
                  if (active) {
                    quickAdd.activeGroupTaskDraftInputId = groupKey;
                  } else if (quickAdd.activeGroupTaskDraftInputId === groupKey) {
                    quickAdd.activeGroupTaskDraftInputId = null;
                  }
                }}
                onSubmit={() => quickAdd.submitGroupTask(group)}
              />
            {/if}
        </section>
      {/each}
    {/if}
    {#if tasks.length === 0 && allProjectTasks.length > 0}
      <div class="rounded-md border border-dashed border-border px-2 py-3 text-[0.8rem] text-muted-foreground">
        {t("projects.filters.noMatchingTasks")}
      </div>
    {/if}
    {#if taskQuery}
      <ProjectListCalculationFooter query={taskQuery} gridTemplate={taskListGridTemplate} gridMinWidth={taskListGridMinWidth} columns={taskListColumns} columnLabel={taskListColumnLabel} />
    {/if}
  </div>
</div>
<ProjectListScrollbars
  scrollContainer={viewport.container}
  getMaxScrollLeft={viewport.maxHorizontalScrollLeft}
  onScrollPositionChange={viewport.setHorizontalScroll}
/>

<style>
  :global(.project-list-scroll .project-list-divider) {
    position: relative;
    --project-list-divider-right: 0.25rem;
  }

  :global(.project-list-scroll .project-list-divider)::after {
    position: absolute;
    right: var(--project-list-divider-right);
    bottom: 0;
    left: var(--project-list-divider-left);
    height: 0;
    border-bottom: 1px solid var(--cal-gridline);
    content: "";
    pointer-events: none;
  }

  :global(.project-list-scroll .project-list-sticky-row) {
    position: relative;
    z-index: 1;
    background-color: var(--cal-bg);
  }

  /* Native sticky positioning moves with the scroll in the same frame; a transform written from script lags behind compositor scrolling. */
  :global(.project-list-scroll .project-list-leading-row) {
    position: sticky;
    left: var(--project-list-content-inset);
  }

  :global(.project-list-scroll .project-list-add-row-caret) {
    animation: project-list-caret-blink 1s step-end infinite;
  }

  @keyframes project-list-caret-blink {
    0%,
    49% {
      opacity: 1;
    }

    50%,
    100% {
      opacity: 0;
    }
  }

  .project-list-scroll {
    --project-list-content-inset: 1rem;
    --collection-surface: var(--cal-bg);
    overflow-x: hidden;
    overflow-y: auto;
    padding-right: 0.5rem;
    padding-bottom: 0.5rem;
    scrollbar-width: none;
  }

  :global(.project-list-scroll .project-list-frozen-cell) {
    position: sticky;
    z-index: 3;
    border-radius: 0;
    background-color: var(--project-list-row-bg, var(--cal-bg));
  }

  /* The gutter sits outside the table, so a frozen checkbox or chevron keeps the list surface instead of the row color. */
  :global(.project-list-scroll .project-list-frozen-cell.project-list-gutter-cell) {
    background-color: var(--cal-bg);
  }

  :global(.project-list-scroll .project-list-frozen-cell:focus-within),
  :global(.project-list-scroll .project-list-frozen-cell:has([aria-expanded="true"])) {
    z-index: 10;
  }

  :global(.project-list-scroll .project-list-wrap-cell .truncate) {
    overflow: visible;
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .project-list-scroll[data-mobile-layout="true"] {
    overflow-x: auto;
    touch-action: pan-x pan-y pinch-zoom;
  }

  .project-list-scroll[data-mobile-layout="true"] :global(.project-list-leading-row) {
    z-index: 2;
    width: min(22rem, calc(100vw - 2rem));
    background-color: var(--cal-bg);
  }

  .project-list-scroll::-webkit-scrollbar {
    display: none;
  }
</style>
