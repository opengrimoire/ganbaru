<script lang="ts">
  import { tick } from "svelte";
  import { moveRovingIndex } from "$lib/calendar/event-panel-utils";
  import { createSmoothScroll, formatTimeLabel } from "$lib/calendar/utils";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  export interface TimePickerInputNavigation {
    key: "ArrowUp" | "ArrowDown";
    sequence: number;
  }

  let {
    currentTime,
    isEnd = false,
    startMinutes = 0,
    focusOnOpen = false,
    activeTime = undefined,
    scrollTime = undefined,
    emphasizedTime = undefined,
    inputNavigation = null,
    onSelect,
    onCancel,
    onTypeDigit,
  }: {
    currentTime: string;
    isEnd?: boolean;
    startMinutes?: number;
    focusOnOpen?: boolean;
    activeTime?: string | undefined;
    scrollTime?: string | undefined;
    emphasizedTime?: string | null | undefined;
    inputNavigation?: TimePickerInputNavigation | null;
    onSelect: (time: string, source?: "keyboard" | "pointer") => void;
    onCancel?: (source?: "keyboard" | "pointer") => void;
    onTypeDigit?: (digit: string) => void;
  } = $props();

  const preferences = getPreferences();
  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);

  const TIME_SLOTS = Array.from({ length: 48 }, (_, i) => {
    const h = Math.floor(i / 2);
    const m = (i % 2) * 30;
    return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
  });

  let scrollEl: HTMLDivElement | undefined = $state();
  let activeIndex = $state(0);
  let previousActiveTime = $state("");
  let centeredScrollEl: HTMLDivElement | undefined;
  let centeredTime = "";
  let lastInputNavigationSequence = 0;
  const onWheel = createSmoothScroll(() => scrollEl, 2, 8);

  function slotIndexFor(time: string): number {
    const [h, m] = (time || "0:0").split(":").map(Number);
    const minutes = Number.isFinite(h) && Number.isFinite(m) ? h * 60 + m : 0;
    return Math.min(Math.max(Math.round(minutes / 30), 0), TIME_SLOTS.length - 1);
  }

  const activeTimeValue = $derived(activeTime ?? currentTime);
  const scrollTimeValue = $derived(scrollTime ?? activeTimeValue);
  const emphasizedSlot = $derived.by(() => {
    if (emphasizedTime === null) return null;
    return TIME_SLOTS[slotIndexFor(emphasizedTime ?? currentTime)];
  });

  $effect(() => {
    if (activeTimeValue !== previousActiveTime) {
      previousActiveTime = activeTimeValue;
      activeIndex = slotIndexFor(activeTimeValue);
    }
  });

  function activeSlotButton(): HTMLButtonElement | null {
    return scrollEl?.querySelector<HTMLButtonElement>(`[data-slot-index="${activeIndex}"]`) ?? null;
  }

  function keepSlotVisible(button: HTMLElement) {
    if (!scrollEl) return;

    const padding = 4;
    const scrollRect = scrollEl.getBoundingClientRect();
    const buttonRect = button.getBoundingClientRect();
    if (buttonRect.top < scrollRect.top + padding) {
      scrollEl.scrollTop -= scrollRect.top + padding - buttonRect.top;
    } else if (buttonRect.bottom > scrollRect.bottom - padding) {
      scrollEl.scrollTop += buttonRect.bottom - (scrollRect.bottom - padding);
    }
  }

  async function focusActiveSlot(ensureVisible = true) {
    await tick();
    const button = activeSlotButton();
    button?.focus({ preventScroll: true });
    if (button && ensureVisible) keepSlotVisible(button);
  }

  function focusSlot(index: number) {
    activeIndex = index;
    void focusActiveSlot();
  }

  $effect(() => {
    const navigation = inputNavigation;
    if (!navigation || navigation.sequence === lastInputNavigationSequence) return;
    lastInputNavigationSequence = navigation.sequence;

    const nextIndex = moveRovingIndex({
      currentIndex: activeIndex,
      itemCount: TIME_SLOTS.length,
      key: navigation.key,
      orientation: "vertical",
    });
    focusSlot(nextIndex);
  });

  $effect(() => {
    if (!scrollEl) return;
    const el = scrollEl;
    const time = scrollTimeValue;
    if (centeredScrollEl === el && centeredTime === time) return;
    centeredScrollEl = el;
    centeredTime = time;
    requestAnimationFrame(() => {
      let target = el.querySelector(`[data-time="${time}"]`);
      if (!target && time) {
        target = el.querySelector(`[data-time="${TIME_SLOTS[slotIndexFor(time)]}"]`);
      }
      target?.scrollIntoView({ block: "center" });
      if (focusOnOpen) void focusActiveSlot(false);
    });
  });

  function handleSlotKeydown(e: KeyboardEvent, index: number, slot: string) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;

    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel?.("keyboard");
      return;
    }

    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      e.stopPropagation();
      onSelect(slot, "keyboard");
      return;
    }

    if (/^\d$/.test(e.key)) {
      e.preventDefault();
      e.stopPropagation();
      onTypeDigit?.(e.key);
      return;
    }

    const nextIndex = moveRovingIndex({
      currentIndex: index,
      itemCount: TIME_SLOTS.length,
      key: e.key,
      orientation: "vertical",
    });
    if (nextIndex === index) return;
    e.preventDefault();
    e.stopPropagation();
    focusSlot(nextIndex);
  }

  function getDurationLabel(slot: string): string {
    if (!isEnd) return "";
    const [h, m] = slot.split(":").map(Number);
    let durationMinutes = h * 60 + m - startMinutes;
    if (durationMinutes <= 0) durationMinutes += 1440;
    if (durationMinutes >= 1440) return "";
    const hours = durationMinutes / 60;
    return t(
      "calendar.timePicker.durationHours",
      formatNumber(locale, hours, { maximumFractionDigits: 1 }),
      hours === 1,
    );
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div bind:this={scrollEl} use:scrollEdgeFadeAction onwheel={onWheel} class="time-picker-scroll max-h-50 overflow-y-auto">
  {#each TIME_SLOTS as slot, index}
    {@const selected = currentTime === slot}
    {@const emphasized = slot === emphasizedSlot}
    {@const active = activeIndex === index}
    {@const durationLabel = getDurationLabel(slot)}
    <button onclick={() => onSelect(slot, "pointer")}
      data-time={slot}
      data-slot-index={index}
      tabindex={activeIndex === index ? 0 : -1}
      onfocus={() => { activeIndex = index; }}
      onkeydown={(e) => handleSlotKeydown(e, index, slot)}
      class="flex w-full items-center px-2.5 py-1 text-left text-[0.866667rem] outline-none hover:bg-black/5 focus-visible:bg-black/5 dark:hover:bg-black/15 dark:focus-visible:bg-black/15
        {selected ? 'bg-accent' : active ? 'bg-black/5 dark:bg-black/15' : ''}"
      style="font-weight: {active || emphasized ? 600 : selected ? 500 : 400}; color: {active || emphasized || selected ? 'var(--foreground)' : 'var(--muted-foreground)'};">
      <span>{formatTimeLabel(slot, preferences.calendarTimeFormat)}</span>
      {#if durationLabel}
        <span class="ml-2 text-[0.733333rem]" style="color: var(--muted-foreground); font-weight: 400;">({durationLabel})</span>
      {/if}
    </button>
  {/each}
</div>
