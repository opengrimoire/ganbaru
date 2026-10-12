<script lang="ts">
  import type { ProjectListDropPosition } from "$lib/projects/list/drag";
  import type { ProjectTaskListResizableColumn } from "$lib/projects/list/view";
  import type {
    ProjectCustomField,
    ProjectCustomFieldOption,
    ProjectCustomFieldValue,
    ProjectCustomFieldValueUpdate,
    ProjectPriority,
    ProjectPriorityConfig,
    ProjectSection,
    ProjectStatus,
    ProjectTag,
    ProjectTask,
    ProjectTaskDependency,
    ProjectTaskListColumn,
  } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import { cn, type MaybePromise } from "$lib/utils";
  import ProjectListColumnHeaders from "./ProjectListColumnHeaders.svelte";
  import ProjectListSectionHeader from "./ProjectListSectionHeader.svelte";
  import ProjectListTaskAddRow from "./ProjectListTaskAddRow.svelte";
  import ProjectListTaskRows from "./ProjectListTaskRows.svelte";
  import type { ProjectListSubtaskQuickAdd } from "./quick-add-controller.svelte";


  let {
    section,
    sectionTasks,
    gridTemplate,
    gridMinWidth,
    leadingGridTemplate,
    taskListColumns,
    taskListColumnLabel,
    statuses,
    priorities,
    projectCustomFields,
    selectedTaskId,
    statusMenuTaskId,
    priorityMenuTaskId,
    startDateMenuTaskId,
    dueDateMenuTaskId,
    draggingTaskId,
    dropPendingTaskId,
    sectionDragOver,
    sectionDragging,
    sectionDropPending,
    sectionDropMarkerBefore,
    sectionDropMarkerAfter,
    taskSectionDropMarkerVisible,
    theme,
    allTasksSelected,
    partiallySelected,
    canDragSection,
    sectionDraft,
    sectionDraftDirty,
    sectionDraftSaveable,
    sectionOptionsMenuOpen,
    taskDraft,
    taskAddLabel,
    taskAddActive,
    taskAddPending,
    taskAddError,
    statusForTask,
    subtasksForTask,
    scheduledLabel,
    visibleTaskTags,
    hiddenTaskTagCount,
    blockedByDependencies,
    blocksDependencies,
    taskSelected,
    estimateLabel,
    customFieldDisplayValue,
    customFieldOptions,
    customFieldValue,
    customFieldOptionValues,
    canStartTaskDrag,
    taskDropMarkerVisible,
    onSectionDragOver,
    onSectionDrop,
    onToggleSectionSelection,
    onToggleSectionCollapsed,
    onSectionDraftChange,
    onSaveSection,
    onToggleSectionOptionsMenu,
    onRestoreSection,
    onHideSection,
    onArchiveSection,
    onSectionPointerDown,
    onSectionPointerUp,
    onSectionPointerCancel,
    onSectionDragStart,
    onSectionDragEnd,
    onResizePointerDown,
    onResizeDoubleClick,
    onResizeKeydown,
    onSaveCustomFieldValue,
    onToggleTaskSelection,
    onOpenTask,
    onTaskPointerDown,
    onTaskPointerUp,
    onTaskPointerCancel,
    onTaskDragStart,
    onTaskDragEnd,
    onTaskDragOver,
    onTaskDrop,
    onToggleStatusMenu,
    onSetStatus,
    onTogglePriorityMenu,
    onSetPriority,
    onToggleStartDateMenu,
    onCloseStartDateMenu,
    onSetStartDate,
    onClearStartDate,
    onSetStartTime,
    onClearStartTime,
    onToggleDueDateMenu,
    onCloseDueDateMenu,
    onSetDueDate,
    onClearDueDate,
    onSetDueTime,
    onClearDueTime,
    onRenameTask,
    taskExpanded,
    onToggleTaskExpanded,
    subtaskQuickAdd,
    onTaskDraftChange,
    onTaskAddActiveChange,
    onSubmitTask,
  }: {
    section: ProjectSection;
    sectionTasks: ProjectTask[];
    gridTemplate: string;
    gridMinWidth: string;
    leadingGridTemplate: string;
    taskListColumns: ProjectTaskListColumn[];
    taskListColumnLabel: (column: ProjectTaskListColumn) => string;
    statuses: ProjectStatus[];
    priorities: ProjectPriorityConfig[];
    projectCustomFields: ProjectCustomField[];
    selectedTaskId: string | null;
    statusMenuTaskId: string | null;
    priorityMenuTaskId: string | null;
    startDateMenuTaskId: string | null;
    dueDateMenuTaskId: string | null;
    draggingTaskId: string | null;
    dropPendingTaskId: string | null;
    sectionDragOver: boolean;
    sectionDragging: boolean;
    sectionDropPending: boolean;
    sectionDropMarkerBefore: boolean;
    sectionDropMarkerAfter: boolean;
    taskSectionDropMarkerVisible: boolean;
    theme: Theme;
    allTasksSelected: boolean;
    partiallySelected: boolean;
    canDragSection: boolean;
    sectionDraft: string;
    sectionDraftDirty: boolean;
    sectionDraftSaveable: boolean;
    sectionOptionsMenuOpen: boolean;
    taskDraft: string;
    taskAddLabel: string;
    taskAddActive: boolean;
    taskAddPending: boolean;
    taskAddError: string | null;
    statusForTask: (task: ProjectTask) => ProjectStatus | undefined;
    subtasksForTask: (task: ProjectTask) => ProjectTask[];
    scheduledLabel: (taskId: string) => string | null;
    visibleTaskTags: (task: ProjectTask) => ProjectTag[];
    hiddenTaskTagCount: (task: ProjectTask) => number;
    blockedByDependencies: (task: ProjectTask) => ProjectTaskDependency[];
    blocksDependencies: (task: ProjectTask) => ProjectTaskDependency[];
    taskSelected: (task: ProjectTask) => boolean;
    estimateLabel: (minutes: number) => string;
    customFieldDisplayValue: (task: ProjectTask, field: ProjectCustomField) => string | undefined;
    customFieldOptions: (field: ProjectCustomField) => ProjectCustomFieldOption[];
    customFieldValue: (task: ProjectTask, field: ProjectCustomField) => ProjectCustomFieldValue | undefined;
    customFieldOptionValues: (task: ProjectTask, field: ProjectCustomField) => ProjectCustomFieldOption[];
    canStartTaskDrag: (task: ProjectTask) => boolean;
    taskDropMarkerVisible: (task: ProjectTask, position: ProjectListDropPosition) => boolean;
    onSectionDragOver: (event: DragEvent) => void;
    onSectionDrop: (event: DragEvent) => void;
    onToggleSectionSelection: () => void;
    onToggleSectionCollapsed: () => MaybePromise;
    onSectionDraftChange: (value: string) => void;
    onSaveSection: () => MaybePromise;
    onToggleSectionOptionsMenu: () => void;
    onRestoreSection: () => MaybePromise;
    onHideSection: () => MaybePromise;
    onArchiveSection: () => MaybePromise;
    onSectionPointerDown: (event: PointerEvent) => void;
    onSectionPointerUp: (event: PointerEvent) => void;
    onSectionPointerCancel: (event: PointerEvent) => void;
    onSectionDragStart: (event: DragEvent) => void;
    onSectionDragEnd: (event: DragEvent) => void;
    onResizePointerDown: (event: PointerEvent, column: ProjectTaskListResizableColumn) => void;
    onResizeDoubleClick: (event: MouseEvent, column: ProjectTaskListResizableColumn) => void;
    onResizeKeydown: (event: KeyboardEvent, column: ProjectTaskListResizableColumn) => void;
    onSaveCustomFieldValue: (
      task: ProjectTask,
      field: ProjectCustomField,
      value: Omit<ProjectCustomFieldValueUpdate, "taskId" | "fieldId">,
    ) => Promise<void>;
    onToggleTaskSelection: (task: ProjectTask) => void;
    onOpenTask: (task: ProjectTask) => void;
    onTaskPointerDown: (event: PointerEvent, task: ProjectTask) => void;
    onTaskPointerUp: (event: PointerEvent) => void;
    onTaskPointerCancel: (event: PointerEvent) => void;
    onTaskDragStart: (event: DragEvent, task: ProjectTask) => void;
    onTaskDragEnd: (event: DragEvent) => void;
    onTaskDragOver: (event: DragEvent, task: ProjectTask) => void;
    onTaskDrop: (event: DragEvent, task: ProjectTask) => void;
    onToggleStatusMenu: (task: ProjectTask) => void;
    onSetStatus: (task: ProjectTask, status: ProjectStatus) => void;
    onTogglePriorityMenu: (task: ProjectTask) => void;
    onSetPriority: (task: ProjectTask, priority: ProjectPriority) => void;
    onToggleStartDateMenu: (task: ProjectTask) => void;
    onCloseStartDateMenu: () => void;
    onSetStartDate: (task: ProjectTask, startDate: string) => void;
    onClearStartDate: (task: ProjectTask) => void;
    onSetStartTime: (task: ProjectTask, startTime: string) => void;
    onClearStartTime: (task: ProjectTask) => void;
    onToggleDueDateMenu: (task: ProjectTask) => void;
    onCloseDueDateMenu: () => void;
    onSetDueDate: (task: ProjectTask, dueDate: string) => void;
    onClearDueDate: (task: ProjectTask) => void;
    onSetDueTime: (task: ProjectTask, dueTime: string) => void;
    onClearDueTime: (task: ProjectTask) => void;
    /** Saves a task title edited in its name cell. */
    onRenameTask: (task: ProjectTask, title: string) => void;
    taskExpanded: (task: ProjectTask, hasSubtasks: boolean) => boolean;
    onToggleTaskExpanded: (task: ProjectTask, hasSubtasks: boolean) => void;
    subtaskQuickAdd: ProjectListSubtaskQuickAdd;
    onTaskDraftChange: (value: string) => void;
    onTaskAddActiveChange: (active: boolean) => void;
    onSubmitTask: () => MaybePromise;
  } = $props();
</script>

{#if sectionDropMarkerBefore}
  <div class="h-1 rounded-full bg-primary"></div>
{/if}
<section
  class={cn(
    "flex flex-col gap-0 border border-transparent",
    sectionDragOver && "border-primary/40 bg-primary/5",
    sectionDragging && "opacity-50",
    sectionDropPending && "opacity-60",
  )}
  style={`min-width: max(100%, ${gridMinWidth});`}
  role="list"
  aria-label={section.name}
  ondragover={onSectionDragOver}
  ondrop={onSectionDrop}
>
  <ProjectListSectionHeader
    {section}
    {gridTemplate}
    {gridMinWidth}
    {leadingGridTemplate}
    taskCount={sectionTasks.length}
    allSelected={allTasksSelected}
    partiallySelected={partiallySelected}
    canDrag={canDragSection}
    draft={sectionDraft}
    draftDirty={sectionDraftDirty}
    draftSaveable={sectionDraftSaveable}
    menuOpen={sectionOptionsMenuOpen}
    onToggleSelection={onToggleSectionSelection}
    onToggleCollapsed={onToggleSectionCollapsed}
    onDraftChange={onSectionDraftChange}
    onSave={onSaveSection}
    onToggleOptionsMenu={onToggleSectionOptionsMenu}
    onRestore={onRestoreSection}
    onHide={onHideSection}
    onArchive={onArchiveSection}
    onPointerDown={onSectionPointerDown}
    onPointerUp={onSectionPointerUp}
    onPointerCancel={onSectionPointerCancel}
    onDragStart={onSectionDragStart}
    onDragEnd={onSectionDragEnd}
  />
  {#if !section.collapsed && !section.archivedAt && !section.hiddenAt}
    <ProjectListColumnHeaders
      mode="section"
      rowKey={`section:${section.id}`}
      {gridTemplate}
      {gridMinWidth}
      {taskListColumns}
      {taskListColumnLabel}
      onResizePointerDown={onResizePointerDown}
      onResizeDoubleClick={onResizeDoubleClick}
      onResizeKeydown={onResizeKeydown}
    />
    <ProjectListTaskRows
      tasks={sectionTasks}
      {statuses}
      {priorities}
      {taskListColumns}
      {projectCustomFields}
      {selectedTaskId}
      {statusMenuTaskId}
      {priorityMenuTaskId}
      {startDateMenuTaskId}
      {dueDateMenuTaskId}
      {gridTemplate}
      {gridMinWidth}
      draggingTaskId={draggingTaskId}
      dropPendingTaskId={dropPendingTaskId}
      {theme}
      {statusForTask}
      {subtasksForTask}
      {scheduledLabel}
      {visibleTaskTags}
      {hiddenTaskTagCount}
      {blockedByDependencies}
      {blocksDependencies}
      {taskSelected}
      {estimateLabel}
      {customFieldDisplayValue}
      {customFieldOptions}
      {customFieldValue}
      {customFieldOptionValues}
      canStartTaskDrag={canStartTaskDrag}
      dropMarkerVisible={taskDropMarkerVisible}
      onSaveCustomFieldValue={onSaveCustomFieldValue}
      onToggleTaskSelection={onToggleTaskSelection}
      onOpenTask={onOpenTask}
      onPointerDown={onTaskPointerDown}
      onPointerUp={onTaskPointerUp}
      onPointerCancel={onTaskPointerCancel}
      onDragStart={onTaskDragStart}
      onDragEnd={onTaskDragEnd}
      onDragOver={onTaskDragOver}
      onDrop={onTaskDrop}
      {onToggleStatusMenu}
      {onSetStatus}
      {onTogglePriorityMenu}
      {onSetPriority}
      {onToggleStartDateMenu}
      {onCloseStartDateMenu}
      {onSetStartDate}
      {onClearStartDate}
      {onSetStartTime}
      {onClearStartTime}
      {onToggleDueDateMenu}
      {onCloseDueDateMenu}
      {onSetDueDate}
      {onClearDueDate}
      {onSetDueTime}
      {onClearDueTime}
      {onRenameTask}
      {taskExpanded}
      {onToggleTaskExpanded}
      {subtaskQuickAdd}
    />
    {#if taskSectionDropMarkerVisible}
      <div class="h-1 rounded-full bg-primary"></div>
    {/if}
    <ProjectListTaskAddRow
      mode="section"
      rowId={section.id}
      {gridTemplate}
      {gridMinWidth}
      label={taskAddLabel}
      draft={taskDraft}
      active={taskAddActive}
      pending={taskAddPending}
      error={taskAddError}
      onDraftChange={onTaskDraftChange}
      onActiveChange={onTaskAddActiveChange}
      onSubmit={onSubmitTask}
    />
  {/if}
</section>
{#if sectionDropMarkerAfter}
  <div class="h-1 rounded-full bg-primary"></div>
{/if}
