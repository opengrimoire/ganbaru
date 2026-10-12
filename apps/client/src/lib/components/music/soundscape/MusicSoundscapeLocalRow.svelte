<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import { onMount } from "svelte";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import { revealLocalFile } from "$lib/api/music";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapeDefinition } from "$lib/music/soundscape/contracts";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";

  let { definition, onRepair, onRemove, onPlaybackStart }: {
    definition: MusicSoundscapeDefinition;
    onRepair: (definition: MusicSoundscapeDefinition) => void;
    onRemove: (definition: MusicSoundscapeDefinition) => void;
    onPlaybackStart: () => void;
  } = $props();
  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  let root = $state<HTMLElement | null>(null);
  let menuOpen = $state(false);
  let editing = $state(false);
  let nameDraft = $state("");
  let iconDraft = $state("lucide:audio-lines");
  const selected = $derived(soundscape.persisted?.activeIds.includes(definition.id) ?? false);
  const playing = $derived(selected && soundscape.snapshot.status === "playing");
  const layered = $derived(soundscape.persisted?.multipleEnabled ?? false);

  onMount(() => {
    const close = (event: PointerEvent) => { if (menuOpen && root && !root.contains(event.target as Node)) menuOpen = false; };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || root?.querySelector('[aria-haspopup="dialog"][aria-expanded="true"]')) return;
      menuOpen = false;
      editing = false;
    };
    window.addEventListener("pointerdown", close, true);
    window.addEventListener("keydown", escape, true);
    return () => { window.removeEventListener("pointerdown", close, true); window.removeEventListener("keydown", escape, true); };
  });

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
        groupId: definition.groupId,
        localPath: definition.localPath,
        expectedVersion: definition.version,
        updatedAtMs: Date.now(),
      });
      editing = false;
    } catch (error) {
      console.error("Could not edit background sound", error);
    }
  }

  async function moveToGroup(groupId: string | null): Promise<void> {
    menuOpen = false;
    if (definition.groupId === groupId) return;
    try {
      await soundscape.saveDefinition({
        id: definition.id,
        sourceKind: definition.sourceKind,
        generatedKind: definition.generatedKind,
        bundledIdentity: definition.bundledIdentity,
        name: definition.name,
        icon: definition.icon,
        groupId,
        localPath: definition.localPath,
        expectedVersion: definition.version,
        updatedAtMs: Date.now(),
      });
    } catch (error) {
      console.error("Could not move background sound", error);
    }
  }
</script>

<div bind:this={root} class="relative min-w-0">
  {#if editing}
    <form class="flex min-h-28 flex-col gap-2 rounded-xl bg-secondary/40 p-2" onsubmit={(event) => { event.preventDefault(); void saveDetails(); }}>
      <div class="flex items-center gap-1.5">
        <IconPicker value={iconDraft} onChange={(value) => iconDraft = value} ariaLabel={t("music.soundscape.soundIcon")} showUpload={false} showRemove={false}>
          {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-background text-foreground hover:bg-accent" aria-label={t("music.soundscape.soundIcon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeGroupIcon icon={iconDraft} size={16} /></button>{/snippet}
        </IconPicker>
        <input class="field h-8 min-w-0 flex-1 px-2 text-xs" bind:value={nameDraft} aria-label={t("music.soundscape.name")} maxlength="200" />
      </div>
      <div class="flex justify-end gap-1"><button type="button" class="h-7 rounded-md px-2 text-xs hover:bg-accent" onclick={() => editing = false}>{t("common.cancel")}</button><button type="submit" disabled={!nameDraft.trim() || soundscape.saving} class="h-7 rounded-md bg-primary px-2 text-xs text-primary-foreground disabled:opacity-40">{t("music.soundscape.saveName")}</button></div>
    </form>
  {:else}
    <button type="button" aria-pressed={selected} aria-label={definition.availability !== "available" ? t("music.soundscape.repair") : selected && layered ? t("music.soundscape.stop", definition.name) : playing ? t("music.soundscape.pause") : t("music.soundscape.play", definition.name)} disabled={soundscape.saving} class={selected ? "flex min-h-28 w-full min-w-0 flex-col items-center justify-center gap-2 rounded-xl bg-primary/10 px-2 text-center hover:bg-primary/15 disabled:opacity-50" : "flex min-h-28 w-full min-w-0 flex-col items-center justify-center gap-2 rounded-xl bg-secondary/40 px-2 text-center hover:bg-secondary/70 disabled:opacity-50"} onclick={() => { if (definition.availability !== "available") { onRepair(definition); return; } if (!playing) onPlaybackStart(); void soundscape.toggleSelection(definition.id); }}>
      <MusicSoundscapeGroupIcon icon={definition.icon} size={27} />
      <span class="max-w-full truncate text-xs font-medium">{definition.name}</span>
      {#if definition.availability !== "available"}<span class="text-[0.68rem] text-muted-foreground">{t("music.soundscape.needsRepair")}</span>{/if}
    </button>
    <button type="button" aria-label={t("music.soundscape.soundActions", definition.name)} aria-expanded={menuOpen} class="absolute right-1 top-1 z-10 grid h-7 w-7 place-items-center rounded-lg text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => menuOpen = !menuOpen}><Ellipsis size={16} /></button>
  {/if}
  {#if menuOpen}
    <div class="surface-floating surface-floating-body absolute right-1 top-8 z-20 flex min-w-36 flex-col">
      <button type="button" class="menu-item" onclick={() => { menuOpen = false; nameDraft = definition.name; iconDraft = definition.icon; editing = true; }}>{t("music.soundscape.editSound")}</button>
      {#if definition.localPath}<button type="button" class="menu-item" onclick={() => { menuOpen = false; if (definition.localPath) void revealLocalFile(definition.localPath); }}>{t("music.soundscape.showFile")}</button>{/if}
      {#if soundscape.groups.length > 0}
        <p class="menu-label">{t("music.soundscape.moveToGroup")}</p>
        <div use:scrollEdgeFadeAction class="max-h-32 overflow-y-auto" data-music-scrollable="true">
          <button type="button" class="menu-item" aria-current={definition.groupId === null ? "true" : undefined} onclick={() => { void moveToGroup(null); }}><span class="min-w-0 flex-1 truncate">{t("music.soundscape.ungrouped")}</span>{#if definition.groupId === null}<Check size={14} />{/if}</button>
          {#each soundscape.groups as group (group.id)}<button type="button" class="menu-item" aria-current={definition.groupId === group.id ? "true" : undefined} onclick={() => { void moveToGroup(group.id); }}><span class="min-w-0 flex-1 truncate">{group.name}</span>{#if definition.groupId === group.id}<Check size={14} />{/if}</button>{/each}
        </div>
      {/if}
      <div class="menu-separator" role="separator"></div>
      <button type="button" class="menu-item menu-item-destructive" onclick={() => { menuOpen = false; onRemove(definition); }}>{t("music.soundscape.remove")}</button>
    </div>
  {/if}
</div>
