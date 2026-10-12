<script lang="ts">
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type {
    ProjectPriorityConfig,
    ProjectStatus,
    ProjectTask,
    ProjectTaskChangeEvent,
    ProjectDashboardTaskAggregates,
  } from "$lib/projects/types";
  import {
    projectPriorityDisplayColor,
    projectPriorityDisplayLabel,
    projectTaskHistoryEventLabel,
  } from "$lib/projects/display";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import ProjectListScrollbars from "$lib/components/projects/list/ProjectListScrollbars.svelte";
  import PriorityFlagIcon from "$lib/components/projects/PriorityFlagIcon.svelte";
  import ProjectStatusBadge from "$lib/components/projects/ProjectStatusBadge.svelte";

  let {
    projectId,
    tasks,
    statuses,
    priorities,
    todayDate,
    scheduledTaskIds,
    scheduledThisWeekMinutes,
    onOpenTask,
    aggregates,
    mobileLayout = false,
  }: {
    projectId: string | null;
    tasks: ProjectTask[];
    statuses: ProjectStatus[];
    priorities: ProjectPriorityConfig[];
    todayDate: string;
    scheduledTaskIds: ReadonlySet<string>;
    scheduledThisWeekMinutes: number;
    onOpenTask: (task: ProjectTask) => void;
    aggregates?: ProjectDashboardTaskAggregates;
    mobileLayout?: boolean;
  } = $props();

  const projects = getProjects();
  const theme = getTheme();
  const { t } = getLocalization();

  let dashboardScrollContainer = $state<HTMLElement | undefined>();

  const completedTaskCount = $derived(aggregates?.completed ?? tasks.filter((task) => isTaskDone(task)).length);

  function taskCountForStatus(status: ProjectStatus): number {
    return aggregates?.statusCounts[status.id]
      ?? tasks.filter((task) => task.statusId === status.id && !task.parentTaskId).length;
  }

  function projectCompletionPercent(): number {
    const total = aggregates?.total ?? tasks.length;
    if (total === 0) return 0;
    return Math.round((completedTaskCount / total) * 100);
  }

  function isTaskDone(task: ProjectTask): boolean {
    return projects.statusById(task.statusId)?.terminal === true;
  }

  function taskDateForDeadline(task: ProjectTask): string | undefined {
    return task.dueDate ?? task.targetEndDate;
  }

  function upcomingDeadlineTasks(): ProjectTask[] {
    return tasks
      .filter((task) => {
        const deadline = taskDateForDeadline(task);
        return deadline !== undefined && !isTaskDone(task) && deadline >= todayDate;
      })
      .sort((a, b) => (taskDateForDeadline(a) ?? "").localeCompare(taskDateForDeadline(b) ?? ""))
      .slice(0, 5);
  }

  function overdueTasks(): ProjectTask[] {
    return tasks
      .filter((task) => {
        const deadline = taskDateForDeadline(task);
        return deadline !== undefined && !isTaskDone(task) && deadline < todayDate;
      })
      .sort((a, b) => (taskDateForDeadline(a) ?? "").localeCompare(taskDateForDeadline(b) ?? ""))
      .slice(0, 5);
  }

  function unscheduledDueTasks(): ProjectTask[] {
    return tasks
      .filter((task) =>
        Boolean(task.dueDate)
        && !isTaskDone(task)
        && !scheduledTaskIds.has(task.id)
      )
      .sort((a, b) => (a.dueDate ?? "").localeCompare(b.dueDate ?? ""))
      .slice(0, 5);
  }

  function blockedDashboardTasks(): ProjectTask[] {
    return tasks
      .filter((task) =>
        projects.statusById(task.statusId)?.category === "blocked"
        || task.summaryBlocked === true
        || Boolean(task.blockerReason?.trim())
        || projects.dependenciesBlockingTask(task.id).length > 0
      )
      .slice(0, 5);
  }

  function recentlyCompletedTasks(): ProjectTask[] {
    return tasks
      .filter((task) => isTaskDone(task) && Boolean(task.completedAt))
      .sort((a, b) => (b.completedAt ?? "").localeCompare(a.completedAt ?? ""))
      .slice(0, 5);
  }

  function tasksWithoutEstimates(): ProjectTask[] {
    return tasks
      .filter((task) => !isTaskDone(task) && task.estimateMinutes === undefined)
      .slice(0, 5);
  }

  function recentProjectChangeEvents(): ProjectTaskChangeEvent[] {
    return projects.recentTaskChangeEventsForProject(projectId, 6);
  }

  function taskById(taskId: string): ProjectTask | undefined {
    return projects.tasks.find((task) => task.id === taskId);
  }

  function historyTaskTitle(event: ProjectTaskChangeEvent): string {
    return taskById(event.taskId)?.title ?? t("projects.detail.missingHistoryTask");
  }

  function totalOpenEstimateMinutes(): number {
    return aggregates?.openEstimateMinutes ?? tasks
      .filter((task) => !isTaskDone(task))
      .reduce((total, task) => total + (task.estimateMinutes ?? 0), 0);
  }

  function formatMinutesAsHours(minutes: number): string {
    if (minutes < 60) return t("projects.dashboard.minutes", minutes);
    const hours = minutes / 60;
    return t("projects.dashboard.hours", Number.isInteger(hours) ? hours.toFixed(0) : hours.toFixed(1));
  }
</script>

<div class="relative h-full min-h-0" class:mobilePresentation={mobileLayout}>
  <div bind:this={dashboardScrollContainer} class="project-dashboard-scroll h-full min-h-0 overflow-y-auto">
    <div class="grid min-h-full gap-3 p-3 min-[760px]:grid-cols-2">
  <section class="rounded-md border border-border bg-card p-3">
    <h2 class="mb-2 text-[0.866667rem] font-semibold">{t("projects.dashboard.taskCounts")}</h2>
    <div class="grid gap-2 min-[520px]:grid-cols-3">
      <div>
        <div class="text-[1.6rem] font-semibold">{projectCompletionPercent()}%</div>
        <div class="text-[0.733333rem] text-muted-foreground">{t("projects.dashboard.complete")}</div>
      </div>
      <div>
        <div class="text-[1.2rem] font-semibold">{formatMinutesAsHours(totalOpenEstimateMinutes())}</div>
        <div class="text-[0.733333rem] text-muted-foreground">{t("projects.dashboard.openEstimate")}</div>
      </div>
      <div>
        <div class="text-[1.2rem] font-semibold">{formatMinutesAsHours(scheduledThisWeekMinutes)}</div>
        <div class="text-[0.733333rem] text-muted-foreground">{t("projects.dashboard.scheduledThisWeek")}</div>
      </div>
    </div>
    <div class="mt-3 grid gap-1 text-[0.8rem] text-muted-foreground">
      {#each statuses as status (status.id)}
        <div class="flex items-center justify-between gap-2">
          <ProjectStatusBadge
            {status}
            theme={theme.current}
            label={status.name}
            class="text-[0.733333rem]"
          />
          <span>{taskCountForStatus(status)}</span>
        </div>
      {/each}
    </div>
  </section>
  <section class="rounded-md border border-border bg-card p-3">
    <h2 class="mb-2 text-[0.866667rem] font-semibold">{t("projects.dashboard.blocked")}</h2>
    <div class="grid gap-1">
      {#each blockedDashboardTasks() as task (task.id)}
        <button
          type="button"
          class="flex min-w-0 items-center gap-2 rounded border border-border px-2 py-1 text-left text-[0.8rem] hover:bg-accent"
          onclick={() => onOpenTask(task)}
        >
          <CircleAlert size={14} strokeWidth={1.75} class="shrink-0 text-destructive" />
          <span class="truncate">{task.title}</span>
        </button>
      {:else}
        <div class="text-[0.8rem] text-muted-foreground">{t("projects.dashboard.noBlockedTasks")}</div>
      {/each}
    </div>
  </section>
  <section class="rounded-md border border-border bg-card p-3">
    <h2 class="mb-2 text-[0.866667rem] font-semibold">{t("projects.dashboard.upcomingDeadlines")}</h2>
    <div class="grid gap-1">
      {#each upcomingDeadlineTasks() as task (task.id)}
        <button
          type="button"
          class="grid min-h-8 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded border border-border px-2 py-1 text-left text-[0.8rem] hover:bg-accent"
          onclick={() => onOpenTask(task)}
        >
          <span class="truncate">{task.title}</span>
          <span class="text-[0.733333rem] text-muted-foreground">{taskDateForDeadline(task)}</span>
        </button>
      {:else}
        <div class="text-[0.8rem] text-muted-foreground">{t("projects.dashboard.noDeadlines")}</div>
      {/each}
    </div>
  </section>
  <section class="rounded-md border border-border bg-card p-3">
    <h2 class="mb-2 text-[0.866667rem] font-semibold">{t("projects.dashboard.overdue")}</h2>
    <div class="grid gap-1">
      {#each overdueTasks() as task (task.id)}
        <button
          type="button"
          class="grid min-h-8 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded border border-destructive/30 bg-destructive/5 px-2 py-1 text-left text-[0.8rem] hover:bg-destructive/10"
          onclick={() => onOpenTask(task)}
        >
          <span class="truncate">{task.title}</span>
          <span class="text-[0.733333rem] text-destructive">{taskDateForDeadline(task)}</span>
        </button>
      {:else}
        <div class="text-[0.8rem] text-muted-foreground">{t("projects.dashboard.noOverdueTasks")}</div>
      {/each}
    </div>
  </section>
  <section class="rounded-md border border-border bg-card p-3">
    <h2 class="mb-2 text-[0.866667rem] font-semibold">{t("projects.dashboard.unscheduledWithDueDate")}</h2>
    <div class="grid gap-1">
      {#each unscheduledDueTasks() as task (task.id)}
        <button
          type="button"
          class="grid min-h-8 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded border border-border px-2 py-1 text-left text-[0.8rem] hover:bg-accent"
          onclick={() => onOpenTask(task)}
        >
          <span class="truncate">{task.title}</span>
          <span class="text-[0.733333rem] text-muted-foreground">{task.dueDate}</span>
        </button>
      {:else}
        <div class="text-[0.8rem] text-muted-foreground">{t("projects.dashboard.noUnscheduledDueTasks")}</div>
      {/each}
    </div>
  </section>
  <section class="rounded-md border border-border bg-card p-3">
    <h2 class="mb-2 text-[0.866667rem] font-semibold">{t("projects.dashboard.needsEstimates")}</h2>
    <div class="grid gap-1">
      {#each tasksWithoutEstimates() as task (task.id)}
        <button
          type="button"
          class="grid min-h-8 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded border border-border px-2 py-1 text-left text-[0.8rem] hover:bg-accent"
          onclick={() => onOpenTask(task)}
        >
          <span class="truncate">{task.title}</span>
          <span class="flex items-center gap-1.5 text-[0.733333rem] text-muted-foreground">
            <PriorityFlagIcon
              color={projectPriorityDisplayColor(task.priority, priorities)}
              theme={theme.current}
              size={12}
              class="shrink-0"
            />
            <span>{projectPriorityDisplayLabel(task.priority, priorities, t)}</span>
          </span>
        </button>
      {:else}
        <div class="text-[0.8rem] text-muted-foreground">{t("projects.dashboard.noMissingEstimates")}</div>
      {/each}
    </div>
  </section>
  <section class="rounded-md border border-border bg-card p-3 min-[760px]:col-span-2">
    <h2 class="mb-2 text-[0.866667rem] font-semibold">{t("projects.dashboard.recentChanges")}</h2>
    <div class="grid gap-1 min-[760px]:grid-cols-2">
      {#each recentProjectChangeEvents() as event (event.id)}
        {@const historyTask = taskById(event.taskId)}
        <button
          type="button"
          class="grid min-h-11 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded border border-border px-2 py-1 text-left hover:bg-accent disabled:cursor-default disabled:opacity-70"
          disabled={!historyTask}
          onclick={() => {
            if (historyTask) onOpenTask(historyTask);
          }}
        >
          <span class="min-w-0">
            <span class="block truncate text-[0.8rem]">{historyTaskTitle(event)}</span>
            <span class="block truncate text-[0.733333rem] text-muted-foreground">{projectTaskHistoryEventLabel(event, t)}</span>
            {#if event.reason}
              <span class="block truncate text-[0.733333rem] text-muted-foreground">
                {t("projects.history.reason", event.reason)}
              </span>
            {/if}
          </span>
          <span class="text-[0.733333rem] text-muted-foreground">{event.occurredAt}</span>
        </button>
      {:else}
        <div class="text-[0.8rem] text-muted-foreground">{t("projects.dashboard.noRecentChanges")}</div>
      {/each}
    </div>
  </section>
  <section class="rounded-md border border-border bg-card p-3 min-[760px]:col-span-2">
    <h2 class="mb-2 text-[0.866667rem] font-semibold">{t("projects.dashboard.recentlyCompleted")}</h2>
    <div class="grid gap-1 min-[760px]:grid-cols-2">
      {#each recentlyCompletedTasks() as task (task.id)}
        <button
          type="button"
          class="grid min-h-8 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded border border-border px-2 py-1 text-left text-[0.8rem] hover:bg-accent"
          onclick={() => onOpenTask(task)}
        >
          <span class="truncate">{task.title}</span>
          <span class="text-[0.733333rem] text-muted-foreground">{task.completedAt?.slice(0, 10)}</span>
        </button>
      {:else}
        <div class="text-[0.8rem] text-muted-foreground">{t("projects.dashboard.noRecentlyCompleted")}</div>
      {/each}
    </div>
  </section>
    </div>
  </div>
  <ProjectListScrollbars scrollContainer={dashboardScrollContainer} />
</div>

<style>
  .project-dashboard-scroll {
    overflow-x: hidden;
    padding-right: 0.5rem;
    padding-bottom: 0.5rem;
    scrollbar-width: none;
  }

  .project-dashboard-scroll::-webkit-scrollbar {
    display: none;
  }

  .mobilePresentation :global(button) {
    min-height: var(--touch-target-min);
  }
</style>
