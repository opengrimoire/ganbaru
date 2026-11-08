<script lang="ts">
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import { formatTimeRange, getEventColor, getPastEventColor } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import {
    BLOCKING_CURSOR,
    BLOCKING_DAY_VIEW,
    BLOCKING_EVENT,
    BLOCKING_RAIL,
    blockingBrowser,
    blockingClock,
    blockingEgogramTouch,
    blockingFocusing,
    blockingNotice,
    blockingPan,
    blockingPhase,
    blockingPhoneApp,
    blockingPulse,
    blockingRailRecorded,
  } from "$lib/vault/welcome-blocking-scene";
  import type { FilmSpan } from "$lib/vault/welcome-film";
  import WelcomeBlockerBrowser from "./WelcomeBlockerBrowser.svelte";
  import WelcomeBlockerPhone from "./WelcomeBlockerPhone.svelte";
  import WelcomeBlockerStage from "./WelcomeBlockerStage.svelte";

  /**
   * One frame of the welcome scene on blocking during Focus: a strip of the day with a Calendar event that runs Focus,
   * above a laptop's browser on Bluedit and a phone. While the current time crosses the event, the sites and the apps
   * the user chose are blocked however often they are tried; before and after it, they open as usual. Every style is a
   * function of `elapsed`.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const { t } = getLocalization();
  const preferences = getPreferences();
  const theme = getTheme();

  const HEADER_H = 64;
  /** The event's palette slot, the Calendar scene's Thesis color. */
  const EVENT_COLOR = 7;
  /**
   * How far the event grows at the peak of a pulse, and its height and inset at rest, in pixels. The pulse grows its
   * box through layout rather than a transform, since a webview under the scene's zoom can leave a shrinking
   * transform's old paint behind.
   */
  const PULSE_GROWTH = 0.05;
  /** The Focus rail above the event, as thick as the Calendar's and as far from the event, in pixels. */
  const RAIL_INSET = 4;
  const RAIL_THICKNESS = 6;
  const RAIL_GAP = 4;
  const EVENT_INSET = RAIL_INSET + RAIL_THICKNESS + RAIL_GAP;
  const EVENT_HEIGHT = 36;
  const VIEW_MINUTES = BLOCKING_DAY_VIEW.end - BLOCKING_DAY_VIEW.start;
  const EVENT_SPAN = at(BLOCKING_EVENT.end) - at(BLOCKING_EVENT.start);

  const clock = $derived(blockingClock(elapsed));
  const focusing = $derived(blockingFocusing(elapsed));
  const phase = $derived(blockingPhase(elapsed));
  const growth = $derived(PULSE_GROWTH * Math.sin(Math.PI * blockingPulse(elapsed)));
  const recorded = $derived(blockingRailRecorded(elapsed));
  const past = $derived(clock >= BLOCKING_EVENT.end);
  const colors = $derived(
    preferences.calendarDimPastEvents && past ? getPastEventColor(EVENT_COLOR, theme.current) : getEventColor(EVENT_COLOR, theme.current),
  );

  /** Where a minute of the day sits along the strip, in percent. */
  function at(minute: number): number {
    return ((minute - BLOCKING_DAY_VIEW.start) / VIEW_MINUTES) * 100;
  }

  /** A minute of the day as the Calendar's time labels take it. */
  function clockLabel(minuteOfDay: number): string {
    return `${String(Math.floor(minuteOfDay / 60)).padStart(2, "0")}:${String(Math.round(minuteOfDay % 60)).padStart(2, "0")}`;
  }

</script>

{#snippet railBand(band: FilmSpan, color: string)}
  <div
    class="absolute inset-y-0"
    style:left={`${((band.start - BLOCKING_RAIL.start) / (BLOCKING_RAIL.end - BLOCKING_RAIL.start)) * 100}%`}
    style:width={`${((band.end - band.start) / (BLOCKING_RAIL.end - BLOCKING_RAIL.start)) * 100}%`}
    style:background-color={color}
  ></div>
{/snippet}

<WelcomeBlockerStage {width} {height} headerHeight={HEADER_H} pan={blockingPan(elapsed)}>
  {#snippet header()}
    <!-- A strip of the day, with the event that runs Focus under its rail and the current time crossing both. -->
    <div class="relative h-14 w-full max-w-120 rounded-xl border border-border bg-background">
      <div
        class="absolute overflow-hidden"
        style:left={`${at(BLOCKING_RAIL.start)}%`}
        style:width={`${at(BLOCKING_RAIL.end) - at(BLOCKING_RAIL.start)}%`}
        style:top={`${RAIL_INSET}px`}
        style:height={`${RAIL_THICKNESS}px`}
        style:background-color="var(--cal-timeline-rail)"
        style:border-radius="3px"
      >
        {#each recorded as band (band.start)}
          {@render railBand(band, "var(--cal-timeline-focus)")}
        {/each}
        {#each BLOCKING_RAIL.breaks as band (band.start)}
          {@render railBand(band, "var(--cal-timeline-break)")}
        {/each}
      </div>
      <div
        class="absolute flex items-center gap-2 overflow-hidden rounded px-2 leading-tight"
        style:left={`${at(BLOCKING_EVENT.start) - (EVENT_SPAN * growth) / 2}%`}
        style:width={`${EVENT_SPAN * (1 + growth)}%`}
        style:top={`${EVENT_INSET - (EVENT_HEIGHT * growth) / 2}px`}
        style:height={`${EVENT_HEIGHT * (1 + growth)}px`}
        style:background-color={colors.bg}
        style:color={colors.text}
      >
        <div class="min-w-0 flex-1">
          <div class="truncate text-[11px] font-medium">{t("vaultSetup.film.writeChapter")}</div>
          <div class="truncate text-[10px]" style:color={`color-mix(in srgb, ${colors.text} 80%, ${colors.bg})`}>
            {formatTimeRange(clockLabel(BLOCKING_EVENT.start), clockLabel(BLOCKING_EVENT.end), preferences.calendarTimeFormat, "compact")}
          </div>
        </div>
        {#if phase}
          <span class="flex shrink-0">
            <PomodoroProgressRing
              active
              remainingSeconds={phase.remainingSeconds}
              totalSeconds={phase.totalSeconds}
              size={14}
              strokeWidth={2}
              trackClass="stroke-current/30"
              progressClass="stroke-current"
            />
          </span>
        {/if}
      </div>
      <div class="pointer-events-none absolute inset-y-0" style:left={`${at(clock)}%`}>
        <span class="absolute -top-1 -left-1 size-2 rounded-full" style:background-color="var(--cal-current-time)"></span>
        <span class="absolute inset-y-0 -left-px w-0.5" style:background-color="var(--cal-current-time)"></span>
      </div>
    </div>
  {/snippet}
  {#snippet browser(browserWidth)}
    <WelcomeBlockerBrowser frame={blockingBrowser(elapsed)} cursor={BLOCKING_CURSOR} {elapsed} width={browserWidth} />
  {/snippet}
  {#snippet phone(phoneWidth, phoneHeight)}
    <WelcomeBlockerPhone
      width={phoneWidth}
      height={phoneHeight}
      {focusing}
      app={blockingPhoneApp(elapsed)}
      notice={blockingNotice(elapsed)}
      touches={{ egogram: blockingEgogramTouch(elapsed) }}
    />
  {/snippet}
</WelcomeBlockerStage>
