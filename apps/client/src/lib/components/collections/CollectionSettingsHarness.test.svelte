<script lang="ts">
  import { untrack } from "svelte";
  import CollectionSettings from "./CollectionSettings.svelte";
  import CollectionMenu from "./CollectionMenu.svelte";
  import Select from "$lib/components/ui/Select.svelte";

  let { showHeader = true, filterDisabled = false, onClose = () => {}, onCommit = () => {} }: { showHeader?: boolean; filterDisabled?: boolean; onClose?: () => void; onCommit?: (value: string) => void } = $props();
  let open = $state(false);
  let anchor: HTMLButtonElement | null = $state(null);
  let draft = $state("");
  let status = $state("todo");
  let saving = $state(false);
  let filterLocked = $state(untrack(() => filterDisabled));
  let filterRow: ReturnType<typeof CollectionMenu> | undefined = $state();
  let layoutShown = $state(true);

  /** Remove the Layout row, as deleting the item a row edits would. */
  export function removeLayout(): void {
    layoutShown = false;
  }

  /** Ask the Filter row to open its page, as a caller outside the panel would. */
  export function showFilter(): void {
    filterRow?.show();
  }

  /** Enable the Filter row after a pending write settles. */
  export function enableFilter(): void {
    filterLocked = false;
  }
</script>

<button bind:this={anchor} type="button" aria-label="View settings" onclick={() => { open = true; }}>Settings</button>
{#if open}
  <CollectionSettings label="View settings" {anchor} {showHeader} onClose={() => { open = false; onClose(); }}>
    <fieldset disabled={saving}>
      <input aria-label="View name" bind:value={draft} onblur={() => onCommit(draft)} />
      <button type="button" onclick={() => { saving = true; }}>Save</button>
    </fieldset>
    {#if layoutShown}
      <CollectionMenu label="Layout" summary="Table" fullWidth>
        <input aria-label="Layout value" />
        <CollectionMenu label="Advanced layout" fullWidth>
          <input aria-label="Advanced value" />
        </CollectionMenu>
      </CollectionMenu>
    {/if}
    <CollectionMenu bind:this={filterRow} label="Filter" summary="1 rule" kind="filter" fullWidth disabled={filterLocked}>
      <Select inline ariaLabel="Status" value={status} options={[{ value: "todo", label: "To do" }, { value: "done", label: "Done" }]} onChange={(value) => { status = value; }} />
    </CollectionMenu>
  </CollectionSettings>
{/if}
