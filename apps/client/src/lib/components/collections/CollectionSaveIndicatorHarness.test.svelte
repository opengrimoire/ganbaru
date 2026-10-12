<script lang="ts">
  import CollectionSaveIndicator from "./CollectionSaveIndicator.svelte";
  let pending = $state(false);
</script>

<button onclick={() => { pending = true; }}>Begin save</button>
<button onclick={() => { pending = false; }}>Finish save</button>
<CollectionSaveIndicator {pending} label="Saving database" />
