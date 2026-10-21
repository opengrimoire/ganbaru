<script lang="ts">
  import type { MusicSoundscapeDefinition } from "$lib/music/soundscape/contracts";
  import { GENERATED_RAIN_ICONS } from "$lib/music/soundscape/presentation";
  import MusicSoundscapeIcon from "./MusicSoundscapeIcon.svelte";

  /** Icon for one soundscape sound: generated rain uses its intensity icon, other sounds use their chosen icon. */
  let { definition, size = 16 }: { definition: MusicSoundscapeDefinition; size?: number } = $props();
</script>

<MusicSoundscapeIcon icon={definition.generatedKind ? GENERATED_RAIN_ICONS[definition.generatedKind] : definition.icon} {size} />
