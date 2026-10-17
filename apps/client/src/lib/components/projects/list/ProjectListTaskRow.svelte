<script lang="ts">
  import CollectionRow from "$lib/components/collections/CollectionRow.svelte";
  import CollectionCell from "$lib/components/collections/CollectionCell.svelte";
  import CollectionExpandToggle from "$lib/components/collections/CollectionExpandToggle.svelte";
  import CollectionSelectionCheckbox from "$lib/components/collections/CollectionSelectionCheckbox.svelte";
  import { collectionRowIndent } from "$lib/components/collections/row-expansion";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    projectTagColorDotStyle,
    projectTagColorSwatchClass,
    projectTaskArchivedBadgeClass,
  } from "$lib/projects/display";
  import type {
    ProjectCustomField,
    ProjectCustomFieldOption,
    ProjectCustomFieldValue,
    ProjectCustomFieldValueUpdate,
    ProjectTag,
    ProjectPriority,
    ProjectPriorityConfig,
    ProjectStatus,
    ProjectTask,
    ProjectTaskListColumn,
  } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import { cn } from "$lib/utils";
  import ProjectListColumnCell from "./ProjectListColumnCell.svelte";
  import { getProjectListTableContext } from "./table-context";

  let {
    task,
    status,
    statuses,
    priorities,
    subtaskCount,
    depth = 0,
    expandable,
    expanded,
    scheduled,
    taskTags,
    hiddenTagCount,
    blockedByCount,
    blocksCount,
    taskListColumns,
    projectCustomFields,
    selectedTaskId,
    taskSelected,
    gridTemplate,
    gridMinWidth,
    sectionName,
    draggable,
    dragging,
    dropPending,
    statusMenuOpen,
    priorityMenuOpen,
    startDateMenuOpen,
    dueDateMenuOpen,
    theme,
    estimateLabel,
    customFieldDisplayValue,
    customFieldOptions,
    customFieldValue,
    customFieldOptionValues,
    onSaveCustomFieldValue,
    onToggleTaskSelection,
    onOpenTask,
    onToggleExpanded,
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
    onRename,
  }: {
    task: ProjectTask;
    status: ProjectStatus | undefined;
    statuses: ProjectStatus[];
    priorities: ProjectPriorityConfig[];
    subtaskCount: number;
    /** Nesting level; subtasks sit one level under their parent. */
    depth?: number;
    /** Whether the row can show nested rows, which subtasks cannot. */
    expandable: boolean;
    expanded: boolean;
    scheduled: string | null;
    taskTags: ProjectTag[];
    hiddenTagCount: number;
    blockedByCount: number;
    blocksCount: number;
    taskListColumns: ProjectTaskListColumn[];
    projectCustomFields: ProjectCustomField[];
    selectedTaskId: string | null;
    taskSelected: boolean;
    gridTemplate: string;
    gridMinWidth: string;
    sectionName?: string;
    draggable: boolean;
    dragging: boolean;
    dropPending: boolean;
    statusMenuOpen: boolean;
    priorityMenuOpen: boolean;
    startDateMenuOpen: boolean;
    dueDateMenuOpen: boolean;
    theme: Theme;
    estimateLabel: (minutes: number) => string;
    customFieldDisplayValue: (task: ProjectTask, field: ProjectCustomField) => string | undefined;
    customFieldOptions: (field: ProjectCustomField) => ProjectCustomFieldOption[];
    customFieldValue: (task: ProjectTask, field: ProjectCustomField) => ProjectCustomFieldValue | undefined;
    customFieldOptionValues: (task: ProjectTask, field: ProjectCustomField) => ProjectCustomFieldOption[];
    onSaveCustomFieldValue: (
      task: ProjectTask,
      field: ProjectCustomField,
      value: Omit<ProjectCustomFieldValueUpdate, "taskId" | "fieldId">,
    ) => Promise<void>;
    onToggleTaskSelection: (task: ProjectTask) => void;
    onOpenTask: (task: ProjectTask) => void;
    onToggleExpanded: () => void;
    onPointerDown?: (event: PointerEvent) => void;
    onPointerUp?: (event: PointerEvent) => void;
    onPointerCancel?: (event: PointerEvent) => void;
    onDragStart?: (event: DragEvent) => void;
    onDragEnd?: (event: DragEvent) => void;
    onDragOver?: (event: DragEvent) => void;
    onDrop?: (event: DragEvent) => void;
    onToggleStatusMenu: () => void;
    onSetStatus: (status: ProjectStatus) => void;
    onTogglePriorityMenu: () => void;
    onSetPriority: (priority: ProjectPriority) => void;
    onToggleStartDateMenu: () => void;
    onCloseStartDateMenu: () => void;
    onSetStartDate: (startDate: string) => void;
    onClearStartDate: () => void;
    onSetStartTime: (startTime: string) => void;
    onClearStartTime: () => void;
    onToggleDueDateMenu: () => void;
    onCloseDueDateMenu: () => void;
    onSetDueDate: (dueDate: string) => void;
    onClearDueDate: () => void;
    onSetDueTime: (dueTime: string) => void;
    onClearDueTime: () => void;
    /** Saves the title edited in the name cell. */
    onRename: (title: string) => void;
  } = $props();

  const { t } = getLocalization();
  const context = getProjectListTableContext();

  /** Save the edited title; an empty title is not allowed, so the field falls back to the saved one. */
  function commitTitle(event: FocusEvent & { currentTarget: HTMLInputElement }): void {
    const title = event.currentTarget.value.trim();
    if (!title) {
      event.currentTarget.value = task.title;
      return;
    }
    if (title !== task.title) onRename(title);
  }

  function handleTitleKeydown(event: KeyboardEvent & { currentTarget: HTMLInputElement }): void {
    if (event.key === "Enter") {
      event.preventDefault();
      event.currentTarget.blur();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.currentTarget.value = task.title;
      event.currentTarget.blur();
    }
  }
</script>

<div
  role="listitem"
  class={cn(
    "project-list-divider relative",
    task.archivedAt && "opacity-70",
    dragging && "opacity-50",
    dropPending && "opacity-60",
  )}
  style={`min-width: ${gridMinWidth}; ${context?.rowStyle(task, selectedTaskId === task.id) ?? ""}`}
>
  <CollectionRow template={gridTemplate} minWidth={gridMinWidth} gutter={context?.gutter} divider={false} selected={selectedTaskId === task.id}
    role="group"
    aria-label={task.title}
    {draggable}
    onpointerdown={(event) => onPointerDown?.(event)}
    onpointerup={(event) => onPointerUp?.(event)}
    onpointercancel={(event) => onPointerCancel?.(event)}
    ondragstart={(event) => onDragStart?.(event)}
    ondragend={(event) => onDragEnd?.(event)}
    ondragover={(event) => onDragOver?.(event)}
    ondrop={(event) => onDrop?.(event)}
  >
    <div class={`flex h-full items-center justify-center ${context?.cellClass("selection") ?? ""}`} style={context?.cellStyle("selection")}>
      <CollectionSelectionCheckbox checked={taskSelected}
        label={taskSelected ? t("projects.actions.unselectTask", task.title) : t("projects.actions.selectTask", task.title)}
        onToggle={() => onToggleTaskSelection(task)} />
    </div>
    <div class={`flex h-full items-center justify-center ${context?.cellClass("expand") ?? ""}`} style={context?.cellStyle("expand")}>
      {#if expandable}
        <CollectionExpandToggle {expanded} hasChildren={subtaskCount > 0}
          label={expanded ? t("collections.selection.collapse", task.title) : t("collections.selection.expand", task.title)}
          onToggle={onToggleExpanded} />
      {/if}
    </div>
    <CollectionCell class={context?.cellClass("name")} style={context?.cellStyle("name")}>
    <!-- Section, tags, and the archived badge share the title line so every row keeps the shared row height. -->
    <div class="flex w-full min-w-0 items-center gap-1.5" style={depth > 0 ? `padding-left: ${collectionRowIndent(depth)};` : undefined}>
      <input
        data-collection-cell-primary
        class="h-7 min-w-0 flex-1 truncate rounded-sm border border-transparent bg-transparent p-0 text-foreground outline-none"
        value={task.title}
        aria-label={t("projects.detail.titleLabel")}
        placeholder={t("projects.detail.titlePlaceholder")}
        disabled={Boolean(task.archivedAt)}
        onblur={commitTitle}
        onkeydown={handleTitleKeydown}
      />
      {#if sectionName || taskTags.length > 0 || task.archivedAt}
        <div class="flex max-w-1/2 min-w-0 shrink-0 items-center gap-1 overflow-hidden text-[0.666667rem] text-muted-foreground">
          {#if sectionName}
            <span class="min-w-0 truncate">{sectionName}</span>
          {/if}
          {#each taskTags as tag (tag.id)}
            <span class="inline-flex min-w-0 items-center gap-1 rounded border border-border bg-muted/50 px-1.5 py-0.5">
              <span
                class={cn("h-1.5 w-1.5 shrink-0 rounded-full border", projectTagColorSwatchClass(tag.color))}
                style={projectTagColorDotStyle(tag.color, theme)}
              ></span>
              <span class="truncate">{tag.name}</span>
            </span>
          {/each}
          {#if hiddenTagCount > 0}
            <span class="shrink-0 rounded border border-border bg-muted/50 px-1.5 py-0.5">
              {t("projects.list.moreTags", hiddenTagCount)}
            </span>
          {/if}
          {#if task.archivedAt}
            <span class={cn("shrink-0 rounded border px-1.5 py-0.5", projectTaskArchivedBadgeClass(task))}>
              {t("projects.taskLifecycle.archived")}
            </span>
          {/if}
        </div>
      {/if}
    </div>
    <button type="button" data-collection-hover-reveal=""
      class="absolute top-1/2 right-1 z-10 flex h-6 -translate-y-1/2 items-center gap-1 rounded-md border border-border bg-popover px-1.5 text-[0.75rem] text-popover-foreground opacity-100 hover:bg-accent hover:text-accent-foreground [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:group-hover/row:opacity-100 [@media(hover:hover)]:group-has-focus-visible/row:opacity-100"
      aria-label={t("projects.actions.openTaskDetails", task.title)} onclick={() => onOpenTask(task)}>
      <PanelRight class="size-3.5" aria-hidden="true" /><span>{t("collections.property.open")}</span>
    </button>
    </CollectionCell>
    {#each taskListColumns as column (column)}
      <ProjectListColumnCell
        {column}
        {task}
        {status}
        {statuses}
        {priorities}
        statusMenuOpen={statusMenuOpen}
        priorityMenuOpen={priorityMenuOpen}
        startDateMenuOpen={startDateMenuOpen}
        dueDateMenuOpen={dueDateMenuOpen}
        {projectCustomFields}
        {scheduled}
        {blockedByCount}
        {blocksCount}
        {estimateLabel}
        {customFieldDisplayValue}
        {customFieldOptions}
        {customFieldValue}
        {customFieldOptionValues}
        onSaveCustomFieldValue={onSaveCustomFieldValue}
        onToggleStatusMenu={onToggleStatusMenu}
        onSetStatus={onSetStatus}
        onTogglePriorityMenu={onTogglePriorityMenu}
        onSetPriority={onSetPriority}
        onToggleStartDateMenu={onToggleStartDateMenu}
        onCloseStartDateMenu={onCloseStartDateMenu}
        onSetStartDate={onSetStartDate}
        onClearStartDate={onClearStartDate}
        onSetStartTime={onSetStartTime}
        onClearStartTime={onClearStartTime}
        onToggleDueDateMenu={onToggleDueDateMenu}
        onCloseDueDateMenu={onCloseDueDateMenu}
        onSetDueDate={onSetDueDate}
        onClearDueDate={onClearDueDate}
        onSetDueTime={onSetDueTime}
        onClearDueTime={onClearDueTime}
      />
    {/each}
    <div class="min-h-(--collection-table-row-height) self-stretch" aria-hidden="true"></div>
  </CollectionRow>
</div>
