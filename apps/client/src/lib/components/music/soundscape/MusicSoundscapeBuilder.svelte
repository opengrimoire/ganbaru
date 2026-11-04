<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import { untrack } from "svelte";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import { pickSoundscapeFile } from "$lib/api/music";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapeDefinition, MusicSoundscapePreset } from "$lib/music/soundscape/contracts";
  import { NEW_LAYER_ICON, orderedGeneratedSounds, SOUNDSCAPE_CHIP_CLASS } from "$lib/music/soundscape/presentation";
  import { presetDisplayName, soundDisplayName } from "$lib/music/soundscape/presets";
  import type { SoundscapeFilter } from "$lib/music/builder/view-state";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import { portal } from "$lib/utils/portal";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import MusicSoundscapeActiveLevel from "./MusicSoundscapeActiveLevel.svelte";
  import MusicSoundscapeChip from "./MusicSoundscapeChip.svelte";
  import MusicSoundscapeIcon from "./MusicSoundscapeIcon.svelte";
  import MusicSoundscapeLocalRow from "./MusicSoundscapeLocalRow.svelte";
  import MusicSoundscapePresetEditor from "./MusicSoundscapePresetEditor.svelte";
  import MusicSoundscapePresetTile from "./MusicSoundscapePresetTile.svelte";

  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  let pendingDelete = $state<MusicSoundscapeDefinition | null>(null);
  let pendingLayerPath = $state<string | null>(null);
  let soundNameDraft = $state("");
  let soundIconDraft = $state(NEW_LAYER_ICON);
  /** Preset being created (`preset: null`) or edited; the editor replaces the builder content while open. */
  let presetEditor = $state<{ preset: MusicSoundscapePreset | null } | null>(null);
  let pendingPresetDelete = $state<MusicSoundscapePreset | null>(null);
  let {
    filter = "all",
    addRequest = 0,
    onPlaybackStart = () => undefined,
  }: {
    filter?: SoundscapeFilter;
    addRequest?: number;
    onPlaybackStart?: () => void;
  } = $props();
  let handledAddRequest = $state(0);

  const generated = $derived(orderedGeneratedSounds(soundscape.definitions));
  const local = $derived(soundscape.definitions.filter((entry) => entry.sourceKind === "local-loop"));
  const showPresets = $derived(filter !== "layers");
  const showLayers = $derived(filter !== "presets");

  async function addLoop(replace?: MusicSoundscapeDefinition): Promise<void> {
    try {
      const path = await pickSoundscapeFile();
      if (!path || !soundscape.catalogReady) return;
      if (!replace) {
        pendingLayerPath = path;
        soundNameDraft = (path.split(/[\\/]/).pop() ?? t("music.soundscape.localLoop")).replace(/\.[^.]+$/, "");
        soundIconDraft = NEW_LAYER_ICON;
        return;
      }
      await soundscape.saveDefinition({
        id: replace.id,
        sourceKind: "local-loop",
        generatedKind: null,
        bundledIdentity: null,
        name: replace.name,
        icon: replace.icon,
        localPath: path,
        expectedVersion: replace.version,
        updatedAtMs: Date.now(),
      });
    } catch (error) {
      console.error("Could not add soundscape sound", error);
    }
  }

  async function saveSound(): Promise<void> {
    const path = pendingLayerPath;
    const name = soundNameDraft.trim();
    if (!path || !name) return;
    try {
      await soundscape.saveDefinition({
        id: crypto.randomUUID(),
        sourceKind: "local-loop",
        generatedKind: null,
        bundledIdentity: null,
        name,
        icon: soundIconDraft,
        localPath: path,
        expectedVersion: null,
        updatedAtMs: Date.now(),
      });
      pendingLayerPath = null;
    } catch (error) {
      console.error("Could not save soundscape sound", error);
    }
  }

  $effect(() => {
    if (addRequest <= handledAddRequest) return;
    handledAddRequest = addRequest;
    presetEditor = null;
    void addLoop();
  });

  // Choosing another section in the sidebar leaves the preset editor.
  $effect(() => {
    void filter;
    untrack(() => presetEditor = null);
  });

  /** Plays a layer on its own, or stops it when it already plays. */
  function playLayer(definition: MusicSoundscapeDefinition): void {
    if (soundscape.playingLayerId !== definition.id) onPlaybackStart();
    void soundscape.playLayer(definition);
  }
</script>

<div class="h-full min-h-0 overflow-y-auto px-4 py-4" aria-busy={soundscape.loading || soundscape.saving} data-music-scrollable="true">
  <div class="@container flex max-w-2xl flex-col gap-7">
    <div class="flex flex-col gap-1 text-xs">
      <MusicSoundscapeActiveLevel idPrefix="builder-soundscape" />
      {#if soundscape.error}
        <Notice class="text-xs" message={t("music.soundscape.genericError")} />
      {/if}
    </div>

    {#if showPresets}
      <section aria-labelledby="soundscape-presets">
        <div class="mb-2 flex h-8 items-center gap-2">
          <h2 id="soundscape-presets" class="min-w-0 flex-1 truncate text-xs font-semibold">{t("music.soundscape.choice.presets")}</h2>
        </div>
        <div class="flex flex-wrap gap-1.5 text-xs">
          {#each soundscape.presets as preset (preset.id)}
            <MusicSoundscapePresetTile {preset} {onPlaybackStart} onEdit={(entry) => presetEditor = { preset: entry }} onRemove={(entry) => pendingPresetDelete = entry} />
          {/each}
        </div>
        <button type="button" disabled={!soundscape.persisted || soundscape.saving} class={[SOUNDSCAPE_CHIP_CLASS, "mt-1.5 text-xs text-muted-foreground hover:text-foreground"]} onclick={() => presetEditor = { preset: null }}><Plus size={14} class="shrink-0" /><span class="truncate">{t("music.soundscape.preset.new")}</span></button>
      </section>
    {/if}

    {#if showLayers}
      <section aria-labelledby="soundscape-layers">
        <div class="mb-2 flex h-8 items-center gap-2"><h2 id="soundscape-layers" class="min-w-0 flex-1 truncate text-xs font-semibold">{t("music.soundscape.layers")}</h2></div>
        <div class="flex flex-wrap gap-1.5 text-xs">
          {#each generated as definition (definition.id)}
            {@const playing = soundscape.playingLayerId === definition.id}
            {@const name = soundDisplayName(definition, t)}
            <MusicSoundscapeChip icon={definition.icon} label={name} active={playing} ariaLabel={playing ? t("music.soundscape.stopLayer", name) : t("music.soundscape.play", name)} tooltip={playing ? t("music.soundscape.stopHint") : undefined} disabled={soundscape.saving} onclick={() => playLayer(definition)} />
          {/each}
          {#each local as definition (definition.id)}
            <MusicSoundscapeLocalRow {definition} onRepair={(entry) => { void addLoop(entry); }} onRemove={(entry) => pendingDelete = entry} {onPlaybackStart} />
          {/each}
        </div>
        {#if pendingLayerPath}
          <form class="mt-1.5 rounded-xl bg-secondary/35 p-3" onsubmit={(event) => { event.preventDefault(); void saveSound(); }}>
            <div class="flex items-center gap-2">
              <IconPicker value={soundIconDraft} onChange={(value) => soundIconDraft = value} ariaLabel={t("music.soundscape.soundIcon")} showUpload={false} showRemove={false}>
                {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-background text-foreground hover:bg-accent" aria-label={t("music.soundscape.soundIcon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeIcon icon={soundIconDraft} /></button>{/snippet}
              </IconPicker>
              <input class="field h-9 min-w-0 flex-1 px-3 text-xs" bind:value={soundNameDraft} aria-label={t("music.soundscape.name")} placeholder={t("music.soundscape.name")} maxlength="200" />
            </div>
            <div class="mt-2 flex justify-end gap-2"><button type="button" class="h-8 rounded-lg px-3 text-xs hover:bg-accent" onclick={() => pendingLayerPath = null}>{t("common.cancel")}</button><button type="submit" disabled={!soundNameDraft.trim() || soundscape.saving} class="h-8 rounded-lg bg-primary px-3 text-xs text-primary-foreground disabled:opacity-40">{t("music.soundscape.addLoop")}</button></div>
          </form>
        {:else}
          <button type="button" disabled={!soundscape.catalogReady} class={[SOUNDSCAPE_CHIP_CLASS, "mt-1.5 text-xs text-muted-foreground hover:text-foreground"]} onclick={() => { void addLoop(); }}><Plus size={14} class="shrink-0" /><span class="truncate">{t("music.soundscape.addLoop")}</span></button>
        {/if}
      </section>
    {/if}
  </div>
</div>

{#if pendingDelete}
  <ConfirmDialog title={t("music.soundscape.removeTitle")} message={t("music.soundscape.removeMessage", pendingDelete.name)} confirmLabel={t("music.soundscape.remove")} cancelLabel={t("common.cancel")} onConfirm={() => { const definition = pendingDelete; pendingDelete = null; if (definition) void soundscape.removeDefinition(definition).catch((error) => console.error("Could not remove soundscape sound", error)); }} onCancel={() => pendingDelete = null} />
{/if}
{#if presetEditor}
  {#key presetEditor}
    <MusicSoundscapePresetEditor preset={presetEditor.preset} onDone={() => presetEditor = null} onRemove={(entry) => pendingPresetDelete = entry} />
  {/key}
{/if}
{#if pendingPresetDelete}
  <!-- Portaled after the preset editor modal so the confirmation stacks above it. -->
  <div use:portal>
    <ConfirmDialog title={t("music.soundscape.preset.removeTitle")} message={t("music.soundscape.preset.removeMessage", presetDisplayName(pendingPresetDelete, t))} confirmLabel={t("music.soundscape.preset.remove")} cancelLabel={t("common.cancel")} onConfirm={() => { const preset = pendingPresetDelete; pendingPresetDelete = null; if (presetEditor?.preset?.id === preset?.id) presetEditor = null; if (preset) void soundscape.removePreset(preset).catch((error) => console.error("Could not remove soundscape preset", error)); }} onCancel={() => pendingPresetDelete = null} />
  </div>
{/if}
