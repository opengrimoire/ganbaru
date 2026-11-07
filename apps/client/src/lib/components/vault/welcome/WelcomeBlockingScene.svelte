<script lang="ts">
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import { formatTimeRange, getEventColor, getPastEventColor } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import {
    BLOCKING_DAY_VIEW,
    BLOCKING_EVENT,
    blockingBrowser,
    blockingClock,
    blockingEgogramTouch,
    blockingFocusing,
    blockingFocusRemaining,
    blockingNotice,
    blockingPan,
    blockingPhoneApp,
    blockingPulse,
  } from "$lib/vault/welcome-blocking-scene";
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
  const EVENT_INSET = 6;
  const EVENT_HEIGHT = 38;
  const VIEW_MINUTES = BLOCKING_DAY_VIEW.end - BLOCKING_DAY_VIEW.start;
  const EVENT_SECONDS = (BLOCKING_EVENT.end - BLOCKING_EVENT.start) * 60;
  const EVENT_SPAN = at(BLOCKING_EVENT.end) - at(BLOCKING_EVENT.start);

  const clock = $derived(blockingClock(elapsed));
  const focusing = $derived(blockingFocusing(elapsed));
  const remaining = $derived(blockingFocusRemaining(elapsed));
  const growth = $derived(PULSE_GROWTH * Math.sin(Math.PI * blockingPulse(elapsed)));
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

  /** A remaining time as the Focus timer shows it, in minutes and seconds. */
  function countdown(seconds: number): string {
    const whole = Math.ceil(seconds);
    return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
  }
</script>

<WelcomeBlockerStage {width} {height} headerHeight={HEADER_H} pan={blockingPan(elapsed)}>
  {#snippet header()}
    <!-- A strip of the day, with the event that runs Focus and the current time crossing it. -->
    <div class="relative h-13 w-full max-w-120 rounded-xl border border-border bg-background">
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
        {#if focusing}
          <span class="flex shrink-0 items-center gap-1 text-[10px] tabular-nums">
            <PomodoroProgressRing
              active
              remainingSeconds={remaining}
              totalSeconds={EVENT_SECONDS}
              size={14}
              strokeWidth={2}
              trackClass="stroke-current/30"
              progressClass="stroke-current"
            />
            {countdown(remaining)}
          </span>
        {/if}
      </div>
      <div class="pointer-events-none absolute inset-y-0" style:left={`${at(clock)}%`}>
        <span class="absolute -top-1 -left-1 size-2 rounded-full" style:background-color="var(--cal-current-time)"></span>
        <span class="absolute inset-y-0 -left-px w-0.5" style:background-color="var(--cal-current-time)"></span>
      </div>
    </div>
  {/snippet}
  {#snippet browser()}
    <WelcomeBlockerBrowser frame={blockingBrowser(elapsed)} />
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
