<script lang="ts" module>
  import { GUTTER_WIDTH_PER_TIMEZONE } from "$lib/calendar/timezone-utils";
  import { calendarZoomGridMinutesForPercent, calendarZoomHourHeightForPercent } from "$lib/calendar/zoom-levels";

  /**
   * The Calendar zoom the film is drawn at: the Calendar's default level, which fits the whole session in view while a
   * five minute break still reads on the Focus rail.
   */
  const FILM_ZOOM_PERCENT = 100;

  /**
   * Canvas geometry in logical pixels, matching the Calendar's own measures at the film's zoom. The width is chosen by
   * the room between these bounds, so a wide window gets wider day columns.
   */
  const MIN_CANVAS_W = 360;
  const MAX_CANVAS_W = 628;
  const TOP_BAR_H = 36;
  const HEADER_H = 40;
  const HOUR_PX = calendarZoomHourHeightForPercent(FILM_ZOOM_PERCENT);
  /** How the drawn event snaps while it is dragged out, in minutes, as the Calendar snaps at the film's zoom. */
  const CREATE_SNAP_MINUTES = calendarZoomGridMinutesForPercent(FILM_ZOOM_PERCENT);
  /** Where the visible part of the day starts, in minutes since midnight, as if the grid were scrolled to the morning. */
  const VIEW_START = 8 * 60 + 40;
  /** How much of the day shows: the whole session at least, and on into the afternoon in a tall room. */
  const MIN_VIEW_MINUTES = 315;
  const MAX_VIEW_MINUTES = 420;
  const GUTTER_W = GUTTER_WIDTH_PER_TIMEZONE;

  /** The bezel around the canvas, matching the drawn phones of the Android access guides (`p-1.5`). */
  const BEZEL = 6;

  /** The canvas plus its bezel at its smallest and largest, for the player to fit. */
  export const FILM_SCENE_MIN_WIDTH = MIN_CANVAS_W + 2 * BEZEL;
  export const FILM_SCENE_MAX_WIDTH = MAX_CANVAS_W + 2 * BEZEL;
  export const FILM_SCENE_MIN_HEIGHT = TOP_BAR_H + HEADER_H + (MIN_VIEW_MINUTES / 60) * HOUR_PX + 2 * BEZEL;
  export const FILM_SCENE_MAX_HEIGHT = TOP_BAR_H + HEADER_H + (MAX_VIEW_MINUTES / 60) * HOUR_PX + 2 * BEZEL;
</script>

<script lang="ts">
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import type { EventColor } from "$lib/calendar/types";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Users from "@lucide/svelte/icons/users";
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
    type PomodoroCompletionScreenState,
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
    FILM_DRAWN_MINUTES,
    FILM_PLANNED_RAIL,
    FILM_PULL_BACK_MS,
    FILM_SESSION_MINUTES,
    easeInOutCubic,
    filmClockMinute,
    filmDayRails,
    filmFocusSpans,
    filmRailBreaks,
    filmSessionMinute,
    filmSessionMoment,
    filmSpan,
    filmSpansWithout,
    filmWorkday,
    type FilmRail,
    type FilmSpan,
  } from "$lib/vault/welcome-film";

  /**
   * One frame of the welcome film, drawn with the Calendar's and Focus's own surfaces: the week grid with its Focus
   * rails and current time, the top bar's Focus timer, and the break and finished screens. Every style is a function
   * of `elapsed`, with no CSS transitions, so text is never handed to a compositing layer mid-animation and keeps its
   * rendering from frame to frame.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const theme = getTheme();
  const preferences = getPreferences();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  /** `Date.getDay()` for Friday, the day that completes the workweek. */
  const FRIDAY = 5;
  /** The desktop title bar's icon size, which its Focus ring follows. */
  const TITLE_ICON_SIZE = 14;

  /** The Calendar's Focus rail, and the event area it keeps beside it on every event. */
  const RAIL_LEFT = 2;
  const RAIL_W = 6;
  const EVENT_LEFT = RAIL_W + 4;
  /** The Calendar's slight separation above an event that starts right as another ends. */
  const EVENT_GAP_PX = 2;
  /** Minimum height for an event to show its time under its title. */
  const EVENT_TIME_MIN_PX = 32;
  const PRESS_FADE_MS = 140;
  const SCREEN_FADE_MS = 260;
  /** The Calendar's pulse on a break that is running. */
  const BREAK_PULSE_MS = 1500;

  /** The session starts at nine: the drawn event, then the planned one right after it. */
  const SESSION_START = 9 * 60;
  const DRAWN_END = SESSION_START + FILM_DRAWN_MINUTES;
  const SESSION_END = SESSION_START + FILM_SESSION_MINUTES;
  /** The planned event's breaks while it has its rail to itself. */
  const PLANNED_BREAKS = inDay(FILM_PLANNED_RAIL.breaks);

  /** Palette slots, so the drawing follows the active theme's event colors. Meals take the Eating routine's color. */
  const BLUE = 18;
  const PURPLE = 22;
  const CYAN = 16;
  const LAVENDER = 20;
  const TAUPE = 28;
  const EATING = 13;

  type FilmTitle = "writeChapter" | "reviewNotes" | "lunch" | "draftOutline" | "readSources" | "teamMeeting";
  /** The Calendar's event icons: a repeating event, one with music, and a meeting. */
  type FilmIcon = "music" | "repeat" | "meeting";
  interface FilmEvent {
    start: number;
    end: number;
    color: EventColor | undefined;
    title: FilmTitle;
    /** The Focus preset of a Focus event. */
    preset?: PomodoroPresetKey;
    icons?: readonly FilmIcon[];
  }

  /** A weekday of the film's week: its events, and the moments its Focus was paused or the user was away. */
  interface FilmWorkday {
    events: readonly FilmEvent[];
    idle?: readonly FilmSpan[];
  }

  /** The Calendar's icon order on an event. */
  const ICON_ORDER: readonly FilmIcon[] = ["music", "repeat", "meeting"];
  /** Minimum height for an event to show its icons. */
  const EVENT_ICONS_MIN_PX = 17;
  const LUNCH_ICONS: readonly FilmIcon[] = ["repeat"];
  const MUSIC_ICONS: readonly FilmIcon[] = ["music"];
  const MEETING_ICONS: readonly FilmIcon[] = ["repeat", "meeting"];

  /** Lunch, at the same hour on every other day of the week. */
  const LUNCH: FilmEvent = { start: 12 * 60, end: 13 * 60, color: EATING, title: "lunch", icons: LUNCH_ICONS };

  /**
   * What the workweek holds from Monday to Friday. Events start and end on the hour or the half hour, as people plan
   * them, except Tuesday's first event, which starts a little late. Focus events run back to back with different
   * presets, so their rails show the rhythm carrying from one event into the next, and most days have a pause or a
   * moment away that their recorded rail leaves empty.
   */
  const WORKWEEK: readonly FilmWorkday[] = [
    {
      events: [
        { start: 540, end: 600, color: BLUE, title: "readSources", preset: "balanced", icons: MUSIC_ICONS },
        { start: 600, end: 690, color: CYAN, title: "draftOutline", preset: "creative" },
        LUNCH,
        { start: 780, end: 840, color: LAVENDER, title: "teamMeeting", icons: MEETING_ICONS },
      ],
      idle: [{ start: 614, end: 622 }],
    },
    {
      events: [
        { start: 550, end: 630, color: CYAN, title: "draftOutline", preset: "deep" },
        { start: 630, end: 690, color: PURPLE, title: "reviewNotes", preset: "creative" },
        LUNCH,
        { start: 810, end: 870, color: BLUE, title: "readSources", preset: "deep", icons: MUSIC_ICONS },
      ],
      idle: [{ start: 568, end: 574 }, { start: 671, end: 683 }],
    },
    {
      events: [
        { start: 540, end: 600, color: CYAN, title: "draftOutline", preset: "creative" },
        { start: 600, end: 690, color: BLUE, title: "readSources", preset: "deep", icons: MUSIC_ICONS },
        LUNCH,
        { start: 780, end: 840, color: LAVENDER, title: "teamMeeting", icons: MEETING_ICONS },
      ],
      idle: [{ start: 606, end: 613 }],
    },
    {
      events: [
        { start: 540, end: 630, color: TAUPE, title: "writeChapter", preset: "creative" },
        { start: 630, end: 690, color: PURPLE, title: "reviewNotes", preset: "balanced" },
        LUNCH,
        { start: 810, end: 870, color: BLUE, title: "readSources", preset: "balanced", icons: MUSIC_ICONS },
      ],
    },
    {
      events: [
        { start: 570, end: 660, color: BLUE, title: "readSources", preset: "deep", icons: MUSIC_ICONS },
        { start: 660, end: 720, color: PURPLE, title: "reviewNotes", preset: "creative" },
        LUNCH,
        { start: 780, end: 840, color: LAVENDER, title: "teamMeeting", icons: MEETING_ICONS },
      ],
    },
  ];

  /** The film's own day: the session drawn on it, the planned event right after it, then a late lunch. */
  const FILM_DAY: readonly FilmEvent[] = [
    { start: DRAWN_END, end: SESSION_END, color: BLUE, title: "readSources", icons: MUSIC_ICONS },
    { ...LUNCH, start: SESSION_END, end: SESSION_END + 60 },
  ];

  const now = new Date();
  const filmDate = filmWorkday(now);
  /** The workweek around the film's day, with each day's offset from it. */
  const days = WORKWEEK.map((workday, weekday) => {
    const offset = weekday + 1 - filmDate.getDay();
    const events = offset === 0 ? FILM_DAY : workday.events;
    return {
      offset,
      events,
      idle: offset === 0 ? [] : (workday.idle ?? []),
      date: new Date(filmDate.getFullYear(), filmDate.getMonth(), filmDate.getDate() + offset),
      rails: filmDayRails(events.flatMap((event) => (event.preset ? [{ start: event.start, end: event.end, preset: event.preset }] : []))),
    };
  });
  const timezone = getLocalTimezone();
  const timezoneLabel = getTimezoneInfo(timezone).acronym;

  /** The canvas inside its bezel, and how much of the day it shows. */
  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);
  const viewMinutes = $derived(((canvasH - TOP_BAR_H - HEADER_H) / HOUR_PX) * 60);
  const hours = $derived(
    Array.from({ length: Math.ceil(viewMinutes / 60) + 1 }, (_, index) => Math.ceil(VIEW_START / 60) + index)
      .filter((hour) => hour * 60 > VIEW_START && hour * 60 < VIEW_START + viewMinutes),
  );

  const breakCopy = $derived(pomodoroBlockedScreenCopy("break_countdown", t));
  /**
   * The drawn event ends with a later Focus event still ahead today, so it is an event finished. The planned event ends
   * the day's Focus, so it completes the day, or the workweek on a Friday, as the Focus runtime classifies it.
   */
  const dayDoneState: PomodoroCompletionScreenState = filmDate.getDay() === FRIDAY ? "workweek_complete" : "day_complete";
  const eventFinishedCopy = $derived(pomodoroBlockedScreenCopy("event_finished", t));
  const dayDoneCopy = $derived(pomodoroBlockedScreenCopy(dayDoneState, t));
  const breakPalette = pomodoroBlockedScreenPalette("break_countdown");
  const eventFinishedPalette = pomodoroBlockedScreenPalette("event_finished");
  const dayDonePalette = pomodoroBlockedScreenPalette(dayDoneState);
  const extensionHint = formatBreakExtensionHint(0, DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT, undefined, t) !== null
    ? formatBreakExtensionShortcut()
    : null;
  const endBreakHint = formatBreakEndEarlyShortcut(0, DEFAULT_FOCUS_BREAK_END_ESC_PRESSES);

  const calendarDark = $derived(isThemeCalendarDark(theme.current));
  const clock = $derived(SESSION_START + filmClockMinute(elapsed));
  const sessionMinute = $derived(filmSessionMinute(elapsed));
  const moment = $derived(filmSessionMoment(sessionMinute));
  const focusing = $derived(elapsed >= FILM_BEATS.focusStart && moment.kind !== "done");
  /** The session's breaks in minutes of the day, sliding into place as the drawn event joins the planned one. */
  const todayBreaks = $derived(inDay(filmRailBreaks(elapsed)));

  /** The session while it is dragged out, then saved. */
  const drawn = $derived(easeInOutCubic(filmSpan(elapsed, FILM_BEATS.createStart, FILM_BEATS.createEnd)));
  const drawnMinutes = $derived(
    Math.max(CREATE_SNAP_MINUTES, Math.round((drawn * FILM_DRAWN_MINUTES) / CREATE_SNAP_MINUTES) * CREATE_SNAP_MINUTES),
  );
  const creating = $derived(elapsed >= FILM_BEATS.createStart && elapsed < FILM_BEATS.createSaved);
  const saved = $derived(elapsed >= FILM_BEATS.createSaved);
  const pressOpacity = $derived(
    filmSpan(elapsed, FILM_BEATS.createStart - PRESS_FADE_MS, FILM_BEATS.createStart)
      * (1 - filmSpan(elapsed, FILM_BEATS.createEnd, FILM_BEATS.createEnd + PRESS_FADE_MS)),
  );

  /** The pull back: other days open beside today, which narrows to one column of the week. */
  const pullBack = $derived(easeInOutCubic(filmSpan(elapsed, FILM_BEATS.pullBack, FILM_BEATS.pullBack + FILM_PULL_BACK_MS)));
  const otherGrow = $derived(pullBack);
  const dayFormat = $derived(dayNameFormatForWidth((canvasW - GUTTER_W) / (1 + (days.length - 1) * otherGrow)));

  const breakScreen = $derived(screenAlpha(FILM_BEATS.breakStart, FILM_BEATS.breakEnd + SCREEN_FADE_MS));
  const eventFinishedScreen = $derived(screenAlpha(FILM_BEATS.eventEnd, FILM_BEATS.eventOut));
  const dayDoneScreen = $derived(screenAlpha(FILM_BEATS.sessionEnd, FILM_BEATS.finishedOut));
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
      case "teamMeeting": return t("vaultSetup.film.teamMeeting");
    }
  }

  function eventColors(color: EventColor | undefined, past: boolean) {
    return preferences.calendarDimPastEvents && past ? getPastEventColor(color, theme.current) : getEventColor(color, theme.current);
  }

  function isPast(offset: number, end: number): boolean {
    return offset < 0 || (offset === 0 && end <= clock);
  }

  /**
   * The focus a rail of the week shows: what was recorded on past days, with the moments the user paused or was away
   * left empty, and none yet on days to come.
   */
  function recordedFocus(rail: FilmRail, offset: number, idle: readonly FilmSpan[]): readonly FilmSpan[] {
    return offset < 0 ? filmSpansWithout(rail.focus, idle) : [];
  }

  /** Session minutes as minutes of the day. */
  function inDay(spans: readonly FilmSpan[]): FilmSpan[] {
    return spans.map((span) => ({ start: SESSION_START + span.start, end: SESSION_START + span.end }));
  }

</script>

{#snippet rail(
  start: number,
  end: number,
  recorded: readonly FilmSpan[],
  breaks: readonly FilmSpan[],
  activeBreak: boolean,
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
  >
    {#each recorded as band (band.start)}
      <div
        class="absolute inset-x-0"
        style:top={`${((band.start - start) / length) * 100}%`}
        style:height={`${((band.end - band.start) / length) * 100}%`}
        style:background-color="var(--cal-timeline-focus)"
      ></div>
    {/each}
    {#each breaks as band, index (index)}
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

{#snippet eventBlock(
  start: number,
  end: number,
  color: EventColor | undefined,
  label: string,
  past: boolean,
  outline: boolean,
  eventBelow = false,
  icons: readonly FilmIcon[] = [],
)}
  {@const colors = outline ? getEventColor(color, theme.current) : eventColors(color, past)}
  {@const shownIcons = px(end - start) >= EVENT_ICONS_MIN_PX ? ICON_ORDER.filter((icon) => icons.includes(icon)) : []}
  <div
    class="absolute inset-x-0 flex overflow-hidden rounded text-[0.8rem] leading-tight"
    style:top={`${y(start)}px`}
    style:height={`${px(end - start) - (eventBelow ? EVENT_GAP_PX : 0)}px`}
    style:background-color={colors.bg}
    style:color={colors.text}
    style:outline={outline ? `2px solid color-mix(in oklab, ${colors.bg} 65%, ${calendarDark ? "white" : "black"})` : undefined}
    style:z-index={outline ? 10 : 3}
  >
    <div class="relative min-w-0 flex-1 overflow-hidden px-1 py-0.5">
      {#if shownIcons.length > 0}
        <div
          class="absolute right-1 flex items-center gap-0.5"
          style:top="5px"
          style:color={`color-mix(in srgb, ${colors.text} 70%, ${colors.bg})`}
        >
          {#each shownIcons as icon (icon)}
            {#if icon === "music"}
              <Music size={9} class="shrink-0" />
            {:else if icon === "repeat"}
              <Repeat size={9} class="shrink-0" />
            {:else}
              <Users size={9} class="shrink-0" />
            {/if}
          {/each}
        </div>
      {/if}
      <div class="truncate font-medium" class:pr-5={shownIcons.length > 0}>{label}</div>
      {#if px(end - start) >= EVENT_TIME_MIN_PX}
        <div class="truncate" style:color={`color-mix(in srgb, ${colors.text} 80%, ${colors.bg})`}>{timeRange(start, end)}</div>
      {/if}
    </div>
  </div>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5">
  <div
    class="relative flex flex-col overflow-hidden rounded-2xl bg-(--cal-bg) text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    {#if mobileShell}
      <!-- The top bar's Focus ring, on its own. -->
      <div class="flex shrink-0 items-center justify-end bg-background px-1.5" style:height={`${TOP_BAR_H}px`}>
        <span class="grid size-8 place-items-center rounded-full text-muted-foreground">
          <PomodoroProgressRing
            active={focusing && moment.kind !== "done"}
            remainingSeconds={moment.kind === "done" ? 0 : moment.remainingSeconds}
            totalSeconds={moment.kind === "done" ? 0 : moment.totalSeconds}
            size={20}
            trackClass="stroke-muted-foreground/35"
            progressClass="text-foreground/76 stroke-foreground/76"
          />
        </span>
      </div>
    {:else}
      <!-- The title bar's Focus ring, on its own. -->
      <div class="flex shrink-0 items-center justify-end bg-sidebar pr-1.5" style:height={`${TOP_BAR_H}px`}>
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
              {@render rail(dayRail.start, dayRail.end, recordedFocus(dayRail, day.offset, day.idle), dayRail.breaks, false)}
            {/each}

            {#if isToday && !saved}
              <!-- The planned event's own rail, until the drawn event joins it and the rail is replanned across both. -->
              {@render rail(DRAWN_END, SESSION_END, [], PLANNED_BREAKS, false)}
            {:else if isToday}
              {@render rail(
                SESSION_START,
                SESSION_END,
                inDay(filmFocusSpans(sessionMinute)),
                todayBreaks,
                focusing && moment.kind === "break",
              )}
            {/if}

            <div class="absolute inset-y-0 right-0" style:left={`${EVENT_LEFT}px`}>
              {#each day.events as event (event.start)}
                {@render eventBlock(
                  event.start,
                  event.end,
                  event.color,
                  title(event.title),
                  isPast(day.offset, event.end),
                  false,
                  day.events.some((other) => other.start === event.end),
                  event.icons,
                )}
              {/each}

              {#if isToday}
                {#if creating}
                  {@render eventBlock(SESSION_START, SESSION_START + drawnMinutes, undefined, t("calendar.event.noTitle"), false, true)}
                {:else if saved}
                  {@render eventBlock(SESSION_START, DRAWN_END, TAUPE, title("writeChapter"), isPast(0, DRAWN_END), false, true)}
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
          {formatBlockedScreenDateTime(new Date(filmDate.getFullYear(), filmDate.getMonth(), filmDate.getDate(), 0, clock), localization.locale)}
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

    <!-- The completion screens: the drawn event finished, then the day's Focus completed. -->
    {#each [
      { alpha: eventFinishedScreen, palette: eventFinishedPalette, copy: eventFinishedCopy },
      { alpha: dayDoneScreen, palette: dayDonePalette, copy: dayDoneCopy },
    ] as completion, index (index)}
      {#if completion.alpha > 0}
        <div class="film-screen absolute inset-0 z-50 flex flex-col items-center justify-center gap-8 px-4 text-center" style={screenStyle(completion.palette, completion.alpha)}>
          <p class="film-screen-title film-screen-main">{completion.copy.title}</p>
          <!-- A phone has no keys: its screens continue with a tap. -->
          <p class="film-screen-small film-screen-muted">
            {mobileShell ? t("pomodoroOverlay.tapToContinue") : completion.copy.subtitle}
          </p>
        </div>
      {/if}
    {/each}
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
