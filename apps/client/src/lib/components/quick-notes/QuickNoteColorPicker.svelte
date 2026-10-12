<script lang="ts">
  import { tick } from "svelte";
  import Palette from "@lucide/svelte/icons/palette";
  import { EVENT_COLOR_OPTIONS, getEventColor } from "$lib/calendar/utils";
  import type { EventColor } from "$lib/calendar/types";
  import { contrastRatio } from "$lib/color/math";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { resolveCalendarTokens, type Theme } from "$lib/themes";
  import { portal } from "$lib/utils/portal";

  const PALETTE_COLUMNS = 4;
  const MOBILE_PALETTE_COLUMNS = 5;
  const PALETTE_SWATCH_REM = 1.375;
  const PALETTE_GAP_REM = 0.5;
  const PALETTE_PADDING_REM = 0.625;
  const PALETTE_EDGE_PX = 8;
  /** Border width on each side of the floating surface. */
  const PALETTE_BORDER_PX = 1;

  let {
    color,
    theme,
    onSelect,
    buttonClass = "",
    mobileLayout = false,
  }: {
    color: EventColor;
    theme: Theme;
    onSelect: (color: EventColor) => void;
    buttonClass?: string;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  let open = $state(false);
  let button = $state<HTMLButtonElement | null>(null);
  let palette = $state<HTMLDivElement | null>(null);
  let palettePosition = $state({ left: PALETTE_EDGE_PX, top: PALETTE_EDGE_PX });
  const calendarTokens = $derived(resolveCalendarTokens(theme));
  const pickerBg = $derived(calendarTokens["--cal-bg"]);
  const pickerText = $derived(calendarTokens["--cal-time-label"]);
  const selectionBorder = $derived(
    contrastRatio(pickerBg, "#000000") >= contrastRatio(pickerBg, "#ffffff")
      ? "#000000"
      : "#ffffff",
  );

  function rootRemPx(): number {
    const fontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
    return Number.isFinite(fontSize) && fontSize > 0 ? fontSize : 16;
  }

  function paletteSizePx(): { width: number; height: number } {
    const rem = rootRemPx();
    const rows = Math.ceil(EVENT_COLOR_OPTIONS.length / PALETTE_COLUMNS);
    return {
      width: PALETTE_COLUMNS * PALETTE_SWATCH_REM * rem
        + (PALETTE_COLUMNS - 1) * PALETTE_GAP_REM * rem
        + PALETTE_PADDING_REM * rem * 2
        + PALETTE_BORDER_PX * 2,
      height: rows * PALETTE_SWATCH_REM * rem
        + Math.max(0, rows - 1) * PALETTE_GAP_REM * rem
        + PALETTE_PADDING_REM * rem * 2
        + PALETTE_BORDER_PX * 2,
    };
  }

  function computePalettePosition(): void {
    if (!button) return;
    const rect = button.getBoundingClientRect();
    const { width, height } = paletteSizePx();
    const rem = rootRemPx();
    const offset = PALETTE_PADDING_REM / 2 * rem;
    const maxLeft = Math.max(PALETTE_EDGE_PX, window.innerWidth - width - PALETTE_EDGE_PX);
    const left = Math.min(
      Math.max(PALETTE_EDGE_PX, rect.right - width + offset),
      maxLeft,
    );
    const belowTop = rect.bottom + offset;
    const aboveTop = rect.top - height - offset;
    const top = belowTop + height + PALETTE_EDGE_PX <= window.innerHeight
      ? belowTop
      : Math.max(PALETTE_EDGE_PX, aboveTop);
    palettePosition = { left, top };
  }

  async function openPicker(): Promise<void> {
    if (!button) return;
    computePalettePosition();
    open = true;
    await tick();
    computePalettePosition();
  }

  function closePicker(): void {
    open = false;
    void tick().then(() => button?.focus());
  }

  function choose(next: EventColor): void {
    onSelect(next);
    closePicker();
  }

  function handlePickerKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    closePicker();
  }

  const paletteStyle = $derived(mobileLayout
    ? `
      left: calc(var(--visual-viewport-offset-left) + var(--safe-area-left) + 0.5rem);
      right: calc(var(--safe-area-right) + 0.5rem);
      bottom: calc(var(--keyboard-inset) + var(--safe-area-bottom) + 0.5rem);
      max-width: 30rem;
      max-height: calc(var(--visual-viewport-height) - var(--safe-area-top) - var(--safe-area-bottom) - 1rem);
      margin-inline: auto;
      grid-template-columns: repeat(${MOBILE_PALETTE_COLUMNS}, 3rem);
      justify-content: center;
      background-color: ${pickerBg};
      color: ${pickerText};
      --selection-border: ${selectionBorder};
    `
    : `
      left: ${palettePosition.left}px;
      top: ${palettePosition.top}px;
      grid-template-columns: repeat(${PALETTE_COLUMNS}, ${PALETTE_SWATCH_REM}rem);
      background-color: ${pickerBg};
      color: ${pickerText};
      --selection-border: ${selectionBorder};
    `);

  $effect(() => {
    if (!open) return;
    const deactivateBack = mobileLayout
      ? mobileBackStack.activate({ handle: closePicker })
      : () => undefined;
    const deactivateFocus = palette
      ? activateModalFocus(palette)
      : () => undefined;
    window.addEventListener("keydown", handlePickerKeydown, true);
    return () => {
      deactivateBack();
      deactivateFocus();
      window.removeEventListener("keydown", handlePickerKeydown, true);
    };
  });
</script>

<button
  bind:this={button}
  type="button"
  class={buttonClass}
  aria-label={t("quickNotes.action.color")}
  title={t("quickNotes.action.color")}
  aria-haspopup="dialog"
  aria-expanded={open}
  onclick={() => { if (open) closePicker(); else void openPicker(); }}
>
  <Palette class="size-4" strokeWidth={1.5} aria-hidden="true" />
</button>

{#if open}
  <button
    use:portal
    type="button"
    class="fixed inset-0 z-90 cursor-default"
    aria-label={t("common.close")}
    onclick={closePicker}
  ></button>
  <div
    bind:this={palette}
    use:portal
    class={mobileLayout ? "surface-floating fixed z-100 grid gap-1 overflow-y-auto overscroll-contain p-2" : "surface-floating fixed z-100 grid gap-2 p-2.5"}
    style={paletteStyle}
    role="dialog"
    aria-modal="true"
    aria-label={t("quickNotes.action.color")}
    tabindex="-1"
  >
    {#each EVENT_COLOR_OPTIONS as entry, index}
      {@const resolved = getEventColor(entry, theme)}
      <button
        type="button"
        class={mobileLayout ? "quick-note-color-swatch min-h-12 min-w-12 rounded-xl" : "quick-note-color-swatch size-5.5 rounded-[3px]"}
        class:swatch-selected={color === entry}
        style="background-color: {resolved.bg};"
        aria-label={`${t("quickNotes.action.color")} ${index + 1}`}
        title={`${t("quickNotes.action.color")} ${index + 1}`}
        onclick={() => choose(entry)}
      ></button>
    {/each}
  </div>
{/if}

<style>
  .quick-note-color-swatch {
    position: relative;
    overflow: hidden;
  }

  .quick-note-color-swatch.swatch-selected::after {
    content: "";
    position: absolute;
    inset: 0;
    border: 2px solid var(--selection-border);
    border-radius: inherit;
    pointer-events: none;
  }
</style>
