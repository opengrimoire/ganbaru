<script lang="ts">
  import type { MusicSourceCollection } from "$lib/music/library/contracts";
  import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";

  let {
    controller,
    collection,
    onClose,
    onApplied,
  }: {
    controller: MusicSourcesController;
    collection: MusicSourceCollection;
    onClose: () => void;
    onApplied: () => void;
  } = $props();
</script>
