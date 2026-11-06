<script lang="ts" module>
  import { GUTTER_WIDTH_PER_TIMEZONE } from "$lib/calendar/timezone-utils";
  import { calendarZoomGridMinutesForPercent, calendarZoomHourHeightForPercent } from "$lib/calendar/zoom-levels";

  /**
   * The Calendar zoom the film is drawn at: one of the Calendar's own levels, close enough for a five minute break to
   * read on the Focus rail.
   */
  const FILM_ZOOM_PERCENT = 150;

  /** Canvas geometry in logical pixels, matching the Calendar's own measures at the film's zoom. */
  const CANVAS_W = 360;
  const TOP_BAR_H = 36;
  const HEADER_H = 40;
  const HOUR_PX = calendarZoomHourHeightForPercent(FILM_ZOOM_PERCENT);
  /** How the drawn event snaps while it is dragged out, in minutes, as the Calendar snaps at the film's zoom. */
  const CREATE_SNAP_MINUTES = calendarZoomGridMinutesForPercent(FILM_ZOOM_PERCENT);
  /** Where the visible part of the day starts, in minutes since midnight, as if the grid were scrolled to the morning. */
  const VIEW_START = 8 * 60 + 40;
  /** How much of the day shows: the morning's sessions at least, and on to the early afternoon in a tall room. */
  const MIN_VIEW_MINUTES = 200;
  const MAX_VIEW_MINUTES = 320;
  const GUTTER_W = GUTTER_WIDTH_PER_TIMEZONE;
  const DAYS_W = CANVAS_W - GUTTER_W;

  /** The bezel around the canvas, matching the drawn phones of the Android access guides (`p-1.5`). */
  const BEZEL = 6;

  /** The canvas plus its bezel at its smallest and largest, for the player to fit. */
  export const FILM_SCENE_WIDTH = CANVAS_W + 2 * BEZEL;
  export const FILM_SCENE_MIN_HEIGHT = TOP_BAR_H + HEADER_H + (MIN_VIEW_MINUTES / 60) * HOUR_PX + 2 * BEZEL;
  export const FILM_SCENE_MAX_HEIGHT = TOP_BAR_H + HEADER_H + (MAX_VIEW_MINUTES / 60) * HOUR_PX + 2 * BEZEL;
</script>

<script lang="ts">
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import type { EventColor } from "$lib/calendar/types";
  import type { PomodoroPresetKey } from "$lib/pomodoro/rhythm";
  import {
    dayNameFormatForWidth,
    formatDayName,
    formatTimeRange,
    getEventColor,
    getHourInTimezone,
    getLocalTimezone,
    getPastEventColor,
    getTimezoneInfo,
  } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import {
    formatBlockedScreenDateTime,
    formatBlockedScreenDuration,
    formatBreakEndEarlyShortcut,
    formatBreakExtensionHint,
    formatBreakExtensionShortcut,
    pomodoroBlockedScreenCopy,
    pomodoroBlockedScreenPalette,
    type PomodoroBlockedScreenPalette,
  } from "$lib/pomodoro/blocked-screen";
  import {
    DEFAULT_FOCUS_BREAK_END_ESC_PRESSES,
    DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT,
  } from "$lib/stores/preference-options";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { isThemeCalendarDark } from "$lib/themes";
  import { cn } from "$lib/utils";
  import {
    FILM_BEATS,
    FILM_BREAKS,
    FILM_PULL_BACK_MS,
    FILM_SESSION_MINUTES,
    easeInOutCubic,
    filmClockMinute,
    filmDayRails,
    filmFocusSpans,
    filmSessionMinute,
    filmSessionMoment,
    filmSpan,
    type FilmRail,
    type FilmSpan,
  } from "$lib/vault/welcome-film";

  /**
   * One frame of the welcome film, drawn with the Calendar's and Focus's own surfaces: the week grid with its Focus
   * rails and current time, the top bar's Focus timer, and the break and finished screens. Every style is a function
   * of `elapsed`, with no CSS transitions, so text is never handed to a compositing layer mid-animation and keeps its
   * rendering from frame to frame.
   */
  let { elapsed, height }: { elapsed: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const theme = getTheme();
  const preferences = getPreferences();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  /** The desktop title bar's icon size. */
  const TITLE_ICON_SIZE = 14;

  /** The Calendar's Focus rail, and the event area it keeps beside it on every event. */
  const RAIL_LEFT = 2;
  const RAIL_W = 6;
  const EVENT_LEFT = RAIL_W + 4;
  /** Minimum height for an event to show its time under its title. */
  const EVENT_TIME_MIN_PX = 32;
  const PRESS_FADE_MS = 140;
  const RAIL_REVEAL_MS = 450;
  const SCREEN_FADE_MS = 260;
  /** The week's recorded focus fills in one day after another once the view has pulled back. */
  const WEEK_FILL_DELAY_MS = 350;
  const WEEK_FILL_MS = 800;
  const WEEK_FILL_STAGGER_MS = 180;
  /** The Calendar's pulse on a break that is running. */
  const BREAK_PULSE_MS = 1500;

  /** The session starts at nine. */
  const SESSION_START = 9 * 60;
  const SESSION_END = SESSION_START + FILM_SESSION_MINUTES;
  /** The session's planned breaks, in minutes of the day. */
  const TODAY_BREAKS = inDay(FILM_BREAKS);

  /** Palette slots, so the drawing follows the active theme's event colors. The session keeps the default color. */
  const BLUE = 18;
  const PURPLE = 22;
  const ORANGE = 7;
  const CYAN = 16;

  type FilmTitle = "writeChapter" | "reviewNotes" | "lunch" | "draftOutline" | "readSources";
  interface FilmEvent {
    start: number;
    end: number;
    color: EventColor | undefined;
    title: FilmTitle;
    /** The Focus preset of a Focus event. */
    preset?: PomodoroPresetKey;
  }

  /**
   * The days shown once the view pulls back, as offsets from today, and what else each day holds. Focus events run
   * back to back with different presets, so their rails show the rhythm carrying from one event into the next, with
   * its long break after the fourth focus.
   */
  const WEEK: readonly { offset: number; events: readonly FilmEvent[] }[] = [
    { offset: -3, events: [
      { start: 540, end: 610, color: BLUE, title: "readSources", preset: "balanced" },
      { start: 610, end: 705, color: CYAN, title: "draftOutline", preset: "creative" },
      { start: 720, end: 780, color: ORANGE, title: "lunch" },
    ] },
    { offset: -2, events: [
      { start: 530, end: 620, color: CYAN, title: "draftOutline", preset: "deep" },
      { start: 620, end: 690, color: PURPLE, title: "reviewNotes", preset: "creative" },
      { start: 730, end: 790, color: ORANGE, title: "lunch" },
      { start: 795, end: 840, color: BLUE, title: "readSources", preset: "deep" },
    ] },
    { offset: -1, events: [
      { start: 540, end: 600, color: CYAN, title: "draftOutline", preset: "creative" },
      { start: 600, end: 715, color: BLUE, title: "readSources", preset: "deep" },
      { start: 730, end: 790, color: ORANGE, title: "lunch" },
    ] },
    { offset: 0, events: [
      { start: 630, end: 670, color: PURPLE, title: "reviewNotes" },
      { start: 700, end: 760, color: ORANGE, title: "lunch" },
      { start: 790, end: 830, color: BLUE, title: "readSources" },
    ] },
    { offset: 1, events: [
      { start: SESSION_START, end: 600, color: undefined, title: "writeChapter", preset: "creative" },
      { start: 600, end: 690, color: PURPLE, title: "reviewNotes", preset: "balanced" },
      { start: 720, end: 780, color: ORANGE, title: "lunch" },
    ] },
  ];
  const PAST_DAYS = WEEK.filter((day) => day.offset < 0).length;

  const now = new Date();
  const days = WEEK.map((day) => ({
    ...day,
    date: new Date(now.getFullYear(), now.getMonth(), now.getDate() + day.offset),
    rails: filmDayRails(day.events.flatMap((event) => (event.preset ? [{ start: event.start, end: event.end, preset: event.preset }] : []))),
  }));
  const timezone = getLocalTimezone();
  const timezoneLabel = getTimezoneInfo(timezone).acronym;

  /** The canvas inside its bezel, and how much of the day it shows. */
  const canvasH = $derived(height - 2 * BEZEL);
  const viewMinutes = $derived(((canvasH - TOP_BAR_H - HEADER_H) / HOUR_PX) * 60);
  const hours = $derived(
    Array.from({ length: Math.ceil(viewMinutes / 60) + 1 }, (_, index) => Math.ceil(VIEW_START / 60) + index)
      .filter((hour) => hour * 60 > VIEW_START && hour * 60 < VIEW_START + viewMinutes),
  );

  const breakCopy = $derived(pomodoroBlockedScreenCopy("break_countdown", t));
  const finishedCopy = $derived(pomodoroBlockedScreenCopy("event_finished", t));
  const breakPalette = pomodoroBlockedScreenPalette("break_countdown");
  const finishedPalette = pomodoroBlockedScreenPalette("event_finished");
  const extensionHint = formatBreakExtensionHint(0, DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT, undefined, t) !== null
    ? formatBreakExtensionShortcut()
    : null;
  const endBreakHint = formatBreakEndEarlyShortcut(0, DEFAULT_FOCUS_BREAK_END_ESC_PRESSES);

  const calendarDark = $derived(isThemeCalendarDark(theme.current));
  const clock = $derived(SESSION_START + filmClockMinute(elapsed));
  const sessionMinute = $derived(filmSessionMinute(elapsed));
  const moment = $derived(filmSessionMoment(sessionMinute));
  const focusing = $derived(elapsed >= FILM_BEATS.focusStart && moment.kind !== "done");

  /** The session while it is dragged out, then saved. */
  const drawn = $derived(easeInOutCubic(filmSpan(elapsed, FILM_BEATS.createStart, FILM_BEATS.createEnd)));
  const drawnMinutes = $derived(
    Math.max(CREATE_SNAP_MINUTES, Math.round((drawn * FILM_SESSION_MINUTES) / CREATE_SNAP_MINUTES) * CREATE_SNAP_MINUTES),
  );
  const creating = $derived(elapsed >= FILM_BEATS.createStart && elapsed < FILM_BEATS.createSaved);
  const saved = $derived(elapsed >= FILM_BEATS.createSaved);
  const pressOpacity = $derived(
    filmSpan(elapsed, FILM_BEATS.createStart - PRESS_FADE_MS, FILM_BEATS.createStart)
      * (1 - filmSpan(elapsed, FILM_BEATS.createEnd, FILM_BEATS.createEnd + PRESS_FADE_MS)),
  );
  const railReveal = $derived(easeInOutCubic(filmSpan(elapsed, FILM_BEATS.railIn, FILM_BEATS.railIn + RAIL_REVEAL_MS)));

  /** The pull back: other days open beside today, which narrows to one column of the week. */
  const pullBack = $derived(easeInOutCubic(filmSpan(elapsed, FILM_BEATS.pullBack, FILM_BEATS.pullBack + FILM_PULL_BACK_MS)));
  const otherGrow = $derived(pullBack);
  const dayFormat = $derived(dayNameFormatForWidth(DAYS_W / (1 + (days.length - 1) * otherGrow)));

  const breakScreen = $derived(screenAlpha(FILM_BEATS.breakStart, FILM_BEATS.breakEnd + SCREEN_FADE_MS));
  const finishedScreen = $derived(screenAlpha(FILM_BEATS.sessionEnd, FILM_BEATS.finishedOut));
  /** The break's countdown, held at zero while its screen lifts. */
  const breakSeconds = $derived(moment.kind === "break" ? moment.remainingSeconds : 0);
  const breakPulse = $derived(0.7 - 0.2 * Math.cos((2 * Math.PI * elapsed) / BREAK_PULSE_MS));
  const settled = $derived(elapsed >= FILM_BEATS.end);

  /** How visible a full screen is: it fades in from `start` and out until `end`. */
  function screenAlpha(start: number, end: number): number {
    return filmSpan(elapsed, start, start + SCREEN_FADE_MS) * (1 - filmSpan(elapsed, end - SCREEN_FADE_MS, end));
  }

  /** A color at `alpha`, so a screen fades by its colors rather than by opacity. */
  function fade(color: string, alpha: number): string {
    return alpha >= 1 ? color : `color-mix(in srgb, ${color} ${alpha * 100}%, transparent)`;
  }

  function screenStyle(palette: PomodoroBlockedScreenPalette, alpha: number): string {
    return `--film-screen-bg: ${fade(palette.background, alpha)}; --film-screen-main: ${fade(palette.mainText, alpha)}; --film-screen-muted: ${fade(palette.mutedText, alpha)}; --film-screen-subtle: ${fade(palette.subtleText, alpha)};`;
  }

  function y(minuteOfDay: number): number {
    return ((minuteOfDay - VIEW_START) / 60) * HOUR_PX;
  }

  function px(minutes: number): number {
    return (minutes / 60) * HOUR_PX;
  }

  function clockLabel(minuteOfDay: number): string {
    const hour = Math.floor(minuteOfDay / 60);
    const minute = Math.round(minuteOfDay % 60);
    return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
  }

  function timeRange(start: number, end: number): string {
    return formatTimeRange(clockLabel(start), clockLabel(end), preferences.calendarTimeFormat, "compact");
  }

  function hourLabel(hour: number): string {
    const label = getHourInTimezone(now, hour, timezone, preferences.calendarTimeFormat, "short");
    if (preferences.calendarTimeFormat !== "12h") return label;
    return label.replace(/\b(am|pm)\b$/, (period) => period.toUpperCase());
  }

  function title(key: FilmTitle): string {
    switch (key) {
      case "writeChapter": return t("vaultSetup.film.writeChapter");
      case "reviewNotes": return t("vaultSetup.film.reviewNotes");
      case "lunch": return t("vaultSetup.film.lunch");
      case "draftOutline": return t("vaultSetup.film.draftOutline");
      case "readSources": return t("vaultSetup.film.readSources");
    }
  }

  function eventColors(color: EventColor | undefined, past: boolean) {
    return preferences.calendarDimPastEvents && past ? getPastEventColor(color, theme.current) : getEventColor(color, theme.current);
  }

  function isPast(offset: number, end: number): boolean {
    return offset < 0 || (offset === 0 && end <= clock);
  }

  /** How much of a past day's rails is filled with recorded focus, from 0 to 1, as the week fills in. */
  function weekFill(offset: number): number {
    if (offset >= 0) return 1;
    const start = FILM_BEATS.pullBack + WEEK_FILL_DELAY_MS + (offset + PAST_DAYS) * WEEK_FILL_STAGGER_MS;
    return easeInOutCubic(filmSpan(elapsed, start, start + WEEK_FILL_MS));
  }

  /** The focus a rail of the week shows: recorded on past days as the week fills in, none yet on days to come. */
  function recordedFocus(rail: FilmRail, offset: number): FilmSpan[] {
    if (offset >= 0) return [];
    return clip(rail.focus, rail.start + weekFill(offset) * (rail.end - rail.start));
  }

  /** Session minutes as minutes of the day. */
  function inDay(spans: readonly FilmSpan[]): FilmSpan[] {
    return spans.map((span) => ({ start: SESSION_START + span.start, end: SESSION_START + span.end }));
  }

  function clip(spans: readonly FilmSpan[], minute: number): FilmSpan[] {
    return spans.filter((span) => minute > span.start).map((span) => ({ start: span.start, end: Math.min(span.end, minute) }));
  }
</script>

{#snippet rail(
  start: number,
  end: number,
  recorded: readonly FilmSpan[],
  breaks: readonly FilmSpan[],
  activeBreak: boolean,
  reveal: number,
)}
  {@const length = end - start}
  <div
    class="pointer-events-none absolute z-3 overflow-hidden"
    style:left={`${RAIL_LEFT}px`}
    style:width={`${RAIL_W}px`}
    style:top={`${y(start)}px`}
    style:height={`${px(length)}px`}
    style:background-color="var(--cal-timeline-rail)"
    style:border-radius="3px"
    style:clip-path={reveal < 1 ? `inset(0 0 ${(1 - reveal) * 100}% 0)` : undefined}
  >
    {#each recorded as band (band.start)}
      <div
        class="absolute inset-x-0"
        style:top={`${((band.start - start) / length) * 100}%`}
        style:height={`${((band.end - band.start) / length) * 100}%`}
        style:background-color="var(--cal-timeline-focus)"
      ></div>
    {/each}
    {#each breaks as band (band.start)}
      <div
        class="absolute inset-x-0"
        style:top={`${((band.start - start) / length) * 100}%`}
        style:height={`${((band.end - band.start) / length) * 100}%`}
        style:background-color="var(--cal-timeline-break)"
        style:opacity={activeBreak ? breakPulse : undefined}
      ></div>
    {/each}
  </div>
{/snippet}

{#snippet eventBlock(start: number, end: number, color: EventColor | undefined, label: string, past: boolean, outline: boolean)}
  {@const colors = outline ? getEventColor(color, theme.current) : eventColors(color, past)}
  <div
    class="absolute inset-x-0 flex overflow-hidden rounded text-[0.8rem] leading-tight"
    style:top={`${y(start)}px`}
    style:height={`${px(end - start)}px`}
    style:background-color={colors.bg}
    style:color={colors.text}
    style:outline={outline ? `2px solid color-mix(in oklab, ${colors.bg} 65%, ${calendarDark ? "white" : "black"})` : undefined}
    style:z-index={outline ? 10 : 3}
  >
    <div class="relative min-w-0 flex-1 overflow-hidden px-1 py-0.5">
      <div class="truncate font-medium">{label}</div>
      {#if px(end - start) >= EVENT_TIME_MIN_PX}
        <div class="truncate" style:color={`color-mix(in srgb, ${colors.text} 80%, ${colors.bg})`}>{timeRange(start, end)}</div>
      {/if}
    </div>
  </div>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5">
  <div
    class="relative flex flex-col overflow-hidden rounded-2xl bg-(--cal-bg) text-foreground"
    style:width={`${CANVAS_W}px`}
    style:height={`${canvasH}px`}
  >
    {#if mobileShell}
      <!-- The top bar's Focus timer: its ring, and the time left once a session runs. -->
      <div class="flex shrink-0 items-center justify-end bg-background px-1.5" style:height={`${TOP_BAR_H}px`}>
        {#if focusing && moment.kind !== "done"}
          <span class="grid h-8 grid-flow-col place-items-center gap-1.5 rounded-full bg-accent/70 pr-2.5 pl-1.5 text-sm font-medium text-foreground tabular-nums">
            <PomodoroProgressRing
              active
              remainingSeconds={moment.remainingSeconds}
              totalSeconds={moment.totalSeconds}
              size={20}
              trackClass="stroke-muted-foreground/35"
              progressClass="text-foreground/76 stroke-foreground/76"
            />
            {formatBlockedScreenDuration(moment.remainingSeconds)}
          </span>
        {:else}
          <span class="grid size-8 place-items-center rounded-full text-muted-foreground">
            <PomodoroProgressRing
              active={false}
              remainingSeconds={0}
              totalSeconds={0}
              size={20}
              trackClass="stroke-muted-foreground/35"
            />
          </span>
        {/if}
      </div>
    {:else}
      <!-- The title bar: the Focus ring alone, then the window controls. -->
      <div class="flex shrink-0 items-center justify-end gap-0.5 bg-sidebar pr-1.5 text-foreground/68 dark:text-white/76" style:height={`${TOP_BAR_H}px`}>
        <span class="grid size-7 place-items-center">
          <PomodoroProgressRing
            active={focusing && moment.kind !== "done"}
            remainingSeconds={moment.kind === "done" ? 0 : moment.remainingSeconds}
            totalSeconds={moment.kind === "done" ? 0 : moment.totalSeconds}
            size={TITLE_ICON_SIZE + 0.5}
            trackClass="stroke-foreground/20 dark:stroke-white/20"
            progressClass="text-foreground/68 dark:text-white/76 stroke-foreground/68 dark:stroke-white/76"
          />
        </span>
        <span class="ml-1.5 grid size-7 place-items-center"><Minus size={TITLE_ICON_SIZE} strokeWidth={1.5} aria-hidden="true" /></span>
        <span class="grid size-7 place-items-center"><Square size={TITLE_ICON_SIZE} strokeWidth={1.5} aria-hidden="true" /></span>
        <span class="grid size-7 place-items-center"><X size={TITLE_ICON_SIZE} strokeWidth={1.5} aria-hidden="true" /></span>
      </div>
    {/if}

    <!-- The week view's header row: the time zone over the gutter, then each day's name and number. -->
    <div class="flex shrink-0 border-b border-sidebar" style:height={`${HEADER_H}px`}>
      <span class="flex shrink-0 items-center justify-center text-[0.866667rem]" style:width={`${GUTTER_W}px`}>{timezoneLabel}</span>
      {#each days as day (day.offset)}
        <div
          class="flex min-w-0 basis-0 items-center justify-center overflow-hidden whitespace-nowrap"
          style:flex-grow={day.offset === 0 ? 1 : otherGrow}
        >
          <span class="text-[0.866667rem]" style:color={day.offset < 0 ? "var(--muted-foreground)" : "var(--foreground)"}>
            {#if dayFormat !== "none"}{formatDayName(day.date, dayFormat, localization.locale)}&nbsp;{/if}{day.date.getDate()}
          </span>
        </div>
      {/each}
    </div>

    <div class="relative flex min-h-0 flex-1">
      <!-- The time gutter. -->
      <div class="relative shrink-0" style:width={`${GUTTER_W}px`}>
        {#each hours as hour (hour)}
          <span
            class="absolute -translate-x-1/2 whitespace-nowrap text-[0.733333rem] leading-none antialiased"
            style:top={`calc(${y(hour * 60)}px - 0.45em)`}
            style:left={preferences.calendarTimeFormat === "12h" ? "calc(50% - 0.2px)" : "50%"}
            style:color="var(--cal-time-label)"
          >{hourLabel(hour)}</span>
        {/each}
      </div>

      <div class="relative flex min-w-0 flex-1">
        <div class="pointer-events-none absolute inset-0 z-1">
          {#each hours as hour (hour)}
            <div class="absolute inset-x-0 h-0 border-b border-(--cal-gridline)" style:top={`${y(hour * 60)}px`}></div>
          {/each}
        </div>

        {#each days as day (day.offset)}
          {@const isToday = day.offset === 0}
          <div
            class={cn("relative min-w-0 basis-0", !isToday && "overflow-hidden")}
            style:flex-grow={isToday ? 1 : otherGrow}
            style:border-left={isToday || otherGrow > 0 ? "1px solid var(--cal-gridline)" : undefined}
          >
            {#each day.rails as dayRail (dayRail.start)}
              {@render rail(dayRail.start, dayRail.end, recordedFocus(dayRail, day.offset), dayRail.breaks, false, 1)}
            {/each}

            {#if isToday}
              {@render rail(
                SESSION_START,
                SESSION_END,
                inDay(filmFocusSpans(sessionMinute)),
                TODAY_BREAKS,
                focusing && moment.kind === "break",
                saved ? railReveal : 0,
              )}
            {/if}

            <div class="absolute inset-y-0 right-0" style:left={`${EVENT_LEFT}px`}>
              {#each day.events as event (event.start)}
                {@render eventBlock(event.start, event.end, event.color, title(event.title), isPast(day.offset, event.end), false)}
              {/each}

              {#if isToday}
                {#if creating}
                  {@render eventBlock(SESSION_START, SESSION_START + drawnMinutes, undefined, t("calendar.event.noTitle"), false, true)}
                {:else if saved}
                  {@render eventBlock(SESSION_START, SESSION_END, undefined, title("writeChapter"), isPast(0, SESSION_END), false)}
                {/if}

                <!-- The press that draws the session, following its growing edge. -->
                {#if pressOpacity > 0}
                  <span
                    class="pointer-events-none absolute left-3/5 z-20 size-6 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35"
                    style:top={`${y(SESSION_START + drawnMinutes)}px`}
                    style:opacity={pressOpacity}
                  ></span>
                {/if}
              {/if}
            </div>

            {#if isToday}
              <!-- The current time, walking up to the session and through it. -->
              <div class="pointer-events-none absolute inset-x-0" style:top={`${y(clock)}px`} style:z-index="46">
                <div
                  class={cn("absolute size-2.5 shrink-0 rounded-full", settled && "film-breathe")}
                  style:background-color="var(--cal-current-time)"
                  style:left="-5px"
                  style:top="-5px"
                ></div>
                <div class="absolute inset-x-0 h-[2.3px]" style:background-color="var(--cal-current-time)" style:top="-1.15px"></div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    </div>

    <!-- The break screen, with its date and time, countdown, and keys. -->
    {#if breakScreen > 0}
      <div class="film-screen absolute inset-0 z-50 flex flex-col items-center justify-center" style={screenStyle(breakPalette, breakScreen)}>
        <p class="film-screen-small film-screen-main absolute inset-x-0 top-4 px-4 text-center">
          {formatBlockedScreenDateTime(new Date(now.getFullYear(), now.getMonth(), now.getDate(), 0, clock), localization.locale)}
        </p>
        <p class="film-screen-timer film-screen-main tabular-nums">{formatBlockedScreenDuration(breakSeconds)}</p>
        {#if !mobileShell}
          <div class="film-screen-hints film-screen-subtle absolute inset-x-0 bottom-5 flex flex-col items-center gap-1.5 px-4 text-center">
            {#if extensionHint && breakCopy.primaryHint}
              <p>{t("pomodoroOverlay.press")} <span class="film-screen-main">{extensionHint}</span> {t("pomodoroOverlay.to")} {breakCopy.primaryHint.label}</p>
            {/if}
            {#if endBreakHint && breakCopy.secondaryHint}
              <p>{t("pomodoroOverlay.press")} <span class="film-screen-main">{endBreakHint}</span> {t("pomodoroOverlay.to")} {breakCopy.secondaryHint.label}</p>
            {/if}
          </div>
        {/if}
      </div>
    {/if}

    <!-- The finished screen once the session ends. -->
    {#if finishedScreen > 0}
      <div class="film-screen absolute inset-0 z-50 flex flex-col items-center justify-center gap-8 px-4 text-center" style={screenStyle(finishedPalette, finishedScreen)}>
        <p class="film-screen-title film-screen-main">{finishedCopy.title}</p>
        <p class="film-screen-small film-screen-muted">{finishedCopy.subtitle}</p>
      </div>
    {/if}
  </div>
</div>

<style>
  /* The Focus screens at the size they take in a window as small as the canvas. */
  .film-screen {
    background: var(--film-screen-bg);
  }

  .film-screen-main {
    color: var(--film-screen-main);
  }

  .film-screen-muted {
    color: var(--film-screen-muted);
  }

  .film-screen-subtle {
    color: var(--film-screen-subtle);
  }

  .film-screen-small {
    font-size: calc(0.85rem * var(--type-scale));
    line-height: 1.25;
  }

  .film-screen-hints {
    font-size: calc(0.6375rem * var(--type-scale));
    line-height: 1.25;
  }

  .film-screen-timer {
    font-size: calc(4.75rem * var(--type-scale));
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0;
    line-height: 0.85;
  }

  .film-screen-title {
    font-size: calc(2rem * var(--type-scale));
    font-weight: 700;
    letter-spacing: 0;
    line-height: 0.95;
  }

  /* The current time keeps breathing on the finished frame; a shadow pulse repaints in place without a layer. */
  .film-breathe {
    animation: film-breathe 2400ms ease-in-out infinite;
  }

  @keyframes film-breathe {
    0%, 100% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--cal-current-time) 45%, transparent); }
    60% { box-shadow: 0 0 0 5px transparent; }
  }

  @media (prefers-reduced-motion: reduce) {
    .film-breathe { animation: none; }
  }
</style>
