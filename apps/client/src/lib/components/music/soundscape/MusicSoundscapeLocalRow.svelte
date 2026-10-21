<script lang="ts">
  import { onMount } from "svelte";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import { revealLocalFile } from "$lib/api/music";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapeDefinition } from "$lib/music/soundscape/contracts";
  import { NEW_LAYER_ICON } from "$lib/music/soundscape/presentation";
  import { singleSoundPreset } from "$lib/music/soundscape/presets";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicSoundscapeChip from "./MusicSoundscapeChip.svelte";
  import MusicSoundscapeIcon from "./MusicSoundscapeIcon.svelte";

  /** Builder option for a local layer; editing its name and icon takes a full row of the option list. */
  let { definition, onRepair, onRemove, onPlaybackStart }: {
    definition: MusicSoundscapeDefinition;
    onRepair: (definition: MusicSoundscapeDefinition) => void;
    onRemove: (definition: MusicSoundscapeDefinition) => void;
    onPlaybackStart: () => void;
  } = $props();
  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  let form = $state<HTMLElement | null>(null);
  let editing = $state(false);
  let nameDraft = $state("");
  let iconDraft = $state(NEW_LAYER_ICON);
  const solo = $derived(singleSoundPreset(soundscape.presets, definition.id));
  const playing = $derived(soundscape.playingLayerId === definition.id);
  const available = $derived(definition.availability === "available");

  onMount(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !editing || form?.querySelector('[aria-haspopup="dialog"][aria-expanded="true"]')) return;
      editing = false;
    };
    window.addEventListener("keydown", escape, true);
    return () => window.removeEventListener("keydown", escape, true);
  });

  function choose(): void {
    if (!available) {
      onRepair(definition);
      return;
    }
    if (!playing) onPlaybackStart();
    void soundscape.playLayer(definition);
  }

  function startEditing(): void {
    nameDraft = definition.name;
    iconDraft = definition.icon;
    editing = true;
  }

  async function saveDetails(): Promise<void> {
    const name = nameDraft.trim();
    if (!name) return;
    try {
      await soundscape.saveDefinition({
        id: definition.id,
        sourceKind: definition.sourceKind,
        generatedKind: definition.generatedKind,
        bundledIdentity: definition.bundledIdentity,
        name,
        icon: iconDraft,
        localPath: definition.localPath,
        expectedVersion: definition.version,
        updatedAtMs: Date.now(),
      });
      editing = false;
    } catch (error) {
      console.error("Could not edit soundscape sound", error);
    }
  }
</script>

{#if editing}
  <form bind:this={form} class="flex basis-full items-center gap-1.5" onsubmit={(event) => { event.preventDefault(); void saveDetails(); }}>
    <IconPicker value={iconDraft} onChange={(value) => iconDraft = value} ariaLabel={t("music.soundscape.soundIcon")} showUpload={false} showRemove={false}>
      {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-8 w-8 shrink-0 place-items-center rounded-floating-item bg-secondary/40 text-foreground hover:bg-accent" aria-label={t("music.soundscape.soundIcon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeIcon icon={iconDraft} size={14} /></button>{/snippet}
    </IconPicker>
    <input class="field h-8 min-w-0 flex-1 px-2 text-xs" bind:value={nameDraft} aria-label={t("music.soundscape.name")} maxlength="200" />
    <button type="button" class="h-8 shrink-0 rounded-floating-item px-2 hover:bg-accent" onclick={() => editing = false}>{t("common.cancel")}</button>
    <button type="submit" disabled={!nameDraft.trim() || soundscape.saving} class="h-8 shrink-0 rounded-floating-item bg-primary px-2 text-primary-foreground disabled:opacity-40">{t("music.soundscape.saveName")}</button>
  </form>
{:else}
  <MusicSoundscapeChip
    icon={definition.icon}
    label={definition.name}
    active={playing}
    muted={!available}
    ariaLabel={!available ? t("music.soundscape.repair") : playing ? t("music.soundscape.stopLayer", definition.name) : t("music.soundscape.play", definition.name)}
    title={available ? undefined : t("music.soundscape.needsRepair")}
    disabled={soundscape.saving}
    onclick={choose}
    menuLabel={t("music.soundscape.soundActions", definition.name)}
  >
    {#snippet menu(close)}
      <button type="button" role="menuitem" class="menu-item" onclick={() => { close(); startEditing(); }}>{t("music.soundscape.editSound")}</button>
      {#if !solo && available}<button type="button" role="menuitem" class="menu-item" disabled={soundscape.saving} onclick={() => { close(); void soundscape.makePresetFromSound(definition, definition.name).catch((error) => console.error("Could not make soundscape preset", error)); }}>{t("music.soundscape.makePreset")}</button>{/if}
      {#if definition.localPath}<button type="button" role="menuitem" class="menu-item" onclick={() => { close(); if (definition.localPath) void revealLocalFile(definition.localPath); }}>{t("music.soundscape.showFile")}</button>{/if}
      <div class="menu-separator" role="separator"></div>
      <button type="button" role="menuitem" class="menu-item menu-item-destructive" onclick={() => { close(); onRemove(definition); }}>{t("music.soundscape.remove")}</button>
    {/snippet}
  </MusicSoundscapeChip>
{/if}
