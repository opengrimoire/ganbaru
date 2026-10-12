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
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { Theme } from "$lib/themes";
  import { collectionRowIndent } from "$lib/components/collections/row-expansion";
  import ProjectListTaskAddRow from "./ProjectListTaskAddRow.svelte";
  import ProjectListTaskRow from "./ProjectListTaskRow.svelte";
  import type { ProjectListSubtaskQuickAdd } from "./quick-add-controller.svelte";

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
    taskExpanded,
    onToggleTaskExpanded,
    subtaskQuickAdd,
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
    /** Whether a top-level task shows its subtasks and the add-subtask row. */
    taskExpanded: (task: ProjectTask, hasSubtasks: boolean) => boolean;
    onToggleTaskExpanded: (task: ProjectTask, hasSubtasks: boolean) => void;
    subtaskQuickAdd: ProjectListSubtaskQuickAdd;
  } = $props();

  const { t } = getLocalization();
</script>

{#snippet taskRow(task: ProjectTask, depth: number, subtaskCount: number, expanded: boolean)}
  {@const topLevel = depth === 0}
  <ProjectListTaskRow
    {task}
    status={statusForTask(task)}
    {statuses}
    {priorities}
    {subtaskCount}
    {depth}
    expandable={topLevel}
    {expanded}
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
    sectionName={topLevel ? sectionNameForTask?.(task) : undefined}
    draggable={topLevel && (canStartTaskDrag?.(task) ?? false)}
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
    {onSaveCustomFieldValue}
    {onToggleTaskSelection}
    {onOpenTask}
    onToggleExpanded={() => onToggleTaskExpanded(task, subtaskCount > 0)}
    onPointerDown={topLevel && onPointerDown ? (event) => onPointerDown(event, task) : undefined}
    onPointerUp={topLevel ? onPointerUp : undefined}
    onPointerCancel={topLevel ? onPointerCancel : undefined}
    onDragStart={topLevel && onDragStart ? (event) => onDragStart(event, task) : undefined}
    onDragEnd={topLevel ? onDragEnd : undefined}
    onDragOver={topLevel && onDragOver ? (event) => onDragOver(event, task) : undefined}
    onDrop={topLevel && onDrop ? (event) => onDrop(event, task) : undefined}
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
  />
{/snippet}

<div class="grid">
  {#each tasks as task (task.id)}
    {@const subtasks = subtasksForTask(task)}
    {@const expanded = taskExpanded(task, subtasks.length > 0)}
    {#if dropMarkerVisible?.(task, "before")}
      <div class="h-1 rounded-full bg-primary"></div>
    {/if}
    {@render taskRow(task, 0, subtasks.length, expanded)}
    {#if expanded}
      {#each subtasks as subtask (subtask.id)}
        {@render taskRow(subtask, 1, 0, false)}
      {/each}
      {#if !task.archivedAt}
        <ProjectListTaskAddRow
          mode="subtask"
          rowId={task.id}
          {gridTemplate}
          {gridMinWidth}
          label={t("projects.detail.addSubtask")}
          indent={collectionRowIndent(1)}
          draft={subtaskQuickAdd.subtaskDraft(task)}
          active={subtaskQuickAdd.subtaskActive(task)}
          pending={subtaskQuickAdd.pending}
          error={subtaskQuickAdd.subtaskError(task)}
          onDraftChange={(value) => subtaskQuickAdd.setSubtaskDraft(task, value)}
          onActiveChange={(active) => subtaskQuickAdd.setSubtaskActive(task, active)}
          onSubmit={() => subtaskQuickAdd.submitSubtask(task)}
        />
      {/if}
    {/if}
    {#if dropMarkerVisible?.(task, "after")}
      <div class="h-1 rounded-full bg-primary"></div>
    {/if}
  {/each}
</div>
