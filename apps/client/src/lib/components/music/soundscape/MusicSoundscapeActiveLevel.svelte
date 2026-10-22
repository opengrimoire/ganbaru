<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { presetLevelAdjustment, presetLevelFromAdjustment } from "$lib/music/soundscape/presets";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicSoundscapeRelativeLevel from "./MusicSoundscapeRelativeLevel.svelte";

  /** Level of the playing preset relative to the main volume. Presets themselves start and stop playback. */
  let { idPrefix }: { idPrefix: string } = $props();
  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  const active = $derived(soundscape.playingPreset);
</script>

<MusicSoundscapeRelativeLevel id={`${idPrefix}-level`} title={active ? t("music.soundscape.levelHint") : t("music.soundscape.noActivePreset")} label={t("music.soundscape.level")} adjustment={active ? presetLevelAdjustment(active.level) : 0} disabled={!active} onchange={(value) => { if (active) void soundscape.setPresetLevel(active, presetLevelFromAdjustment(value)); }} />
