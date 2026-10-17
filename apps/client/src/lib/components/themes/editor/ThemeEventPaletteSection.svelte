<script lang="ts">
  import { PALETTE_SIZE } from "$lib/calendar/types";
  import { blendHex } from "$lib/calendar/utils";
  import {
    checkerboardBackgroundForCells,
  } from "$lib/color/display";
  import { hexToRgba, normalizeHex } from "$lib/color/math";
  import ColorField from "$lib/components/ui/ColorField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    isThemeCalendarDark,
    resolveCalendarTokens,
    type Theme,
  } from "$lib/themes";
  import { cn } from "$lib/utils";

  let {
    theme,
    readOnly,
    onSetSlot,
  }: {
    theme: Theme;
    readOnly: boolean;
    onSetSlot: (index: number, hex: string) => void;
  } = $props();

  const { t } = getLocalization();

  const PALETTE_SWATCH_SIZE = 22;
  const PALETTE_SWATCH_CHECKER_BG = checkerboardBackgroundForCells(
    PALETTE_SWATCH_SIZE,
    3,
  );
  const paletteIndices = Array.from({ length: PALETTE_SIZE }, (_, i) => i);
  const calendarBg = $derived(resolveCalendarTokens(theme)["--cal-bg"]);

  function swatchHasTransparency(hex: string): boolean {
    const rgba = hexToRgba(hex);
    return rgba !== null && rgba.a < 255;
  }

  function swatchFrameStyle(hex: string): string {
    return swatchHasTransparency(hex)
      ? `background: ${PALETTE_SWATCH_CHECKER_BG};`
      : "";
  }

  function swatchColor(hex: string): string {
    return normalizeHex(hex) ?? "#000000";
  }
</script>

{#snippet palettePreviewSwatch(value: string, title: string)}
  <span
    class={cn(
      "relative block h-5.5 w-5.5 shrink-0 overflow-hidden rounded-md border",
      swatchHasTransparency(value)
        ? "border-transparent bg-clip-padding shadow-none"
        : "border-border shadow-sm",
    )}
    style={swatchFrameStyle(value)}
    title={title}
  >
    <span
      class="absolute inset-0 block"
      style="background: {swatchColor(value)};"
    ></span>
  </span>
{/snippet}

<section class="flex flex-col gap-2 py-2.5">
  <header class="px-1">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">
      {t("settings.theme.editor.eventPalette")}
    </h2>
    <div class="text-[0.733333rem] text-muted-foreground">
      {t("settings.theme.editor.eventPaletteDescription")}
    </div>
  </header>
  <div
    class="flex flex-col gap-3 p-3"
    style="background-color: {calendarBg};"
  >
    <div class="theme-palette-grid grid gap-x-2 gap-y-1.5">
      {#each paletteIndices as index}
        {@const base = theme.eventPalette[index]}
        {@const past = blendHex(
          base,
          calendarBg,
          isThemeCalendarDark(theme) ? 0.5 : 0.3,
        )}
        <div class="flex min-w-0 items-center gap-1.5">
          {@render palettePreviewSwatch(
            past,
            t("settings.theme.editor.pastVariant", past),
          )}
          <ColorField
            value={base}
            onChange={(hex) => {
              if (!readOnly) onSetSlot(index, hex);
            }}
            {readOnly}
            fluid
            swatchSize={22}
            class="min-w-0 flex-1"
          />
        </div>
      {/each}
    </div>
  </div>
</section>

<style>
  .theme-palette-grid {
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 9.5rem), 1fr));
  }
</style>
