<script lang="ts">
  import type { SoundscapeFilter } from "$lib/music/builder/view-state";

  let {
    filter = "all",
    compact = false,
    addRequest = 0,
    onPlaybackStart = () => undefined,
  }: {
    filter?: SoundscapeFilter;
    compact?: boolean;
    addRequest?: number;
    onPlaybackStart?: () => void;
    onGroupRemoved?: () => void;
  } = $props();
</script>
