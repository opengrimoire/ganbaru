<script lang="ts">
  import CollectionCell from "./CollectionCell.svelte";

  let { primary, onPrimary, onSecondary }: {
    primary: "button" | "input" | "disabled-input";
    onPrimary: () => void;
    onSecondary: () => void;
  } = $props();
</script>

<CollectionCell data-testid="cell">
  <span data-testid="static">Value</span>
  <button type="button" data-testid="secondary" onclick={onSecondary}>Clear</button>
  {#if primary === "button"}
    <button type="button" data-collection-cell-primary onclick={onPrimary}>Open</button>
  {:else}
    <input data-collection-cell-primary aria-label="Value" disabled={primary === "disabled-input"} />
  {/if}
</CollectionCell>
