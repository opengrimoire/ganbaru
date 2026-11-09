<script lang="ts" module>
  import { calendarZoomGridMinutesForPercent, calendarZoomHourHeightForPercent } from "$lib/calendar/zoom-levels";

  /** The Calendar zoom the scene's day is drawn at, zoomed in from the default so the hour long event reads tall. */
  const SCENE_ZOOM_PERCENT = 200;
  const HOUR_PX = calendarZoomHourHeightForPercent(SCENE_ZOOM_PERCENT);
  /** How the event snaps while it is dragged out, in minutes, as the Calendar snaps at the scene's zoom. */
  const CREATE_SNAP_MINUTES = calendarZoomGridMinutesForPercent(SCENE_ZOOM_PERCENT);

  /**
   * Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. The smallest height
   * keeps two playlist cards in view under the review's player on a phone's builder.
   */
  const MIN_CANVAS_W = 360;
  const MAX_CANVAS_W = 760;
  const MIN_CANVAS_H = 470;
  const MAX_CANVAS_H = 540;
  /** The bezel around the canvas, matching the other welcome scenes (`p-1.5`). */
  const BEZEL = 6;

  export const MUSIC_SCENE_MIN_WIDTH = MIN_CANVAS_W + 2 * BEZEL;
  export const MUSIC_SCENE_MAX_WIDTH = MAX_CANVAS_W + 2 * BEZEL;
  export const MUSIC_SCENE_MIN_HEIGHT = MIN_CANVAS_H + 2 * BEZEL;
  export const MUSIC_SCENE_MAX_HEIGHT = MAX_CANVAS_H + 2 * BEZEL;
</script>

<script lang="ts">
  import AudioLines from "@lucide/svelte/icons/audio-lines";
  import Check from "@lucide/svelte/icons/check";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import ListMusic from "@lucide/svelte/icons/list-music";
  import ListPlus from "@lucide/svelte/icons/list-plus";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import PauseIcon from "@lucide/svelte/icons/pause";
  import Pencil from "@lucide/svelte/icons/pencil";
  import PlayIcon from "@lucide/svelte/icons/play";
  import RadioTower from "@lucide/svelte/icons/radio-tower";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Search from "@lucide/svelte/icons/search";
  import Users from "@lucide/svelte/icons/users";
  import X from "@lucide/svelte/icons/x";
  import MusicPlaylistIcon from "$lib/components/music/builder/MusicPlaylistIcon.svelte";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import WelcomeCursor from "$lib/components/vault/welcome/WelcomeCursor.svelte";
  import WelcomeTitleIcon from "$lib/components/vault/welcome/WelcomeTitleIcon.svelte";
  import WelcomeTouch from "$lib/components/vault/welcome/WelcomeTouch.svelte";
  import WelcomeFilmEventPanel from "$lib/components/vault/welcome/WelcomeFilmEventPanel.svelte";
  import WelcomeFilmNotification, { type FilmNotificationKind } from "$lib/components/vault/welcome/WelcomeFilmNotification.svelte";
  import { CALENDAR_SCROLLBAR_TRACK_PX, calendarScrollbarThumb } from "$lib/calendar/timeline-scroll";
  import { GUTTER_WIDTH_PER_TIMEZONE } from "$lib/calendar/timezone-utils";
  import type { EventColor } from "$lib/calendar/types";
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
  import { BUILD_PLATFORM, BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { isThemeCalendarDark } from "$lib/themes";
  import { cn } from "$lib/utils";
  import type { EventPanelAnchor } from "$lib/utils/responsive";
  import { welcomeCursorHovered } from "$lib/vault/welcome-cursor";
  import { filmSpan, filmWorkday, type FilmSpan } from "$lib/vault/welcome-film";
  import {
    MUSIC_BEATS,
    MUSIC_CREATE_PRESS_MS,
    MUSIC_CURSOR_SCRIPT,
    MUSIC_EVENT,
    MUSIC_PLAYLISTS,
    musicBuilderShown,
    musicBuilderTouches,
    musicCalendarShown,
    musicClock,
    musicCursorCard,
    musicEventCreating,
    musicEventDrawn,
    musicEventDrawnIn,
    musicEventPanelFrame,
    musicEventRail,
    musicItemReviewed,
    musicNotification,
    musicNowPlaying,
    musicPanelOpen,
    musicPhase,
    musicPhaseSeconds,
    musicPlaylistBump,
    musicPlaylistChecked,
    musicPlaylistCount,
    musicProjectPicked,
    musicRailRecorded,
    musicReviewItem,
    musicReviewItemIn,
    musicReviewProgress,
    musicReviewProgressOfTrack,
    musicTracks,
    musicTypedTitle,
    type MusicScenePlaylist,
    type MusicSceneSource,
    type MusicSceneTrack,
  } from "$lib/vault/welcome-music-scene";

  /**
   * One frame of the welcome scene on Music, drawn with the app's own surfaces. First the playlist builder's review,
   * with its folder tree beside it when the canvas is wide enough and its dock below, saves a local track to a work
   * playlist and a YouTube video to a break playlist. Then the Calendar's day, as the Calendar film draws it, where an
   * event is dragged out and its panel gives it the Thesis project, and with it the project's playlist for each Focus
   * phase, and its title. Once the event's Focus starts on its own, the app's notification says the work track
   * autoplays, and when its break starts, the break video. A phone shows its taps as touches, and a desktop as a mouse
   * pointer that clicks and drags. Every style is a function of `elapsed`, with no CSS transitions.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const theme = getTheme();
  const preferences = getPreferences();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const showSoundscapes = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.soundscapes");

  const TOP_BAR_H = 36;
  const HEADER_H = 40;
  const GUTTER_W = GUTTER_WIDTH_PER_TIMEZONE;
  /** The title bar's Focus ring size and its end padding, as the title bar draws them. */
  const TITLE_ICON_SIZE = 14;
  const TITLE_BAR_END_PX = 6;
  /** The app's notification that a phase's music autoplays, drawn as this platform's system shows it. */
  const notificationKind: FilmNotificationKind = mobileShell
    ? "android"
    : BUILD_PLATFORM === "windows"
      ? "windows"
      : "gnome";

  /** From this canvas width the builder keeps its folder tree beside the review, as the builder's two columns fit. */
  const TREE_MIN_W = 576;
  /** Below this width the review draws its media and play button smaller, as the review does. */
  const TIGHT_REVIEW_W = 380;
  /** Below this width the dock shows only its icons, as the dock does. */
  const DOCK_LABEL_MIN_W = 340;
  /** How much a playlist's count leans toward the primary color at the peak of its bump. */
  const BUMP_TINT = 70;

  /** The Calendar's Focus rail, and the event area it keeps beside it on every event. */
  const RAIL_LEFT = 2;
  const RAIL_W = 6;
  const EVENT_LEFT = RAIL_W + 4;
  const DAY_BORDER_PX = 1;
  const EVENT_GAP_PX = 2;
  const EVENT_TIME_MIN_PX = 32;
  const EVENT_ICONS_MIN_PX = 17;
  /** The Calendar's pulse on a break that is running. */
  const BREAK_PULSE_MS = 1500;
  /** Where the visible part of the day starts, as if the grid were scrolled so the event sits near the top. */
  const VIEW_START = 8 * 60 + 50;

  /** Palette slots, so the drawing follows the active theme's event colors, as the Calendar film picks them. */
  const BLUE = 18;
  const PURPLE = 22;
  const LAVENDER = 20;
  const EATING = 13;
  const EXERCISE = 0;
  /** The Thesis project's color, the eighth swatch of the color picker. */
  const THESIS = 7;

  type SceneTitle = "readSources" | "reviewNotes" | "lunch" | "teamMeeting" | "gym";
  type SceneIcon = "music" | "repeat" | "meeting";
  interface SceneEvent {
    start: number;
    end: number;
    color: EventColor;
    title: SceneTitle;
    icons: readonly SceneIcon[];
  }

  const ICON_ORDER: readonly SceneIcon[] = ["music", "repeat", "meeting"];
  /** The rest of the day around the event the scene opens. */
  const DAY_EVENTS: readonly SceneEvent[] = [
    { start: 8 * 60 + 30, end: 9 * 60 + 30, color: BLUE, title: "readSources", icons: [] },
    { start: 11 * 60 + 30, end: 12 * 60 + 30, color: PURPLE, title: "reviewNotes", icons: [] },
    { start: 13 * 60, end: 14 * 60, color: EATING, title: "lunch", icons: ["repeat"] },
    { start: 14 * 60, end: 15 * 60, color: LAVENDER, title: "teamMeeting", icons: ["repeat", "meeting"] },
    { start: 16 * 60, end: 17 * 60, color: EXERCISE, title: "gym", icons: ["repeat"] },
  ];

  /** The cards of the review's playlists, the two the scene uses first so they stay in view on a narrow builder. */
  const CARD_ORDER: readonly MusicScenePlaylist[] = ["workFocus", "breakCalm", "workGanbare", "breakActive"];
  const LEAD_CARDS = CARD_ORDER.slice(0, 2);
  const MORE_CARDS = CARD_ORDER.slice(2);
  const SOURCES: readonly MusicSceneSource[] = ["local", "youtube"];

  const now = new Date();
  const sceneDate = filmWorkday(now);
  const timezone = getLocalTimezone();
  const timezoneLabel = getTimezoneInfo(timezone).acronym;

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);
  const withTree = $derived(canvasW >= TREE_MIN_W);
  const tightReview = $derived(!withTree && canvasW < TIGHT_REVIEW_W);
  const calendarDark = $derived(isThemeCalendarDark(theme.current));

  const builderShown = $derived(musicBuilderShown(elapsed));
  const calendarShown = $derived(musicCalendarShown(elapsed));
  const builderTouches = $derived(musicBuilderTouches(elapsed));
  const reviewItem = $derived(musicReviewItem(elapsed));
  const reviewTrack = $derived(musicTracks(reviewItem.source)[reviewItem.row]);
  const reviewIn = $derived(musicReviewItemIn(elapsed));
  const reviewProgress = $derived(musicReviewProgress(elapsed));
  const reviewPlayed = $derived(musicReviewProgressOfTrack(elapsed));
  const anyChecked = $derived(CARD_ORDER.some((key) => musicPlaylistChecked(elapsed, key)));
  /** The pick and save touches of the track under review. */
  const pickTouch = $derived(reviewItem.source === "local" ? builderTouches.localPick : builderTouches.youtubePick);
  const saveTouch = $derived(Math.max(builderTouches.localSave, builderTouches.youtubeSave));
  const pickedCard = $derived<MusicScenePlaylist>(reviewItem.source === "local" ? "workFocus" : "breakCalm");

  /**
   * The review's scroll on a narrow builder, which keeps the first two playlist cards above its actions. Beside the
   * tree, the player stays put and only the playlist cards run past the bottom, as the review lays itself out.
   */
  let reviewViewportHeight = $state(0);
  let reviewLeadHeight = $state(0);
  const reviewScroll = $derived(withTree ? 0 : Math.max(0, reviewLeadHeight - reviewViewportHeight));

  const clock = $derived(musicClock(elapsed));
  const phase = $derived(musicPhase(elapsed));
  const focusing = $derived(phase !== null);
  const onBreak = $derived(phase === "short-break" || phase === "long-break");
  const phaseSeconds = $derived(musicPhaseSeconds(elapsed));
  const rail = $derived(musicEventRail(elapsed));
  const recorded = $derived(musicRailRecorded(elapsed));
  const picked = $derived(musicProjectPicked(elapsed));
  const panelOpen = $derived(musicPanelOpen(elapsed));
  const notification = $derived(musicNotification(elapsed));
  const playing = $derived(musicNowPlaying(elapsed));
  const playingTrack = $derived(playing ? musicTracks(playing.source)[playing.row] : null);
  const breakPulse = $derived(0.7 - 0.2 * Math.cos((2 * Math.PI * elapsed) / BREAK_PULSE_MS));
  const eventTitle = $derived(t("vaultSetup.film.writeChapter"));
  /** The event while it is dragged out, typed, and saved. */
  const drawnIn = $derived(musicEventDrawnIn(elapsed));
  const creating = $derived(musicEventCreating(elapsed));
  const drawnMinutes = $derived(
    Math.max(
      CREATE_SNAP_MINUTES,
      Math.round((musicEventDrawn(elapsed) * (MUSIC_EVENT.end - MUSIC_EVENT.start)) / CREATE_SNAP_MINUTES) * CREATE_SNAP_MINUTES,
    ),
  );
  /** Where the press that drags the event out is, in minutes past its start: on the start line, then smoothly down as the event snaps. */
  const pressMinutes = $derived(musicEventDrawn(elapsed) * (MUSIC_EVENT.end - MUSIC_EVENT.start));
  const typedTitle = $derived(musicTypedTitle(elapsed, eventTitle));
  const pressOpacity = $derived(
    filmSpan(elapsed, MUSIC_BEATS.createStart - MUSIC_CREATE_PRESS_MS, MUSIC_BEATS.createStart)
      * (1 - filmSpan(elapsed, MUSIC_BEATS.createEnd, MUSIC_BEATS.createEnd + MUSIC_CREATE_PRESS_MS)),
  );

  const viewMinutes = $derived(((canvasH - TOP_BAR_H - HEADER_H) / HOUR_PX) * 60);
  const hours = $derived(
    Array.from({ length: Math.ceil(viewMinutes / 60) + 1 }, (_, index) => Math.ceil(VIEW_START / 60) + index)
      .filter((hour) => hour * 60 > VIEW_START && hour * 60 < VIEW_START + viewMinutes),
  );
  const scrollThumb = $derived.by(() => {
    const visibleHeight = canvasH - TOP_BAR_H - HEADER_H;
    return calendarScrollbarThumb(
      { scrollTop: (VIEW_START / 60) * HOUR_PX, scrollHeight: 24 * HOUR_PX, clientHeight: visibleHeight },
      visibleHeight,
    );
  });
  const dayFormat = $derived(dayNameFormatForWidth(canvasW - GUTTER_W));
  /** The event panel's anchor: the event, in the day's only column. */
  const panelAnchor = $derived.by<EventPanelAnchor>(() => {
    const columnLeft = GUTTER_W + DAY_BORDER_PX + EVENT_LEFT;
    const blockWidth = ((canvasW - columnLeft) * (100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT)) / 100;
    return {
      x: columnLeft + blockWidth,
      y: TOP_BAR_H + HEADER_H + y(MUSIC_EVENT.start),
      width: blockWidth,
      height: px(MUSIC_EVENT.end - MUSIC_EVENT.start),
    };
  });

  function playlistName(key: MusicScenePlaylist): string {
    return t(`music.builder.defaultPlaylist.${key}`);
  }

  function playlistIcon(key: MusicScenePlaylist): string {
    return MUSIC_PLAYLISTS.find((entry) => entry.key === key)?.icon ?? "";
  }

  function playlistLookup(key: MusicScenePlaylist): { icon: string; name: string } {
    return { icon: playlistIcon(key), name: playlistName(key) };
  }

  function sourceName(source: MusicSceneSource): string {
    return source === "local" ? t("music.builder.localMusic") : t("music.builder.youtube");
  }

  /** A track's length or position as a player shows it: minutes and seconds, with hours when it has them. */
  function duration(seconds: number): string {
    const whole = Math.floor(seconds);
    const hoursPart = Math.floor(whole / 3600);
    const minutes = Math.floor((whole % 3600) / 60);
    const rest = String(whole % 60).padStart(2, "0");
    return hoursPart > 0 ? `${hoursPart}:${String(minutes).padStart(2, "0")}:${rest}` : `${minutes}:${rest}`;
  }

  /** A track's artwork or a video's thumbnail, blending its two palette colors. */
  function artwork(track: MusicSceneTrack): string {
    const [from, to] = track.artwork;
    return `linear-gradient(135deg, ${getEventColor(from, theme.current).bg}, ${getEventColor(to, theme.current).bg})`;
  }

  function title(key: SceneTitle): string {
    switch (key) {
      case "readSources": return t("vaultSetup.film.readSources");
      case "reviewNotes": return t("vaultSetup.film.reviewNotes");
      case "lunch": return t("vaultSetup.film.lunch");
      case "teamMeeting": return t("vaultSetup.film.teamMeeting");
      case "gym": return t("vaultSetup.film.gym");
    }
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

  function eventColors(color: EventColor | undefined, past: boolean) {
    return preferences.calendarDimPastEvents && past ? getPastEventColor(color, theme.current) : getEventColor(color, theme.current);
  }
</script>

{#snippet touch(opacity: number, left: string, top: string)}
  {#if mobileShell}<WelcomeTouch {opacity} {left} {top} />{/if}
{/snippet}

{#snippet checkbox(checked: boolean)}
  <span class="relative inline-grid size-4 shrink-0 place-items-center" aria-hidden="true">
    <span
      class={cn(
        "grid size-full place-items-center rounded-sm border",
        checked ? "border-primary bg-primary text-primary-foreground" : "border-foreground/25 text-transparent",
      )}
    >
      {#if checked}<Check size={11} strokeWidth={2.5} />{/if}
    </span>
  </span>
{/snippet}

{#snippet dock(labelled: boolean)}
  {@const unreviewed = reviewProgress.total - reviewProgress.reviewed}
  <nav
    class="flex h-11 shrink-0 items-center justify-center gap-1 px-1.5"
    style:border-top="1px solid color-mix(in srgb, var(--border) 46%, transparent)"
  >
    <span class="relative inline-flex h-8 min-w-8 items-center justify-center gap-1.5 rounded-lg px-2 text-foreground" style:background="var(--secondary)">
      <ListChecks size={15} />
      {#if labelled}<span class="truncate text-[0.65rem] font-medium">{t("music.builder.review")}</span>{/if}
      {#if unreviewed > 0}
        <span class="absolute -right-0.5 -top-0.5 min-w-4 rounded-full bg-secondary px-1 text-center text-[0.52rem] font-semibold leading-4 tabular-nums">{unreviewed}</span>
      {/if}
    </span>
    <span class="relative inline-flex h-8 min-w-8 items-center justify-center rounded-lg px-2 text-foreground"><ListMusic size={15} /></span>
    <span class="relative inline-flex h-8 min-w-8 items-center justify-center rounded-lg px-2 text-foreground"><RadioTower size={15} /></span>
    {#if showSoundscapes}
      <span class="relative inline-flex h-8 min-w-8 items-center justify-center rounded-lg px-2 text-foreground"><AudioLines size={15} /></span>
    {/if}
  </nav>
{/snippet}

{#snippet playlistCard(key: MusicScenePlaylist)}
  {@const checked = musicPlaylistChecked(elapsed, key)}
  {@const bump = musicPlaylistBump(elapsed, key)}
  <div class={cn("relative flex min-w-0 items-center gap-2 rounded-lg px-3 py-2.5", checked ? "bg-primary/10" : "bg-secondary/35")}>
    <span class="flex min-w-0 flex-1 items-center gap-3">
      <span class="grid h-8 w-8 shrink-0 place-items-center text-foreground"><MusicPlaylistIcon icon={playlistIcon(key)} size={16} /></span>
      <span class="min-w-0 flex-1">
        <strong class="block truncate text-xs font-medium">{playlistName(key)}</strong>
        <span
          class="block text-[0.62rem] tabular-nums text-muted-foreground"
          style:color={bump > 0 ? `color-mix(in srgb, var(--primary) ${Math.sin(Math.PI * bump) * BUMP_TINT}%, var(--muted-foreground))` : undefined}
        >{t("music.tracks", musicPlaylistCount(elapsed, key))}</span>
      </span>
    </span>
    <span data-cursor={musicCursorCard(key)} class="grid h-6 w-6 shrink-0 place-items-center">{@render checkbox(checked)}</span>
    {#if key === pickedCard}
      {@render touch(pickTouch, "calc(100% - 1.5rem)", "50%")}
    {/if}
  </div>
{/snippet}

{#snippet eventBlock(
  start: number,
  end: number,
  color: EventColor | undefined,
  label: string,
  past: boolean,
  outline: boolean,
  eventBelow: boolean,
  icons: readonly SceneIcon[],
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
        <div class="absolute right-1 flex items-center gap-0.5" style:top="5px" style:color={`color-mix(in srgb, ${colors.text} 70%, ${colors.bg})`}>
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

{#snippet focusRail(start: number, end: number, focus: readonly FilmSpan[], breaks: readonly FilmSpan[], activeBreak: boolean)}
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
    {#each focus as band (band.start)}
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
        style:opacity={activeBreak && clock >= band.start && clock < band.end ? breakPulse : undefined}
      ></div>
    {/each}
  </div>
{/snippet}

{#snippet ring(size: number, trackClass: string, progressClass: string)}
  <PomodoroProgressRing
    active={focusing}
    remainingSeconds={phaseSeconds.remaining}
    totalSeconds={phaseSeconds.total}
    paused={false}
    pausedPulseAmount={null}
    {size}
    {trackClass}
    {progressClass}
  />
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5">
  <div
    class="relative flex flex-col overflow-hidden rounded-2xl bg-(--cal-bg) text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    {#if mobileShell}
      <div class="flex shrink-0 items-center justify-end bg-(--cal-bg) px-1.5" style:height={`${TOP_BAR_H}px`}>
        <span class={cn("relative grid size-8 place-items-center rounded-full text-muted-foreground", focusing && "bg-accent/70 text-foreground")}>
          {@render ring(20, "stroke-muted-foreground/35", "text-foreground/76 stroke-foreground/76")}
        </span>
      </div>
    {:else}
      <!-- The title bar's Focus ring and Music, with Music open while the builder shows. -->
      <div class="flex shrink-0 items-center justify-end gap-0.5 bg-sidebar" style:height={`${TOP_BAR_H}px`} style:padding-right={`${TITLE_BAR_END_PX}px`}>
        <span class="relative grid size-7 place-items-center rounded-lg">
          {@render ring(
            TITLE_ICON_SIZE + 0.5,
            "stroke-foreground/20 dark:stroke-white/20",
            "text-foreground/68 dark:text-white/76 stroke-foreground/68 dark:stroke-white/76",
          )}
        </span>
        <WelcomeTitleIcon icon={Music} open={builderShown > 0.5} />
      </div>
    {/if}

    <div class="relative min-h-0 flex-1">
      <!-- The playlist builder's review. -->
      {#if builderShown > 0}
        <div
          class={cn("absolute inset-0 min-h-0 bg-(--cal-bg)", withTree ? "grid" : "flex flex-col")}
          style:grid-template-columns={withTree ? "minmax(14rem, 0.72fr) minmax(22rem, 2fr)" : undefined}
          style:opacity={builderShown < 1 ? builderShown : undefined}
        >
          {#if withTree}
            <aside
              class="flex min-h-0 min-w-0 flex-col bg-background/20"
              style:border-right="1px solid color-mix(in srgb, var(--border) 46%, transparent)"
            >
              <div class="shrink-0 p-2">
                <div class="field flex h-8 items-center gap-2 px-2.5">
                  <Search size={13} class="shrink-0 text-muted-foreground" />
                  <span class="truncate text-[0.7rem] text-muted-foreground">{t("music.builder.searchReviewTree")}</span>
                </div>
              </div>
              <div class="min-h-0 flex-1 overflow-hidden px-1.5 pb-2">
                {#each SOURCES as source (source)}
                  {@const tracks = musicTracks(source)}
                  <div class="relative flex h-8 min-w-0 items-center rounded-lg">
                    <span class="grid h-8 w-7 shrink-0 place-items-center text-muted-foreground"><ChevronRight size={14} class="rotate-90" /></span>
                    <span class="grid h-full w-7 shrink-0 place-items-center">{@render checkbox(false)}</span>
                    <span class="flex min-w-0 flex-1 items-center pr-2">
                      <span class="min-w-0 flex-1 truncate text-[0.7rem] font-medium">{sourceName(source)}</span>
                      <span class="text-[0.6rem] tabular-nums text-muted-foreground">{tracks.length}</span>
                    </span>
                  </div>
                  {#each tracks as track, row (track.title)}
                    {@const active = reviewItem.source === source && reviewItem.row === row}
                    <div class={cn("relative flex h-8 min-w-0 items-center rounded-lg", active && "bg-primary/10 text-foreground")} style:padding-left="2.5rem">
                      <span class="grid h-full w-7 shrink-0 place-items-center">{@render checkbox(false)}</span>
                      <span class="flex min-w-0 flex-1 items-center pr-2">
                        <span class="min-w-0 flex-1 truncate text-[0.68rem]">{track.title}</span>
                        {#if musicItemReviewed(elapsed, source, row)}
                          <span class="ml-2 grid h-5 w-5 shrink-0 place-items-center text-primary"><Check size={12} strokeWidth={2.5} /></span>
                        {/if}
                      </span>
                    </div>
                  {/each}
                {/each}
              </div>
              {@render dock(false)}
            </aside>
          {/if}

          <div class="flex min-h-0 min-w-0 flex-1 flex-col bg-background/30">
            <div class="relative min-h-0 flex-1 overflow-hidden" bind:clientHeight={reviewViewportHeight}>
              <div style:transform={reviewScroll > 0 ? `translateY(${-reviewScroll}px)` : undefined}>
                <div bind:offsetHeight={reviewLeadHeight}>
                  <section class={withTree ? "px-4 pb-3 pt-2" : "p-2.5"}>
                    <div class="flex items-center justify-between gap-1.5">
                      {#if !withTree}
                        <span class="grid h-8 w-8 shrink-0 place-items-center rounded-lg text-foreground"><PanelLeft size={14} /></span>
                      {/if}
                      <span class="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full bg-secondary px-2.5 text-[0.7rem]">
                        <ChevronLeft size={14} />{withTree ? t("music.backToPlayer") : t("music.returnToPlayerShort")}
                      </span>
                      <span class="min-w-0 flex-1 truncate text-center text-[0.68rem] font-medium text-muted-foreground">
                        {t("music.builder.reviewProgress", reviewProgress.reviewed, reviewProgress.total)}
                      </span>
                      <span class="flex shrink-0 flex-nowrap items-center justify-end gap-x-0.5">
                        <span class="inline-flex h-8 items-center gap-1 rounded-md px-1.5 text-[0.65rem] text-foreground"><PlayIcon size={13} />{t("music.builder.reviewAutoplayOn")}</span>
                        <span class="inline-flex h-8 items-center gap-1 rounded-md px-1.5 text-[0.65rem] text-foreground"><X size={13} />{t("music.builder.ignore")}</span>
                      </span>
                    </div>

                    <div class={cn("mt-4 flex min-w-0 gap-4", tightReview ? "items-start" : "items-center")} style:opacity={reviewIn < 1 ? reviewIn : undefined}>
                      <div
                        data-cursor="music-review-media"
                        class="relative shrink-0 overflow-hidden rounded-xl"
                        style:height={tightReview ? "5rem" : "7rem"}
                        style:width={reviewItem.source === "youtube" ? (tightReview ? "6.6667rem" : "9.3333rem") : (tightReview ? "5rem" : "7rem")}
                        style:background={artwork(reviewTrack)}
                      ></div>
                      <div class="min-w-0 flex-1">
                        <h2 class="truncate text-base font-semibold">{reviewTrack.title}</h2>
                        <p class="mt-0.5 truncate text-xs text-muted-foreground">{reviewTrack.artist}</p>
                        <div class="mt-4 flex items-center gap-3">
                          <div class="min-w-0 flex-1 text-[0.68rem] tabular-nums text-muted-foreground">
                            <div class="relative h-3.5">
                              <div class="absolute inset-x-1.75 top-1/2 h-1 -translate-y-1/2 rounded-full" style:background="color-mix(in srgb, var(--foreground) 22%, transparent)">
                                <div class="h-full rounded-full bg-primary" style:width={`${reviewPlayed * 100}%`}></div>
                              </div>
                              <span
                                class="absolute top-0 size-3.5 rounded-full bg-card"
                                style:left={`calc((100% - 0.875rem) * ${reviewPlayed})`}
                                style:border="1px solid color-mix(in srgb, var(--foreground) 18%, transparent)"
                              ></span>
                            </div>
                            <div class="mt-1 flex justify-between">
                              <span>{duration(reviewPlayed * reviewTrack.seconds)}</span>
                              <span>{duration(reviewTrack.seconds)}</span>
                            </div>
                          </div>
                          <span class={cn("grid shrink-0 place-items-center rounded-full bg-primary text-primary-foreground", tightReview ? "size-8" : "size-10")}>
                            <PauseIcon size={18} fill="currentColor" />
                          </span>
                        </div>
                      </div>
                    </div>
                  </section>

                  <div class="p-3">
                    <h2 class="text-sm font-semibold">{t("music.builder.classifyPlaylists")}</h2>
                    <div class="mt-2 flex flex-wrap items-center gap-2">
                      <span class="inline-flex h-8 items-center gap-1.5 rounded-full bg-primary px-3 text-xs font-medium text-primary-foreground"><ListPlus size={14} />{t("music.builder.newPlaylist")}</span>
                      <span class="inline-flex h-8 items-center gap-1.5 rounded-full bg-secondary px-3 text-xs font-medium text-foreground"><Pencil size={13} />{t("music.builder.managePlaylists")}</span>
                    </div>
                  </div>
                  <div class={cn("playlist-grid grid gap-2 px-3", withTree && "two-columns")}>
                    {#each LEAD_CARDS as key (key)}{@render playlistCard(key)}{/each}
                  </div>
                </div>
                <div class={cn("playlist-grid mt-2 grid gap-2 px-3 pb-3", withTree && "two-columns")}>
                  {#each MORE_CARDS as key (key)}{@render playlistCard(key)}{/each}
                </div>
              </div>
            </div>

            <div class="grid shrink-0 grid-cols-2 gap-3 p-3">
              <span class="review-action border border-border/70 bg-background text-foreground">{t("music.builder.skipTrack")}</span>
              <span data-cursor="music-save" class={cn("review-action relative", anyChecked ? "bg-primary text-primary-foreground" : "bg-secondary text-muted-foreground")}>
                <Check size={14} />{t("music.builder.saveAndContinue")}<ChevronRight size={14} />
                {@render touch(saveTouch, "50%", "50%")}
              </span>
            </div>
            {#if !withTree}{@render dock(canvasW >= DOCK_LABEL_MIN_W)}{/if}
          </div>
        </div>
      {/if}

      <!-- The Calendar's day, as the Calendar film draws it, with the event the panel gives its project's music. -->
      {#if calendarShown > 0}
        <div class="absolute inset-0 flex flex-col bg-(--cal-bg)" style:opacity={calendarShown < 1 ? calendarShown : undefined}>
          <div class="flex shrink-0 border-b border-sidebar" style:height={`${HEADER_H}px`}>
            <span class="flex shrink-0 items-center justify-center text-[0.866667rem]" style:width={`${GUTTER_W}px`}>{timezoneLabel}</span>
            <div class="flex min-w-0 flex-1 items-center justify-center overflow-hidden whitespace-nowrap">
              <span class="text-[0.866667rem]">
                {#if dayFormat !== "none"}{formatDayName(sceneDate, dayFormat, localization.locale)}&nbsp;{/if}{sceneDate.getDate()}
              </span>
            </div>
          </div>

          <div class="relative flex min-h-0 flex-1 overflow-hidden">
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

            <div class="relative min-w-0 flex-1" style:border-left="1px solid var(--cal-gridline)">
              <div class="pointer-events-none absolute inset-0 z-1">
                {#each hours as hour (hour)}
                  <div class="absolute inset-x-0 h-0 border-b border-(--cal-gridline)" style:top={`${y(hour * 60)}px`}></div>
                {/each}
              </div>

              {#if rail}
                {@render focusRail(rail.start, rail.end, recorded, rail.breaks, onBreak)}
              {/if}

              <div class="absolute inset-y-0 right-0" style:left={`${EVENT_LEFT}px`}>
                {#each DAY_EVENTS as event (event.start)}
                  {@render eventBlock(
                    event.start,
                    event.end,
                    event.color,
                    title(event.title),
                    event.end <= clock,
                    false,
                    DAY_EVENTS.some((other) => other.start === event.end),
                    event.icons,
                  )}
                {/each}
                {#if creating}
                  {@render eventBlock(
                    MUSIC_EVENT.start,
                    MUSIC_EVENT.start + drawnMinutes,
                    picked ? THESIS : undefined,
                    typedTitle || t("calendar.event.noTitle"),
                    false,
                    true,
                    false,
                    picked ? ["music"] : [],
                  )}
                {:else if drawnIn}
                  {@render eventBlock(MUSIC_EVENT.start, MUSIC_EVENT.end, THESIS, eventTitle, false, false, false, ["music"])}
                {/if}

                <!-- The drawn event's resize knobs, as a phone shows them from the start of the drawing until Save. -->
                {#if mobileShell && creating}
                  <div
                    class="pointer-events-none absolute left-0"
                    style:width={`${100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT}%`}
                    style:top={`${y(MUSIC_EVENT.start)}px`}
                    style:height={`${px(drawnMinutes)}px`}
                    style:z-index="11"
                    style:--knob-color={getEventColor(picked ? THESIS : undefined, theme.current).bg}
                  >
                    <div class="resize-knob absolute left-0 top-0"></div>
                    <div class="resize-knob absolute bottom-0 right-0"></div>
                  </div>
                {/if}

                <!-- The press that draws the event, following its growing edge, or on desktop the spot the pointer drags. -->
                {#if !mobileShell}
                  <span
                    data-cursor="music-create-edge"
                    class="pointer-events-none absolute left-3/5 size-px"
                    style:top={`${y(MUSIC_EVENT.start + pressMinutes)}px`}
                  ></span>
                {:else}
                  <WelcomeTouch opacity={pressOpacity} left="60%" top={`${y(MUSIC_EVENT.start + pressMinutes)}px`} />
                {/if}
              </div>

              <div class="pointer-events-none absolute inset-x-0" style:top={`${y(clock)}px`} style:z-index="46">
                <div class="absolute size-2.5 shrink-0 rounded-full" style:background-color="var(--cal-current-time)" style:left="-5px" style:top="-5px"></div>
                <div class="absolute inset-x-0 h-[2.3px]" style:background-color="var(--cal-current-time)" style:top="-1.15px"></div>
              </div>
            </div>
          </div>
        </div>
      {/if}
    </div>

    <!-- The app's notification as the system shows it, saying which track the phase's playlist autoplays. -->
    {#if notification > 0 && playing && playingTrack}
      <WelcomeFilmNotification
        kind={notificationKind}
        shown={notification}
        title={eventTitle}
        body={t("vaultSetup.music.playingNotice", playingTrack.title, playlistName(playing.playlist))}
        androidIcon="music"
      />
    {/if}

    <!-- The event panel, where the Thesis project gives the event its music for each phase. -->
    {#if panelOpen}
      <WelcomeFilmEventPanel
        frame={musicEventPanelFrame(elapsed, eventTitle, playlistLookup)}
        canvasWidth={canvasW}
        canvasHeight={canvasH}
        titleBarHeight={TOP_BAR_H}
        anchor={panelAnchor}
        date={sceneDate}
        start={MUSIC_EVENT.start}
        end={MUSIC_EVENT.end}
        projectColor={THESIS}
        title={eventTitle}
        hovered={mobileShell ? null : welcomeCursorHovered(MUSIC_CURSOR_SCRIPT, elapsed)}
      />
    {/if}

    {#if !mobileShell}
      <WelcomeCursor script={MUSIC_CURSOR_SCRIPT} {elapsed} width={canvasW} />
    {/if}
  </div>
</div>

<style>
  /* The review's playlist cards and actions, as the review draws them. */
  .playlist-grid {
    grid-template-columns: repeat(auto-fit, minmax(min(12rem, 100%), 1fr));
  }

  /* Beside the tree, the scene's smaller type fits two cards across the review, as a full size review does. */
  .playlist-grid.two-columns {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  /* The phone's resize knobs on an event being created, as the Calendar draws them. */
  .resize-knob {
    width: 2.75rem;
    height: 2.75rem;
    margin: -1.375rem -0.5rem;
  }

  .resize-knob::after {
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

  .review-action {
    display: inline-flex;
    min-height: 2.25rem;
    align-items: center;
    justify-content: center;
    gap: 0.375rem;
    border-radius: 0.5rem;
    padding: 0 0.5rem;
    font-size: calc(0.72rem * var(--type-scale));
    font-weight: 600;
  }
</style>
