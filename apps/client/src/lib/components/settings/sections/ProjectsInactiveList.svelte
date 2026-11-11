<script lang="ts">
  import { onMount } from "svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectLifecycleLabel } from "$lib/projects/display";
  import type { Project, ProjectGroup } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";

  /** Hidden and archived projects in sidebar order, each with the action that makes it active again. */
  const { t } = getLocalization();
  const projects = getProjects();

  interface InactiveProjectRow {
    project: Project;
    /** The project's group, or `null` for a project outside every group. */
    group: ProjectGroup | null;
  }

  const rows = $derived<InactiveProjectRow[]>(
    projects.projectLists(true).flatMap(({ group, projects: listProjects }) => listProjects
      .filter((project) => project.status !== "active")
      .map((project) => ({ project, group }))),
  );

  function rowDetail({ project, group }: InactiveProjectRow): string {
    const status = projectLifecycleLabel(project.status, t);
    return group ? t("settings.projectsInactive.detail", group.name, status) : status;
  }
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
  <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.projectsInactive.heading")}</h2>
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
    <ul class="flex flex-col gap-3">
      {#each rows as row (row.project.id)}
        {@const project = row.project}
        <li class="flex items-center justify-between gap-4 px-1 py-1">
          <div class="min-w-0 flex-1">
            <div class="truncate text-[0.866667rem] text-foreground">{project.name}</div>
            <div class="mt-0.5 truncate text-[0.8rem] text-muted-foreground">
              {rowDetail(row)}
            </div>
          </div>
          <button
            type="button"
            class="inline-flex h-(--control-height) shrink-0 items-center justify-center rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-wait disabled:opacity-50 dark:bg-transparent"
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
