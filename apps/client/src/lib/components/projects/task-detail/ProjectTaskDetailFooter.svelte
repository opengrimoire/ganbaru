<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectTask } from "$lib/projects/types";
  import { cn, type MaybePromise } from "$lib/utils";


  let {
    task,
    saving,
    dirty,
    onArchive,
    onRestore,
  }: {
    task: ProjectTask | null;
    saving: boolean;
    dirty: boolean;
    onArchive: (task: ProjectTask) => MaybePromise;
    onRestore: (task: ProjectTask) => MaybePromise;
  } = $props();

  const { t } = getLocalization();
</script>

<footer class="flex shrink-0 items-center justify-end gap-2 border-t border-border bg-card px-5 py-3">
  {#if task?.archivedAt}
    <button
      type="button"
      class="mr-auto flex min-h-8 items-center gap-1.5 rounded-md border border-border bg-card px-2 text-[0.8rem] hover:bg-accent disabled:cursor-not-allowed"
      disabled={saving}
      onclick={() => { void onRestore(task); }}
    >
      <ArchiveRestore size={14} strokeWidth={1.75} />
      <span>{t("projects.detail.restore")}</span>
    </button>
  {:else if task}
    <button
      type="button"
      class="mr-auto flex min-h-8 items-center gap-1.5 rounded-md px-2 text-[0.8rem] text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed"
      disabled={saving}
      onclick={() => { void onArchive(task); }}
    >
      <Archive size={14} strokeWidth={1.75} />
      <span>{t("projects.detail.archive")}</span>
    </button>
  {/if}
  <button
    type="submit"
    class={cn(
      "flex min-h-9 items-center justify-center gap-1.5 rounded-lg bg-primary px-4 text-[0.8rem] font-medium text-primary-foreground disabled:cursor-not-allowed",
      dirty || saving ? "hover:bg-primary/90" : "opacity-60",
    )}
    disabled={saving || !dirty}
  >
    <span>{task ? t("projects.detail.save") : t("projects.detail.create")}</span>
  </button>
</footer>
