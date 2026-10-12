<script lang="ts">
  import { tick } from "svelte";
  import type { CalendarEvent } from "$lib/calendar/types";
  import type { DayNameFormat } from "$lib/calendar/utils";
  import {
    getMonthGrid,
    isPastDay,
    getEventColor,
    getEventStatusPatternClass,
    getPastEventColor,
    getOutsideMonthEventColor,
    isEventSurfaceCancelled,
    formatDayName,
    formatDatePart,
    formatTimeRange,
  } from "$lib/calendar/utils";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { Theme } from "$lib/themes";
  import { resolveAppTokens, resolveCalendarTokens } from "$lib/themes";
  import { dayNumberShade } from "$lib/color/math";
  import {
    layoutMonthDayEvents,
    MONTH_EVENT_CHIP_HEIGHT_PX,
    MONTH_EVENT_ROW_GAP_PX,
    type MonthDayLayoutItem,
    type MonthMoreLabelFormatter,
  } from "./month-event-layout";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getMeetingIndicatorState } from "./event-indicators";
  import Video from "@lucide/svelte/icons/video";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import Users from "@lucide/svelte/icons/users";
  import X from "@lucide/svelte/icons/x";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    anchorDate,
    eventsByDay,
    theme,
    onDayClick,
    onEventClick,
    onEventPrefetch,
    onRequestPanelClose,
    onWheelNavigate,
  }: {
    anchorDate: Date;
    eventsByDay: Map<string, CalendarEvent[]>;
    theme: Theme;
    onDayClick: (date: Date) => void;
    onEventClick: (event: CalendarEvent, rect?: DOMRect) => void;
    onEventPrefetch?: (event: CalendarEvent) => void;
    onRequestPanelClose?: () => void;
    onWheelNavigate?: (direction: "back" | "forward") => void;
  } = $props();

  const preferences = getPreferences();
  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);

  /** Stable empty fallback for days with no events. */
  const EMPTY_DAY: CalendarEvent[] = [];
  const MONTH_CELL_PAD_X_PX = 4;
  const MONTH_CELL_PAD_Y_PX = 4;
  const MONTH_DAY_NUMBER_HEIGHT_PX = 24;
  const MONTH_DAY_NUMBER_MARGIN_PX = 1;
  const MONTH_CELL_BORDER_PX = 1;
  const MONTH_CELL_RIGHT_EDGE_INSET_PX = 1;
  const MONTH_MORE_MODAL_MIN_WIDTH_PX = 280;
  const MONTH_MORE_MODAL_MAX_WIDTH_PX = 560;
  const MONTH_MORE_MODAL_EXTRA_WIDTH_PX = 96;

  interface MonthMoreModalState {
    day: Date;
    events: CalendarEvent[];
  }

  /**
   * Per-cell ordering: all-day events first, sorted by title then start;
   * timed events second, sorted by start. Works on the pre-bucketed day slice
   * so cells never scan the full visible-event array.
   */
  function sortDayCellEvents(dayEvents: CalendarEvent[]): CalendarEvent[] {
    if (dayEvents.length === 0) return dayEvents;
    const allDay: CalendarEvent[] = [];
    const timed: CalendarEvent[] = [];
    for (const e of dayEvents) (e.allDay ? allDay : timed).push(e);
    if (allDay.length > 1) {
      allDay.sort((a, b) => {
        const at = (a.title || "").toLowerCase();
        const bt = (b.title || "").toLowerCase();
        if (at !== bt) return at < bt ? -1 : 1;
        return a.start.localeCompare(b.start);
      });
    }
    if (timed.length > 1) {
      timed.sort((a, b) => a.start.localeCompare(b.start));
    }
    if (allDay.length === 0) return timed;
    if (timed.length === 0) return allDay;
    return allDay.concat(timed);
  }

  let wheelCooldown = false;

  function handleHeaderWheel(e: WheelEvent) {
    e.preventDefault();
    if (!onWheelNavigate || wheelCooldown) return;
    if (Math.abs(e.deltaY) < 5) return;
    wheelCooldown = true;
    onWheelNavigate(e.deltaY > 0 ? "forward" : "back");
    setTimeout(() => { wheelCooldown = false; }, 300);
  }

  const currentMonth = $derived(anchorDate.getMonth());
  const weeks = $derived(
    getMonthGrid(anchorDate.getFullYear(), anchorDate.getMonth()),
  );

  const shadeRefs = $derived.by(() => {
    const app = resolveAppTokens(theme);
    const calendarTokens = resolveCalendarTokens(theme);
    return { bg: calendarTokens["--cal-bg"], ink: app["--foreground"] };
  });

  // Reference days for day-of-week headers (Mon-Sun from first week)
  const dayNameDates = $derived(weeks[0]);

  // Responsive day name format based on column width
  let headerCells: HTMLElement[] = $state([]);
  let headerCellWidths: number[] = $state([]);
  let dayFormat: DayNameFormat = $state("short");
  let monthGridEl: HTMLDivElement | undefined = $state();
  let monthGridWidth = $state(0);
  let monthGridHeight = $state(0);
  let monthMoreModal: MonthMoreModalState | undefined = $state();
  let monthMoreDialogEl: HTMLDivElement | undefined = $state();
  let monthMoreScrollEl: HTMLDivElement | undefined = $state();

  $effect(() => {
    const observedCells = headerCells.filter((el): el is HTMLElement => !!el);
    if (observedCells.length === 0) return;
    const observer = new ResizeObserver((entries) => {
      const next = [...headerCellWidths];
      for (const entry of entries) {
        const index = headerCells.indexOf(entry.target as HTMLElement);
        if (index < 0) continue;
        next[index] = resizeObserverBox(entry).width;
      }
      headerCellWidths = next;
      const firstWidth = next[0] ?? 0;
      if (firstWidth >= 110) dayFormat = "long";
      else if (firstWidth >= 60) dayFormat = "short";
      else dayFormat = "narrow";
    });
    for (const el of observedCells) observer.observe(el);
    return () => observer.disconnect();
  });

  function resizeObserverBox(entry: ResizeObserverEntry): { width: number; height: number } {
    const rawBox = entry.contentBoxSize;
    const box = Array.isArray(rawBox) ? rawBox[0] : rawBox;
    return {
      width: box?.inlineSize ?? entry.contentRect.width,
      height: box?.blockSize ?? entry.contentRect.height,
    };
  }

  $effect(() => {
    const el = monthGridEl;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = resizeObserverBox(entry);
      monthGridWidth = Math.max(0, width);
      monthGridHeight = Math.max(0, height);
    });
    observer.observe(el);
    return () => observer.disconnect();
  });

  const formatMoreLabel: MonthMoreLabelFormatter = $derived.by(() => {
    const currentLocale = locale;
    return (hiddenCount, variant) => {
      const count = formatNumber(currentLocale, hiddenCount);
      return variant === "full" ? t("calendar.moreEvents", count) : t("calendar.moreEventsCompact", count);
    };
  });

  const untitledLabel = $derived(t("calendar.event.noTitle"));

  const monthLayoutOptionsByColumn = $derived.by(() => {
    const cellHeightPx = monthGridHeight > 0 && weeks.length > 0
      ? monthGridHeight / weeks.length
      : 0;
    const availableHeightPx = Math.max(
      0,
      cellHeightPx
        - MONTH_CELL_PAD_Y_PX * 2
        - MONTH_DAY_NUMBER_HEIGHT_PX
        - MONTH_DAY_NUMBER_MARGIN_PX,
    );
    return Array.from({ length: 7 }, (_, i) => {
      const fallbackColumnWidth = monthGridWidth > 0 ? monthGridWidth / 7 : 0;
      const columnWidth = headerCellWidths[i] ?? fallbackColumnWidth;
      const borderWidth = i < 6 ? MONTH_CELL_BORDER_PX : 0;
      const cellWidthPx = Math.max(
        0,
        columnWidth
          - MONTH_CELL_PAD_X_PX * 2
          - borderWidth
          - MONTH_CELL_RIGHT_EDGE_INSET_PX,
      );
      return {
        cellWidthPx,
        availableHeightPx,
        chipHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
        rowGapPx: MONTH_EVENT_ROW_GAP_PX,
        formatMoreLabel,
        untitledLabel,
      };
    });
  });

  const fallbackMonthLayoutOptions = $derived({
    cellWidthPx: 0,
    availableHeightPx: 0,
    chipHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    rowGapPx: MONTH_EVENT_ROW_GAP_PX,
    formatMoreLabel,
    untitledLabel,
  });

  function monthLayoutOptionsForColumn(columnIndex: number) {
    return monthLayoutOptionsByColumn[columnIndex] ?? fallbackMonthLayoutOptions;
  }

  function monthLayoutItemKey(item: MonthDayLayoutItem): string {
    return item.kind === "event" ? `event:${item.event.id}` : "more";
  }

  function displayEventTitle(event: CalendarEvent): string {
    const title = event.title.trim();
    return title.length > 0 ? title : untitledLabel;
  }

  function eventTimeRangeLabel(event: CalendarEvent): string {
    return event.allDay
      ? t("calendar.event.allDay")
      : formatTimeRange(event.start, event.end, preferences.calendarTimeFormat);
  }

  function monthMoreEventLabel(event: CalendarEvent): string {
    return `${displayEventTitle(event)} · ${eventTimeRangeLabel(event)}`;
  }

  function formatMonthMoreModalTitle(day: Date): string {
    return new Intl.DateTimeFormat(locale, {
      weekday: "long",
      month: "long",
      day: "numeric",
      year: "numeric",
    }).format(day);
  }

  function openMonthMoreModal(day: Date, events: CalendarEvent[]): void {
    monthMoreModal = {
      day: new Date(day),
      events: [...events],
    };
  }

  function closeMonthMoreModal(): void {
    onRequestPanelClose?.();
    monthMoreModal = undefined;
  }

  function handleMonthMoreModalKeydown(e: KeyboardEvent): void {
    if (!monthMoreModal) return;
    if (e.target instanceof Element && e.target.closest(".panel-root, .confirm-dialog") !== null) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closeMonthMoreModal();
      return;
    }
    e.stopPropagation();
  }

  function openEventFromMonthMoreModal(event: CalendarEvent, rect: DOMRect): void {
    onEventClick(event, rect);
  }

  $effect(() => {
    if (!monthMoreModal) return;
    void tick().then(() => {
      monthMoreDialogEl?.focus();
    });
  });

  const monthMoreModalWidthPx = $derived.by(() => {
    if (!monthMoreModal) return MONTH_MORE_MODAL_MIN_WIDTH_PX;
    const longestLabelLength = monthMoreModal.events.reduce(
      (max, event) => Math.max(max, monthMoreEventLabel(event).length),
      0,
    );
    return Math.min(
      MONTH_MORE_MODAL_MAX_WIDTH_PX,
      Math.max(
        MONTH_MORE_MODAL_MIN_WIDTH_PX,
        longestLabelLength * 5.8 + MONTH_MORE_MODAL_EXTRA_WIDTH_PX,
      ),
    );
  });
</script>

<svelte:window onkeydown={handleMonthMoreModalKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="flex h-full flex-col overflow-hidden"
  style="background-color: var(--cal-bg);"
  onwheel={handleHeaderWheel}
>
  <!-- Day name headers (matches week/day view design) -->
  <div
    class="sticky top-0 z-10 shrink-0"
    style="
      height: var(--cal-header-row-h);
      background-color: var(--cal-header-bg);
      border-bottom: 1px solid var(--sidebar);
    "
  >
    <div class="grid h-full grid-cols-7">
      {#each dayNameDates as day, i}
        <div
          bind:this={headerCells[i]}
          class="flex items-center justify-center"
        >
          <span class="text-[0.866667rem]" style="color: var(--foreground);">
            {formatDayName(day, dayFormat, locale)}
          </span>
        </div>
      {/each}
    </div>
  </div>

  <!-- Week rows -->
  <div
    bind:this={monthGridEl}
    data-calendar-edit-close-zone
    class="grid min-h-0 flex-1 overflow-x-hidden"
    style="grid-template-rows: repeat({weeks.length}, minmax(0, 1fr));"
  >
    {#each weeks as week, weekIndex}
      <div
        class="grid grid-cols-7"
        style="{weekIndex < weeks.length - 1 ? `border-bottom: 1px solid var(--cal-gridline);` : ''}"
      >
        {#each week as day, dayIndex}
          {@const inMonth = day.getMonth() === currentMonth}
          {@const past = isPastDay(day)}
          {@const active = inMonth && !past}
          {@const dayNumberColor = dayNumberShade(
            shadeRefs.bg,
            shadeRefs.ink,
            inMonth,
            past,
          )}
          {@const moreColor = dayNumberShade(
            shadeRefs.bg,
            shadeRefs.ink,
            inMonth,
            past,
          )}
          {@const dateStr = formatDatePart(day)}
          {@const dayEvents = sortDayCellEvents(eventsByDay.get(dateStr) ?? EMPTY_DAY)}
          {@const dayLayout = layoutMonthDayEvents(dayEvents, monthLayoutOptionsForColumn(dayIndex))}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="relative flex min-h-0 cursor-pointer flex-col overflow-hidden p-1"
            style="{dayIndex < 6 ? `border-right: 1px solid var(--cal-gridline);` : ''}"
            onclick={() => onDayClick(day)}
          >
            <span
              class="relative z-2 mb-px flex h-6 w-6 items-center justify-center self-center text-xs"
              style="color: {dayNumberColor};"
            >
              {day.getDate()}
            </span>

            <div class="relative min-h-0 flex-1 overflow-hidden">
              {#each dayLayout.items as item (monthLayoutItemKey(item))}
                {#if item.kind === "event"}
                  {@const evt = item.event}
                  {@const evtColors = preferences.calendarDimPastEvents && past
                    ? getPastEventColor(evt.color, theme)
                    : !inMonth
                      ? getOutsideMonthEventColor(evt.color, theme)
                      : getEventColor(evt.color, theme)}
                  {@const evtIsCancelled = isEventSurfaceCancelled(evt)}
                  {@const evtStatusPatternClass = getEventStatusPatternClass(evt)}
                  {@const evtMeetingIndicators = getMeetingIndicatorState(evt)}
                  {@const evtIconColor = `color-mix(in srgb, ${evtColors.text} 70%, ${evtColors.bg})`}
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <div
                    data-event-id={evt.id}
                    class="month-event-surface absolute z-2 flex min-w-0 items-center gap-1 overflow-hidden rounded px-1 text-[0.666667rem] leading-5 {evtStatusPatternClass}"
                    style="
                      left: {item.leftPx}px;
                      top: {item.topPx}px;
                      width: {item.widthPx}px;
                      height: {item.heightPx}px;
                      background-color: {evtColors.bg};
                      color: {evtColors.text};
                    "
                    onpointerenter={() => onEventPrefetch?.(evt)}
                    onpointerdown={() => onEventPrefetch?.(evt)}
                    onclick={(e) => { e.stopPropagation(); onEventClick(evt, (e.currentTarget as HTMLElement).getBoundingClientRect()); }}
                  >
                    <span class="relative z-10 min-w-0 flex-1 truncate" style={evtIsCancelled ? 'text-decoration: line-through;' : ''}>{displayEventTitle(evt)}</span>
                    {#if evtMeetingIndicators.iconCount > 0}
                      <span class="relative z-10 flex shrink-0 items-center gap-0.5" style="color: {evtIconColor};">
                        {#if evtMeetingIndicators.hasCallLink}
                          <Video size={8} class="shrink-0" />
                        {/if}
                        {#if evtMeetingIndicators.hasLocation}
                          <MapPin size={8} class="shrink-0" />
                        {/if}
                        {#if evtMeetingIndicators.hasGenericMeeting}
                          <Users size={8} class="shrink-0" />
                        {/if}
                      </span>
                    {/if}
                  </div>
                {:else}
                  <button
                    type="button"
                    aria-label={t(
                      "calendar.event.showAllEventsFor",
                      formatMonthMoreModalTitle(day),
                    )}
                    title={t("calendar.event.showAllEvents")}
                    class="month-more-surface absolute z-2 flex min-w-0 cursor-pointer items-center justify-center overflow-hidden truncate rounded px-1 text-[0.666667rem] leading-5"
                    style="
                      left: {item.leftPx}px;
                      top: {item.topPx}px;
                      width: {item.widthPx}px;
                      height: {item.heightPx}px;
                      --month-more-color: {moreColor};
                      color: var(--month-more-color);
                    "
                    onclick={(e) => { e.stopPropagation(); openMonthMoreModal(day, dayEvents); }}
                  >
                    {item.label}
                  </button>
                {/if}
              {/each}
            </div>
          </div>
        {/each}
      </div>
    {/each}
  </div>
</div>

{#if monthMoreModal}
  {@const modalPast = isPastDay(monthMoreModal.day)}
  {@const modalInMonth = monthMoreModal.day.getMonth() === currentMonth}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-40 flex items-center justify-center px-3 py-4"
    onclick={(e) => { e.stopPropagation(); closeMonthMoreModal(); }}
  >
    <div class="surface-backdrop absolute inset-0"></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      bind:this={monthMoreDialogEl}
      class="month-more-dialog surface-dialog relative z-10 flex max-h-[min(82vh,42rem)] min-h-0 flex-col text-foreground outline-none"
      style="
        width: min(calc(100vw - 1.5rem), {monthMoreModalWidthPx}px);
        background-color: var(--cal-bg);
      "
      role="dialog"
      aria-modal="true"
      aria-label={formatMonthMoreModalTitle(monthMoreModal.day)}
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
    >
      <div data-calendar-edit-close-zone class="flex shrink-0 items-center justify-between gap-3 border-b border-border px-3 py-2.5">
        <div class="flex min-h-7 min-w-0 items-center">
          <h2 class="truncate text-sm font-semibold leading-7 text-foreground">
            {formatMonthMoreModalTitle(monthMoreModal.day)}
          </h2>
        </div>
        <button
          type="button"
          class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          aria-label={t("common.close")}
          onclick={closeMonthMoreModal}
        >
          <X size={14} />
        </button>
      </div>

      <div class="relative min-h-0">
        <div
          bind:this={monthMoreScrollEl}
          use:scrollEdgeFadeAction
          class="hide-scrollbar min-h-0 overflow-y-auto overflow-x-hidden px-3 py-3"
          style="max-height: min(62vh, 34rem);"
        >
          <div class="flex min-w-0 flex-col gap-1.5">
            {#each monthMoreModal.events as evt (evt.id)}
              {@const evtColors = preferences.calendarDimPastEvents && modalPast
                ? getPastEventColor(evt.color, theme)
                : !modalInMonth
                  ? getOutsideMonthEventColor(evt.color, theme)
                  : getEventColor(evt.color, theme)}
              {@const evtIsCancelled = isEventSurfaceCancelled(evt)}
              {@const evtStatusPatternClass = getEventStatusPatternClass(evt)}
              {@const evtMeetingIndicators = getMeetingIndicatorState(evt)}
              {@const evtIconColor = `color-mix(in srgb, ${evtColors.text} 70%, ${evtColors.bg})`}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                data-event-id={evt.id}
                class="month-event-surface relative flex h-7 min-w-0 cursor-pointer items-center gap-1.5 overflow-hidden rounded px-1.5 text-[0.8rem] leading-7 {evtStatusPatternClass}"
                style="
                  background-color: {evtColors.bg};
                  color: {evtColors.text};
                "
                onpointerenter={() => onEventPrefetch?.(evt)}
                onpointerdown={() => onEventPrefetch?.(evt)}
                onclick={(e) => {
                  e.stopPropagation();
                  openEventFromMonthMoreModal(evt, (e.currentTarget as HTMLElement).getBoundingClientRect());
                }}
              >
                <span class="relative z-10 min-w-0 flex-1 truncate" style={evtIsCancelled ? "text-decoration: line-through;" : ""}>
                  {displayEventTitle(evt)}
                </span>
                {#if evtMeetingIndicators.iconCount > 0}
                  <span class="relative z-10 flex shrink-0 items-center gap-0.5" style="color: {evtIconColor};">
                    {#if evtMeetingIndicators.hasCallLink}
                      <Video size={9} class="shrink-0" />
                    {/if}
                    {#if evtMeetingIndicators.hasLocation}
                      <MapPin size={9} class="shrink-0" />
                    {/if}
                    {#if evtMeetingIndicators.hasGenericMeeting}
                      <Users size={9} class="shrink-0" />
                    {/if}
                  </span>
                {/if}
                <span class="relative z-10 shrink-0 whitespace-nowrap" style="color: {evtIconColor};">
                  {eventTimeRangeLabel(evt)}
                </span>
              </div>
            {/each}
          </div>
        </div>
        <CalendarScrollbar scrollContainer={monthMoreScrollEl} wheelPassthrough />
      </div>
    </div>
  </div>
{/if}

<style>
  .month-event-surface,
  .month-more-surface {
    box-sizing: border-box;
  }

  .month-event-surface::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 0;
    pointer-events: none;
  }

  .month-event-surface.event-pattern-tentative::before {
    background-image: linear-gradient(
      90deg,
      color-mix(in srgb, currentColor 10%, transparent) 0 1px,
      transparent 1px
    );
    background-repeat: repeat;
    background-size: 6px 100%;
  }

  .month-event-surface.event-pattern-declined::before {
    background-image: linear-gradient(
      135deg,
      transparent 0 44%,
      color-mix(in srgb, currentColor 11%, transparent) 44% 56%,
      transparent 56%
    );
    background-repeat: repeat;
    background-size: 7px 7px;
  }

  .month-event-surface.event-pattern-pending::before {
    background-image: radial-gradient(
      circle,
      color-mix(in srgb, currentColor 15%, transparent) 1px,
      transparent 1.3px
    );
    background-size: 8px 8px;
  }

  .month-more-surface {
    appearance: none;
    background-color: transparent;
    border: 1px solid transparent;
  }

  .month-more-surface:hover {
    background-color: transparent;
  }
</style>
