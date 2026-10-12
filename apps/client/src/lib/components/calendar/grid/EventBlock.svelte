<script lang="ts">
  import type { PositionedEvent } from "$lib/calendar/types";
  import {
    getEventColor,
    getEventStatusPatternClass,
    getPastEventColor,
    formatTimeRange,
    isEventSurfaceCancelled,
  } from "$lib/calendar/utils";
  import { getCalendarZoom } from "$lib/stores/calendar-zoom.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { isThemeCalendarDark, type Theme } from "$lib/themes";
  import { getEventIndicatorState } from "./event-indicators";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Video from "@lucide/svelte/icons/video";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import Users from "@lucide/svelte/icons/users";

  const calendarZoom = getCalendarZoom();
  const preferences = getPreferences();
  const { t } = getLocalization();

  let {
    positioned,
    theme,
    editing = false,
    preview = false,
    grabbing = false,
    animateLayout = false,
    canDrag = true,
    mobileLayout = false,
    isPast = false,
    inResizeZone = false,
    onClick,
    onPrefetch,
    onPointerDown,
  }: {
    positioned: PositionedEvent;
    theme: Theme;
    editing?: boolean;
    preview?: boolean;
    grabbing?: boolean;
    animateLayout?: boolean;
    canDrag?: boolean;
    mobileLayout?: boolean;
    isPast?: boolean;
    inResizeZone?: boolean;
    onClick: (rect?: DOMRect) => void;
    onPrefetch?: () => void;
    onPointerDown?: (e: PointerEvent) => void;
  } = $props();

  const isDark = $derived(isThemeCalendarDark(theme));

  const timeRange = $derived(
    formatTimeRange(
      positioned.event.start.split(" ")[1] ?? "",
      positioned.event.end.split(" ")[1] ?? "",
      preferences.calendarTimeFormat,
      "compact",
    ),
  );
  const indicators = $derived(getEventIndicatorState(positioned.event));
  const hasIcons = $derived(indicators.iconCount > 0);
  const isCancelled = $derived(isEventSurfaceCancelled(positioned.event));
  const eventTitle = $derived(positioned.event.title || t("calendar.event.noTitle"));
  const eventPixelHeight = $derived((positioned.durationMinutes / 60) * calendarZoom.hourHeight);

  const usePastColors = $derived(
    preferences.calendarDimPastEvents && isPast && !editing && !preview && !grabbing,
  );
  const statusPatternClass = $derived(getEventStatusPatternClass(positioned.event));
  const showContour = $derived(editing || preview || grabbing);
  const activeColors = $derived(
    usePastColors
      ? getPastEventColor(positioned.event.color, theme)
      : getEventColor(positioned.event.color, theme)
  );

  const timeColor = $derived(`color-mix(in srgb, ${activeColors.text} 80%, ${activeColors.bg})`);
  const locationColor = $derived(`color-mix(in srgb, ${activeColors.text} 60%, ${activeColors.bg})`);
  const iconColor = $derived(`color-mix(in srgb, ${activeColors.text} 70%, ${activeColors.bg})`);

  function handlePointerDown(e: PointerEvent) {
    onPrefetch?.();
    e.stopPropagation();
    onPointerDown?.(e);
  }

  let eventEl: HTMLDivElement | undefined = $state();

  function handleClick(e: MouseEvent | KeyboardEvent) {
    e.stopPropagation();
    const eventRect = eventEl?.getBoundingClientRect();
    const colRect = eventEl?.closest("[data-day-column]")?.getBoundingClientRect();
    // Use column boundaries for horizontal positioning so the panel clears the rail
    const rect = eventRect && colRect
      ? new DOMRect(colRect.x, eventRect.y, colRect.width, eventRect.height)
      : eventRect;
    onClick(rect);
  }

  function handleKeydown(e: KeyboardEvent): void {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    e.stopPropagation();
    if (!e.repeat) handleClick(e);
  }

  // Cursor is controlled by parent (DayColumn) via inResizeZone prop
  const effectiveCursor = $derived(
    !canDrag ? 'pointer' : inResizeZone ? 'ns-resize' : 'default'
  );
</script>

<div
  bind:this={eventEl}
  role="button"
  tabindex="0"
  aria-label={`${eventTitle} ${timeRange}`}
  data-event-id={positioned.event.id}
  data-clipped-top={positioned.isClippedTop || undefined}
  data-clipped-bottom={positioned.isClippedBottom || undefined}
  title={eventPixelHeight <= 14 ? `${eventTitle} ${timeRange}` : undefined}
  class="event-block-wrapper absolute flex text-[0.8rem] leading-tight select-none {statusPatternClass} {showContour ? 'event-editing' : ''} {animateLayout ? 'event-layout-transition' : ''} {mobileLayout ? 'mobile-event-block' : ''} overflow-hidden {positioned.isClippedTop && positioned.isClippedBottom ? '' : positioned.isClippedTop ? 'rounded-b' : positioned.isClippedBottom ? 'rounded-t' : 'rounded'}"
  style="
    top: calc({positioned.startMinute} / 60 * var(--hour-h) * 1px);
    height: calc({positioned.durationMinutes} / 60 * var(--hour-h) * 1px - {positioned.isClippedBottom || !positioned.hasEventBelow ? 0 : 2}px);
    left: {positioned.left}%;
    width: {positioned.totalColumns > 1 ? `calc(${positioned.width}% - 2px)` : `${positioned.width}%`};
    background-color: {activeColors.bg};
    color: {activeColors.text};
    --event-bg: {activeColors.bg};
    --outline-mix: {isDark ? 'white' : 'black'};
    cursor: {effectiveCursor};
    z-index: {editing ? 45 : 3};
    filter: none;
  "
  onclick={handleClick}
  onkeydown={handleKeydown}
  onfocus={onPrefetch}
  onpointerenter={onPrefetch}
  onpointerdown={handlePointerDown}
>
  <!-- Resize handle: top (hidden on clipped edge) -->
  {#if !positioned.isClippedTop}
    <div
      class="resize-handle-top"
    ></div>
  {/if}

  <div class="relative z-10 min-w-0 flex-1 overflow-hidden px-1 py-0.5">
    {#if hasIcons}
      <div class="event-icons absolute right-1 flex items-center gap-0.5" style="top: 5px; color: {iconColor};">
        {#if indicators.hasRepeat}
          <Repeat size={9} class="shrink-0" />
        {/if}
        {#if indicators.hasCallLink}
          <Video size={9} class="shrink-0" />
        {/if}
        {#if indicators.hasLocation}
          <MapPin size={9} class="shrink-0" />
        {/if}
        {#if indicators.hasGenericMeeting}
          <Users size={9} class="shrink-0" />
        {/if}
      </div>
    {/if}
    <div
      class="event-title truncate font-medium"
      class:pr-5={indicators.iconCount > 0 && indicators.iconCount <= 2}
      class:pr-8={indicators.iconCount > 2}
      style={isCancelled ? 'text-decoration: line-through;' : ''}
    >
      {eventTitle}
    </div>
    <div class="event-time truncate" style="color: {timeColor};">{timeRange}</div>
    {#if positioned.event.location}
      <div class="event-location truncate text-[0.666667rem]" style="color: {locationColor};">{positioned.event.location}</div>
    {/if}
  </div>

  <!-- Resize handle: bottom (hidden on clipped edge) -->
  {#if !positioned.isClippedBottom}
    <div
      class="resize-handle-bottom"
    ></div>
  {/if}
</div>

<style>
  .event-block-wrapper:focus-visible {
    outline: 2px solid var(--ring);
    outline-offset: -2px;
  }

  .resize-handle-top,
  .resize-handle-bottom {
    position: absolute;
    left: 0;
    right: 0;
    height: 11px;
    z-index: 20;
  }

  .resize-handle-top {
    top: -5px;
  }

  .resize-handle-bottom {
    bottom: -5px;
  }

  .event-block-wrapper {
    container-type: size;
    container-name: event-block;
  }

  .event-block-wrapper.event-layout-transition {
    transition: left 250ms cubic-bezier(0.25, 0.1, 0.25, 1), width 250ms cubic-bezier(0.25, 0.1, 0.25, 1);
  }

  .event-block-wrapper::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 0;
    pointer-events: none;
    border-radius: inherit;
  }

  .event-block-wrapper.event-pattern-tentative::before {
    background-image: linear-gradient(
      90deg,
      color-mix(in srgb, currentColor 10%, transparent) 0 1px,
      transparent 1px
    );
    background-repeat: repeat;
    background-size: 6px 100%;
  }

  .event-block-wrapper.event-pattern-declined::before {
    background-image: linear-gradient(
      135deg,
      transparent 0 44%,
      color-mix(in srgb, currentColor 11%, transparent) 44% 56%,
      transparent 56%
    );
    background-repeat: repeat;
    background-size: 7px 7px;
  }

  .event-block-wrapper.event-pattern-pending::before {
    background-image: radial-gradient(
      circle,
      color-mix(in srgb, currentColor 16%, transparent) 1px,
      transparent 1.3px
    );
    background-size: 9px 9px;
  }

  .event-title,
  .event-icons,
  .event-time,
  .event-location {
    display: none;
  }

  @container event-block (min-height: 14px) {
    .event-title { display: block; }
  }

  @container event-block (min-height: 17px) {
    .event-icons { display: flex; }
  }

  @container event-block (min-height: 32px) {
    .event-time { display: block; }
  }

  @container event-block (min-height: 48px) {
    .event-location { display: block; }
  }

  /* Disable layout transition for events with an explicit contour to avoid jank */
  .event-block-wrapper.event-editing {
    transition: none;
  }

  .mobile-event-block {
    touch-action: none;
  }

  .event-editing {
    outline: 2px solid color-mix(in oklab, var(--event-bg) 65%, var(--outline-mix));
    outline-offset: 0;
  }
</style>
