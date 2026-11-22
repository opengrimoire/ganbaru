<script lang="ts">
  import { getProjects } from "$lib/stores/projects.svelte";
  import type { Project } from "$lib/projects/types";

  let { onOpenProjectSettings }: { onOpenProjectSettings: (project: Project) => void } = $props();

  const projects = getProjects();

  /** Open settings for a project from its sidebar menu, as the real sidebar does. */
  function openSettings(projectId: string): void {
    const project = projects.projects.find((item) => item.id === projectId);
    if (project) onOpenProjectSettings(project);
  }
</script>

<button type="button" data-open-settings="project-2" onclick={() => openSettings("project-2")}>Settings</button>
<button type="button" data-select-project="project-3" onclick={() => { void projects.selectProject("project-3"); }}>Select</button>
