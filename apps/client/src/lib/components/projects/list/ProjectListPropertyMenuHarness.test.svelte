<script lang="ts">
  import { untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import type { ProjectCustomField, ProjectTaskListColumn } from "$lib/projects/types";
  import ProjectListPropertyMenu from "./ProjectListPropertyMenu.svelte";
  import { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";
  import { setProjectListTableContext } from "./table-context";
  import { projectTaskListGutterWidth } from "$lib/projects/list/view";

  let { initialField, column }: { initialField: ProjectCustomField; column?: ProjectTaskListColumn } = $props();
  let field = $state(untrack(() => initialField));
  type Input = ConstructorParameters<typeof ProjectTaskQueryController>[0];
  const query = new ProjectTaskQueryController({
    projects: {
      get selectedProject() { return { id: field.projectId }; },
      activeView: "list",
      get activeProjectView() { return { id: "view-list", projectId: field.projectId, viewType: "list" }; },
      customFieldsForProject: () => [field],
      prioritiesForProject: () => [],
      customFieldOptionsForField: (fieldId: string) => getProjects().customFieldOptionsForField(fieldId),
      removeCustomField: (fieldId: string) => getProjects().removeCustomField(fieldId),
      saveTaskListPresentation: async () => undefined,
    } as unknown as Input["projects"],
    calendar: {sourceEvents: []} as unknown as Input["calendar"],
    translate: getLocalization().t,
  });
  setProjectListTableContext({query, gutter: projectTaskListGutterWidth(), cellClass: () => "", cellStyle: () => "", rowStyle: () => ""});

  /** Expose the shared query so tests can observe sorting and grouping. */
  export function taskQuery(): ProjectTaskQueryController {
    return query;
  }

  /** Apply a canonical schema update while keeping the existing header menu mounted. */
  export function setField(next: ProjectCustomField): void {
    field = next;
  }
</script>

<div data-floating-root>
  <ProjectListPropertyMenu column={column ?? `custom:${field.id}`} label={column ?? field.name} />
</div>
