<script lang="ts">
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MoveDirection, ProjectStatus, ProjectTask } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import type { MaybePromise } from "$lib/utils";
  import ProjectStatusBadge from "$lib/components/projects/ProjectStatusBadge.svelte";
  import ProjectSettingsSectionHeading from "$lib/components/projects/settings/ProjectSettingsSectionHeading.svelte";


  let {
    task,
    subtasks,
    theme,
    draft,
    statusForTask,
    onDraftChange,
    onSubmitSubtask,
    onToggleComplete,
    onOpenTask,
    onPromoteSubtask,
    onMoveSubtask,
    onRemoveSubtask,
  }: {
    task: ProjectTask;
    subtasks: ProjectTask[];
    theme: Theme;
    draft: string;
    statusForTask: (task: ProjectTask) => ProjectStatus | undefined;
    onDraftChange: (value: string) => void;
    onSubmitSubtask: (task: ProjectTask) => MaybePromise;
    onToggleComplete: (task: ProjectTask) => MaybePromise;
    onOpenTask: (task: ProjectTask) => void;
    onPromoteSubtask: (task: ProjectTask) => MaybePromise;
    onMoveSubtask: (task: ProjectTask, direction: MoveDirection) => MaybePromise;
    /**
     * Removes an unsaved subtask. When given, rows are not openable and offer
     * removal instead of promotion, as in a task draft.
     */
    onRemoveSubtask?: (task: ProjectTask) => void;
  } = $props();

  const { t } = getLocalization();

  function adjacentSubtask(subtask: ProjectTask, direction: MoveDirection): ProjectTask | undefined {
    const index = subtasks.findIndex((entry) => entry.id === subtask.id);
    if (index < 0) return undefined;
    return subtasks[index + direction];
  }
</script>

<section class="task-detail-section grid min-w-0 content-start gap-3">
  <ProjectSettingsSectionHeading label={t("projects.detail.subtasks")} count={subtasks.length} inlineCount />
  <div class="grid gap-1">
    {#each subtasks as subtask (subtask.id)}
      {@const subtaskStatus = statusForTask(subtask)}
      {@const previousSubtask = adjacentSubtask(subtask, -1)}
      {@const nextSubtask = adjacentSubtask(subtask, 1)}
      <div class="grid min-h-8 grid-cols-[auto_minmax(0,1fr)_auto_auto_auto_auto] items-center gap-1 rounded-md bg-transparent px-2 hover:bg-muted/40">
        <Checkbox
          checked={Boolean(subtaskStatus?.terminal)}
          label={t("projects.actions.toggleComplete")}
          onChange={() => { void onToggleComplete(subtask); }}
        />
        {#if onRemoveSubtask}
          <span class="block min-w-0 truncate text-[0.8rem]">{subtask.title}</span>
        {:else}
          <button
            type="button"
            class="min-w-0 text-left"
            aria-label={t("projects.actions.openTaskDetails", subtask.title)}
            onclick={() => onOpenTask(subtask)}
          >
            <span class="block truncate text-[0.8rem]">{subtask.title}</span>
          </button>
        {/if}
        <ProjectStatusBadge
          status={subtaskStatus}
          {theme}
          label={subtaskStatus?.name ?? t("projects.list.status")}
          class="text-[0.733333rem]"
        />
        {#if onRemoveSubtask}
          <button
            type="button"
            class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
            aria-label={t("projects.actions.removeSubtask", subtask.title)}
            title={t("projects.actions.removeSubtask", subtask.title)}
            onclick={() => onRemoveSubtask(subtask)}
          >
            <X size={13} strokeWidth={1.75} />
          </button>
        {:else}
          <button
            type="button"
            class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
            disabled={Boolean(subtask.archivedAt)}
            aria-label={t("projects.actions.promoteSubtask", subtask.title)}
            title={t("projects.actions.promoteSubtask", subtask.title)}
            onclick={() => { void onPromoteSubtask(subtask); }}
          >
            <ArrowLeft size={13} strokeWidth={1.75} />
          </button>
        {/if}
        <button
          type="button"
          class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          disabled={!previousSubtask}
          aria-label={previousSubtask ? t("projects.actions.moveSubtaskUp", subtask.title) : t("projects.actions.noPreviousSubtask")}
          title={previousSubtask ? t("projects.actions.moveSubtaskUp", subtask.title) : t("projects.actions.noPreviousSubtask")}
          onclick={() => { void onMoveSubtask(subtask, -1); }}
        >
          <ArrowUp size={13} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          disabled={!nextSubtask}
          aria-label={nextSubtask ? t("projects.actions.moveSubtaskDown", subtask.title) : t("projects.actions.noNextSubtask")}
          title={nextSubtask ? t("projects.actions.moveSubtaskDown", subtask.title) : t("projects.actions.noNextSubtask")}
          onclick={() => { void onMoveSubtask(subtask, 1); }}
        >
          <ArrowDown size={13} strokeWidth={1.75} />
        </button>
      </div>
    {/each}
  </div>
  <div class="flex gap-1">
    <input
      value={draft}
      aria-label={t("projects.detail.addSubtask")}
      placeholder={t("projects.detail.addSubtaskPlaceholder")}
      class="min-h-8 min-w-0 flex-1 rounded-md bg-transparent px-2 hover:bg-muted/40 text-[0.8rem]"
      oninput={(event) => onDraftChange(event.currentTarget.value)}
      onkeydown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          void onSubmitSubtask(task);
        }
      }}
    />
    <button
      type="button"
      class="flex min-h-8 items-center justify-center rounded-md bg-transparent px-2 text-[0.8rem] hover:bg-accent"
      aria-label={t("projects.detail.addSubtask")}
      onclick={() => { void onSubmitSubtask(task); }}
    >
      <Plus size={14} strokeWidth={1.75} />
    </button>
  </div>
</section>
