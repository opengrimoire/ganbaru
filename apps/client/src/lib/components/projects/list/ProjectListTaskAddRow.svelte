<script lang="ts">
  import type { MaybePromise } from "$lib/utils";
  import CollectionRow from "$lib/components/collections/CollectionRow.svelte";
  import CollectionQuickAdd from "$lib/components/collections/CollectionQuickAdd.svelte";
  import { getProjectListTableContext } from "./table-context";

  type TaskAddMode = "section" | "group" | "subtask";

  let {
    mode,
    rowId,
    gridTemplate,
    gridMinWidth,
    label,
    draft,
    active,
    pending,
    error = null,
    indent,
    onDraftChange,
    onActiveChange,
    onSubmit,
  }: {
    mode: TaskAddMode;
    rowId: string;
    gridTemplate: string;
    gridMinWidth: string;
    label: string;
    draft: string;
    active: boolean;
    pending: boolean;
    error?: string | null;
    /** Title inset of the nested rows the add row belongs to. */
    indent?: string;
    onDraftChange: (value: string) => void;
    onActiveChange: (active: boolean) => void;
    onSubmit: () => MaybePromise;
  } = $props();

  const context = getProjectListTableContext();
</script>

<CollectionRow template={gridTemplate} minWidth={gridMinWidth} gutter={context?.gutter} divider={false} class="project-list-divider"
  data-section-task-add-row={mode === "section" ? rowId : undefined}
  data-group-task-add-row={mode === "group" ? rowId : undefined}
  data-subtask-add-row={mode === "subtask" ? rowId : undefined}>
  <CollectionQuickAdd {label} {draft} {active} disabled={pending} contentColumn={3} {indent} {onDraftChange} {onActiveChange}
    inputAttributes={{ "data-section-task-input": mode === "section" ? rowId : undefined, "data-group-task-input": mode === "group" ? rowId : undefined }}
    onSubmit={async () => { await onSubmit(); return !error; }} />
  {#if error}<p class="col-span-full px-2 py-1 text-[0.8rem] text-destructive" role="alert">{error}</p>{/if}
</CollectionRow>
