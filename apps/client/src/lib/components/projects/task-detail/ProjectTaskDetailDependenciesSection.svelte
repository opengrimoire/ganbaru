<script lang="ts">
  import type { MaybePromise } from "$lib/utils";
  import Select from "$lib/components/ui/Select.svelte";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectTask, ProjectTaskDependency } from "$lib/projects/types";
  import ProjectSettingsSectionHeading from "$lib/components/projects/settings/ProjectSettingsSectionHeading.svelte";


  let {
    task,
    blockedByDependencies,
    blocksDependencies,
    dependencyCandidates,
    dependencySearch,
    taskById,
    onDependencySearchChange,
    onAddBlockingDependency,
    onRemoveDependency,
  }: {
    task: ProjectTask;
    blockedByDependencies: ProjectTaskDependency[];
    blocksDependencies: ProjectTaskDependency[];
    dependencyCandidates: ProjectTask[];
    dependencySearch: string;
    taskById: (taskId: string) => ProjectTask | undefined;
    onDependencySearchChange: (value: string) => void;
    onAddBlockingDependency: (blockingTask: ProjectTask, blockedTask: ProjectTask) => MaybePromise;
    onRemoveDependency: (dependencyId: string) => MaybePromise;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="task-detail-section grid min-w-0 content-start gap-3">
  <ProjectSettingsSectionHeading label={t("projects.detail.dependencies")} count={blockedByDependencies.length + blocksDependencies.length} inlineCount />
  <div class="grid gap-3 sm:grid-cols-2">
  <div class="grid gap-1">
    <div class="text-[0.733333rem] font-medium text-muted-foreground">{t("projects.detail.blockedBy")}</div>
    {#each blockedByDependencies as dependency (dependency.id)}
      {@const blockingTask = taskById(dependency.blockingTaskId)}
      <div class="grid min-h-8 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-md bg-muted/30 px-2">
        <span class="truncate text-[0.8rem]">
          {blockingTask?.title ?? t("projects.detail.missingDependencyTask")}
        </span>
        <button
          type="button"
          class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
          aria-label={t("projects.actions.deleteDependency")}
          title={t("projects.actions.deleteDependency")}
          onclick={() => { void onRemoveDependency(dependency.id); }}
        >
          <Trash2 size={13} strokeWidth={1.75} />
        </button>
      </div>
    {:else}
      <div class="px-1 py-1 text-[0.8rem] text-muted-foreground">
        {t("projects.detail.noBlockedBy")}
      </div>
    {/each}
  </div>

  <div class="grid gap-1">
    <div class="text-[0.733333rem] font-medium text-muted-foreground">{t("projects.detail.blocks")}</div>
    {#each blocksDependencies as dependency (dependency.id)}
      {@const blockedTask = taskById(dependency.blockedTaskId)}
      <div class="grid min-h-8 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-md bg-muted/30 px-2">
        <span class="truncate text-[0.8rem]">
          {blockedTask?.title ?? t("projects.detail.missingDependencyTask")}
        </span>
        <button
          type="button"
          class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
          aria-label={t("projects.actions.deleteDependency")}
          title={t("projects.actions.deleteDependency")}
          onclick={() => { void onRemoveDependency(dependency.id); }}
        >
          <Trash2 size={13} strokeWidth={1.75} />
        </button>
      </div>
    {:else}
      <div class="px-1 py-1 text-[0.8rem] text-muted-foreground">
        {t("projects.detail.noBlocks")}
      </div>
    {/each}
  </div>

  </div>
  <Select inline appearance="quiet" contentAlign="start" class="w-full" value=""
    triggerLabel={t("projects.detail.addBlockedBy")} ariaLabel={t("projects.detail.addBlockedBy")}
    options={dependencyCandidates.map((candidate) => ({ value: candidate.id, label: candidate.title }))}
    searchPlaceholder={t("projects.detail.addBlockedByPlaceholder")}
    searchValue={dependencySearch} onSearchChange={onDependencySearchChange}
    emptyLabel={t("projects.detail.noDependencyCandidates")}
    onChange={(value) => {
      const candidate = dependencyCandidates.find((entry) => entry.id === value);
      if (candidate) void onAddBlockingDependency(candidate, task);
    }} />
</section>
