<script lang="ts">
  import type { CalendarEvent, PersistedSegment, PositionedAllDayEvent, PositionedEvent } from "$lib/calendar/types";
  import type { DayNameFormat, TimezoneAbbrMode } from "$lib/calendar/utils";
  import {
    formatDayName,
    formatDatePart,
    getEventColor,
    GUTTER_WIDTH_PER_TIMEZONE,
    visibleMinuteRangeForScroll,
  } from "$lib/calendar/utils";
  import { createTimelineWheelScroll } from "$lib/calendar/timeline-scroll";
  import TimeGutter from "./TimeGutter.svelte";
  import { calendarZoomKeyAction } from "./zoom-keys";
  import DayColumn from "./DayColumn.svelte";
  import HourGridlines from "./HourGridlines.svelte";
  import TimezoneSelector from "./TimezoneSelector.svelte";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import AllDayEventChip from "./AllDayEventChip.svelte";
  import { getEventIndicatorState } from "./event-indicators";
  import { createTimedEventDragController } from "./timed-event-drag-controller.svelte";
  import { createAllDayEventDragController } from "./all-day-event-drag-controller.svelte";
  import { eventMatchesActiveOccurrence } from "$lib/components/calendar/occurrence-protection";
  import { hasCalendarEventStarted } from "$lib/components/calendar/event-edit-permissions";
  import type { PanelAnchor } from "$lib/components/calendar/edit-session.svelte";
  import { getCalendarZoom } from "$lib/stores/calendar-zoom.svelte";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { onMount } from "svelte";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Video from "@lucide/svelte/icons/video";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import Users from "@lucide/svelte/icons/users";
  import type { Theme } from "$lib/themes";

  let {
    anchorDate,
    days = [] as Date[],
    events,
    positionedTimedEventsByDay,
    positionedAllDayEvents,
    theme,
    timezones = [] as string[],
    timezoneAbbreviationMode = "acronym" as TimezoneAbbrMode,
    onEventClick,
    onEventPrefetch,
    onEventUpdate,
    onEventCreate,
    editingId,
    previewedIds,
    persistedSegmentsByEvent = new Map<string, PersistedSegment[]>(),
    initialScrollMinute = -1,
    onScrollChange,
    onAddTimezone,
    onRemoveTimezone,
    onReorderTimezone,
    onTimezoneAbbreviationModeChange,
    onWheelNavigate,
    onDayHeaderClick,
    mobileLayout = false,
    onMobileTouchEditStart,
    onMobileTouchEditEnd,
  }: {
    anchorDate: Date;
    days?: Date[];
    events: CalendarEvent[];
    positionedTimedEventsByDay: Map<string, PositionedEvent[]>;
    positionedAllDayEvents: PositionedAllDayEvent[];
    theme: Theme;
    timezones?: string[];
    timezoneAbbreviationMode?: TimezoneAbbrMode;
    onEventClick: (event: CalendarEvent, rect?: DOMRect) => void;
    onEventPrefetch?: (event: CalendarEvent) => void;
    onEventUpdate: (event: CalendarEvent) => void;
    onEventCreate: (start: string, end: string, allDay?: boolean, anchor?: PanelAnchor) => void;
    editingId?: string;
    previewedIds?: Set<string>;
    persistedSegmentsByEvent?: ReadonlyMap<string, PersistedSegment[]>;
    initialScrollMinute?: number;
    onScrollChange?: (minute: number) => void;
    onAddTimezone?: (tz: string) => void;
    onRemoveTimezone?: (index: number) => void;
    onReorderTimezone?: (from: number, to: number) => void;
    onTimezoneAbbreviationModeChange?: (mode: TimezoneAbbrMode) => void;
    onWheelNavigate?: (direction: "back" | "forward") => void;
    onDayHeaderClick?: (date: Date) => void;
    mobileLayout?: boolean;
    onMobileTouchEditStart?: () => void;
    onMobileTouchEditEnd?: () => void;
  } = $props();

  /** Stable empty fallback so day columns without events keep a consistent prop reference. */
  const EMPTY_POSITIONED: PositionedEvent[] = [];

  const ALL_DAY_ROW_H = 21;
  const ALL_DAY_GAP = 1;
  const ALL_DAY_PAD = 2;
  const ALL_DAY_MAX_VISIBLE = 2;

  const visibleDays = $derived(days.length > 0 ? days : [anchorDate]);
  const dayCount = $derived(visibleDays.length);

  const allDayPositioned = $derived(positionedAllDayEvents);
  const allDayMaxRow = $derived(allDayPositioned.length > 0 ? Math.max(...allDayPositioned.map((p) => p.row)) + 1 : 0);
  const timezoneCount = $derived(Math.max(1, timezones.length));
  const gridCols = $derived(
    `repeat(${timezoneCount}, ${GUTTER_WIDTH_PER_TIMEZONE}px) repeat(${dayCount}, 1fr)`,
  );

  let allDayExpanded = $state(false);
  const allDayCollapsible = $derived(allDayMaxRow > ALL_DAY_MAX_VISIBLE);
  const allDayVisibleRows = $derived(allDayExpanded || !allDayCollapsible ? allDayMaxRow : ALL_DAY_MAX_VISIBLE);
  // +1 row for the "+N more" button when collapsed
  const allDayGridRows = $derived(allDayCollapsible && !allDayExpanded ? allDayVisibleRows + 1 : allDayVisibleRows);

  // Reset expanded state on range change.
  $effect(() => { void visibleDays; allDayExpanded = false; });

  let headerCells: HTMLElement[] = $state([]);
  let dayFormat: DayNameFormat = $state("short");
  let stickyHeaderHeight = $state(0);
  let allDayOverlayHeight = $state(0);
  $effect(() => { if (allDayMaxRow === 0) allDayOverlayHeight = 0; });
  const gutterTopHeight = $derived(stickyHeaderHeight);

  $effect(() => {
    const el = headerCells[0];
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const w = entries[0].contentBoxSize[0].inlineSize;
      if (w >= 110) dayFormat = "long";
      else if (w >= 60) dayFormat = "short";
      else if (w >= 36) dayFormat = "narrow";
      else dayFormat = "none";
    });
    observer.observe(el);
    return () => observer.disconnect();
  });

  let scrollContainer: HTMLDivElement | undefined = $state();
  let currentTimeMinute = $state(-1);
  let todayStr = $state(formatDatePart(new Date()));
  let wheelCooldown = false;
  let ready = $state(false);
  let visibleStartMinute = $state(0);
  let visibleEndMinute = $state(1440);

  const calendarZoom = getCalendarZoom();
  const timelineWheelScroll = createTimelineWheelScroll(() => scrollContainer);
  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);

  function renderedHourHeight(): number {
    const raw = scrollContainer?.style.getPropertyValue("--hour-h") ?? "";
    const parsed = raw ? Number.parseFloat(raw) : Number.NaN;
    return Number.isFinite(parsed) && parsed > 0 ? parsed : calendarZoom.hourHeight;
  }

  function updateVisibleMinuteRange() {
    if (!scrollContainer) return;
    const range = visibleMinuteRangeForScroll({
      scrollTop: scrollContainer.scrollTop,
      viewportHeight: scrollContainer.clientHeight,
      stickyTop: allDayOverlayHeight,
      hourHeight: renderedHourHeight(),
    });
    visibleStartMinute = range.startMinute;
    visibleEndMinute = range.endMinute;
  }

  function scrollTopForMinute(minute: number, hourHeight: number): number {
    if (minute <= 0) return 0;
    return allDayOverlayHeight + (minute / 60) * hourHeight;
  }

  function scrollMinuteFromTop(scrollTop: number, hourHeight: number): number {
    return (Math.max(0, scrollTop - allDayOverlayHeight) / hourHeight) * 60;
  }

  function scrollTimelineByWheel(e: WheelEvent) {
    if (!scrollContainer) return;
    e.preventDefault();
    e.stopPropagation();
    timelineWheelScroll(e);
  }

  function handleHeaderWheel(e: WheelEvent) {
    if (e.ctrlKey || e.shiftKey) {
      scrollTimelineByWheel(e);
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (!onWheelNavigate || wheelCooldown) return;
    if (Math.abs(e.deltaY) < 5) return;
    wheelCooldown = true;
    onWheelNavigate(e.deltaY > 0 ? "forward" : "back");
    setTimeout(() => { wheelCooldown = false; }, 300);
  }

  const SCROLL_SETTLE_MS = 150;
  let lastScrollAt = 0;

  function updateCurrentTime() {
    if (calendarZoom.isAnimating) return;
    if (performance.now() - lastScrollAt < SCROLL_SETTLE_MS) return;
    const now = new Date();
    currentTimeMinute = now.getHours() * 60 + now.getMinutes() + now.getSeconds() / 60;
    const nowStr = formatDatePart(now);
    if (nowStr !== todayStr) todayStr = nowStr;
  }

  onMount(() => {
    updateCurrentTime();
    const interval = setInterval(updateCurrentTime, 1000);

    // Immediately refresh time on wake from sleep / tab re-focus
    function onVisibilityChange() {
      if (!document.hidden) updateCurrentTime();
    }
    document.addEventListener("visibilitychange", onVisibilityChange);

    if (scrollContainer) {
      // Set --hour-h before any scroll/layout so the initial paint is correct
      scrollContainer.style.setProperty("--hour-h", String(calendarZoom.hourHeight));

      // Register scroll container so buttons/keyboard can animate
      calendarZoom.registerScrollContainer(scrollContainer, 0);

      const hourHeight = calendarZoom.hourHeight;
      if (initialScrollMinute >= 0) {
        scrollContainer.scrollTop = scrollTopForMinute(initialScrollMinute, hourHeight);
      } else {
        const now = new Date();
        const targetHour = Math.max(0, now.getHours() - 2);
        scrollContainer.scrollTop = scrollTopForMinute(targetHour * 60, hourHeight);
      }

      scrollContainer.addEventListener("scroll", handleScroll);
      updateVisibleMinuteRange();
      ready = true;
    }

    // Keyboard shortcuts for internal calendar zoom; layout handling lives in zoom-keys.ts.
    function handleKeyDown(e: KeyboardEvent) {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || (e.target as HTMLElement)?.isContentEditable) return;

      const action = calendarZoomKeyAction(e);
      if (!action) return;
      e.preventDefault();
      if (action === "reset") calendarZoom.reset();
      else calendarZoom.zoomStep(action === "in" ? 1 : -1);
    }
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      clearInterval(interval);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("keydown", handleKeyDown);
      scrollContainer?.removeEventListener("scroll", handleScroll);
      timelineWheelScroll.cancel();
    };
  });

  // Sync --hour-h when hourHeight changes outside of a zoom animation
  // (e.g. button-driven zoom). Also adjusts scrollTop to keep the
  // center time stable. Skipped during animations because the store
  // manages --hour-h imperatively until commit.
  $effect(() => {
    const nextHourHeight = calendarZoom.hourHeight;
    if (!scrollContainer || calendarZoom.isAnimating) return;

    const previousHourHeightValue = scrollContainer.style.getPropertyValue("--hour-h");
    const previousHourHeight = previousHourHeightValue ? parseFloat(previousHourHeightValue) : nextHourHeight;

    if (previousHourHeight !== nextHourHeight && previousHourHeight > 0) {
      // Compute center time at old zoom level
      const viewportHeight = scrollContainer.clientHeight;
      const centerOffset = viewportHeight / 2;
      const centerMinute = scrollMinuteFromTop(scrollContainer.scrollTop + centerOffset, previousHourHeight);

      scrollContainer.style.setProperty("--hour-h", String(nextHourHeight));

      // Adjust scrollTop to keep the same center time visible
      const nextScrollTop = scrollTopForMinute(centerMinute, nextHourHeight) - centerOffset;
      scrollContainer.scrollTop = Math.max(0, nextScrollTop);
    } else {
      scrollContainer.style.setProperty("--hour-h", String(nextHourHeight));
    }
    updateVisibleMinuteRange();
  });

  let previousAllDayOverlayHeight = 0;
  $effect(() => {
    const nextAllDayOverlayHeight = allDayOverlayHeight;
    if (scrollContainer && previousAllDayOverlayHeight !== nextAllDayOverlayHeight) {
      const delta = nextAllDayOverlayHeight - previousAllDayOverlayHeight;
      if (scrollContainer.scrollTop > 0) {
        scrollContainer.scrollTop = Math.max(0, scrollContainer.scrollTop + delta);
      }
    }
    previousAllDayOverlayHeight = nextAllDayOverlayHeight;
    void gutterTopHeight;
    updateVisibleMinuteRange();
  });

  function handleScroll() {
    lastScrollAt = performance.now();
    updateVisibleMinuteRange();
    if (!scrollContainer || !onScrollChange || calendarZoom.isAnimating) return;
    const minute = scrollMinuteFromTop(scrollContainer.scrollTop, calendarZoom.hourHeight);
    onScrollChange(Math.round(minute));
  }

  // Column date resolution for cross-column move drag
  function getColumnDate(clientX: number): string {
    const gridEl = scrollContainer?.querySelector(".week-grid");
    const cols = gridEl?.querySelectorAll(".day-col");
    if (!cols?.length) return formatDatePart(visibleDays[0]);

    for (let i = 0; i < cols.length; i++) {
      const rect = cols[i].getBoundingClientRect();
      if (clientX >= rect.left && clientX < rect.right) {
        return formatDatePart(visibleDays[i]);
      }
    }
    // Fallback: closest edge
    const firstRect = cols[0].getBoundingClientRect();
    if (clientX < firstRect.left) return formatDatePart(visibleDays[0]);
    return formatDatePart(visibleDays[dayCount - 1]);
  }

  const pomodoroStore = getPomodoro();

  function isActiveCalendarEvent(event: CalendarEvent): boolean {
    return eventMatchesActiveOccurrence(event, {
      occurrenceId: pomodoroStore.activeOccurrenceId,
    });
  }

  function isLockedCalendarEvent(id: string): boolean {
    const ev = events.find((event) => event.id === id);
    if (!ev) return false;
    if (isActiveCalendarEvent(ev)) return false;
    return hasCalendarEventStarted(ev);
  }

  const drag = createTimedEventDragController({
    events: () => events,
    hourHeight: () => calendarZoom.hourHeight,
    getColumnDate,
    getScrollContainer: () => scrollContainer ?? null,
    onEventUpdate: (e) => onEventUpdate(e),
    onEventCreate: (s, e, anchor) => onEventCreate(s, e, false, anchor),
    canDrag: (id) => editingId ? id === editingId : !previewedIds || !previewedIds.has(id),
    isActiveEvent: isActiveCalendarEvent,
    isEventLocked: isLockedCalendarEvent,
    mobileLayout: () => mobileLayout,
    onTouchEditStart: () => onMobileTouchEditStart?.(),
    onTouchEditEnd: () => onMobileTouchEditEnd?.(),
  });

  // All-day column bounds from header cells
  let allDayGridEl: HTMLDivElement | undefined = $state();

  function getAllDayColumnBounds(): DOMRect[] {
    if (!allDayGridEl) return [];
    const cells = allDayGridEl.querySelectorAll<HTMLElement>(".allday-col-target");
    return Array.from(cells, (c) => c.getBoundingClientRect());
  }

  const allDayDrag = createAllDayEventDragController({
    events: () => events,
    days: () => visibleDays,
    getColumnBounds: getAllDayColumnBounds,
    getPositionedEvents: () => allDayPositioned,
    onEventUpdate: (e) => onEventUpdate(e),
    canDrag: (id) => editingId ? id === editingId : !previewedIds || !previewedIds.has(id),
    isEventLocked: isLockedCalendarEvent,
    mobileLayout: () => mobileLayout,
    onTouchEditStart: () => onMobileTouchEditStart?.(),
    onTouchEditEnd: () => onMobileTouchEditEnd?.(),
  });

  const allDayEffectiveRows = $derived.by(() => {
    const base = Math.max(1, allDayGridRows);
    const preview = allDayDrag.allDayDragPreview;
    if (!preview) return base;
    return Math.max(base, preview.row + 1);
  });

  // Per-column count of hidden events when collapsed
  const allDayOverflowPerCol = $derived.by(() => {
    if (!allDayCollapsible || allDayExpanded) return new Array(dayCount).fill(0) as number[];
    const counts = new Array(dayCount).fill(0) as number[];
    for (const pos of allDayPositioned) {
      if (pos.row >= ALL_DAY_MAX_VISIBLE) {
        for (let c = pos.startCol; c < pos.startCol + pos.spanCols && c < counts.length; c++) {
          counts[c]++;
        }
      }
    }
    return counts;
  });

  function allDayCreateAnchorFromHeader(target: HTMLElement): PanelAnchor {
    const rect = target.getBoundingClientRect();
    return {
      x: rect.right,
      y: rect.bottom + ALL_DAY_PAD,
      width: rect.width,
      height: ALL_DAY_ROW_H,
    };
  }

  function allDayCreateAnchorFromGrid(target: HTMLElement): PanelAnchor {
    const rect = target.getBoundingClientRect();
    return {
      x: rect.right,
      y: rect.top + ALL_DAY_PAD,
      width: rect.width,
      height: ALL_DAY_ROW_H,
    };
  }

  </script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="flex h-full flex-col" style="visibility: {ready ? 'visible' : 'hidden'};">
<div class="relative min-h-0 flex-1" style="background-color: var(--cal-header-bg);">
  <div
    class="absolute left-0 right-0 top-0 z-40 grid"
    style="grid-template-columns: {gridCols}; background-color: var(--cal-header-bg);"
  >
    <!-- Header row -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      bind:offsetHeight={stickyHeaderHeight}
      class="grid {allDayMaxRow === 0 ? 'border-b border-sidebar' : ''}"
      onwheel={handleHeaderWheel}
      style="
        grid-column: 1 / -1;
        grid-template-columns: subgrid;
        height: var(--cal-header-row-h);
        background-color: var(--cal-header-bg);
      "
    >
      <TimezoneSelector
        {timezones}
        timezoneCount={timezoneCount}
        abbrMode={timezoneAbbreviationMode}
        onAdd={(tz) => onAddTimezone?.(tz)}
        onRemove={(i) => onRemoveTimezone?.(i)}
        onReorder={(from, to) => onReorderTimezone?.(from, to)}
        onAbbrModeChange={(m) => onTimezoneAbbreviationModeChange?.(m)}
      />

      <div
        class="grid"
        style="grid-column: span {dayCount}; grid-template-columns: subgrid;"
      >
        {#each visibleDays as day, i}
          {@const past = formatDatePart(day) < todayStr}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            bind:this={headerCells[i]}
            class="relative flex flex-col"
          >
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="flex flex-1 cursor-pointer items-center justify-center hover:bg-accent/50"
              onclick={() => onDayHeaderClick?.(day)}
              role="button"
              tabindex="-1"
            >
              <span class="text-[0.866667rem]" style="color: {past ? 'var(--muted-foreground)' : 'var(--foreground)'};">
                {#if dayFormat !== "none"}{formatDayName(day, dayFormat, locale)}&nbsp;{/if}{day.getDate()}
              </span>
            </div>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="absolute inset-x-0 bottom-0 cursor-pointer transition-colors hover:bg-foreground/10"
              style="height: 6px;"
              onclick={(e) => {
                e.stopPropagation();
                const dateStr = formatDatePart(day);
                onEventCreate(
                  `${dateStr} 00:00`,
                  `${dateStr} 00:00`,
                  true,
                  allDayCreateAnchorFromHeader(e.currentTarget as HTMLElement),
                );
              }}
            ></div>
          </div>
        {/each}
      </div>
    </div>

    {#if allDayMaxRow > 0}
    <!-- All-day banner -->
    <div
      bind:offsetHeight={allDayOverlayHeight}
      class="grid border-b border-sidebar"
      onwheel={scrollTimelineByWheel}
      style="
        grid-column: 1 / -1;
        grid-template-columns: subgrid;
        background-color: var(--cal-header-bg);
      "
    >
      <!-- Gutter spacer -->
      <div style="grid-column: span {timezoneCount};"></div>
      <!-- Event grid -->
      <div
        bind:this={allDayGridEl}
        data-calendar-edit-close-zone
        class="relative grid"
        style="
          grid-column: span {dayCount};
          grid-template-columns: subgrid;
          grid-template-rows: repeat({allDayEffectiveRows}, {ALL_DAY_ROW_H}px);
          padding: {ALL_DAY_PAD}px 0;
          gap: {ALL_DAY_GAP}px 0;
        "
      >
        <!-- Column measurement targets -->
        {#each visibleDays as day, i}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="allday-col-target cursor-pointer"
            style="grid-column: {i + 1}; grid-row: 1 / -1; z-index: 0;"
            onclick={(e) => {
              e.stopPropagation();
              const dateStr = formatDatePart(day);
              onEventCreate(
                `${dateStr} 00:00`,
                `${dateStr} 00:00`,
                true,
                allDayCreateAnchorFromGrid(e.currentTarget as HTMLElement),
              );
            }}
          ></div>
        {/each}

        <!-- Positioned all-day events: absolutely positioned to avoid
             CSS Grid relayout when chip classes change (glow on select). -->
        {#each allDayPositioned as pos (pos.event.id)}
          {@const endDateStr = pos.event.end.split(" ")[0]}
          {@const visible = pos.event.id !== allDayDrag.draggingEventId && (allDayExpanded || pos.row < ALL_DAY_MAX_VISIBLE || !allDayCollapsible)}
          <div
            class="absolute flex items-center px-0.5"
            style="
              left: {(pos.startCol / dayCount) * 100}%;
              width: {(pos.spanCols / dayCount) * 100}%;
              top: {ALL_DAY_PAD + pos.row * (ALL_DAY_ROW_H + ALL_DAY_GAP)}px;
              height: {ALL_DAY_ROW_H}px;
              z-index: 2;
              min-width: 0;
              {visible ? '' : 'display: none;'}
            "
          >
            <AllDayEventChip
              event={pos.event}
              {theme}
              editing={editingId === pos.event.id}
              preview={previewedIds?.has(pos.event.id) ?? false}
              grabbing={allDayDrag.grabbingId === pos.event.id}
              canDrag={(!editingId || pos.event.id === editingId) && !isLockedCalendarEvent(pos.event.id)}
              {mobileLayout}
              isPast={endDateStr < todayStr}
              onClick={(rect) => { if (!allDayDrag.didDrag) onEventClick(pos.event, rect); }}
              onPrefetch={() => onEventPrefetch?.(pos.event)}
              onPointerDown={(e) => allDayDrag.handleDragStart(pos.event.id, e)}
            />
          </div>
        {/each}

        <!-- "+N more" per column when collapsed -->
        {#if allDayCollapsible && !allDayExpanded}
          {#each allDayOverflowPerCol as count, i}
            {#if count > 0}
              <button
                type="button"
                class="absolute z-3 flex items-center px-1.5 text-[0.666667rem] text-muted-foreground hover:text-foreground"
                style="
                  left: {(i / dayCount) * 100}%;
                  width: {(1 / dayCount) * 100}%;
                  top: {ALL_DAY_PAD + ALL_DAY_MAX_VISIBLE * (ALL_DAY_ROW_H + ALL_DAY_GAP)}px;
                  height: {ALL_DAY_ROW_H}px;
                "
                onclick={(e) => { e.stopPropagation(); allDayExpanded = true; }}
              >
                {t("calendar.moreEvents", formatNumber(locale, count))}
              </button>
            {/if}
          {/each}
        {/if}

        <!-- Drag preview -->
        {#if allDayDrag.allDayDragPreview}
          {@const preview = allDayDrag.allDayDragPreview}
          {@const previewColor = getEventColor(preview.event.color, theme)}
          {@const previewIconColor = `color-mix(in srgb, ${previewColor.text} 70%, ${previewColor.bg})`}
          {@const previewIndicators = getEventIndicatorState(preview.event)}
          <div
            class="pointer-events-none absolute flex items-center px-0.5"
            style="
              left: {(preview.startCol / dayCount) * 100}%;
              width: {(preview.spanCols / dayCount) * 100}%;
              top: {ALL_DAY_PAD + preview.row * (ALL_DAY_ROW_H + ALL_DAY_GAP)}px;
              height: {ALL_DAY_ROW_H}px;
              z-index: 10;
              min-width: 0;
            "
          >
          <div
            class="allday-drag-preview min-w-0 flex-1 select-none truncate rounded px-1.5 text-[0.666667rem] leading-5"
            style="
              background-color: {previewColor.bg};
              color: {previewColor.text};
              --event-bg: {previewColor.bg};
              --outline-mix: {previewColor.text};
            "
          >
            {#if previewIndicators.iconCount > 0}
              <span class="absolute right-1 top-0.75 flex items-center gap-0.5" style="color: {previewIconColor};">
                {#if previewIndicators.hasRepeat}
                  <Repeat size={8} class="shrink-0" />
                {/if}
                {#if previewIndicators.hasCallLink}
                  <Video size={8} class="shrink-0" />
                {/if}
                {#if previewIndicators.hasLocation}
                  <MapPin size={8} class="shrink-0" />
                {/if}
                {#if previewIndicators.hasGenericMeeting}
                  <Users size={8} class="shrink-0" />
                {/if}
              </span>
            {/if}
            <span
              class="truncate"
              class:pr-5={previewIndicators.iconCount > 0 && previewIndicators.iconCount <= 2}
              class:pr-8={previewIndicators.iconCount > 2}
            >
              {#if preview.event.title}{preview.event.title}{:else}{t("calendar.event.noTitle")}{/if}
            </span>
          </div>
          </div>
        {/if}

      </div>
    </div>
    {/if}
  </div>

  <div
    bind:this={scrollContainer}
    data-calendar-scroll-container
    onwheel={scrollTimelineByWheel}
    class="hide-scrollbar absolute inset-x-0 bottom-0 overflow-y-auto overflow-x-hidden"
    style="top: {gutterTopHeight}px; --hour-h: {calendarZoom.hourHeight}; background-color: var(--cal-bg);"
  >
    <div class="relative" style="height: calc({allDayOverlayHeight}px + 24 * var(--hour-h) * 1px);">
    <div
      class="week-grid absolute inset-x-0 top-0 grid"
      style="grid-template-columns: {gridCols}; transform: translateY({allDayOverlayHeight}px);"
    >
      <!-- Body: one cell per timezone + 7 day columns -->
      <div
        data-zoom-body
        class="grid"
        style="grid-column: 1 / -1; grid-template-columns: subgrid; {calendarZoom.isAnimating ? 'pointer-events: none;' : ''}"
      >
      <TimeGutter {timezones} {anchorDate} timezoneCount={timezoneCount} />

      <div
        data-calendar-edit-close-zone
        class="relative grid"
        style="grid-column: span {dayCount}; grid-template-columns: subgrid;"
      >
        <HourGridlines />
        {#each visibleDays as day, i}
          {@const dateStr = formatDatePart(day)}
          <div data-day-column-shell class="day-col min-w-0" style="border-left: 1px solid var(--cal-gridline);">
            <DayColumn
              date={day}
              positionedEvents={positionedTimedEventsByDay.get(dateStr) ?? EMPTY_POSITIONED}
              {theme}
              isToday={formatDatePart(day) === todayStr}
              isPast={formatDatePart(day) < todayStr}
              {currentTimeMinute}
              {editingId}
              {previewedIds}
              {persistedSegmentsByEvent}
              draggingEventId={drag.dragPreview ? drag.dragState?.eventId : undefined}
              grabbingId={drag.grabbingId ?? undefined}
              didDrag={drag.didDrag}
              dragPreview={drag.getDragPreviewForDate(dateStr)}
              createPreview={drag.getCreatePreviewForDate(dateStr)}
              {visibleStartMinute}
              {visibleEndMinute}
              onEventClick={onEventClick}
              onEventPrefetch={onEventPrefetch}
              onDragStart={drag.handleDragStart}
              onCreateStart={drag.handleCreateStart}
              isActiveEvent={isActiveCalendarEvent}
              isEventLocked={isLockedCalendarEvent}
              allowPointerEditing={true}
              {mobileLayout}
            />
          </div>
        {/each}

              </div>
      </div>
    </div>
    </div>
  </div>
  <CalendarScrollbar {scrollContainer} stickyTop={gutterTopHeight + allDayOverlayHeight} onTimelineWheel={scrollTimelineByWheel} />
</div>
</div>

<style>
  .allday-drag-preview {
    position: relative;
  }

  .allday-drag-preview::after {
    content: "";
    position: absolute;
    inset: 0;
    border: 1px solid color-mix(in oklab, var(--event-bg) 65%, var(--outline-mix));
    border-radius: inherit;
    pointer-events: none;
    z-index: 3;
  }
</style>
