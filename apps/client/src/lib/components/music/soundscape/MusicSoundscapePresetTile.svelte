<script lang="ts">
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import { onMount } from "svelte";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapeDefinition, MusicSoundscapePreset } from "$lib/music/soundscape/contracts";
  import { presetSoundNames } from "$lib/music/soundscape/presets";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";

  /** Builder tile for one preset: the tile plays or pauses it, the menu edits or removes it. */
  let { preset, onEdit, onRemove, onPlaybackStart }: {
    preset: MusicSoundscapePreset;
    onEdit: (preset: MusicSoundscapePreset) => void;
    onRemove: (preset: MusicSoundscapePreset) => void;
    onPlaybackStart: () => void;
  } = $props();
  const localization = getLocalization();
  const { t } = localization;
  const soundscape = getSoundscapeStore();
  let root = $state<HTMLElement | null>(null);
  let menuOpen = $state(false);
  const selected = $derived(soundscape.activePreset?.id === preset.id);
  const playing = $derived(selected && soundscape.snapshot.status === "playing");
  const playable = $derived(soundscape.presetIsPlayable(preset));
  const names = $derived(presetSoundNames(preset.soundscapeIds, soundscape.definitions, soundName));
  const tooltip = $derived(playable && names.length > 0 ? t("music.soundscape.preset.includes", formatList(localization.locale, names)) : t("music.soundscape.needsRepair"));

  onMount(() => {
    const close = (event: PointerEvent) => { if (menuOpen && root && !root.contains(event.target as Node)) menuOpen = false; };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !menuOpen) return;
      event.preventDefault();
      event.stopPropagation();
      menuOpen = false;
    };
    window.addEventListener("pointerdown", close, true);
    window.addEventListener("keydown", escape, true);
    return () => { window.removeEventListener("pointerdown", close, true); window.removeEventListener("keydown", escape, true); };
  });

  function soundName(definition: MusicSoundscapeDefinition): string {
    return definition.generatedKind ? t(`music.soundscape.rainName.${definition.generatedKind}`) : definition.name;
  }

  function choose(): void {
    if (playing) {
      void soundscape.togglePlayback();
      return;
    }
    onPlaybackStart();
    void soundscape.playPreset(preset);
  }
</script>

<div bind:this={root} class="relative min-w-0">
  <button type="button" aria-pressed={selected} aria-label={playing ? t("music.soundscape.pause") : t("music.soundscape.play", preset.name)} title={tooltip} disabled={!playable || soundscape.saving} class={selected ? "flex min-h-28 w-full min-w-0 flex-col items-center justify-center gap-2 rounded-xl bg-primary/10 px-3 text-center hover:bg-primary/15 disabled:opacity-50" : "flex min-h-28 w-full min-w-0 flex-col items-center justify-center gap-2 rounded-xl bg-secondary/40 px-3 text-center hover:bg-secondary/70 disabled:opacity-50"} onclick={choose}>
    <MusicSoundscapeGroupIcon icon={preset.icon} size={27} />
    <span class="max-w-full truncate text-xs font-medium">{preset.name}</span>
  </button>
  <button type="button" aria-label={t("music.soundscape.preset.actions", preset.name)} aria-expanded={menuOpen} class="absolute top-1 right-1 z-10 grid h-7 w-7 place-items-center rounded-lg text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => menuOpen = !menuOpen}><Ellipsis size={16} /></button>
  {#if menuOpen}
    <div class="surface-floating surface-floating-body absolute top-8 right-1 z-20 flex min-w-36 flex-col">
      <button type="button" class="menu-item" onclick={() => { menuOpen = false; onEdit(preset); }}>{t("music.soundscape.preset.editTitle")}</button>
      <div class="menu-separator" role="separator"></div>
      <button type="button" class="menu-item menu-item-destructive" onclick={() => { menuOpen = false; onRemove(preset); }}>{t("music.soundscape.preset.remove")}</button>
    </div>
  {/if}
</div>
