<script lang="ts">
  import type { Snippet } from "svelte";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /** Data source actions shared by the end of every database view's settings panel. */
  let { reloadLabel, editingLocked, reloadDisabled, onEditProperties, onReload, actions }: {
    reloadLabel: string;
    editingLocked: boolean;
    reloadDisabled: boolean;
    onEditProperties: () => void;
    onReload: () => void;
    /** Extra data source actions shown between Edit properties and Reload. */
    actions?: Snippet;
  } = $props();

  const { t } = getLocalization();
</script>

<CollectionMenuSeparator />
{#if !editingLocked}
  <CollectionMenuItem icon={Settings2} label={t("notes.databaseViewEditProperties")} onclick={onEditProperties} />
  {@render actions?.()}
{/if}
<CollectionMenuItem icon={RefreshCw} label={reloadLabel} disabled={reloadDisabled} onclick={onReload} />
