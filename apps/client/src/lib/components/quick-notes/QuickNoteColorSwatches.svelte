<script lang="ts">
  import { EVENT_COLOR_OPTIONS, getEventColor } from "$lib/calendar/utils";
  import type { EventColor } from "$lib/calendar/types";
  import { contrastRatio } from "$lib/color/math";
  import { cn } from "$lib/utils";
  import { resolveCalendarTokens, type Theme } from "$lib/themes";

  /**
   * The palette as a grid of swatches with the current color marked, shared by the note color picker and the tag menus.
   * The selection border contrasts with the Quick notes surface the grid sits on.
   */
  let {
    color,
    theme,
    label,
    onSelect,
    columns,
    swatchClass,
    class: className = "",
    inMenu = false,
  }: {
    color: EventColor;
    theme: Theme;
    /** Names the grid and, with a position, each swatch. */
    label: string;
    onSelect: (color: EventColor) => void;
    /** Column template for the grid, for example `repeat(8, 1.375rem)`. */
    columns: string;
    swatchClass: string;
    class?: string;
    /** Marks the swatches as radio items of the menu that holds the grid. */
    inMenu?: boolean;
  } = $props();

  const surface = $derived(resolveCalendarTokens(theme)["--cal-bg"]);
  const selectionBorder = $derived(
    contrastRatio(surface, "#000000") >= contrastRatio(surface, "#ffffff") ? "#000000" : "#ffffff",
  );
</script>

<div
  class={cn("grid", className)}
  style="grid-template-columns: {columns}; --selection-border: {selectionBorder};"
  role="group"
  aria-label={label}
>
  {#each EVENT_COLOR_OPTIONS as entry, index}
    <button
      type="button"
      class={cn("quick-note-color-swatch", swatchClass)}
      class:swatch-selected={color === entry}
      style="background-color: {getEventColor(entry, theme).bg};"
      aria-label={`${label} ${index + 1}`}
      role={inMenu ? "menuitemradio" : undefined}
      aria-checked={inMenu ? color === entry : undefined}
      aria-pressed={inMenu ? undefined : color === entry}
      title={`${label} ${index + 1}`}
      onclick={() => onSelect(entry)}
    ></button>
  {/each}
</div>

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
