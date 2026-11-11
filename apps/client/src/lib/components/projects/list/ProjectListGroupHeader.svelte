<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ProjectListSelectionButton from "./ProjectListSelectionButton.svelte";

  let {
    title,
    gridTemplate,
    gridMinWidth,
    leadingGridTemplate,
    taskCount,
    allSelected,
    partiallySelected,
    onToggleSelection,
  }: {
    title: string;
    gridTemplate: string;
    gridMinWidth: string;
    leadingGridTemplate: string;
    taskCount: number;
    allSelected: boolean;
    partiallySelected: boolean;
    onToggleSelection: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<div
  class="project-list-divider project-list-sticky-row group/list-group-header grid min-h-(--collection-table-row-height) items-center px-1"
  style={`grid-template-columns: ${gridTemplate}; min-width: ${gridMinWidth};`}
>
  <div
    class="project-list-leading-row grid min-h-(--collection-table-row-height) items-center"
    style={`grid-column: 1 / span 3; grid-template-columns: ${leadingGridTemplate};`}
  >
    <ProjectListSelectionButton
      mode="group"
      {allSelected}
      {partiallySelected}
      disabled={taskCount === 0}
      ariaLabel={allSelected
        ? t("projects.actions.unselectTaskGroup", title)
        : t("projects.actions.selectTaskGroup", title)}
      onToggle={onToggleSelection}
    />
    <div></div>
    <span class="min-w-0 truncate px-2 text-[0.866667rem] font-semibold">
      {title}
    </span>
  </div>
</div>
