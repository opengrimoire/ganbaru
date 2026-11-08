<script lang="ts">
  import Ban from "@lucide/svelte/icons/ban";
  import Bell from "@lucide/svelte/icons/bell";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Clock4 from "@lucide/svelte/icons/clock-4";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import Lock from "@lucide/svelte/icons/lock";
  import Plus from "@lucide/svelte/icons/plus";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Search from "@lucide/svelte/icons/search";
  import SquareDashedPlus from "@lucide/svelte/icons/square-dashed-plus";
  import Timer from "@lucide/svelte/icons/timer";
  import Users from "@lucide/svelte/icons/users";
  import X from "@lucide/svelte/icons/x";
  import { untrack, type Component } from "svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import StarredIconBadge from "$lib/components/ui/StarredIconBadge.svelte";
  import { displayCanonicalTime } from "$lib/components/calendar/event-panel/date-time-controller.svelte";
  import { PANEL_MAX_WIDTH } from "$lib/components/calendar/event-panel/geometry-controller.svelte";
  import {
    EVENT_PANEL_SHEET_PEEK_PADDING_PX,
    EVENT_PANEL_SHEET_TOP_GAP,
  } from "$lib/components/calendar/event-panel/mobile-sheet-controller.svelte";
  import {
    clampFloatingLeft,
    clampFloatingTop,
    getAvailablePanelHeight,
    shouldConstrainPanelHeight,
  } from "$lib/components/calendar/event-panel/geometry";
  import { formatNotificationOffset } from "$lib/calendar/event-notifications";
  import type { EventColor } from "$lib/calendar/types";
  import { getEventColor } from "$lib/calendar/utils";
  import { formatDateLabel } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import type { PomodoroPresetKey } from "$lib/pomodoro/rhythm";
  import {
    projectPickerSubpanelGeometry,
    type ProjectPickerPanelBounds,
    type ProjectPickerPanelRect,
  } from "$lib/projects/picker-panels";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { cn } from "$lib/utils";
  import {
    EVENT_PANEL_EDGE_MARGIN,
    getResponsivePanelWidth,
    pickEventPanelLayout,
    type EventPanelAnchor,
  } from "$lib/utils/responsive";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import MusicPlaylistIcon from "$lib/components/music/builder/MusicPlaylistIcon.svelte";
  import AudioLinesXIcon from "$lib/components/music/soundscape/AudioLinesXIcon.svelte";
  import type { WelcomeEventPanelFrame, WelcomeEventPanelMusicPhase } from "$lib/vault/welcome-event-panel";

  /**
   * The event panel of the welcome scenes, drawn with the event panel's own classes: it opens on an event, where a
   * project is picked from the project picker, which gives the event the project's color, Focus rhythm, and music, a
   * title may be typed and the Music section opened, and the event is saved. On desktop the panel floats over the
   * Calendar as it would in a window the size of the canvas, with the picker's dropdown and its projects flyout, except
   * that where the Calendar would center it, the scene keeps it to the right so the event stays in view; on a phone it
   * is the bottom sheet, risen to its peek over the grid with Save in its header, with the picker's full screen dialog.
   * Every frame is a function of `frame`, which the scene computes from its time.
   */
  let {
    frame,
    canvasWidth,
    canvasHeight,
    titleBarHeight,
    anchor,
    date,
    start,
    end,
    projectColor,
    title,
  }: {
    frame: WelcomeEventPanelFrame;
    canvasWidth: number;
    canvasHeight: number;
    /** The height of the bar the panel keeps clear, as the window's title bar. */
    titleBarHeight: number;
    /** The drawn event's place on the canvas, as the Calendar anchors the panel on it. */
    anchor: EventPanelAnchor;
    date: Date;
    /** The event's start and end, in minutes since midnight. */
    start: number;
    end: number;
    /** The color the picked project gives the event. */
    projectColor: EventColor;
    /** The title typed into the panel. */
    title: string;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const theme = getTheme();
  const preferences = getPreferences();
  const mobileLayout = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const notificationSchedulingAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "notifications.native-scheduling");
  const musicAssignmentsAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.context-assignments");

  const EDGE = EVENT_PANEL_EDGE_MARGIN;
  /** How much taller than its limit the panel may be before its body scrolls, as the panel allows. */
  const HEIGHT_TOLERANCE = 2;
  /** The project picker's measures, as the picker and its selector draw it. */
  const PICKER_MIN_WIDTH = 202;
  const PICKER_MAX_WIDTH = 259;
  const PICKER_RIGHT_NUDGE = 4;
  const PICKER_GAP = 4;
  const SUBPANEL_VISIBLE_ROWS = 6;
  const SUBPANEL_LIST_PADDING = 8;
  const SUBPANEL_ROW_HEIGHT = 32;
  const PICKER_ICON_SIZE = mobileLayout ? 18 : 13;
  const PICKER_ICON_STROKE_WIDTH = 1.6;
  const PICKER_EMOJI_SCALE = 0.94;
  /** The project trigger's icon, as the selector draws it. */
  const TRIGGER_ICON_SIZE = 18;
  const TRIGGER_ICON_STROKE_WIDTH = 1.5;
  const TRIGGER_EMPTY_STROKE_WIDTH = 1.6;
  const TRIGGER_EMOJI_SCALE = 0.82;
  const METADATA_ICON_SIZE = 11;
  const METADATA_ICON_CLASS = "shrink-0 translate-y-[0.5px]";
  const SECTION_ICON_SIZE = 14;
  /** The Music section's select triggers, as the event panel styles them. */
  const MUSIC_TRIGGER_CLASS = "flex h-7 w-full min-w-0 items-center gap-2 rounded border-0 bg-black/5 px-2.5 text-left text-[0.8rem] font-normal text-event-panel-input-text dark:bg-black/15";
  const MUSIC_ICON_SIZE = 14;
  const MUSIC_ICON_STROKE_WIDTH = 1.5;
  const showSounds = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.soundscapes");

  type FilmLabel = "university" | "health" | "personal" | "thesis" | "assignments";
  interface FilmPickerRow {
    id: string;
    icon: string;
    label: FilmLabel;
  }
  /** The project groups, and the projects of the group the session belongs to. */
  const GROUPS: readonly FilmPickerRow[] = [
    { id: "university", icon: "lucide:graduation-cap", label: "university" },
    { id: "health", icon: "lucide:apple", label: "health" },
    { id: "personal", icon: "lucide:heart", label: "personal" },
  ];
  const PROJECTS: readonly FilmPickerRow[] = [
    { id: "thesis", icon: "lucide:book-open", label: "thesis" },
    { id: "assignments", icon: "lucide:folder", label: "assignments" },
  ];
  const PICKED_GROUP = GROUPS[0];
  const PICKED_PROJECT = PROJECTS[0];

  interface FilmSection {
    key: string;
    icon: Component<{ size?: number }>;
    label: string;
    summary: string;
  }

  const pane = $derived(frame.pane);
  const picked = $derived(frame.picked);
  const typed = $derived(frame.typed);
  const typing = $derived(frame.typing);
  const caretOn = $derived(frame.caretOn);
  const projectTouch = $derived(frame.touches.project);
  const groupTouch = $derived(frame.touches.group);
  const pickTouch = $derived(frame.touches.projectPick);
  const saveTouch = $derived(frame.touches.save);
  const music = $derived(frame.music);

  const color = $derived(picked ? projectColor : undefined);
  const preset = $derived<PomodoroPresetKey>(frame.preset);
  const dateLabel = $derived(formatDateLabel(localization.locale, date, { weekday: "short", month: "short", day: "numeric" }));
  const startLabel = $derived(displayCanonicalTime(canonicalTime(start), preferences.calendarTimeFormat));
  const endLabel = $derived(displayCanonicalTime(canonicalTime(end), preferences.calendarTimeFormat));
  const sections = $derived.by<FilmSection[]>(() => [
    { key: "meeting", icon: Users, label: t("calendar.meeting.title"), summary: "" },
    { key: "pomodoro", icon: Timer, label: t("calendar.pomodoro.title"), summary: presetLabel(preset) },
    ...(notificationSchedulingAvailable
      ? [{ key: "notifications", icon: Bell, label: t("calendar.notifications.title"), summary: formatNotificationOffset(0, t) }]
      : []),
    { key: "repeat", icon: Repeat, label: t("calendar.recurrence.repeat"), summary: "" },
  ]);
  /** The Music section's summary: the phases that play something once the event has its project's music. */
  const musicSummary = $derived.by(() => {
    if (!music?.enabled) return "";
    const plays = (phase: WelcomeEventPanelMusicPhase) => music.rows.some((row) => row.phase === phase);
    return t("calendar.eventPanel.musicPhases", plays("focus"), plays("short-break"), plays("long-break"));
  });
  /** The Music section's body, measured so it opens by its own height as the section slides open. */
  let musicBodyHeight = $state(0);

  /** The panel's parts, measured, so it sizes and places itself as the event panel does. */
  let barHeight = $state(0);
  let contentHeight = $state(0);
  let footerHeight = $state(0);
  const naturalHeight = $derived(barHeight + contentHeight + footerHeight);

  const panelWidth = $derived(getResponsivePanelWidth(canvasWidth, PANEL_MAX_WIDTH, EDGE));
  const layout = $derived(
    mobileLayout
      ? "fullscreen"
      : pickEventPanelLayout({
        viewport: { width: canvasWidth, height: canvasHeight },
        anchor,
        panelWidth: PANEL_MAX_WIDTH,
        edgeMargin: EDGE,
        titleBarHeight,
      }),
  );
  const minTop = $derived(titleBarHeight + EDGE);
  const availableHeight = $derived(getAvailablePanelHeight(canvasHeight, titleBarHeight, EDGE));
  const heightLimit = $derived(layout === "bottom" ? Math.min(560, availableHeight) : availableHeight);
  const constrained = $derived(mobileLayout || shouldConstrainPanelHeight(naturalHeight, heightLimit, HEIGHT_TOLERANCE));
  /** The phone sheet's root and the last row of its peek, measured to find the peek's height. */
  let sheetElement = $state<HTMLDivElement>();
  let peekEndElement = $state<HTMLDivElement>();
  let peekHeight = $state(0);
  $effect(() => {
    void canvasWidth;
    void canvasHeight;
    if (!mobileLayout || !layer || !sheetElement || !peekEndElement) return;
    // The sheet's transform moves both boxes alike, so their distance is the peek wherever the sheet is.
    const summaryBottom = canvasRect(layer, peekEndElement).bottom - canvasRect(layer, sheetElement).top;
    peekHeight = Math.round(summaryBottom + EVENT_PANEL_SHEET_PEEK_PADDING_PX);
  });
  const sheetVisible = $derived(peekHeight * frame.sheetRise);
  /** The body's scroll: the panel's visible height, so the scene can scroll its body to the end. */
  let viewportHeight = $state(0);
  const scrollOffset = $derived(constrained ? frame.scroll * Math.max(0, contentHeight - viewportHeight) : 0);

  const panelStyle = $derived.by(() => {
    if (mobileLayout) {
      return `left: 0; right: 0; bottom: 0; height: calc(100% - ${EVENT_PANEL_SHEET_TOP_GAP}); transform: translateY(calc((100% - ${sheetVisible}px) * ${1 - frame.sheetExpand}));`;
    }
    if (layout === "fullscreen") return `left: ${EDGE}px; right: ${EDGE}px; top: ${minTop}px; bottom: ${EDGE}px;`;
    const heightCss = constrained ? `height: ${Math.round(heightLimit)}px;` : "";
    if (layout === "bottom") {
      return `left: ${EDGE}px; right: ${EDGE}px; bottom: ${EDGE}px; max-height: ${Math.round(heightLimit)}px; ${heightCss}`;
    }
    const visibleHeight = Math.min(naturalHeight, availableHeight);
    const left = clampFloatingLeft(floatingLeft(), canvasWidth, panelWidth, EDGE);
    const top = clampFloatingTop(
      layout === "centered" ? Math.round((canvasHeight - visibleHeight) / 2) : anchor.y,
      canvasHeight,
      visibleHeight,
      minTop,
      EDGE,
    );
    return `left: ${Math.round(left)}px; top: ${Math.round(top)}px; width: ${Math.round(panelWidth)}px; max-height: ${Math.round(availableHeight)}px; ${heightCss}`;
  });

  /** Places on the canvas the desktop picker is laid out from, measured once the panel is drawn. */
  let layer = $state<HTMLDivElement>();
  let scrollElement = $state<HTMLDivElement>();
  let colorTrigger = $state<HTMLDivElement>();
  let pickerElement = $state<HTMLDivElement>();
  let groupRow = $state<HTMLDivElement>();
  let pickerHeight = $state(0);
  let flyoutFooterHeight = $state(0);
  let measured = $state<{
    scroll: ProjectPickerPanelRect;
    colorTrigger: ProjectPickerPanelRect;
    picker: ProjectPickerPanelRect | null;
    groupRow: ProjectPickerPanelRect | null;
  } | null>(null);

  $effect(() => {
    void panelStyle;
    void pickerStyle;
    void pane;
    if (!layer || !scrollElement || !colorTrigger) {
      measured = null;
      return;
    }
    const next = {
      scroll: canvasRect(layer, scrollElement),
      colorTrigger: canvasRect(layer, colorTrigger),
      picker: pickerElement ? canvasRect(layer, pickerElement) : null,
      groupRow: groupRow ? canvasRect(layer, groupRow) : null,
    };
    // The picker is placed from these measures and measured again once placed, so only a real move is written back.
    if (!untrack(() => measured && sameMeasures(measured, next))) measured = next;
  });

  function sameMeasures(a: NonNullable<typeof measured>, b: NonNullable<typeof measured>): boolean {
    return sameRect(a.scroll, b.scroll)
      && sameRect(a.colorTrigger, b.colorTrigger)
      && sameRect(a.picker, b.picker)
      && sameRect(a.groupRow, b.groupRow);
  }

  function sameRect(a: ProjectPickerPanelRect | null, b: ProjectPickerPanelRect | null): boolean {
    if (!a || !b) return a === b;
    return a.left === b.left && a.top === b.top && a.width === b.width && a.height === b.height;
  }

  const canvasBounds = $derived<ProjectPickerPanelBounds>({
    left: EDGE,
    right: canvasWidth - EDGE,
    top: EDGE,
    bottom: canvasHeight - EDGE,
  });
  /** The picker's dropdown under the color trigger, right aligned to it and kept inside the panel's body. */
  const pickerStyle = $derived.by(() => {
    if (!measured || mobileLayout) return "visibility: hidden;";
    const anchorRect = measured.colorTrigger;
    const bounds = {
      left: Math.max(measured.scroll.left, canvasBounds.left),
      right: Math.min(measured.scroll.right, canvasBounds.right),
    };
    const width = Math.min(PICKER_MAX_WIDTH, Math.max(PICKER_MIN_WIDTH, Math.max(0, bounds.right - bounds.left)));
    const left = Math.min(Math.max(anchorRect.right + PICKER_RIGHT_NUDGE - width, bounds.left), bounds.right - width);
    const maxHeight = Math.max(0, canvasHeight - EDGE * 2);
    const height = Math.min(pickerHeight || maxHeight, maxHeight);
    const spaceBelow = canvasHeight - EDGE - anchorRect.bottom - PICKER_GAP;
    const spaceAbove = anchorRect.top - EDGE - PICKER_GAP;
    const openAbove = spaceBelow < height && spaceAbove > spaceBelow;
    const preferredTop = openAbove ? anchorRect.top - PICKER_GAP - height : anchorRect.bottom + PICKER_GAP;
    const top = Math.min(Math.max(EDGE, preferredTop), Math.max(EDGE, canvasHeight - EDGE - height));
    return `left: ${Math.round(left)}px; top: ${Math.round(top)}px; width: ${Math.round(width)}px; max-height: ${Math.round(maxHeight)}px;`;
  });
  /** The group's projects, in a flyout beside the group's row, placed by the picker's own geometry. */
  const flyoutStyle = $derived.by(() => {
    if (!measured?.picker || !measured.groupRow) return "visibility: hidden;";
    const { panel } = projectPickerSubpanelGeometry({
      anchorRect: measured.groupRow,
      panelRect: measured.picker,
      bounds: canvasBounds,
      gap: PICKER_GAP,
      footerHeight: flyoutFooterHeight,
      projectCount: PROJECTS.length,
      visibleRows: SUBPANEL_VISIBLE_ROWS,
      listPadding: SUBPANEL_LIST_PADDING,
      rowHeight: SUBPANEL_ROW_HEIGHT,
    });
    return `left: ${Math.round(panel.left)}px; top: ${Math.round(panel.top)}px; width: ${Math.round(panel.width)}px; height: ${Math.round(panel.height)}px; max-height: ${Math.round(panel.maxHeight)}px;`;
  });

  /** Where the floating panel goes across, before it is kept inside the canvas, as the panel places itself. */
  /** The panel against the canvas's right edge, leaving the drawn event's left part and the Focus rail in view. */
  function rightAlignedLeft(): number {
    return canvasWidth - EDGE - panelWidth;
  }

  function floatingLeft(): number {
    if (layout === "centered") return rightAlignedLeft();
    const anchorLeft = anchor.x - anchor.width;
    const requiredSideSpace = panelWidth + EDGE * 2;
    if (anchorLeft >= requiredSideSpace) return anchorLeft - EDGE - panelWidth;
    if (canvasWidth - anchor.x >= requiredSideSpace) return anchor.x + EDGE;
    return rightAlignedLeft();
  }

  /**
   * An element's box on the canvas, in the canvas's own pixels, so the picker places itself the same way however the
   * film is scaled to fit its player.
   */
  function canvasRect(canvas: HTMLElement, element: HTMLElement): ProjectPickerPanelRect {
    const origin = canvas.getBoundingClientRect();
    const scale = canvas.offsetWidth > 0 ? origin.width / canvas.offsetWidth : 1;
    const rect = element.getBoundingClientRect();
    const left = (rect.left - origin.left) / scale;
    const top = (rect.top - origin.top) / scale;
    const width = rect.width / scale;
    const height = rect.height / scale;
    return { left, top, width, height, right: left + width, bottom: top + height };
  }

  function canonicalTime(minuteOfDay: number): string {
    return `${String(Math.floor(minuteOfDay / 60)).padStart(2, "0")}:${String(minuteOfDay % 60).padStart(2, "0")}`;
  }

  /** A preset's name, as the Pomodoro section sums it up. */
  function presetLabel(key: PomodoroPresetKey): string {
    switch (key) {
      case "adaptive": return t("calendar.pomodoro.adaptive");
      case "creative": return t("calendar.pomodoro.creative");
      case "balanced": return t("calendar.pomodoro.balanced");
      case "deep": return t("calendar.pomodoro.deepFocus");
      case "extended": return t("calendar.pomodoro.extended");
    }
  }

  /** A phase's name, as the Music section labels its rows. */
  function phaseLabel(phase: WelcomeEventPanelMusicPhase): string {
    return t(`music.assignment.phase.${phase}`);
  }

  function rowLabel(label: FilmLabel): string {
    switch (label) {
      case "university": return t("vaultSetup.film.universityGroup");
      case "health": return t("vaultSetup.film.healthGroup");
      case "personal": return t("vaultSetup.film.personalGroup");
      case "thesis": return t("vaultSetup.film.thesisProject");
      case "assignments": return t("vaultSetup.film.assignmentsProject");
    }
  }

  /** The floating panel is placed from its measured height, so it stays hidden until its first measure. */
  const placed = $derived(mobileLayout ? peekHeight > 0 : naturalHeight > 0);
  const pickerOpen = $derived(pane !== null);
  /** The group row stays highlighted from its tap while its projects are open; the project row while it is tapped. */
  const groupHighlighted = $derived(groupTouch > 0 || pane === "projects");
  const projectHighlighted = $derived(pickTouch > 0);
</script>

{#snippet touch(opacity: number, position: string)}
  {#if opacity > 0}
    <span
      class={cn("pointer-events-none absolute z-20 size-6 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35", position)}
      style:opacity
    ></span>
  {/if}
{/snippet}

{#snippet searchField()}
  <div class={cn("field flex items-center gap-1.5", mobileLayout ? "min-h-12 rounded-xl pl-3" : "py-0 pl-2 pr-1")}>
    <Search size={PICKER_ICON_SIZE} strokeWidth={PICKER_ICON_STROKE_WIDTH} class="shrink-0 text-popover-foreground/60" />
    <span
      class={cn(
        "project-picker-search field-bare truncate text-popover-foreground/60",
        mobileLayout && "flex h-12 items-center text-base",
      )}
    >{t("calendar.eventPanel.searchProjects")}</span>
  </div>
{/snippet}

{#snippet pickerFooter(label: string, mobile: boolean)}
  <div class={cn("relative z-10 shrink-0 bg-popover", mobile ? "max-h-[55%] overflow-y-auto overscroll-contain p-2" : "p-1.5")}>
    <div class="pointer-events-none absolute left-1.5 right-1.5 top-0 border-t border-border/70"></div>
    <div class={cn("menu-item justify-center", mobile && "min-h-12 rounded-xl text-sm")}>
      <Plus size={mobile ? 18 : 13} strokeWidth={PICKER_ICON_STROKE_WIDTH} />
      <span>{label}</span>
    </div>
  </div>
{/snippet}

{#snippet projectRow(project: FilmPickerRow, mobile: boolean)}
  <div
    class={cn("menu-item relative flex gap-2 text-popover-foreground", mobile && "min-h-12 px-3 text-sm")}
    data-highlighted={project.id === PICKED_PROJECT.id && projectHighlighted ? "" : undefined}
  >
    <StarredIconBadge>
      <ProjectIcon name={project.icon} size={PICKER_ICON_SIZE} strokeWidth={PICKER_ICON_STROKE_WIDTH} emojiScale={PICKER_EMOJI_SCALE} />
    </StarredIconBadge>
    <span class="min-w-0 flex-1 truncate">{rowLabel(project.label)}</span>
    {#if project.id === PICKED_PROJECT.id}
      {@render touch(pickTouch, "left-2/5 top-1/2")}
    {/if}
  </div>
{/snippet}

<div bind:this={layer} class="pointer-events-none absolute inset-0 z-49" style:visibility={placed ? undefined : "hidden"} aria-hidden="true">
  <div
    bind:this={sheetElement}
    class={cn("panel-root absolute flex flex-col", mobileLayout && "rounded-t-floating")}
    data-mobile={mobileLayout || undefined}
    style="box-shadow: 0 0 2px 0px var(--panel-edge), 0 1px 2px var(--panel-shadow); background-color: var(--panel-bg); {panelStyle}"
  >
    {#if mobileLayout}
      <!-- The sheet's header: the grabber, then close and Save. -->
      <div bind:offsetHeight={barHeight} class="sticky top-0 z-10 shrink-0" style="background-color: var(--panel-bg);">
        <div class="flex justify-center pt-2 pb-1.5">
          <div class="h-1 w-9 rounded-full bg-muted-foreground/40"></div>
        </div>
        <div class="flex items-center gap-1 px-1">
          <div class="panel-chrome flex h-12 w-12 shrink-0 items-center justify-center rounded-floating-item text-muted-foreground">
            <X size={22} />
          </div>
          <div class="flex-1"></div>
          <div class="flex h-12 shrink-0 items-center pl-1 pr-3">
            <span class="relative rounded-full bg-foreground px-6 py-2 text-[0.866667rem] font-medium text-background">
              {t("common.save")}
              {@render touch(saveTouch, "left-1/2 top-1/2")}
            </span>
          </div>
        </div>
      </div>
    {:else}
      <!-- The drag bar. -->
      <div bind:offsetHeight={barHeight} class="sticky top-0 z-10 flex items-center pl-4 pr-2" style="background-color: var(--sidebar);">
        <div class="flex flex-1 items-center justify-center py-2.5">
          <div class="h-[1.5px] w-9 bg-muted-foreground/50"></div>
        </div>
      </div>
    {/if}

    <div class={cn("relative min-h-0", constrained ? "flex-1 overflow-hidden" : "shrink-0")}>
      <div
        bind:this={scrollElement}
        bind:clientHeight={viewportHeight}
        class={cn("event-panel-scroll hide-scrollbar overscroll-contain", constrained ? "h-full overflow-hidden" : "overflow-visible")}
      >
        <div bind:offsetHeight={contentHeight} style:transform={scrollOffset > 0 ? `translateY(${-scrollOffset}px)` : undefined}>
          <!-- The title, the project and color triggers, and the date and time. -->
          <div class={cn("shrink-0 px-4", !mobileLayout && "pt-2.5")}>
            <div class="relative top-0.5 flex items-center gap-2.5 px-1">
              <div class="title-wrapper relative min-w-0 flex-1" data-focused={typing || undefined}>
                <div
                  class={cn(
                    "w-full overflow-hidden whitespace-pre bg-transparent py-0.5 text-[1rem] font-semibold",
                    typed ? "text-foreground" : "text-event-panel-placeholder",
                  )}
                >{#if typed}{typed}{:else if !typing}{t("calendar.eventPanel.titlePlaceholder")}{/if}{#if typing && caretOn}<span class="film-caret"></span>{/if}{#if typing && !typed && !caretOn}&#8203;{/if}</div>
              </div>
              <div class={cn("event-identity-controls flex items-center", mobileLayout ? "gap-1" : "gap-2.5")}>
                <div
                  data-control
                  class={cn(
                    "event-identity-trigger relative flex shrink-0 items-center justify-center text-event-panel-muted-text",
                    mobileLayout ? "min-h-12 min-w-12 rounded-xl" : "size-4.5 rounded-sm",
                  )}
                >
                  {#if picked}
                    <ProjectIcon
                      name={PICKED_PROJECT.icon}
                      size={TRIGGER_ICON_SIZE}
                      strokeWidth={TRIGGER_ICON_STROKE_WIDTH}
                      emojiScale={TRIGGER_EMOJI_SCALE}
                    />
                  {:else}
                    <SquareDashedPlus size={TRIGGER_ICON_SIZE} strokeWidth={TRIGGER_EMPTY_STROKE_WIDTH} />
                  {/if}
                  {@render touch(projectTouch, "left-1/2 top-1/2")}
                </div>
                <div
                  bind:this={colorTrigger}
                  data-control
                  class={cn(
                    "event-identity-trigger",
                    mobileLayout ? "flex size-12 shrink-0 items-center justify-center rounded-xl" : "size-4.5 shrink-0 rounded-sm",
                  )}
                  style:background-color={mobileLayout ? undefined : getEventColor(color, theme.current).bg}
                >
                  {#if mobileLayout}
                    <span class="size-5 rounded-md border border-border" style:background-color={getEventColor(color, theme.current).bg}></span>
                  {/if}
                </div>
              </div>
            </div>
            <hr class="border-event-panel-divider mx-1 mt-0.5" />

            <div class="mt-1.5 flex flex-col gap-px px-1">
              <div
                bind:this={peekEndElement}
                class="date-time-grid relative text-[0.866667rem] leading-none"
                data-stacked={panelWidth < 300 || undefined}
              >
                <div class="relative z-1 min-w-0 justify-self-start">
                  <div data-control class="date-chip max-w-full rounded py-0.5 text-event-panel-input-text">{dateLabel}</div>
                </div>
                <div class="time-group relative z-2 flex items-center justify-center gap-1.5 py-0.5">
                  <span
                    class={cn(
                      "time-input-shell relative z-30 rounded text-center text-event-panel-input-text",
                      preferences.calendarTimeFormat === "12h" ? "text-[0.8rem]" : "text-[0.866667rem]",
                    )}
                    data-value={startLabel}
                  ><span class="time-input flex items-center justify-center bg-transparent px-0 py-0.5">{startLabel}</span></span>
                  <span class="text-muted-foreground/60">-</span>
                  <span
                    class={cn(
                      "time-input-shell relative z-30 rounded text-center text-event-panel-input-text",
                      preferences.calendarTimeFormat === "12h" ? "text-[0.8rem]" : "text-[0.866667rem]",
                    )}
                    data-value={endLabel}
                  ><span class="time-input flex items-center justify-center bg-transparent px-0 py-0.5">{endLabel}</span></span>
                </div>
                <div class="relative z-1 min-w-0 justify-self-end text-right">
                  <div data-control class="date-chip max-w-full rounded py-0.5 text-event-panel-input-text">{dateLabel}</div>
                </div>
              </div>
            </div>
          </div>

          <!-- Timed, busy, and private. -->
          <div class="flex flex-col gap-3 px-4 pb-0 pt-1.5">
            <div class="flex w-full items-center justify-evenly overflow-hidden rounded-none bg-event-panel-contrast text-[0.733333rem]">
              <div data-control class="flex min-w-0 max-w-full items-center justify-center gap-1.5 rounded-none px-0 py-1.5 text-foreground">
                <Clock4 size={METADATA_ICON_SIZE} class={METADATA_ICON_CLASS} />
                <span class="translate-y-[1.13px] truncate">{t("calendar.eventPanel.timed")}</span>
              </div>
              <div data-control class="flex min-w-0 max-w-full items-center justify-center gap-1.5 rounded-none px-0 py-1.5 text-foreground">
                <Ban size={METADATA_ICON_SIZE} class={METADATA_ICON_CLASS} />
                <span class="translate-y-[1.13px] truncate">{t("calendar.eventPanel.busy")}</span>
              </div>
              <div data-control class="flex min-w-0 max-w-full items-center justify-center gap-1.5 rounded-none px-0 py-1.5 capitalize text-foreground">
                <Lock size={METADATA_ICON_SIZE} class={METADATA_ICON_CLASS} />
                <span class="translate-y-[1.13px] truncate">{t("calendar.eventPanel.visibilityPrivate")}</span>
              </div>
            </div>
          </div>

          <!-- The sections, with Pomodoro and notifications on as on a new event. -->
          <div class="flex shrink-0 flex-col gap-1.5 px-4 py-1.5">
            {#each sections as section (section.key)}
              {@const enabled = section.summary !== ""}
              <div class="flex flex-col overflow-hidden rounded-none" style="background-color: var(--panel-contrast);">
                <div class="section-header flex items-stretch">
                  <div
                    data-control
                    class={cn(
                      "flex w-10 shrink-0 items-center justify-center",
                      enabled ? "bg-black/3 text-foreground dark:bg-black/30" : "text-muted-foreground/50",
                    )}
                  >
                    <section.icon size={SECTION_ICON_SIZE} />
                  </div>
                  <div data-control class="flex flex-1 items-center gap-2.5 px-3 py-2 text-left">
                    <span class={cn("translate-y-[1.13px] text-[0.8rem]", enabled ? "text-foreground" : "text-muted-foreground")}>{section.label}</span>
                    <span class="ml-auto translate-y-[1.13px] truncate text-[0.733333rem] text-muted-foreground">{section.summary}</span>
                  </div>
                </div>
              </div>
            {/each}
            {#if musicAssignmentsAvailable}
              {@const musicOn = music?.enabled ?? false}
              {@const expanded = music?.expanded ?? 0}
              <!-- The Music section, which takes the project's playlist for each phase and opens on its rows. -->
              <div class="flex flex-col overflow-hidden rounded-none" style="background-color: var(--panel-contrast);">
                <div class="section-header flex items-stretch">
                  <div
                    data-control
                    class={cn(
                      "flex w-10 shrink-0 items-center justify-center",
                      musicOn ? "bg-black/3 text-foreground dark:bg-black/30" : "text-muted-foreground/50",
                    )}
                  >
                    <Music size={SECTION_ICON_SIZE} />
                  </div>
                  <div data-control class="relative flex min-w-0 flex-1 items-center gap-2.5 px-3 py-2 text-left">
                    <span class={cn("translate-y-[1.13px] text-[0.8rem]", musicOn ? "text-foreground" : "text-muted-foreground")}>{t("calendar.eventPanel.music")}</span>
                    <span class="ml-auto min-w-0 translate-y-[1.13px] truncate text-[0.733333rem] text-muted-foreground">{musicSummary}</span>
                    {@render touch(frame.touches.music, "left-1/4 top-1/2")}
                  </div>
                </div>
                {#if music && expanded > 0}
                  <div class="overflow-hidden" style:height={expanded < 1 ? `${musicBodyHeight * expanded}px` : undefined}>
                    <div bind:offsetHeight={musicBodyHeight} class="music-body py-2.5 pl-1" style="background-color: var(--panel-bg);">
                      <div class={cn("music-grid", showSounds && "phase-columns")}>
                        {#each music.rows as row (row.phase)}
                          {@const none = Math.max(0, 1 - 2 * row.fill)}
                          {@const chosen = Math.max(0, 2 * row.fill - 1)}
                          <span class="music-label truncate text-[0.8rem] text-foreground">{phaseLabel(row.phase)}</span>
                          <div class={MUSIC_TRIGGER_CLASS}>
                            {#if row.fill < 0.5}
                              <span class="flex min-w-0 flex-1 items-center gap-2" style:opacity={none}>
                                <VolumeX size={MUSIC_ICON_SIZE} strokeWidth={MUSIC_ICON_STROKE_WIDTH} class="shrink-0 text-muted-foreground" />
                                <span class="min-w-0 flex-1 truncate">{t("common.none")}</span>
                              </span>
                            {:else}
                              <span class="flex min-w-0 flex-1 items-center gap-2" style:opacity={chosen}>
                                <span class="grid size-3.5 shrink-0 place-items-center text-muted-foreground">
                                  <MusicPlaylistIcon icon={row.playlistIcon} size={MUSIC_ICON_SIZE} strokeWidth={MUSIC_ICON_STROKE_WIDTH} />
                                </span>
                                <span class="min-w-0 flex-1 truncate">{row.playlistName}</span>
                              </span>
                            {/if}
                            <ChevronDown size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
                          </div>
                          {#if showSounds}
                            <div class={MUSIC_TRIGGER_CLASS}>
                              <AudioLinesXIcon size={MUSIC_ICON_SIZE} strokeWidth={MUSIC_ICON_STROKE_WIDTH} class="shrink-0 text-muted-foreground" />
                              <span class="min-w-0 flex-1 truncate">{t("common.none")}</span>
                              <ChevronDown size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
                            </div>
                          {/if}
                        {/each}
                      </div>
                    </div>
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        </div>
      </div>
    </div>

    {#if !mobileLayout}
      <!-- Save, outside the scrolling body. -->
      <div
        bind:offsetHeight={footerHeight}
        class={cn("shrink-0 px-4", layout === "fullscreen" ? "pb-2 pt-1" : "pb-3.5 pt-1.5")}
        style="background-color: var(--panel-bg);"
      >
        <div class="panel-footer-actions flex">
          <div
            data-control
            class="relative flex flex-1 items-center justify-center gap-2 bg-action-confirm py-1.5 text-[0.866667rem] text-action-confirm-foreground"
          >
            <span>{t("calendar.eventPanel.saveShortcut", formatShortcut("Mod + Enter"))}</span>
            {@render touch(saveTouch, "left-1/2 top-1/2")}
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#if pickerOpen && mobileLayout}
    <!-- The picker's full screen dialog: the groups, then the tapped group's projects. -->
    <div class="absolute inset-0 z-80 box-border flex items-stretch justify-center p-2">
      <div class="surface-backdrop absolute inset-0"></div>
      <div class="relative z-10 h-full min-h-0 w-full max-w-xl">
        <div class="project-picker-panel surface-floating flex h-full min-h-0 w-full flex-col overflow-hidden rounded-2xl">
          <div class="border-b border-border/70 p-2">
            <div class="flex min-h-14 items-center gap-1">
              {#if pane === "projects"}
                <div class="flex min-h-12 min-w-12 items-center justify-center rounded-xl">
                  <ChevronLeft size={22} strokeWidth={PICKER_ICON_STROKE_WIDTH} />
                </div>
              {/if}
              <h2 class="min-w-0 flex-1 truncate px-2 text-base font-semibold">
                {pane === "projects" ? rowLabel(PICKED_GROUP.label) : t("projects.navigator.pickerLabel")}
              </h2>
              <div class="flex min-h-12 min-w-12 items-center justify-center rounded-xl">
                <X size={22} strokeWidth={PICKER_ICON_STROKE_WIDTH} />
              </div>
            </div>
            {@render searchField()}
          </div>
          <div class="relative min-h-0 flex-1">
            <div class="hide-scrollbar h-full min-h-0 overflow-hidden overscroll-contain px-1 py-2">
              <div class="grid px-1">
                {#if pane === "projects"}
                  {#each PROJECTS as project (project.id)}
                    <div class="px-1">{@render projectRow(project, true)}</div>
                  {/each}
                {:else}
                  {#each GROUPS as group (group.id)}
                    <div class="px-1">
                      <div
                        class="menu-item relative grid min-h-12 grid-cols-[1.125rem_minmax(0,1fr)_1.5rem] gap-2 px-3"
                        data-highlighted={group.id === PICKED_GROUP.id && groupHighlighted ? "" : undefined}
                      >
                        <ProjectIcon name={group.icon} size={PICKER_ICON_SIZE} strokeWidth={PICKER_ICON_STROKE_WIDTH} emojiScale={PICKER_EMOJI_SCALE} class="shrink-0" />
                        <span class="truncate text-sm">{rowLabel(group.label)}</span>
                        <ChevronRight size={18} strokeWidth={PICKER_ICON_STROKE_WIDTH} class="justify-self-end text-popover-foreground/60" />
                        {#if group.id === PICKED_GROUP.id}
                          {@render touch(groupTouch, "left-2/5 top-1/2")}
                        {/if}
                      </div>
                    </div>
                  {/each}
                {/if}
              </div>
            </div>
          </div>
          {@render pickerFooter(pane === "projects" ? t("calendar.eventPanel.createProject") : t("calendar.eventPanel.createGroup"), true)}
        </div>
      </div>
    </div>
  {:else if pickerOpen}
    <!-- The picker's dropdown under the color trigger, with the tapped group's projects in a flyout beside it. -->
    <div class="absolute z-61" style={pickerStyle}>
      <div
        bind:this={pickerElement}
        bind:offsetHeight={pickerHeight}
        class="project-picker-panel surface-floating flex min-h-0 w-full flex-col overflow-hidden"
      >
        <div class="px-1.5 pb-0.5 pt-1.5">{@render searchField()}</div>
        <div class="relative min-h-0 flex-1">
          <div class="hide-scrollbar h-full min-h-0 overflow-hidden pb-1 pt-0.5">
            <div class="grid pl-1">
              {#each GROUPS as group (group.id)}
                <div class="pl-1 pr-1">
                  {#if group.id === PICKED_GROUP.id}
                    <div
                      bind:this={groupRow}
                      class="menu-item relative grid grid-cols-[1rem_minmax(0,1fr)_1rem] gap-2"
                      data-highlighted={groupHighlighted ? "" : undefined}
                    >
                      <ProjectIcon name={group.icon} size={PICKER_ICON_SIZE} strokeWidth={PICKER_ICON_STROKE_WIDTH} emojiScale={PICKER_EMOJI_SCALE} class="shrink-0" />
                      <span class="truncate">{rowLabel(group.label)}</span>
                      <ChevronRight size={13} strokeWidth={PICKER_ICON_STROKE_WIDTH} class="justify-self-end text-popover-foreground/60" />
                      {@render touch(groupTouch, "left-2/5 top-1/2")}
                    </div>
                  {:else}
                    <div class="menu-item grid grid-cols-[1rem_minmax(0,1fr)_1rem] gap-2">
                      <ProjectIcon name={group.icon} size={PICKER_ICON_SIZE} strokeWidth={PICKER_ICON_STROKE_WIDTH} emojiScale={PICKER_EMOJI_SCALE} class="shrink-0" />
                      <span class="truncate">{rowLabel(group.label)}</span>
                      <ChevronRight size={13} strokeWidth={PICKER_ICON_STROKE_WIDTH} class="justify-self-end text-popover-foreground/60" />
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          </div>
        </div>
        {@render pickerFooter(t("calendar.eventPanel.createGroup"), false)}
      </div>
    </div>
    {#if pane === "projects"}
      <div class="project-picker-panel surface-floating absolute z-61 flex min-h-0 flex-col overflow-hidden" style={flyoutStyle}>
        <div class="relative min-h-0 flex-1">
          <div class="hide-scrollbar h-full min-h-0 overflow-hidden px-1 py-1">
            <div class="grid">
              {#each PROJECTS as project (project.id)}
                {@render projectRow(project, false)}
              {/each}
            </div>
          </div>
        </div>
        <div bind:offsetHeight={flyoutFooterHeight}>
          {@render pickerFooter(t("calendar.eventPanel.createProject"), false)}
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  /* The event panel's own styles, for the parts the film draws. */
  .panel-root {
    --panel-bg: var(--event-panel-bg);
    --panel-contrast: var(--event-panel-contrast);
    --panel-edge: var(--event-panel-edge);
    --panel-shadow: var(--event-panel-shadow);
    --foreground: var(--event-panel-text);
    --muted-foreground: var(--event-panel-muted-text);
    font-variant-numeric: tabular-nums;
    min-height: 0;
    overflow: hidden;
  }

  .panel-root[data-mobile="true"] [data-control] {
    min-height: 3rem;
  }

  .panel-root[data-mobile="true"] .event-identity-controls .event-identity-trigger {
    width: 2.5rem;
    min-width: 2.5rem;
    height: 2.5rem;
    min-height: 2.5rem;
  }

  .panel-root[data-mobile="true"] .time-input-shell {
    min-height: 3rem;
  }

  .date-time-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    column-gap: 0.375rem;
  }

  .date-time-grid[data-stacked] {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    row-gap: 0.125rem;
  }

  .date-time-grid[data-stacked] .time-group {
    grid-column: 1 / -1;
    grid-row: 2;
    justify-self: center;
  }

  .date-chip {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .time-input-shell {
    position: relative;
    display: inline-block;
  }

  .time-input-shell::after {
    content: attr(data-value);
    display: block;
    font: inherit;
    letter-spacing: inherit;
    padding-block: 0.125rem;
    visibility: hidden;
    white-space: pre;
    pointer-events: none;
  }

  .time-input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    font: inherit;
    letter-spacing: inherit;
    color: inherit;
  }

  .title-wrapper::after {
    content: "";
    position: absolute;
    right: 0;
    top: 0;
    bottom: 0;
    width: 24px;
    background: linear-gradient(to right, transparent, var(--panel-bg));
    pointer-events: none;
  }

  .title-wrapper[data-focused]::after {
    opacity: 0;
  }

  /* The Music section's rows, as the event panel lays them out. */
  .music-body {
    container-type: inline-size;
  }

  .music-grid {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 0.375rem 1.25rem;
  }

  .music-grid.phase-columns {
    grid-template-columns: auto minmax(0, 1fr) minmax(0, 1fr);
    column-gap: 0.5rem;
  }

  @container (width < 24rem) {
    .music-grid.phase-columns { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .phase-columns .music-label { grid-column: 1 / -1; margin-top: 0.25rem; }
  }

  /* The title's caret, as a focused text input draws it. */
  .film-caret {
    display: inline-block;
    width: 1px;
    height: 1.1em;
    margin-left: 1px;
    vertical-align: text-bottom;
    background-color: currentColor;
  }
</style>
