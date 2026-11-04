<script lang="ts">
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import CollectionBoard from "$lib/components/collections/CollectionBoard.svelte";
  import CollectionCard from "$lib/components/collections/CollectionCard.svelte";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionQuickAdd from "$lib/components/collections/CollectionQuickAdd.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { projectKanbanDropPlacement } from "$lib/projects/kanban-drag";
  import { projectPriorityDisplayColor, projectPriorityDisplayLabel } from "$lib/projects/display";
  import { manualStatusCompare } from "$lib/projects/tasks/view";
  import type { ProjectPriorityConfig, ProjectStatus, ProjectTask, ProjectTaskSortDirection, ProjectTaskSortMode, ProjectTaskColumnCount } from "$lib/projects/types";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import PriorityFlagIcon from "$lib/components/projects/PriorityFlagIcon.svelte";
  import ProjectStatusBadge from "$lib/components/projects/ProjectStatusBadge.svelte";

  let { tasks, statuses, priorities, selectedTaskIds, taskSortMode, taskSortDirection, onOpenTask, onToggleTaskSelection, columnCounts, onNeedMore, mobileLayout = false }: {
    tasks: ProjectTask[];
    statuses: ProjectStatus[];
    priorities: ProjectPriorityConfig[];
    selectedTaskIds: string[];
    taskSortMode: ProjectTaskSortMode;
    taskSortDirection: ProjectTaskSortDirection;
    onOpenTask: (task: ProjectTask) => void;
    onToggleTaskSelection: (task: ProjectTask) => void;
    columnCounts: ProjectTaskColumnCount[];
    onNeedMore: () => void;
    mobileLayout?: boolean;
  } = $props();

  const projects = useProjects();
  const theme = getTheme();
  const localization = getLocalization();
  const { t } = localization;
  let error = $state<string | null>(null);
  const selectedTaskIdSet = $derived(new Set(selectedTaskIds));

  function tasksForStatus(status: ProjectStatus): ProjectTask[] {
    const rows = tasks.filter((task) => task.statusId === status.id && !task.parentTaskId);
    return taskSortMode === "manual" ? [...rows].sort((a, b) => taskSortDirection === "asc" ? manualStatusCompare(a, b) : manualStatusCompare(b, a)) : rows;
  }

  function total(status: ProjectStatus): number {
    return columnCounts.find((column) => column.statusId === status.id)?.count ?? tasksForStatus(status).length;
  }

  async function move(task: ProjectTask, status: ProjectStatus, target: ProjectTask | null, position: "before" | "after"): Promise<void> {
    if (taskSortMode !== "manual") {
      if (task.statusId !== status.id) await projects.setTaskStatus(task, status.id);
      return;
    }
    const statusPlacement = projectKanbanDropPlacement({ orderedTasks: tasksForStatus(status), draggedTaskId: task.id, overTaskId: target?.id, position, sortDirection: taskSortDirection });
    if (!statusPlacement) return;
    await projects.updateTask(task, { statusId: status.id, statusPlacement });
  }

  async function perform(action: () => Promise<void>): Promise<void> {
    error = null;
    try { await action(); } catch (caught) { error = caught instanceof Error ? caught.message : String(caught); }
  }

  function canMove(task: ProjectTask, status: ProjectStatus): boolean {
    return !task.archivedAt && !task.parentTaskId && (!status.deletedAt || task.statusId === status.id) && task.projectId === status.projectId;
  }
</script>

{#if error}<Notice class="px-3 py-2 text-[0.8rem]" message={error} />{/if}
<CollectionBoard groups={statuses} items={tasksForStatus} label={(status) => status.name} count={total} fill
  reorder={taskSortMode === "manual"} {canMove} onMove={move}
  emptyLabel={t("projects.kanban.emptyColumn")} dragLabel={(task) => t("projects.actions.dragTask", task.title)}
  virtualize>
  {#snippet header(status)}
    <div class="min-w-0 flex-1"><ProjectStatusBadge {status} theme={theme.current} label={status.name} class="text-[0.8rem]" /></div>
    <span class="text-[0.8rem] tabular-nums text-muted-foreground">{formatNumber(localization.locale, total(status))}</span>
  {/snippet}
  {#snippet card(task, status, dragHandle)}
    {@const blockedByCount = projects.dependenciesBlockingTask(task.id).length}
    {@const blockingCount = projects.dependenciesBlockedByTask(task.id).length}
    <CollectionCard title={task.title} onOpen={() => onOpenTask(task)} selected={selectedTaskIdSet.has(task.id)} muted={Boolean(task.archivedAt)}>
      {#snippet leading()}
        {@render dragHandle()}
        <label class="mt-1 flex shrink-0 cursor-pointer items-center justify-center {mobileLayout ? 'size-11' : 'size-6'}"><Checkbox checked={selectedTaskIdSet.has(task.id)} label={t("projects.actions.selectTask", task.title)} onChange={() => onToggleTaskSelection(task)} /></label>
      {/snippet}
      {#snippet actions()}
        <CollectionMenu kind="actions" iconOnly showHeader={false} label={t("projects.actions.openTaskDetails", task.title)}>
          <button type="button" class="menu-item" onclick={() => onOpenTask(task)}>{t("projects.kanban.openDetails")}</button>
          {#each statuses.filter((entry) => entry.id !== status.id && !entry.deletedAt) as destination (destination.id)}
            <button type="button" class="menu-item" disabled={!canMove(task, destination)} onclick={() => { void perform(() => projects.setTaskStatus(task, destination.id)); }}>{t("projects.actions.moveTaskToStatus", task.title, destination.name)}</button>
          {/each}
          {#if taskSortMode === "manual"}
            {#each [-1, 1] as direction}
              {@const order = tasksForStatus(status)}
              {@const index = order.findIndex((entry) => entry.id === task.id)}
              <button type="button" class="menu-item" disabled={Boolean(task.archivedAt) || !order[index + direction]} onclick={() => { void perform(() => projects.moveTaskInStatus(task, (taskSortDirection === "asc" ? direction : -direction) as -1 | 1)); }}>{#if direction === -1}<ArrowUp class="size-4" />{:else}<ArrowDown class="size-4" />{/if}{direction === -1 ? t("projects.actions.moveTaskUp", task.title) : t("projects.actions.moveTaskDown", task.title)}</button>
            {/each}
          {/if}
        </CollectionMenu>
      {/snippet}
      <div class="flex flex-wrap items-center gap-1.5 text-muted-foreground"><PriorityFlagIcon color={projectPriorityDisplayColor(task.priority, priorities)} theme={theme.current} size={13} /><span>{projectPriorityDisplayLabel(task.priority, priorities, t)}</span>{#if task.dueDate}<span class="ml-auto">{formatDateTime(localization.locale, new Date(`${task.dueDate}T00:00:00Z`), { month: "short", day: "numeric", timeZone: "UTC" })}</span>{/if}</div>
      {#if blockedByCount || blockingCount}<div class="mt-2 flex flex-wrap gap-1 text-[0.733333rem]">{#if blockedByCount}<span class="rounded bg-destructive/10 px-1.5 py-0.5 text-destructive">{t("projects.list.blockedBy", blockedByCount)}</span>{/if}{#if blockingCount}<span class="rounded bg-muted px-1.5 py-0.5 text-muted-foreground">{t("projects.list.blocks", blockingCount)}</span>{/if}</div>{/if}
      {#if task.archivedAt}<span class="mt-1 block text-[0.733333rem] text-muted-foreground">{t("projects.taskLifecycle.archived")}</span>{/if}
    </CollectionCard>
  {/snippet}
  {#snippet footer(status)}
    {#if !status.deletedAt}<CollectionQuickAdd label={t("projects.list.addTaskInSection", status.name)} onSubmit={async (title) => Boolean(await projects.addTask(status.projectId, title, undefined, status.id))} />{/if}
    {#if tasksForStatus(status).length < total(status)}<button type="button" class="min-h-9 w-full rounded-md px-2 text-[0.8rem] text-muted-foreground hover:bg-accent" onclick={onNeedMore}>{t("common.loadMore")}</button>{/if}
  {/snippet}
</CollectionBoard>
