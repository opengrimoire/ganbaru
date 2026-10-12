<script lang="ts">
  import AudioLines from "@lucide/svelte/icons/audio-lines";
  import CloudHail from "@lucide/svelte/icons/cloud-hail";
  import CloudRain from "@lucide/svelte/icons/cloud-rain";
  import CloudRainWind from "@lucide/svelte/icons/cloud-rain-wind";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import { onMount, tick } from "svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { orderedGeneratedSounds } from "$lib/music/soundscape/presentation";
  import { centeredSoundscapePanelLeft } from "$lib/music/soundscape/popover-position";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";
  import MusicSoundscapeSectionControls from "./MusicSoundscapeSectionControls.svelte";

  let { onOpenSoundscapes }: { onOpenSoundscapes: () => void } = $props();
  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  let root = $state<HTMLElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);
  let popover = $state<HTMLElement | null>(null);
  let popoverLeft = $state<number | null>(null);
  let open = $state(false);
  const active = $derived(soundscape.activeDefinition);
  const selectedIds = $derived(soundscape.persisted?.activeIds ?? []);
  const activeName = $derived(selectedIds.length > 1 ? t("music.soundscape.selectedCount", selectedIds.length) : active?.generatedKind ? t(`music.soundscape.rainName.${active.generatedKind}`) : active?.name ?? "");
  const generated = $derived(orderedGeneratedSounds(soundscape.definitions));
  const local = $derived(soundscape.definitions.filter((entry) => entry.sourceKind === "local-loop"));
  const ungrouped = $derived(local.filter((entry) => !entry.groupId || !soundscape.groups.some((group) => group.id === entry.groupId)));
  const volume = $derived(soundscape.snapshot.volume);
  const playing = $derived(soundscape.snapshot.status === "playing");

  onMount(() => {
    const close = (event: PointerEvent) => {
      if (!open || !root || root.contains(event.target as Node)) return;
      if (event.target instanceof Element && event.target.closest('[data-soundscape-subpanel="player-soundscape"]')) return;
      open = false;
    };
    const closeWithKeyboard = (event: KeyboardEvent) => {
      if (!open || event.key !== "Escape" || document.querySelector('[data-soundscape-subpanel="player-soundscape"]')) return;
      event.preventDefault();
      event.stopPropagation();
      open = false;
      trigger?.focus();
    };
    window.addEventListener("pointerdown", close, true);
    window.addEventListener("keydown", closeWithKeyboard, true);
    window.addEventListener("resize", positionPopover);
    return () => { window.removeEventListener("pointerdown", close, true); window.removeEventListener("keydown", closeWithKeyboard, true); window.removeEventListener("resize", positionPopover); };
  });

  function positionPopover(): void {
    if (!root || !trigger || !popover) return;
    const triggerRect = trigger.getBoundingClientRect();
    popoverLeft = centeredSoundscapePanelLeft(triggerRect.left, triggerRect.width, popover.offsetWidth, window.innerWidth) - root.getBoundingClientRect().left;
  }

  async function toggle(): Promise<void> {
    open = !open;
    if (!open) return;
    popoverLeft = null;
    await tick();
    positionPopover();
    await tick();
    (popover?.querySelector<HTMLElement>("button:not(:disabled), input:not(:disabled)") ?? popover)?.focus();
  }

  function handleFocusOut(event: FocusEvent): void {
    if (!open || !(event.relatedTarget instanceof Node) || root?.contains(event.relatedTarget)) return;
    if (event.relatedTarget instanceof Element && event.relatedTarget.closest('[data-soundscape-subpanel="player-soundscape"]')) return;
    open = false;
  }

  function toggleActive(): void {
    if (!active) return;
    void soundscape.togglePlayback();
  }
</script>

<div bind:this={root} class="relative" onfocusout={handleFocusOut}>
  <button bind:this={trigger} type="button" class="relative inline-flex h-9 w-9 items-center justify-center rounded-md bg-secondary text-secondary-foreground transition-colors hover:bg-accent" aria-haspopup="dialog" aria-expanded={open} aria-label={t("music.soundscape.controls")} onclick={() => { void toggle(); }}>
    <AudioLines size={15} strokeWidth={1.5} />
    {#if playing}<span class="absolute bottom-1 right-1 h-1.5 w-1.5 rounded-full bg-primary" aria-hidden="true"></span>{/if}
  </button>
  {#if open}
    <div bind:this={popover} tabindex="-1" class="soundscape-popover surface-floating absolute bottom-full z-40 mb-2 flex w-floating-lg max-h-[calc(100vh-1rem)] flex-col overflow-hidden" style={popoverLeft === null ? "visibility:hidden" : `--soundscape-popover-left:${popoverLeft}px`} role="dialog" aria-label={t("music.soundscape.controls")}>
      <div use:scrollEdgeFadeAction class="min-h-0 flex-1 overflow-y-auto p-3">
        <div class="flex min-h-9 items-center gap-2 px-1">
          <Volume2 size={15} class="shrink-0 text-muted-foreground" />
          <input id="soundscape-volume" class="min-w-0 flex-1 accent-primary disabled:opacity-40" type="range" min="0" max="1" step="0.01" value={volume} aria-label={t("music.soundscape.volume")} disabled={!soundscape.persisted || soundscape.loading} oninput={(event) => { void soundscape.setVolume(Number(event.currentTarget.value)); }} />
          <span class="w-9 shrink-0 text-right text-panel-detail tabular-nums text-muted-foreground">{Math.round(volume * 100)}%</span>
          <button type="button" disabled={!active || soundscape.saving} aria-label={playing ? t("music.soundscape.pause") : t("music.soundscape.play", activeName)} class="grid h-8 w-8 shrink-0 place-items-center rounded-floating-item bg-secondary hover:bg-accent disabled:cursor-not-allowed disabled:opacity-40" onclick={toggleActive}>{#if playing}<Pause size={15} />{:else}<Play size={15} />{/if}</button>
        </div>
        {#if soundscape.error}<Notice class="mt-1 px-1 text-panel-detail" message={t("music.soundscape.genericError")} />{/if}
        <div class="mt-3 px-1"><MusicSoundscapeSectionControls title={t("music.soundscape.generated")} section="generated" idPrefix="player-soundscape" compact /></div>
        <div class="grid grid-cols-3 gap-1.5">
          {#each generated as definition (definition.id)}
            {@const selected = selectedIds.includes(definition.id)}
            <button type="button" aria-pressed={selected} aria-label={selected && soundscape.persisted?.multipleEnabled ? t("music.soundscape.stop", t(`music.soundscape.rainName.${definition.generatedKind ?? "brown"}`)) : selected && playing ? t("music.soundscape.pause") : t("music.soundscape.play", t(`music.soundscape.rainName.${definition.generatedKind ?? "brown"}`))} disabled={soundscape.saving} class={selected ? "flex min-h-16 flex-col items-center justify-center gap-1 rounded-floating-item bg-primary/10 text-panel-detail transition-colors hover:bg-accent/70 disabled:opacity-50" : "flex min-h-16 flex-col items-center justify-center gap-1 rounded-floating-item text-panel-detail transition-colors hover:bg-accent/70 disabled:opacity-50"} onclick={() => { void soundscape.toggleSelection(definition.id); }}>
              {#if definition.generatedKind === "brown"}<CloudHail size={19} strokeWidth={1.5} />{:else if definition.generatedKind === "pink"}<CloudRain size={19} strokeWidth={1.5} />{:else}<CloudRainWind size={19} strokeWidth={1.5} />{/if}
              <span>{t(`music.soundscape.rainName.${definition.generatedKind ?? "brown"}`)}</span>
            </button>
          {/each}
        </div>
        {#if local.length > 0}
          <div class="mt-3 px-1"><MusicSoundscapeSectionControls title={t("music.soundscape.localLoops")} section="local" idPrefix="player-soundscape" compact /></div>
          <div use:scrollEdgeFadeAction class="max-h-40 space-y-0.5 overflow-y-auto" data-music-scrollable="true">
            {#each soundscape.groups as group (group.id)}
              {#if local.some((entry) => entry.groupId === group.id)}
                <p class="flex items-center gap-1.5 px-2 pt-2 text-panel-detail text-muted-foreground"><MusicSoundscapeGroupIcon icon={group.icon} size={12} />{group.name}</p>
                <div class="grid grid-cols-3 gap-1.5">
                  {#each local.filter((entry) => entry.groupId === group.id) as definition (definition.id)}
                    <button type="button" aria-pressed={selectedIds.includes(definition.id)} aria-label={selectedIds.includes(definition.id) && soundscape.persisted?.multipleEnabled ? t("music.soundscape.stop", definition.name) : selectedIds.includes(definition.id) && playing ? t("music.soundscape.pause") : t("music.soundscape.play", definition.name)} disabled={definition.availability !== "available" || soundscape.saving} class={selectedIds.includes(definition.id) ? "flex min-h-16 min-w-0 flex-col items-center justify-center gap-1 rounded-floating-item bg-primary/10 px-1 text-panel-detail transition-colors hover:bg-accent/70 disabled:opacity-50" : "flex min-h-16 min-w-0 flex-col items-center justify-center gap-1 rounded-floating-item px-1 text-panel-detail transition-colors hover:bg-accent/70 disabled:opacity-50"} onclick={() => { void soundscape.toggleSelection(definition.id); }}><MusicSoundscapeGroupIcon icon={definition.icon} size={19} /><span class="max-w-full truncate">{definition.name}</span></button>
                  {/each}
                </div>
              {/if}
            {/each}
            {#if ungrouped.length > 0 && soundscape.groups.length > 0}<p class="px-2 pt-2 text-panel-detail text-muted-foreground">{t("music.soundscape.ungrouped")}</p>{/if}
            <div class="grid grid-cols-3 gap-1.5">
              {#each ungrouped as definition (definition.id)}
                <button type="button" aria-pressed={selectedIds.includes(definition.id)} aria-label={selectedIds.includes(definition.id) && soundscape.persisted?.multipleEnabled ? t("music.soundscape.stop", definition.name) : selectedIds.includes(definition.id) && playing ? t("music.soundscape.pause") : t("music.soundscape.play", definition.name)} disabled={definition.availability !== "available" || soundscape.saving} class={selectedIds.includes(definition.id) ? "flex min-h-16 min-w-0 flex-col items-center justify-center gap-1 rounded-floating-item bg-primary/10 px-1 text-panel-detail transition-colors hover:bg-accent/70 disabled:opacity-50" : "flex min-h-16 min-w-0 flex-col items-center justify-center gap-1 rounded-floating-item px-1 text-panel-detail transition-colors hover:bg-accent/70 disabled:opacity-50"} onclick={() => { void soundscape.toggleSelection(definition.id); }}><MusicSoundscapeGroupIcon icon={definition.icon} size={19} /><span class="max-w-full truncate">{definition.name}</span></button>
              {/each}
            </div>
          </div>
        {/if}
        <button type="button" class="menu-item mt-3 justify-between text-muted-foreground hover:text-foreground" onclick={() => { open = false; onOpenSoundscapes(); }}><span>{t("music.soundscape.openBuilder")}</span><ChevronRight size={14} /></button>
      </div>
    </div>
  {/if}
</div>

<style>
  .soundscape-popover { left: var(--soundscape-popover-left, 0px); }

  @media (max-height: 260px) {
    .soundscape-popover { position: fixed; inset: 0.5rem; width: auto; max-height: none; margin: 0; }
  }
</style>
