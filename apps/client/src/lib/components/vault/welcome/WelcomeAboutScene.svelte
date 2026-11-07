<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 384;
  const MAX_CANVAS_W = 600;
  const MIN_CANVAS_H = 256;
  const MAX_CANVAS_H = 360;

  export const ABOUT_SCENE_MIN_WIDTH = MIN_CANVAS_W;
  export const ABOUT_SCENE_MAX_WIDTH = MAX_CANVAS_W;
  export const ABOUT_SCENE_MIN_HEIGHT = MIN_CANVAS_H;
  export const ABOUT_SCENE_MAX_HEIGHT = MAX_CANVAS_H;
</script>

<script lang="ts">
  import Book from "@lucide/svelte/icons/book";
  import BriefcaseBusiness from "@lucide/svelte/icons/briefcase-business";
  import Calendar from "@lucide/svelte/icons/calendar";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import GlobeOff from "@lucide/svelte/icons/globe-off";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Timer from "@lucide/svelte/icons/timer";
  import { getEventColor } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import {
    ABOUT_BEATS,
    ABOUT_RECEIPT_ITEMS,
    ABOUT_SEGMENTS,
    type AboutSegment,
    aboutFeed,
    aboutImpact,
    aboutPrinter,
    aboutPrinting,
    aboutStamp,
  } from "$lib/vault/welcome-about-scene";

  /**
   * One frame of the welcome scene about the app itself: a receipt printer prints the app's features one per line, each
   * priced free, then a free total, and the license stamps the receipt as the reason it stays free. Every style is a
   * function of `elapsed`, with no CSS transitions, so text is never handed to a compositing layer mid-animation.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const { t } = getLocalization();
  const theme = getTheme();

  const RECEIPT_W = 212;
  /** The height of each kind of segment the printer feeds. */
  const SEGMENT_H: Readonly<Record<AboutSegment, number>> = {
    lead: 10,
    header: 30,
    item: 14,
    rule: 11,
    total: 22,
    tail: 16,
  };
  const PAPER_H = ABOUT_SEGMENTS.reduce((sum, segment) => sum + SEGMENT_H[segment], 0);
  /** The torn top edge of the paper: the width and depth of each tooth. */
  const TOOTH_W = 8;
  const TOOTH_H = 4;
  const PRINTER_W = RECEIPT_W + 44;
  const PRINTER_H = 30;
  const SLOT_W = RECEIPT_W + 14;
  const SLOT_H = 5;
  /** How far the printer rises into place as it appears, and how far the receipt drops when the stamp lands. */
  const RISE = 8;
  const IMPACT_DROP = 2;
  const STAMP_SIZE = 74;
  const STAMP_TILT = -12;
  const STAMP_SCALE_FROM = 1.8;
  /** Where the stamp sits: in from the receipt's right edge, and up from its bottom edge. */
  const STAMP_INSET_X = 30;
  const STAMP_INSET_Y = 40;
  /** Palette slot for the printer's light, so it follows the active theme's event colors. */
  const GREEN = 12;
  /** How fast the printer's light blinks while it prints. */
  const BLINK_RATE = 1 / 70;

  const PAPER = "color-mix(in srgb, var(--foreground) 9%, var(--background))";
  const PRINTER = "color-mix(in srgb, var(--foreground) 16%, var(--background))";
  const SLOT = "color-mix(in srgb, var(--foreground) 40%, var(--background))";

  /** The features the receipt lists, in order, each with the icon the app gives it. */
  const ITEMS: readonly { icon: typeof Calendar; label: () => string }[] = [
    { icon: Calendar, label: () => t("settings.section.calendars") },
    { icon: Timer, label: () => t("settings.section.focus") },
    { icon: BriefcaseBusiness, label: () => t("settings.section.projects") },
    { icon: Book, label: () => t("settings.section.notes") },
    { icon: MessageSquare, label: () => t("settings.section.chat") },
    { icon: Music, label: () => t("settings.section.music") },
    { icon: GlobeOff, label: () => t("vaultSetup.about.receipt.blocker") },
    { icon: RefreshCw, label: () => t("vaultSetup.about.receipt.sync") },
  ].slice(0, ABOUT_RECEIPT_ITEMS);

  /** The torn top edge, as a clip path across the receipt's width. */
  const TORN_EDGE = (() => {
    const teeth = Math.ceil(RECEIPT_W / TOOTH_W);
    const points = Array.from({ length: teeth * 2 + 1 }, (_, index) => {
      const x = Math.min(RECEIPT_W, (index * TOOTH_W) / 2);
      return `${x}px ${index % 2 === 0 ? TOOTH_H : 0}px`;
    });
    return `polygon(${[...points, "100% 100%", "0 100%"].join(", ")})`;
  })();

  const center = $derived(width / 2);
  const left = $derived(center - RECEIPT_W / 2);
  /** The receipt and the printer, centered together in the canvas. */
  const groupTop = $derived((height - PAPER_H - PRINTER_H) / 2);
  const printerTop = $derived(groupTop + PAPER_H);
  const printer = $derived(aboutPrinter(elapsed));
  const fed = $derived(ABOUT_SEGMENTS.reduce((sum, segment, index) => sum + SEGMENT_H[segment] * aboutFeed(elapsed, index), 0));
  const drop = $derived(aboutImpact(elapsed) * IMPACT_DROP);
  const stamp = $derived(aboutStamp(elapsed));
  const light = $derived(lightLevel(elapsed));
  const lightColor = $derived(getEventColor(GREEN, theme.current).bg);

  /** How bright the printer's light is: dim while it waits, blinking while it prints, and steady once done. */
  function lightLevel(at: number): number {
    if (aboutPrinting(at)) return 0.55 + 0.45 * Math.sin(at * BLINK_RATE);
    return at >= ABOUT_BEATS.printEnd ? 1 : 0.35;
  }

  /** The index of the first item segment, so item `item` is segment `FIRST_ITEM + item`. */
  const FIRST_ITEM = ABOUT_SEGMENTS.indexOf("item");
</script>

{#snippet segmentContent(segment: AboutSegment, index: number)}
  {#if segment === "header"}
    <div class="flex h-full flex-col items-center justify-center border-b border-dashed border-foreground/25 text-xs font-semibold text-foreground">
      {t("common.appName")}
    </div>
  {:else if segment === "item"}
    {@const item = ITEMS[index - FIRST_ITEM]}
    <div class="flex h-full items-center gap-1.5 text-[10px] text-foreground">
      <item.icon size={11} strokeWidth={2} class="shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 truncate">{item.label()}</span>
      <span class="mb-1 min-w-2 flex-1 self-end border-b border-dotted border-foreground/30"></span>
      <span class="font-medium">{t("vaultSetup.about.receipt.free")}</span>
    </div>
  {:else if segment === "rule"}
    <div class="flex h-full items-center"><div class="w-full border-t border-dashed border-foreground/25"></div></div>
  {:else if segment === "total"}
    <div class="flex h-full items-center justify-between text-[11.5px] font-semibold text-foreground">
      <span>{t("vaultSetup.about.receipt.total")}</span>
      <span>{t("vaultSetup.about.receipt.free")}</span>
    </div>
  {/if}
{/snippet}

<div class="relative overflow-hidden" style:width={`${width}px`} style:height={`${height}px`}>
  {#if printer > 0}
    <div class="absolute inset-0" style:top={`${(1 - printer) * RISE}px`} style:opacity={printer}>
      <!-- The receipt, fed out of the printer's slot: its content rises with the paper, and what is not fed yet stays inside. -->
      {#if fed > 0}
        <div
          class="absolute overflow-hidden"
          style:left={`${left}px`}
          style:top={`${printerTop - fed + drop}px`}
          style:width={`${RECEIPT_W}px`}
          style:height={`${fed}px`}
          style:background-color={PAPER}
          style:clip-path={TORN_EDGE}
        >
          <div class="px-3">
            {#each ABOUT_SEGMENTS as segment, index (index)}
              <div style:height={`${SEGMENT_H[segment]}px`}>{@render segmentContent(segment, index)}</div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- The license's stamp, coming down onto the printed receipt. -->
      {#if stamp > 0}
        <div
          class="absolute grid place-items-center rounded-full border-[2.5px] border-primary text-primary"
          style:left={`${left + RECEIPT_W - STAMP_INSET_X - STAMP_SIZE / 2}px`}
          style:top={`${printerTop - STAMP_INSET_Y - STAMP_SIZE / 2 + drop}px`}
          style:width={`${STAMP_SIZE}px`}
          style:height={`${STAMP_SIZE}px`}
          style:background-color="color-mix(in srgb, var(--primary) 10%, transparent)"
          style:opacity={Math.min(1, stamp * 3)}
          style:transform={`rotate(${STAMP_TILT}deg) scale(${STAMP_SCALE_FROM - (STAMP_SCALE_FROM - 1) * stamp})`}
        >
          <div class="absolute inset-1 rounded-full border border-primary/70"></div>
          <div class="flex flex-col items-center px-2 text-center leading-tight">
            <span class="text-xs font-bold">AGPL 3.0</span>
            <span class="text-[8px] font-semibold">{t("vaultSetup.about.receipt.stamp")}</span>
          </div>
        </div>
      {/if}

      <!-- The printer, in front of the paper, with its slot along the top and its light. -->
      <div
        class="absolute rounded-2xl"
        style:left={`${center - PRINTER_W / 2}px`}
        style:top={`${printerTop}px`}
        style:width={`${PRINTER_W}px`}
        style:height={`${PRINTER_H}px`}
        style:background-color={PRINTER}
      >
        <span
          class="absolute top-1/2 right-4 size-1.5 -translate-y-1/2 rounded-full"
          style:background-color={lightColor}
          style:opacity={light}
        ></span>
      </div>
      <div
        class="absolute rounded-full"
        style:left={`${center - SLOT_W / 2}px`}
        style:top={`${printerTop - SLOT_H / 2}px`}
        style:width={`${SLOT_W}px`}
        style:height={`${SLOT_H}px`}
        style:background-color={SLOT}
      ></div>
    </div>
  {/if}
</div>
