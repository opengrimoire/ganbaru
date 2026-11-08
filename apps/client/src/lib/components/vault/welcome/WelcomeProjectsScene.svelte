<script lang="ts" module>
  import { calendarZoomHourHeightForPercent } from "$lib/calendar/zoom-levels";

  /** The Calendar zoom the scene's day is drawn at, as the other welcome scenes draw it. */
  const SCENE_ZOOM_PERCENT = 200;
  const HOUR_PX = calendarZoomHourHeightForPercent(SCENE_ZOOM_PERCENT);

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
</script>

<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ArrowUpDown from "@lucide/svelte/icons/arrow-up-down";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import GraduationCap from "@lucide/svelte/icons/graduation-cap";
  import Layers from "@lucide/svelte/icons/layers";
  import List from "@lucide/svelte/icons/list";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import MailPlus from "@lucide/svelte/icons/mail-plus";
  import Minus from "@lucide/svelte/icons/minus";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import Plus from "@lucide/svelte/icons/plus";
  import Repeat from "@lucide/svelte/icons/repeat";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Save from "@lucide/svelte/icons/save";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Users from "@lucide/svelte/icons/users";
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
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import PriorityFlagIcon from "$lib/components/projects/PriorityFlagIcon.svelte";
  import ProjectStatusBadge from "$lib/components/projects/ProjectStatusBadge.svelte";
  import { projectListColumnKind } from "$lib/components/projects/list/property-kinds";
  import WorkspaceBreadcrumbTerminalIcon from "$lib/components/ui/WorkspaceBreadcrumbTerminalIcon.svelte";
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
  import { cn } from "$lib/utils";
  import { pickSelectPopoverGeometry, type SelectPopoverRect } from "$lib/utils/select-popover-position";
  import { easeInOutCubic, filmSpan, filmWorkday, type FilmSpan } from "$lib/vault/welcome-film";
  import { MUSIC_PLAYLISTS, MUSIC_PROJECT_PLAYLISTS, type MusicScenePlaylist } from "$lib/vault/welcome-music-scene";
  import {
    PROJECTS_BEATS,
    PROJECTS_DEFAULT_MINUTES,
    PROJECTS_EVENTS,
    PROJECTS_PRESET,
    PROJECTS_PRIORITY_COLORS,
    PROJECTS_SCHEDULE_START,
    PROJECTS_STATUS_COLORS,
    PROJECTS_TASKS,
    projectsCalendarShown,
    projectsClock,
    projectsDefaultMinutes,
    projectsDefaultPreset,
    projectsEventIn,
    projectsEventRail,
    projectsFocusing,
    projectsListShown,
    projectsMinutesBump,
    projectsPhaseSeconds,
    projectsRailRecorded,
    projectsRowCheckboxShown,
    projectsRowChecked,
    projectsRowTouch,
    projectsScheduleOpen,
    projectsScheduleTouches,
    projectsSelectedCount,
    projectsSettingsDirty,
    projectsSettingsOpen,
    projectsSettingsPicker,
    projectsSettingsScroll,
    projectsSettingsShown,
    projectsSettingsTouches,
    type ProjectsScenePriority,
    type ProjectsSceneStatus,
  } from "$lib/vault/welcome-projects-scene";

  /**
   * One frame of the welcome scene on Projects, drawn with the app's own surfaces. The Thesis project's List view shows
   * its tasks, and Project settings open from the header, scroll to Event defaults, take a duration and a Pomodoro
   * beside the color and playlists already set, and are saved. Back in the list, three tasks are checked, and the
   * selection bar's Schedule form, whose minutes already read the project's duration, schedules them. Then the
   * Calendar's day, as the Calendar film draws it, receives three events that each carry the project's color, rhythm,
   * and music, and the first one's Focus starts on its own. Every style is a function of `elapsed`, with no CSS
   * transitions.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

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
  const CALENDAR_HEADER_H = 40;
  const GUTTER_W = GUTTER_WIDTH_PER_TIMEZONE;
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
  /** Room kept under the playlists once the settings scroll on to them. */
  const MUSIC_SCROLL_PAD = 12;
  /** The project header's end padding (`px-3`), so the settings button's right edge is known. */
  const HEADER_END_PX = 12;
  /** The selection bar's height (`min-h-8`) and its gap above the list (`mb-1.5`), as the bar floats over the header. */
  const BAR_H = 32;
  const BAR_GAP = 6;
  const BAR_INSET = 12;
  /** A selection bar menu trigger, as the bulk action bar styles it. */
  const BAR_SEGMENT_CLASS = "inline-flex h-8 min-w-0 shrink-0 items-center gap-1.5 px-2.5 text-collection font-normal whitespace-nowrap text-muted-foreground";
  /** The Schedule menu's width (`FLOATING_WIDTH.sm`) and its gap below the bar. */
  const SCHEDULE_MENU_W = 240;
  const SCHEDULE_MENU_GAP = 6;
  /** How long a picked option shows its highlight before the tap lands, as the pointer rests on it. */
  const OPTION_HOVER_MS = 420;
  /** How much the Schedule form's minutes lean toward the primary color at the peak of their bump. */
  const BUMP_TINT = 70;

  /** The Calendar's Focus rail, and the event area it keeps beside it on every event. */
  const RAIL_LEFT = 2;
  const RAIL_W = 6;
  const EVENT_LEFT = RAIL_W + 4;
  const EVENT_GAP_PX = 2;
  const EVENT_TIME_MIN_PX = 32;
  const EVENT_ICONS_MIN_PX = 17;
  /** How far a scheduled event drops in from as it lands on the day. */
  const EVENT_DROP_PX = 8;
  /** Where the visible part of the day starts, as if the grid were scrolled so the scheduled events sit near the top. */
  const VIEW_START = 9 * 60 + 45;

  /** Palette slots, so the drawing follows the active theme's event colors, as the Calendar film picks them. */
  const LAVENDER = 20;
  const EATING = 13;
  const EXERCISE = 0;
  /** The Thesis project's color, the eighth swatch of the color picker. */
  const THESIS = 7;

  type SceneTitle = "lunch" | "teamMeeting" | "gym";
  type SceneIcon = "music" | "repeat" | "meeting";
  interface SceneEvent {
    start: number;
    end: number;
    color: EventColor;
    title: SceneTitle;
    icons: readonly SceneIcon[];
  }

  const ICON_ORDER: readonly SceneIcon[] = ["music", "repeat", "meeting"];
  /** The rest of the day after the scheduled events. */
  const DAY_EVENTS: readonly SceneEvent[] = [
    { start: 13 * 60 + 30, end: 14 * 60 + 30, color: EATING, title: "lunch", icons: ["repeat"] },
    { start: 15 * 60, end: 16 * 60, color: LAVENDER, title: "teamMeeting", icons: ["repeat", "meeting"] },
    { start: 17 * 60, end: 18 * 60, color: EXERCISE, title: "gym", icons: ["repeat"] },
  ];

  /** Each status the scene shows, with the category a new project seeds it with. */
  const STATUS_CATEGORIES: Readonly<Record<ProjectsSceneStatus, ProjectStatusCategory>> = {
    backlog: "not_started",
    todo: "not_started",
    inProgress: "active",
    done: "done",
  };

  /** The duration options of Event defaults, as the settings list them, and the one the scene picks. */
  type DurationOption = { value: string; label: string };
  const PICKED_DURATION = String(PROJECTS_DEFAULT_MINUTES);

  /** A trigger, a viewport, or a section measured within the settings, in logical pixels. */
  interface Box {
    top: number;
    left: number;
    width: number;
    height: number;
  }

  /** Where the settings' parts sit, measured once the panel lays itself out for the canvas. */
  interface SettingsLayout {
    viewport: Box;
    contentHeight: number;
    defaultsTop: number;
    playlistsBottom: number;
    duration: Box;
    pomodoro: Box;
    durationMenuHeight: number;
    pomodoroMenuHeight: number;
    durationMenuWidth: number;
    pomodoroMenuWidth: number;
  }

  /** Where the selection bar's parts sit, measured once it lays itself out for the canvas. */
  interface BarLayout {
    innerWidth: number;
    scheduleLeft: number;
    scheduleRight: number;
  }

  const EMPTY_BOX: Box = { top: 0, left: 0, width: 0, height: 0 };

  const now = new Date();
  const sceneDate = filmWorkday(now);
  const timezone = getLocalTimezone();
  const timezoneLabel = getTimezoneInfo(timezone).acronym;
  const today = isoDay(0);

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

  const listShown = $derived(projectsListShown(elapsed));
  const calendarShown = $derived(projectsCalendarShown(elapsed));
  const settingsShown = $derived(projectsSettingsShown(elapsed));
  const settingsOpen = $derived(projectsSettingsOpen(elapsed));
  const settingsScroll = $derived(projectsSettingsScroll(elapsed));
  const settingsTouches = $derived(projectsSettingsTouches(elapsed));
  const picker = $derived(projectsSettingsPicker(elapsed));
  const defaultMinutes = $derived(projectsDefaultMinutes(elapsed));
  const defaultPreset = $derived(projectsDefaultPreset(elapsed));
  const dirty = $derived(projectsSettingsDirty(elapsed));
  const selectedCount = $derived(projectsSelectedCount(elapsed));
  const scheduleOpen = $derived(projectsScheduleOpen(elapsed));
  const scheduleTouches = $derived(projectsScheduleTouches(elapsed));
  const minutesBump = $derived(projectsMinutesBump(elapsed));
  /** The selection bar shows from the first check until the list gives way. */
  const barShown = $derived(selectedCount > 0 && elapsed < PROJECTS_BEATS.listOut);
  /** The pointer scrolls a narrow selection bar until Schedule is in view, after the last check and before its tap. */
  const barReveal = $derived(
    easeInOutCubic(filmSpan(elapsed, PROJECTS_BEATS.selectThirdTap + 400, PROJECTS_BEATS.scheduleTap - 400)),
  );

  const clock = $derived(projectsClock(elapsed));
  const focusing = $derived(projectsFocusing(elapsed));
  const phaseSeconds = $derived(projectsPhaseSeconds(elapsed));
  const rail = $derived(projectsEventRail(elapsed));
  const recorded = $derived(projectsRailRecorded(elapsed));

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

  let settingsViewport = $state<HTMLDivElement>();
  let settingsContent = $state<HTMLDivElement>();
  let defaultsSection = $state<HTMLElement>();
  let playlistRows = $state<HTMLDivElement>();
  let durationTrigger = $state<HTMLDivElement>();
  let pomodoroTrigger = $state<HTMLDivElement>();
  let durationMenu = $state<HTMLDivElement>();
  let pomodoroMenu = $state<HTMLDivElement>();
  let barInner = $state<HTMLDivElement>();
  let scheduleSegment = $state<HTMLSpanElement>();
  let settingsLayout = $state<SettingsLayout | null>(null);
  let barLayout = $state<BarLayout | null>(null);

  /** Measure the settings and the selection bar once they lay themselves out for the canvas and the language. */
  $effect(() => {
    void canvasW;
    void canvasH;
    void stackedRows;
    void localization.locale;
    if (settingsViewport && settingsContent && defaultsSection && playlistRows && durationTrigger && pomodoroTrigger && durationMenu && pomodoroMenu) {
      settingsLayout = {
        viewport: boxOf(settingsViewport),
        contentHeight: settingsContent.offsetHeight,
        defaultsTop: defaultsSection.offsetTop,
        playlistsBottom: playlistRows.offsetTop + playlistRows.offsetHeight,
        duration: boxOf(durationTrigger),
        pomodoro: boxOf(pomodoroTrigger),
        durationMenuHeight: durationMenu.offsetHeight,
        pomodoroMenuHeight: pomodoroMenu.offsetHeight,
        durationMenuWidth: durationMenu.scrollWidth,
        pomodoroMenuWidth: pomodoroMenu.scrollWidth,
      };
    }
    if (barInner && scheduleSegment) {
      barLayout = {
        innerWidth: barInner.clientWidth,
        scheduleLeft: scheduleSegment.offsetLeft,
        scheduleRight: scheduleSegment.offsetLeft + scheduleSegment.offsetWidth,
      };
    }
  });

  /** How far the settings have scrolled: to the Defaults heading, then on until the playlists are in view. */
  const settingsScrollPx = $derived.by(() => {
    if (!settingsLayout) return 0;
    const maxScroll = Math.max(0, settingsLayout.contentHeight - settingsLayout.viewport.height);
    const defaultsTarget = Math.min(maxScroll, Math.max(0, settingsLayout.defaultsTop - DEFAULTS_SCROLL_PAD));
    const musicTarget = Math.min(
      maxScroll,
      Math.max(defaultsTarget, settingsLayout.playlistsBottom + MUSIC_SCROLL_PAD - settingsLayout.viewport.height),
    );
    return defaultsTarget * settingsScroll.defaults + (musicTarget - defaultsTarget) * settingsScroll.music;
  });

  /** The open picker's popover, placed as the settings' select places it against the canvas. */
  const pickerGeometry = $derived.by(() => {
    if (!settingsLayout || !picker) return null;
    const trigger = picker === "duration" ? settingsLayout.duration : settingsLayout.pomodoro;
    const top = panel.top + settingsLayout.viewport.top + trigger.top - settingsScrollPx;
    const left = panel.left + settingsLayout.viewport.left + trigger.left;
    const triggerRect: SelectPopoverRect = {
      top,
      left,
      right: left + trigger.width,
      bottom: top + trigger.height,
      width: trigger.width,
      height: trigger.height,
    };
    const boundaryRect: SelectPopoverRect = { top: 0, left: 0, right: canvasW, bottom: canvasH, width: canvasW, height: canvasH };
    return pickSelectPopoverGeometry({
      triggerRect,
      boundaryRect,
      contentHeight: picker === "duration" ? settingsLayout.durationMenuHeight : settingsLayout.pomodoroMenuHeight,
      contentWidth: picker === "duration" ? settingsLayout.durationMenuWidth : settingsLayout.pomodoroMenuWidth,
      horizontalAlign: picker === "duration" ? "start" : "end",
    });
  });

  /** How far the narrow selection bar has scrolled to bring Schedule into view. */
  const barScroll = $derived(barLayout ? Math.max(0, barLayout.scheduleRight - barLayout.innerWidth) * barReveal : 0);
  /** The Schedule menu's left edge, below the Schedule segment and kept off the canvas edges. */
  const scheduleMenuLeft = $derived.by(() => {
    const anchor = BAR_INSET + (barLayout?.scheduleLeft ?? 0) - barScroll;
    return Math.min(Math.max(anchor, PANEL_EDGE), canvasW - PANEL_EDGE - SCHEDULE_MENU_W);
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
  const durationHovered = $derived(picker === "duration" && elapsed >= PROJECTS_BEATS.durationPick - OPTION_HOVER_MS);
  const pomodoroHovered = $derived(picker === "pomodoro" && elapsed >= PROJECTS_BEATS.pomodoroPick - OPTION_HOVER_MS);

  const viewMinutes = $derived(((canvasH - TOP_BAR_H - CALENDAR_HEADER_H) / HOUR_PX) * 60);
  const hours = $derived(
    Array.from({ length: Math.ceil(viewMinutes / 60) + 1 }, (_, index) => Math.ceil(VIEW_START / 60) + index)
      .filter((hour) => hour * 60 > VIEW_START && hour * 60 < VIEW_START + viewMinutes),
  );
  const scrollThumb = $derived.by(() => {
    const visibleHeight = canvasH - TOP_BAR_H - CALENDAR_HEADER_H;
    return calendarScrollbarThumb(
      { scrollTop: (VIEW_START / 60) * HOUR_PX, scrollHeight: 24 * HOUR_PX, clientHeight: visibleHeight },
      visibleHeight,
    );
  });
  const dayFormat = $derived(dayNameFormatForWidth(canvasW - GUTTER_W));
  const personName = $derived(preferences.profileDisplayName || t("projects.people.you"));
  /** The project's only member, as the members section names the person using this device. */
  const memberName = $derived(preferences.profileDisplayName.trim() || t("contacts.you"));
  const memberDetail = $derived(preferences.profileDisplayName.trim() ? t("contacts.you") : null);

  /** An element's place within its positioned parent. */
  function boxOf(element: HTMLElement): Box {
    return element ? { top: element.offsetTop, left: element.offsetLeft, width: element.offsetWidth, height: element.offsetHeight } : EMPTY_BOX;
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
      projectId: "scene-thesis",
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

  function phaseLabel(phase: "focus" | "short-break" | "long-break"): string {
    if (showSoundscapes) return t(`music.assignment.phase.${phase}`);
    if (phase === "focus") return t("projects.settings.focusPlaylist");
    return phase === "short-break" ? t("projects.settings.shortBreakPlaylist") : t("projects.settings.longBreakPlaylist");
  }

  function title(key: SceneTitle): string {
    switch (key) {
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
  {#if opacity > 0}
    <span
      class="pointer-events-none absolute z-20 size-6 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35"
      style:left
      style:top
      style:opacity
    ></span>
  {/if}
{/snippet}

{#snippet checkbox(state: "checked" | "partial" | "unchecked")}
  <span
    class={cn(
      "grid size-4 shrink-0 place-items-center rounded-sm border",
      state === "unchecked" ? "border-foreground/25 text-transparent" : "border-primary bg-primary text-primary-foreground",
    )}
    aria-hidden="true"
  >
    {#if state === "checked"}<Check size={11} strokeWidth={2.5} />{:else if state === "partial"}<Minus size={11} strokeWidth={2.5} />{/if}
  </span>
{/snippet}

{#snippet chevron(open: boolean)}
  <ChevronDown size={13} strokeWidth={2} class={cn("shrink-0 text-muted-foreground", open && "rotate-180")} />
{/snippet}

<!-- A labeled settings row, as the settings' fields and selects lay themselves out. -->
{#snippet settingsRow(label: string, control: import("svelte").Snippet)}
  <div class={cn("flex justify-between px-1 py-1", stackedRows ? "flex-col items-stretch gap-2" : "items-center gap-4")}>
    <span class="min-w-0 flex-1 text-[0.866667rem] text-foreground">{label}</span>
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
  <span class={cn("field flex min-w-0 items-center px-2.5 font-medium text-foreground", stackedRows ? "w-full" : "w-44")}>{t("vaultSetup.film.thesisProject")}</span>
{/snippet}

{#snippet groupControl()}
  <span class={cn("relative min-w-0", stackedRows ? "w-full" : "w-44")}>{@render selectTrigger(t("vaultSetup.film.universityGroup"), false)}</span>
{/snippet}

{#snippet lifecycleControl()}
  <span class={cn("relative min-w-0", stackedRows ? "w-full" : "w-44")}>{@render selectTrigger(t("projects.lifecycle.active"), false)}</span>
{/snippet}

{#snippet iconControl()}
  <span class={cn("flex h-7 min-w-0 items-center gap-2 rounded-md border border-border px-2.5 text-foreground", stackedRows ? "w-full" : "w-44")}>
    <BookOpen size={14} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
    <span class="min-w-0 flex-1"></span>
    {@render chevron(false)}
  </span>
{/snippet}

{#snippet eventNameControl()}
  <span class={cn("field flex min-w-0 items-center px-2.5 font-medium text-muted-foreground", stackedRows ? "w-full" : "w-44")}>{t("common.none")}</span>
{/snippet}

{#snippet colorControl()}
  <span class={cn("min-w-0", stackedRows ? "w-full" : "w-44")}>
    <span class="flex h-7 w-full items-center justify-between gap-2 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium dark:bg-transparent">
      <span class="flex min-w-0 items-center gap-2">
        <span class="h-3.5 w-3.5 shrink-0 rounded-[3px]" style:background-color={getEventColor(THESIS, theme.current).bg}></span>
        <span class="truncate">{t("calendar.color.eventColorNumber", THESIS + 1)}</span>
      </span>
      <ChevronDown size={13} class="shrink-0 text-muted-foreground" />
    </span>
  </span>
{/snippet}

{#snippet durationControl()}
  <div bind:this={durationTrigger} class={cn("relative min-w-0", stackedRows ? "w-full" : "w-44")}>
    {@render selectTrigger(durationLabel, picker === "duration")}
    {@render touch(settingsTouches.duration, "50%", "50%")}
  </div>
{/snippet}

{#snippet pomodoroControl()}
  <div bind:this={pomodoroTrigger} class={cn("relative min-w-0", stackedRows ? "w-full" : "w-44")}>
    {@render selectTrigger(pomodoroLabel, picker === "pomodoro")}
    {@render touch(settingsTouches.pomodoro, "50%", "50%")}
  </div>
{/snippet}

{#snippet eventBlock(
  start: number,
  end: number,
  color: EventColor | undefined,
  label: string,
  past: boolean,
  eventBelow: boolean,
  icons: readonly SceneIcon[],
  landed: number,
)}
  {@const colors = eventColors(color, past)}
  {@const shownIcons = px(end - start) >= EVENT_ICONS_MIN_PX ? ICON_ORDER.filter((icon) => icons.includes(icon)) : []}
  <div
    class="absolute left-0 z-3 flex overflow-hidden rounded text-[0.8rem] leading-tight"
    style:width={`${100 - CALENDAR_EVENT_RIGHT_GUTTER_PCT}%`}
    style:top={`${y(start)}px`}
    style:height={`${px(end - start) - (eventBelow ? EVENT_GAP_PX : 0)}px`}
    style:background-color={colors.bg}
    style:color={colors.text}
    style:opacity={landed < 1 ? landed : undefined}
    style:transform={landed < 1 ? `translateY(${(landed - 1) * EVENT_DROP_PX}px)` : undefined}
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

{#snippet focusRail(start: number, end: number, focus: readonly FilmSpan[], breaks: readonly FilmSpan[])}
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
      <!-- The phone's top bar, with the Thesis project's context switcher and the Focus ring. -->
      <div class="flex shrink-0 items-center justify-between gap-2 bg-background px-1.5" style:height={`${TOP_BAR_H}px`}>
        <span class="flex h-9 min-w-0 items-center gap-2 rounded-full bg-accent/55 pr-2.5 pl-2 text-identity font-medium">
          <span class="grid size-5 shrink-0 place-items-center"><BookOpen size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} /></span>
          <span class="truncate">{t("vaultSetup.film.thesisProject")}</span>
          <ChevronDown size={16} class="shrink-0 text-muted-foreground" />
        </span>
        <span class={cn("relative grid size-8 shrink-0 place-items-center rounded-full text-muted-foreground", focusing && "bg-accent/70 text-foreground")}>
          {@render ring(20, "stroke-muted-foreground/35", "text-foreground/76 stroke-foreground/76")}
        </span>
      </div>
    {:else}
      <!-- The title bar's Focus ring and Music. -->
      <div class="flex shrink-0 items-center justify-end gap-0.5 bg-sidebar" style:height={`${TOP_BAR_H}px`} style:padding-right={`${TITLE_BAR_END_PX}px`}>
        <span class="relative grid size-7 place-items-center rounded-lg">
          {@render ring(
            TITLE_ICON_SIZE + 0.5,
            "stroke-foreground/20 dark:stroke-white/20",
            "text-foreground/68 dark:text-white/76 stroke-foreground/68 dark:stroke-white/76",
          )}
        </span>
        <span class={cn("grid size-7 place-items-center rounded-lg", TITLE_ICON_COLOR_CLASS)}>
          <Music size={TITLE_ICON_SIZE} strokeWidth={TITLE_ICON_STROKE_WIDTH} />
        </span>
      </div>
    {/if}

    <div class="relative min-h-0 flex-1">
      <!-- The Thesis project's List view. -->
      {#if listShown > 0}
        <div class="absolute inset-0 flex flex-col bg-(--cal-bg)" style:opacity={listShown < 1 ? listShown : undefined}>
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
                {@render touch(settingsTouches.settings, "50%", "50%")}
              </span>
            </div>
          {:else}
            <div class="flex shrink-0 items-center gap-1 px-3" style:height={`${HEADER_H}px`}>
              <span class="flex h-7 min-w-0 items-center gap-0.5 overflow-hidden text-identity font-medium">
                <span class="flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5">
                  <GraduationCap size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
                  <span class="truncate">{t("vaultSetup.film.universityGroup")}</span>
                </span>
                <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
                <span class="flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5">
                  <BookOpen size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
                  <span class="truncate">{t("vaultSetup.film.thesisProject")}</span>
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
                <span class={cn("relative flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-foreground", settingsOpen && "bg-accent")}>
                  <Settings2 size={TOOL_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} />
                  {@render touch(settingsTouches.settings, "50%", "50%")}
                </span>
              </span>
            </div>
          {/if}

          <div class="list-scroll relative min-h-0 flex-1 overflow-hidden" style:--list-divider-left={contentStart}>
            <div class="flex flex-col p-3">
              <div class="list-divider grid min-h-(--collection-table-row-height) items-center px-1" style:grid-template-columns={gridTemplate} style:min-width={gridMinWidth}>
                <div class="grid min-h-(--collection-table-row-height) items-center" style:grid-column="1 / span 3" style:grid-template-columns={leadingGridTemplate}>
                  <span class="flex h-7 items-center justify-center">
                    {#if mobileShell}
                      {@render checkbox(selectedCount === 0 ? "unchecked" : selectedCount === PROJECTS_TASKS.length ? "checked" : "partial")}
                    {/if}
                  </span>
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

              {#each PROJECTS_TASKS as task, row (task.key)}
                {@const checked = projectsRowChecked(elapsed, row)}
                {@const boxShown = mobileShell || projectsRowCheckboxShown(elapsed, row)}
                <div class="list-divider relative" style:min-width={gridMinWidth}>
                  <CollectionRow template={gridTemplate} minWidth={gridMinWidth} {gutter} divider={false} selected={checked}>
                    <div class="relative flex h-full items-center justify-center">
                      {#if boxShown}{@render checkbox(checked ? "checked" : "unchecked")}{/if}
                      {@render touch(projectsRowTouch(elapsed, row), "50%", "50%")}
                    </div>
                    <div></div>
                    <CollectionCell>
                      <span class="flex h-7 min-w-0 items-center"><span class="truncate text-foreground">{t(`vaultSetup.projects.task.${task.key}`)}</span></span>
                    </CollectionCell>
                    {#each taskColumns as column (column)}
                      <CollectionCell>
                        {#if column === "status"}
                          <ProjectStatusBadge status={sceneStatus(task.status)} theme={theme.current} label={statusName(task.status)} class="text-[0.733333rem]" />
                        {:else if column === "priority"}
                          <span class="flex min-w-0 items-center gap-1.5 text-foreground">
                            <PriorityFlagIcon color={PROJECTS_PRIORITY_COLORS[task.priority]} theme={theme.current} size={13} class="shrink-0" />
                            <span class="truncate">{priorityName(task.priority)}</span>
                          </span>
                        {:else if column === "assignee" || column === "reviewer"}
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
                    {/each}
                    <div></div>
                  </CollectionRow>
                </div>
              {/each}
            </div>
          </div>
        </div>
      {/if}

      <!-- The Calendar's day, as the Calendar film draws it, where the scheduled tasks land as events. -->
      {#if calendarShown > 0}
        <div class="absolute inset-0 flex flex-col bg-(--cal-bg)" style:opacity={calendarShown < 1 ? calendarShown : undefined}>
          <div class="flex shrink-0 border-b border-sidebar" style:height={`${CALENDAR_HEADER_H}px`}>
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
                {@render focusRail(rail.start, rail.end, recorded, rail.breaks)}
              {/if}

              <div class="absolute inset-y-0 right-0" style:left={`${EVENT_LEFT}px`}>
                {#each DAY_EVENTS as event (event.start)}
                  {@render eventBlock(event.start, event.end, event.color, title(event.title), event.end <= clock, false, event.icons, 1)}
                {/each}
                {#each PROJECTS_EVENTS as event, index (event.row)}
                  {@const landed = projectsEventIn(elapsed, index)}
                  {#if landed > 0}
                    {@render eventBlock(
                      event.start,
                      event.end,
                      THESIS,
                      t(`vaultSetup.projects.task.${PROJECTS_TASKS[event.row].key}`),
                      false,
                      PROJECTS_EVENTS.some((other) => other.start === event.end),
                      ["music"],
                      landed,
                    )}
                  {/if}
                {/each}
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

    <!-- The selection bar, floating over the bottom of the header from the first check. -->
    <div
      class="absolute flex flex-col items-start"
      style:left={`${BAR_INSET}px`}
      style:right={`${BAR_INSET}px`}
      style:bottom={`${canvasH - listTop + BAR_GAP}px`}
      style:visibility={barShown ? undefined : "hidden"}
      style:z-index="30"
    >
      <div class="surface-floating flex min-w-0 overflow-hidden" style:max-width="100%" style:min-height={`${BAR_H}px`}>
        <div bind:this={barInner} class="flex min-w-0 overflow-hidden">
          <div class="flex min-w-0 shrink-0 items-stretch divide-x divide-border" style:transform={barScroll > 0 ? `translateX(${-barScroll}px)` : undefined}>
            <span class="flex min-h-8 shrink-0 items-center px-2.5 font-medium whitespace-nowrap text-primary">{t("collections.selection.selected", Math.max(1, selectedCount))}</span>
            <span class={BAR_SEGMENT_CLASS}>
              <CollectionPropertyIcon kind="status" class="size-3.5 shrink-0 text-muted-foreground" /><span class="truncate">{t("projects.list.status")}</span>
            </span>
            <span class={BAR_SEGMENT_CLASS}>
              <CollectionPropertyIcon kind="select" class="size-3.5 shrink-0 text-muted-foreground" /><span class="truncate">{t("projects.list.priority")}</span>
            </span>
            <span
              bind:this={scheduleSegment}
              class={cn("relative", BAR_SEGMENT_CLASS, scheduleOpen && "bg-accent text-foreground")}
            >
              <CalendarDays strokeWidth={1.75} class="size-3.5 shrink-0 text-muted-foreground" /><span class="truncate">{t("projects.bulk.schedule")}</span>
              {@render touch(scheduleTouches.schedule, "50%", "50%")}
            </span>
            <span class="flex h-8 w-8 shrink-0 items-center justify-center text-collection text-muted-foreground"><Archive strokeWidth={1.75} class="size-3.5 shrink-0" /></span>
            <span class={BAR_SEGMENT_CLASS}><Ellipsis strokeWidth={1.75} class="size-3.5 shrink-0 text-muted-foreground" /></span>
          </div>
        </div>
      </div>
    </div>

    <!-- The Schedule form, whose minutes already read the project's default duration. -->
    {#if scheduleOpen}
      <div
        class="surface-floating absolute z-40 flex flex-col overflow-hidden"
        style:left={`${scheduleMenuLeft}px`}
        style:top={`${listTop - BAR_GAP + SCHEDULE_MENU_GAP}px`}
        style:width={`${SCHEDULE_MENU_W}px`}
      >
        <div class="p-1.5">
          <div class="grid gap-2 p-1">
            <p class="text-muted-foreground">{t("projects.bulk.scheduleHint", PROJECTS_EVENTS.length)}</p>
            <span class="grid gap-1 text-muted-foreground">
              {t("projects.schedule.date")}
              <span class="field flex min-w-0 items-center text-foreground">{today}</span>
            </span>
            <span class="grid grid-cols-2 gap-2">
              <span class="grid gap-1 text-muted-foreground">
                {t("projects.schedule.start")}
                <span class="field flex min-w-0 items-center text-foreground">{clockLabel(PROJECTS_SCHEDULE_START)}</span>
              </span>
              <span class="grid gap-1 text-muted-foreground">
                {t("projects.schedule.duration")}
                <span
                  class="field flex min-w-0 items-center text-foreground"
                  style:color={minutesBump > 0 ? `color-mix(in srgb, var(--primary) ${minutesBump * BUMP_TINT}%, var(--foreground))` : undefined}
                  style:border-color={minutesBump > 0 ? `color-mix(in srgb, var(--primary) ${minutesBump * BUMP_TINT}%, var(--border))` : undefined}
                >{PROJECTS_DEFAULT_MINUTES}</span>
              </span>
            </span>
            <span class="relative flex h-(--panel-row-height) items-center justify-center rounded-md bg-primary px-2 font-medium whitespace-nowrap text-primary-foreground">
              {t("projects.bulk.scheduleSelected")}
              {@render touch(scheduleTouches.submit, "50%", "50%")}
            </span>
          </div>
        </div>
      </div>
    {/if}

    <!-- Project settings, as the header's settings button opens them. They stay laid out while hidden so they can be measured. -->
    <div
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
          <span class="grid h-8 w-8 shrink-0 place-items-center rounded-md text-muted-foreground"><X size={15} /></span>
        </div>

        <div bind:this={settingsViewport} class="relative min-h-0 flex-1 overflow-hidden">
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
                      <span class="block min-w-0 truncate text-[0.866667rem] text-foreground">{t("vaultSetup.film.thesisProject")}</span>
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
                <div bind:this={playlistRows} class="phase-music">
                  <div class={cn("phase-music-grid", showSoundscapes && "with-sounds")}>
                    {#each MUSIC_PROJECT_PLAYLISTS as assignment (assignment.phase)}
                      <span class="phase-music-label min-w-0 truncate text-[0.866667rem] text-foreground">{phaseLabel(assignment.phase)}</span>
                      <span class="flex h-7 w-full min-w-0 items-center gap-2 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground dark:bg-transparent">
                        <span class="grid size-3.5 shrink-0 place-items-center text-muted-foreground"><MusicPlaylistIcon icon={playlistIcon(assignment.playlist)} size={14} strokeWidth={1.5} /></span>
                        <span class="min-w-0 flex-1 truncate">{playlistName(assignment.playlist)}</span>
                        <ChevronDown size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
                      </span>
                      {#if showSoundscapes}
                        <span class="flex h-7 w-full min-w-0 items-center gap-2 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground dark:bg-transparent">
                          <AudioLinesXIcon size={14} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />
                          <span class="min-w-0 flex-1 truncate">{t("common.none")}</span>
                          <ChevronDown size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
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
              {#each ["backlog", "todo", "inProgress", "done"] as const as status (status)}
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
            class={cn(
              "relative flex min-h-8 items-center gap-1.5 rounded-md px-2 text-[0.8rem] font-medium",
              dirty ? "bg-primary text-primary-foreground" : "bg-secondary text-muted-foreground",
            )}
          >
            <Save size={14} strokeWidth={1.75} />{t("projects.settings.save")}
            {@render touch(settingsTouches.save, "50%", "50%")}
          </span>
        </div>
      </div>
    </div>

    <!-- The Duration and Pomodoro options. Both stay laid out while hidden so their size is known before they open. -->
    <div
      bind:this={durationMenu}
      class="surface-floating absolute z-60 flex flex-col overflow-hidden"
      style:left={`${pickerGeometry && picker === "duration" ? pickerGeometry.left : 0}px`}
      style:top={`${pickerGeometry && picker === "duration" ? pickerGeometry.top : 0}px`}
      style:min-width={pickerGeometry && picker === "duration" ? `${pickerGeometry.minWidth}px` : "max-content"}
      style:max-width={pickerGeometry && picker === "duration" ? `${pickerGeometry.maxWidth}px` : "max-content"}
      style:max-height={pickerGeometry && picker === "duration" ? `${pickerGeometry.maxHeight}px` : undefined}
      style:visibility={picker === "duration" ? undefined : "hidden"}
    >
      <div class="surface-floating-body min-h-0 overflow-hidden">
        {#each durationOptions as option (option.value)}
          {@const picked = option.value === PICKED_DURATION}
          <span class={cn("menu-item relative justify-between gap-3", picked && durationHovered && "bg-accent")}>
            <span class="flex min-w-0 flex-1 items-center gap-1.5"><span class="truncate">{option.label}</span></span>
            {#if option.value === durationValue}<Check size={12} strokeWidth={2.5} class="shrink-0" />{/if}
            {#if picked}{@render touch(settingsTouches.durationOption, "50%", "50%")}{/if}
          </span>
        {/each}
      </div>
    </div>

    <div
      bind:this={pomodoroMenu}
      class="surface-floating absolute z-60 flex flex-col overflow-hidden"
      style:left={`${pickerGeometry && picker === "pomodoro" ? pickerGeometry.left : 0}px`}
      style:top={`${pickerGeometry && picker === "pomodoro" ? pickerGeometry.top : 0}px`}
      style:width={pickerGeometry && picker === "pomodoro" && pickerGeometry.width !== null ? `${pickerGeometry.width}px` : undefined}
      style:min-width={pickerGeometry && picker === "pomodoro" ? `${pickerGeometry.minWidth}px` : "max-content"}
      style:max-width={pickerGeometry && picker === "pomodoro" ? `${pickerGeometry.maxWidth}px` : "max-content"}
      style:max-height={pickerGeometry && picker === "pomodoro" ? `${pickerGeometry.maxHeight}px` : undefined}
      style:visibility={picker === "pomodoro" ? undefined : "hidden"}
    >
      <div class="surface-floating-body min-h-0 overflow-hidden">
        <span class="menu-item justify-between gap-3">
          <span class="grid min-w-0 flex-1 grid-cols-[minmax(max-content,1fr)_max-content] items-center gap-4"><span class="truncate">{t("common.none")}</span></span>
        </span>
        {#each PROJECT_POMODORO_PRESET_ORDER as preset (preset)}
          {@const picked = preset === PROJECTS_PRESET}
          <span class={cn("menu-item relative justify-between gap-3", picked && pomodoroHovered && "bg-accent")}>
            <span class="grid min-w-0 flex-1 grid-cols-[minmax(max-content,1fr)_max-content] items-center gap-4">
              <span class="truncate">{presetLabel(preset)}</span>
              <span class="max-w-72 justify-self-end truncate text-panel-detail text-muted-foreground">{projectPomodoroSummaryLabel(COUNT_PRESET_RHYTHMS[preset])}</span>
            </span>
            {#if picked}{@render touch(settingsTouches.pomodoroOption, "50%", "50%")}{/if}
          </span>
        {/each}
        <span class="menu-item justify-between gap-3">
          <span class="grid min-w-0 flex-1 grid-cols-[minmax(max-content,1fr)_max-content] items-center gap-4"><span class="truncate">{t("calendar.pomodoro.custom")}</span></span>
        </span>
      </div>
    </div>
  </div>
</div>

<style>
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
