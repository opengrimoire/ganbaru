<script lang="ts">
  import Layers2 from "@lucide/svelte/icons/layers-2";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import { tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import { portal } from "$lib/utils/portal";
  import type { SelectPopoverGeometry } from "$lib/utils/select-popover-position";
  import { pickSoundscapePopoverGeometry, SOUNDSCAPE_POPOVER_WIDTH } from "$lib/music/soundscape/popover-position";
  import { soundscapeLevelFromAdjustment, soundscapeVolumeAdjustment } from "$lib/music/soundscape/volume-adjustment";

  let { title, section, idPrefix, compact = false }: { title: string; section: "generated" | "local"; idPrefix: string; compact?: boolean } = $props();
  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  let panel = $state<"mode" | "balance" | null>(null);
  let modeTrigger = $state<HTMLButtonElement | null>(null);
  let balanceTrigger = $state<HTMLButtonElement | null>(null);
  let popover = $state<HTMLDivElement | null>(null);
  let geometry = $state<SelectPopoverGeometry | null>(null);
  const level = $derived(soundscape.sectionLevels[section]);
  const adjustment = $derived(soundscapeVolumeAdjustment(level));
  const layered = $derived(soundscape.persisted?.multipleEnabled ?? false);

  function activeTrigger(): HTMLButtonElement | null {
    return panel === "mode" ? modeTrigger : balanceTrigger;
  }

  async function toggle(next: "mode" | "balance"): Promise<void> {
    if (panel === next) {
      panel = null;
      return;
    }
    panel = next;
    geometry = null;
    await tick();
    position();
    popover?.querySelector<HTMLElement>("button:not(:disabled), input:not(:disabled)")?.focus();
  }

  function position(): void {
    if (!modeTrigger || !balanceTrigger || !panel) return;
    const first = modeTrigger.getBoundingClientRect();
    const last = balanceTrigger.getBoundingClientRect();
    const rect = { top: Math.min(first.top, last.top), bottom: Math.max(first.bottom, last.bottom), left: first.left, right: last.right, width: last.right - first.left, height: Math.max(first.bottom, last.bottom) - Math.min(first.top, last.top) };
    const contentHeight = popover?.offsetHeight || 64;
    geometry = pickSoundscapePopoverGeometry(
      rect,
      { top: 0, left: 0, right: window.innerWidth, bottom: window.innerHeight, width: window.innerWidth, height: window.innerHeight },
      SOUNDSCAPE_POPOVER_WIDTH,
      contentHeight,
    );
  }

  function popoverStyle(): string {
    if (!geometry) return `visibility:hidden;top:0;left:0;width:min(${SOUNDSCAPE_POPOVER_WIDTH}px,calc(100vw - 16px))`;
    return `top:${geometry.top}px;left:${geometry.left}px;width:${geometry.width ?? geometry.minWidth}px;max-height:${geometry.maxHeight}px`;
  }

  $effect(() => {
    if (!panel) return;
    const pointer = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      if (!activeTrigger()?.contains(event.target) && !popover?.contains(event.target)) panel = null;
    };
    const keydown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      const trigger = activeTrigger();
      panel = null;
      trigger?.focus();
    };
    window.addEventListener("pointerdown", pointer, true);
    window.addEventListener("keydown", keydown, true);
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    return () => {
      window.removeEventListener("pointerdown", pointer, true);
      window.removeEventListener("keydown", keydown, true);
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
    };
  });
</script>

<div class="min-w-0 flex-1">
  <div class={compact ? "flex h-7 items-center gap-0.5" : "flex h-8 items-center gap-0.5"}>
    <h2 class="min-w-0 flex-1 truncate text-xs font-semibold">{title}</h2>
    <button bind:this={modeTrigger} type="button" aria-label={t("music.soundscape.sectionMode")} aria-haspopup="dialog" aria-expanded={panel === "mode"} disabled={!soundscape.persisted || soundscape.saving} class={panel === "mode" ? "grid h-6 w-6 shrink-0 place-items-center rounded-md bg-accent text-foreground" : "grid h-6 w-6 shrink-0 place-items-center rounded-md text-foreground hover:bg-accent disabled:opacity-40"} onclick={() => { void toggle("mode"); }}><Layers2 size={13} strokeWidth={1.5} /></button>
    <button bind:this={balanceTrigger} type="button" aria-label={t("music.soundscape.sectionLevel")} aria-haspopup="dialog" aria-expanded={panel === "balance"} disabled={!soundscape.persisted} class={panel === "balance" || adjustment !== 0 ? "grid h-6 w-6 shrink-0 place-items-center rounded-md bg-accent text-foreground" : "grid h-6 w-6 shrink-0 place-items-center rounded-md text-foreground hover:bg-accent disabled:opacity-40"} onclick={() => { void toggle("balance"); }}><Volume2 size={13} strokeWidth={1.5} /></button>
  </div>
</div>

{#if panel}
  <div use:portal bind:this={popover} data-soundscape-subpanel={idPrefix} role="dialog" aria-label={panel === "mode" ? t("music.soundscape.sectionMode") : t("music.soundscape.sectionLevel")} style={popoverStyle()} class="surface-floating fixed z-120 flex h-20 select-none flex-col justify-between overflow-y-auto p-3" onfocusout={(event) => { if (event.relatedTarget instanceof Node && (popover?.contains(event.relatedTarget) || activeTrigger()?.contains(event.relatedTarget))) return; panel = null; }}>
  <div class="flex min-w-0 items-center justify-between gap-2">
    <p class={panel === "balance" ? "translate-y-0.5 truncate font-semibold" : "truncate font-semibold"}>{panel === "mode" ? t("music.soundscape.sectionMode") : t("music.soundscape.sectionLevel")}</p>
    {#if panel === "balance"}<span class="shrink-0 translate-y-0.5 text-panel-detail tabular-nums text-muted-foreground">{adjustment > 0 ? "+" : ""}{adjustment}%</span>{/if}
  </div>
  {#if panel === "mode"}
    <div class="flex h-8 items-center gap-1">
      <button type="button" aria-pressed={!layered} class={!layered ? "h-8 min-w-0 flex-1 rounded-floating-item bg-secondary font-medium" : "h-8 min-w-0 flex-1 rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground"} onclick={() => { void soundscape.setMultipleEnabled(false); }}>{t("music.soundscape.singleMode")}</button>
      <button type="button" aria-pressed={layered} class={layered ? "h-8 min-w-0 flex-1 rounded-floating-item bg-secondary font-medium" : "h-8 min-w-0 flex-1 rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground"} onclick={() => { void soundscape.setMultipleEnabled(true); }}>{t("music.soundscape.multipleMode")}</button>
    </div>
  {:else if panel === "balance"}
    <div class="flex h-8 translate-y-0.5 items-center gap-2">
      <span class="shrink-0 text-panel-detail tabular-nums text-muted-foreground">-100%</span>
      <label class="sr-only" for={`${idPrefix}-${section}-balance`}>{t("music.soundscape.sectionLevel")}</label>
      <input id={`${idPrefix}-${section}-balance`} type="range" min="-100" max="100" step="5" value={adjustment} disabled={!soundscape.persisted} class="soundscape-adjustment-slider min-w-8 flex-1 disabled:opacity-40" style={`--soundscape-adjustment-start: ${Math.min(50, (adjustment + 100) / 2)}%; --soundscape-adjustment-end: ${Math.max(50, (adjustment + 100) / 2)}%;`} aria-valuetext={`${adjustment > 0 ? "+" : ""}${adjustment}%`} aria-describedby={`${idPrefix}-${section}-balance-hint`} oninput={(event) => { void soundscape.setSectionLevel(section, soundscapeLevelFromAdjustment(Number(event.currentTarget.value))); }} />
      <span id={`${idPrefix}-${section}-balance-hint`} class="sr-only">{t("music.soundscape.balanceHint")}</span>
      <span class="shrink-0 text-panel-detail tabular-nums text-muted-foreground">100%</span>
    </div>
  {/if}
  </div>
{/if}

<style>
  .soundscape-adjustment-slider {
    --soundscape-adjustment-thumb-size: 0.875rem;
    --soundscape-adjustment-track-height: 0.25rem;
    --soundscape-adjustment-track-color: color-mix(in srgb, var(--foreground) 22%, transparent);
    --soundscape-adjustment-thumb-border: color-mix(in srgb, var(--foreground) 18%, transparent);

    height: var(--soundscape-adjustment-thumb-size);
    appearance: none;
    cursor: pointer;
    background: linear-gradient(
      to right,
      var(--soundscape-adjustment-track-color) 0%,
      var(--soundscape-adjustment-track-color) var(--soundscape-adjustment-start),
      var(--primary) var(--soundscape-adjustment-start),
      var(--primary) var(--soundscape-adjustment-end),
      var(--soundscape-adjustment-track-color) var(--soundscape-adjustment-end),
      var(--soundscape-adjustment-track-color) 100%
    ) center / calc(100% - var(--soundscape-adjustment-thumb-size)) var(--soundscape-adjustment-track-height) no-repeat;
  }

  .soundscape-adjustment-slider:disabled { cursor: default; }

  .soundscape-adjustment-slider::-webkit-slider-runnable-track {
    height: var(--soundscape-adjustment-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .soundscape-adjustment-slider::-webkit-slider-thumb {
    width: var(--soundscape-adjustment-thumb-size);
    height: var(--soundscape-adjustment-thumb-size);
    margin-top: calc((var(--soundscape-adjustment-track-height) - var(--soundscape-adjustment-thumb-size)) / 2);
    appearance: none;
    border: 1px solid var(--soundscape-adjustment-thumb-border);
    border-radius: 999px;
    background: var(--card);
  }

  .soundscape-adjustment-slider::-moz-range-track,
  .soundscape-adjustment-slider::-moz-range-progress {
    height: var(--soundscape-adjustment-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .soundscape-adjustment-slider::-moz-range-thumb {
    width: var(--soundscape-adjustment-thumb-size);
    height: var(--soundscape-adjustment-thumb-size);
    border: 1px solid var(--soundscape-adjustment-thumb-border);
    border-radius: 999px;
    background: var(--card);
  }
</style>
