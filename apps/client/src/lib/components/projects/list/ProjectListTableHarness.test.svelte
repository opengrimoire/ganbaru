<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import type { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";
  import ProjectListColumnHeaders from "./ProjectListColumnHeaders.svelte";
  import ProjectListCalculationFooter from "./ProjectListCalculationFooter.svelte";
  import { createProjectListColumnReorder } from "./column-reorder";
  import { setProjectListTableContext } from "./table-context";

  let { query }: { query: ProjectTaskQueryController } = $props();
  const columnReorder = createProjectListColumnReorder(untrack(() => query));
  onDestroy(() => columnReorder.destroy());
  setProjectListTableContext({ get query() { return query; }, cellClass: () => "", cellStyle: () => "", rowStyle: () => "", columnReorder });
</script>

<div data-floating-root>
  <ProjectListColumnHeaders mode="group" rowKey="group:all" gridTemplate="1.5rem 1.75rem 24rem 8rem 8rem 2.25rem" gridMinWidth="45.5rem"
    taskListColumns={query.listColumns} taskListColumnLabel={(column) => query.columnLabel(column)} onResizePointerDown={() => undefined} onResizeDoubleClick={() => undefined} onResizeKeydown={() => undefined} />
  <ProjectListCalculationFooter {query} gridTemplate="1.5rem 1.75rem 24rem 8rem 8rem 2.25rem" gridMinWidth="45.5rem" columns={query.listColumns} columnLabel={(column) => query.columnLabel(column)} />
  {#if query.presentationError}<p role="alert">{query.presentationError}</p>{/if}
  {#if query.propertyError}<p role="alert">{query.propertyError}</p>{/if}
  {#if query.listColumnsError}<p role="alert">{query.listColumnsError}</p>{/if}
</div>
