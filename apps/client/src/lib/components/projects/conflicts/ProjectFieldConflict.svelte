<script lang="ts">
  import {
    ProjectConflictError,
    type ProjectConflictField,
    type ProjectConflictTarget,
    type ProjectConflictVersion,
    type ProjectFieldChoice,
  } from "$lib/api/project-conflicts";
  import SyncConflictCard from "$lib/components/sync/SyncConflictCard.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { findFieldConflict, previewVersionText } from "$lib/projects/conflicts";
  import { projectConflicts } from "$lib/stores/project-conflicts.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { formatSyncTime, syncDeviceName } from "$lib/sync/status";

  let {
    projectId,
    target,
    id,
    field,
    onResolved,
    class: className,
  }: {
    projectId: string | null | undefined;
    target: ProjectConflictTarget;
    id: string;
    field: ProjectConflictField;
    /** Receives the text of another version once it is chosen, so a view holding an edit draft can show it. */
    onResolved?: (text: string) => void;
    class?: string;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const projects = useProjects();
  const conflict = $derived(findFieldConflict(projectConflicts.read(projectId), target, id, field));
  const title = $derived.by(() => {
    if (target === "project") return t("projects.conflict.nameTitle");
    if (target === "checklist_item") return t("projects.conflict.checklistTitle");
    return field === "description" ? t("projects.conflict.descriptionTitle") : t("projects.conflict.titleTitle");
  });
  let busy = $state(false);
  let message = $state("");
  let messageKind = $state<"resolved" | "error">("error");

  $effect(() => {
    void projectConflicts.revision;
    if (projectId) void projectConflicts.ensure(projectId);
  });

  function source(version: ProjectConflictVersion): string {
    return t(
      "projects.conflict.versionFrom",
      syncDeviceName(version.device, t("projects.conflict.thisDevice"), t("projects.conflict.otherDevice")),
      formatSyncTime(t, localization.locale, version.editedAtMs, Date.now()),
    );
  }

  async function resolve(choice: ProjectFieldChoice, text: string): Promise<void> {
    if (busy) return;
    busy = true;
    message = "";
    try {
      await projects.resolveConflict({ kind: "field", target, id, field, choice });
      if (choice.kind === "version") onResolved?.(text);
    } catch (cause: unknown) {
      if (cause instanceof ProjectConflictError && cause.code === "resolved") {
        message = t("projects.conflict.resolved");
        messageKind = "resolved";
      } else {
        console.warn("Projects conflict could not be resolved", cause);
        message = t("projects.conflict.failed");
        messageKind = "error";
      }
    } finally {
      busy = false;
      projectConflicts.invalidate();
    }
  }
</script>

{#if conflict}
  <SyncConflictCard {title} hint={t("projects.conflict.hint")} {message} {messageKind} class={className}>
    {#each conflict.versions as version (version.version)}
      <div class="flex min-w-0 flex-col gap-1 rounded-md bg-black/5 px-2 py-1.5 dark:bg-white/5">
        <div class="max-h-32 overflow-y-auto whitespace-pre-wrap wrap-anywhere">
          {#if version.text.trim()}
            {previewVersionText(version.text)}
          {:else}
            <span class="text-muted-foreground">{t("projects.conflict.empty")}</span>
          {/if}
        </div>
        <div class="flex flex-wrap items-baseline gap-x-2 text-muted-foreground">
          <span>{source(version)}</span>
          {#if version.displayed}<span>{t("projects.conflict.shown")}</span>{/if}
        </div>
        <div class="flex flex-wrap items-center gap-1">
          {#if version.displayed}
            <button
              class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
              type="button"
              disabled={busy}
              onclick={() => void resolve({ kind: "displayed" }, version.text)}
            >{t("projects.conflict.keepThis")}</button>
          {:else}
            <button
              class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
              type="button"
              disabled={busy}
              onclick={() => void resolve({ kind: "version", version: version.version }, version.text)}
            >{t("projects.conflict.useOther")}</button>
          {/if}
        </div>
      </div>
    {/each}
  </SyncConflictCard>
{/if}
