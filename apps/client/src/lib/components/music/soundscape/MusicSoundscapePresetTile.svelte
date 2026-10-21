<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapePreset } from "$lib/music/soundscape/contracts";
  import { presetDisplayName } from "$lib/music/soundscape/presets";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicSoundscapeChip from "./MusicSoundscapeChip.svelte";

  /** Builder option for one preset: the chip plays or pauses it, the menu edits or removes it. */
  let { preset, onEdit, onRemove, onPlaybackStart }: {
    preset: MusicSoundscapePreset;
    onEdit: (preset: MusicSoundscapePreset) => void;
    onRemove: (preset: MusicSoundscapePreset) => void;
    onPlaybackStart: () => void;
  } = $props();
  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  const playing = $derived(soundscape.playingPreset?.id === preset.id);
  const playable = $derived(soundscape.presetIsPlayable(preset));
  const name = $derived(presetDisplayName(preset, t));

  function choose(): void {
    if (playing) {
      void soundscape.togglePlayback();
      return;
    }
    onPlaybackStart();
    void soundscape.playPreset(preset);
  }
</script>

<MusicSoundscapeChip icon={preset.icon} label={name} active={playing} ariaLabel={playing ? t("music.soundscape.pause") : t("music.soundscape.playPreset", name)} title={playable ? undefined : t("music.soundscape.needsRepair")} tooltip={playing ? t("music.soundscape.pauseHint") : undefined} disabled={!playable || soundscape.saving} onclick={choose} menuLabel={t("music.soundscape.preset.actions", name)}>
  {#snippet menu(close)}
    <button type="button" role="menuitem" class="menu-item" onclick={() => { close(); onEdit(preset); }}>{t("music.soundscape.preset.editTitle")}</button>
    <div class="menu-separator" role="separator"></div>
    <button type="button" role="menuitem" class="menu-item menu-item-destructive" onclick={() => { close(); onRemove(preset); }}>{t("music.soundscape.preset.remove")}</button>
  {/snippet}
</MusicSoundscapeChip>
