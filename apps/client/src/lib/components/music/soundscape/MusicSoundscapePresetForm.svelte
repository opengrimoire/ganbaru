<script lang="ts">
  import { onMount } from "svelte";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapeDefinition, MusicSoundscapePreset } from "$lib/music/soundscape/contracts";
  import { presetSoundNames } from "$lib/music/soundscape/presets";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";

  const DEFAULT_PRESET_ICON = "lucide:layers";
  /** Mirrors the native preset name limit so the field never accepts a name the store rejects. */
  const MAX_PRESET_NAME_CHARS = 80;

  /**
   * Quick save from the background sound controls: names a new preset made of the listed sounds. Choosing sounds
   * explicitly and editing presets happen in the builder.
   */
  let {
    soundIds,
    nested = false,
    onDone,
  }: {
    soundIds: readonly string[];
    /** Keeps the icon picker inside the nearest floating root so an enclosing popover stays open. */
    nested?: boolean;
    onDone: (preset: MusicSoundscapePreset | null) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const soundscape = getSoundscapeStore();
  let nameInput = $state<HTMLInputElement | null>(null);
  let name = $state("");
  let icon = $state(DEFAULT_PRESET_ICON);
  const names = $derived(presetSoundNames(soundIds, soundscape.definitions, soundName));
  const canSave = $derived(name.trim().length > 0 && soundIds.length > 0 && !soundscape.saving);

  onMount(() => nameInput?.focus());

  function soundName(definition: MusicSoundscapeDefinition): string {
    return definition.generatedKind ? t(`music.soundscape.rainName.${definition.generatedKind}`) : definition.name;
  }

  function cancelWithEscape(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    event.stopPropagation();
    onDone(null);
  }

  async function save(): Promise<void> {
    if (!canSave) return;
    try {
      const saved = await soundscape.savePreset({
        id: crypto.randomUUID(),
        name: name.trim(),
        icon,
        soundscapeIds: [...soundIds],
        expectedVersion: null,
        updatedAtMs: Date.now(),
      });
      onDone(saved);
    } catch (error) {
      console.error("Could not save background sound preset", error);
    }
  }
</script>

<form class="rounded-floating-item bg-secondary/40 p-2" onsubmit={(event) => { event.preventDefault(); void save(); }}>
  <div class="flex items-center gap-2">
    <IconPicker value={icon} onChange={(value) => icon = value} ariaLabel={t("music.soundscape.preset.icon")} showUpload={false} showRemove={false} {nested}>
      {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-background text-foreground hover:bg-accent" aria-label={t("music.soundscape.preset.icon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeGroupIcon {icon} /></button>{/snippet}
    </IconPicker>
    <input bind:this={nameInput} class="field h-9 min-w-0 flex-1 px-3 text-xs" bind:value={name} aria-label={t("music.soundscape.preset.name")} placeholder={t("music.soundscape.preset.name")} maxlength={MAX_PRESET_NAME_CHARS} onkeydown={cancelWithEscape} />
  </div>
  {#if names.length > 0}
    <p class="mt-2 truncate text-panel-detail text-muted-foreground">{t("music.soundscape.preset.includes", formatList(localization.locale, names))}</p>
  {/if}
  <div class="mt-2 flex items-center justify-end gap-2">
    <button type="button" class="h-8 rounded-lg px-3 text-xs hover:bg-accent" onclick={() => onDone(null)}>{t("common.cancel")}</button>
    <button type="submit" disabled={!canSave} class="h-8 rounded-lg bg-primary px-3 text-xs text-primary-foreground disabled:opacity-40">{t("music.soundscape.preset.save")}</button>
  </div>
</form>
