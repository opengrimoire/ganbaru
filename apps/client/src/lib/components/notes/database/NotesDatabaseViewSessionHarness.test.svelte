<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import type { NotesDataSourcePropertyType } from "$lib/notes/types";

  let { viewId, dataSourceId = "", editingLocked = false, settingsHeader, settingsOpen = false, onCloseSettings = () => {}, newRowRequest = 0, onReady = () => {}, onSavingChange = () => {}, onAddProperty = async () => {} }: {
    dataSourceId?: string;
    editingLocked?: boolean;
    settingsHeader?: import("svelte").Snippet;
    settingsOpen?: boolean;
    onCloseSettings?: () => void;
    viewId?: string | null;
    newRowRequest?: number;
    onReady?: () => void;
    onSavingChange?: (saving: boolean) => void;
    onAddProperty?: (type: NotesDataSourcePropertyType, name: string, icon: string | null) => Promise<void>;
  } = $props();

  let draft = $state(untrack(() => viewId ?? ""));
  let snapshot = $state(untrack(() => viewId ?? ""));
  let saving = $state(false);

  /** Model a layout write that retains its originating callback across an awaited native request. */
  async function save(): Promise<void> {
    if (saving) return;
    const origin = viewId ?? "";
    const reportSaving = onSavingChange;
    saving = true;
    reportSaving(true);
    try {
      await onAddProperty("rich_text", origin, null);
      snapshot = `Saved ${origin}`;
    } finally {
      saving = false;
      reportSaving(false);
    }
  }

  onDestroy(() => onSavingChange(false));
</script>

<div data-session-view={viewId} data-source-id={dataSourceId} data-editing-locked={editingLocked} data-layout-snapshot={snapshot} data-new-row-request={newRowRequest}></div>
{#if settingsOpen}<CollectionSettings label="View settings" onClose={onCloseSettings}>{@render settingsHeader?.()}</CollectionSettings>{/if}
<input aria-label="Layout draft" bind:value={draft} />
<button type="button" data-start-save disabled={saving} onclick={() => { void save(); }}>Save layout</button>
<button type="button" data-finish-view onclick={onReady}>Finish view loading</button>
