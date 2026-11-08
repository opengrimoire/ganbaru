<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 300;
  const MAX_CANVAS_W = 760;
  const MIN_CANVAS_H = 360;
  const MAX_CANVAS_H = 440;

  export const MUSIC_SCENE_MIN_WIDTH = MIN_CANVAS_W;
  export const MUSIC_SCENE_MAX_WIDTH = MAX_CANVAS_W;
  export const MUSIC_SCENE_MIN_HEIGHT = MIN_CANVAS_H;
  export const MUSIC_SCENE_MAX_HEIGHT = MAX_CANVAS_H;
</script>

<script lang="ts">
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import MonitorPlay from "@lucide/svelte/icons/monitor-play";
  import Pause from "@lucide/svelte/icons/pause";
  import Plus from "@lucide/svelte/icons/plus";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import SkipForward from "@lucide/svelte/icons/skip-forward";
  import MusicPlaylistIcon from "$lib/components/music/builder/MusicPlaylistIcon.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import { formatTimeRange, getEventColor } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM } from "$lib/platform";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import type { FilmSpan } from "$lib/vault/welcome-film";
  import {
    MUSIC_DAY_VIEW,
    MUSIC_EVENT,
    MUSIC_LOCAL_TRACKS,
    MUSIC_PLAYLISTS,
    MUSIC_PROJECT_PLAYLISTS,
    MUSIC_RAIL,
    MUSIC_YOUTUBE_VIDEOS,
    musicAddMenu,
    musicBuilderShown,
    musicCalendarShown,
    musicClock,
    musicNowPlaying,
    musicPhase,
    musicPlaylistBump,
    musicPlaylistCount,
    musicProjectChosen,
    musicProjectPicker,
    musicRailRecorded,
    musicRowFill,
    musicSource,
    musicSourceIn,
    musicTouches,
    musicTrackAdded,
    type MusicScenePlaylist,
    type MusicSceneSource,
    type MusicSceneTrack,
  } from "$lib/vault/welcome-music-scene";

  /**
   * One frame of the welcome scene on Music: the playlist builder adds a local track and a YouTube video to built-in
   * playlists, then a Calendar event picks the Thesis project, whose playlists fill the event's music for each phase,
   * and the player follows the event's Focus from the work playlist to the break playlist and back. A wide canvas
   * keeps the builder's playlists beside its sources; a narrow one shows them as a row of counts above. Every style is
   * a function of `elapsed`, with no CSS transitions.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const { t } = getLocalization();
  const preferences = getPreferences();
  const theme = getTheme();

  /** The window's frame, matching the other welcome scenes. */
  const BEZEL = 6;
  /** From this canvas width, the builder has room for its playlists beside its sources. */
  const SIDEBAR_MIN_W = 520;
  /** Apple's windows keep their controls on the left; other systems keep them on the right. */
  const CONTROLS_LEADING = BUILD_PLATFORM === "macos" || BUILD_PLATFORM === "ios";
  /** The event's and the Thesis project's palette slot, the Calendar scene's Thesis color, and the Assignments project's. */
  const THESIS_COLOR = 7;
  const ASSIGNMENTS_COLOR = 9;
  /** How far a menu drops as it opens, and how much a playlist's count grows at the peak of its bump. */
  const MENU_DROP = 4;
  const BUMP_GROWTH = 0.35;
  /** How far the player rises as it appears. */
  const PLAYER_RISE = 12;
  /** The Focus rail above the event, as thick as the Calendar's and as far from the event, in pixels. */
  const RAIL_INSET = 4;
  const RAIL_THICKNESS = 6;
  const RAIL_GAP = 4;
  const EVENT_INSET = RAIL_INSET + RAIL_THICKNESS + RAIL_GAP;
  const VIEW_MINUTES = MUSIC_DAY_VIEW.end - MUSIC_DAY_VIEW.start;

  const PLAYLIST_ICONS: Readonly<Record<MusicScenePlaylist, string>> = Object.fromEntries(
    MUSIC_PLAYLISTS.map((entry) => [entry.key, entry.icon]),
  ) as Record<MusicScenePlaylist, string>;
  const PICKER_PROJECTS = [
    { name: () => t("vaultSetup.film.thesisProject"), icon: "lucide:book-open", color: THESIS_COLOR },
    { name: () => t("vaultSetup.film.assignmentsProject"), icon: "lucide:folder", color: ASSIGNMENTS_COLOR },
  ] as const;

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);
  const wide = $derived(canvasW >= SIDEBAR_MIN_W);
  const builderShown = $derived(musicBuilderShown(elapsed));
  const calendarShown = $derived(musicCalendarShown(elapsed));
  const source = $derived(musicSource(elapsed));
  const tracks = $derived(source === "local" ? MUSIC_LOCAL_TRACKS : MUSIC_YOUTUBE_VIDEOS);
  const menu = $derived(musicAddMenu(elapsed));
  const touches = $derived(musicTouches(elapsed));
  const picker = $derived(musicProjectPicker(elapsed));
  const chosen = $derived(musicProjectChosen(elapsed));
  const clock = $derived(musicClock(elapsed));
  const phase = $derived(musicPhase(elapsed));
  const recorded = $derived(musicRailRecorded(elapsed));
  const playing = $derived(musicNowPlaying(elapsed));
  const eventColors = $derived(getEventColor(THESIS_COLOR, theme.current));

  /** A playlist's localized name. */
  function playlistName(key: MusicScenePlaylist): string {
    return t(`music.builder.defaultPlaylist.${key}`);
  }

  /** A track's length as a player shows it: minutes and seconds, with hours when it has them. */
  function duration(seconds: number): string {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const rest = String(seconds % 60).padStart(2, "0");
    return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${rest}` : `${minutes}:${rest}`;
  }

  /** A track's artwork or thumbnail, blending its two palette colors. */
  function artwork(track: MusicSceneTrack): string {
    const [from, to] = track.artwork;
    return `linear-gradient(135deg, ${getEventColor(from, theme.current).bg}, ${getEventColor(to, theme.current).bg})`;
  }

  /** The touch on a track's add button in the open root. */
  function addTouch(row: number): number {
    if (source === "local") return row === 0 ? touches.localAdd : 0;
    return row === 1 ? touches.youtubeAdd : 0;
  }

  /** Where a minute of the day sits along the strip, in percent. */
  function at(minute: number): number {
    return ((minute - MUSIC_DAY_VIEW.start) / VIEW_MINUTES) * 100;
  }

  /** A minute of the day as the Calendar's time labels take it. */
  function clockLabel(minuteOfDay: number): string {
    return `${String(Math.floor(minuteOfDay / 60)).padStart(2, "0")}:${String(Math.round(minuteOfDay % 60)).padStart(2, "0")}`;
  }
</script>

{#snippet controls()}
  <span class="flex shrink-0 gap-1" aria-hidden="true">
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
  </span>
{/snippet}

{#snippet touch(shown: number)}
  {#if shown > 0}
    <span
      class="pointer-events-none absolute top-1/2 left-1/2 size-5 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35"
      style:opacity={shown}
    ></span>
  {/if}
{/snippet}

{#snippet sourceIcon(kind: MusicSceneSource, size: number)}
  {#if kind === "local"}
    <HardDrive {size} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
  {:else}
    <MonitorPlay {size} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
  {/if}
{/snippet}

{#snippet cover(track: MusicSceneTrack, kind: MusicSceneSource, heightClass: string)}
  <!-- Local tracks show square artwork; YouTube videos show a wide thumbnail with the video's length on it. -->
  <span
    class="relative shrink-0 overflow-hidden rounded {heightClass} {kind === 'local' ? 'aspect-square' : 'aspect-video'}"
    style:background-image={artwork(track)}
  >
    {#if kind === "youtube"}
      <span class="absolute right-0.5 bottom-0.5 rounded-sm bg-black/70 px-0.5 text-[7px] leading-tight text-white tabular-nums">
        {duration(track.seconds)}
      </span>
    {/if}
  </span>
{/snippet}

{#snippet railBand(band: FilmSpan, color: string)}
  <div
    class="absolute inset-y-0"
    style:left={`${((band.start - MUSIC_RAIL.start) / (MUSIC_RAIL.end - MUSIC_RAIL.start)) * 100}%`}
    style:width={`${((band.end - band.start) / (MUSIC_RAIL.end - MUSIC_RAIL.start)) * 100}%`}
    style:background-color={color}
  ></div>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5" style:width={`${width}px`} style:height={`${height}px`}>
  <div
    class="relative flex flex-col overflow-hidden rounded-2xl bg-background text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    <!-- The window's bar, naming the open area, with the window's controls on the system's side. -->
    <div class="flex h-8 shrink-0 items-center gap-2 border-b border-border px-3">
      {#if CONTROLS_LEADING}{@render controls()}{/if}
      <span class="relative flex min-w-0 flex-1 justify-center text-[10px] text-muted-foreground">
        <span class="flex items-center gap-1.5" style:opacity={1 - calendarShown}>
          <Music size={11} strokeWidth={1.8} aria-hidden="true" />
          {t("titleBar.control.music")}
        </span>
        <span class="absolute inset-0 flex items-center justify-center gap-1.5" style:opacity={calendarShown}>
          <CalendarIcon size={11} strokeWidth={1.8} aria-hidden="true" />
          {t("titleBar.tab.calendar")}
        </span>
      </span>
      {#if !CONTROLS_LEADING}{@render controls()}{/if}
    </div>

    <div class="relative min-h-0 flex-1">
      {#if builderShown > 0}
        <!-- The playlist builder: its playlists, and the sources with the Local music and YouTube roots. -->
        <div class="absolute inset-0 flex gap-3 p-3" style:opacity={builderShown}>
          {#if wide}
            <div class="flex w-44 shrink-0 flex-col gap-1 text-[11px]">
              <span class="mb-1 px-1.5 text-[10px] font-semibold text-muted-foreground">{t("music.builder.playlists")}</span>
              {#each MUSIC_PLAYLISTS as entry (entry.key)}
                {@const bump = musicPlaylistBump(elapsed, entry.key)}
                <span class={["flex h-8 items-center gap-2 rounded-md px-1.5", bump > 0 && "bg-foreground/6"]}>
                  <span class="grid size-6 shrink-0 place-items-center rounded-md bg-foreground/6">
                    <MusicPlaylistIcon icon={entry.icon} size={13} />
                  </span>
                  <span class="flex min-w-0 flex-1 flex-col leading-tight">
                    <span class="truncate">{playlistName(entry.key)}</span>
                    <span
                      class="origin-left truncate text-[9.5px]"
                      class:text-muted-foreground={bump === 0}
                      class:text-primary={bump > 0}
                      style:transform={`scale(${1 + BUMP_GROWTH * Math.sin(Math.PI * bump)})`}
                    >
                      {t("music.tracks", musicPlaylistCount(elapsed, entry.key))}
                    </span>
                  </span>
                </span>
              {/each}
            </div>
          {/if}

          <div class="flex min-w-0 flex-1 flex-col gap-2">
            <div class="flex shrink-0 items-center gap-1.5 text-[10.5px]">
              <span class="mr-1 text-[10px] font-semibold text-muted-foreground">{t("music.builder.sources")}</span>
              {#each ["local", "youtube"] as const as kind (kind)}
                <span
                  class={["relative flex h-6 min-w-0 items-center gap-1.5 rounded-md px-2", source === kind ? "bg-foreground/8" : "text-muted-foreground"]}
                >
                  {@render sourceIcon(kind, 12)}
                  <span class="truncate">{kind === "local" ? t("music.builder.localMusic") : t("music.builder.youtube")}</span>
                  {#if kind === "youtube"}{@render touch(touches.youtubeRoot)}{/if}
                </span>
              {/each}
            </div>

            {#if !wide}
              <!-- Without room for the playlists' column, a row of their counts. -->
              <div class="flex shrink-0 gap-1.5 text-[10px]">
                {#each MUSIC_PLAYLISTS as entry (entry.key)}
                  {@const bump = musicPlaylistBump(elapsed, entry.key)}
                  <span class="flex h-6 min-w-0 flex-1 items-center justify-center gap-1 rounded-md bg-foreground/5 px-1.5">
                    <MusicPlaylistIcon icon={entry.icon} size={11} />
                    <span
                      class="tabular-nums"
                      class:text-primary={bump > 0}
                      style:transform={`scale(${1 + BUMP_GROWTH * Math.sin(Math.PI * bump)})`}
                    >
                      {musicPlaylistCount(elapsed, entry.key)}
                    </span>
                  </span>
                {/each}
              </div>
            {/if}

            <div class="flex min-h-0 flex-1 flex-col rounded-lg border border-border" style:opacity={musicSourceIn(elapsed)}>
              {#each tracks as track, row (`${source}-${row}`)}
                {@const added = musicTrackAdded(elapsed, source, row)}
                <div class="relative flex h-10 shrink-0 items-center gap-2.5 border-b border-border/60 px-2.5 last:border-b-0">
                  {@render cover(track, source, "h-7")}
                  <span class="flex min-w-0 flex-1 flex-col leading-tight">
                    <span class="truncate text-[11px]">{track.title}</span>
                    <span class="truncate text-[9.5px] text-muted-foreground">{track.artist}</span>
                  </span>
                  {#if source === "local"}
                    <span class="shrink-0 text-[9.5px] text-muted-foreground tabular-nums">{duration(track.seconds)}</span>
                  {/if}
                  <span
                    class={["relative grid size-6 shrink-0 place-items-center rounded-md", added ? "text-primary" : "bg-foreground/6"]}
                  >
                    {#if added}
                      <Check size={12} strokeWidth={2} aria-hidden="true" />
                    {:else}
                      <Plus size={12} strokeWidth={1.8} aria-hidden="true" />
                    {/if}
                    {@render touch(addTouch(row))}
                  </span>

                  {#if menu && menu.source === source && menu.row === row}
                    <!-- The playlists menu from the add button, with the chosen playlist ticked. -->
                    <div
                      class="absolute top-full right-1.5 z-10 -mt-1 flex w-40 flex-col gap-px rounded-lg border border-border bg-background p-1 text-[10.5px] shadow-lg"
                      style:opacity={menu.shown}
                      style:transform={`translateY(${(menu.shown - 1) * MENU_DROP}px)`}
                    >
                      {#each MUSIC_PLAYLISTS as entry (entry.key)}
                        {@const target = entry.key === menu.playlist}
                        <span class={["relative flex h-6 items-center gap-2 rounded-md px-1.5", target && menu.picked && "bg-foreground/6"]}>
                          <MusicPlaylistIcon icon={entry.icon} size={12} />
                          <span class="min-w-0 flex-1 truncate">{playlistName(entry.key)}</span>
                          {#if target && menu.picked}<Check size={11} strokeWidth={2} class="shrink-0 text-primary" aria-hidden="true" />{/if}
                          {#if target}{@render touch(source === "local" ? touches.localPick : touches.youtubePick)}{/if}
                        </span>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          </div>
        </div>
      {/if}

      {#if calendarShown > 0}
        <!-- The Calendar: a strip of the day with the event, its panel with the project and music, and the player. -->
        <div class="absolute inset-0 flex flex-col items-center gap-2.5 p-3" style:opacity={calendarShown}>
          <div class="relative h-14 w-full max-w-120 shrink-0 rounded-xl border border-border bg-background">
            <div
              class="absolute overflow-hidden"
              style:left={`${at(MUSIC_RAIL.start)}%`}
              style:width={`${at(MUSIC_RAIL.end) - at(MUSIC_RAIL.start)}%`}
              style:top={`${RAIL_INSET}px`}
              style:height={`${RAIL_THICKNESS}px`}
              style:background-color="var(--cal-timeline-rail)"
              style:border-radius="3px"
            >
              {#each recorded as band (band.start)}
                {@render railBand(band, "var(--cal-timeline-focus)")}
              {/each}
              {#each MUSIC_RAIL.breaks as band (band.start)}
                {@render railBand(band, "var(--cal-timeline-break)")}
              {/each}
            </div>
            <div
              class="absolute flex h-9 items-center gap-2 overflow-hidden rounded px-2 leading-tight"
              style:left={`${at(MUSIC_EVENT.start)}%`}
              style:width={`${at(MUSIC_EVENT.end) - at(MUSIC_EVENT.start)}%`}
              style:top={`${EVENT_INSET}px`}
              style:background-color={eventColors.bg}
              style:color={eventColors.text}
            >
              <div class="min-w-0 flex-1">
                <div class="truncate text-[11px] font-medium">{t("vaultSetup.film.writeChapter")}</div>
                <div class="truncate text-[10px]" style:color={`color-mix(in srgb, ${eventColors.text} 80%, ${eventColors.bg})`}>
                  {formatTimeRange(clockLabel(MUSIC_EVENT.start), clockLabel(MUSIC_EVENT.end), preferences.calendarTimeFormat, "compact")}
                </div>
              </div>
              {#if chosen}<Music size={12} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />{/if}
            </div>
            <div class="pointer-events-none absolute inset-y-0" style:left={`${at(clock)}%`}>
              <span class="absolute -top-1 -left-1 size-2 rounded-full" style:background-color="var(--cal-current-time)"></span>
              <span class="absolute inset-y-0 -left-px w-0.5" style:background-color="var(--cal-current-time)"></span>
            </div>
          </div>

          <div class="flex w-full max-w-120 shrink-0 flex-col gap-2 rounded-xl border border-border p-2.5 text-[11px]">
            <!-- The event's project, picked from a list that drops below it. -->
            <div class="relative z-10 flex items-center gap-2">
              <span class="w-22 shrink-0 text-muted-foreground">{t("calendar.eventPanel.project")}</span>
              <span class="relative flex h-6 min-w-0 flex-1 items-center gap-1.5 rounded-md bg-foreground/5 px-2">
                {#if chosen}
                  <span class="flex shrink-0" style:color={eventColors.bg}><ProjectIcon name="lucide:book-open" size={12} strokeWidth={2} /></span>
                  <span class="min-w-0 flex-1 truncate">{t("vaultSetup.film.thesisProject")}</span>
                {:else}
                  <span class="min-w-0 flex-1 truncate text-muted-foreground">{t("calendar.eventPanel.projectPlaceholder")}</span>
                {/if}
                <ChevronDown size={11} strokeWidth={1.8} class="shrink-0 text-muted-foreground" aria-hidden="true" />
                {@render touch(touches.project)}
                {#if picker > 0}
                  <div
                    class="absolute top-full right-0 left-0 mt-1 flex flex-col gap-px rounded-lg border border-border bg-background p-1 shadow-lg"
                    style:opacity={picker}
                    style:transform={`translateY(${(picker - 1) * MENU_DROP}px)`}
                  >
                    <span class="px-1.5 pt-0.5 pb-1 text-[9.5px] font-semibold text-muted-foreground">{t("vaultSetup.film.universityGroup")}</span>
                    {#each PICKER_PROJECTS as project, index (project.icon)}
                      <span class={["relative flex h-6 items-center gap-2 rounded-md px-1.5", index === 0 && chosen && "bg-foreground/6"]}>
                        <span class="flex shrink-0" style:color={getEventColor(project.color, theme.current).bg}>
                          <ProjectIcon name={project.icon} size={12} strokeWidth={2} />
                        </span>
                        <span class="min-w-0 flex-1 truncate">{project.name()}</span>
                        {#if index === 0}{@render touch(touches.projectPick)}{/if}
                      </span>
                    {/each}
                  </div>
                {/if}
              </span>
            </div>

            <!-- The event's music, copied from the project's playlist for each phase; the phase playing now is marked. -->
            <div class="flex items-center gap-1.5 border-t border-border pt-2 font-medium">
              <Music size={12} strokeWidth={1.8} aria-hidden="true" />
              {t("calendar.eventPanel.music")}
            </div>
            {#each MUSIC_PROJECT_PLAYLISTS as row, index (row.phase)}
              {@const fill = musicRowFill(elapsed, index)}
              <div class="flex items-center gap-2">
                <span class="w-22 shrink-0 truncate" class:text-muted-foreground={phase !== row.phase} class:font-medium={phase === row.phase}>
                  {t(`music.assignment.phase.${row.phase}`)}
                </span>
                <span class={["relative flex h-6 min-w-0 flex-1 items-center rounded-md px-2", phase === row.phase ? "bg-foreground/10" : "bg-foreground/5"]}>
                  <span class="absolute inset-x-2 truncate text-muted-foreground" style:opacity={1 - fill}>{t("common.none")}</span>
                  <span class="flex min-w-0 flex-1 items-center gap-1.5" style:opacity={fill} style:transform={`translateX(${(1 - fill) * 6}px)`}>
                    <MusicPlaylistIcon icon={PLAYLIST_ICONS[row.playlist]} size={12} />
                    <span class="min-w-0 flex-1 truncate">{playlistName(row.playlist)}</span>
                  </span>
                  <ChevronDown size={11} strokeWidth={1.8} class="shrink-0 text-muted-foreground" aria-hidden="true" />
                </span>
              </div>
            {/each}
          </div>

          {#if playing}
            {@const list = playing.source === "local" ? MUSIC_LOCAL_TRACKS : MUSIC_YOUTUBE_VIDEOS}
            {@const track = list[playing.index]}
            <!-- The player: the track, where it comes from, the playlist it plays from, and its controls. -->
            <div
              class="flex w-full max-w-120 shrink-0 flex-col gap-1.5 rounded-xl bg-foreground/5 px-2.5 py-2"
              style:opacity={playing.shown}
              style:transform={`translateY(${(1 - playing.shown) * PLAYER_RISE}px)`}
            >
              <div class="flex items-center gap-2.5" style:opacity={playing.swap}>
                {@render cover(track, playing.source, "h-8")}
                <span class="flex min-w-0 flex-1 flex-col leading-tight">
                  <span class="truncate text-[11px] font-medium">{track.title}</span>
                  <span class="flex min-w-0 items-center gap-1 text-[9.5px] text-muted-foreground">
                    {@render sourceIcon(playing.source, 10)}
                    <span class="truncate">{playing.source === "local" ? t("music.builder.local") : t("music.builder.youtube")} · {track.artist}</span>
                  </span>
                </span>
                {#if wide}
                  <span class="flex h-5 shrink-0 items-center gap-1 rounded-full bg-background px-2 text-[9.5px]">
                    <MusicPlaylistIcon icon={PLAYLIST_ICONS[playing.playlist]} size={10} />
                    {playlistName(playing.playlist)}
                  </span>
                {/if}
                <span class="flex shrink-0 items-center gap-1.5 text-muted-foreground">
                  <SkipBack size={11} strokeWidth={1.8} aria-hidden="true" />
                  <Pause size={13} strokeWidth={2} class="text-foreground" aria-hidden="true" />
                  <SkipForward size={11} strokeWidth={1.8} aria-hidden="true" />
                </span>
              </div>
              <span class="h-0.5 overflow-hidden rounded-full bg-foreground/10">
                <span class="block h-full rounded-full bg-foreground/50" style:width={`${playing.progress * playing.swap * 100}%`}></span>
              </span>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
</div>
