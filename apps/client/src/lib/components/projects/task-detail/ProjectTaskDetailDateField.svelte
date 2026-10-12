<script lang="ts">
  import { tick } from "svelte";
  import { formatTimeLabel } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import { pickSelectPopoverGeometry } from "$lib/utils/select-popover-position";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Clock from "@lucide/svelte/icons/clock";
  import Eraser from "@lucide/svelte/icons/eraser";
  import X from "@lucide/svelte/icons/x";
  import MiniDatePicker from "$lib/components/ui/MiniDatePicker.svelte";
  import TimePicker from "$lib/components/ui/TimePicker.svelte";
  import { cn } from "$lib/utils";

  type MiniDatePickerHighlightMode = "day" | "week" | "workweek" | "none";

  let {
    label = null,
    inline = false,
    value,
    noDateLabel,
    clearLabel,
    pickerOpen,
    selectedDate,
    rangeStartDate = undefined,
    rangeEndDate = undefined,
    highlightToday = undefined,
    highlightMode = undefined,
    time = "",
    onTimeChange = undefined,
    onToggle,
    onClear,
    onSelect,
    onCancel,
  }: {
    label?: string | null;
    inline?: boolean;
    value: string;
    noDateLabel: string;
    clearLabel: string;
    pickerOpen: boolean;
    selectedDate: string;
    rangeStartDate?: string | undefined;
    rangeEndDate?: string | undefined;
    highlightToday?: boolean | undefined;
    highlightMode?: MiniDatePickerHighlightMode | undefined;
    /** The `HH:MM` time of the date, or empty for a whole day. */
    time?: string;
    /** Offers a time below the calendar once a date is set; null removes the time. */
    onTimeChange?: ((time: string | null) => void) | undefined;
    onToggle: () => void;
    onClear: () => void;
    onSelect: (date: string) => void;
    onCancel: () => void;
  } = $props();

  /** The slot a time picker centers on before the date has a time. */
  const DEFAULT_TIME_ANCHOR = "12:00";

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const pickerId = $props.id();
  let trigger: HTMLButtonElement | undefined = $state();
  let popup: HTMLDivElement | undefined = $state();
  let timeTrigger: HTMLButtonElement | undefined = $state();
  let timePanel: HTMLDivElement | undefined = $state();
  let timeOpen = $state(false);
  let popupStyle = $state("visibility: hidden;");
  const timeLabel = $derived(time ? formatTimeLabel(time, preferences.calendarTimeFormat) : "");
  const displayValue = $derived.by(() => {
    if (!value) return noDateLabel;
    const date = new Date(`${value}T00:00:00Z`);
    const day = Number.isNaN(date.getTime()) ? value : formatDateTime(localization.locale, date, {
      day: "numeric", month: "short", year: "numeric", timeZone: "UTC",
    });
    return timeLabel ? `${day} ${timeLabel}` : day;
  });

  $effect(() => {
    if (!pickerOpen || !value) timeOpen = false;
  });

  function closeTime(): void {
    timeOpen = false;
    timeTrigger?.focus({ preventScroll: true });
  }

  function pickTime(next: string): void {
    onTimeChange?.(next);
    closeTime();
  }

  /** Place the calendar beside its field, bounded by the visible viewport. */
  function positionPicker(): void {
    if (!trigger || !popup) return;
    const viewport = window.visualViewport;
    const left = viewport?.offsetLeft ?? 0;
    const top = viewport?.offsetTop ?? 0;
    const width = viewport?.width ?? window.innerWidth;
    const height = viewport?.height ?? window.innerHeight;
    const rect = popup.getBoundingClientRect();
    const geometry = pickSelectPopoverGeometry({
      triggerRect: trigger.getBoundingClientRect(),
      boundaryRect: { left, top, right: left + width, bottom: top + height, width, height },
      contentHeight: popup.scrollHeight,
      contentWidth: rect.width,
      horizontalAlign: "end",
    });
    popupStyle = `left: ${geometry.left}px; top: ${geometry.top}px; width: ${geometry.width ?? rect.width}px; max-height: ${geometry.maxHeight}px; max-width: ${geometry.maxWidth}px;`;
  }

  /** Close the calendar without scrolling the editor back to the field. */
  function cancelPicker(): void {
    onCancel();
    trigger?.focus({ preventScroll: true });
  }

  $effect(() => {
    if (!pickerOpen || !popup) return;
    positionPicker();
    void tick().then(() => {
      if (!pickerOpen || !popup) return;
      positionPicker();
      popup.querySelector<HTMLButtonElement>(`[data-date="${selectedDate}"]`)?.focus({ preventScroll: true });
    });
    const observer = new ResizeObserver(positionPicker);
    observer.observe(popup);
    const outside = (event: PointerEvent): void => {
      if (!(event.target instanceof Node)) return;
      if (!trigger?.contains(event.target) && !popup?.contains(event.target) && !timePanel?.contains(event.target)) onCancel();
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("resize", positionPicker);
    window.addEventListener("scroll", positionPicker, true);
    window.visualViewport?.addEventListener("resize", positionPicker);
    return () => {
      observer.disconnect();
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("resize", positionPicker);
      window.removeEventListener("scroll", positionPicker, true);
      window.visualViewport?.removeEventListener("resize", positionPicker);
    };
  });
</script>

<div class={cn("min-w-0 text-[0.8rem]", inline ? "task-property-row" : "grid gap-1")}>
  {#if label}<span class="text-muted-foreground">{label}</span>{/if}
  <div class="flex min-w-0 items-center gap-1">
    <button bind:this={trigger} type="button"
      class={cn("flex min-h-8 min-w-0 flex-1 items-center gap-2 rounded-md px-1.5 text-left hover:bg-accent/60", !value && "text-muted-foreground")}
      aria-label={label ? `${label}: ${displayValue}` : displayValue}
      aria-haspopup="dialog" aria-expanded={pickerOpen} aria-controls={pickerOpen ? pickerId : undefined}
      onclick={onToggle}>
      <CalendarDays size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
      <span class="truncate">{displayValue}</span>
    </button>
    {#if value}
      <button type="button" class="flex size-7 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-label={clearLabel} onclick={onClear}>
        <X size={12} />
      </button>
    {/if}
  </div>
</div>
{#if pickerOpen}
  <div bind:this={popup} use:portal={trigger?.closest<HTMLElement>("[data-floating-root]") ?? "body"}
    id={pickerId} role="dialog" aria-label={label ?? clearLabel} tabindex="-1" data-app-floating-surface
    class="task-date-popover surface-floating fixed z-90 w-floating overflow-auto p-3"
    style={popupStyle}
    onkeydowncapture={(event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        cancelPicker();
      }
    }}>
    <MiniDatePicker {selectedDate} {rangeStartDate} {rangeEndDate} small {highlightToday} {highlightMode}
      activeHighlight="primary"
      onSelect={(date) => { onSelect(date); trigger?.focus({ preventScroll: true }); }}
      onCancel={cancelPicker} />
    {#if value && onTimeChange}
      <div class="mt-2 flex items-center gap-1 border-t border-border/70 pt-2">
        <button bind:this={timeTrigger} type="button" class="menu-item min-w-0 flex-1 text-muted-foreground hover:text-foreground"
          aria-haspopup="dialog" aria-expanded={timeOpen} onclick={() => { timeOpen = !timeOpen; }}>
          <Clock size={13} strokeWidth={1.75} class="shrink-0" />
          <span class="truncate">{timeLabel || t("projects.columns.selectHour")}</span>
        </button>
        {#if time}
          <button type="button" class="flex size-(--panel-row-height) shrink-0 items-center justify-center rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground"
            aria-label={t("projects.columns.clearHour")} data-app-tooltip-disabled="true"
            onclick={() => { onTimeChange(null); timeOpen = false; }}>
            <Eraser size={13} strokeWidth={1.75} />
          </button>
        {/if}
      </div>
    {/if}
  </div>
{/if}
{#if timeOpen && pickerOpen && value}
  <div bind:this={timePanel} use:portal={trigger?.closest<HTMLElement>("[data-floating-root]") ?? "body"}
    role="dialog" aria-label={`${label ?? clearLabel} ${t("projects.columns.selectHour")}`} data-app-floating-surface
    use:anchoredPopover={{ anchor: timeTrigger, onOutside: () => { timeOpen = false; } }}
    class="surface-floating fixed z-90 w-40 overflow-hidden"
    onkeydowncapture={(event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      closeTime();
    }}>
    <TimePicker currentTime={time} activeTime={time || DEFAULT_TIME_ANCHOR} scrollTime={time || DEFAULT_TIME_ANCHOR}
      emphasizedTime={time ? undefined : null} focusOnOpen onSelect={pickTime} onCancel={closeTime} />
  </div>
{/if}
