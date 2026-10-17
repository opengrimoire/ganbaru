<script lang="ts">
  import CollectionPropertyDraft from "$lib/components/collections/CollectionPropertyDraft.svelte";
  import type { CollectionPropertyDraftPlacement } from "$lib/components/collections/property-draft";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { uniqueProjectCustomFieldName } from "$lib/projects/custom-fields";
  import { projectCustomFieldTypeLabel } from "$lib/projects/display";
  import type { ProjectTaskListResizableColumn } from "$lib/projects/list/view";
  import type { ProjectCustomFieldType } from "$lib/projects/types";
  import type { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";
  import { projectCustomFieldTypeOptions } from "./property-kinds";

  /**
   * A draft column header that creates a custom field from the task table and shows it beside `column`.
   * Creation failures stay in the draft and are reported through the query's property error.
   */
  let { query, column, side, anchor, placement, trigger, onClose }: {
    query: ProjectTaskQueryController;
    /** The column the new field is placed beside. */
    column: ProjectTaskListResizableColumn;
    side: "left" | "right";
    anchor: HTMLElement;
    placement: CollectionPropertyDraftPlacement;
    trigger?: HTMLElement | null;
    onClose: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const pending = $derived(query.propertySaving || query.presentationSaving || query.listColumnsSaving);
  const types = $derived(projectCustomFieldTypeOptions(t));

  /** Create the field, naming it after its type when the name was left blank. */
  function create(type: ProjectCustomFieldType, name: string, icon: string | null): Promise<boolean> {
    const fieldName = name || uniqueProjectCustomFieldName(projectCustomFieldTypeLabel(type, t), query.customFields, localization.locale);
    return query.addColumnProperty(fieldName, type, column, side, { icon });
  }
</script>

<CollectionPropertyDraft {anchor} {placement} {trigger} {types} {pending} onCreate={create} {onClose} />
