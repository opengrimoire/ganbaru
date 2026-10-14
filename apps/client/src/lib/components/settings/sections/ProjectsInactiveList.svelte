<script lang="ts">
  import { onMount } from "svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import ProjectLifecycleIcon from "$lib/components/projects/ProjectLifecycleIcon.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { Project, ProjectGroup } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";

  /** Hidden and archived projects in sidebar order, each with the action that makes it active again. */
  const { t } = getLocalization();
  const projects = getProjects();

  interface InactiveProjectRow {
    project: Project;
    group: ProjectGroup;
  }

  const rows = $derived<InactiveProjectRow[]>(
    projects.visibleGroups().flatMap((group) => projects.projectsForGroupIncludingInactive(group.id)
      .filter((project) => project.status !== "active")
      .map((project) => ({ project, group }))),
  );
  let pendingProjectId = $state<string | null>(null);
  let actionError = $state<string | null>(null);

  async function activate(project: Project): Promise<void> {
    pendingProjectId = project.id;
    actionError = null;
    try {
      await projects.setProjectStatus(project, "active");
    } catch (error) {
      actionError = t("settings.projectsInactive.activateFailed", project.name, error instanceof Error ? error.message : String(error));
    } finally {
      pendingProjectId = null;
    }
  }

  onMount(() => {
    void projects.ensureLoaded();
  });
</script>

<section class="flex flex-col gap-4">
  <div class="flex flex-col gap-1 px-1">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.projectsInactive.heading")}</h2>
    <p class="text-[0.8rem] text-muted-foreground">{t("settings.projectsInactive.description")}</p>
  </div>
  {#if actionError}
    <Notice class="px-1 text-[0.8rem]" message={actionError} onDismiss={() => { actionError = null; }} />
  {/if}
  {#if projects.loading && !projects.loaded}
    <Loading layout="rows" rows={2} label={t("projects.loading")} />
  {:else if projects.loadError}
    <Notice class="px-1 text-[0.8rem]" message={t("projects.loadFailed", projects.loadError)} onRetry={() => projects.ensureLoaded()} />
  {:else if rows.length === 0}
    <p class="px-1 text-[0.8rem] text-muted-foreground">{t("settings.projectsInactive.empty")}</p>
  {:else}
    <ul class="flex flex-col">
      {#each rows as { project, group } (project.id)}
        <li class="inactive-project-row">
          <ProjectIcon name={project.icon} size={15} />
          <span class="inactive-project-name">{project.name}</span>
          <span class="inactive-project-group">{group.name}</span>
          <ProjectLifecycleIcon status={project.status} />
          <button
            type="button"
            class="inactive-project-action"
            disabled={pendingProjectId === project.id}
            aria-label={t(project.status === "hidden" ? "settings.projectsInactive.showProject" : "settings.projectsInactive.restoreProject", project.name)}
            onclick={() => void activate(project)}
          >
            {project.status === "hidden" ? t("projects.sidebar.show") : t("projects.sidebar.restore")}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .inactive-project-row { display: flex; min-height: 2.25rem; align-items: center; gap: 0.6rem; padding-inline: 0.375rem; }
  .inactive-project-name { min-width: 0; flex: 0 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .inactive-project-group { min-width: 0; flex: 1; overflow: hidden; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); text-overflow: ellipsis; white-space: nowrap; }
  .inactive-project-action { display: inline-flex; height: 1.625rem; flex: 0 0 auto; align-items: center; border-radius: 0.375rem; padding-inline: 0.5rem; color: var(--muted-foreground); font-size: calc(0.75rem * var(--type-scale)); font-weight: 500; white-space: nowrap; transition: background-color 120ms ease, color 120ms ease; }
  .inactive-project-action:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .inactive-project-action:disabled { opacity: 0.6; }
</style>
