<script lang="ts">
  import { untrack, type ComponentProps } from "svelte";
  import type { ProjectCustomFieldValue } from "$lib/projects/types";
  import ProjectListColumnCell from "./ProjectListColumnCell.svelte";

  let { props: cellProps }: { props: ComponentProps<typeof ProjectListColumnCell> } = $props();
  let value = $state<ProjectCustomFieldValue | undefined>(untrack(() => {
    const field = cellProps.projectCustomFields[0];
    return field ? cellProps.customFieldValue(cellProps.task, field) : undefined;
  }));

  /** Mirror canonical store changes only after the native numeric write succeeds. */
  async function save(...arguments_: Parameters<typeof cellProps.onSaveCustomFieldValue>): Promise<void> {
    await cellProps.onSaveCustomFieldValue(...arguments_);
    const [task, field, update] = arguments_;
    value = { taskId: task.id, fieldId: field.id, numberValue: update.numberValue ?? undefined, updatedAt: "" };
  }
</script>

<ProjectListColumnCell {...cellProps} customFieldValue={() => value} onSaveCustomFieldValue={save} />
