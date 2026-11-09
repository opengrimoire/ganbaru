<script lang="ts" module>
  import { CALENDAR_SCROLLBAR_TRACK_PX, calendarScrollbarThumb } from "$lib/calendar/timeline-scroll";
  import { GUTTER_WIDTH_PER_TIMEZONE } from "$lib/calendar/timezone-utils";
  import { calendarZoomGridMinutesForPercent, calendarZoomHourHeightForPercent } from "$lib/calendar/zoom-levels";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";

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
  const MAX_CANVAS_W = 760;
  /** The title bar on desktop, or the phone's top bar, tall enough for its label. */
  const TOP_BAR_H = BUILD_PLATFORM_PROFILE.shell === "mobile" ? 48 : 36;
  const HEADER_H = 40;
  const HOUR_PX = calendarZoomHourHeightForPercent(FILM_ZOOM_PERCENT);
  /** How the drawn event snaps while it is dragged out, in minutes, as the Calendar snaps at the film's zoom. */
  const CREATE_SNAP_MINUTES = calendarZoomGridMinutesForPercent(FILM_ZOOM_PERCENT);
  /**
   * Where the visible part of the day starts, in minutes since midnight, as if the grid were scrolled to the morning:
   * a little before eight, so the workday's first hour label shows.
   */
  const VIEW_START = 7 * 60 + 40;
  /**
   * How much of the day shows: at least the whole session and the lunch after it, so an event without Focus is always
   * in view beside the Focus events, and on through the afternoon's events in a tall room such as a phone's, where the
   * drawing's width is already at its smallest and only more hours can fill the height.
   */
  const MIN_VIEW_MINUTES = 14 * 60 - VIEW_START;
  const MAX_VIEW_MINUTES = 690;
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
  import PomodoroMenuVolume from "$lib/components/pomodoro/PomodoroMenuVolume.svelte";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import WelcomeCursor from "$lib/components/vault/welcome/WelcomeCursor.svelte";
  import WelcomeTouch from "$lib/components/vault/welcome/WelcomeTouch.svelte";
  import WelcomePhoneTopBar from "$lib/components/vault/welcome/WelcomePhoneTopBar.svelte";
  import WelcomeTitleBar, {
    WELCOME_TITLE_AFTER_RING_PX,
    WELCOME_TITLE_RING_PROGRESS_CLASS,
    WELCOME_TITLE_RING_SIZE,
    WELCOME_TITLE_RING_TRACK_CLASS,
    type WelcomeTitleTab,
  } from "$lib/components/vault/welcome/WelcomeTitleBar.svelte";
  import WelcomeFilmEventPanel from "$lib/components/vault/welcome/WelcomeFilmEventPanel.svelte";
  import WelcomeFilmNotification, { type FilmNotificationKind } from "$lib/components/vault/welcome/WelcomeFilmNotification.svelte";
  import type { EventPanelAnchor } from "$lib/utils/responsive";
  import type { EventColor } from "$lib/calendar/types";
  import ClockPlus from "@lucide/svelte/icons/clock-plus";
  import Coffee from "@lucide/svelte/icons/coffee";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import PauseIcon from "@lucide/svelte/icons/pause";
  import PlayIcon from "@lucide/svelte/icons/play";
  import Repeat from "@lucide/svelte/icons/repeat";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import SkipForward from "@lucide/svelte/icons/skip-forward";
  import Users from "@lucide/svelte/icons/users";
  import { DEFAULT_PLAYBACK_SNAPSHOT, MAX_VOLUME } from "$lib/music/playback";
  import type { PomodoroPresetKey } from "$lib/pomodoro/rhythm";
  import {
    CALENDAR_EVENT_RIGHT_GUTTER_PCT,
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
  import { BUILD_PLATFORM } from "$lib/platform";
  import { focusPausedPulseAmount } from "$lib/pomodoro/native-focus";
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
    FILM_DRAWN_MINUTES,
    FILM_PLANNED_RAIL,
    FILM_PULL_BACK_MS,
    FILM_SESSION_MINUTES,
    FILM_TAP_MS,
    easeInOutCubic,
    filmBreakTint,
    filmClockMinute,
    filmDayRails,
    filmFocusSpans,
    filmPaused,
    filmRailBreaks,
    filmNotification,
    filmRingMenuOpen,
    filmSessionMinute,
    filmSessionMoment,
    filmEventPanelFrame,
    filmCursorScript,
    filmPanelOpen,
    filmProjectPicked,
    filmSpan,
    filmTap,
    filmTypedTitle,
    filmWorkday,
    type FilmRail,
    type FilmSpan,
  } from "$lib/vault/welcome-film";
  import { welcomeCursorHovered } from "$lib/vault/welcome-cursor";

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
  /** The desktop title bar's end padding, as the title bar draws it. */
  const TITLE_BAR_END_PX = 6;
  /** The title bar's tabs: the Calendar alone, as a new vault opens. */
  const TITLE_TABS: readonly WelcomeTitleTab[] = [{ kind: "calendar", active: true }];
  /** The Focus menu's icons and how much its extend item adds, as the menu draws them. */
  const MENU_ICON_SIZE = 14;
  const MENU_ICON_STROKE_WIDTH = 1.8;
  const FOCUS_EXTENSION_MINUTES = 3;
  /** Where the Focus menu opens: a little under the bar, its end on the ring's end on desktop and by the screen's edge on a phone. */
  const MENU_TOP_PX = TOP_BAR_H + 4;
  const MENU_RIGHT_PX = mobileShell ? 8 : TITLE_BAR_END_PX + WELCOME_TITLE_AFTER_RING_PX;

  /** The Calendar's Focus rail, and the event area it keeps beside it on every event. */
  const RAIL_LEFT = 2;
  const RAIL_W = 6;
  const EVENT_LEFT = RAIL_W + 4;
  /** The gridline on a day column's left edge. */
  const DAY_BORDER_PX = 1;
  /** The Calendar's slight separation above an event that starts right as another ends. */
  const EVENT_GAP_PX = 2;
  /** Minimum height for an event to show its time under its title. */
  const EVENT_TIME_MIN_PX = 32;
  const PRESS_FADE_MS = 140;
  const SCREEN_FADE_MS = 260;
  /** The Calendar's pulse on a break that is running. */
  const BREAK_PULSE_MS = 1500;

  /** The session starts at eight: the drawn event, then the planned one right after it. */
  const SESSION_START = 8 * 60;
  const DRAWN_END = SESSION_START + FILM_DRAWN_MINUTES;
  const SESSION_END = SESSION_START + FILM_SESSION_MINUTES;
  /** The planned event's breaks while it has its rail to itself. */
  const PLANNED_BREAKS = inDay(FILM_PLANNED_RAIL.breaks);

  /** Palette slots, so the drawing follows the active theme's event colors. Meals take the Eating routine's color. */
  const BLUE = 18;
  const PURPLE = 22;
  const CYAN = 16;
  const LAVENDER = 20;
  /** The Thesis project's color, the eighth swatch of the color picker. */
  const THESIS = 7;
  const ASSIGNMENTS = 9;
  const EATING = 13;
  const EXERCISE = 0;

  type FilmTitle =
    | "writeChapter"
    | "reviewNotes"
    | "lunch"
    | "draftOutline"
    | "readSources"
    | "teamMeeting"
    | "problemSet"
    | "gym";
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

  /** The Calendar's icon order on an event. */
  const ICON_ORDER: readonly FilmIcon[] = ["music", "repeat", "meeting"];
  /** Minimum height for an event to show its icons. */
  const EVENT_ICONS_MIN_PX = 17;
  const REPEAT_ICONS: readonly FilmIcon[] = ["repeat"];
  const MUSIC_ICONS: readonly FilmIcon[] = ["music"];
  const MEETING_ICONS: readonly FilmIcon[] = ["repeat", "meeting"];

  /** Lunch, at the same hour every day of the week, right as the film's session ends. */
  const LUNCH: FilmEvent = { start: 13 * 60, end: 14 * 60, color: EATING, title: "lunch", icons: REPEAT_ICONS };
  /** The team meeting on Monday, Wednesday, and Friday afternoons, and the gym on Tuesday and Thursday evenings. */
  const MEETING: FilmEvent = { start: 14 * 60, end: 15 * 60, color: LAVENDER, title: "teamMeeting", icons: MEETING_ICONS };
  const GYM: FilmEvent = { start: 16 * 60, end: 17 * 60, color: EXERCISE, title: "gym", icons: REPEAT_ICONS };

  /**
   * What the workweek holds from Monday to Friday. Events start and end on the hour or the half hour, as people plan
   * them. Each morning is a chain of Focus events back to back with different presets, long enough for a full cycle of
   * four focuses, so its rail shows the rhythm carrying from one event into the next and reaching its long break.
   */
  const WORKWEEK: readonly (readonly FilmEvent[])[] = [
    [
      { start: 480, end: 600, color: BLUE, title: "readSources", preset: "deep", icons: MUSIC_ICONS },
      { start: 600, end: 720, color: CYAN, title: "draftOutline", preset: "creative" },
      LUNCH,
      MEETING,
      { start: 900, end: 990, color: ASSIGNMENTS, title: "problemSet", preset: "balanced" },
    ],
    [
      { start: 510, end: 660, color: CYAN, title: "draftOutline", preset: "deep" },
      { start: 660, end: 750, color: PURPLE, title: "reviewNotes", preset: "creative" },
      LUNCH,
      { start: 840, end: 930, color: BLUE, title: "readSources", preset: "balanced", icons: MUSIC_ICONS },
      GYM,
    ],
    [
      { start: 480, end: 570, color: ASSIGNMENTS, title: "problemSet", preset: "creative" },
      { start: 570, end: 720, color: BLUE, title: "readSources", preset: "deep", icons: MUSIC_ICONS },
      LUNCH,
      MEETING,
      { start: 900, end: 990, color: CYAN, title: "draftOutline", preset: "balanced" },
    ],
    [
      { start: 480, end: 630, color: THESIS, title: "writeChapter", preset: "creative" },
      { start: 630, end: 720, color: PURPLE, title: "reviewNotes", preset: "balanced" },
      LUNCH,
      { start: 870, end: 960, color: BLUE, title: "readSources", preset: "balanced", icons: MUSIC_ICONS },
      GYM,
    ],
    [
      { start: 480, end: 630, color: BLUE, title: "readSources", preset: "deep", icons: MUSIC_ICONS },
      { start: 630, end: 720, color: PURPLE, title: "reviewNotes", preset: "creative" },
      LUNCH,
      MEETING,
      { start: 900, end: 990, color: ASSIGNMENTS, title: "problemSet", preset: "balanced" },
    ],
  ];

  /**
   * The film's own day: the session drawn on it, the planned event right after it that is still ahead when the film
   * ends, lunch, then an afternoon with no more Focus.
   */
  const FILM_DAY: readonly FilmEvent[] = [
    { start: DRAWN_END, end: SESSION_END, color: BLUE, title: "readSources", icons: MUSIC_ICONS },
    LUNCH,
    GYM,
  ];

  const now = new Date();
  const filmDate = filmWorkday(now);
  /** The workweek around the film's day, with each day's offset from it. */
  const days = WORKWEEK.map((template, weekday) => {
    const offset = weekday + 1 - filmDate.getDay();
    const events = offset === 0 ? FILM_DAY : template;
    return {
      offset,
      events,
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
  /** The Calendar's scrollbar thumb, placed as if the whole day's grid were scrolled to the visible part. */
  const scrollThumb = $derived.by(() => {
    const visibleHeight = canvasH - TOP_BAR_H - HEADER_H;
    return calendarScrollbarThumb(
      { scrollTop: (VIEW_START / 60) * HOUR_PX, scrollHeight: 24 * HOUR_PX, clientHeight: visibleHeight },
      visibleHeight,
    );
  });

  const breakCopy = $derived(pomodoroBlockedScreenCopy("break_countdown", t));
  /** The drawn event ends with a later Focus event still ahead today, so it is an event finished. */
  const eventFinishedCopy = $derived(pomodoroBlockedScreenCopy("event_finished", t));
  const breakPalette = pomodoroBlockedScreenPalette("break_countdown");
  const eventFinishedPalette = pomodoroBlockedScreenPalette("event_finished");
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

  /** The pause from the Focus ring: a tap opens its menu, and a tap on its first item pauses Focus. */
  const paused = $derived(filmPaused(elapsed));
  const ringMenuOpen = $derived(filmRingMenuOpen(elapsed));
  const ringTouch = $derived(filmTap(elapsed, FILM_BEATS.pauseTap + FILM_TAP_MS));
  const menuItemTouch = $derived(filmTap(elapsed, FILM_BEATS.pauseStart));
  /**
   * The mouse pointer a desktop build draws in place of the touches, and the control it rests on, which shows as
   * hovered. A phone keeps its touches.
   */
  const cursorScript = filmCursorScript();
  const hovered = $derived(mobileShell ? null : welcomeCursorHovered(cursorScript, elapsed));
  /** The ring menu's pause item shows as highlighted while it is touched on a phone, or while the pointer is on it. */
  const pauseItemHighlighted = $derived(mobileShell ? menuItemTouch > 0 : hovered === "film-ring-pause");
  const ringRemaining = $derived(moment.kind === "done" ? 0 : moment.remainingSeconds);
  const ringTotal = $derived(moment.kind === "done" ? 0 : moment.totalSeconds);
  /**
   * The paused notification the app sends, drawn as this platform's system shows it: Android's ongoing Focus
   * notification, a Windows toast, or a GNOME banner on Linux. Its Resume action is what resumes Focus.
   */
  const notificationKind: FilmNotificationKind = mobileShell
    ? "android"
    : BUILD_PLATFORM === "windows"
      ? "windows"
      : "gnome";
  const notification = $derived(filmNotification(elapsed));
  const resumeTouch = $derived(filmTap(elapsed, FILM_BEATS.resumeAt));
  /** The paused phase's progress, as the phone's ongoing notification draws it. */
  const notificationProgress = $derived(ringTotal > 0 ? (ringTotal - ringRemaining) / ringTotal : 0);

  /** The session while it is dragged out, then saved. */
  const drawn = $derived(easeInOutCubic(filmSpan(elapsed, FILM_BEATS.createStart, FILM_BEATS.createEnd)));
  const drawnMinutes = $derived(
    Math.max(CREATE_SNAP_MINUTES, Math.round((drawn * FILM_DRAWN_MINUTES) / CREATE_SNAP_MINUTES) * CREATE_SNAP_MINUTES),
  );
  /** Where the press that drags the session out is, in minutes past its start: on the start line, then smoothly down as the event snaps. */
  const pressMinutes = $derived(drawn * FILM_DRAWN_MINUTES);
  const creating = $derived(elapsed >= FILM_BEATS.createStart && elapsed < FILM_BEATS.createSaved);
  const saved = $derived(elapsed >= FILM_BEATS.createSaved);
  /** The drawn event's title as it is typed into the event panel, and whether its project has been picked. */
  const typedTitle = $derived(filmTypedTitle(elapsed, title("writeChapter")));
  const projectPicked = $derived(filmProjectPicked(elapsed));
  /** The event panel's anchor: the drawn event, in today's column while it still fills the grid. */
  const panelAnchor = $derived.by<EventPanelAnchor>(() => {
    const columnLeft = GUTTER_W + DAY_BORDER_PX + EVENT_LEFT;
    const blockWidth = ((canvasW - columnLeft) * (100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT)) / 100;
    return {
      x: columnLeft + blockWidth,
      y: TOP_BAR_H + HEADER_H + y(SESSION_START),
      width: blockWidth,
      height: px(FILM_DRAWN_MINUTES),
    };
  });
  const pressOpacity = $derived(
    filmSpan(elapsed, FILM_BEATS.createStart - PRESS_FADE_MS, FILM_BEATS.createStart)
      * (1 - filmSpan(elapsed, FILM_BEATS.createEnd, FILM_BEATS.createEnd + PRESS_FADE_MS)),
  );

  /** The pull back: other days open beside today, which narrows to one column of the week. */
  const pullBack = $derived(easeInOutCubic(filmSpan(elapsed, FILM_BEATS.pullBack, FILM_BEATS.pullBack + FILM_PULL_BACK_MS)));
  const otherGrow = $derived(pullBack);
  const dayFormat = $derived(dayNameFormatForWidth((canvasW - GUTTER_W) / (1 + (days.length - 1) * otherGrow)));

  /** The first break's screen, then a faint glimpse of it for each break the session fast-forwards through. */
  const breakScreen = $derived(
    Math.max(screenAlpha(FILM_BEATS.breakStart, FILM_BEATS.breakEnd + SCREEN_FADE_MS), filmBreakTint(elapsed)),
  );
  const eventFinishedScreen = $derived(screenAlpha(FILM_BEATS.eventEnd, FILM_BEATS.eventOut));
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

  /** A Focus countdown as the ring's menu shows it, in minutes and seconds. */
  function timerLabel(seconds: number): string {
    return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
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
      case "problemSet": return t("vaultSetup.film.problemSet");
      case "gym": return t("vaultSetup.film.gym");
    }
  }

  function eventColors(color: EventColor | undefined, past: boolean) {
    return preferences.calendarDimPastEvents && past ? getPastEventColor(color, theme.current) : getEventColor(color, theme.current);
  }

  function isPast(offset: number, end: number): boolean {
    return offset < 0 || (offset === 0 && end <= clock);
  }

  /** The focus a rail of the week shows: all of it recorded on past days, none yet on days to come. */
  function recordedFocus(rail: FilmRail, offset: number): readonly FilmSpan[] {
    return offset < 0 ? rail.focus : [];
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

{#snippet touch(opacity: number, left?: string)}
  {#if mobileShell}<WelcomeTouch {opacity} {left} />{/if}
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
    class="absolute left-0 flex overflow-hidden rounded text-[0.8rem] leading-tight"
    style:width={`${100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT}%`}
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
      <!-- The phone's top bar, labelled with the Calendar, with its Focus ring lit while its menu is open. -->
      <WelcomePhoneTopBar height={TOP_BAR_H} identity="calendar" ringOpen={ringMenuOpen}>
        {#snippet ring()}
          <PomodoroProgressRing
            active={focusing}
            remainingSeconds={ringRemaining}
            totalSeconds={ringTotal}
            {paused}
            pausedPulseAmount={paused ? focusPausedPulseAmount(elapsed) : null}
            size={20}
            trackClass="stroke-muted-foreground/35"
            progressClass="text-foreground/76 stroke-foreground/76"
          />
          {@render touch(ringTouch)}
        {/snippet}
      </WelcomePhoneTopBar>
    {:else}
      <!-- The title bar's Calendar tab and utilities, with its Focus ring running with Focus. -->
      <WelcomeTitleBar
        height={TOP_BAR_H}
        endPadding={TITLE_BAR_END_PX}
        tabs={TITLE_TABS}
        ringOpen={ringMenuOpen}
        ringHovered={hovered === "film-ring"}
        ringCursor="film-ring"
      >
        {#snippet ring()}
          <PomodoroProgressRing
            active={focusing}
            remainingSeconds={ringRemaining}
            totalSeconds={ringTotal}
            {paused}
            pausedPulseAmount={paused ? focusPausedPulseAmount(elapsed) : null}
            size={WELCOME_TITLE_RING_SIZE}
            trackClass={WELCOME_TITLE_RING_TRACK_CLASS}
            progressClass={WELCOME_TITLE_RING_PROGRESS_CLASS}
          />
        {/snippet}
      </WelcomeTitleBar>
    {/if}

    <!-- The Focus ring's menu, open while it is used to pause Focus. -->
    {#if ringMenuOpen}
      <div
        class="surface-floating surface-floating-body pointer-events-none absolute z-50 w-floating-sm"
        style:top={`${MENU_TOP_PX}px`}
        style:right={`${MENU_RIGHT_PX}px`}
      >
        <div class="menu-label tabular-nums">{t("titleBar.pomodoro.left", timerLabel(ringRemaining))}</div>
        <div
          data-cursor="film-ring-pause"
          class="menu-item relative justify-between gap-4 whitespace-nowrap"
          data-highlighted={pauseItemHighlighted ? "" : undefined}
        >
          <span>{t("titleBar.pomodoro.pauseFocus")}</span>
          <PauseIcon class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
          {@render touch(menuItemTouch, "40%")}
        </div>
        <div class="menu-item justify-between gap-4 whitespace-nowrap">
          <span>{t("titleBar.pomodoro.extendFocusMinutes", FOCUS_EXTENSION_MINUTES)}</span>
          <ClockPlus class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
        </div>
        <div class="menu-item justify-between gap-4 whitespace-nowrap">
          <span>{t("titleBar.pomodoro.goToBreakNow")}</span>
          <Coffee class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
        </div>
        <div role="separator" class="menu-separator"></div>
        <div class="menu-label"><span class="block truncate">{t("titleBar.music.noMusicLoaded")}</span></div>
        <div class="menu-item justify-between gap-4 whitespace-nowrap" aria-disabled="true">
          <span>{t("titleBar.music.play")}</span>
          <PlayIcon class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
        </div>
        <div class="menu-item justify-between gap-4 whitespace-nowrap" aria-disabled="true">
          <span>{t("titleBar.music.previous")}</span>
          <SkipBack class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
        </div>
        <div class="menu-item justify-between gap-4 whitespace-nowrap" aria-disabled="true">
          <span>{t("titleBar.music.next")}</span>
          <SkipForward class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
        </div>
        <PomodoroMenuVolume value={DEFAULT_PLAYBACK_SNAPSHOT.volume} max={MAX_VOLUME} />
        <div class="menu-item justify-between gap-4 whitespace-nowrap">
          <span>{t("titleBar.music.open")}</span>
          <ExternalLink class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
        </div>
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
      <!-- The Calendar's scrollbar, over the right edge of the grid as in the week view. -->
      {#if scrollThumb.height > 0}
        <div class="absolute inset-y-0 right-0 z-48" style:width={`${CALENDAR_SCROLLBAR_TRACK_PX}px`}>
          <div
            class="absolute inset-x-0.5 rounded-full"
            style:top={`${scrollThumb.top}px`}
            style:height={`${scrollThumb.height}px`}
            style:background-color="var(--cal-scrollbar-thumb, var(--muted))"
          ></div>
        </div>
      {/if}
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

      <div data-cursor="film-grid" class="relative flex min-w-0 flex-1">
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
              {@render rail(dayRail.start, dayRail.end, recordedFocus(dayRail, day.offset), dayRail.breaks, false)}
            {/each}

            {#if isToday && elapsed < FILM_BEATS.panelOpen}
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
                  {@render eventBlock(
                    SESSION_START,
                    SESSION_START + drawnMinutes,
                    projectPicked ? THESIS : undefined,
                    typedTitle || t("calendar.event.noTitle"),
                    false,
                    true,
                  )}
                {:else if saved}
                  {@render eventBlock(SESSION_START, DRAWN_END, THESIS, title("writeChapter"), isPast(0, DRAWN_END), false, true)}
                {/if}

                <!-- The drawn event's resize knobs, as a phone shows them from the start of the drawing until Save. -->
                {#if mobileShell && creating}
                  <div
                    class="pointer-events-none absolute left-0"
                    style:width={`${100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT}%`}
                    style:top={`${y(SESSION_START)}px`}
                    style:height={`${px(drawnMinutes)}px`}
                    style:z-index="11"
                    style:--knob-color={getEventColor(projectPicked ? THESIS : undefined, theme.current).bg}
                  >
                    <div class="film-resize-knob absolute left-0 top-0"></div>
                    <div class="film-resize-knob absolute bottom-0 right-0"></div>
                  </div>
                {/if}

                <!--
                  Where the desktop pointer presses on the empty slot and drags the session out: its bottom edge follows
                  the drag smoothly while the event snaps to the grid.
                -->
                {#if !mobileShell && elapsed < FILM_BEATS.panelOpen}
                  <span
                    data-cursor="film-slot film-drag"
                    class="pointer-events-none absolute left-0"
                    style:width={`${100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT}%`}
                    style:top={`${y(SESSION_START)}px`}
                    style:height={`${px(pressMinutes)}px`}
                  ></span>
                {/if}

                <!-- The press that draws the session on a phone, following its growing edge. -->
                {#if mobileShell}
                  <WelcomeTouch opacity={pressOpacity} left="60%" top={`${y(SESSION_START + pressMinutes)}px`} />
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

    <!-- The event panel on the drawn event, where its project is picked and its title typed before it is saved. -->
    {#if filmPanelOpen(elapsed)}
      <WelcomeFilmEventPanel
        frame={filmEventPanelFrame(elapsed, title("writeChapter"))}
        canvasWidth={canvasW}
        canvasHeight={canvasH}
        titleBarHeight={TOP_BAR_H}
        anchor={panelAnchor}
        date={filmDate}
        start={SESSION_START}
        end={DRAWN_END}
        projectColor={THESIS}
        title={title("writeChapter")}
        {hovered}
      />
    {/if}

    <!-- The paused notification, drawn as the system shows it, until its Resume action is pressed. -->
    {#if notification > 0}
      <WelcomeFilmNotification
        kind={notificationKind}
        shown={notification}
        title={notificationKind === "android" ? title("writeChapter") : t("pomodoroNotification.pausedReminderTitle")}
        body={notificationKind === "android" ? t("pomodoroNotification.pausedText") : t("pomodoroNotification.pausedReminderBody")}
        actions={notificationKind === "android"
          ? [{ label: t("pomodoroNotification.resumeFocusLabel"), touch: resumeTouch }]
          : [
              { label: t("pomodoroNotification.resumeFocusLabel"), touch: resumeTouch },
              { label: t("pomodoroNotification.dismissPromptsLabel"), touch: 0 },
            ]}
        progress={notificationKind === "android" ? notificationProgress : null}
      />
    {/if}

    <!-- The drawn event's finished screen. -->
    {#if eventFinishedScreen > 0}
      <div class="film-screen absolute inset-0 z-50 flex flex-col items-center justify-center gap-8 px-4 text-center" style={screenStyle(eventFinishedPalette, eventFinishedScreen)}>
        <p class="film-screen-title film-screen-main">{eventFinishedCopy.title}</p>
        <!-- A phone has no keys: its screen continues with a tap. -->
        <p class="film-screen-small film-screen-muted">
          {mobileShell ? t("pomodoroOverlay.tapToContinue") : eventFinishedCopy.subtitle}
        </p>
      </div>
    {/if}

    {#if !mobileShell}
      <WelcomeCursor script={cursorScript} {elapsed} width={canvasW} />
    {/if}
  </div>
</div>

<style>
  /* The Calendar's touch resize knob: a dot centered on the event's edge, inset from its side. */
  .film-resize-knob {
    width: 2.75rem;
    height: 2.75rem;
    margin: -1.375rem -0.5rem;
  }

  .film-resize-knob::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 50%;
    width: 0.875rem;
    height: 0.875rem;
    border-radius: 9999px;
    background-color: var(--cal-bg);
    border: 2px solid var(--knob-color);
    transform: translate(-50%, -50%);
  }

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
