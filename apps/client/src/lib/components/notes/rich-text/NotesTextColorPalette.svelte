<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    NOTES_BACKGROUND_COLORS,
    NOTES_TEXT_COLORS,
    notesBlockColorSwatchStyle,
  } from "$lib/notes/blocks/color";
  import type { NotesColor } from "$lib/notes/types";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    currentColor,
    calloutBackgroundColor,
    onSelect,
  }: {
    currentColor: NotesColor;
    calloutBackgroundColor?: NotesColor;
    onSelect: (color: NotesColor) => void;
  } = $props();

  const { t } = getLocalization();

  function colorLabel(color: NotesColor): string {
    switch (color) {
      case "default": return t("notes.blockColor.default");
      case "gray": return t("notes.blockColor.gray");
      case "brown": return t("notes.blockColor.brown");
      case "orange": return t("notes.blockColor.orange");
      case "yellow": return t("notes.blockColor.yellow");
      case "green": return t("notes.blockColor.green");
      case "blue": return t("notes.blockColor.blue");
      case "purple": return t("notes.blockColor.purple");
      case "pink": return t("notes.blockColor.pink");
      case "red": return t("notes.blockColor.red");
      case "gray_background": return t("notes.blockColor.grayBackground");
      case "brown_background": return t("notes.blockColor.brownBackground");
      case "orange_background": return t("notes.blockColor.orangeBackground");
      case "yellow_background": return t("notes.blockColor.yellowBackground");
      case "green_background": return t("notes.blockColor.greenBackground");
      case "blue_background": return t("notes.blockColor.blueBackground");
      case "purple_background": return t("notes.blockColor.purpleBackground");
      case "pink_background": return t("notes.blockColor.pinkBackground");
      case "red_background": return t("notes.blockColor.redBackground");
    }
  }
</script>

<div class="surface-floating flex max-h-[min(19rem,calc(100vh-1rem))] w-full flex-col overflow-hidden" role="menu" aria-label={t("notes.textColor")} tabindex="-1" data-app-floating-surface onmousedown={(event) => event.preventDefault()}>
  <div class="min-h-0 flex-1 overflow-y-auto p-2" use:scrollEdgeFadeAction>
  <div class="px-1 pb-1 text-panel-detail font-medium text-muted-foreground">{t("notes.textColors")}</div>
  <div class="grid grid-cols-5 gap-0.5" role="group" aria-label={t("notes.textColors")}>
    {#each NOTES_TEXT_COLORS as color}
      <button
        type="button"
        class="flex size-7 items-center justify-center rounded-floating-item hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        role="menuitemradio"
        aria-label={colorLabel(color)}
        title={colorLabel(color)}
        aria-checked={currentColor === color}
        onclick={() => onSelect(color)}
      >
        <span class:notes-color-selected={currentColor === color} class="notes-color-swatch" style={notesBlockColorSwatchStyle(color)} aria-hidden="true">A</span>
      </button>
    {/each}
  </div>
  <div class="mt-2 border-t border-border px-1 pb-1 pt-2 text-panel-detail font-medium text-muted-foreground">{t("notes.backgroundColors")}</div>
  <div class="grid grid-cols-5 gap-0.5" role="group" aria-label={t("notes.backgroundColors")}>
    {#each NOTES_BACKGROUND_COLORS as color}
      <button
        type="button"
        class="flex size-7 items-center justify-center rounded-floating-item hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        role="menuitemradio"
        aria-label={colorLabel(color)}
        title={colorLabel(color)}
        aria-checked={(calloutBackgroundColor ?? currentColor) === color}
        onclick={() => onSelect(color)}
      >
        <span class:notes-color-selected={(calloutBackgroundColor ?? currentColor) === color} class="notes-color-swatch" style={notesBlockColorSwatchStyle(color)} aria-hidden="true">A</span>
      </button>
    {/each}
  </div>
  </div>
</div>

<style>
  .notes-color-swatch {
    display: inline-flex;
    width: 1.375rem;
    height: 1.375rem;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--notes-color-swatch-border);
    border-radius: 0.25rem;
    background: var(--notes-color-swatch-bg);
    color: var(--notes-color-swatch-fg);
    font-size: 0.8rem;
    font-weight: 600;
    line-height: 1;
  }

  .notes-color-selected {
    outline: 2px solid var(--ring);
    outline-offset: 2px;
  }
</style>
