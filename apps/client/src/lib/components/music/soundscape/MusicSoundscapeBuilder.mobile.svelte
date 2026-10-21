<script lang="ts">
  import type { SoundscapeFilter } from "$lib/music/builder/view-state";

  let {
    filter = "all",
    addRequest = 0,
    onPlaybackStart = () => undefined,
  }: {
    filter?: SoundscapeFilter;
    addRequest?: number;
    onPlaybackStart?: () => void;
  } = $props();
</script>
