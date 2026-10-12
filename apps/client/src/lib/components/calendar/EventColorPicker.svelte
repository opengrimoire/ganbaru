<script lang="ts">
  import { tick } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import { FALLBACK_COLOR_INDEX, type EventColor } from "$lib/calendar/types";
  import { moveRovingIndex } from "$lib/calendar/event-panel-utils";
  import { EVENT_COLOR_OPTIONS, getEventColor } from "$lib/calendar/utils";
  import { contrastRatio } from "$lib/color/math";
  import { activateModalFocus } from "$lib/modal-focus";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { resolveCalendarTokens, type Theme } from "$lib/themes";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";

  const { t } = getLocalization();
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
    ariaLabel,
    displayLabel = false,
    mobileLayout = false,
    class: className = "",
    buttonClass = "",
  }: {
    color: EventColor | undefined;
    theme: Theme;
    onSelect: (color: EventColor | undefined) => void;
    ariaLabel?: string;
    displayLabel?: boolean;
    mobileLayout?: boolean;
    class?: string;
    buttonClass?: string;
  } = $props();

  const mobileBackStack = getMobileBackStack();
  let open = $state(false);
  let buttonEl: HTMLButtonElement | undefined = $state();
  let paletteEl: HTMLDivElement | undefined = $state();
  let activeIndex = $state(0);
  let palettePosition = $state({ left: PALETTE_EDGE_PX, top: PALETTE_EDGE_PX });

  const selectedColor = $derived(color ?? FALLBACK_COLOR_INDEX);
  const colorEntry = $derived(getEventColor(color, theme));
  const buttonLabel = $derived(t("calendar.color.eventColorNumber", selectedColor + 1));
  const calendarTokens = $derived(resolveCalendarTokens(theme));
  const pickerBg = $derived(calendarTokens["--cal-bg"]);
  const pickerText = $derived(calendarTokens["--cal-time-label"]);
  const selectionBorder = $derived(
    contrastRatio(pickerBg, "#000000") >= contrastRatio(pickerBg, "#ffffff")
      ? "#000000"
      : "#ffffff",
  );

  function swatchStyle(bg: string): string {
    return `background-color: ${bg};`;
  }

  function selectedIndex(): number {
    return Math.max(0, EVENT_COLOR_OPTIONS.findIndex((entry) => entry === selectedColor));
  }

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
    if (!buttonEl) return;
    const rect = buttonEl.getBoundingClientRect();
    const { width, height } = paletteSizePx();
    const rem = rootRemPx();
    const offset = PALETTE_PADDING_REM / 2 * rem;
    const maxLeft = Math.max(PALETTE_EDGE_PX, window.innerWidth - width - PALETTE_EDGE_PX);
    const preferredLeft = displayLabel
      ? rect.left
      : rect.right - width + offset;
    const left = Math.min(Math.max(PALETTE_EDGE_PX, preferredLeft), maxLeft);
    const belowTop = rect.bottom + offset;
    const aboveTop = rect.top - height - offset;
    const top = belowTop + height + PALETTE_EDGE_PX <= window.innerHeight
      ? belowTop
      : Math.max(PALETTE_EDGE_PX, aboveTop);
    palettePosition = { left, top };
  }

  async function focusButton() {
    await tick();
    buttonEl?.focus();
  }

  async function focusSwatch(index: number) {
    await tick();
    paletteEl?.querySelector<HTMLButtonElement>(`[data-color-index="${index}"]`)?.focus();
  }

  function openPalette(source: "keyboard" | "pointer") {
    activeIndex = selectedIndex();
    computePalettePosition();
    open = true;
    void tick().then(() => {
      computePalettePosition();
      if (source === "keyboard") void focusSwatch(activeIndex);
    });
  }

  function closePalette(source: "keyboard" | "pointer") {
    open = false;
    if (source === "keyboard") void focusButton();
  }

  function togglePalette() {
    if (open) closePalette("pointer");
    else openPalette("pointer");
  }

  function selectColor(nextColor: EventColor, source: "keyboard" | "pointer"): void {
    if (color !== nextColor) onSelect(nextColor);
    if (source === "keyboard" || mobileLayout) closePalette(source);
  }

  function handleButtonKeydown(e: KeyboardEvent) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    e.stopPropagation();
    openPalette("keyboard");
  }

  function handleSwatchKeydown(e: KeyboardEvent, index: number, nextColor: EventColor) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;

    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closePalette("keyboard");
      return;
    }

    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      e.stopPropagation();
      selectColor(nextColor, "keyboard");
      return;
    }

    const nextIndex = moveRovingIndex({
      currentIndex: index,
      itemCount: EVENT_COLOR_OPTIONS.length,
      key: e.key,
      orientation: "grid",
      columns: mobileLayout ? MOBILE_PALETTE_COLUMNS : PALETTE_COLUMNS,
    });
    if (nextIndex === index) return;
    e.preventDefault();
    e.stopPropagation();
    activeIndex = nextIndex;
    void focusSwatch(nextIndex);
  }

  function handleBackdropPointerDown(event: PointerEvent): void {
    event.stopPropagation();
    closePalette("pointer");
  }

  $effect(() => {
    if (!open) return;
    function handleResize(): void {
      computePalettePosition();
    }
    function handleScroll(event: Event): void {
      const target = event.target;
      if (target instanceof Node && paletteEl?.contains(target)) return;
      closePalette("pointer");
    }
    window.addEventListener("resize", handleResize);
    window.addEventListener("scroll", handleScroll, true);
    return () => {
      window.removeEventListener("resize", handleResize);
      window.removeEventListener("scroll", handleScroll, true);
    };
  });

  $effect(() => {
    if (!open || !mobileLayout) return;
    const deactivateBack = mobileBackStack.activate({
      handle: () => closePalette("keyboard"),
    });
    const deactivateFocus = paletteEl
      ? activateModalFocus(paletteEl)
      : () => undefined;
    return () => {
      deactivateBack();
      deactivateFocus();
    };
  });

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
      grid-template-columns: repeat(${PALETTE_COLUMNS}, 1.375rem);
      background-color: ${pickerBg};
      color: ${pickerText};
      --selection-border: ${selectionBorder};
    `);
</script>

<div class={cn("relative flex items-center", displayLabel && "min-w-0", className)}>
  <button
    type="button"
    bind:this={buttonEl}
    onclick={togglePalette}
    onkeydown={handleButtonKeydown}
    class={displayLabel
      ? cn("flex w-full max-w-full items-center justify-between gap-2 rounded-md border border-border bg-card px-2.5 text-left text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent/60 dark:bg-transparent", mobileLayout ? "h-12" : "h-7", buttonClass)
      : cn(mobileLayout ? "flex size-12 shrink-0 items-center justify-center rounded-xl active:bg-accent" : "size-4.5 shrink-0 rounded-sm", buttonClass)}
    style={!displayLabel && !mobileLayout ? `background-color: ${colorEntry.bg};` : undefined}
    aria-label={ariaLabel ?? t("calendar.color.eventColor")}
    aria-haspopup="dialog"
    aria-expanded={open}
    data-app-tooltip-disabled="true"
  >
    {#if displayLabel}
      <span class="flex min-w-0 items-center gap-2">
        <span
          class="h-3.5 w-3.5 shrink-0 rounded-[3px] border border-transparent"
          style="background-color: {colorEntry.bg};"
          aria-hidden="true"
        ></span>
        <span class="truncate">{buttonLabel}</span>
      </span>
      <ChevronDown
        size={13}
        strokeWidth={2}
        class={cn("shrink-0 text-muted-foreground transition-transform", open && "rotate-180")}
      />
    {:else if mobileLayout}
      <span
        class="size-5 rounded-md border border-border"
        style="background-color: {colorEntry.bg};"
        aria-hidden="true"
      ></span>
    {/if}
  </button>
  {#if open}
    <div
      use:portal
      role="presentation"
      aria-hidden="true"
      class="fixed inset-0 z-90"
      data-app-floating-surface
      onpointerdown={handleBackdropPointerDown}
    ></div>
    <div
      bind:this={paletteEl}
      use:portal
      data-app-floating-surface
      class={cn(
        mobileLayout
          ? "surface-floating fixed z-100 grid gap-1 overflow-y-auto overscroll-contain p-2"
          : "surface-floating fixed z-100 grid gap-2 p-2.5",
      )}
      style={paletteStyle}
      role="dialog"
      aria-modal={mobileLayout ? "true" : undefined}
      aria-label={ariaLabel ?? t("calendar.color.eventColor")}
      tabindex="-1"
    >
      {#each EVENT_COLOR_OPTIONS as option, index}
        {@const entry = getEventColor(option, theme)}
        <button
          type="button"
          data-color-index={index}
          aria-label={ariaLabel ? `${ariaLabel} ${index + 1}` : t("calendar.color.selectEventColor", index + 1)}
          tabindex={activeIndex === index ? 0 : -1}
          onclick={() => { selectColor(option, "pointer"); }}
          onfocus={() => { activeIndex = index; }}
          onkeydown={(e) => handleSwatchKeydown(e, index, option)}
          class={mobileLayout
            ? "calendar-color-swatch min-h-12 min-w-12 rounded-xl"
            : "calendar-color-swatch size-5.5 rounded-[3px]"}
          class:swatch-selected={selectedColor === option}
          style={swatchStyle(entry.bg)}
          data-app-tooltip-disabled="true"
          data-app-tooltip-focus-disabled="true"
        ></button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .calendar-color-swatch {
    position: relative;
    overflow: hidden;
  }

  .calendar-color-swatch.swatch-selected::after {
    content: "";
    position: absolute;
    inset: 0;
    border: 2px solid var(--selection-border);
    border-radius: inherit;
    pointer-events: none;
  }
</style>
