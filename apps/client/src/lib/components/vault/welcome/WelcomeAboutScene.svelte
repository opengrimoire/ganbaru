<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 384;
  const MAX_CANVAS_W = 600;
  const MIN_CANVAS_H = 340;
  const MAX_CANVAS_H = 400;

  export const ABOUT_SCENE_MIN_WIDTH = MIN_CANVAS_W;
  export const ABOUT_SCENE_MAX_WIDTH = MAX_CANVAS_W;
  export const ABOUT_SCENE_MIN_HEIGHT = MIN_CANVAS_H;
  export const ABOUT_SCENE_MAX_HEIGHT = MAX_CANVAS_H;

  /** The Open Grimoire logo, inked at the center of the stamp. */
  const LOGO_URL = new URL("../../../../../static/brand/opengrimoire.svg", import.meta.url).href;
</script>

<script lang="ts">
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    ABOUT_BEATS,
    ABOUT_RECEIPT_ITEMS,
    ABOUT_SEGMENTS,
    type AboutSegment,
    aboutConfirmCheck,
    aboutConfirmRing,
    aboutFeed,
    aboutImpact,
    aboutPrinter,
    aboutPrinting,
    aboutScreen,
    aboutStamp,
    aboutTotal,
  } from "$lib/vault/welcome-about-scene";

  /**
   * One frame of the welcome scene about the app itself: a payment terminal shows a total that is free and confirms there
   * is nothing to pay, then prints the app's features one per line, each priced free, then a free total, and a rubber
   * stamp with the Open Grimoire logo marks the receipt with the open source license. The terminal, the thermal paper,
   * and the ink are drawn as the physical things they are, in their own colors rather than the theme's, so the receipt
   * reads as a receipt in every theme. Every style is a function of `elapsed`, with no CSS transitions, so text is never
   * handed to a compositing layer mid-animation.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const uid = $props.id();

  const RECEIPT_W = 236;
  /** The height of each kind of segment the printer feeds. */
  const SEGMENT_H: Readonly<Record<AboutSegment, number>> = {
    lead: 12,
    header: 38,
    rule: 10,
    item: 14,
    total: 22,
    barcode: 34,
    tail: 10,
  };
  const PAPER_H = ABOUT_SEGMENTS.reduce((sum, segment) => sum + SEGMENT_H[segment], 0);
  /** The torn top edge of the paper: the width and depth of each tooth. */
  const TOOTH_W = 7;
  const TOOTH_H = 3.5;

  /** The terminal: a top face with the paper slot, seen a little from above, over a front face with the screen and keys. */
  const PRINTER_W = RECEIPT_W + 64;
  const TOP_FACE_H = 15;
  const FRONT_H = 64;
  const SLOT_Y = 6;
  const SLOT_W = RECEIPT_W + 16;
  const SLOT_H = 4;
  const PRINTER_RADIUS = 14;
  const SCREEN_W = 150;
  const SCREEN_H = 44;
  const KEY_ROWS = 3;
  const KEY_COLUMNS = 3;

  /** The confirmation on the screen: the ring's size and radius, and the check drawn inside it. */
  const CONFIRM_SIZE = 22;
  const CONFIRM_R = 9.5;
  const CONFIRM_CIRCUMFERENCE = 2 * Math.PI * CONFIRM_R;
  const CHECK_PATH = "M6.6 11.4 L9.8 14.6 L15.6 8.2";
  /** How far the screen's text slides as it comes and goes. */
  const SCREEN_SLIDE = 4;

  /** How far the terminal rises into place as it appears, and how far the receipt drops when the stamp lands. */
  const RISE = 8;
  const IMPACT_DROP = 2;

  const STAMP_SIZE = 96;
  const STAMP_TILT = -14;
  const STAMP_SCALE_FROM = 1.7;
  /** Where the stamp's center sits: in from the receipt's right edge, and up from the slot. */
  const STAMP_INSET_X = 58;
  const STAMP_RISE = 74;
  /** The radius the stamp's ring text runs along, and the logo's size, in the stamp's 100 unit drawing. */
  const RING_TEXT_R = 33;
  const LOGO_SIZE = 36;

  /** How fast the terminal's light blinks while it prints. */
  const BLINK_RATE = 1 / 70;

  /** Bar and space widths of the barcode, alternating, in units of one thin bar. */
  const BARCODE = "21131221312112313122113212311213221131213121132112".split("").map(Number);
  const BARCODE_UNIT = 1.4;
  const BARCODE_H = 20;

  /** Thermal paper, its ink, and the stamp's ink, which keep their own colors in every theme. */
  const PAPER = "linear-gradient(90deg, #eeede8 0%, #fcfbf7 12%, #fdfcf9 88%, #ecebe6 100%)";
  const INK = "#28282c";
  const INK_SOFT = "#77777e";
  const INK_RULE = "#b4b4ba";
  const STAMP_INK = "#c42f3c";
  /** The terminal's charcoal body, its slot, keys, and light. */
  const TOP_FACE = "linear-gradient(180deg, #6a6d74 0%, #52555b 100%)";
  const TOP_FACE_FRONT = "linear-gradient(180deg, #5a5d63 0%, #52555b 100%)";
  const FRONT_FACE = "linear-gradient(180deg, #464950 0%, #2d2f33 100%)";
  const SLOT = "#111113";
  const KEY = "linear-gradient(180deg, #4b4e55 0%, #3a3d43 100%)";
  const LIGHT = "#4ade80";
  /** The screen: its bezel, its glass when off and when lit, its text, and the confirmation's green. */
  const BEZEL = "#141518";
  const SCREEN_OFF = "#17191c";
  const SCREEN_ON = "linear-gradient(160deg, #26394a 0%, #142230 100%)";
  const SCREEN_GLARE = "linear-gradient(115deg, rgb(255 255 255 / 0.09) 0%, rgb(255 255 255 / 0.03) 38%, transparent 40%)";
  const SCREEN_TEXT = "#f1f5f9";
  const SCREEN_TEXT_SOFT = "#9fb3c4";
  const CONFIRM_GREEN = "#4ade80";

  /** The features the receipt lists, in order. */
  const ITEMS: readonly (() => string)[] = [
    () => t("settings.section.calendars"),
    () => t("settings.section.focus"),
    () => t("settings.section.projects"),
    () => t("settings.section.notes"),
    () => t("settings.section.chat"),
    () => t("settings.section.music"),
    () => t("vaultSetup.about.receipt.blocker"),
    () => t("vaultSetup.about.receipt.sync"),
  ].slice(0, ABOUT_RECEIPT_ITEMS);
  const FIRST_ITEM = ABOUT_SEGMENTS.indexOf("item");

  /** The torn top edge, as a clip path across the receipt's width. */
  const TORN_EDGE = (() => {
    const teeth = Math.ceil(RECEIPT_W / TOOTH_W);
    const points = Array.from({ length: teeth * 2 + 1 }, (_, index) => {
      const x = Math.min(RECEIPT_W, (index * TOOTH_W) / 2);
      return `${x}px ${index % 2 === 0 ? TOOTH_H : 0}px`;
    });
    return `polygon(${[...points, "100% 100%", "0 100%"].join(", ")})`;
  })();

  const BARCODE_BARS = (() => {
    let x = 0;
    const bars: { x: number; width: number }[] = [];
    BARCODE.forEach((units, index) => {
      if (index % 2 === 0) bars.push({ x, width: units * BARCODE_UNIT });
      x += units * BARCODE_UNIT;
    });
    return { bars, width: x };
  })();

  const RING_CIRCUMFERENCE = 2 * Math.PI * RING_TEXT_R;
  const dashes = `repeating-linear-gradient(90deg, ${INK_RULE} 0 4px, transparent 4px 7px)`;

  /** The moment the receipt is printed, fixed for the scene's life. */
  const printedAt = new Date();
  const printedOn = $derived(formatDateTime(localization.locale, printedAt, { dateStyle: "medium", timeStyle: "short" }));

  const center = $derived(width / 2);
  const left = $derived(center - RECEIPT_W / 2);
  const printerLeft = $derived(center - PRINTER_W / 2);
  /** The receipt and the terminal, centered together in the canvas. */
  const groupTop = $derived((height - PAPER_H - TOP_FACE_H - FRONT_H) / 2);
  const printerTop = $derived(groupTop + PAPER_H - SLOT_Y);
  const slotY = $derived(printerTop + SLOT_Y + SLOT_H / 2);
  const printer = $derived(aboutPrinter(elapsed));
  const screen = $derived(aboutScreen(elapsed));
  const total = $derived(aboutTotal(elapsed));
  const ring = $derived(aboutConfirmRing(elapsed));
  const check = $derived(aboutConfirmCheck(elapsed));
  const fed = $derived(ABOUT_SEGMENTS.reduce((sum, segment, index) => sum + SEGMENT_H[segment] * aboutFeed(elapsed, index), 0));
  const drop = $derived(aboutImpact(elapsed) * IMPACT_DROP);
  const stamp = $derived(aboutStamp(elapsed));
  const light = $derived(lightLevel(elapsed));

  /** How bright the terminal's light is: dim while it waits, blinking while it prints, and steady once done. */
  function lightLevel(at: number): number {
    if (aboutPrinting(at)) return 0.55 + 0.45 * Math.sin(at * BLINK_RATE);
    return at >= ABOUT_BEATS.printEnd ? 1 : 0.3;
  }
</script>

{#snippet segmentContent(segment: AboutSegment, index: number)}
  {#if segment === "header"}
    <div class="flex h-full flex-col items-center justify-center gap-0.5">
      <span class="text-[15px] leading-none font-bold tracking-tight">{t("common.appName")}</span>
      <span class="font-mono text-[8.5px] leading-none" style:color={INK_SOFT}>{printedOn}</span>
    </div>
  {:else if segment === "rule"}
    <div class="flex h-full items-center"><div class="h-px w-full" style:background-image={dashes}></div></div>
  {:else if segment === "item"}
    <div class="flex h-full items-center gap-1.5 font-mono text-[10px]">
      <span class="min-w-0 truncate">{ITEMS[index - FIRST_ITEM]()}</span>
      <span class="mb-1 min-w-2 flex-1 self-end border-b border-dotted" style:border-color={INK_RULE}></span>
      <span>{t("vaultSetup.about.receipt.free")}</span>
    </div>
  {:else if segment === "total"}
    <div class="flex h-full items-center justify-between font-mono text-[12.5px] font-bold">
      <span>{t("vaultSetup.about.receipt.total")}</span>
      <span>{t("vaultSetup.about.receipt.free")}</span>
    </div>
  {:else if segment === "barcode"}
    <div class="flex h-full items-center justify-center">
      <svg width={BARCODE_BARS.width} height={BARCODE_H} aria-hidden="true">
        {#each BARCODE_BARS.bars as bar, index (index)}
          <rect x={bar.x} y="0" width={bar.width} height={BARCODE_H} fill={INK} />
        {/each}
      </svg>
    </div>
  {/if}
{/snippet}

<div class="relative overflow-hidden" style:width={`${width}px`} style:height={`${height}px`}>
  {#if printer > 0}
    <div class="absolute inset-0" style:top={`${(1 - printer) * RISE}px`} style:opacity={printer}>
      <!-- The terminal's shadow on the table, and the back of its top face, behind the paper. -->
      <div
        class="absolute"
        style:left={`${printerLeft + 10}px`}
        style:top={`${printerTop + TOP_FACE_H + FRONT_H - 7}px`}
        style:width={`${PRINTER_W - 20}px`}
        style:height="16px"
        style:background="radial-gradient(closest-side, rgb(0 0 0 / 0.35), transparent)"
      ></div>
      <div
        class="absolute"
        style:left={`${printerLeft}px`}
        style:top={`${printerTop}px`}
        style:width={`${PRINTER_W}px`}
        style:height={`${TOP_FACE_H}px`}
        style:border-radius={`${PRINTER_RADIUS}px ${PRINTER_RADIUS}px 0 0`}
        style:background={TOP_FACE}
        style:box-shadow="inset 0 1px 0 rgb(255 255 255 / 0.18)"
      >
        <div
          class="absolute rounded-full"
          style:left={`${(PRINTER_W - SLOT_W) / 2}px`}
          style:top={`${SLOT_Y}px`}
          style:width={`${SLOT_W}px`}
          style:height={`${SLOT_H}px`}
          style:background-color={SLOT}
          style:box-shadow="0 1px 0 rgb(255 255 255 / 0.12)"
        ></div>
      </div>

      <!-- The receipt, fed out of the slot: its content rises with the paper, and what is not fed yet stays inside. -->
      {#if fed > 0}
        <div
          class="absolute"
          style:left={`${left}px`}
          style:top={`${slotY - fed + drop}px`}
          style:width={`${RECEIPT_W}px`}
          style:height={`${fed}px`}
          style:filter="drop-shadow(0 1px 1.5px rgb(0 0 0 / 0.22))"
        >
          <div class="relative h-full overflow-hidden" style:background={PAPER} style:clip-path={TORN_EDGE} style:color={INK}>
            <div class="px-3.5">
              {#each ABOUT_SEGMENTS as segment, index (index)}
                <div style:height={`${SEGMENT_H[segment]}px`}>{@render segmentContent(segment, index)}</div>
              {/each}
            </div>
            <!-- The paper darkens a little where it goes into the slot. -->
            <div class="absolute inset-x-0 bottom-0 h-3" style:background="linear-gradient(180deg, transparent, rgb(0 0 0 / 0.14))"></div>
          </div>
        </div>
      {/if}

      <!-- The front of the top face, in front of the paper, and the terminal's front face with its screen, keys, and light. -->
      <div
        class="absolute"
        style:left={`${printerLeft}px`}
        style:top={`${slotY}px`}
        style:width={`${PRINTER_W}px`}
        style:height={`${printerTop + TOP_FACE_H - slotY}px`}
        style:background={TOP_FACE_FRONT}
      ></div>
      <div
        class="absolute flex items-center justify-between px-3.5"
        style:left={`${printerLeft}px`}
        style:top={`${printerTop + TOP_FACE_H}px`}
        style:width={`${PRINTER_W}px`}
        style:height={`${FRONT_H}px`}
        style:border-radius={`0 0 ${PRINTER_RADIUS}px ${PRINTER_RADIUS}px`}
        style:background={FRONT_FACE}
        style:box-shadow="inset 0 1px 0 rgb(255 255 255 / 0.1)"
      >
        <div
          class="rounded-lg p-0.75"
          style:background-color={BEZEL}
          style:box-shadow="0 1px 0 rgb(255 255 255 / 0.1), inset 0 1px 2px rgb(0 0 0 / 0.6)"
        >
          <div
            class="relative overflow-hidden rounded-[5px]"
            style:width={`${SCREEN_W}px`}
            style:height={`${SCREEN_H}px`}
            style:background-color={SCREEN_OFF}
          >
            <div class="absolute inset-0" style:background={SCREEN_ON} style:opacity={screen}></div>
            {#if total > 0}
              <div
                class="absolute inset-0 flex flex-col items-center justify-center gap-0.5"
                style:opacity={total}
                style:transform={`translateY(${(1 - total) * (ring > 0 ? -SCREEN_SLIDE : SCREEN_SLIDE)}px)`}
              >
                <span class="text-[8px] leading-none font-semibold tracking-wider uppercase" style:color={SCREEN_TEXT_SOFT}>
                  {t("vaultSetup.about.receipt.total")}
                </span>
                <span class="text-[16px] leading-none font-bold" style:color={SCREEN_TEXT}>{t("vaultSetup.about.receipt.free")}</span>
              </div>
            {/if}
            {#if ring > 0}
              <div class="absolute inset-0 flex items-center justify-center gap-1.5 px-2">
                <svg class="shrink-0" width={CONFIRM_SIZE} height={CONFIRM_SIZE} viewBox={`0 0 ${CONFIRM_SIZE} ${CONFIRM_SIZE}`} aria-hidden="true">
                  <circle
                    cx={CONFIRM_SIZE / 2}
                    cy={CONFIRM_SIZE / 2}
                    r={CONFIRM_R}
                    fill={CONFIRM_GREEN}
                    fill-opacity={0.16 * check}
                    stroke={CONFIRM_GREEN}
                    stroke-width="1.8"
                    stroke-linecap="round"
                    stroke-dasharray={CONFIRM_CIRCUMFERENCE}
                    stroke-dashoffset={CONFIRM_CIRCUMFERENCE * (1 - ring)}
                    transform={`rotate(-90 ${CONFIRM_SIZE / 2} ${CONFIRM_SIZE / 2})`}
                  />
                  <path
                    d={CHECK_PATH}
                    fill="none"
                    stroke={CONFIRM_GREEN}
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    pathLength="1"
                    stroke-dasharray="1"
                    stroke-dashoffset={1 - check}
                  />
                </svg>
                <span
                  class="min-w-0 truncate text-[11px] leading-none font-semibold"
                  style:color={SCREEN_TEXT}
                  style:opacity={ring}
                  style:transform={`translateX(${(1 - ring) * -SCREEN_SLIDE}px)`}
                >
                  {t("vaultSetup.about.receipt.confirmed")}
                </span>
              </div>
            {/if}
            <div class="absolute inset-0" style:background={SCREEN_GLARE}></div>
          </div>
        </div>

        <div class="flex flex-col items-end gap-1.5">
          <span
            class="mr-0.5 size-1.5 rounded-full"
            style:background-color={LIGHT}
            style:opacity={light}
            style:box-shadow={`0 0 ${4 + light * 4}px ${LIGHT}`}
          ></span>
          <div class="grid gap-0.75" style:grid-template-columns={`repeat(${KEY_COLUMNS}, 1fr)`}>
            {#each { length: KEY_ROWS * KEY_COLUMNS } as _, index (index)}
              <span
                class="h-2 w-4 rounded-[2px]"
                style:background={KEY}
                style:box-shadow="inset 0 1px 0 rgb(255 255 255 / 0.12), 0 1px 0 rgb(0 0 0 / 0.4)"
              ></span>
            {/each}
          </div>
        </div>
      </div>

      <!-- The rubber stamp, coming down onto the printed receipt in a textured red ink. -->
      {#if stamp > 0}
        <svg
          class="absolute overflow-visible"
          viewBox="0 0 100 100"
          width={STAMP_SIZE}
          height={STAMP_SIZE}
          style:left={`${left + RECEIPT_W - STAMP_INSET_X - STAMP_SIZE / 2}px`}
          style:top={`${slotY - STAMP_RISE - STAMP_SIZE / 2 + drop}px`}
          style:opacity={Math.min(1, stamp * 3) * 0.92}
          style:transform={`rotate(${STAMP_TILT}deg) scale(${STAMP_SCALE_FROM - (STAMP_SCALE_FROM - 1) * stamp})`}
          aria-hidden="true"
        >
          <defs>
            <filter id={`${uid}-ink`} x="-5%" y="-5%" width="110%" height="110%">
              <feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" seed="7" result="noise" />
              <feDisplacementMap in="SourceGraphic" in2="noise" scale="1.6" result="rough" />
              <feColorMatrix in="noise" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 -2.4 2.05" result="speckle" />
              <feComposite in="rough" in2="speckle" operator="in" />
            </filter>
            <filter id={`${uid}-tint`}>
              <feFlood flood-color={STAMP_INK} result="ink" />
              <feComposite in="ink" in2="SourceAlpha" operator="in" />
            </filter>
            <path id={`${uid}-ring`} d={`M 50 ${50 - RING_TEXT_R} a ${RING_TEXT_R} ${RING_TEXT_R} 0 1 1 -0.01 0`} />
          </defs>
          <g filter={`url(#${uid}-ink)`} fill={STAMP_INK} stroke={STAMP_INK}>
            <circle cx="50" cy="50" r="46" fill="none" stroke-width="3.4" />
            <circle cx="50" cy="50" r="41" fill="none" stroke-width="1.1" />
            <circle cx="50" cy="50" r="25" fill="none" stroke-width="1.1" />
            <text font-size="7.2" font-weight="700" stroke="none" letter-spacing="0.4">
              <textPath href={`#${uid}-ring`} textLength={RING_CIRCUMFERENCE - 4} lengthAdjust="spacing">
                {t("vaultSetup.about.receipt.stamp")} ★ AGPL 3.0 ★
              </textPath>
            </text>
            <g filter={`url(#${uid}-tint)`}>
              <image href={LOGO_URL} x={50 - LOGO_SIZE / 2} y={50 - LOGO_SIZE / 2} width={LOGO_SIZE} height={LOGO_SIZE} />
            </g>
          </g>
        </svg>
      {/if}
    </div>
  {/if}
</div>
