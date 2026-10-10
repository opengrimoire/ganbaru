<script lang="ts">
  import ProjectListPropertyMenu from "./ProjectListPropertyMenu.svelte";
  import ProjectListPropertyDraft from "./ProjectListPropertyDraft.svelte";
  import { getProjectListTableContext } from "./table-context";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type {
    ProjectTaskListColumn,
  } from "$lib/projects/types";
  import type { ProjectTaskListResizableColumn } from "$lib/projects/list/view";
  import CollectionRow from "$lib/components/collections/CollectionRow.svelte";
  import CollectionColumnHeader from "$lib/components/collections/CollectionColumnHeader.svelte";
  import CollectionAddPropertyButton from "$lib/components/collections/CollectionAddPropertyButton.svelte";

  type HeaderMode = "section" | "group";

  let {
    mode,
    gridTemplate,
    gridMinWidth,
    taskListColumns,
    taskListColumnLabel,
    onResizePointerDown,
    onResizeDoubleClick,
    onResizeKeydown,
  }: {
    mode: HeaderMode;
    gridTemplate: string;
    gridMinWidth: string;
    taskListColumns: ProjectTaskListColumn[];
    taskListColumnLabel: (column: ProjectTaskListColumn) => string;
    onResizePointerDown: (event: PointerEvent, column: ProjectTaskListResizableColumn) => void;
    onResizeDoubleClick: (event: MouseEvent, column: ProjectTaskListResizableColumn) => void;
    onResizeKeydown: (event: KeyboardEvent, column: ProjectTaskListResizableColumn) => void;
  } = $props();

  const { t } = getLocalization();
  const context = getProjectListTableContext();
  let addButton: HTMLButtonElement | undefined = $state();
  let adding = $state(false);
  const addAnchor = $derived(adding ? addButton?.parentElement ?? null : null);
</script>

{#snippet columnHeaderCell(label: string, column: ProjectTaskListResizableColumn)}
  <CollectionColumnHeader {label} resizeLabel={t("projects.columns.resizeColumn", label)}
    class={context?.cellClass(column)} style={context?.cellStyle(column)}
    onpointerdown={(event) => onResizePointerDown(event, column)}
    ondblclick={(event) => onResizeDoubleClick(event, column)}
    onkeydown={(event) => onResizeKeydown(event, column)}>
    {#snippet actions()}<ProjectListPropertyMenu {column} {label} />{/snippet}
  </CollectionColumnHeader>
{/snippet}

<CollectionRow template={gridTemplate} minWidth={gridMinWidth} header divider={false} role="row"
  class={mode === "section" ? "project-list-divider group/column-header" : "project-list-divider group/list-column-header"}>
  <div class={context?.cellClass("selection")} style={context?.cellStyle("selection")}></div>
  <div class={context?.cellClass("open")} style={context?.cellStyle("open")}></div>
  {@render columnHeaderCell(t("projects.list.name"), "name")}
  {#each taskListColumns as column (column)}
    {@render columnHeaderCell(taskListColumnLabel(column), column)}
  {/each}
  <div class="grid min-h-(--collection-table-row-height) min-w-0 place-items-center self-stretch text-muted-foreground">
    {#if context}
      {@const query = context.query}
      <CollectionAddPropertyButton bind:element={addButton} open={adding} disabled={query.propertySaving || query.presentationSaving || query.listColumnsSaving}
        onclick={() => { adding = !adding; }} />
      {#if addAnchor}
        <ProjectListPropertyDraft {query} column={taskListColumns.at(-1) ?? "name"} side="right" anchor={addAnchor} placement="start" trigger={addButton}
          onClose={() => { adding = false; }} />
      {/if}
    {/if}
  </div>
</CollectionRow>
