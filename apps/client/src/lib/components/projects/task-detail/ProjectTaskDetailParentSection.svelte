<script lang="ts">
  import type { MaybePromise } from "$lib/utils";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectTask } from "$lib/projects/types";
  import ProjectSettingsSectionHeading from "$lib/components/projects/settings/ProjectSettingsSectionHeading.svelte";


  let {
    task,
    parentTask,
    parentCandidates,
    parentSearch,
    hasSubtasks,
    onParentSearchChange,
    onPromoteSubtask,
    onDemoteTask,
  }: {
    task: ProjectTask;
    parentTask: ProjectTask | undefined;
    parentCandidates: ProjectTask[];
    parentSearch: string;
    hasSubtasks: boolean;
    onParentSearchChange: (value: string) => void;
    onPromoteSubtask: (task: ProjectTask) => MaybePromise;
    onDemoteTask: (task: ProjectTask, parentTask: ProjectTask) => MaybePromise;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="task-detail-section grid min-w-0 content-start gap-3">
  <ProjectSettingsSectionHeading label={t("projects.detail.parentTask")} count={task.parentTaskId ? 1 : 0} inlineCount />
  {#if task.parentTaskId}
    <div class="flex items-center gap-2">
      <div class="min-w-0 flex-1 rounded-md border border-border bg-background px-2 py-1.5 text-[0.8rem]">
        {parentTask?.title ?? t("projects.detail.missingDependencyTask")}
      </div>
      <button
        type="button"
        class="flex min-h-7 shrink-0 items-center gap-1 rounded-md border border-border bg-background px-2 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground"
        onclick={() => { void onPromoteSubtask(task); }}
      >
        <ArrowLeft size={13} strokeWidth={1.75} />
        <span>{t("projects.detail.promoteSubtask")}</span>
      </button>
    </div>
  {:else if hasSubtasks}
    <div class="px-1 py-1 text-[0.8rem] text-muted-foreground">
      {t("projects.detail.demoteBlockedBySubtasks")}
    </div>
  {:else}
    <Select inline appearance="quiet" contentAlign="start" class="w-full"
      value="" triggerLabel={t("projects.detail.demoteToParent")}
      ariaLabel={t("projects.detail.parentTask")}
      options={parentCandidates.map((candidate) => ({ value: candidate.id, label: candidate.title }))}
      searchPlaceholder={t("projects.detail.demoteToParentPlaceholder")}
      searchValue={parentSearch} onSearchChange={onParentSearchChange}
      emptyLabel={t("projects.detail.noParentCandidates")}
      onChange={(value) => {
        const parent = parentCandidates.find((candidate) => candidate.id === value);
        if (parent) void onDemoteTask(task, parent);
      }} />
  {/if}
</section>
