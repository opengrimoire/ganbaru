<script lang="ts">
  import type { ProjectListDropPosition } from "$lib/projects/list/drag";
  import type {
    ProjectCustomField,
    ProjectCustomFieldOption,
    ProjectCustomFieldValue,
    ProjectCustomFieldValueUpdate,
    ProjectPriority,
    ProjectPriorityConfig,
    ProjectStatus,
    ProjectTag,
    ProjectTask,
    ProjectTaskDependency,
    ProjectTaskListColumn,
  } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import ProjectListTaskRow from "./ProjectListTaskRow.svelte";

  let {
    tasks,
    statuses,
    priorities,
    taskListColumns,
    projectCustomFields,
    selectedTaskId,
    statusMenuTaskId,
    priorityMenuTaskId,
    startDateMenuTaskId,
    dueDateMenuTaskId,
    gridTemplate,
    gridMinWidth,
    draggingTaskId = null,
    dropPendingTaskId = null,
    theme,
    statusForTask,
    subtasksForTask,
    scheduledLabel,
    visibleTaskTags,
    hiddenTaskTagCount,
    blockedByDependencies,
    blocksDependencies,
    taskSelected,
    sectionNameForTask,
    estimateLabel,
    customFieldDisplayValue,
    customFieldOptions,
    customFieldValue,
    customFieldOptionValues,
    canStartTaskDrag,
    dropMarkerVisible,
    onSaveCustomFieldValue,
    onToggleTaskSelection,
    onOpenTask,
    onPointerDown,
    onPointerUp,
    onPointerCancel,
    onDragStart,
    onDragEnd,
    onDragOver,
    onDrop,
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
    onToggleSubtaskDone,
  }: {
    tasks: ProjectTask[];
    statuses: ProjectStatus[];
    priorities: ProjectPriorityConfig[];
    taskListColumns: ProjectTaskListColumn[];
    projectCustomFields: ProjectCustomField[];
    selectedTaskId: string | null;
    statusMenuTaskId: string | null;
    priorityMenuTaskId: string | null;
    startDateMenuTaskId: string | null;
    dueDateMenuTaskId: string | null;
    gridTemplate: string;
    gridMinWidth: string;
    draggingTaskId?: string | null;
    dropPendingTaskId?: string | null;
    theme: Theme;
    statusForTask: (task: ProjectTask) => ProjectStatus | undefined;
    subtasksForTask: (task: ProjectTask) => ProjectTask[];
    scheduledLabel: (taskId: string) => string | null;
    visibleTaskTags: (task: ProjectTask) => ProjectTag[];
    hiddenTaskTagCount: (task: ProjectTask) => number;
    blockedByDependencies: (task: ProjectTask) => ProjectTaskDependency[];
    blocksDependencies: (task: ProjectTask) => ProjectTaskDependency[];
    taskSelected: (task: ProjectTask) => boolean;
    sectionNameForTask?: (task: ProjectTask) => string | undefined;
    estimateLabel: (minutes: number) => string;
    customFieldDisplayValue: (task: ProjectTask, field: ProjectCustomField) => string | undefined;
    customFieldOptions: (field: ProjectCustomField) => ProjectCustomFieldOption[];
    customFieldValue: (task: ProjectTask, field: ProjectCustomField) => ProjectCustomFieldValue | undefined;
    customFieldOptionValues: (task: ProjectTask, field: ProjectCustomField) => ProjectCustomFieldOption[];
    canStartTaskDrag?: (task: ProjectTask) => boolean;
    dropMarkerVisible?: (task: ProjectTask, position: ProjectListDropPosition) => boolean;
    onSaveCustomFieldValue: (
      task: ProjectTask,
      field: ProjectCustomField,
      value: Omit<ProjectCustomFieldValueUpdate, "taskId" | "fieldId">,
    ) => Promise<void>;
    onToggleTaskSelection: (task: ProjectTask) => void;
    onOpenTask: (task: ProjectTask) => void;
    onPointerDown?: (event: PointerEvent, task: ProjectTask) => void;
    onPointerUp?: (event: PointerEvent) => void;
    onPointerCancel?: (event: PointerEvent) => void;
    onDragStart?: (event: DragEvent, task: ProjectTask) => void;
    onDragEnd?: (event: DragEvent) => void;
    onDragOver?: (event: DragEvent, task: ProjectTask) => void;
    onDrop?: (event: DragEvent, task: ProjectTask) => void;
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
    onToggleSubtaskDone: (task: ProjectTask) => void;
  } = $props();
</script>

<div class="grid">
  {#each tasks as task (task.id)}
    {@const status = statusForTask(task)}
    {@const subtasks = subtasksForTask(task)}
    {#if dropMarkerVisible?.(task, "before")}
      <div class="h-1 rounded-full bg-primary"></div>
    {/if}
    <ProjectListTaskRow
      {task}
      {status}
      {statuses}
      {priorities}
      {subtasks}
      scheduled={scheduledLabel(task.id)}
      taskTags={visibleTaskTags(task)}
      hiddenTagCount={hiddenTaskTagCount(task)}
      blockedByCount={blockedByDependencies(task).length}
      blocksCount={blocksDependencies(task).length}
      {taskListColumns}
      {projectCustomFields}
      {selectedTaskId}
      taskSelected={taskSelected(task)}
      {gridTemplate}
      {gridMinWidth}
      sectionName={sectionNameForTask?.(task)}
      draggable={canStartTaskDrag?.(task) ?? false}
      dragging={draggingTaskId === task.id}
      dropPending={dropPendingTaskId === task.id}
      statusMenuOpen={statusMenuTaskId === task.id}
      priorityMenuOpen={priorityMenuTaskId === task.id}
      startDateMenuOpen={startDateMenuTaskId === task.id}
      dueDateMenuOpen={dueDateMenuTaskId === task.id}
      {theme}
      {estimateLabel}
      {customFieldDisplayValue}
      {customFieldOptions}
      {customFieldValue}
      {customFieldOptionValues}
      {statusForTask}
      {onSaveCustomFieldValue}
      {onToggleTaskSelection}
      {onOpenTask}
      onPointerDown={onPointerDown ? (event) => onPointerDown(event, task) : undefined}
      {onPointerUp}
      {onPointerCancel}
      onDragStart={onDragStart ? (event) => onDragStart(event, task) : undefined}
      {onDragEnd}
      onDragOver={onDragOver ? (event) => onDragOver(event, task) : undefined}
      onDrop={onDrop ? (event) => onDrop(event, task) : undefined}
      onToggleStatusMenu={() => onToggleStatusMenu(task)}
      onSetStatus={(nextStatus) => onSetStatus(task, nextStatus)}
      onTogglePriorityMenu={() => onTogglePriorityMenu(task)}
      onSetPriority={(priority) => onSetPriority(task, priority)}
      onToggleStartDateMenu={() => onToggleStartDateMenu(task)}
      {onCloseStartDateMenu}
      onSetStartDate={(startDate) => onSetStartDate(task, startDate)}
      onClearStartDate={() => onClearStartDate(task)}
      onSetStartTime={(startTime) => onSetStartTime(task, startTime)}
      onClearStartTime={() => onClearStartTime(task)}
      onToggleDueDateMenu={() => onToggleDueDateMenu(task)}
      {onCloseDueDateMenu}
      onSetDueDate={(dueDate) => onSetDueDate(task, dueDate)}
      onClearDueDate={() => onClearDueDate(task)}
      onSetDueTime={(dueTime) => onSetDueTime(task, dueTime)}
      onClearDueTime={() => onClearDueTime(task)}
      onRename={(title) => onRenameTask(task, title)}
      onToggleSubtaskDone={onToggleSubtaskDone}
    />
    {#if dropMarkerVisible?.(task, "after")}
      <div class="h-1 rounded-full bg-primary"></div>
    {/if}
  {/each}
</div>
