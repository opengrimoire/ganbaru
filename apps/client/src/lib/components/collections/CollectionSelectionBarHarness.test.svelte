<script lang="ts">
  import CollectionSelectionBar from "./CollectionSelectionBar.svelte";

  let { onClear }: { onClear: () => void } = $props();
</script>

<button type="button" data-testid="outside">Sidebar</button>
<div data-testid="floating" data-app-floating-surface>Menu</div>
<section data-collection-selection-scope>
  <button type="button" data-testid="inside">Row</button>
  <CollectionSelectionBar count={2} {onClear}>
    <button type="button" data-testid="action">Trash</button>
  </CollectionSelectionBar>
</section>
