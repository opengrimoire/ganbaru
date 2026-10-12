<script lang="ts">
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    buildProjectGanttDatePatch,
    buildProjectGanttTimeline,
    type ProjectGanttDateInteraction,
    type ProjectGanttDependencyEdge,
    type ProjectGanttRow,
    type ProjectGanttTick,
  } from "$lib/projects/scheduling/gantt";
  import type { ProjectDependencyCascadeItem } from "$lib/api/project-cascade";
  import { ProjectDependencyCascadeController } from "$lib/projects/scheduling/dependency-cascade-controller.svelte";
  import type { ProjectSection, ProjectStatus, ProjectTask } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { cn } from "$lib/utils";

  let {
    tasks,
    statuses,
    sections,
    todayDate,
    onOpenTask,
    onToggleSectionCollapsed,
  }: {
    tasks: ProjectTask[];
    statuses: ProjectStatus[];
    sections: ProjectSection[];
    todayDate: string;
    onOpenTask: (task: ProjectTask) => void;
    onToggleSectionCollapsed: (section: ProjectSection) => void;
  } = $props();

  const projects = getProjects();
  const { t } = getLocalization();

  const cascade = new ProjectDependencyCascadeController(() => projects.selectedProjectId, (preview) => projects.applyDependencyCascade(preview));
  $effect(() => {
    projects.selectedProjectId;
    cascade.reset();
  });
  let ganttDateDrag = $state<{
    taskId: string;
    interaction: ProjectGanttDateInteraction;
    startClientX: number;
    dayWidthPx: number;
    pointerId: number;
    dayDelta: number;
  } | null>(null);
  let ganttDatePendingTaskId = $state<string | null>(null);
  let ganttSuppressClickTaskId = $state<string | null>(null);

  const taskIds = $derived.by(() => new Set(tasks.map((task) => task.id)));
  const dependencyBlockedTaskIds = $derived.by(() => new Set(
    projects.dependencies
      .filter((dependency) => taskIds.has(dependency.blockedTaskId))
      .map((dependency) => dependency.blockedTaskId),
  ));
  const ganttTimeline = $derived.by(() => buildProjectGanttTimeline({
    tasks,
    statuses,
    dependencies: projects.dependencies,
    dependencyBlockedTaskIds,
    today: todayDate,
  }));
  const dependencyCascadeProposal = $derived(cascade.preview);

  function taskById(taskId: string): ProjectTask | undefined {
    return tasks.find((task) => task.id === taskId);
  }

  function ganttRowsForSection(section: ProjectSection): ProjectGanttRow[] {
    const datedTaskIds = new Set(ganttTimeline.rows.map((row) => row.task.id));
    return ganttTimeline.rows.filter((row) =>
      row.task.sectionId === section.id
      && (!row.task.parentTaskId || !datedTaskIds.has(row.task.parentTaskId))
    );
  }

  function ganttSubtaskRows(parent: ProjectTask): ProjectGanttRow[] {
    return ganttTimeline.rows.filter((row) => row.task.parentTaskId === parent.id);
  }

  function percentStyle(leftPercent: number): string {
    return `left: ${leftPercent.toFixed(3)}%;`;
  }

  function ganttBarStyle(row: ProjectGanttRow): string {
    return `left: ${row.leftPercent.toFixed(3)}%; width: max(${row.widthPercent.toFixed(3)}%, 0.75rem);`;
  }

  function ganttDateControlClass(row: ProjectGanttRow): string {
    return cn(
      ganttRowTone(row),
      ganttDatePendingTaskId === row.task.id && "opacity-60",
      ganttDateDrag?.taskId === row.task.id && "ring-2 ring-ring",
    );
  }

  function ganttResizeHandleStyle(row: ProjectGanttRow, edge: "start" | "end"): string {
    const leftPercent = edge === "start"
      ? row.leftPercent
      : Math.min(100, row.leftPercent + row.widthPercent);
    return `left: ${leftPercent.toFixed(3)}%;`;
  }

  function ganttTrackForPointer(event: PointerEvent): HTMLElement | null {
    return (event.currentTarget as HTMLElement).closest<HTMLElement>("[data-gantt-track]");
  }

  function startGanttDateDrag(
    event: PointerEvent,
    row: ProjectGanttRow,
    interaction: ProjectGanttDateInteraction,
  ): void {
    if (row.task.archivedAt || ganttDatePendingTaskId) return;
    const track = ganttTrackForPointer(event);
    if (!track || ganttTimeline.totalDays <= 0) return;
    event.preventDefault();
    event.stopPropagation();
    const rect = track.getBoundingClientRect();
    const dayWidthPx = rect.width / ganttTimeline.totalDays;
    if (!Number.isFinite(dayWidthPx) || dayWidthPx <= 0) return;
    ganttDateDrag = {
      taskId: row.task.id,
      interaction,
      startClientX: event.clientX,
      dayWidthPx,
      pointerId: event.pointerId,
      dayDelta: 0,
    };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function updateGanttDateDrag(event: PointerEvent): void {
    if (!ganttDateDrag || ganttDateDrag.pointerId !== event.pointerId) return;
    event.preventDefault();
    const dayDelta = Math.round((event.clientX - ganttDateDrag.startClientX) / ganttDateDrag.dayWidthPx);
    if (dayDelta === ganttDateDrag.dayDelta) return;
    ganttDateDrag = { ...ganttDateDrag, dayDelta };
  }

  function clearGanttSuppressedClick(taskId: string): void {
    setTimeout(() => {
      if (ganttSuppressClickTaskId === taskId) ganttSuppressClickTaskId = null;
    }, 0);
  }

  async function commitGanttDateDrag(event: PointerEvent, row: ProjectGanttRow): Promise<void> {
    const drag = ganttDateDrag;
    if (!drag || drag.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    ganttDateDrag = null;
    if (drag.dayDelta === 0) return;
    ganttSuppressClickTaskId = row.task.id;
    clearGanttSuppressedClick(row.task.id);
    const patch = buildProjectGanttDatePatch(row.task, drag.interaction, drag.dayDelta);
    if (!patch) return;
    ganttDatePendingTaskId = row.task.id;
    try {
      await projects.updateTask(row.task, patch);
    } finally {
      ganttDatePendingTaskId = null;
    }
  }

  function cancelGanttDateDrag(event: PointerEvent): void {
    if (!ganttDateDrag || ganttDateDrag.pointerId !== event.pointerId) return;
    event.preventDefault();
    ganttDateDrag = null;
  }

  function openGanttTaskFromBar(task: ProjectTask): void {
    if (ganttSuppressClickTaskId === task.id) {
      ganttSuppressClickTaskId = null;
      return;
    }
    onOpenTask(task);
  }

  function ganttTickStyle(tick: ProjectGanttTick): string {
    return percentStyle(tick.leftPercent);
  }

  function ganttEdgeStyle(edge: ProjectGanttDependencyEdge): string {
    return `left: ${edge.lineStartPercent.toFixed(3)}%; width: max(${edge.lineWidthPercent.toFixed(3)}%, 0.5rem);`;
  }

  function ganttDateRangeLabel(startDate: string, endDate: string): string {
    return startDate === endDate ? startDate : t("projects.gantt.dateRange", startDate, endDate);
  }

  function cascadeItemRangeLabel(item: ProjectDependencyCascadeItem, next: boolean): string {
    return next
      ? ganttDateRangeLabel(item.nextRangeStart, item.nextRangeEnd)
      : ganttDateRangeLabel(item.originalRangeStart, item.originalRangeEnd);
  }

  function cascadeShiftLabel(item: ProjectDependencyCascadeItem): string {
    return t("projects.gantt.cascadeShiftDays", item.shiftDays);
  }

  async function openCascadeTask(taskId: string): Promise<void> {
    const projectId = dependencyCascadeProposal?.projectId;
    if (!projectId) return;
    try {
      await projects.ensureTaskDetailData(projectId, taskId);
      if (projects.selectedProjectId !== projectId) return;
      const task = projects.taskById(taskId);
      if (task) onOpenTask(task);
    } catch (error) {
      if (projects.selectedProjectId === projectId) cascade.error = error instanceof Error ? error.message : String(error);
    }
  }

  function ganttRowTone(row: ProjectGanttRow): string {
    if (row.overdue) return "border-destructive bg-destructive text-destructive-foreground";
    if (row.blocked) return "border-amber-600 bg-amber-500 text-black";
    if (row.done) return "border-emerald-600 bg-emerald-500 text-white";
    return "border-primary bg-primary text-primary-foreground";
  }

  function ganttDependencyLabel(row: ProjectGanttRow): string | undefined {
    if (row.blockedByCount > 0 && row.blocksCount > 0) {
      return t("projects.gantt.dependenciesBoth", row.blockedByCount, row.blocksCount);
    }
    if (row.blockedByCount > 0) return t("projects.gantt.blockedBy", row.blockedByCount);
    if (row.blocksCount > 0) return t("projects.gantt.blocks", row.blocksCount);
    return undefined;
  }

  function ganttEdgesFrom(taskId: string): ProjectGanttDependencyEdge[] {
    return ganttTimeline.dependencyEdges.filter((edge) => edge.blockingTaskId === taskId);
  }

  function ganttConflictsForTask(taskId: string): ProjectGanttDependencyEdge[] {
    return ganttTimeline.dependencyEdges.filter((edge) =>
      edge.violated && (edge.blockingTaskId === taskId || edge.blockedTaskId === taskId)
    );
  }

  function ganttEdgeTitle(edge: ProjectGanttDependencyEdge): string {
    return edge.violated
      ? t("projects.gantt.dependencyConflict", edge.blockingTitle, edge.blockedTitle)
      : t("projects.gantt.dependencyOk", edge.blockingTitle, edge.blockedTitle);
  }

  function openGanttDependencyTarget(edge: ProjectGanttDependencyEdge): void {
    const blockedTask = taskById(edge.blockedTaskId);
    if (blockedTask) onOpenTask(blockedTask);
  }
</script>

          <div class="flex min-h-full flex-col gap-3 p-3">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div>
                <h2 class="text-[0.866667rem] font-semibold">{t("projects.gantt.timeline")}</h2>
                <div class="text-[0.733333rem] text-muted-foreground">
                  {#if ganttTimeline.startDate && ganttTimeline.endDate}
                    {ganttDateRangeLabel(ganttTimeline.startDate, ganttTimeline.endDate)}
                  {:else}
                    {t("projects.gantt.noDateRange")}
                  {/if}
                </div>
              </div>
              {#if ganttTimeline.rows.length > 0}
                <div class="flex flex-wrap items-center gap-1 text-[0.733333rem] text-muted-foreground">
                  <span class="rounded border border-border bg-card px-1.5 py-0.5">{t("projects.gantt.taskCount", ganttTimeline.rows.length)}</span>
                  <span class="rounded border border-border bg-card px-1.5 py-0.5">{t("projects.gantt.dependencyCount", ganttTimeline.dependencyEdges.length)}</span>
                  <span class="rounded border border-destructive/40 bg-destructive/10 px-1.5 py-0.5 text-destructive">{t("projects.gantt.lateCount", ganttTimeline.rows.filter((row) => row.overdue).length)}</span>
                  <span class="rounded border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-amber-700 dark:text-amber-300">{t("projects.gantt.blockedCount", ganttTimeline.rows.filter((row) => row.blocked).length)}</span>
                  <span class="rounded border border-destructive/40 bg-destructive/10 px-1.5 py-0.5 text-destructive">{t("projects.gantt.dependencyConflictCount", ganttTimeline.dependencyEdges.filter((edge) => edge.violated).length)}</span>
                </div>
              {/if}
              <button
                type="button"
                class="rounded border border-border bg-card px-2 py-1 text-[0.733333rem] font-medium hover:bg-accent disabled:opacity-60"
                disabled={cascade.loading || cascade.applying || !projects.selectedProjectId}
                onclick={() => { void cascade.review(); }}
              >
                {cascade.loading ? t("common.loading") : t("projects.gantt.cascadeTitle")}
              </button>
            </div>

            {#if cascade.open}
              <section class="grid gap-2 rounded-md border border-border bg-card p-2">
                <div class="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <h3 class="text-[0.8rem] font-semibold">{t("projects.gantt.cascadeTitle")}</h3>
                    <div class="text-[0.733333rem] text-muted-foreground">
                      {t("projects.gantt.cascadeDescription")}
                    </div>
                  </div>
                  <div class="flex items-center gap-1">
                    <button
                      type="button"
                      class="rounded-md border border-border bg-background px-2 py-1 text-[0.766667rem] hover:bg-accent"
                      onclick={() => {
                        cascade.reset();
                      }}
                    >
                      {t("common.cancel")}
                    </button>
                    <button
                      type="button"
                      class="rounded-md bg-primary px-2 py-1 text-[0.766667rem] font-medium text-primary-foreground disabled:cursor-not-allowed disabled:opacity-60"
                      disabled={cascade.loading || cascade.applying || !dependencyCascadeProposal?.items.length || (dependencyCascadeProposal?.conflicts.length ?? 0) > 0}
                      onclick={() => { void cascade.apply(); }}
                    >
                      {cascade.applying ? t("common.loading") : t("projects.gantt.cascadeApply")}
                    </button>
                  </div>
                </div>
                <div class="grid gap-1">
                  {#each dependencyCascadeProposal?.items ?? [] as item (item.taskId)}
                    <button
                      type="button"
                      class="grid gap-1 rounded border border-border bg-background px-2 py-1.5 text-left hover:bg-accent"
                      onclick={() => { void openCascadeTask(item.taskId); }}
                    >
                      <span class="flex min-w-0 flex-wrap items-center gap-1">
                        <span class="min-w-0 flex-1 truncate text-[0.8rem] font-medium">{item.title}</span>
                        <span class="rounded border border-sky-500/40 bg-sky-500/10 px-1.5 py-0.5 text-[0.733333rem] text-sky-700 dark:text-sky-300">
                          {cascadeShiftLabel(item)}
                        </span>
                      </span>
                      <span class="text-[0.733333rem] text-muted-foreground">
                        {cascadeItemRangeLabel(item, false)} {t("projects.gantt.cascadeTo")} {cascadeItemRangeLabel(item, true)}
                      </span>
                      <span class="text-[0.733333rem] text-muted-foreground">
                        {#each item.reasons as reason, index (reason.dependencyId)}
                          {#if index > 0} / {/if}
                          {t("projects.gantt.cascadeReason", reason.blockingTitle, reason.requiredStartDate)}
                        {/each}
                      </span>
                    </button>
                  {/each}
                </div>
                {#if cascade.loading}
                  <p class="text-[0.766667rem] text-muted-foreground">{t("common.loading")}</p>
                {:else if dependencyCascadeProposal && !dependencyCascadeProposal.items.length && !dependencyCascadeProposal.conflicts.length}
                  <p class="text-[0.766667rem] text-muted-foreground">{t("projects.gantt.cascadeNoChanges")}</p>
                {/if}
                {#each dependencyCascadeProposal?.conflicts ?? [] as conflict (conflict.dependencyId)}
                  <p class="rounded border border-destructive/30 bg-destructive/10 px-2 py-1 text-[0.766667rem] text-destructive">
                    {conflict.title}: {t(`projects.gantt.cascadeConflicts.${conflict.reason}`)}
                  </p>
                {/each}
                {#if cascade.error}
                  <Notice class="text-[0.766667rem]" message={cascade.error} />
                {/if}
              </section>
            {/if}

            {#if ganttTimeline.rows.length > 0}
              <div class="min-h-0 overflow-x-auto rounded-md border border-border bg-card">
                <div class="min-w-208">
                  <div class="grid min-h-10 grid-cols-[minmax(12rem,16rem)_minmax(34rem,1fr)] border-b border-border bg-muted/40">
                    <div class="flex items-center border-r border-border px-2 text-[0.733333rem] font-medium text-muted-foreground">
                      {t("projects.gantt.taskColumn")}
                    </div>
                    <div class="relative">
                      {#each ganttTimeline.ticks as tick (tick.date)}
                        <div class="absolute inset-y-0 border-l border-border/70" style={ganttTickStyle(tick)}>
                          <span class="absolute left-1 top-1 text-[0.666667rem] text-muted-foreground">{tick.date}</span>
                        </div>
                      {/each}
                      {#if ganttTimeline.todayPercent !== undefined}
                        <div class="absolute inset-y-0 border-l border-primary/70" style={percentStyle(ganttTimeline.todayPercent)}>
                          <span class="absolute bottom-1 left-1 text-[0.666667rem] font-medium text-primary">{t("projects.gantt.today")}</span>
                        </div>
                      {/if}
                    </div>
                  </div>

                  {#each sections as section (section.id)}
                    {@const sectionRows = ganttRowsForSection(section)}
                    {#if sectionRows.length > 0}
                      <div class="grid min-h-8 grid-cols-[minmax(12rem,16rem)_minmax(34rem,1fr)] border-b border-border/70 bg-background/70">
                        <button
                          type="button"
                          class="flex min-w-0 items-center gap-1 border-r border-border px-2 text-left text-[0.8rem] font-medium hover:bg-accent"
                          aria-label={section.collapsed ? t("projects.actions.expandSection", section.name) : t("projects.actions.collapseSection", section.name)}
                          onclick={() => { onToggleSectionCollapsed(section); }}
                        >
                          {#if section.collapsed}
                            <ChevronRight size={13} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
                          {:else}
                            <ChevronDown size={13} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
                          {/if}
                          <span class="truncate">{section.name}</span>
                          <span class="ml-auto text-[0.733333rem] text-muted-foreground">{sectionRows.length}</span>
                        </button>
                        <div class="relative">
                          {#each ganttTimeline.ticks as tick (tick.date)}
                            <div class="absolute inset-y-0 border-l border-border/50" style={ganttTickStyle(tick)}></div>
                          {/each}
                        </div>
                      </div>
                      {#if !section.collapsed}
                        {#each sectionRows as row (row.task.id)}
                          {@const rowDependencyLabel = ganttDependencyLabel(row)}
                          {@const rowDependencyConflicts = ganttConflictsForTask(row.task.id)}
                          <div class="project-gantt-row grid min-h-11 grid-cols-[minmax(12rem,16rem)_minmax(34rem,1fr)] border-b border-border/50">
                            <button
                              type="button"
                              class="grid min-w-0 content-center gap-0.5 border-r border-border px-2 text-left hover:bg-accent"
                              aria-label={t("projects.actions.openTaskDetails", row.task.title)}
                              onclick={() => onOpenTask(row.task)}
                            >
                              <span class="truncate text-[0.8rem] font-medium">{row.task.title}</span>
                              <span class="truncate text-[0.733333rem] text-muted-foreground">
                                {ganttDateRangeLabel(row.startDate, row.endDate)}
                                {#if rowDependencyLabel}
                                  / {rowDependencyLabel}
                                {/if}
                                {#if rowDependencyConflicts.length > 0}
                                  / {t("projects.gantt.conflictCount", rowDependencyConflicts.length)}
                                {/if}
                              </span>
                            </button>
                            <div class="relative bg-muted/20" data-gantt-track>
                              {#each ganttTimeline.ticks as tick (tick.date)}
                                <div class="absolute inset-y-0 border-l border-border/50" style={ganttTickStyle(tick)}></div>
                              {/each}
                              {#if ganttTimeline.todayPercent !== undefined}
                                <div class="absolute inset-y-0 border-l border-primary/60" style={percentStyle(ganttTimeline.todayPercent)}></div>
                              {/if}
                              {#each ganttEdgesFrom(row.task.id) as edge (edge.id)}
                                <button
                                  type="button"
                                  class="absolute top-1 z-10 flex h-4 min-w-2 items-center overflow-hidden rounded text-[0.666667rem]"
                                  style={ganttEdgeStyle(edge)}
                                  aria-label={ganttEdgeTitle(edge)}
                                  title={ganttEdgeTitle(edge)}
                                  onclick={() => openGanttDependencyTarget(edge)}
                                >
                                  <span class={cn("h-px min-w-2 flex-1", edge.violated ? "bg-destructive" : "bg-muted-foreground/60")}></span>
                                  <ArrowRight
                                    size={10}
                                    strokeWidth={2}
                                    class={edge.violated ? "shrink-0 text-destructive" : "shrink-0 text-muted-foreground"}
                                  />
                                </button>
                              {/each}
                              {#if row.milestone}
                                <button
                                  type="button"
                                  class={cn(
                                    "absolute top-1/2 h-4 w-4 -translate-x-1/2 -translate-y-1/2 rotate-45 cursor-grab rounded-sm border shadow-sm active:cursor-grabbing disabled:cursor-not-allowed",
                                    ganttDateControlClass(row),
                                  )}
                                  style={percentStyle(row.markerPercent)}
                                  aria-label={t("projects.actions.openTaskDetails", row.task.title)}
                                  title={`${row.task.title} / ${row.startDate}`}
                                  disabled={Boolean(row.task.archivedAt) || ganttDatePendingTaskId === row.task.id}
                                  onpointerdown={(event) => startGanttDateDrag(event, row, "move")}
                                  onpointermove={updateGanttDateDrag}
                                  onpointerup={(event) => { void commitGanttDateDrag(event, row); }}
                                  onpointercancel={cancelGanttDateDrag}
                                  onclick={() => openGanttTaskFromBar(row.task)}
                                ></button>
                              {:else}
                                <button
                                  type="button"
                                  class="absolute top-1/2 z-20 h-6 w-3 -translate-x-1/2 -translate-y-1/2 rounded border border-background/80 bg-foreground/20 hover:bg-foreground/35 disabled:cursor-not-allowed disabled:opacity-40"
                                  style={ganttResizeHandleStyle(row, "start")}
                                  disabled={Boolean(row.task.archivedAt) || ganttDatePendingTaskId === row.task.id}
                                  aria-label={t("projects.actions.resizeGanttTaskStart", row.task.title)}
                                  title={t("projects.actions.resizeGanttTaskStart", row.task.title)}
                                  onpointerdown={(event) => startGanttDateDrag(event, row, "resize-start")}
                                  onpointermove={updateGanttDateDrag}
                                  onpointerup={(event) => { void commitGanttDateDrag(event, row); }}
                                  onpointercancel={cancelGanttDateDrag}
                                  onclick={(event) => event.stopPropagation()}
                                ></button>
                                <button
                                  type="button"
                                  class={cn(
                                    "absolute top-1/2 flex h-5 -translate-y-1/2 cursor-grab items-center rounded border px-1.5 text-left text-[0.733333rem] shadow-sm active:cursor-grabbing disabled:cursor-not-allowed",
                                    ganttDateControlClass(row),
                                  )}
                                  style={ganttBarStyle(row)}
                                  aria-label={t("projects.actions.openTaskDetails", row.task.title)}
                                  title={`${row.task.title} / ${ganttDateRangeLabel(row.startDate, row.endDate)}`}
                                  disabled={Boolean(row.task.archivedAt) || ganttDatePendingTaskId === row.task.id}
                                  onpointerdown={(event) => startGanttDateDrag(event, row, "move")}
                                  onpointermove={updateGanttDateDrag}
                                  onpointerup={(event) => { void commitGanttDateDrag(event, row); }}
                                  onpointercancel={cancelGanttDateDrag}
                                  onclick={() => openGanttTaskFromBar(row.task)}
                                >
                                  <span class="truncate">{row.task.title}</span>
                                </button>
                                <button
                                  type="button"
                                  class="absolute top-1/2 z-20 h-6 w-3 -translate-x-1/2 -translate-y-1/2 rounded border border-background/80 bg-foreground/20 hover:bg-foreground/35 disabled:cursor-not-allowed disabled:opacity-40"
                                  style={ganttResizeHandleStyle(row, "end")}
                                  disabled={Boolean(row.task.archivedAt) || ganttDatePendingTaskId === row.task.id}
                                  aria-label={t("projects.actions.resizeGanttTaskEnd", row.task.title)}
                                  title={t("projects.actions.resizeGanttTaskEnd", row.task.title)}
                                  onpointerdown={(event) => startGanttDateDrag(event, row, "resize-end")}
                                  onpointermove={updateGanttDateDrag}
                                  onpointerup={(event) => { void commitGanttDateDrag(event, row); }}
                                  onpointercancel={cancelGanttDateDrag}
                                  onclick={(event) => event.stopPropagation()}
                                ></button>
                              {/if}
                            </div>
                          </div>
                          {#each ganttSubtaskRows(row.task) as subtaskRow (subtaskRow.task.id)}
                            {@const subtaskDependencyLabel = ganttDependencyLabel(subtaskRow)}
                            {@const subtaskDependencyConflicts = ganttConflictsForTask(subtaskRow.task.id)}
                            <div class="project-gantt-row grid min-h-10 grid-cols-[minmax(12rem,16rem)_minmax(34rem,1fr)] border-b border-border/50 bg-muted/10">
                              <button
                                type="button"
                                class="grid min-w-0 content-center gap-0.5 border-r border-border px-2 pl-6 text-left hover:bg-accent"
                                aria-label={t("projects.actions.openTaskDetails", subtaskRow.task.title)}
                                onclick={() => onOpenTask(subtaskRow.task)}
                              >
                                <span class="truncate text-[0.8rem]">{subtaskRow.task.title}</span>
                                <span class="truncate text-[0.733333rem] text-muted-foreground">
                                  {ganttDateRangeLabel(subtaskRow.startDate, subtaskRow.endDate)}
                                  {#if subtaskDependencyLabel}
                                    / {subtaskDependencyLabel}
                                  {/if}
                                  {#if subtaskDependencyConflicts.length > 0}
                                    / {t("projects.gantt.conflictCount", subtaskDependencyConflicts.length)}
                                  {/if}
                                </span>
                              </button>
                              <div class="relative bg-muted/20" data-gantt-track>
                                {#each ganttTimeline.ticks as tick (tick.date)}
                                  <div class="absolute inset-y-0 border-l border-border/50" style={ganttTickStyle(tick)}></div>
                                {/each}
                                {#if ganttTimeline.todayPercent !== undefined}
                                  <div class="absolute inset-y-0 border-l border-primary/60" style={percentStyle(ganttTimeline.todayPercent)}></div>
                                {/if}
                                {#each ganttEdgesFrom(subtaskRow.task.id) as edge (edge.id)}
                                  <button
                                    type="button"
                                    class="absolute top-1 z-10 flex h-4 min-w-2 items-center overflow-hidden rounded text-[0.666667rem]"
                                    style={ganttEdgeStyle(edge)}
                                    aria-label={ganttEdgeTitle(edge)}
                                    title={ganttEdgeTitle(edge)}
                                    onclick={() => openGanttDependencyTarget(edge)}
                                  >
                                    <span class={cn("h-px min-w-2 flex-1", edge.violated ? "bg-destructive" : "bg-muted-foreground/60")}></span>
                                    <ArrowRight
                                      size={10}
                                      strokeWidth={2}
                                      class={edge.violated ? "shrink-0 text-destructive" : "shrink-0 text-muted-foreground"}
                                    />
                                  </button>
                                {/each}
                                {#if subtaskRow.milestone}
                                  <button
                                    type="button"
                                    class={cn(
                                      "absolute top-1/2 h-4 w-4 -translate-x-1/2 -translate-y-1/2 rotate-45 cursor-grab rounded-sm border shadow-sm active:cursor-grabbing disabled:cursor-not-allowed",
                                      ganttDateControlClass(subtaskRow),
                                    )}
                                    style={percentStyle(subtaskRow.markerPercent)}
                                    aria-label={t("projects.actions.openTaskDetails", subtaskRow.task.title)}
                                    title={`${subtaskRow.task.title} / ${subtaskRow.startDate}`}
                                    disabled={Boolean(subtaskRow.task.archivedAt) || ganttDatePendingTaskId === subtaskRow.task.id}
                                    onpointerdown={(event) => startGanttDateDrag(event, subtaskRow, "move")}
                                    onpointermove={updateGanttDateDrag}
                                    onpointerup={(event) => { void commitGanttDateDrag(event, subtaskRow); }}
                                    onpointercancel={cancelGanttDateDrag}
                                    onclick={() => openGanttTaskFromBar(subtaskRow.task)}
                                  ></button>
                                {:else}
                                  <button
                                    type="button"
                                    class="absolute top-1/2 z-20 h-6 w-3 -translate-x-1/2 -translate-y-1/2 rounded border border-background/80 bg-foreground/20 hover:bg-foreground/35 disabled:cursor-not-allowed disabled:opacity-40"
                                    style={ganttResizeHandleStyle(subtaskRow, "start")}
                                    disabled={Boolean(subtaskRow.task.archivedAt) || ganttDatePendingTaskId === subtaskRow.task.id}
                                    aria-label={t("projects.actions.resizeGanttTaskStart", subtaskRow.task.title)}
                                    title={t("projects.actions.resizeGanttTaskStart", subtaskRow.task.title)}
                                    onpointerdown={(event) => startGanttDateDrag(event, subtaskRow, "resize-start")}
                                    onpointermove={updateGanttDateDrag}
                                    onpointerup={(event) => { void commitGanttDateDrag(event, subtaskRow); }}
                                    onpointercancel={cancelGanttDateDrag}
                                    onclick={(event) => event.stopPropagation()}
                                  ></button>
                                  <button
                                    type="button"
                                    class={cn(
                                      "absolute top-1/2 flex h-5 -translate-y-1/2 cursor-grab items-center rounded border px-1.5 text-left text-[0.733333rem] shadow-sm active:cursor-grabbing disabled:cursor-not-allowed",
                                      ganttDateControlClass(subtaskRow),
                                    )}
                                    style={ganttBarStyle(subtaskRow)}
                                    aria-label={t("projects.actions.openTaskDetails", subtaskRow.task.title)}
                                    title={`${subtaskRow.task.title} / ${ganttDateRangeLabel(subtaskRow.startDate, subtaskRow.endDate)}`}
                                    disabled={Boolean(subtaskRow.task.archivedAt) || ganttDatePendingTaskId === subtaskRow.task.id}
                                    onpointerdown={(event) => startGanttDateDrag(event, subtaskRow, "move")}
                                    onpointermove={updateGanttDateDrag}
                                    onpointerup={(event) => { void commitGanttDateDrag(event, subtaskRow); }}
                                    onpointercancel={cancelGanttDateDrag}
                                    onclick={() => openGanttTaskFromBar(subtaskRow.task)}
                                  >
                                    <span class="truncate">{subtaskRow.task.title}</span>
                                  </button>
                                  <button
                                    type="button"
                                    class="absolute top-1/2 z-20 h-6 w-3 -translate-x-1/2 -translate-y-1/2 rounded border border-background/80 bg-foreground/20 hover:bg-foreground/35 disabled:cursor-not-allowed disabled:opacity-40"
                                    style={ganttResizeHandleStyle(subtaskRow, "end")}
                                    disabled={Boolean(subtaskRow.task.archivedAt) || ganttDatePendingTaskId === subtaskRow.task.id}
                                    aria-label={t("projects.actions.resizeGanttTaskEnd", subtaskRow.task.title)}
                                    title={t("projects.actions.resizeGanttTaskEnd", subtaskRow.task.title)}
                                    onpointerdown={(event) => startGanttDateDrag(event, subtaskRow, "resize-end")}
                                    onpointermove={updateGanttDateDrag}
                                    onpointerup={(event) => { void commitGanttDateDrag(event, subtaskRow); }}
                                    onpointercancel={cancelGanttDateDrag}
                                    onclick={(event) => event.stopPropagation()}
                                  ></button>
                                {/if}
                              </div>
                            </div>
                          {/each}
                        {/each}
                      {/if}
                    {/if}
                  {/each}
                </div>
              </div>
            {:else}
              <div class="rounded-md border border-dashed border-border px-2 py-3 text-[0.8rem] text-muted-foreground">
                {t("projects.gantt.noDatedTasks")}
              </div>
            {/if}
          </div>

<style>
  .project-gantt-row {
    content-visibility: auto;
    contain-intrinsic-block-size: 44px;
  }
</style>
