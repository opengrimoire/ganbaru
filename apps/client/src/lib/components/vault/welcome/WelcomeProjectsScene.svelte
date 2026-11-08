<script lang="ts" module>
  import { calendarZoomGridMinutesForPercent, calendarZoomHourHeightForPercent } from "$lib/calendar/zoom-levels";

  /** The Calendar zoom the scene's day is drawn at, zoomed in from the default so the short drawn event reads tall. */
  const SCENE_ZOOM_PERCENT = 150;
  const HOUR_PX = calendarZoomHourHeightForPercent(SCENE_ZOOM_PERCENT);
  /** How the event snaps while it is dragged out, in minutes, as the Calendar snaps at the scene's zoom. */
  const CREATE_SNAP_MINUTES = calendarZoomGridMinutesForPercent(SCENE_ZOOM_PERCENT);

  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 360;
  const MAX_CANVAS_W = 760;
  const MIN_CANVAS_H = 470;
  const MAX_CANVAS_H = 540;
  /** The bezel around the canvas, matching the other welcome scenes (`p-1.5`). */
  const BEZEL = 6;

  export const PROJECTS_SCENE_MIN_WIDTH = MIN_CANVAS_W + 2 * BEZEL;
  export const PROJECTS_SCENE_MAX_WIDTH = MAX_CANVAS_W + 2 * BEZEL;
  export const PROJECTS_SCENE_MIN_HEIGHT = MIN_CANVAS_H + 2 * BEZEL;
  export const PROJECTS_SCENE_MAX_HEIGHT = MAX_CANVAS_H + 2 * BEZEL;

  /** The part of the Novel project a scene shows: its event defaults and their event, or its task list. */
  export type WelcomeProjectsScenePart = "defaults" | "tasks";
</script>

<script lang="ts">
  import ArrowUpDown from "@lucide/svelte/icons/arrow-up-down";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Users from "@lucide/svelte/icons/users";
  import Feather from "@lucide/svelte/icons/feather";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Heart from "@lucide/svelte/icons/heart";
  import Layers from "@lucide/svelte/icons/layers";
  import List from "@lucide/svelte/icons/list";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import MailPlus from "@lucide/svelte/icons/mail-plus";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import Plus from "@lucide/svelte/icons/plus";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Save from "@lucide/svelte/icons/save";
  import Search from "@lucide/svelte/icons/search";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import WandSparkles from "@lucide/svelte/icons/wand-sparkles";
  import X from "@lucide/svelte/icons/x";
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import MemberRoleChip from "$lib/components/contacts/MemberRoleChip.svelte";
  import MemberRow from "$lib/components/contacts/MemberRow.svelte";
  import CollectionCell from "$lib/components/collections/CollectionCell.svelte";
  import CollectionPropertyIcon from "$lib/components/collections/CollectionPropertyIcon.svelte";
  import CollectionRow from "$lib/components/collections/CollectionRow.svelte";
  import { collectionRowContentStart } from "$lib/components/collections/row-expansion";
  import MusicPlaylistIcon from "$lib/components/music/builder/MusicPlaylistIcon.svelte";
  import AudioLinesXIcon from "$lib/components/music/soundscape/AudioLinesXIcon.svelte";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import PriorityFlagIcon from "$lib/components/projects/PriorityFlagIcon.svelte";
  import ProjectStatusBadge from "$lib/components/projects/ProjectStatusBadge.svelte";
  import { projectListColumnKind } from "$lib/components/projects/list/property-kinds";
  import WorkspaceBreadcrumbTerminalIcon from "$lib/components/ui/WorkspaceBreadcrumbTerminalIcon.svelte";
  import WelcomeFilmEventPanel from "$lib/components/vault/welcome/WelcomeFilmEventPanel.svelte";
  import WelcomeCursor from "$lib/components/vault/welcome/WelcomeCursor.svelte";
  import { welcomeCursorHovered } from "$lib/vault/welcome-cursor";
  import { CALENDAR_SCROLLBAR_TRACK_PX, calendarScrollbarThumb } from "$lib/calendar/timeline-scroll";
  import { GUTTER_WIDTH_PER_TIMEZONE } from "$lib/calendar/timezone-utils";
  import { FALLBACK_COLOR_INDEX, type EventColor } from "$lib/calendar/types";
  import {
    CALENDAR_EVENT_RIGHT_GUTTER_PCT,
    dayNameFormatForWidth,
    EVENT_COLOR_OPTIONS,
    formatDayName,
    formatTimeRange,
    getEventColor,
    getHourInTimezone,
    getLocalTimezone,
    getPastEventColor,
    getTimezoneInfo,
  } from "$lib/calendar/utils";
  import { contrastRatio } from "$lib/color/math";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { COUNT_PRESET_RHYTHMS, type PomodoroPresetKey } from "$lib/pomodoro/rhythm";
  import { PROJECT_POMODORO_PRESET_ORDER, projectPomodoroSummaryLabel } from "$lib/projects/default-pomodoro";
  import { projectListDateLabel } from "$lib/projects/list/presentation";
  import {
    projectTaskListGridMinWidth,
    projectTaskListGridTemplate,
    projectTaskListGutterWidth,
    projectTaskListLeadingGridTemplate,
  } from "$lib/projects/list/view";
  import { DEFAULT_TASK_LIST_COLUMNS } from "$lib/projects/tasks/list-columns";
  import type { ProjectCoreTaskListColumn, ProjectStatus, ProjectStatusCategory } from "$lib/projects/types";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { isThemeCalendarDark, resolveCalendarTokens } from "$lib/themes";
  import { cn } from "$lib/utils";
  import type { EventPanelAnchor } from "$lib/utils/responsive";
  import { pickSelectPopoverGeometry, type SelectPopoverRect } from "$lib/utils/select-popover-position";
  import type { WelcomeEventPanelPicker } from "$lib/vault/welcome-event-panel";
  import { filmSpan, filmWorkday, type FilmSpan } from "$lib/vault/welcome-film";
  import { MUSIC_PLAYLISTS, type MusicScenePlaylist } from "$lib/vault/welcome-music-scene";
  import {
    PROJECTS_BEATS,
    PROJECTS_CLOCK,
    PROJECTS_COLOR,
    PROJECTS_CREATE_PRESS_MS,
    PROJECTS_CURSOR,
    PROJECTS_DEFAULT_MINUTES,
    PROJECTS_EVENT,
    PROJECTS_PHASES,
    PROJECTS_PLAYLISTS,
    PROJECTS_PRESET,
    PROJECTS_PRIORITY_COLORS,
    PROJECTS_PRIORITY_ORDER,
    PROJECTS_SETTINGS_TARGETS,
    PROJECTS_STATUS_COLORS,
    PROJECTS_STATUS_ORDER,
    PROJECTS_TASKS,
    projectsCalendarShown,
    projectsDefaultColor,
    projectsDefaultMinutes,
    projectsDefaultPreset,
    projectsEventCreating,
    projectsEventDrawn,
    projectsEventDrawnIn,
    projectsEventMinutes,
    projectsEventName,
    projectsEventNameCaretOn,
    projectsEventNameFocused,
    projectsEventPanelFrame,
    projectsEventPicked,
    projectsEventRail,
    projectsListShown,
    projectsPanelOpen,
    projectsOptionHovered,
    projectsPlaylist,
    projectsSettingsDirty,
    projectsSettingsOpen,
    projectsSettingsPicker,
    projectsSettingsScroll,
    projectsSettingsShown,
    projectsTouches,
    type ProjectsScenePhase,
    type ProjectsScenePriority,
    type ProjectsSceneStatus,
    type ProjectsSceneTaskEntry,
    type ProjectsSettingsTarget,
  } from "$lib/vault/welcome-projects-scene";
  import {
    TASKS_CURSOR,
    TASKS_NEW_PRIORITY,
    TASKS_NEW_STATUS,
    TASKS_NEW_TASK,
    tasksAddActive,
    tasksAdded,
    tasksCaretOn,
    tasksCellHighlighted,
    tasksCellMenu,
    tasksDraft,
    tasksListScroll,
    tasksNewPriority,
    tasksNewStatus,
    tasksOptionHovered,
    tasksTouches,
    type TasksSceneCell,
  } from "$lib/vault/welcome-tasks-scene";

  /**
   * One frame of a welcome scene on the Novel project, drawn with the app's own surfaces. With `part` on `defaults`,
   * project settings open from the header in the List view on Event defaults, where the event name is typed, a color, a
   * duration, and a Pomodoro are picked, a playlist is picked for each phase, and the settings are saved and closed, each
   * control scrolled into view before its tap. In the Calendar, an event is then dragged out, and picking Novel in its
   * panel gives it every default just saved. With `part` on `tasks`, a task is typed into the list's add row and added,
   * and its status and priority are changed from their cells' menus. Every style is a function of `elapsed`, with no CSS
   * transitions.
   */
  let { part, elapsed, width, height }: { part: WelcomeProjectsScenePart; elapsed: number; width: number; height: number } = $props();

  /** The time into each part's timeline: the part not shown stays at its start, where none of its moments has happened. */
  const defaultsElapsed = $derived(part === "defaults" ? elapsed : 0);
  const tasksElapsed = $derived(part === "tasks" ? elapsed : 0);

  const localization = getLocalization();
  const { t } = localization;
  const theme = getTheme();
  const preferences = getPreferences();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const showSoundscapes = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.soundscapes");
  const idleDetectionAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "pomodoro.native-idle-detection");

  /** The title bar on desktop, or the phone's top bar, tall enough for its context switcher. */
  const TOP_BAR_H = mobileShell ? 48 : 36;
  /** The project header, as `--toolbar-h` sizes it on each shell. */
  const HEADER_H = mobileShell ? 56 : 40;
  /** The title bar's icons and its end padding, as the title bar draws them. */
  const TITLE_ICON_SIZE = 14;
  const TITLE_ICON_STROKE_WIDTH = 1.5;
  const TITLE_BAR_END_PX = 6;
  const TITLE_ICON_COLOR_CLASS = "text-foreground/68 dark:text-white/76";
  /** The project's header icons, as the breadcrumb and the view toolbar draw them. */
  const IDENTITY_ICON_SIZE = 14;
  const IDENTITY_ICON_STROKE_WIDTH = 1.5;
  const TOOL_ICON_SIZE = 14;
  const TOOL_ICON_STROKE_WIDTH = 1.75;
  const MOBILE_ICON_SIZE = 16;
  /** The avatar size of the members section's rows. */
  const MEMBER_AVATAR_SIZE = 26;
  /** A phone header icon button, as the project header draws it. */
  const MOBILE_CHROME_BUTTON_CLASS = "mobile-chrome-button flex w-(--mobile-chrome-column) shrink-0 items-center justify-center text-foreground";

  /** Project settings as a toolbar popover places them, from `projectToolbarPanelGeometry`. */
  const PANEL_WIDTH = 430;
  const PANEL_MAX_HEIGHT = 680;
  const PANEL_EDGE = 8;
  const PANEL_GAP = 6;
  const PANEL_COMPACT_W = 460;
  /** At or below this width the settings rows stack their labels above their controls, as the panel does. */
  const STACKED_ROWS_W = 480;
  /** Where the Defaults heading rests once the settings scroll to it, below the top of their viewport. */
  const DEFAULTS_SCROLL_PAD = 4;
  /** Room kept above or below a settings control once the settings scroll it into view. */
  const CONTROL_SCROLL_PAD = 8;
  /** Room kept so a popover that just fits by measure still fits after rounding. */
  const FIT_SLACK_PX = 1;
  /** The project header's end padding (`px-3`), so the settings button's right edge is known. */
  const HEADER_END_PX = 12;

  /** A cell menu's edge margin and its gap from the cell, as the list's cell menus place themselves. */
  const CELL_MENU_MARGIN = 8;
  const CELL_MENU_GAP = 6;
  /** Room kept beside a cell once the list scrolls it into view. */
  const CELL_SCROLL_PAD = 12;
  /** The color palette's edge margin and its padding, whose half is its gap from the trigger, as the picker places it. */
  const PALETTE_EDGE = 8;
  const PALETTE_PADDING_REM = 0.625;
  /** The playlist picker's width and height caps, as the playlist select sizes it. */
  const PLAYLIST_POPOVER_W = 320;
  const PLAYLIST_POPOVER_MAX_H = 520;

  /** The Calendar's day header and time gutter. */
  const CAL_HEADER_H = 40;
  const GUTTER_W = GUTTER_WIDTH_PER_TIMEZONE;
  /** The Calendar's Focus rail, and the event area it keeps beside it on every event. */
  const RAIL_LEFT = 2;
  const RAIL_W = 6;
  const EVENT_LEFT = RAIL_W + 4;
  const DAY_BORDER_PX = 1;
  const EVENT_GAP_PX = 2;
  const EVENT_TIME_MIN_PX = 32;
  const EVENT_ICONS_MIN_PX = 17;
  /** Where the visible part of the day starts, as if the grid were scrolled so the event sits near the top. */
  const VIEW_START = 9 * 60 + 20;

  /** Palette slots, so the drawing follows the active theme's event colors, as the Calendar film picks them. */
  const BLUE = 18;
  const LAVENDER = 20;
  const EATING = 13;

  type SceneTitle = "readSources" | "lunch" | "teamMeeting";
  type SceneIcon = "music" | "repeat" | "meeting";
  interface SceneEvent {
    start: number;
    end: number;
    color: EventColor;
    title: SceneTitle;
    icons: readonly SceneIcon[];
  }

  const ICON_ORDER: readonly SceneIcon[] = ["music", "repeat", "meeting"];
  /** The rest of the day around the event, clear of the two hours the event grows to. */
  const DAY_EVENTS: readonly SceneEvent[] = [
    { start: 8 * 60 + 30, end: 9 * 60 + 30, color: BLUE, title: "readSources", icons: [] },
    { start: 13 * 60, end: 14 * 60, color: EATING, title: "lunch", icons: ["repeat"] },
    { start: 14 * 60, end: 15 * 60, color: LAVENDER, title: "teamMeeting", icons: ["repeat", "meeting"] },
  ];

  /** Each status a new project seeds, with its category. */
  const STATUS_CATEGORIES: Readonly<Record<ProjectsSceneStatus, ProjectStatusCategory>> = {
    backlog: "not_started",
    todo: "not_started",
    inProgress: "active",
    inReview: "active",
    blocked: "blocked",
    done: "done",
  };

  /** The duration options of Event defaults, as the settings list them, and the one the scene picks. */
  type DurationOption = { value: string; label: string };
  const PICKED_DURATION = String(PROJECTS_DEFAULT_MINUTES);

  /** A trigger, a viewport, or a section measured within its positioned parent or the canvas, in logical pixels. */
  interface Box {
    top: number;
    left: number;
    width: number;
    height: number;
  }

  /** Where the list's parts sit, measured once the added task's row lays itself out. Cells are in canvas coordinates, before any scroll. */
  interface ListLayout {
    viewportWidth: number;
    contentWidth: number;
    status: Box;
    priority: Box;
    menuWidth: number;
    statusMenuHeight: number;
    priorityMenuHeight: number;
  }

  /**
   * Where the settings' parts sit, measured once the panel lays itself out for the canvas. The viewport is within the
   * panel, and the Defaults heading and the controls are within the settings' content, before any scroll.
   */
  interface SettingsLayout {
    viewport: Box;
    contentHeight: number;
    defaultsTop: number;
    controls: Record<ProjectsSettingsTarget, Box>;
    paletteWidth: number;
    paletteHeight: number;
    durationMenuHeight: number;
    pomodoroMenuHeight: number;
    durationMenuWidth: number;
    pomodoroMenuWidth: number;
    playlistMenuHeight: number;
    rem: number;
  }

  const now = new Date();
  const sceneDate = filmWorkday(now);
  const today = isoDay(0);
  const timezone = getLocalTimezone();
  const timezoneLabel = getTimezoneInfo(timezone).acronym;

  const taskColumns: readonly ProjectCoreTaskListColumn[] = DEFAULT_TASK_LIST_COLUMNS;
  const gridTemplate = projectTaskListGridTemplate(taskColumns);
  const gridMinWidth = projectTaskListGridMinWidth(taskColumns);
  const leadingGridTemplate = projectTaskListLeadingGridTemplate(taskColumns);
  const gutter = projectTaskListGutterWidth();
  const contentStart = collectionRowContentStart(gutter);

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);
  const stackedRows = $derived(canvasW <= STACKED_ROWS_W);
  /** The top of the list, below the title bar and the project header. */
  const listTop = TOP_BAR_H + HEADER_H;

  const touches = $derived(projectsTouches(defaultsElapsed));
  const taskTouches = $derived(tasksTouches(tasksElapsed));
  const addActive = $derived(tasksAddActive(tasksElapsed));
  const taskAdded = $derived(tasksAdded(tasksElapsed));
  const newStatus = $derived(tasksNewStatus(tasksElapsed));
  const newPriority = $derived(tasksNewPriority(tasksElapsed));
  const listScroll = $derived(tasksListScroll(tasksElapsed));
  const cellMenu = $derived(tasksCellMenu(tasksElapsed));
  /** The part's desktop pointer, and the place it rests on, as a hover sees it. A phone has touches and no hover. */
  const cursorScript = $derived(part === "tasks" ? TASKS_CURSOR : PROJECTS_CURSOR);
  const cursorHovered = $derived(mobileShell ? null : welcomeCursorHovered(cursorScript, elapsed));
  /** Whether the pointer is on the added task's row, which shows its Open button on desktop. */
  const addedRowHovered = $derived(cursorHovered === "tasks-status" || cursorHovered === "tasks-priority");
  const settingsShown = $derived(projectsSettingsShown(defaultsElapsed));
  const settingsOpen = $derived(projectsSettingsOpen(defaultsElapsed));
  const settingsScroll = $derived(projectsSettingsScroll(defaultsElapsed));
  const picker = $derived(projectsSettingsPicker(defaultsElapsed));
  const defaultColor = $derived(projectsDefaultColor(defaultsElapsed));
  const defaultMinutes = $derived(projectsDefaultMinutes(defaultsElapsed));
  const defaultPreset = $derived(projectsDefaultPreset(defaultsElapsed));
  const dirty = $derived(projectsSettingsDirty(defaultsElapsed));
  const eventName = $derived(projectsEventName(defaultsElapsed, t("vaultSetup.projects.eventName")));
  const openPhase = $derived(picker === "focus" || picker === "short-break" || picker === "long-break" ? picker : null);

  const listShown = $derived(projectsListShown(defaultsElapsed));
  const calendarShown = $derived(projectsCalendarShown(defaultsElapsed));
  const calendarDark = $derived(isThemeCalendarDark(theme.current));
  const fullEventName = $derived(t("vaultSetup.projects.eventName"));
  const drawnIn = $derived(projectsEventDrawnIn(defaultsElapsed));
  const creating = $derived(projectsEventCreating(defaultsElapsed));
  const eventPicked = $derived(projectsEventPicked(defaultsElapsed));
  const eventPanelOpen = $derived(projectsPanelOpen(defaultsElapsed));
  const rail = $derived(projectsEventRail(defaultsElapsed));
  /** The event's length: snapped as the Calendar snaps while it is dragged out, then growing to the project's default. */
  const eventMinutes = $derived(
    eventPicked
      ? projectsEventMinutes(defaultsElapsed)
      : Math.max(CREATE_SNAP_MINUTES, Math.round(projectsEventMinutes(defaultsElapsed) / CREATE_SNAP_MINUTES) * CREATE_SNAP_MINUTES),
  );
  const pressOpacity = $derived(
    filmSpan(defaultsElapsed, PROJECTS_BEATS.createStart - PROJECTS_CREATE_PRESS_MS, PROJECTS_BEATS.createStart)
      * (1 - filmSpan(defaultsElapsed, PROJECTS_BEATS.createEnd, PROJECTS_BEATS.createEnd + PROJECTS_CREATE_PRESS_MS)),
  );
  const viewMinutes = $derived(((canvasH - TOP_BAR_H - CAL_HEADER_H) / HOUR_PX) * 60);
  const hours = $derived(
    Array.from({ length: Math.ceil(viewMinutes / 60) + 1 }, (_, index) => Math.ceil(VIEW_START / 60) + index)
      .filter((hour) => hour * 60 > VIEW_START && hour * 60 < VIEW_START + viewMinutes),
  );
  const scrollThumb = $derived.by(() => {
    const visibleHeight = canvasH - TOP_BAR_H - CAL_HEADER_H;
    return calendarScrollbarThumb(
      { scrollTop: (VIEW_START / 60) * HOUR_PX, scrollHeight: 24 * HOUR_PX, clientHeight: visibleHeight },
      visibleHeight,
    );
  });
  const dayFormat = $derived(dayNameFormatForWidth(canvasW - GUTTER_W));
  /** The event panel's anchor: the event as it was drawn, in the day's only column. */
  const panelAnchor = $derived.by<EventPanelAnchor>(() => {
    const columnLeft = GUTTER_W + DAY_BORDER_PX + EVENT_LEFT;
    const blockWidth = ((canvasW - columnLeft) * (100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT)) / 100;
    return {
      x: columnLeft + blockWidth,
      y: TOP_BAR_H + CAL_HEADER_H + y(PROJECTS_EVENT.start),
      width: blockWidth,
      height: px(PROJECTS_EVENT.drawnEnd - PROJECTS_EVENT.start),
    };
  });
  /** The event panel's project picker: the groups the Calendar film shows, with Novel in Personal. */
  const eventPicker = $derived<WelcomeEventPanelPicker>({
    groups: [
      { id: "university", icon: "lucide:graduation-cap", label: t("vaultSetup.film.universityGroup") },
      { id: "health", icon: "lucide:apple", label: t("vaultSetup.film.healthGroup") },
      { id: "personal", icon: "lucide:heart", label: t("vaultSetup.film.personalGroup") },
    ],
    projects: [{ id: "novel", icon: "lucide:feather", label: t("vaultSetup.projects.project") }],
    group: "personal",
    project: "novel",
  });

  /** The tasks the list shows: the General section's, then the added one with the status and priority it has so far. */
  const tasks = $derived<readonly ProjectsSceneTaskEntry[]>(
    taskAdded ? [...PROJECTS_TASKS, { ...TASKS_NEW_TASK, status: newStatus, priority: newPriority }] : PROJECTS_TASKS,
  );

  /** Project settings as a toolbar popover places them, or inset below the top bar on a phone. */
  const panel = $derived.by<Box>(() => {
    if (mobileShell) {
      const top = TOP_BAR_H + PANEL_EDGE;
      return { top, left: PANEL_EDGE, width: canvasW - 2 * PANEL_EDGE, height: canvasH - top - PANEL_EDGE };
    }
    const compact = canvasW <= PANEL_COMPACT_W;
    const panelWidth = compact ? canvasW - 2 * PANEL_EDGE : Math.min(PANEL_WIDTH, canvasW - 2 * PANEL_EDGE);
    const anchorRight = canvasW - HEADER_END_PX;
    const left = compact
      ? PANEL_EDGE
      : Math.min(Math.max(anchorRight - panelWidth, PANEL_EDGE), canvasW - PANEL_EDGE - panelWidth);
    const top = listTop + PANEL_GAP;
    return { top, left, width: panelWidth, height: Math.min(PANEL_MAX_HEIGHT, canvasH - top - PANEL_EDGE) };
  });

  let canvas = $state<HTMLDivElement>();
  let listViewport = $state<HTMLDivElement>();
  let listContent = $state<HTMLDivElement>();
  let statusCell = $state<HTMLSpanElement>();
  let priorityCell = $state<HTMLSpanElement>();
  let statusMenu = $state<HTMLDivElement>();
  let priorityMenu = $state<HTMLDivElement>();
  let settingsPanel = $state<HTMLDivElement>();
  let settingsViewport = $state<HTMLDivElement>();
  let settingsContent = $state<HTMLDivElement>();
  let defaultsSection = $state<HTMLElement>();
  let eventNameTrigger = $state<HTMLSpanElement>();
  let colorTrigger = $state<HTMLDivElement>();
  let durationTrigger = $state<HTMLDivElement>();
  let pomodoroTrigger = $state<HTMLDivElement>();
  let playlistTriggers = $state<Record<ProjectsScenePhase, HTMLDivElement | undefined>>({
    focus: undefined,
    "short-break": undefined,
    "long-break": undefined,
  });
  let palette = $state<HTMLDivElement>();
  let durationMenu = $state<HTMLDivElement>();
  let pomodoroMenu = $state<HTMLDivElement>();
  let playlistMenu = $state<HTMLDivElement>();
  let listLayout = $state<ListLayout | null>(null);
  let settingsLayout = $state<SettingsLayout | null>(null);
  /** Bumped whenever the canvas or the measured content resizes, or fonts finish loading, so the layout is measured again. */
  let layoutTick = $state(0);

  $effect(() => {
    const bump = () => {
      layoutTick += 1;
    };
    const observed = [canvas, listContent, settingsContent].filter((element): element is HTMLDivElement => element !== undefined);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(bump);
    for (const element of observed) observer?.observe(element);
    const fonts = typeof document === "undefined" ? undefined : document.fonts;
    fonts?.addEventListener("loadingdone", bump);
    return () => {
      observer?.disconnect();
      fonts?.removeEventListener("loadingdone", bump);
    };
  });

  /** Measure the list and the settings once they lay themselves out for the canvas and the language. */
  $effect(() => {
    void layoutTick;
    void canvasW;
    void canvasH;
    void stackedRows;
    void localization.locale;
    if (listViewport && listContent && statusCell && priorityCell && statusMenu && priorityMenu) {
      listLayout = {
        viewportWidth: listViewport.clientWidth,
        contentWidth: listContent.offsetWidth,
        status: listCellBox(statusCell, listViewport, listContent),
        priority: listCellBox(priorityCell, listViewport, listContent),
        menuWidth: statusMenu.offsetWidth,
        statusMenuHeight: statusMenu.offsetHeight,
        priorityMenuHeight: priorityMenu.offsetHeight,
      };
    }
    const { focus, "short-break": shortBreak, "long-break": longBreak } = playlistTriggers;
    if (
      settingsPanel && settingsViewport && settingsContent && defaultsSection && eventNameTrigger && colorTrigger && durationTrigger
      && pomodoroTrigger && focus && shortBreak && longBreak && palette && durationMenu && pomodoroMenu && playlistMenu
    ) {
      const content = settingsContent;
      settingsLayout = {
        viewport: relBox(settingsViewport, settingsPanel),
        contentHeight: content.offsetHeight,
        defaultsTop: relBox(defaultsSection, content).top,
        controls: {
          name: relBox(eventNameTrigger, content),
          color: relBox(colorTrigger, content),
          duration: relBox(durationTrigger, content),
          pomodoro: relBox(pomodoroTrigger, content),
          focus: relBox(focus, content),
          "short-break": relBox(shortBreak, content),
          "long-break": relBox(longBreak, content),
        },
        paletteWidth: palette.offsetWidth,
        paletteHeight: palette.offsetHeight,
        durationMenuHeight: durationMenu.offsetHeight,
        pomodoroMenuHeight: pomodoroMenu.offsetHeight,
        durationMenuWidth: durationMenu.scrollWidth,
        pomodoroMenuWidth: pomodoroMenu.scrollWidth,
        playlistMenuHeight: Math.min(playlistMenu.scrollHeight, PLAYLIST_POPOVER_MAX_H),
        rem: rootRemPx(),
      };
    }
  });

  /** How far the list has scrolled sideways: until the added task's status is in view, then on until its priority is. */
  const listScrollPx = $derived.by(() => {
    if (!listLayout) return 0;
    const { viewportWidth } = listLayout;
    const maxScroll = Math.max(0, listLayout.contentWidth - viewportWidth);
    const target = (cell: Box) => Math.min(maxScroll, Math.max(0, cell.left + cell.width + CELL_SCROLL_PAD - viewportWidth));
    const statusTarget = target(listLayout.status);
    const priorityTarget = Math.max(statusTarget, target(listLayout.priority));
    return statusTarget * listScroll.status + (priorityTarget - statusTarget) * listScroll.priority;
  });

  /** The open cell menu's place, below its cell when it fits there and above it otherwise, as the list's cell menus place themselves. */
  const cellMenuPlace = $derived.by(() => {
    if (!listLayout || !cellMenu) return null;
    const cell = cellMenu === "status" ? listLayout.status : listLayout.priority;
    const menuWidth = listLayout.menuWidth;
    const menuHeight = cellMenu === "status" ? listLayout.statusMenuHeight : listLayout.priorityMenuHeight;
    const cellLeft = cell.left - listScrollPx;
    const left = Math.max(CELL_MENU_MARGIN, Math.min(cellLeft, canvasW - menuWidth - CELL_MENU_MARGIN));
    const belowTop = cell.top + cell.height + CELL_MENU_GAP;
    const aboveBottom = cell.top - CELL_MENU_GAP;
    const belowSpace = Math.max(0, canvasH - CELL_MENU_MARGIN - belowTop);
    const aboveSpace = Math.max(0, aboveBottom - CELL_MENU_MARGIN);
    const below = belowSpace >= menuHeight || belowSpace >= aboveSpace;
    const top = Math.max(CELL_MENU_MARGIN, Math.min(below ? belowTop : aboveBottom - menuHeight, canvasH - CELL_MENU_MARGIN - menuHeight));
    return { left, top };
  });

  /**
   * Where the settings rest for each control: the Defaults heading at the top for the event name, then for every other
   * control the least scroll from the one before that brings it into view. The color also keeps room for its palette
   * below or above it on the canvas, so the palette never covers its own trigger on a short canvas.
   */
  const settingsOffsets = $derived.by<Record<ProjectsSettingsTarget, number> | null>(() => {
    const layout = settingsLayout;
    if (!layout) return null;
    const viewportHeight = layout.viewport.height;
    const maxScroll = Math.max(0, layout.contentHeight - viewportHeight);
    const clamp = (value: number) => Math.min(maxScroll, Math.max(0, value));
    const base = panel.top + layout.viewport.top;
    const paletteGap = (PALETTE_PADDING_REM / 2) * layout.rem;
    const offsets = {} as Record<ProjectsSettingsTarget, number>;
    let previous = 0;
    for (const target of PROJECTS_SETTINGS_TARGETS) {
      const box = layout.controls[target];
      let offset: number;
      if (target === "name") {
        offset = clamp(layout.defaultsTop - DEFAULTS_SCROLL_PAD);
      } else {
        const pad = CONTROL_SCROLL_PAD;
        let low = clamp(box.top + box.height + pad - viewportHeight);
        let high = Math.max(low, clamp(box.top - pad));
        if (target === "color") {
          const belowLow = base + box.top + box.height + paletteGap + layout.paletteHeight + PALETTE_EDGE - canvasH + FIT_SLACK_PX;
          const aboveHigh = base + box.top - paletteGap - layout.paletteHeight - PALETTE_EDGE - FIT_SLACK_PX;
          const nearest = (from: number, to: number) => Math.min(to, Math.max(from, previous));
          const below = belowLow <= high ? { low: Math.max(low, belowLow), high } : null;
          const above = aboveHigh >= low ? { low, high: Math.min(high, aboveHigh) } : null;
          const fits = [below, above].filter((range): range is { low: number; high: number } => range !== null && range.low <= range.high);
          if (fits.length > 0) {
            const best = fits.reduce((chosen, range) =>
              Math.abs(nearest(range.low, range.high) - previous) < Math.abs(nearest(chosen.low, chosen.high) - previous) ? range : chosen);
            low = best.low;
            high = best.high;
          }
        }
        offset = Math.min(high, Math.max(low, previous));
      }
      offsets[target] = offset;
      previous = offset;
    }
    return offsets;
  });

  /** How far the settings have scrolled, easing from one control's rest to the next as each comes up. */
  const settingsScrollPx = $derived.by(() => {
    if (!settingsOffsets) return 0;
    const from = settingsScroll.from ? settingsOffsets[settingsScroll.from] : 0;
    const to = settingsScroll.to ? settingsOffsets[settingsScroll.to] : 0;
    return from + (to - from) * settingsScroll.progress;
  });

  /** A settings control's trigger as a rectangle on the canvas, at the settings' current scroll. */
  function settingsRect(trigger: Box): SelectPopoverRect {
    const layout = settingsLayout;
    const top = panel.top + (layout?.viewport.top ?? 0) + trigger.top - settingsScrollPx;
    const left = panel.left + (layout?.viewport.left ?? 0) + trigger.left;
    return { top, left, right: left + trigger.width, bottom: top + trigger.height, width: trigger.width, height: trigger.height };
  }

  const canvasRect = $derived<SelectPopoverRect>({ top: 0, left: 0, right: canvasW, bottom: canvasH, width: canvasW, height: canvasH });

  /** The open Duration or Pomodoro options, placed as the settings' select places them against the canvas. */
  const selectGeometry = $derived.by(() => {
    if (!settingsLayout || (picker !== "duration" && picker !== "pomodoro")) return null;
    const duration = picker === "duration";
    return pickSelectPopoverGeometry({
      triggerRect: settingsRect(duration ? settingsLayout.controls.duration : settingsLayout.controls.pomodoro),
      boundaryRect: canvasRect,
      contentHeight: duration ? settingsLayout.durationMenuHeight : settingsLayout.pomodoroMenuHeight,
      contentWidth: duration ? settingsLayout.durationMenuWidth : settingsLayout.pomodoroMenuWidth,
      horizontalAlign: duration ? "start" : "end",
    });
  });

  /** The open phase's playlists, placed as the playlist select places them against the canvas. */
  const playlistGeometry = $derived.by(() => {
    if (!settingsLayout || !openPhase) return null;
    return pickSelectPopoverGeometry({
      triggerRect: settingsRect(settingsLayout.controls[openPhase]),
      boundaryRect: canvasRect,
      contentHeight: settingsLayout.playlistMenuHeight,
      contentWidth: PLAYLIST_POPOVER_W,
      horizontalAlign: "end",
    });
  });

  /** The color palette's place: below its trigger when it fits, above it otherwise, and kept on the canvas. */
  const palettePlace = $derived.by(() => {
    if (!settingsLayout || picker !== "color") return null;
    const trigger = settingsRect(settingsLayout.controls.color);
    const { paletteWidth, paletteHeight } = settingsLayout;
    const offset = (PALETTE_PADDING_REM / 2) * settingsLayout.rem;
    const maxLeft = Math.max(PALETTE_EDGE, canvasW - paletteWidth - PALETTE_EDGE);
    const left = Math.min(Math.max(PALETTE_EDGE, trigger.left), maxLeft);
    const belowTop = trigger.bottom + offset;
    const aboveTop = trigger.top - paletteHeight - offset;
    const top = belowTop + paletteHeight + PALETTE_EDGE <= canvasH
      ? belowTop
      : aboveTop >= PALETTE_EDGE
        ? aboveTop
        : Math.max(PALETTE_EDGE, canvasH - paletteHeight - PALETTE_EDGE);
    return { left, top };
  });

  const durationOptions = $derived<DurationOption[]>([
    { value: "default", label: t("projects.settings.defaultDurationNone") },
    { value: "10", label: t("projects.settings.durationMinutes", 10) },
    { value: "15", label: t("projects.settings.durationMinutes", 15) },
    { value: "30", label: t("projects.settings.durationMinutes", 30) },
    { value: "60", label: t("projects.settings.durationHours", 1) },
    { value: "120", label: t("projects.settings.durationHours", 2) },
    { value: "180", label: t("projects.settings.durationHours", 3) },
    { value: "240", label: t("projects.settings.durationHours", 4) },
    { value: "all_day", label: t("projects.settings.defaultDurationAllDay") },
    { value: "custom", label: t("projects.settings.durationCustom") },
  ]);
  const durationValue = $derived(defaultMinutes === null ? "default" : String(defaultMinutes));
  const durationLabel = $derived(durationOptions.find((option) => option.value === durationValue)?.label ?? "");
  const pomodoroLabel = $derived(defaultPreset === null ? t("common.none") : presetLabel(defaultPreset));
  const selectedColor = $derived(defaultColor ?? FALLBACK_COLOR_INDEX);
  const calendarTokens = $derived(resolveCalendarTokens(theme.current));
  const paletteBg = $derived(calendarTokens["--cal-bg"]);
  const selectionBorder = $derived(contrastRatio(paletteBg, "#000000") >= contrastRatio(paletteBg, "#ffffff") ? "#000000" : "#ffffff");

  const personName = $derived(preferences.profileDisplayName || t("projects.people.you"));
  /** The project's only member, as the members section names the person using this device. */
  const memberName = $derived(preferences.profileDisplayName.trim() || t("contacts.you"));
  const memberDetail = $derived(preferences.profileDisplayName.trim() ? t("contacts.you") : null);

  /**
   * An element's place within `ancestor`, in the ancestor's logical pixels, so the scene's scale and any transform the two
   * share do not count.
   */
  function relBox(element: HTMLElement, ancestor: HTMLElement): Box {
    const rect = element.getBoundingClientRect();
    const origin = ancestor.getBoundingClientRect();
    const scale = ancestor.offsetWidth > 0 && origin.width > 0 ? origin.width / ancestor.offsetWidth : 1;
    return {
      top: (rect.top - origin.top) / scale,
      left: (rect.left - origin.left) / scale,
      width: rect.width / scale,
      height: rect.height / scale,
    };
  }

  /**
   * A list cell's place on the canvas with the list unscrolled: its box within the list content, which carries the
   * sideways scroll, from where the list viewport sits. Rendered boxes keep the scene's zoom out, which offsets do not
   * in every webview.
   */
  function listCellBox(cell: HTMLElement, viewport: HTMLElement, content: HTMLElement): Box {
    const inContent = relBox(cell, content);
    const origin = canvas ? relBox(viewport, canvas) : { top: 0, left: 0 };
    return { ...inContent, top: origin.top + inContent.top, left: origin.left + inContent.left };
  }

  function rootRemPx(): number {
    const fontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
    return Number.isFinite(fontSize) && fontSize > 0 ? fontSize : 16;
  }

  /** The scene's day shifted by `offset` days, as the canonical date a task column stores. */
  function isoDay(offset: number): string {
    const day = new Date(sceneDate.getFullYear(), sceneDate.getMonth(), sceneDate.getDate() + offset);
    return `${day.getFullYear()}-${String(day.getMonth() + 1).padStart(2, "0")}-${String(day.getDate()).padStart(2, "0")}`;
  }

  function presetLabel(preset: PomodoroPresetKey): string {
    switch (preset) {
      case "adaptive": return t("projects.pomodoro.adaptive");
      case "creative": return t("projects.pomodoro.creative");
      case "balanced": return t("projects.pomodoro.balanced");
      case "deep": return t("projects.pomodoro.deep");
      case "extended": return t("projects.pomodoro.extended");
    }
  }

  function columnLabel(column: ProjectCoreTaskListColumn): string {
    switch (column) {
      case "priority": return t("projects.columns.priority");
      case "estimate": return t("projects.columns.estimate");
      case "start": return t("projects.columns.start");
      case "due": return t("projects.columns.due");
      case "scheduled": return t("projects.columns.scheduled");
      case "dependencies": return t("projects.columns.dependencies");
      case "assignee": return t("projects.columns.assignee");
      case "reviewer": return t("projects.columns.reviewer");
      default: return t("projects.columns.status");
    }
  }

  function statusName(key: ProjectsSceneStatus): string {
    return t(`vaultSetup.projects.status.${key}`);
  }

  /** A status as a new project seeds it, for the list's status badge. */
  function sceneStatus(key: ProjectsSceneStatus): ProjectStatus {
    return {
      id: `scene-${key}`,
      projectId: "scene-novel",
      name: statusName(key),
      category: STATUS_CATEGORIES[key],
      color: PROJECTS_STATUS_COLORS[key],
      orderKey: key,
      terminal: key === "done",
      createdAt: "",
      updatedAt: "",
    };
  }

  function priorityName(key: ProjectsScenePriority): string {
    return t(`projects.priority.${key}`);
  }

  function playlistName(key: MusicScenePlaylist): string {
    return t(`music.builder.defaultPlaylist.${key}`);
  }

  function playlistIcon(key: MusicScenePlaylist): string {
    return MUSIC_PLAYLISTS.find((entry) => entry.key === key)?.icon ?? "";
  }

  function playlistLookup(key: MusicScenePlaylist): { icon: string; name: string } {
    return { icon: playlistIcon(key), name: playlistName(key) };
  }

  function title(key: SceneTitle): string {
    switch (key) {
      case "readSources": return t("vaultSetup.film.readSources");
      case "lunch": return t("vaultSetup.film.lunch");
      case "teamMeeting": return t("vaultSetup.film.teamMeeting");
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

  function phaseLabel(phase: ProjectsScenePhase): string {
    if (showSoundscapes) return t(`music.assignment.phase.${phase}`);
    if (phase === "focus") return t("projects.settings.focusPlaylist");
    return phase === "short-break" ? t("projects.settings.shortBreakPlaylist") : t("projects.settings.longBreakPlaylist");
  }

  /** The highlight a cell takes as the pointer rests on it and while its menu is open, or from its touch on a phone. */
  function cellHighlight(cell: TasksSceneCell): string | undefined {
    const highlighted = mobileShell ? tasksCellHighlighted(tasksElapsed, cell) : cursorHovered === `tasks-${cell}` || cellMenu === cell;
    return highlighted ? "linear-gradient(var(--collection-highlight), var(--collection-highlight))" : undefined;
  }

  /**
   * Whether the option about to be picked shows as hovered: under the pointer on desktop, or just before its touch lands
   * on a phone, given by `touchHovered`.
   */
  function optionHovered(target: string, touchHovered: boolean): boolean {
    return mobileShell ? touchHovered : cursorHovered === target;
  }
</script>

{#snippet touch(opacity: number, left: string, top: string)}
  {#if mobileShell && opacity > 0}
    <span
      class="pointer-events-none absolute z-20 size-6 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35"
      style:left
      style:top
      style:opacity
    ></span>
  {/if}
{/snippet}

{#snippet checkbox()}
  <span class="grid size-4 shrink-0 place-items-center rounded-sm border border-foreground/25" aria-hidden="true"></span>
{/snippet}

{#snippet chevron(open: boolean)}
  <ChevronDown size={13} strokeWidth={2} class={cn("shrink-0 text-muted-foreground", open && "rotate-180")} />
{/snippet}

<!-- A focused text field's caret, as a text input draws it. -->
{#snippet caret()}
  <span class="film-caret"></span>
{/snippet}

<!-- A labeled settings row, as the settings' fields and selects lay themselves out. -->
{#snippet settingsRow(label: string, control: import("svelte").Snippet)}
  <div class={cn("flex justify-between px-1 py-1", stackedRows ? "flex-col items-stretch gap-2" : "items-center gap-4")}>
    <span class="min-w-0 flex-1 text-[0.866667rem] text-foreground">
      {label}
    </span>
    {@render control()}
  </div>
{/snippet}

{#snippet selectTrigger(label: string, open: boolean)}
  <span class="flex h-7 w-full items-center justify-between gap-2 rounded-md border border-border px-2.5 font-medium text-foreground">
    <span class="flex min-w-0 flex-1 items-center gap-1.5"><span class="truncate">{label}</span></span>
    {@render chevron(open)}
  </span>
{/snippet}

{#snippet nameControl()}
  <span class={cn("field flex min-w-0 items-center px-2.5 font-medium text-foreground", stackedRows ? "w-full" : "w-44")}>{t("vaultSetup.projects.project")}</span>
{/snippet}

{#snippet groupControl()}
  <span class={cn("relative min-w-0", stackedRows ? "w-full" : "w-44")}>{@render selectTrigger(t("vaultSetup.film.personalGroup"), false)}</span>
{/snippet}

{#snippet lifecycleControl()}
  <span class={cn("relative min-w-0", stackedRows ? "w-full" : "w-44")}>{@render selectTrigger(t("projects.lifecycle.active"), false)}</span>
{/snippet}

{#snippet iconControl()}
  <span class={cn("flex h-7 min-w-0 items-center gap-2 rounded-md border border-border px-2.5 text-foreground", stackedRows ? "w-full" : "w-44")}>
    <Feather size={14} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
    <span class="min-w-0 flex-1"></span>
    {@render chevron(false)}
  </span>
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

{#snippet focusRail(start: number, end: number, breaks: readonly FilmSpan[])}
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
    {#each breaks as band, index (index)}
      <div
        class="absolute inset-x-0"
        style:top={`${((band.start - start) / length) * 100}%`}
        style:height={`${((band.end - band.start) / length) * 100}%`}
        style:background-color="var(--cal-timeline-break)"
      ></div>
    {/each}
  </div>
{/snippet}

{#snippet eventNameControl()}
  {@const focused = projectsEventNameFocused(defaultsElapsed)}
  {@const caretOn = projectsEventNameCaretOn(defaultsElapsed)}
  <span bind:this={eventNameTrigger} data-cursor="projects-name" class={cn("field relative flex min-w-0 items-center px-2.5 text-left font-medium text-foreground", stackedRows ? "w-full" : "w-44")}>
    <span class="min-w-0 truncate">
      {#if eventName}{eventName}{#if focused && caretOn}{@render caret()}{/if}{:else}{#if focused && caretOn}{@render caret()}{/if}<span class="text-muted-foreground">{t("common.none")}</span>{/if}
    </span>
    {@render touch(touches.name, "50%", "50%")}
  </span>
{/snippet}

{#snippet colorControl()}
  <div bind:this={colorTrigger} data-cursor="projects-color" class={cn("relative min-w-0", stackedRows ? "w-full" : "w-44")}>
    <span class="flex h-7 w-full max-w-full items-center justify-between gap-2 rounded-md border border-border bg-card px-2.5 text-left text-[0.8rem] font-medium text-foreground dark:bg-transparent">
      <span class="flex min-w-0 items-center gap-2">
        <span class="h-3.5 w-3.5 shrink-0 rounded-[3px] border border-transparent" style:background-color={getEventColor(selectedColor, theme.current).bg}></span>
        <span class="truncate">{t("calendar.color.eventColorNumber", selectedColor + 1)}</span>
      </span>
      {@render chevron(picker === "color")}
    </span>
    {@render touch(touches.color, "50%", "50%")}
  </div>
{/snippet}

{#snippet durationControl()}
  <div bind:this={durationTrigger} data-cursor="projects-duration" class={cn("relative min-w-0", stackedRows ? "w-full" : "w-44")}>
    {@render selectTrigger(durationLabel, picker === "duration")}
    {@render touch(touches.duration, "50%", "50%")}
  </div>
{/snippet}

{#snippet pomodoroControl()}
  <div bind:this={pomodoroTrigger} data-cursor="projects-pomodoro" class={cn("relative min-w-0", stackedRows ? "w-full" : "w-44")}>
    {@render selectTrigger(pomodoroLabel, picker === "pomodoro")}
    {@render touch(touches.pomodoro, "50%", "50%")}
  </div>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5">
  <div
    bind:this={canvas}
    class="relative flex flex-col overflow-hidden rounded-2xl bg-(--cal-bg) text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    {#if mobileShell}
      <!-- The phone's top bar, with the Novel project's context switcher. -->
      <div class="flex shrink-0 items-center gap-2 bg-background px-1.5" style:height={`${TOP_BAR_H}px`}>
        <span
          class="flex h-9 min-w-0 items-center gap-2 rounded-full bg-accent/55 pr-2.5 pl-2 text-identity font-medium"
          style:opacity={listShown < 1 ? listShown : undefined}
        >
          <span class="grid size-5 shrink-0 place-items-center"><Feather size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} /></span>
          <span class="truncate">{t("vaultSetup.projects.project")}</span>
          <ChevronDown size={16} class="shrink-0 text-muted-foreground" />
        </span>
      </div>
    {:else}
      <!-- The title bar's Music. -->
      <div class="flex shrink-0 items-center justify-end gap-0.5 bg-sidebar" style:height={`${TOP_BAR_H}px`} style:padding-right={`${TITLE_BAR_END_PX}px`}>
        <span class={cn("grid size-7 place-items-center rounded-lg", TITLE_ICON_COLOR_CLASS)}>
          <Music size={TITLE_ICON_SIZE} strokeWidth={TITLE_ICON_STROKE_WIDTH} />
        </span>
      </div>
    {/if}

    <!-- The Novel project's List view. -->
    <div class="relative flex min-h-0 flex-1 flex-col bg-(--cal-bg)">
      {#if mobileShell}
        <div class="flex shrink-0 items-center gap-0 px-(--mobile-chrome-inset)" style:height={`${HEADER_H}px`}>
          <div class="flex h-12 min-w-0 flex-1 items-center">
            <span class={MOBILE_CHROME_BUTTON_CLASS}><PanelLeft size={MOBILE_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
            <span class="mobile-chrome-button flex min-w-0 items-center justify-start gap-1.5 px-1.5 text-foreground">
              <List size={MOBILE_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} class="shrink-0" />
              <span class="min-w-0 truncate text-xs font-medium">{t("projects.tabs.list")}</span>
              <ChevronDown size={11} class="shrink-0" />
            </span>
          </div>
          <span class={MOBILE_CHROME_BUTTON_CLASS}><SlidersHorizontal size={MOBILE_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
          <span class={cn("relative", MOBILE_CHROME_BUTTON_CLASS, settingsOpen && "bg-accent")}>
            <Settings2 size={MOBILE_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} />
            {@render touch(touches.settings, "50%", "50%")}
          </span>
        </div>
      {:else}
        <div class="flex shrink-0 items-center gap-1 px-3" style:height={`${HEADER_H}px`}>
          <span class="flex h-7 min-w-0 items-center gap-0.5 overflow-hidden text-identity font-medium">
            <span class="flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5">
              <Heart size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
              <span class="truncate">{t("vaultSetup.film.personalGroup")}</span>
            </span>
            <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
            <span class="flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5">
              <Feather size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
              <span class="truncate">{t("vaultSetup.projects.project")}</span>
              <WorkspaceBreadcrumbTerminalIcon kind="chevron" class="shrink-0 text-muted-foreground" />
            </span>
            <span class="grid h-7 w-7 shrink-0 place-items-center rounded-md"><WorkspaceBreadcrumbTerminalIcon kind="plus" /></span>
          </span>
          <span class="flex-1"></span>
          <span class="flex shrink-0 items-center gap-1">
            <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-foreground"><ListFilter size={TOOL_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
            <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-foreground"><ArrowUpDown size={TOOL_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
            <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-foreground"><Columns3 size={TOOL_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
            <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-foreground"><Layers size={TOOL_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
            <span data-cursor="projects-settings" class={cn("relative flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-foreground", settingsOpen && "bg-accent")}>
              <Settings2 size={TOOL_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} />
              {@render touch(touches.settings, "50%", "50%")}
            </span>
          </span>
        </div>
      {/if}

      <div bind:this={listViewport} data-cursor="projects-list" class="list-scroll relative min-h-0 flex-1 overflow-hidden" style:--list-divider-left={contentStart}>
        <div
          bind:this={listContent}
          class="flex w-max min-w-full flex-col p-3"
          style:transform={listScrollPx > 0 ? `translateX(${-listScrollPx}px)` : undefined}
        >
          <!-- The section header, whose leading part stays in place as the list scrolls sideways. -->
          <div class="list-divider grid min-h-(--collection-table-row-height) items-center px-1" style:grid-template-columns={gridTemplate} style:min-width={gridMinWidth}>
            <div
              class="grid min-h-(--collection-table-row-height) items-center"
              style:grid-column="1 / span 3"
              style:grid-template-columns={leadingGridTemplate}
              style:transform={listScrollPx > 0 ? `translateX(${listScrollPx}px)` : undefined}
            >
              <span class="flex h-7 items-center justify-center">{#if mobileShell}{@render checkbox()}{/if}</span>
              <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground"><ChevronDown size={14} strokeWidth={1.75} /></span>
              <span class="flex min-h-8 min-w-0 flex-1 items-center gap-2 rounded-md px-2">
                <span class="truncate text-[0.866667rem] font-semibold">{t("projects.list.general")}</span>
              </span>
            </div>
          </div>

          <div class="list-divider">
            <CollectionRow template={gridTemplate} minWidth={gridMinWidth} {gutter} header divider={false}>
              <div></div>
              <div></div>
              <CollectionCell class="gap-1.5">
                {#if preferences.projectsShowPropertyIcons}
                  <CollectionPropertyIcon kind={projectListColumnKind("name", undefined)} class="size-3.5 shrink-0 text-muted-foreground" />
                {/if}
                <span class="truncate">{t("projects.list.name")}</span>
              </CollectionCell>
              {#each taskColumns as column (column)}
                <CollectionCell class="gap-1.5">
                  {#if preferences.projectsShowPropertyIcons}
                    <CollectionPropertyIcon kind={projectListColumnKind(column, undefined)} class="size-3.5 shrink-0 text-muted-foreground" />
                  {/if}
                  <span class="truncate">{columnLabel(column)}</span>
                </CollectionCell>
              {/each}
              <div class="grid min-h-(--collection-table-row-height) min-w-0 place-items-center self-stretch text-muted-foreground">
                <span class="grid size-9 place-items-center"><Plus size={14} strokeWidth={1.75} class="size-3.5" /></span>
              </div>
            </CollectionRow>
          </div>

          {#each tasks as task (task.key)}
            {@const added = task.key === TASKS_NEW_TASK.key}
            <div class="list-divider">
              <CollectionRow template={gridTemplate} minWidth={gridMinWidth} {gutter} divider={false}>
                <div class="flex h-full items-center justify-center">{#if mobileShell}{@render checkbox()}{/if}</div>
                <div></div>
                <CollectionCell>
                  <span class="flex h-7 min-w-0 items-center"><span class="truncate text-foreground">{t(`vaultSetup.projects.task.${task.key}`)}</span></span>
                  <!-- The name cell's Open button, as the row draws it: always on touch, and on the hovered row with a pointer. -->
                  {#if mobileShell || (added && addedRowHovered)}
                    <span class="absolute top-1/2 right-1 z-10 flex h-6 -translate-y-1/2 items-center gap-1 rounded-md border border-border bg-popover px-1.5 text-[0.75rem] text-popover-foreground">
                      <PanelRight class="size-3.5" aria-hidden="true" /><span>{t("collections.property.open")}</span>
                    </span>
                  {/if}
                </CollectionCell>
                {#each taskColumns as column (column)}
                  {#if column === "status"}
                    <CollectionCell style={added ? `background-image: ${cellHighlight("status") ?? "none"}` : undefined}>
                      {#if added}<span bind:this={statusCell} data-cursor="tasks-status" class="absolute inset-0"></span>{/if}
                      <div class="pointer-events-none relative z-10 min-w-0 max-w-full">
                        <ProjectStatusBadge status={sceneStatus(task.status)} theme={theme.current} label={statusName(task.status)} class="text-[0.733333rem]" />
                      </div>
                      {#if added}{@render touch(taskTouches.status, "2rem", "50%")}{/if}
                    </CollectionCell>
                  {:else if column === "priority"}
                    <CollectionCell style={added ? `background-image: ${cellHighlight("priority") ?? "none"}` : undefined}>
                      {#if added}<span bind:this={priorityCell} data-cursor="tasks-priority" class="absolute inset-0"></span>{/if}
                      <div class="pointer-events-none relative z-10 min-w-0 max-w-full">
                        <span class="flex min-w-0 items-center gap-1.5 text-foreground">
                          <PriorityFlagIcon color={PROJECTS_PRIORITY_COLORS[task.priority]} theme={theme.current} size={13} class="shrink-0" />
                          <span class="min-w-0 truncate">{priorityName(task.priority)}</span>
                        </span>
                      </div>
                      {#if added}{@render touch(taskTouches.priority, "2rem", "50%")}{/if}
                    </CollectionCell>
                  {:else}
                    <CollectionCell>
                      {#if column === "assignee" || column === "reviewer"}
                        <ProfileAvatar displayName={personName} imagePath={preferences.profileAvatarImagePath} crop={preferences.profileImageCrop} color={preferences.profileColor} size={24} />
                      {:else if column === "start" || column === "due"}
                        {@const offset = column === "start" ? task.start : task.due}
                        <span class={cn("block min-w-0 truncate", offset === null ? "text-muted-foreground" : "text-foreground")}>
                          {offset === null
                            ? (column === "start" ? t("projects.detail.noDate") : t("projects.filters.noDueDate"))
                            : projectListDateLabel(isoDay(offset), "locale", localization.locale, today)}
                        </span>
                      {/if}
                    </CollectionCell>
                  {/if}
                {/each}
                <div></div>
              </CollectionRow>
            </div>
          {/each}

          <!-- The section's add row, as the list's quick add draws it closed and open. -->
          <div class="list-divider">
            <CollectionRow template={gridTemplate} minWidth={gridMinWidth} {gutter} divider={false}>
              <div class="col-span-full grid min-w-0 grid-cols-subgrid self-stretch">
                {#if addActive}
                  {@const draft = tasksDraft(tasksElapsed, t("vaultSetup.projects.task.draftChapter"))}
                  {@const caretOn = tasksCaretOn(tasksElapsed)}
                  <span class="flex min-h-(--collection-table-row-height) min-w-0 items-center gap-1 rounded-md px-2" style:grid-column="3 / -1">
                    <span class="flex min-h-8 w-full min-w-0 items-center text-collection text-foreground">
                      <span class="min-w-0 truncate">
                        {#if draft}{draft}{#if caretOn}{@render caret()}{/if}{:else}{#if caretOn}{@render caret()}{/if}<span class="text-muted-foreground">{t("projects.list.addTaskInSection", t("projects.list.general"))}</span>{/if}
                      </span>
                    </span>
                    <span class="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground"><Plus class="size-4" /></span>
                    <span class="rounded-md px-2 py-1 text-[0.8rem] text-muted-foreground">{t("common.cancel")}</span>
                  </span>
                {:else}
                  <span class="col-span-full grid min-h-(--collection-table-row-height) grid-cols-subgrid items-center text-left text-collection text-muted-foreground">
                    <span class="flex min-w-0 items-center gap-2 px-2" style:grid-column="3 / -1">
                      <Plus class="size-4 shrink-0" />
                      <span class="relative min-w-0" data-cursor="tasks-add">
                        <span class="block truncate">{t("projects.list.addTaskInSection", t("projects.list.general"))}</span>
                        {@render touch(taskTouches.add, "50%", "50%")}
                      </span>
                    </span>
                  </span>
                {/if}
              </div>
            </CollectionRow>
          </div>
        </div>
      </div>

      <!-- The Calendar's day, as the Calendar film draws it, where the drawn event takes the Novel project's defaults. -->
      {#if calendarShown > 0}
        <div class="absolute inset-0 z-45 flex flex-col bg-(--cal-bg)" style:opacity={calendarShown < 1 ? calendarShown : undefined}>
          <div class="flex shrink-0 border-b border-sidebar" style:height={`${CAL_HEADER_H}px`}>
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
                {@render focusRail(rail.start, rail.end, rail.breaks)}
              {/if}

              <div class="absolute inset-y-0 right-0" style:left={`${EVENT_LEFT}px`}>
                {#each DAY_EVENTS as event (event.start)}
                  {@render eventBlock(
                    event.start,
                    event.end,
                    event.color,
                    title(event.title),
                    event.end <= PROJECTS_CLOCK,
                    false,
                    DAY_EVENTS.some((other) => other.start === event.end),
                    event.icons,
                  )}
                {/each}
                {#if creating}
                  {@render eventBlock(
                    PROJECTS_EVENT.start,
                    PROJECTS_EVENT.start + eventMinutes,
                    eventPicked ? PROJECTS_COLOR : undefined,
                    eventPicked ? fullEventName : t("calendar.event.noTitle"),
                    false,
                    true,
                    false,
                    eventPicked ? ["music"] : [],
                  )}
                {:else if drawnIn}
                  {@render eventBlock(
                    PROJECTS_EVENT.start,
                    PROJECTS_EVENT.start + PROJECTS_DEFAULT_MINUTES,
                    PROJECTS_COLOR,
                    fullEventName,
                    false,
                    false,
                    false,
                    ["music"],
                  )}
                {/if}

                <!--
                  Where the desktop pointer presses on the empty slot and drags the event out: its bottom edge follows
                  the drag smoothly while the event snaps to the grid.
                -->
                {#if !mobileShell && defaultsElapsed < PROJECTS_BEATS.panelOpen}
                  <span
                    data-cursor="projects-drag"
                    class="pointer-events-none absolute left-0"
                    style:width={`${100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT}%`}
                    style:top={`${y(PROJECTS_EVENT.start)}px`}
                    style:height={`${px(projectsEventDrawn(defaultsElapsed) * (PROJECTS_EVENT.drawnEnd - PROJECTS_EVENT.start))}px`}
                  ></span>
                {/if}

                <!-- The drawn event's resize knobs, as a phone shows them from the start of the drawing until Save. -->
                {#if mobileShell && creating}
                  <div
                    class="pointer-events-none absolute left-0"
                    style:width={`${100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT}%`}
                    style:top={`${y(PROJECTS_EVENT.start)}px`}
                    style:height={`${px(eventMinutes)}px`}
                    style:z-index="11"
                    style:--knob-color={getEventColor(eventPicked ? PROJECTS_COLOR : undefined, theme.current).bg}
                  >
                    <div class="resize-knob absolute top-0 left-0"></div>
                    <div class="resize-knob absolute right-0 bottom-0"></div>
                  </div>
                {/if}

                <!-- The press that draws the event, following its growing edge. -->
                {#if mobileShell && pressOpacity > 0}
                  <span
                    class="pointer-events-none absolute left-3/5 z-20 size-6 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35"
                    style:top={`${y(PROJECTS_EVENT.start + eventMinutes)}px`}
                    style:opacity={pressOpacity}
                  ></span>
                {/if}
              </div>

              <div class="pointer-events-none absolute inset-x-0" style:top={`${y(PROJECTS_CLOCK)}px`} style:z-index="46">
                <div class="absolute size-2.5 shrink-0 rounded-full" style:background-color="var(--cal-current-time)" style:left="-5px" style:top="-5px"></div>
                <div class="absolute inset-x-0 h-[2.3px]" style:background-color="var(--cal-current-time)" style:top="-1.15px"></div>
              </div>
            </div>
          </div>
        </div>
      {/if}
    </div>

    {#if part === "tasks"}
      <!-- The added task's status and priority menus. Both stay laid out while hidden so their size is known before they open. -->
      <div
        bind:this={statusMenu}
        class="surface-floating surface-floating-body absolute z-40 w-44"
        style:left={`${cellMenu === "status" && cellMenuPlace ? cellMenuPlace.left : 0}px`}
        style:top={`${cellMenu === "status" && cellMenuPlace ? cellMenuPlace.top : 0}px`}
        style:visibility={cellMenu === "status" && cellMenuPlace ? undefined : "hidden"}
      >
        {#each PROJECTS_STATUS_ORDER as status (status)}
          {@const picked = status === TASKS_NEW_STATUS}
          <span data-cursor={picked ? "tasks-status-option" : undefined} class={cn("menu-item relative justify-between", picked && optionHovered("tasks-status-option", tasksOptionHovered(tasksElapsed, "status")) && "bg-accent")}>
            <ProjectStatusBadge status={sceneStatus(status)} theme={theme.current} label={statusName(status)} class="text-[0.733333rem]" />
            {#if status === newStatus}<Check size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />{/if}
            {#if picked}{@render touch(taskTouches.statusOption, "2.5rem", "50%")}{/if}
          </span>
        {/each}
      </div>

      <div
        bind:this={priorityMenu}
        class="surface-floating surface-floating-body absolute z-40 w-44"
        style:left={`${cellMenu === "priority" && cellMenuPlace ? cellMenuPlace.left : 0}px`}
        style:top={`${cellMenu === "priority" && cellMenuPlace ? cellMenuPlace.top : 0}px`}
        style:visibility={cellMenu === "priority" && cellMenuPlace ? undefined : "hidden"}
      >
        {#each PROJECTS_PRIORITY_ORDER as priority (priority)}
          {@const picked = priority === TASKS_NEW_PRIORITY}
          <span data-cursor={picked ? "tasks-priority-option" : undefined} class={cn("menu-item relative justify-between", picked && optionHovered("tasks-priority-option", tasksOptionHovered(tasksElapsed, "priority")) && "bg-accent")}>
            <span class="flex min-w-0 items-center gap-1.5 text-foreground">
              <PriorityFlagIcon color={PROJECTS_PRIORITY_COLORS[priority]} theme={theme.current} size={13} class="shrink-0" />
              <span class="min-w-0 truncate">{priorityName(priority)}</span>
            </span>
            {#if priority === newPriority}<Check size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />{/if}
            {#if picked}{@render touch(taskTouches.priorityOption, "2.5rem", "50%")}{/if}
          </span>
        {/each}
      </div>
    {/if}

    {#if part === "defaults"}
      <!-- Project settings, as the header's settings button opens them. They stay laid out while hidden so they can be measured. -->
      <div
        bind:this={settingsPanel}
        class="absolute z-50"
        style:left={`${panel.left}px`}
        style:top={`${panel.top}px`}
        style:width={`${panel.width}px`}
        style:height={`${panel.height}px`}
        style:opacity={settingsShown < 1 ? settingsShown : undefined}
        style:visibility={settingsShown > 0 ? undefined : "hidden"}
      >
        <div class="surface-floating flex h-full min-h-0 flex-col overflow-hidden">
          <div class="flex shrink-0 items-center gap-2 px-3 pt-2 pb-1">
            <span class="min-w-0 flex-1 truncate text-[0.8125rem] font-medium">{t("projects.settings.title")}</span>
            <span class="grid h-8 w-8 shrink-0 place-items-center rounded-md text-muted-foreground"><RotateCcw size={14} /></span>
            <span data-cursor="projects-close" class="relative grid h-8 w-8 shrink-0 place-items-center rounded-md text-muted-foreground">
              <X size={15} />
              {@render touch(touches.close, "50%", "50%")}
            </span>
          </div>

          <div bind:this={settingsViewport} data-cursor="projects-settings-scroll" class="relative min-h-0 flex-1 overflow-hidden">
            <div
              bind:this={settingsContent}
              class="relative flex flex-col gap-3 px-3 pt-1 pb-3"
              style:transform={settingsScrollPx > 0 ? `translateY(${-settingsScrollPx}px)` : undefined}
            >
              <section class="flex flex-col gap-1.5">
                <div class="flex min-h-7 items-center justify-between gap-3 px-1">
                  <h2 class="truncate text-[0.8rem] font-medium text-foreground">{t("projects.settings.identity")}</h2>
                </div>
                <div class="flex flex-col gap-1.5">
                  {@render settingsRow(t("projects.settings.name"), nameControl)}
                  {@render settingsRow(t("projects.settings.group"), groupControl)}
                  {@render settingsRow(t("projects.settings.lifecycle"), lifecycleControl)}
                  {@render settingsRow(t("projects.settings.icon"), iconControl)}
                </div>
              </section>

              <div class="h-px bg-border/70"></div>

              <section class="flex flex-col gap-2">
                <div class="flex items-center justify-between gap-2">
                  <div class="flex min-h-7 items-center gap-2">
                    <h2 class="truncate text-[0.8rem] font-medium text-foreground">{t("projects.settings.members.heading")}</h2>
                    <span class="shrink-0 text-[0.733333rem] text-muted-foreground tabular-nums">1</span>
                  </div>
                  <span class="flex min-h-7 shrink-0 items-center gap-1.5 rounded-md px-1.5 text-[0.733333rem] text-muted-foreground">
                    <MailPlus size={14} strokeWidth={1.75} />{t("projects.settings.members.invite")}
                  </span>
                </div>
                <ul class="-mx-0.5 grid">
                  <MemberRow name={memberName} detail={memberDetail}>
                    {#snippet avatar()}<LocalPersonAvatar size={MEMBER_AVATAR_SIZE} />{/snippet}
                    {#snippet trailing()}<MemberRoleChip label={t("contacts.role.owner")} />{/snippet}
                  </MemberRow>
                </ul>
              </section>

              <div class="h-px bg-border/70"></div>

              {#if !mobileShell}
                <section class="flex flex-col gap-2">
                  <div class="flex items-center justify-between gap-2">
                    <div class="flex min-h-7 items-center justify-between gap-3 px-1">
                      <h2 class="truncate text-[0.8rem] font-medium text-foreground">{t("projects.settings.workingFolders.title")}</h2>
                    </div>
                    <span class="flex min-h-7 shrink-0 items-center gap-1.5 rounded-md px-1.5 text-[0.733333rem] text-muted-foreground">
                      <FolderPlus size={14} strokeWidth={1.75} />{t("projects.settings.workingFolders.addExisting")}
                    </span>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <div class={cn("flex min-w-0 justify-between px-1 py-1", stackedRows ? "flex-col items-stretch gap-2" : "items-center gap-4")}>
                      <div class="flex min-w-0 flex-1 items-center gap-1.5">
                        <span class="block min-w-0 truncate text-[0.866667rem] text-foreground">{t("vaultSetup.projects.project")}</span>
                        <span class="inline-flex size-6 shrink-0 items-center justify-center text-muted-foreground"><Ellipsis strokeWidth={1.75} class="size-3.5" /></span>
                      </div>
                      <span class={cn("min-w-0 shrink-0", stackedRows ? "w-full" : "w-44")}>
                        <span class="flex h-7 w-full items-center justify-between gap-2 rounded-md border border-border px-2.5 font-medium text-foreground">
                          <span class="flex min-w-0 flex-1 items-center gap-1.5">
                            <WandSparkles size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
                            <span class="truncate">{t("projects.settings.workingFolders.providerAutomatic")}</span>
                          </span>
                          {@render chevron(false)}
                        </span>
                      </span>
                    </div>
                  </div>
                </section>

                <div class="h-px bg-border/70"></div>
              {/if}

              <section bind:this={defaultsSection} class="flex flex-col gap-1.5">
                <div class="flex min-h-7 items-center justify-between gap-3 px-1">
                  <h2 class="truncate text-[0.8rem] font-medium text-foreground">{t("projects.settings.defaults")}</h2>
                </div>
                <div class="flex flex-col gap-1.5">
                  {@render settingsRow(t("projects.settings.defaultEventName"), eventNameControl)}
                  {@render settingsRow(t("projects.settings.color"), colorControl)}
                  {@render settingsRow(t("projects.settings.defaultDuration"), durationControl)}
                  {@render settingsRow(t("projects.settings.defaultPomodoro"), pomodoroControl)}
                  {#if idleDetectionAvailable}
                    <div class="flex items-center justify-between gap-4 px-1 py-1">
                      <span class="min-w-0 flex-1 text-[0.866667rem] text-foreground">{t("projects.settings.useGlobalIdleSettings")}</span>
                      <span class="inline-flex h-6 w-10 shrink-0 items-center rounded-full border border-primary bg-primary p-0.5">
                        <span class="block size-4.5 translate-x-4 rounded-full bg-background"></span>
                      </span>
                    </div>
                  {/if}
                  <div class="phase-music">
                    <div class={cn("phase-music-grid", showSoundscapes && "with-sounds")}>
                      {#each PROJECTS_PHASES as phase (phase)}
                        {@const playlist = projectsPlaylist(defaultsElapsed, phase)}
                        <span class="phase-music-label min-w-0 truncate text-[0.866667rem] text-foreground">{phaseLabel(phase)}</span>
                        <div bind:this={playlistTriggers[phase]} data-cursor={`projects-${phase}`} class="relative min-w-0">
                          <span class="flex h-7 w-full min-w-0 items-center gap-2 rounded-md border border-border bg-card px-2.5 text-left text-[0.8rem] font-medium text-foreground dark:bg-transparent">
                            {#if playlist === null}
                              <VolumeX size={14} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />
                            {:else}
                              <span class="grid size-3.5 shrink-0 place-items-center text-muted-foreground"><MusicPlaylistIcon icon={playlistIcon(playlist)} size={14} strokeWidth={1.5} /></span>
                            {/if}
                            <span class="min-w-0 flex-1 truncate">{playlist === null ? t("common.none") : playlistName(playlist)}</span>
                            {@render chevron(openPhase === phase)}
                          </span>
                          {@render touch(touches.playlist[phase], "50%", "50%")}
                        </div>
                        {#if showSoundscapes}
                          <span class="flex h-7 w-full min-w-0 items-center gap-2 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground dark:bg-transparent">
                            <AudioLinesXIcon size={14} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />
                            <span class="min-w-0 flex-1 truncate">{t("common.none")}</span>
                            {@render chevron(false)}
                          </span>
                        {/if}
                      {/each}
                    </div>
                  </div>
                </div>
              </section>

              <div class="h-px bg-border/70"></div>

              <section class="flex flex-col gap-0.5">
                <div class="flex min-h-7 items-center justify-between gap-3 px-1">
                  <h2 class="truncate text-[0.8rem] font-medium text-foreground">{t("projects.settings.tags")}</h2>
                </div>
              </section>

              <div class="h-px bg-border/70"></div>

              <section class="flex flex-col gap-1.5">
                <div class="flex min-h-7 items-center justify-between gap-3 px-1">
                  <h2 class="truncate text-[0.8rem] font-medium text-foreground">{t("projects.customFields.title")}</h2>
                </div>
              </section>

              <div class="h-px bg-border/70"></div>

              <section class="flex flex-col gap-1.5">
                <div class="flex min-h-7 items-center justify-between gap-3 px-1">
                  <h2 class="truncate text-[0.8rem] font-medium text-foreground">{t("projects.settings.taskStatuses")}</h2>
                </div>
                {#each PROJECTS_STATUS_ORDER as status (status)}
                  <div class="flex min-h-7 items-center gap-2 px-1">
                    <ProjectStatusBadge status={sceneStatus(status)} theme={theme.current} label={statusName(status)} class="text-[0.733333rem]" />
                  </div>
                {/each}
              </section>
            </div>
          </div>

          <div class="flex shrink-0 items-center gap-2 px-3 pt-1 pb-2">
            <span class="flex-1"></span>
            <span
              data-cursor="projects-save"
              class={cn(
                "relative flex min-h-8 items-center gap-1.5 rounded-md px-2 text-[0.8rem] font-medium",
                dirty ? "bg-primary text-primary-foreground" : "bg-secondary text-muted-foreground",
              )}
            >
              <Save size={14} strokeWidth={1.75} />{t("projects.settings.save")}
              {@render touch(touches.save, "50%", "50%")}
            </span>
          </div>
        </div>
      </div>

      <!-- The color palette, as the color picker draws it over the settings. It stays laid out while hidden so its size is known. -->
      <div
        bind:this={palette}
        class="surface-floating absolute z-60 grid gap-2 p-2.5"
        style:left={`${palettePlace?.left ?? 0}px`}
        style:top={`${palettePlace?.top ?? 0}px`}
        style:grid-template-columns="repeat(4, 1.375rem)"
        style:background-color={paletteBg}
        style:color={calendarTokens["--cal-time-label"]}
        style:--selection-border={selectionBorder}
        style:visibility={palettePlace ? undefined : "hidden"}
      >
        {#each EVENT_COLOR_OPTIONS as option (option)}
          <span
            data-cursor={option === PROJECTS_COLOR ? "projects-color-option" : undefined}
            class={cn("calendar-color-swatch relative size-5.5 rounded-[3px]", selectedColor === option && "swatch-selected")}
            style:background-color={getEventColor(option, theme.current).bg}
          >
            {#if option === PROJECTS_COLOR}{@render touch(touches.colorOption, "50%", "50%")}{/if}
          </span>
        {/each}
      </div>

      <!-- The Duration and Pomodoro options. Both stay laid out while hidden so their size is known before they open. -->
      <div
        bind:this={durationMenu}
        class="surface-floating absolute z-60 flex flex-col overflow-hidden"
        style:left={`${selectGeometry && picker === "duration" ? selectGeometry.left : 0}px`}
        style:top={`${selectGeometry && picker === "duration" ? selectGeometry.top : 0}px`}
        style:min-width={selectGeometry && picker === "duration" ? `${selectGeometry.minWidth}px` : "max-content"}
        style:max-width={selectGeometry && picker === "duration" ? `${selectGeometry.maxWidth}px` : "max-content"}
        style:max-height={selectGeometry && picker === "duration" ? `${selectGeometry.maxHeight}px` : undefined}
        style:visibility={picker === "duration" ? undefined : "hidden"}
      >
        <div class="surface-floating-body min-h-0 overflow-hidden">
          {#each durationOptions as option (option.value)}
            {@const picked = option.value === PICKED_DURATION}
            <span data-cursor={picked ? "projects-duration-option" : undefined} class={cn("menu-item relative justify-between gap-3", picked && optionHovered("projects-duration-option", projectsOptionHovered(defaultsElapsed, "duration")) && "bg-accent")}>
              <span class="flex min-w-0 flex-1 items-center gap-1.5"><span class="truncate">{option.label}</span></span>
              {#if option.value === durationValue}<Check size={12} strokeWidth={2.5} class="shrink-0" />{/if}
              {#if picked}{@render touch(touches.durationOption, "50%", "50%")}{/if}
            </span>
          {/each}
        </div>
      </div>

      <div
        bind:this={pomodoroMenu}
        class="surface-floating absolute z-60 flex flex-col overflow-hidden"
        style:left={`${selectGeometry && picker === "pomodoro" ? selectGeometry.left : 0}px`}
        style:top={`${selectGeometry && picker === "pomodoro" ? selectGeometry.top : 0}px`}
        style:width={selectGeometry && picker === "pomodoro" && selectGeometry.width !== null ? `${selectGeometry.width}px` : undefined}
        style:min-width={selectGeometry && picker === "pomodoro" ? `${selectGeometry.minWidth}px` : "max-content"}
        style:max-width={selectGeometry && picker === "pomodoro" ? `${selectGeometry.maxWidth}px` : "max-content"}
        style:max-height={selectGeometry && picker === "pomodoro" ? `${selectGeometry.maxHeight}px` : undefined}
        style:visibility={picker === "pomodoro" ? undefined : "hidden"}
      >
        <div class="surface-floating-body min-h-0 overflow-hidden">
          <span class="menu-item justify-between gap-3">
            <span class="grid min-w-0 flex-1 grid-cols-[minmax(max-content,1fr)_max-content] items-center gap-4"><span class="truncate">{t("common.none")}</span></span>
          </span>
          {#each PROJECT_POMODORO_PRESET_ORDER as preset (preset)}
            {@const picked = preset === PROJECTS_PRESET}
            <span data-cursor={picked ? "projects-pomodoro-option" : undefined} class={cn("menu-item relative justify-between gap-3", picked && optionHovered("projects-pomodoro-option", projectsOptionHovered(defaultsElapsed, "pomodoro")) && "bg-accent")}>
              <span class="grid min-w-0 flex-1 grid-cols-[minmax(max-content,1fr)_max-content] items-center gap-4">
                <span class="truncate">{presetLabel(preset)}</span>
                <span class="max-w-72 justify-self-end truncate text-panel-detail text-muted-foreground">{projectPomodoroSummaryLabel(COUNT_PRESET_RHYTHMS[preset])}</span>
              </span>
              {#if picked}{@render touch(touches.pomodoroOption, "50%", "50%")}{/if}
            </span>
          {/each}
          <span class="menu-item justify-between gap-3">
            <span class="grid min-w-0 flex-1 grid-cols-[minmax(max-content,1fr)_max-content] items-center gap-4"><span class="truncate">{t("calendar.pomodoro.custom")}</span></span>
          </span>
        </div>
      </div>

      <!-- The open phase's playlists, as the playlist select lists them. It stays laid out while hidden so its height is known. -->
      <div
        bind:this={playlistMenu}
        class="surface-floating absolute z-60 flex min-h-0 flex-col overflow-hidden"
        style:left={`${playlistGeometry?.left ?? 0}px`}
        style:top={`${playlistGeometry?.top ?? 0}px`}
        style:width={`${playlistGeometry?.width ?? PLAYLIST_POPOVER_W}px`}
        style:max-width={playlistGeometry ? `${playlistGeometry.maxWidth}px` : undefined}
        style:max-height={`${Math.min(playlistGeometry?.maxHeight ?? PLAYLIST_POPOVER_MAX_H, PLAYLIST_POPOVER_MAX_H)}px`}
        style:visibility={playlistGeometry ? undefined : "hidden"}
      >
        <div class="shrink-0 px-1.5 pt-1.5">
          <span class="field flex items-center gap-1.5">
            <Search size={13} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />
            <span class="min-w-0 flex-1 truncate text-muted-foreground">{t("music.assignment.searchPlaylists")}</span>
          </span>
        </div>
        <div class="surface-floating-body min-h-0 flex-1 overflow-hidden">
          <span class="menu-item">
            <VolumeX size={15} strokeWidth={1.75} />
            <span class="min-w-0 flex-1 truncate font-medium">{t("common.none")}</span>
            {#if openPhase && projectsPlaylist(defaultsElapsed, openPhase) === null}<Check size={14} />{/if}
          </span>
          {#each MUSIC_PLAYLISTS as entry (entry.key)}
            {@const picked = openPhase !== null && PROJECTS_PLAYLISTS[openPhase] === entry.key}
            <span data-cursor={picked && openPhase ? `projects-${openPhase}-option` : undefined} class={cn("menu-item relative", picked && openPhase && optionHovered(`projects-${openPhase}-option`, projectsOptionHovered(defaultsElapsed, openPhase)) && "bg-accent")}>
              <span class="grid size-4 shrink-0 place-items-center"><MusicPlaylistIcon icon={entry.icon} size={15} /></span>
              <span class="min-w-0 flex-1 truncate font-medium">{playlistName(entry.key)}</span>
              <span class="shrink-0 text-panel-detail tabular-nums text-muted-foreground">{formatNumber(localization.locale, entry.tracks)}</span>
              {#if picked && openPhase}{@render touch(touches.playlistOption[openPhase], "50%", "50%")}{/if}
            </span>
          {/each}
        </div>
      </div>

      <!-- The event panel, where picking the Novel project gives the drawn event the defaults just saved. -->
      {#if eventPanelOpen}
        <WelcomeFilmEventPanel
          frame={projectsEventPanelFrame(defaultsElapsed, fullEventName, playlistLookup)}
          canvasWidth={canvasW}
          canvasHeight={canvasH}
          titleBarHeight={TOP_BAR_H}
          anchor={panelAnchor}
          date={sceneDate}
          start={PROJECTS_EVENT.start}
          end={PROJECTS_EVENT.start + (eventPicked ? PROJECTS_DEFAULT_MINUTES : PROJECTS_EVENT.drawnEnd - PROJECTS_EVENT.start)}
          projectColor={PROJECTS_COLOR}
          title={fullEventName}
          picker={eventPicker}
          hovered={cursorHovered}
        />
      {/if}
    {/if}

    {#if !mobileShell}
      <WelcomeCursor script={cursorScript} {elapsed} width={canvasW} />
    {/if}
  </div>
</div>

<style>
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

  /* The list's row dividers, as the List view draws them from the content start to just before the right edge. */
  .list-scroll .list-divider {
    position: relative;
  }

  .list-scroll .list-divider::after {
    position: absolute;
    right: 0.25rem;
    bottom: 0;
    left: var(--list-divider-left);
    height: 0;
    border-bottom: 1px solid var(--cal-gridline);
    content: "";
    pointer-events: none;
  }

  /* A focused text field's caret, as a text input draws it. */
  .film-caret {
    display: inline-block;
    width: 1px;
    height: 1.1em;
    margin-left: 1px;
    vertical-align: text-bottom;
    background-color: currentColor;
  }

  /* The picked swatch's outline, as the color picker draws it. */
  .calendar-color-swatch {
    overflow: hidden;
  }

  .calendar-color-swatch.swatch-selected::after {
    position: absolute;
    inset: 0;
    border: 2px solid var(--selection-border);
    border-radius: inherit;
    content: "";
  }

  /* The project's playlists for each phase, as the Event defaults lay them out. */
  .phase-music {
    container-type: inline-size;
  }

  .phase-music-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 11rem;
    align-items: center;
    gap: 0.5rem 1rem;
    padding-inline: 0.25rem;
  }

  .phase-music-grid.with-sounds {
    grid-template-columns: minmax(0, 1fr) 11rem 11rem;
    column-gap: 0.5rem;
  }

  @container (width < 32rem) {
    .phase-music-grid.with-sounds {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .with-sounds .phase-music-label {
      grid-column: 1 / -1;
      margin-top: 0.25rem;
    }
  }

  @container (width < 20rem) {
    .phase-music-grid:not(.with-sounds) {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
