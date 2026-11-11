<script lang="ts">
  import { onMount, tick } from "svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { ROUTINE_GROUP_ID, systemProjectGroupName } from "$lib/projects/system-defaults";
  import { normalizeProjectName } from "$lib/projects/text";
  import type { ProjectGroup } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";

  /**
   * Visible project groups in sidebar order, each renamed in place or deleted after a confirmation that names how many
   * projects move outside every group. The built-in Routine group keeps its name and cannot be deleted.
   */
  const { t } = getLocalization();
  const projects = getProjects();

  const groups = $derived(projects.visibleGroups());

  let renamingGroupId = $state<string | null>(null);
  let nameDraft = $state("");
  let nameInput = $state<HTMLInputElement | null>(null);
  let deleteCandidate = $state<ProjectGroup | null>(null);
  let pendingGroupId = $state<string | null>(null);
  let actionError = $state<string | null>(null);

  function displayName(group: ProjectGroup): string {
    return systemProjectGroupName(group.id, group.name, t);
  }

  /** Counts every project of the group, including hidden and archived ones, since all of them leave the group. */
  function projectCount(group: ProjectGroup): number {
    return projects.projectsForGroupIncludingInactive(group.id).length;
  }

  function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  function beginRename(group: ProjectGroup): void {
    renamingGroupId = group.id;
    nameDraft = group.name;
    actionError = null;
    void tick().then(() => {
      nameInput?.focus();
      nameInput?.select();
    });
  }

  function cancelRename(): void {
    renamingGroupId = null;
  }

  /** Saves a changed, non-empty name; an empty or unchanged draft keeps the current name. */
  async function commitRename(group: ProjectGroup): Promise<void> {
    if (renamingGroupId !== group.id) return;
    renamingGroupId = null;
    const name = normalizeProjectName(nameDraft);
    if (!name || name === group.name) return;
    pendingGroupId = group.id;
    try {
      await projects.updateGroup(group, { name });
    } catch (error: unknown) {
      actionError = t("projects.navigator.groupSaveFailed", errorMessage(error));
    } finally {
      pendingGroupId = null;
    }
  }

  function handleNameKeydown(event: KeyboardEvent, group: ProjectGroup): void {
    if (event.key === "Enter") {
      event.preventDefault();
      void commitRename(group);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancelRename();
    }
  }

  async function confirmDelete(): Promise<void> {
    const group = deleteCandidate;
    deleteCandidate = null;
    if (!group) return;
    pendingGroupId = group.id;
    actionError = null;
    try {
      await projects.removeGroup(group);
    } catch (error: unknown) {
      actionError = t("projects.navigator.groupDeleteFailed", errorMessage(error));
    } finally {
      pendingGroupId = null;
    }
  }

  onMount(() => {
    void projects.ensureLoaded();
  });
</script>

<section class="flex flex-col gap-4">
  <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.projectsGroups.heading")}</h2>
  {#if actionError}
    <Notice class="px-1 text-[0.8rem]" message={actionError} onDismiss={() => { actionError = null; }} />
  {/if}
  {#if !projects.loaded}
    {#if projects.loading}<Loading layout="rows" rows={2} label={t("projects.loading")} />{/if}
  {:else if groups.length === 0}
    <p class="px-1 text-[0.8rem] text-muted-foreground">{t("settings.projectsGroups.empty")}</p>
  {:else}
    <ul class="flex flex-col gap-3">
      {#each groups as group (group.id)}
        {@const builtIn = group.id === ROUTINE_GROUP_ID}
        {@const count = projectCount(group)}
        <li class="flex items-center justify-between gap-4 px-1 py-1">
          <div class="min-w-0 flex-1">
            {#if renamingGroupId === group.id}
              <input
                bind:this={nameInput}
                bind:value={nameDraft}
                class="field w-full px-2 py-1 text-[0.866667rem] text-foreground"
                aria-label={t("projects.navigator.groupNamePlaceholder")}
                placeholder={t("projects.navigator.groupNamePlaceholder")}
                onkeydown={(event) => handleNameKeydown(event, group)}
                onblur={() => void commitRename(group)}
              />
            {:else}
              <div class="truncate text-[0.866667rem] text-foreground">{displayName(group)}</div>
              <div class="mt-0.5 truncate text-[0.8rem] text-muted-foreground">
                {builtIn ? t("settings.projectsGroups.builtIn") : t("settings.projectsGroups.projectCount", count)}
              </div>
            {/if}
          </div>
          {#if !builtIn && renamingGroupId !== group.id}
            <div class="flex shrink-0 items-center gap-2">
              <button
                type="button"
                class="inline-flex h-(--control-height) items-center justify-center rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-wait disabled:opacity-50 dark:bg-transparent"
                disabled={pendingGroupId === group.id}
                aria-label={t("settings.projectsGroups.renameGroup", displayName(group))}
                onclick={() => beginRename(group)}
              >
                {t("projects.sidebar.rename")}
              </button>
              <button
                type="button"
                class="inline-flex h-(--control-height) items-center justify-center rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-destructive transition-colors hover:bg-accent disabled:cursor-wait disabled:opacity-50 dark:bg-transparent"
                disabled={pendingGroupId === group.id}
                aria-label={t("projects.navigator.deleteGroup", displayName(group))}
                onclick={() => { deleteCandidate = group; }}
              >
                {t("common.delete")}
              </button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if deleteCandidate}
  <ConfirmDialog
    title={t("projects.navigator.deleteGroupTitle", displayName(deleteCandidate))}
    message={t("projects.navigator.deleteGroupMessage", displayName(deleteCandidate), projectCount(deleteCandidate))}
    confirmLabel={t("projects.navigator.deleteGroupConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void confirmDelete()}
    onCancel={() => { deleteCandidate = null; }}
  />
{/if}
