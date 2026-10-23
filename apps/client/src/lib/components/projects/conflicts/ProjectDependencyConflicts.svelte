<script lang="ts">
  import {
    ProjectConflictError,
    type ProjectDependencyChoice,
    type ProjectDependencyConflict,
  } from "$lib/api/project-conflicts";
  import SyncConflictCard from "$lib/components/sync/SyncConflictCard.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { canRestoreDependency, dependencyConflictsForTask } from "$lib/projects/conflicts";
  import { projectConflicts } from "$lib/stores/project-conflicts.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { formatSyncTime, syncDeviceName } from "$lib/sync/status";

  let {
    projectId,
    taskId,
    class: className,
  }: {
    projectId: string | null | undefined;
    taskId: string;
    class?: string;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const projects = useProjects();
  const hidden = $derived(dependencyConflictsForTask(projectConflicts.read(projectId), taskId));
  let busyKey = $state<string | null>(null);
  let message = $state("");
  let messageKind = $state<"resolved" | "error">("error");

  $effect(() => {
    void projectConflicts.revision;
    if (projectId) void projectConflicts.ensure(projectId);
  });

  function source(conflict: ProjectDependencyConflict): string {
    return t(
      "projects.conflict.versionFrom",
      syncDeviceName(conflict.device, t("projects.conflict.thisDevice"), t("projects.conflict.otherDevice")),
      formatSyncTime(t, localization.locale, conflict.addedAtMs, Date.now()),
    );
  }

  async function resolve(conflict: ProjectDependencyConflict, choice: ProjectDependencyChoice): Promise<void> {
    if (busyKey !== null) return;
    busyKey = conflict.key;
    message = "";
    try {
      await projects.resolveConflict({
        kind: "dependency",
        blockingTaskId: conflict.blockingTaskId,
        blockedTaskId: conflict.blockedTaskId,
        choice,
      });
    } catch (cause: unknown) {
      if (cause instanceof ProjectConflictError && cause.code === "resolved") {
        message = t("projects.conflict.resolved");
        messageKind = "resolved";
      } else if (cause instanceof ProjectConflictError && cause.code === "would_cycle") {
        message = t("projects.conflict.restoreCycles");
        messageKind = "error";
      } else {
        console.warn("Projects dependency conflict could not be resolved", cause);
        message = t("projects.conflict.failed");
        messageKind = "error";
      }
    } finally {
      busyKey = null;
      projectConflicts.invalidate();
    }
  }
</script>

{#if hidden.length > 0}
  <SyncConflictCard
    title={t("projects.conflict.dependencyTitle")}
    hint={t("projects.conflict.dependencyHint")}
    {message}
    {messageKind}
    class={className}
  >
    {#each hidden as conflict (conflict.key)}
      {@const restorable = canRestoreDependency(conflict)}
      <div class="flex min-w-0 flex-col gap-1 rounded-md bg-black/5 px-2 py-1.5 dark:bg-white/5">
        <div class="wrap-anywhere">
          {t("projects.conflict.dependencyLine", conflict.blockingTaskTitle, conflict.blockedTaskTitle)}
        </div>
        <div class="text-muted-foreground">{source(conflict)}</div>
        {#if !restorable}
          <div class="text-muted-foreground">{t("projects.conflict.restoreCycles")}</div>
        {/if}
        <div class="flex flex-wrap items-center gap-1">
          <button
            class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
            type="button"
            disabled={busyKey !== null}
            onclick={() => void resolve(conflict, "keep_removed")}
          >{t("projects.conflict.keepRemoved")}</button>
          <button
            class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
            type="button"
            disabled={busyKey !== null || !restorable}
            onclick={() => void resolve(conflict, "restore")}
          >{t("projects.conflict.restore")}</button>
        </div>
      </div>
    {/each}
  </SyncConflictCard>
{/if}
