<script lang="ts">
  import { untrack } from "svelte";
  import CollectionPropertyNameField from "./CollectionPropertyNameField.svelte";

  let { initialPropertyId, initialName, editable = true, onRename }: {
    initialPropertyId: string;
    initialName: string;
    editable?: boolean;
    onRename: (name: string) => Promise<void>;
  } = $props();

  let propertyId = $state(untrack(() => initialPropertyId));
  let name = $state(untrack(() => initialName));

  /** Deliver a canonical property, as a table does after a schema change. */
  export function show(nextPropertyId: string, nextName: string): void {
    propertyId = nextPropertyId;
    name = nextName;
  }
</script>

<CollectionPropertyNameField {propertyId} {name} kind="number" {editable} {onRename} />
