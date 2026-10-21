<script lang="ts">
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { presetDisplayName, presetLevelAdjustment, presetLevelFromAdjustment } from "$lib/music/soundscape/presets";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicSoundscapeLevelSlider from "./MusicSoundscapeLevelSlider.svelte";

  /** Level of the active preset relative to the main volume, with play and pause for it. */
  let { idPrefix, onPlaybackStart = () => undefined }: { idPrefix: string; onPlaybackStart?: () => void } = $props();
  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  const active = $derived(soundscape.activePreset);
  const playing = $derived(soundscape.snapshot.status === "playing");
  const adjustment = $derived(active ? presetLevelAdjustment(active.level) : 0);
  const adjustmentText = $derived(`${adjustment > 0 ? "+" : ""}${adjustment}%`);
</script>

<p id={`${idPrefix}-level-hint`} class="px-1 text-panel-detail text-muted-foreground">{active ? t("music.soundscape.levelHint") : t("music.soundscape.noActivePreset")}</p>
<div class="flex min-h-9 items-center gap-2 px-1">
  <MusicSoundscapeLevelSlider id={`${idPrefix}-level`} label={t("music.soundscape.level")} value={adjustment} min={-100} max={100} centered disabled={!active} valueText={t("music.soundscape.levelValue", adjustmentText)} describedBy={`${idPrefix}-level-hint`} onchange={(value) => { if (active) void soundscape.setPresetLevel(active, presetLevelFromAdjustment(value)); }} />
  <span class="w-11 shrink-0 text-right text-panel-detail tabular-nums text-muted-foreground">{adjustmentText}</span>
  <button type="button" disabled={!active} aria-label={playing ? t("music.soundscape.pause") : t("music.soundscape.playPreset", active ? presetDisplayName(active, t) : "")} class="grid h-8 w-8 shrink-0 place-items-center rounded-floating-item bg-secondary hover:bg-accent disabled:cursor-not-allowed disabled:opacity-40" onclick={() => { if (!playing) onPlaybackStart(); void soundscape.togglePlayback(); }}>{#if playing}<Pause size={15} />{:else}<Play size={15} />{/if}</button>
</div>
