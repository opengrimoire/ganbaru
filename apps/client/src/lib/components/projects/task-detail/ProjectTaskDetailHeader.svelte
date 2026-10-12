<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectTaskArchivedBadgeClass } from "$lib/projects/display";
  import type { ProjectTask } from "$lib/projects/types";
  import { cn } from "$lib/utils";

  let {
    task,
    projectName,
    title,
    onTitleChange,
    onClose,
  }: {
    task: ProjectTask | null;
    projectName: string;
    title: string;
    onTitleChange: (value: string) => void;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<header class="flex shrink-0 items-start gap-4 border-b border-border bg-card px-8 py-5">
  <div class="min-w-0 flex-1">
    <div class="mb-1 px-1 text-[0.733333rem] text-muted-foreground">{projectName}</div>
    <div class="flex min-w-0 items-center gap-2">
      <input
        value={title}
        aria-label={t("projects.detail.titleLabel")}
        placeholder={t("projects.detail.titlePlaceholder")}
        class="min-h-10 min-w-0 flex-1 rounded-md bg-transparent px-1 text-xl font-semibold tracking-tight text-foreground"
        oninput={(event) => onTitleChange(event.currentTarget.value)}
      />
      {#if task?.archivedAt}
        <span class={cn("shrink-0 rounded border px-1.5 py-0.5 text-[0.666667rem]", projectTaskArchivedBadgeClass(task))}>
          {t("projects.taskLifecycle.archived")}
        </span>
      {/if}
    </div>
  </div>
  <button
    type="button"
    class="flex size-9 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
    aria-label={t("projects.detail.close")}
    title={t("projects.detail.close")}
    onclick={onClose}
  >
    <X size={15} strokeWidth={1.75} />
  </button>
</header>
