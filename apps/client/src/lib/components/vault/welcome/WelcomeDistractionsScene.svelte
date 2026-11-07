<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 300;
  const MAX_CANVAS_W = 760;
  const MIN_CANVAS_H = 400;
  const MAX_CANVAS_H = 500;

  export const DISTRACTIONS_SCENE_MIN_WIDTH = MIN_CANVAS_W;
  export const DISTRACTIONS_SCENE_MAX_WIDTH = MAX_CANVAS_W;
  export const DISTRACTIONS_SCENE_MIN_HEIGHT = MIN_CANVAS_H;
  export const DISTRACTIONS_SCENE_MAX_HEIGHT = MAX_CANVAS_H;
</script>

<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Check from "@lucide/svelte/icons/check";
  import FileText from "@lucide/svelte/icons/file-text";
  import Globe from "@lucide/svelte/icons/globe";
  import Plus from "@lucide/svelte/icons/plus";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import Smartphone from "@lucide/svelte/icons/smartphone";
  import X from "@lucide/svelte/icons/x";
  import appIconUrl from "../../../../../src-tauri/icons/64x64.png";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import { getEventColor } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM } from "$lib/platform";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { cn } from "$lib/utils";
  import {
    DISTRACTIONS_BLOCK_MINUTES_LEFT,
    DISTRACTIONS_FOCUS_HOST,
    DISTRACTIONS_FOCUS_SECONDS,
    DISTRACTIONS_LIMIT_HOST,
    DISTRACTIONS_LIMIT_MINUTES,
    distractionsAddress,
    distractionsBrowserPage,
    distractionsCaretOn,
    distractionsFeedScroll,
    distractionsFocusing,
    distractionsFocusRemaining,
    distractionsLimitShown,
    distractionsLoading,
    distractionsNewTabOpen,
    distractionsNotification,
    distractionsPageIn,
    distractionsPan,
    distractionsPhoneApp,
    distractionsPulse,
    distractionsTouches,
    distractionsUsage,
    type DistractionsApp,
  } from "$lib/vault/welcome-distractions-scene";

  /**
   * One frame of the distraction blocker's welcome scene: a laptop's browser and a phone under one Focus timer. The
   * browser draws the extension's real block page, which is in English only, so its text stays in English in every app
   * language; the phone draws Android's Home action and the app's block notification. Once the day's Focus is done,
   * the timer gives way to a daily limit drawn as Settings draws it, with one source on each device. A wide canvas
   * shows both devices side by side; a narrow one moves the camera to the device that acts. Every style is a function
   * of `elapsed`, with no CSS transitions, so text is never handed to a compositing layer mid-animation.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const { t } = getLocalization();
  const theme = getTheme();

  /** The extension's block page text, which it shows in English only. */
  const PAGE_LOCALE = "en";
  const PAGE_TEXT = {
    tabTitle: "Blocked during focus",
    focusTitle: `${DISTRACTIONS_FOCUS_HOST} is blocked`,
    focusCopy: "Stay strong and keep moving forward.",
    focusTime: `${DISTRACTIONS_BLOCK_MINUTES_LEFT} min left in focus`,
    limitTitle: `${DISTRACTIONS_LIMIT_HOST} reached today's limit`,
    limitCopy: "Come back after the limit resets.",
    limitTime: "Daily limit reached",
    recheck: "Recheck",
    closeTab: "Close tab",
  } as const;

  /** The parody apps' names, which stay the same in every language. */
  const APP_NAMES: Record<DistractionsApp, string> = { y: "Y", bluedit: "Bluedit", egogram: "Egogram" };
  /** The parody apps' brand colors, the same in every theme. */
  const BLUEDIT_BLUE = "#2f6bff";
  const EGOGRAM_TEAL = "#0f9d8a";
  /** The block page's colors, which the extension keeps dark in every theme. */
  const BLOCK_PAGE = { bg: "#0f1110", fg: "#f4f5f0", muted: "#aab5ac", button: "#f4f5f0", buttonFg: "#101010" } as const;

  /** Palette slots for the daily limit's sources, so the drawing follows the active theme's event colors. */
  const BROWSER_COLOR = 18;
  const PHONE_COLOR = 22;

  /** The room taken by the Focus timer and the daily limit above the devices, and the gap below it. */
  const HEADER_H = 64;
  const HEADER_GAP = 12;
  /** From this canvas width, both devices fit side by side; below it the camera moves between them. */
  const WIDE_MIN_W = 520;
  const DEVICE_GAP = 16;
  /** The phone's width against its height, and its tallest size beside the browser. */
  const PHONE_ASPECT = 0.5;
  const PHONE_MAX_H = 380;
  const BEZEL = 6;
  /** The phone's home screen: its status bar, its icon grid of four columns, and its side padding. */
  const STATUS_H = 18;
  const GRID_COLUMNS = 4;
  const GRID_PAD_X = 8;
  const GRID_TOP = STATUS_H + 14;
  const ICON_LABEL_H = 16;
  /** The home screen's slots, row by row, with the parody apps among unnamed ones. */
  const HOME_SLOTS: readonly (DistractionsApp | null)[] = [null, null, "bluedit", null, null, "egogram", null, "y", null, null, null, null];
  const DOCK_SLOTS = 4;
  /** How far the Focus timer grows at the peak of a pulse. */
  const PULSE_GROWTH = 0.14;
  /** How far the feeds scroll at their end, in pixels. */
  const BROWSER_FEED_TRAVEL = 150;
  const PHONE_FEED_TRAVEL = 170;
  /** The widths of the placeholder lines of the work document and the feeds' posts, as fractions of their room. */
  const DOC_WIDTHS = [1, 0.94, 0.97, 0.62, 0, 1, 0.9, 0.96, 0.85, 0.48, 0, 0.98, 0.92, 0.7];
  const POST_WIDTHS = [0.9, 0.7, 0.82, 0.55, 0.76, 0.64];
  /** Apple's windows keep their controls on the left; other systems keep them on the right. */
  const CONTROLS_LEADING = BUILD_PLATFORM === "macos" || BUILD_PLATFORM === "ios";

  const bodyH = $derived(height - HEADER_H - HEADER_GAP);
  const wide = $derived(width >= WIDE_MIN_W);
  const phoneH = $derived(wide ? Math.min(bodyH, PHONE_MAX_H) : bodyH);
  const phoneW = $derived(Math.round(phoneH * PHONE_ASPECT));
  const browserW = $derived(wide ? width - phoneW - DEVICE_GAP : width);
  const phoneX = $derived(browserW + DEVICE_GAP);
  const cameraX = $derived(wide ? 0 : distractionsPan(elapsed) * (phoneX - (width - phoneW) / 2));

  const screenW = $derived(phoneW - 2 * BEZEL);
  const screenH = $derived(phoneH - 2 * BEZEL);
  const cellW = $derived((screenW - 2 * GRID_PAD_X) / GRID_COLUMNS);
  const iconSize = $derived(Math.min(30, Math.floor(cellW - 8)));
  const rowH = $derived(iconSize + ICON_LABEL_H);

  const focusing = $derived(distractionsFocusing(elapsed));
  const remaining = $derived(distractionsFocusRemaining(elapsed));
  const pulse = $derived(distractionsPulse(elapsed));
  const limitShown = $derived(distractionsLimitShown(elapsed));
  const page = $derived(distractionsBrowserPage(elapsed));
  const pageIn = $derived(distractionsPageIn(elapsed));
  const address = $derived(distractionsAddress(elapsed));
  const loading = $derived(distractionsLoading(elapsed));
  const newTabOpen = $derived(distractionsNewTabOpen(elapsed));
  const touches = $derived(distractionsTouches(elapsed));
  const phoneApp = $derived(distractionsPhoneApp(elapsed));
  const notification = $derived(distractionsNotification(elapsed));
  const usage = $derived(distractionsUsage(elapsed));
  const usedMinutes = $derived(Math.floor(usage.browser + usage.phone));
  const browserColor = $derived(getEventColor(BROWSER_COLOR, theme.current).bg);
  const phoneColor = $derived(getEventColor(PHONE_COLOR, theme.current).bg);
  /** The work tab carries the limited site once it opens there. */
  const workTabOnSite = $derived(page === "feed" || page === "limitBlock");

  /** A remaining time as the Focus timer shows it, in minutes and seconds. */
  function clock(seconds: number): string {
    const whole = Math.ceil(seconds);
    return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
  }

  /** Minutes of use as Settings shows them, in hours and minutes. */
  function usageLabel(minutes: number): string {
    if (minutes < 60) return `${minutes}m`;
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return rest > 0 ? `${hours}h ${rest}m` : `${hours}h`;
  }

  /** The center of a home screen slot's icon, in pixels from the phone screen's corner. */
  function slotCenter(slot: number): { x: number; y: number } {
    const column = slot % GRID_COLUMNS;
    const row = Math.floor(slot / GRID_COLUMNS);
    return { x: GRID_PAD_X + cellW * (column + 0.5), y: GRID_TOP + rowH * row + iconSize / 2 };
  }

  const openSlot = $derived(phoneApp ? HOME_SLOTS.indexOf(phoneApp.app) : -1);
  const openOrigin = $derived(openSlot >= 0 ? slotCenter(openSlot) : { x: screenW / 2, y: screenH / 2 });
</script>

{#snippet touch(opacity: number)}
  {#if opacity > 0}
    <span
      class="pointer-events-none absolute top-1/2 left-1/2 z-20 size-6 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35"
      style:opacity
    ></span>
  {/if}
{/snippet}

{#snippet line(fraction: number, tone = "bg-foreground/12")}
  <span class={cn("block h-1.5 rounded-full", tone)} style:width={`${fraction * 100}%`}></span>
{/snippet}

{#snippet brand(app: DistractionsApp, size: number)}
  <span
    class="relative grid shrink-0 place-items-center overflow-hidden"
    style:width={`${size}px`}
    style:height={`${size}px`}
    style:border-radius={`${size * 0.26}px`}
    style:background-color={app === "y" ? "#000000" : app === "bluedit" ? BLUEDIT_BLUE : EGOGRAM_TEAL}
  >
    {#if app === "y"}
      <span class="absolute inset-0 rounded-[inherit] ring-1 ring-white/15 ring-inset"></span>
      <span class="font-black leading-none text-white" style:font-size={`${size * 0.62}px`}>Y</span>
    {:else if app === "bluedit"}
      <svg viewBox="0 0 24 24" style:width={`${size * 0.74}px`} style:height={`${size * 0.74}px`} aria-hidden="true">
        <path d="M12 3.5c-4.7 0-8.5 3.1-8.5 7 0 2.1 1.1 4 2.9 5.3L5.7 20l3.9-2c.8.2 1.6.3 2.4.3 4.7 0 8.5-3.1 8.5-7s-3.8-7.8-8.5-7.8z" fill="white" />
        <path d="M12 6.8l-3.4 3.6h2.2v3.4h2.4v-3.4h2.2z" fill={BLUEDIT_BLUE} />
      </svg>
    {:else}
      <svg viewBox="0 0 24 24" style:width={`${size * 0.74}px`} style:height={`${size * 0.74}px`} aria-hidden="true">
        <circle cx="12" cy="9.5" r="5.2" fill="none" stroke="white" stroke-width="2.2" />
        <path d="M9.6 7.6a3 3 0 0 1 2.4-1.3" fill="none" stroke="white" stroke-width="1.4" stroke-linecap="round" />
        <path d="M12 14.8V21" stroke="white" stroke-width="2.6" stroke-linecap="round" />
      </svg>
    {/if}
  </span>
{/snippet}

{#snippet controls()}
  <span class="flex shrink-0 gap-1 px-1" aria-hidden="true">
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
  </span>
{/snippet}

{#snippet tab(active: boolean, icon: "work" | "site" | "blocked", label: string | null, closable: boolean)}
  <span
    class={cn(
      "flex h-6 min-w-0 flex-1 basis-0 items-center gap-1.5 rounded-t-md px-2 text-[10px]",
      active ? "bg-background text-foreground" : "text-muted-foreground",
    )}
    style:max-width="9rem"
  >
    {#if icon === "work"}
      <FileText size={11} strokeWidth={1.8} class="shrink-0 text-primary" aria-hidden="true" />
    {:else if icon === "blocked"}
      <img src={appIconUrl} alt="" class="size-3 shrink-0 rounded-full" draggable="false" />
    {:else}
      {@render brand("y", 11)}
    {/if}
    {#if label === null}
      <span class="h-1.5 w-12 rounded-full bg-foreground/12"></span>
    {:else}
      <span class="min-w-0 flex-1 truncate">{label}</span>
    {/if}
    {#if closable}<X size={10} strokeWidth={1.8} class="ml-auto shrink-0 opacity-60" aria-hidden="true" />{/if}
  </span>
{/snippet}

{#snippet blockPage(kind: "focus" | "limit")}
  <div
    class="absolute inset-0 flex flex-col items-center justify-center gap-1.5 px-4 text-center"
    lang={PAGE_LOCALE}
    style:background-color={BLOCK_PAGE.bg}
    style:color={BLOCK_PAGE.fg}
  >
    <span class="mb-1.5">{@render brand(kind === "focus" ? "bluedit" : "y", 22)}</span>
    <p class="max-w-full truncate text-[15px] leading-tight font-bold">{kind === "focus" ? PAGE_TEXT.focusTitle : PAGE_TEXT.limitTitle}</p>
    <p class="text-[10.5px] leading-snug" style:color={BLOCK_PAGE.muted}>{kind === "focus" ? PAGE_TEXT.focusCopy : PAGE_TEXT.limitCopy}</p>
    <p class="text-[10px] font-bold">{kind === "focus" ? PAGE_TEXT.focusTime : PAGE_TEXT.limitTime}</p>
    <div class="mt-3 flex gap-2 text-[10px] font-semibold">
      {#each [PAGE_TEXT.recheck, PAGE_TEXT.closeTab] as action (action)}
        <span
          class="relative flex h-6 min-w-16 items-center justify-center rounded-md px-3"
          style:background-color={BLOCK_PAGE.button}
          style:color={BLOCK_PAGE.buttonFg}
        >
          {action}
          {#if kind === "focus" && action === PAGE_TEXT.closeTab}{@render touch(touches.close)}{/if}
        </span>
      {/each}
    </div>
  </div>
{/snippet}

{#snippet post(index: number, media: boolean)}
  <div class="flex gap-2 border-b border-border/60 px-3 py-2.5">
    <span class="size-6 shrink-0 rounded-full bg-foreground/12"></span>
    <div class="flex min-w-0 flex-1 flex-col gap-1.5">
      <span class="flex gap-1.5">
        <span class="h-1.5 w-12 rounded-full bg-foreground/25"></span>
        <span class="h-1.5 w-8 rounded-full bg-foreground/10"></span>
      </span>
      {@render line(POST_WIDTHS[index % POST_WIDTHS.length])}
      {@render line(POST_WIDTHS[(index + 3) % POST_WIDTHS.length] * 0.7)}
      {#if media}<span class="mt-0.5 block h-14 rounded-md bg-foreground/8"></span>{/if}
    </div>
  </div>
{/snippet}

{#snippet yFeed(scroll: number, travel: number)}
  <div class="flex h-full flex-col overflow-hidden bg-background">
    <div class="z-1 flex h-8 shrink-0 items-center justify-center border-b border-border bg-background">{@render brand("y", 16)}</div>
    <div class="flex flex-col" style:transform={`translateY(${-scroll * travel}px)`}>
      {#each [0, 1, 2, 3, 4, 5, 6] as index (index)}{@render post(index, index % 3 === 1)}{/each}
    </div>
  </div>
{/snippet}

{#snippet egogramApp()}
  <div class="flex h-full flex-col overflow-hidden bg-background">
    <div class="flex h-7 shrink-0 items-center gap-1.5 px-2.5" style:padding-top={`${STATUS_H - 8}px`}>
      <span class="text-[12px] font-semibold italic tracking-tight">{APP_NAMES.egogram}</span>
    </div>
    <div class="flex shrink-0 gap-2 px-2.5 py-2">
      {#each [0, 1, 2, 3] as index (index)}
        <span class="size-7 shrink-0 rounded-full border-2 bg-foreground/10" style:border-color={EGOGRAM_TEAL}></span>
      {/each}
    </div>
    <div class="flex items-center gap-1.5 px-2.5 py-1.5">
      <span class="size-4 rounded-full bg-foreground/15"></span>
      <span class="h-1.5 w-14 rounded-full bg-foreground/20"></span>
    </div>
    <span class="block aspect-square w-full shrink-0 bg-foreground/8"></span>
    <div class="flex flex-col gap-1.5 px-2.5 py-2">{@render line(0.5, "bg-foreground/20")}{@render line(0.8)}</div>
  </div>
{/snippet}

<div class="relative overflow-hidden" style:width={`${width}px`} style:height={`${height}px`}>
  <!-- The Focus timer both devices follow, which gives way to the daily limit once the day's Focus is done. -->
  <div class="absolute inset-x-0 top-0 flex items-center justify-center" style:height={`${HEADER_H}px`}>
    {#if limitShown < 1}
      <span
        class={cn(
          "absolute flex h-9 items-center gap-2 rounded-full border border-border bg-background px-3.5 text-[13px] font-medium tabular-nums",
          pulse > 0 && "bg-accent",
        )}
        style:opacity={1 - limitShown}
        style:transform={`scale(${1 + PULSE_GROWTH * Math.sin(Math.PI * pulse)})`}
      >
        {#if focusing}
          <PomodoroProgressRing
            active
            remainingSeconds={remaining}
            totalSeconds={DISTRACTIONS_FOCUS_SECONDS}
            size={18}
            trackClass="stroke-muted-foreground/35"
            progressClass="text-foreground/76 stroke-foreground/76"
          />
          {clock(remaining)}
        {:else}
          <span class="grid size-4.5 place-items-center rounded-full bg-primary text-primary-foreground">
            <Check size={11} strokeWidth={2.6} aria-hidden="true" />
          </span>
          {clock(0)}
        {/if}
      </span>
    {/if}
    {#if limitShown > 0}
      <div
        class="flex w-full max-w-80 flex-col gap-1.5 rounded-xl border border-border bg-background px-3 py-2"
        style:opacity={limitShown}
        style:transform={`translateY(${(1 - limitShown) * 6}px)`}
      >
        <div class="flex min-w-0 items-center gap-2">
          {@render brand("y", 14)}
          <span class="text-[12px] font-medium">{APP_NAMES.y}</span>
          <span class="ml-auto truncate text-[10.5px] text-muted-foreground">
            {t("settings.distractions.limits.usedOfLimit", usageLabel(usedMinutes), usageLabel(DISTRACTIONS_LIMIT_MINUTES), t("settings.distractions.limits.today"))}
          </span>
        </div>
        <div class="flex h-1.5 overflow-hidden rounded-full bg-muted">
          <span class="h-full" style:width={`${(usage.browser / DISTRACTIONS_LIMIT_MINUTES) * 100}%`} style:background-color={browserColor}></span>
          <span class="h-full" style:width={`${(usage.phone / DISTRACTIONS_LIMIT_MINUTES) * 100}%`} style:background-color={phoneColor}></span>
        </div>
        <div class="flex gap-1.5 text-[10px]">
          <span class="flex items-center gap-1 rounded-full border border-border px-1.5 py-px">
            <span class="size-1.5 rounded-full" style:background-color={browserColor}></span>
            <Globe size={10} strokeWidth={1.8} aria-hidden="true" />
            {DISTRACTIONS_LIMIT_HOST}
          </span>
          <span class="flex items-center gap-1 rounded-full border border-border px-1.5 py-px">
            <span class="size-1.5 rounded-full" style:background-color={phoneColor}></span>
            <Smartphone size={10} strokeWidth={1.8} aria-hidden="true" />
            {APP_NAMES.y}
          </span>
        </div>
      </div>
    {/if}
  </div>

  <div
    class="absolute left-0"
    style:top={`${HEADER_H + HEADER_GAP}px`}
    style:height={`${bodyH}px`}
    style:width={`${phoneX + phoneW}px`}
    style:transform={`translateX(${-cameraX}px)`}
  >
    <!-- The laptop's browser, with the Ganbaru extension beside its address bar. -->
    <div class="absolute top-0 left-0 rounded-3xl bg-foreground/6 p-1.5" style:width={`${browserW}px`} style:height={`${bodyH}px`}>
      <div class="relative flex h-full flex-col overflow-hidden rounded-2xl bg-background text-foreground">
        <div class="flex h-8 shrink-0 items-end gap-1 bg-foreground/5 px-1.5">
          {#if CONTROLS_LEADING}<span class="flex h-6 items-center">{@render controls()}</span>{/if}
          {@render tab(!newTabOpen, workTabOnSite ? (page === "limitBlock" ? "blocked" : "site") : "work", workTabOnSite ? (page === "limitBlock" ? PAGE_TEXT.tabTitle : DISTRACTIONS_LIMIT_HOST) : t("vaultSetup.distractions.workTab"), false)}
          {#if newTabOpen}
            {@render tab(true, page === "focusBlock" ? "blocked" : "work", page === "focusBlock" ? PAGE_TEXT.tabTitle : null, true)}
          {/if}
          <span class="relative mb-1 grid size-5 shrink-0 place-items-center rounded-md text-muted-foreground">
            <Plus size={12} strokeWidth={1.8} aria-hidden="true" />
            {@render touch(touches.newTab)}
          </span>
          <span class="flex-1"></span>
          {#if !CONTROLS_LEADING}<span class="flex h-6 items-center">{@render controls()}</span>{/if}
        </div>

        <div class="flex h-8 shrink-0 items-center gap-2 border-b border-border px-2.5 text-muted-foreground">
          <ArrowLeft size={12} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
          <ArrowRight size={12} strokeWidth={1.8} class="shrink-0 opacity-50" aria-hidden="true" />
          <RotateCw size={11} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
          <span class="relative flex h-5 min-w-0 flex-1 items-center gap-1.5 rounded-full bg-foreground/6 px-2.5 text-[10px] text-foreground">
            {#if address.kind === "placeholder"}
              <span class="h-1.5 w-2/5 rounded-full bg-foreground/15"></span>
            {:else if address.kind === "extension"}
              <span class="flex shrink-0 items-center gap-1 rounded-full bg-background px-1.5 py-px">
                <img src={appIconUrl} alt="" class="size-2.5 rounded-full" draggable="false" />
                {t("common.appName")}
              </span>
              <span class="h-1.5 w-1/4 rounded-full bg-foreground/12"></span>
            {:else}
              <span class="truncate">{address.text}</span>
              {#if address.editing && distractionsCaretOn(elapsed)}<span class="-ml-1.5 h-3 w-px bg-foreground"></span>{/if}
            {/if}
            <span class="absolute top-1/2 left-1/4">{@render touch(touches.address)}</span>
          </span>
          <img src={appIconUrl} alt="" class="size-3.5 shrink-0 rounded-full" draggable="false" />
        </div>

        <div class="relative min-h-0 flex-1 overflow-hidden">
          <div class="absolute inset-0" style:opacity={pageIn}>
            {#if page === "work"}
              <div class="flex flex-col gap-2 px-5 py-4">
                <span class="mb-1 h-2.5 w-2/5 rounded-full bg-foreground/25"></span>
                {#each DOC_WIDTHS as fraction, index (index)}
                  {#if fraction === 0}<span class="h-1.5"></span>{:else}{@render line(fraction)}{/if}
                {/each}
              </div>
            {:else if page === "feed"}
              {@render yFeed(distractionsFeedScroll(elapsed, "browser"), BROWSER_FEED_TRAVEL)}
            {:else if page === "focusBlock"}
              {@render blockPage("focus")}
            {:else if page === "limitBlock"}
              {@render blockPage("limit")}
            {/if}
          </div>
          {#if loading !== null}
            <span class="absolute top-0 left-0 h-0.5 bg-primary" style:width={`${loading * 100}%`}></span>
          {/if}
        </div>
      </div>
    </div>

    <!-- The phone, on its home screen with the parody apps among others. -->
    <div
      class="absolute rounded-[26px] bg-foreground/6 p-1.5"
      style:left={`${phoneX}px`}
      style:top={`${(bodyH - phoneH) / 2}px`}
      style:width={`${phoneW}px`}
      style:height={`${phoneH}px`}
    >
      <div class="relative h-full overflow-hidden rounded-[20px] bg-foreground/3 text-foreground">
        <div class="absolute inset-x-0 top-0 z-20 flex items-center gap-1 px-3 text-muted-foreground" style:height={`${STATUS_H}px`}>
          {#if focusing}
            <svg class="size-2.5" viewBox="0 0 24 24" aria-hidden="true">
              <path
                fill="currentColor"
                d="M12,2A10,10 0,1 0,22 12A10,10 0,0 0,12 2M12,5A7,7 0,1 1,5 12A7,7 0,0 1,12 5M11,7H13V12.4L16.4,14.4L15.4,16.1L11,13.5Z"
              />
            </svg>
          {/if}
          <span class="flex-1"></span>
          <span class="h-1.5 w-2.5 rounded-xs bg-current opacity-70"></span>
          <span class="h-1.5 w-3.5 rounded-xs bg-current opacity-70"></span>
        </div>

        {#each HOME_SLOTS as slot, index (index)}
          {@const center = slotCenter(index)}
          <span
            class="absolute flex flex-col items-center gap-1"
            style:left={`${center.x - cellW / 2}px`}
            style:top={`${center.y - iconSize / 2}px`}
            style:width={`${cellW}px`}
          >
            <span class="relative">
              {#if slot}
                {@render brand(slot, iconSize)}
                {#if slot === "egogram"}{@render touch(touches.egogram)}{/if}
                {#if slot === "y"}{@render touch(touches.y)}{/if}
              {:else}
                <span class="block rounded-[30%] bg-foreground/10" style:width={`${iconSize}px`} style:height={`${iconSize}px`}></span>
              {/if}
            </span>
            {#if slot}
              <span class="max-w-full truncate text-[8px] leading-none">{APP_NAMES[slot]}</span>
            {:else}
              <span class="h-1 w-5 rounded-full bg-foreground/12"></span>
            {/if}
          </span>
        {/each}

        <div class="absolute inset-x-0 flex justify-around px-2" style:bottom={`${18}px`}>
          {#each Array.from({ length: DOCK_SLOTS }, (_, index) => index) as index (index)}
            <span class="block rounded-[30%] bg-foreground/10" style:width={`${iconSize}px`} style:height={`${iconSize}px`}></span>
          {/each}
        </div>
        <span class="absolute bottom-1.5 left-1/2 h-1 w-10 -translate-x-1/2 rounded-full bg-foreground/30"></span>

        {#if phoneApp}
          <div
            class="absolute inset-0 z-10 overflow-hidden"
            style:transform-origin={`${openOrigin.x}px ${openOrigin.y}px`}
            style:transform={`scale(${0.12 + 0.88 * phoneApp.open})`}
            style:opacity={Math.min(1, phoneApp.open * 3)}
            style:border-radius={`${20 + (1 - phoneApp.open) * 40}px`}
          >
            {#if phoneApp.app === "egogram"}
              {@render egogramApp()}
            {:else}
              <div class="h-full bg-background" style:padding-top={`${STATUS_H}px`}>
                {@render yFeed(distractionsFeedScroll(elapsed, "phone"), PHONE_FEED_TRAVEL)}
              </div>
            {/if}
          </div>
        {/if}

        {#if notification}
          <div
            class="block-notice absolute inset-x-1.5 top-1.5 z-30 flex gap-2 rounded-2xl px-2.5 py-2"
            style:transform={`translateY(calc(${-(1 - notification.shown)} * (100% + 8px)))`}
          >
            <img src={appIconUrl} alt="" class="mt-px size-4 shrink-0 rounded-full" draggable="false" />
            <div class="min-w-0 flex-1 leading-tight">
              <p class="notice-muted truncate text-[8px]">{t("common.appName")}</p>
              <p class="truncate text-[10px] font-medium">{APP_NAMES[notification.app]}</p>
              <p class="notice-muted text-[9px]">
                {notification.reason === "focus" ? t("settings.distractions.mobile.blockedMessage") : t("settings.distractions.mobile.limitMessage")}
              </p>
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  /* Android's Material notification palette, in its light and dark variants, since the system paints it, not the app theme. */
  .block-notice {
    background: #f3f3f8;
    color: #1b1b1f;
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.2);
    font-family: "Google Sans Text", "Google Sans", Roboto, "Noto Sans", sans-serif;
  }

  .notice-muted {
    color: #45464f;
  }

  @media (prefers-color-scheme: dark) {
    .block-notice {
      background: #1f2024;
      color: #e3e2e6;
      box-shadow: 0 4px 12px rgb(0 0 0 / 0.45);
    }

    .notice-muted {
      color: #c5c6d0;
    }
  }
</style>
