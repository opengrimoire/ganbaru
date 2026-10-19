<script lang="ts">
  import CloudHail from "@lucide/svelte/icons/cloud-hail";
  import CloudRain from "@lucide/svelte/icons/cloud-rain";
  import CloudRainWind from "@lucide/svelte/icons/cloud-rain-wind";
  import type { MusicSoundscapeDefinition } from "$lib/music/soundscape/contracts";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";

  /** Icon for one background sound: generated rain uses its intensity icon, other sounds use their chosen icon. */
  let { definition, size = 16 }: { definition: MusicSoundscapeDefinition; size?: number } = $props();
</script>

{#if definition.generatedKind === "brown"}<CloudHail {size} strokeWidth={1.5} />
{:else if definition.generatedKind === "pink"}<CloudRain {size} strokeWidth={1.5} />
{:else if definition.generatedKind === "white"}<CloudRainWind {size} strokeWidth={1.5} />
{:else}<MusicSoundscapeGroupIcon icon={definition.icon} {size} />{/if}
