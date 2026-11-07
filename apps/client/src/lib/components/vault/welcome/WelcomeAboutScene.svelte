<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 300;
  const MAX_CANVAS_W = 520;
  const MIN_CANVAS_H = 490;
  const MAX_CANVAS_H = 540;

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
   * One frame of the welcome scene about the app itself: a handheld payment terminal, shaped like a phone with a receipt
   * printer on top, shows a total that is free and confirms there is nothing to pay. A receipt then unrolls above it from
   * the bottom up, its curled edge rising as it goes: the barcode and the free total first, then every feature priced
   * free, and the header last. A rubber stamp with the Open Grimoire logo then marks the receipt with the open source
   * license. The terminal, the thermal paper, and the ink are drawn as the physical things they are, in their own colors,
   * apart from the printer's lid in the theme's primary color, so the receipt reads as a receipt in every theme. Every
   * style is a function of `elapsed`, with no CSS transitions, so text is never handed to a compositing layer
   * mid-animation.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const uid = $props.id();

  const RECEIPT_W = 156;
  /** The height of each kind of segment on the receipt. */
  const SEGMENT_H: Readonly<Record<AboutSegment, number>> = {
    lead: 12,
    header: 30,
    rule: 8,
    item: 12,
    total: 18,
    barcode: 26,
    tail: 6,
  };
  const PAPER_H = ABOUT_SEGMENTS.reduce((sum, segment) => sum + SEGMENT_H[segment], 0);
  /** The paper's curled edge: its thickness, how much of it overlaps the flat paper, and the shadow it casts there. */
  const CURL_H = 12;
  const CURL_OVERLAP = 3;
  const CURL_SHADOW_H = 9;

  /**
   * The terminal, seen from the front: a phone-shaped body whose top edge holds the paper slot, with the printer's lid
   * rising behind the paper above it.
   */
  const DEVICE_W = RECEIPT_W + 24;
  const LID_INSET = 3;
  const LID_H = 16;
  const HEAD_H = 14;
  const SCREEN_INSET = 9;
  const SCREEN_H = 224;
  const CHIN_H = 22;
  const BODY_H = HEAD_H + SCREEN_H + CHIN_H;
  const BODY_RADIUS = 26;
  const HEAD_RADIUS = 10;
  const SCREEN_RADIUS = 12;
  const SIDE_BUTTON_Y = 64;
  const SIDE_BUTTON_H = 30;

  /** The confirmation on the screen: the ring's size and radius, and the check drawn inside it. */
  const CONFIRM_SIZE = 52;
  const CONFIRM_R = 23;
  const CONFIRM_CIRCUMFERENCE = 2 * Math.PI * CONFIRM_R;
  const CHECK_PATH = "M16.5 26.5 L23.5 33.5 L36.5 19.5";
  /** How far the screen's text slides as it comes and goes. */
  const SCREEN_SLIDE = 6;

  /** How far the terminal rises into place as it appears, and how far the receipt drops when the stamp lands. */
  const RISE = 8;
  const IMPACT_DROP = 1.5;

  const STAMP_SIZE = 78;
  const STAMP_TILT = -14;
  const STAMP_SCALE_FROM = 1.7;
  /** Where the stamp's center sits: in from the receipt's right edge, and up from the slot. */
  const STAMP_INSET_X = 44;
  const STAMP_RISE = 70;
  /** The radius the stamp's ring text runs along, and the logo's size, in the stamp's 100 unit drawing. */
  const RING_TEXT_R = 33;
  const LOGO_SIZE = 36;

  /** How fast the printer's light blinks while it prints. */
  const BLINK_RATE = 1 / 70;

  /** Bar and space widths of the barcode, alternating, in units of one thin bar. */
  const BARCODE = "21131221312112313122113212311213221131213121132112".split("").map(Number);
  const BARCODE_UNIT = 1.1;
  const BARCODE_H = 15;

  /** Thermal paper, its ink, and the stamp's ink, which keep their own colors in every theme. */
  const PAPER = "linear-gradient(90deg, #eeede8 0%, #fcfbf7 12%, #fdfcf9 88%, #ecebe6 100%)";
  const CURL =
    "linear-gradient(180deg, #c9c8c2 0%, #efeee9 28%, #ffffff 46%, #f3f2ed 66%, #d6d5cf 88%, #b9b8b2 100%)";
  const INK = "#28282c";
  const INK_SOFT = "#77777e";
  const INK_RULE = "#b4b4ba";
  const STAMP_INK = "#c42f3c";
  /** The terminal's body, the slot's tear bar, the lid's cylinder shading over the theme color, and the light. */
  const BODY = "linear-gradient(90deg, #1b1c1f 0%, #2c2e33 10%, #26282c 85%, #18191b 100%)";
  const BODY_EDGE = "inset 0 1px 0 rgb(255 255 255 / 0.14), inset 1px 0 0 rgb(255 255 255 / 0.05), inset -1px 0 0 rgb(0 0 0 / 0.4)";
  const TEAR_BAR = "repeating-linear-gradient(90deg, #0d0d0f 0 2px, #3a3c41 2px 3px)";
  const LID_SHADE =
    "linear-gradient(90deg, rgb(0 0 0 / 0.32) 0%, rgb(255 255 255 / 0.2) 22%, rgb(255 255 255 / 0.04) 55%, rgb(0 0 0 / 0.3) 100%)";
  const LIGHT = "#4ade80";
  /** The screen: its glass when off, its light interface when lit, its text, and the confirmation's green. */
  const SCREEN_OFF = "#0e0f11";
  const SCREEN_ON = "linear-gradient(180deg, #ffffff 0%, #f1f3f6 100%)";
  const SCREEN_GLARE = "linear-gradient(120deg, rgb(255 255 255 / 0.1) 0%, rgb(255 255 255 / 0.03) 42%, transparent 44%)";
  const SCREEN_TEXT = "#1d2127";
  const SCREEN_TEXT_SOFT = "#6b7280";
  const CONFIRM_GREEN = "#16a34a";

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
  const clock = $derived(formatDateTime(localization.locale, printedAt, { timeStyle: "short" }));

  const center = $derived(width / 2);
  const left = $derived(center - RECEIPT_W / 2);
  const deviceLeft = $derived(center - DEVICE_W / 2);
  /** The receipt with its curl and the terminal, centered together in the canvas, meeting at the slot. */
  const slotY = $derived((height - PAPER_H - BODY_H + CURL_H - CURL_OVERLAP) / 2 + PAPER_H);
  const printer = $derived(aboutPrinter(elapsed));
  const screen = $derived(aboutScreen(elapsed));
  const total = $derived(aboutTotal(elapsed));
  const ring = $derived(aboutConfirmRing(elapsed));
  const check = $derived(aboutConfirmCheck(elapsed));
  /** How much of the receipt has unrolled, measured up from the slot. */
  const fed = $derived(ABOUT_SEGMENTS.reduce((sum, segment, index) => sum + SEGMENT_H[segment] * aboutFeed(elapsed, index), 0));
  const drop = $derived(aboutImpact(elapsed) * IMPACT_DROP);
  const paperTop = $derived(slotY - fed + drop);
  const stamp = $derived(aboutStamp(elapsed));
  const light = $derived(lightLevel(elapsed));

  /** How bright the printer's light is: dim while it waits, blinking while it prints, and steady once done. */
  function lightLevel(at: number): number {
    if (aboutPrinting(at)) return 0.55 + 0.45 * Math.sin(at * BLINK_RATE);
    return at >= ABOUT_BEATS.printEnd ? 1 : 0.25;
  }
</script>

{#snippet segmentContent(segment: AboutSegment, index: number)}
  {#if segment === "header"}
    <div class="flex h-full flex-col items-center justify-center gap-0.75">
      <span class="text-[13px] leading-none font-bold tracking-tight">{t("common.appName")}</span>
      <span class="font-mono text-[7.5px] leading-none" style:color={INK_SOFT}>{printedOn}</span>
    </div>
  {:else if segment === "rule"}
    <div class="flex h-full items-center"><div class="h-px w-full" style:background-image={dashes}></div></div>
  {:else if segment === "item"}
    <div class="flex h-full items-center gap-1 font-mono text-[8px]">
      <span class="min-w-0 truncate">{ITEMS[index - FIRST_ITEM]()}</span>
      <span class="mb-0.75 min-w-1 flex-1 self-end border-b border-dotted" style:border-color={INK_RULE}></span>
      <span>{t("vaultSetup.about.receipt.free")}</span>
    </div>
  {:else if segment === "total"}
    <div class="flex h-full items-center justify-between font-mono text-[10.5px] font-bold">
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
      <!-- The terminal's shadow on the table, and the printer's lid in the theme's color, behind the paper. -->
      <div
        class="absolute"
        style:left={`${deviceLeft + 12}px`}
        style:top={`${slotY + BODY_H - 8}px`}
        style:width={`${DEVICE_W - 24}px`}
        style:height="18px"
        style:background="radial-gradient(closest-side, rgb(0 0 0 / 0.38), transparent)"
      ></div>
      <div
        class="absolute overflow-hidden bg-primary"
        style:left={`${deviceLeft + LID_INSET}px`}
        style:top={`${slotY - LID_H}px`}
        style:width={`${DEVICE_W - LID_INSET * 2}px`}
        style:height={`${LID_H + HEAD_H}px`}
        style:border-radius={`${LID_H}px ${LID_H}px 0 0`}
      >
        <div class="absolute inset-0" style:background={LID_SHADE}></div>
        <div class="absolute inset-x-0 top-0 h-1.5" style:background="linear-gradient(180deg, rgb(255 255 255 / 0.22), transparent)"></div>
      </div>

      <!-- The flat receipt, unrolled up from the slot: the printed lines stay still as more of them are uncovered. -->
      {#if fed > 0}
        <div
          class="absolute overflow-hidden"
          style:left={`${left}px`}
          style:top={`${paperTop}px`}
          style:width={`${RECEIPT_W}px`}
          style:height={`${slotY - paperTop}px`}
          style:background={PAPER}
          style:color={INK}
          style:box-shadow="0 1px 2px rgb(0 0 0 / 0.2)"
        >
          <div class="absolute inset-x-0 px-2.5" style:bottom="0" style:height={`${PAPER_H}px`}>
            {#each ABOUT_SEGMENTS as segment, index (index)}
              <div style:height={`${SEGMENT_H[segment]}px`}>{@render segmentContent(segment, index)}</div>
            {/each}
          </div>
          <!-- The paper darkens a little where it goes into the slot. -->
          <div class="absolute inset-x-0 bottom-0 h-3" style:background="linear-gradient(180deg, transparent, rgb(0 0 0 / 0.16))"></div>
        </div>
      {/if}

      <!-- The curled edge of the paper, still rolled, rising as the receipt unrolls, with its shadow on the flat paper. -->
      <div
        class="absolute"
        style:left={`${left - 1}px`}
        style:top={`${paperTop - CURL_H + CURL_OVERLAP}px`}
        style:width={`${RECEIPT_W + 2}px`}
        style:height={`${CURL_H}px`}
      >
        <div
          class="absolute inset-x-0.5 top-full"
          style:height={`${Math.min(CURL_SHADOW_H, fed)}px`}
          style:background="linear-gradient(180deg, rgb(0 0 0 / 0.18), transparent)"
        ></div>
        <div
          class="absolute inset-0 rounded-[3px]"
          style:background={CURL}
          style:box-shadow="0 1px 1.5px rgb(0 0 0 / 0.25)"
        ></div>
      </div>

      <!-- The body: the slot's tear bar along its top edge, the side button, the screen, and the printer's light. -->
      <div
        class="absolute"
        style:left={`${deviceLeft}px`}
        style:top={`${slotY}px`}
        style:width={`${DEVICE_W}px`}
        style:height={`${BODY_H}px`}
        style:border-radius={`${HEAD_RADIUS}px ${HEAD_RADIUS}px ${BODY_RADIUS}px ${BODY_RADIUS}px`}
        style:background={BODY}
        style:box-shadow={BODY_EDGE}
      >
        <div class="absolute inset-x-3 top-0 h-0.75 rounded-b-xs" style:background={TEAR_BAR}></div>
        <div
          class="absolute -right-0.75 w-0.75 rounded-r-xs bg-[#18191b]"
          style:top={`${SIDE_BUTTON_Y}px`}
          style:height={`${SIDE_BUTTON_H}px`}
        ></div>
        <span
          class="absolute right-4 size-1.25 rounded-full"
          style:top={`${HEAD_H / 2 - 1}px`}
          style:background-color={LIGHT}
          style:opacity={light}
          style:box-shadow={`0 0 ${3 + light * 4}px ${LIGHT}`}
        ></span>

        <div
          class="absolute overflow-hidden"
          style:left={`${SCREEN_INSET}px`}
          style:top={`${HEAD_H}px`}
          style:width={`${DEVICE_W - SCREEN_INSET * 2}px`}
          style:height={`${SCREEN_H}px`}
          style:border-radius={`${SCREEN_RADIUS}px`}
          style:background-color={SCREEN_OFF}
          style:box-shadow="0 0 0 1px rgb(0 0 0 / 0.6)"
        >
          <div class="absolute inset-0" style:background={SCREEN_ON} style:opacity={screen}></div>
          {#if screen > 0}
            <div
              class="absolute inset-x-0 top-0 flex h-4 items-center justify-between px-3 text-[7.5px] font-semibold"
              style:color={SCREEN_TEXT}
              style:opacity={screen}
            >
              <span>{clock}</span>
              <span class="flex items-center gap-px" aria-hidden="true">
                <span class="h-1.5 w-2.75 rounded-[1.5px] border border-current p-px"><span class="block h-full w-3/4 rounded-[0.5px] bg-current"></span></span>
                <span class="h-0.75 w-px rounded-r-xs bg-current"></span>
              </span>
            </div>
            <div class="absolute bottom-1.5 left-1/2 h-0.75 w-12 -translate-x-1/2 rounded-full" style:background-color={SCREEN_TEXT} style:opacity={screen * 0.35}></div>
          {/if}
          {#if total > 0}
            <div
              class="absolute inset-0 flex flex-col items-center justify-center gap-1.5"
              style:opacity={total}
              style:transform={`translateY(${(1 - total) * (ring > 0 ? -SCREEN_SLIDE : SCREEN_SLIDE)}px)`}
            >
              <span class="text-[10px] leading-none font-semibold tracking-wider uppercase" style:color={SCREEN_TEXT_SOFT}>
                {t("vaultSetup.about.receipt.total")}
              </span>
              <span class="text-[30px] leading-none font-bold tracking-tight" style:color={SCREEN_TEXT}>{t("vaultSetup.about.receipt.free")}</span>
            </div>
          {/if}
          {#if ring > 0}
            <div class="absolute inset-0 flex flex-col items-center justify-center gap-2.5 px-3">
              <svg width={CONFIRM_SIZE} height={CONFIRM_SIZE} viewBox={`0 0 ${CONFIRM_SIZE} ${CONFIRM_SIZE}`} aria-hidden="true">
                <circle
                  cx={CONFIRM_SIZE / 2}
                  cy={CONFIRM_SIZE / 2}
                  r={CONFIRM_R}
                  fill={CONFIRM_GREEN}
                  fill-opacity={0.12 * check}
                  stroke={CONFIRM_GREEN}
                  stroke-width="2.5"
                  stroke-linecap="round"
                  stroke-dasharray={CONFIRM_CIRCUMFERENCE}
                  stroke-dashoffset={CONFIRM_CIRCUMFERENCE * (1 - ring)}
                  transform={`rotate(-90 ${CONFIRM_SIZE / 2} ${CONFIRM_SIZE / 2})`}
                />
                <path
                  d={CHECK_PATH}
                  fill="none"
                  stroke={CONFIRM_GREEN}
                  stroke-width="3"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  pathLength="1"
                  stroke-dasharray="1"
                  stroke-dashoffset={1 - check}
                />
              </svg>
              <span
                class="text-center text-[14px] leading-tight font-semibold"
                style:color={SCREEN_TEXT}
                style:opacity={ring}
                style:transform={`translateY(${(1 - ring) * SCREEN_SLIDE}px)`}
              >
                {t("vaultSetup.about.receipt.confirmed")}
              </span>
            </div>
          {/if}
          <div class="absolute inset-0" style:background={SCREEN_GLARE}></div>
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
