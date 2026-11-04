<script lang="ts">
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import { soundscapeIconStrokeWidth } from "$lib/music/soundscape/presentation";

  /** Soundscape or layer icon whose stroke thins at tile sizes, so large icons read as light as list icons. */
  let { icon, size = 16 }: { icon: string; size?: number } = $props();
  const strokeWidth = $derived(soundscapeIconStrokeWidth(size));
</script>

<!-- The variables reach Lucide components that the global icon stroke rule would otherwise override. -->
<span class="contents" style={`--icon-stroke-width: ${strokeWidth}; --icon-stroke-width-compact: ${strokeWidth};`}><ProjectIcon name={icon} {size} {strokeWidth} /></span>
