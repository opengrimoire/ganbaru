<script lang="ts">
  import { onMount } from "svelte";
  import MusicBuilderDialog from "$lib/components/music/builder/MusicBuilderDialog.svelte";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    DEFAULT_SOUNDSCAPE_PRESET_LEVEL,
    MAX_SOUNDSCAPE_LAYERS,
    type MusicSoundscapeDefinition,
    type MusicSoundscapePreset,
    type MusicSoundscapePresetSound,
  } from "$lib/music/soundscape/contracts";
  import { orderedGeneratedSounds, SOUNDSCAPE_CHIP_CLASS } from "$lib/music/soundscape/presentation";
  import {
    formatPresetLevelAdjustment,
    PRESET_LEVEL_ADJUSTMENT_RANGE,
    presetDisplayName,
    presetLevelAdjustment,
    presetLevelFromAdjustment,
    setPresetDraftSoundLevel,
    soundDisplayName,
    togglePresetDraftSound,
  } from "$lib/music/soundscape/presets";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import { growThenFade } from "$lib/utils/grow-then-fade";
  import { portal } from "$lib/utils/portal";
  import MusicSoundscapeIcon from "./MusicSoundscapeIcon.svelte";
  import MusicSoundscapeLevelSlider from "./MusicSoundscapeLevelSlider.svelte";
  import MusicSoundscapeRelativeLevel from "./MusicSoundscapeRelativeLevel.svelte";

  const DEFAULT_PRESET_ICON = "lucide:layers";
  /** Mirrors the native preset name limit so the field never accepts a name the store rejects. */
  const MAX_PRESET_NAME_CHARS = 80;
  /** How long layer rows and notices take to open or close, so the editor never jumps in height. */
  const ROW_MOTION_MS = 240;

  /**
   * Creates or edits a soundscape preset by picking its sounds explicitly. While it is open, Music and the soundscape
   * pause and the draft plays in an isolated preview; leaving brings back what was playing, unchanged.
   */
  let {
    preset = null,
    onDone,
    onRemove,
  }: {
    preset?: MusicSoundscapePreset | null;
    onDone: (preset: MusicSoundscapePreset | null) => void;
    onRemove?: (preset: MusicSoundscapePreset) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const soundscape = getSoundscapeStore();
  const player = getMusicPlayer();
  // Drafts start from the preset once; later store refreshes must not overwrite what the person is choosing.
  let name = $state(initialName());
  let icon = $state(initialIcon());
  let sounds = $state<MusicSoundscapePresetSound[]>(initialSounds());
  let level = $state(initialLevel());
  const initialDraft = draftSignature(initialName(), initialIcon(), initialLevel(), initialSounds());
  const dirty = $derived(draftSignature(name, icon, level, sounds) !== initialDraft);
  let confirmingDiscard = $state(false);
  const atLimit = $derived(sounds.length >= MAX_SOUNDSCAPE_LAYERS);
  const canSave = $derived(name.trim().length > 0 && sounds.length > 0 && !soundscape.saving);
  const mix = $derived(sounds.flatMap((sound) => {
    const definition = soundscape.definitions.find((entry) => entry.id === sound.soundscapeId);
    return definition ? [{ definition, level: sound.level }] : [];
  }));
  let listening = $state(true);
  const layers = $derived(orderedLayers(soundscape.definitions));
  const widestLevelLabel = $derived(formatPresetLevelAdjustment(localization.locale, PRESET_LEVEL_ADJUSTMENT_RANGE.max));
  const rowMotion = { duration: prefersReducedMotion() ? 0 : ROW_MOTION_MS };

  onMount(() => {
    const resumesMusic = player.isPlaying;
    if (resumesMusic) void player.pausePlayback("system");
    void soundscape.beginPreview();
    return () => {
      void soundscape.endPreview();
      if (resumesMusic && !player.isPlaying) void player.playPlayback("system");
    };
  });

  $effect(() => {
    void soundscape.previewMix(listening ? { sounds, level } : null);
  });

  function initialName(): string {
    return preset ? presetDisplayName(preset, t) : "";
  }

  function initialIcon(): string {
    return preset?.icon ?? DEFAULT_PRESET_ICON;
  }

  function initialLevel(): number {
    return preset?.level ?? DEFAULT_SOUNDSCAPE_PRESET_LEVEL;
  }

  function initialSounds(): MusicSoundscapePresetSound[] {
    return preset ? preset.sounds.map((sound) => ({ ...sound })) : [];
  }

  /** Comparable form of a draft, so returning every field to where it started counts as unchanged. */
  function draftSignature(draftName: string, draftIcon: string, draftLevel: number, draftSounds: readonly MusicSoundscapePresetSound[]): string {
    return JSON.stringify([draftName.trim(), draftIcon, draftLevel, draftSounds.map((sound) => [sound.soundscapeId, sound.level])]);
  }

  /** Leaves without saving, asking first when the draft has changes. */
  function requestClose(): void {
    if (dirty) confirmingDiscard = true;
    else onDone(null);
  }

  function prefersReducedMotion(): boolean {
    return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  /** Layers in builder order: generated noise first, then local loops. */
  function orderedLayers(definitions: readonly MusicSoundscapeDefinition[]): MusicSoundscapeDefinition[] {
    return [...orderedGeneratedSounds(definitions), ...definitions.filter((entry) => entry.sourceKind === "local-loop")];
  }

  /** Keeps the stored name of an unrenamed seeded preset so it stays localized in every language. */
  function savedName(): string {
    const trimmed = name.trim();
    return preset && trimmed === presetDisplayName(preset, t) ? preset.name : trimmed;
  }

  async function save(): Promise<void> {
    if (!canSave) return;
    try {
      const saved = await soundscape.savePreset({
        id: preset?.id ?? crypto.randomUUID(),
        name: savedName(),
        icon,
        level,
        sounds: sounds.map((sound) => ({ ...sound })),
        expectedVersion: preset ? soundscape.presets.find((entry) => entry.id === preset.id)?.version ?? preset.version : null,
        updatedAtMs: Date.now(),
      });
      onDone(saved);
    } catch (error) {
      console.error("Could not save soundscape preset", error);
    }
  }
</script>

<MusicBuilderDialog
  title={preset ? t("music.soundscape.preset.editTitle") : t("music.soundscape.preset.new")}
  titleId="soundscape-preset-editor"
  size="large"
  dismissDisabled={soundscape.saving}
  onDismiss={requestClose}
>
  <div class="@container flex flex-col gap-5">
    <div class="flex items-center gap-2">
      <IconPicker value={icon} onChange={(value) => icon = value} ariaLabel={t("music.soundscape.preset.icon")} showUpload={false} showRemove={false}>
        {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-secondary/40 text-foreground hover:bg-accent" aria-label={t("music.soundscape.preset.icon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeIcon {icon} /></button>{/snippet}
      </IconPicker>
      <input data-dialog-autofocus class="field h-9 min-w-0 flex-1 px-3 text-xs" bind:value={name} aria-label={t("music.soundscape.preset.name")} placeholder={t("music.soundscape.preset.name")} maxlength={MAX_PRESET_NAME_CHARS} onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); void save(); } }} />
    </div>

    <div class="text-xs">
      <MusicSoundscapeRelativeLevel id="soundscape-preset-level" title={t("music.soundscape.levelHint")} label={t("music.soundscape.preset.level")} adjustment={presetLevelAdjustment(level)} onchange={(value) => level = presetLevelFromAdjustment(value)} />
      <p id="soundscape-preset-layer-levels" class="mt-5 truncate">{t("music.soundscape.preset.layerLevels")}</p>
      <!-- The list and its empty hint share one cell, so swapping them changes the height once instead of stacking both. -->
      <div class="grid items-start">
        {#if mix.length > 0}
          <ul transition:growThenFade={rowMotion} class="col-start-1 row-start-1 mt-1 flex flex-col" aria-labelledby="soundscape-preset-layer-levels">
            {#each mix as entry (entry.definition.id)}
              {@const soundName = soundDisplayName(entry.definition, t)}
              {@const percent = Math.round(entry.level * 100)}
              {@const percentText = formatNumber(localization.locale, percent / 100, { style: "percent" })}
              <!-- Rows carry their own spacing so a closing row takes its gap with it. -->
              <li transition:growThenFade={rowMotion} class="flex h-9 items-center gap-3">
                <span class="flex w-28 shrink-0 items-center gap-1.5"><span class="shrink-0 text-muted-foreground"><MusicSoundscapeIcon icon={entry.definition.icon} size={14} /></span><span class="min-w-0 truncate">{soundName}</span></span>
                <MusicSoundscapeLevelSlider id={`soundscape-preset-sound-${entry.definition.id}`} label={t("music.soundscape.preset.soundLevel", soundName)} value={percent} min={0} max={100} valueText={percentText} onchange={(value) => sounds = setPresetDraftSoundLevel(sounds, entry.definition.id, value / 100)} />
                <!-- Sized by the widest overall level label so every slider ends where the overall one does. -->
                <span class="grid shrink-0 text-right tabular-nums text-muted-foreground"><span class="invisible col-start-1 row-start-1" aria-hidden="true">{widestLevelLabel}</span><span class="col-start-1 row-start-1">{percentText}</span></span>
              </li>
            {/each}
          </ul>
        {:else}
          <p transition:growThenFade={rowMotion} class="col-start-1 row-start-1 mt-2 text-muted-foreground">{t("music.soundscape.preset.chooseLayer")}</p>
        {/if}
      </div>
    </div>

    <section aria-labelledby="soundscape-preset-sounds">
      <div class="flex h-8 items-center gap-2">
        <h3 id="soundscape-preset-sounds" class="min-w-0 flex-1 truncate text-xs font-semibold">{t("music.soundscape.preset.sounds")}</h3>
        <span class="min-w-0 truncate text-xs text-muted-foreground" aria-hidden="true">{t("music.soundscape.preset.listen")}</span>
        <Switch size="compact" checked={listening} ariaLabel={t("music.soundscape.preset.listen")} onChange={(value) => listening = value} />
      </div>
      {#if atLimit}<div transition:growThenFade={rowMotion} class="pt-2"><Notice class="text-xs" tone="warning" role="status" copy={false} message={t("music.soundscape.preset.limitReached", MAX_SOUNDSCAPE_LAYERS)} /></div>{/if}

      <div class="mt-2 flex flex-wrap gap-1.5 text-xs">
        {#each layers as definition (definition.id)}
          {@const picked = sounds.some((sound) => sound.soundscapeId === definition.id)}
          <button type="button" aria-pressed={picked} disabled={!picked && atLimit} title={definition.availability === "available" ? undefined : t("music.soundscape.needsRepair")} class={[SOUNDSCAPE_CHIP_CLASS, picked && "bg-primary/10", definition.availability !== "available" && "text-muted-foreground"]} onclick={() => sounds = togglePresetDraftSound(sounds, definition.id)}><MusicSoundscapeIcon icon={definition.icon} size={14} /><span class="truncate">{soundDisplayName(definition, t)}</span></button>
        {/each}
      </div>
    </section>
  </div>

  {#snippet footer()}
    <button type="button" onclick={requestClose} disabled={soundscape.saving} class="min-h-10 rounded-md border border-border bg-card px-3.5 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50">{t("common.cancel")}</button>
    <button type="button" data-preset-save onclick={() => { void save(); }} disabled={!canSave} class="min-h-10 rounded-md border border-border bg-primary px-3.5 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50">{t("music.soundscape.preset.save")}</button>
    {#if preset && onRemove}<button type="button" onclick={() => onRemove(preset)} disabled={soundscape.saving} class="ml-auto min-h-10 rounded-md px-3.5 py-2 text-sm font-medium text-destructive hover:bg-accent disabled:opacity-50">{t("music.soundscape.preset.remove")}</button>{/if}
  {/snippet}
</MusicBuilderDialog>
{#if confirmingDiscard}
  <!-- Portaled after the editor modal so the confirmation stacks above it. -->
  <div use:portal>
    <ConfirmDialog title={t("calendar.view.discardUnsavedTitle")} message={t("calendar.view.changesLost")} confirmLabel={t("calendar.view.discard")} cancelLabel={t("calendar.view.keepEditing")} onConfirm={() => { confirmingDiscard = false; onDone(null); }} onCancel={() => confirmingDiscard = false} />
  </div>
{/if}
