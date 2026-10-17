<script lang="ts">
  import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";

  let {
    controller,
    itemId,
    onClose,
    onRepaired,
  }: {
    controller: MusicSourcesController;
    itemId: string;
    onClose: () => void;
    onRepaired: () => void;
  } = $props();
</script>
