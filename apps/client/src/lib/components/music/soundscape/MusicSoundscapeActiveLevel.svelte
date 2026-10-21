<script lang="ts">
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { presetDisplayName, presetLevelAdjustment, presetLevelFromAdjustment } from "$lib/music/soundscape/presets";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicSoundscapeRelativeLevel from "./MusicSoundscapeRelativeLevel.svelte";

  /** Level of the active preset relative to the main volume, with play and pause for it. */
  let { idPrefix, onPlaybackStart = () => undefined }: { idPrefix: string; onPlaybackStart?: () => void } = $props();
  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  const active = $derived(soundscape.activePreset);
  const playing = $derived(soundscape.snapshot.status === "playing");
</script>

<div class="px-1">
  <MusicSoundscapeRelativeLevel id={`${idPrefix}-level`} title={active ? t("music.soundscape.levelHint") : t("music.soundscape.noActivePreset")} label={t("music.soundscape.level")} adjustment={active ? presetLevelAdjustment(active.level) : 0} disabled={!active} onchange={(value) => { if (active) void soundscape.setPresetLevel(active, presetLevelFromAdjustment(value)); }}>
    {#snippet trailing()}
      <button type="button" disabled={!active} aria-label={playing ? t("music.soundscape.pause") : t("music.soundscape.playPreset", active ? presetDisplayName(active, t) : "")} class="grid h-8 w-8 shrink-0 place-items-center rounded-floating-item bg-secondary hover:bg-accent disabled:cursor-not-allowed disabled:opacity-40" onclick={() => { if (!playing) onPlaybackStart(); void soundscape.togglePlayback(); }}>{#if playing}<Pause size={15} />{:else}<Play size={15} />{/if}</button>
    {/snippet}
  </MusicSoundscapeRelativeLevel>
</div>
