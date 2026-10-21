<script lang="ts">
  import AudioLines from "@lucide/svelte/icons/audio-lines";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { onMount, tick } from "svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapePreset } from "$lib/music/soundscape/contracts";
  import { presetDisplayName } from "$lib/music/soundscape/presets";
  import { centeredSoundscapePanelLeft } from "$lib/music/soundscape/popover-position";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";
  import MusicSoundscapeActiveLevel from "./MusicSoundscapeActiveLevel.svelte";

  let { onOpenSoundscapes }: { onOpenSoundscapes: () => void } = $props();
  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  let root = $state<HTMLElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);
  let popover = $state<HTMLElement | null>(null);
  let popoverLeft = $state<number | null>(null);
  let open = $state(false);
  const active = $derived(soundscape.activePreset);
  const playing = $derived(soundscape.snapshot.status === "playing");

  onMount(() => {
    const close = (event: PointerEvent) => {
      if (!open || !root || root.contains(event.target as Node)) return;
      open = false;
    };
    const closeWithKeyboard = (event: KeyboardEvent) => {
      if (!open || event.key !== "Escape") return;
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
    // The dialog itself takes focus, so the panel's S and Spacebar shortcuts keep working until a control is chosen.
    popover?.focus();
  }

  /** Lets go of the level slider after a drag, like the volume slider, so panel shortcuts reach the popover again. */
  function releaseSliderFocus(event: PointerEvent): void {
    const focused = document.activeElement;
    if (focused instanceof HTMLInputElement && focused.type === "range" && event.currentTarget instanceof Node && event.currentTarget.contains(focused)) focused.blur();
  }

  function handleFocusOut(event: FocusEvent): void {
    if (!open || !(event.relatedTarget instanceof Node) || root?.contains(event.relatedTarget)) return;
    open = false;
  }

  /** Opens or closes the popover, for the panel's keyboard shortcut. */
  export function toggleOpen(): void {
    if (open && popover?.contains(document.activeElement)) trigger?.focus();
    void toggle();
  }

  /** Whether the popover is open, so the panel sends the Spacebar key to the soundscape instead of Music. */
  export function isOpen(): boolean {
    return open;
  }

  /** Pauses the playing preset or plays the chosen one again. */
  export function togglePlayback(): void {
    void soundscape.togglePlayback();
  }

  /** Plays a preset, or pauses it when it is the one already playing. */
  function choosePreset(preset: MusicSoundscapePreset): void {
    if (preset.id === active?.id && playing) void soundscape.togglePlayback();
    else void soundscape.playPreset(preset);
  }
</script>

<div bind:this={root} class="relative" data-floating-root onfocusout={handleFocusOut}>
  <button bind:this={trigger} type="button" class="relative inline-flex h-9 w-9 items-center justify-center rounded-md bg-secondary text-secondary-foreground transition-colors hover:bg-accent" aria-haspopup="dialog" aria-expanded={open} aria-label={t("music.soundscape.controls")} title={t("music.soundscape.controlsTitle")} onclick={() => { void toggle(); }}>
    <AudioLines size={15} strokeWidth={1.5} />
    {#if playing}<span class="absolute bottom-1 right-1 h-1.5 w-1.5 rounded-full bg-primary" aria-hidden="true"></span>{/if}
  </button>
  {#if open}
    <div bind:this={popover} tabindex="-1" class="soundscape-popover surface-floating absolute bottom-full z-40 mb-2 flex focus:outline-none w-floating-lg max-h-[calc(100vh-1rem)] flex-col overflow-hidden" style={popoverLeft === null ? "visibility:hidden" : `--soundscape-popover-left:${popoverLeft}px`} role="dialog" aria-label={t("music.soundscape.controls")}>
      <div use:scrollEdgeFadeAction class="flex min-h-0 flex-1 flex-col overflow-y-auto p-2">
        <div class="px-2 pt-2 pb-2.5" role="presentation" onpointerup={releaseSliderFocus} onpointercancel={releaseSliderFocus}><MusicSoundscapeActiveLevel idPrefix="player-soundscape" /></div>
        {#if soundscape.error}<Notice class="px-2 py-1" message={t("music.soundscape.genericError")} />{/if}
        {#if soundscape.presets.length > 0}
          <div class="flex flex-wrap gap-1.5">
            {#each soundscape.presets as preset (preset.id)}
              {@const selected = preset.id === active?.id && playing}
              {@const name = presetDisplayName(preset, t)}
              <button type="button" aria-pressed={selected} aria-label={selected ? t("music.soundscape.pause") : t("music.soundscape.playPreset", name)} data-app-tooltip={selected ? t("music.soundscape.stopHint") : undefined} disabled={!soundscape.presetIsPlayable(preset)} class={["inline-flex h-8 min-w-0 max-w-full items-center gap-2 rounded-floating-item px-2 transition-colors hover:bg-accent/70 disabled:opacity-50", selected && "bg-primary/10"]} onclick={() => choosePreset(preset)}><MusicSoundscapeGroupIcon icon={preset.icon} size={14} /><span class="truncate">{name}</span></button>
            {/each}
          </div>
        {/if}
        <button type="button" class="menu-item mt-1.5 justify-between" onclick={() => { open = false; onOpenSoundscapes(); }}><span>{t("music.soundscape.openBuilder")}</span><ChevronRight size={14} /></button>
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
