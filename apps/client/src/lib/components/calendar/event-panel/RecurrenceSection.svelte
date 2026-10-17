<script lang="ts">
  import type { RecurrenceConfig, RecurrenceFrequency, Weekday } from "$lib/calendar/types";
  import { formatRecurrenceLabel } from "$lib/calendar/rrule";
  import { commitIntegerDraft, moveRovingIndex, panelInputKeydown } from "$lib/calendar/event-panel-utils";
  import MiniDatePicker from "$lib/components/ui/MiniDatePicker.svelte";
  import { tick } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Repeat from "@lucide/svelte/icons/repeat";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  const WEEKDAY_LABEL_DATES: Record<Weekday, Date> = {
    MO: new Date(2024, 0, 1),
    TU: new Date(2024, 0, 2),
    WE: new Date(2024, 0, 3),
    TH: new Date(2024, 0, 4),
    FR: new Date(2024, 0, 5),
    SA: new Date(2024, 0, 6),
    SU: new Date(2024, 0, 7),
  };

  let {
    recurrence = $bindable<RecurrenceConfig | undefined>(undefined),
    startDate,
    rdate,
    expanded,
    onToggle,
    onExpand,
    onChange,
  }: {
    recurrence: RecurrenceConfig | undefined;
    startDate: string;
    rdate?: string[];
    expanded: boolean;
    onToggle: () => void;
    onExpand: () => void;
    onChange: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);

  const frequencyOptions = $derived.by((): { value: RecurrenceFrequency; label: string }[] => [
    { value: "daily", label: t("calendar.recurrence.unitDays") },
    { value: "weekly", label: t("calendar.recurrence.unitWeeks") },
    { value: "monthly", label: t("calendar.recurrence.unitMonths") },
    { value: "yearly", label: t("calendar.recurrence.unitYears") },
  ]);

  const weekdayOptions = $derived.by((): { value: Weekday; label: string }[] => [
    { value: "MO", label: weekdayLabel("MO") },
    { value: "TU", label: weekdayLabel("TU") },
    { value: "WE", label: weekdayLabel("WE") },
    { value: "TH", label: weekdayLabel("TH") },
    { value: "FR", label: weekdayLabel("FR") },
    { value: "SA", label: weekdayLabel("SA") },
    { value: "SU", label: weekdayLabel("SU") },
  ]);

  const WEEKDAY_MAP: Weekday[] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];

  const recFrequency = $derived.by((): RecurrenceFrequency => recurrence?.frequency ?? "daily");
  const recInterval = $derived.by(() => recurrence?.interval ?? 1);
  const recWeekdays = $derived.by(() => new Set<Weekday>(recurrence?.weekdays ?? []));
  const recEndType = $derived.by((): "never" | "until" | "count" => recurrence?.end.type ?? "never");
  const recEndDate = $derived.by(() => {
    if (recurrence && recurrence.end.type === "until") return recurrence.end.date;
    return "";
  });
  const recEndCount = $derived.by(() => {
    if (recurrence && recurrence.end.type === "count") return recurrence.end.count;
    return 13;
  });
  const label = $derived(
    recurrence ? formatRecurrenceLabel(recurrence, { t, locale }) : "",
  );
  let sectionEl: HTMLDivElement | undefined = $state();
  let intervalDraft = $state("1");
  let endCountDraft = $state("13");
  let frequencyFocusIndex = $state(0);
  let weekdayFocusIndex = $state(0);
  let monthlyModeFocusIndex = $state(0);
  let endTypeFocusIndex = $state(0);

  $effect(() => {
    intervalDraft = String(recInterval);
  });

  $effect(() => {
    endCountDraft = String(recEndCount);
  });

  $effect(() => {
    const frequencyIndex = frequencyOptions.findIndex((option) => option.value === recFrequency);
    if (frequencyIndex >= 0) frequencyFocusIndex = frequencyIndex;
  });

  $effect(() => {
    endTypeFocusIndex = recEndType === "never" ? 0 : recEndType === "until" ? 1 : 2;
  });

  function defaultUntilDate(): string {
    const [y, m, d] = startDate.split("-").map(Number);
    const dt = new Date(y, m - 1 + 3, d - 1);
    return `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
  }

  function getEventWeekday(): Weekday {
    const [y, m, d] = startDate.split("-").map(Number);
    return WEEKDAY_MAP[new Date(y, m - 1, d).getDay()];
  }

  function getEventDayOfMonth(): number {
    return parseInt(startDate.split("-")[2], 10);
  }

  function weekdayLabel(weekday: Weekday): string {
    return new Intl.DateTimeFormat(locale, { weekday: "short" }).format(
      WEEKDAY_LABEL_DATES[weekday],
    );
  }

  function getEventOrdinalWeekday(): { ordinal: number; day: Weekday; label: string } {
    const [y, m, d] = startDate.split("-").map(Number);
    const date = new Date(y, m - 1, d);
    const weekday = WEEKDAY_MAP[date.getDay()];
    const ordinal = Math.ceil(d / 7);
    const dayLabel = weekdayLabel(weekday);
    const ordSuffix = t("calendar.recurrence.ordinal", ordinal);
    return {
      ordinal,
      day: weekday,
      label: t("calendar.recurrence.ordinalWeekday", ordSuffix, dayLabel),
    };
  }

  type MonthlyMode = "day" | "ordinal";
  const monthlyMode = $derived.by((): MonthlyMode => {
    if (recurrence?.ordinalWeekdays && recurrence.ordinalWeekdays.length > 0) return "ordinal";
    return "day";
  });

  function setMonthlyMode(mode: MonthlyMode) {
    if (!recurrence || recurrence.frequency !== "monthly") return;
    if (mode === "day") {
      recurrence = { ...recurrence, ordinalWeekdays: undefined, byMonthDay: [getEventDayOfMonth()] };
    } else {
      const ow = getEventOrdinalWeekday();
      recurrence = { ...recurrence, byMonthDay: undefined, ordinalWeekdays: [{ day: ow.day, ordinal: ow.ordinal }] };
    }
    onChange();
  }

  function updateFrequency(freq: RecurrenceFrequency) {
    if (!recurrence) return;
    let weekdays = freq === "weekly" ? recurrence.weekdays : undefined;
    if (freq === "weekly" && (!weekdays || weekdays.length === 0)) weekdays = [getEventWeekday()];
    recurrence = { ...recurrence, frequency: freq, weekdays, ordinalWeekdays: undefined, byMonthDay: undefined, byMonth: undefined };
    onChange();
  }

  function updateInterval(val: number) {
    if (!recurrence) return;
    recurrence = { ...recurrence, interval: Math.max(1, val) };
    onChange();
  }

  function toggleWeekday(day: Weekday) {
    if (!recurrence) return;
    const current = new Set(recurrence.weekdays ?? []);
    if (current.has(day)) current.delete(day);
    else current.add(day);
    if (current.size === 0) return;
    recurrence = { ...recurrence, weekdays: weekdayOptions.map((w) => w.value).filter((d) => current.has(d)) };
    onChange();
  }

  function updateEndType(type: "never" | "until" | "count") {
    if (!recurrence) return;
    if (type === "never") recurrence = { ...recurrence, end: { type: "never" } };
    else if (type === "until") recurrence = { ...recurrence, end: { type: "until", date: defaultUntilDate() } };
    else recurrence = { ...recurrence, end: { type: "count", count: 13 } };
    onChange();
  }

  function updateEndDate(date: string) {
    if (!recurrence) return;
    recurrence = { ...recurrence, end: { type: "until", date } };
    onChange();
  }

  function updateEndCount(count: number) {
    if (!recurrence) return;
    recurrence = { ...recurrence, end: { type: "count", count: Math.max(1, count) } };
    onChange();
  }

  async function focusRovingButton(group: string, index: number) {
    await tick();
    sectionEl
      ?.querySelector<HTMLButtonElement>(`[data-recurrence-roving="${group}"][data-roving-index="${index}"]`)
      ?.focus();
  }

  function setRovingIndex(group: "frequency" | "weekday" | "monthly" | "end", index: number) {
    if (group === "frequency") frequencyFocusIndex = index;
    else if (group === "weekday") weekdayFocusIndex = index;
    else if (group === "monthly") monthlyModeFocusIndex = index;
    else endTypeFocusIndex = index;
  }

  function handleRovingKeydown(
    e: KeyboardEvent,
    group: "frequency" | "weekday" | "monthly" | "end",
    index: number,
    itemCount: number,
    orientation: "horizontal" | "vertical" | "grid",
    columns?: number,
  ) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const nextIndex = moveRovingIndex({
      currentIndex: index,
      itemCount,
      key: e.key,
      orientation,
      columns,
    });
    if (nextIndex === index) return;
    e.preventDefault();
    e.stopPropagation();
    setRovingIndex(group, nextIndex);
    void focusRovingButton(group, nextIndex);
  }

  function commitIntervalDraft() {
    const result = commitIntegerDraft(intervalDraft, recInterval, 1, 99);
    intervalDraft = String(result.value);
    if (result.committed) updateInterval(result.value);
  }

  function commitEndCountDraft() {
    const result = commitIntegerDraft(endCountDraft, recEndCount, 1, 999);
    endCountDraft = String(result.value);
    if (result.committed) updateEndCount(result.value);
  }

  function handleNumberDraftKeydown(e: KeyboardEvent, commit: () => void, restore: () => void) {
    if (!e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
      if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        commit();
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        restore();
        return;
      }
    }
    panelInputKeydown(e);
  }

  let pickerOpen = $state(false);
  let endDateInput: HTMLInputElement | undefined = $state();
  let dateText = $state("");

  function syncDateText() {
    const d = recEndDate || defaultUntilDate();
    const [y, m, day] = d.split("-").map(Number);
    const dt = new Date(y, m - 1, day);
    dateText = dt.toLocaleDateString(locale, {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  }

  function parseDateInput() {
    const parsed = new Date(dateText);
    if (!isNaN(parsed.getTime())) {
      const iso = `${parsed.getFullYear()}-${String(parsed.getMonth() + 1).padStart(2, "0")}-${String(parsed.getDate()).padStart(2, "0")}`;
      updateEndDate(iso);
    }
    syncDateText();
  }

  function handleDateInputKeydown(e: KeyboardEvent) {
    if (!e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
      if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        parseDateInput();
        pickerOpen = false;
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        pickerOpen = false;
        syncDateText();
        void focusDateInput();
        return;
      }
    }
    panelInputKeydown(e);
  }

  $effect(() => {
    const _dep = recEndDate;
    syncDateText();
  });

  function positionPicker(node: HTMLElement) {
    if (!endDateInput) return { destroy() {} };
    const margin = 8;
    const gap = 4;
    const pickerWidth = 240;

    function updatePosition() {
      if (!endDateInput) return;
      node.style.maxHeight = `${Math.max(96, window.innerHeight - margin * 2)}px`;
      node.style.overflowY = "auto";

      const inputRect = endDateInput.getBoundingClientRect();
      const pickerRect = node.getBoundingClientRect();
      const pickerHeight = pickerRect.height;

      let left = inputRect.left + inputRect.width / 2 - pickerWidth / 2;
      left = Math.max(margin, Math.min(window.innerWidth - pickerWidth - margin, left));

      const belowTop = inputRect.bottom + gap;
      const aboveTop = inputRect.top - pickerHeight - gap;
      const belowFits = belowTop + pickerHeight <= window.innerHeight - margin;
      const aboveFits = aboveTop >= margin;
      const top = belowFits || (!aboveFits && belowTop <= window.innerHeight - pickerHeight - margin)
        ? belowTop
        : Math.max(margin, aboveTop);

      node.style.left = `${left}px`;
      node.style.top = `${Math.min(top, window.innerHeight - pickerHeight - margin)}px`;
    }

    const frame = requestAnimationFrame(updatePosition);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);

    return {
      destroy() {
        cancelAnimationFrame(frame);
        window.removeEventListener("resize", updatePosition);
        window.removeEventListener("scroll", updatePosition, true);
      },
    };
  }

  async function focusDateInput() {
    await tick();
    endDateInput?.focus();
  }

  function cancelCalendarDay(source?: "keyboard" | "pointer") {
    pickerOpen = false;
    if (source === "keyboard") void focusDateInput();
  }

  function selectCalendarDay(dateStr: string, source?: "keyboard" | "pointer") {
    updateEndDate(dateStr);
    pickerOpen = false;
    if (source === "keyboard") void focusDateInput();
  }
</script>

<div bind:this={sectionEl} class="flex flex-col rounded-none overflow-hidden" style="background-color: var(--panel-contrast);">
  <div class="section-header flex items-stretch">
    <button onclick={onToggle}
      class="flex w-10 shrink-0 items-center justify-center
        {recurrence ? 'bg-black/3 dark:bg-black/30 text-foreground' : 'text-muted-foreground/50'}">
      <Repeat size={14} />
    </button>
    <button onclick={onExpand}
      class="flex flex-1 items-center gap-2.5 px-3 py-2 text-left">
      <span class="translate-y-[1.13px] text-[0.8rem] {recurrence ? 'text-foreground' : 'text-muted-foreground'}">{t("calendar.recurrence.repeat")}</span>
      <span class="ml-auto translate-y-[1.13px] truncate text-[0.733333rem] text-muted-foreground">{recurrence ? label : ""}</span>
    </button>
  </div>
  {#if expanded && recurrence}
    <div transition:slide={{ duration: 180, easing: cubicOut }} data-section="repeat" class="flex flex-col gap-2.5 p-2.5" style="background-color: var(--panel-bg);">
      <!-- Every N [frequency] -->
      <div class="flex flex-wrap items-center gap-2">
        <span class="shrink-0 text-[0.8rem] text-muted-foreground">{t("calendar.recurrence.every")}</span>
        <input type="number" value={intervalDraft}
          oninput={(e) => { intervalDraft = e.currentTarget.value; }}
          onblur={commitIntervalDraft}
          min={1} max={99}
          class="num-input w-11 shrink-0 rounded-none bg-black/5 px-1 py-0.5 text-center text-[0.8rem] text-event-panel-input-text outline-none dark:bg-black/15"
          onkeydown={(e) => handleNumberDraftKeydown(e, commitIntervalDraft, () => { intervalDraft = String(recInterval); })} />
        <div class="flex min-w-0 flex-1 flex-wrap gap-1.5">
          {#each frequencyOptions as opt, index}
            <button onclick={() => updateFrequency(opt.value)}
              onfocus={() => { frequencyFocusIndex = index; }}
              onkeydown={(e) => handleRovingKeydown(e, "frequency", index, frequencyOptions.length, "horizontal")}
              data-recurrence-roving="frequency"
              data-roving-index={index}
              tabindex={frequencyFocusIndex === index ? 0 : -1}
              class="min-w-0 flex-1 rounded-none px-2.5 py-1 text-[0.8rem]
                {recFrequency === opt.value
                  ? 'bg-black/5 dark:bg-black/15 text-foreground'
                  : 'text-muted-foreground'}"
            >{opt.label}</button>
          {/each}
        </div>
      </div>

      <!-- Weekday picker (weekly) -->
      {#if recFrequency === "weekly"}
        <div class="flex flex-col gap-1.5">
          <span class="text-[0.733333rem] uppercase tracking-wider text-muted-foreground">{t("calendar.recurrence.repeatOn")}</span>
          <div class="grid grid-cols-7 gap-1">
            {#each weekdayOptions as wd, index}
              <button onclick={() => toggleWeekday(wd.value)}
                onfocus={() => { weekdayFocusIndex = index; }}
                onkeydown={(e) => handleRovingKeydown(e, "weekday", index, weekdayOptions.length, "grid", 7)}
                data-recurrence-roving="weekday"
                data-roving-index={index}
                tabindex={weekdayFocusIndex === index ? 0 : -1}
                class="flex h-7 items-center justify-center rounded-none text-[0.733333rem]
                  {recWeekdays.has(wd.value)
                    ? 'bg-black/5 dark:bg-black/15 text-foreground'
                    : 'bg-black/2 dark:bg-black/6 text-muted-foreground'}"
              >{wd.label}</button>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Monthly sub-options -->
      {#if recFrequency === "monthly"}
        <div class="flex flex-col gap-1.5">
          <span class="text-[0.733333rem] uppercase tracking-wider text-muted-foreground">{t("calendar.recurrence.repeatOn")}</span>
          <div class="flex flex-wrap gap-1.5">
            <button onclick={() => setMonthlyMode("day")}
              onfocus={() => { monthlyModeFocusIndex = 0; }}
              onkeydown={(e) => handleRovingKeydown(e, "monthly", 0, 2, "horizontal")}
              data-recurrence-roving="monthly"
              data-roving-index="0"
              tabindex={monthlyModeFocusIndex === 0 ? 0 : -1}
              class="flex min-w-0 items-center gap-2.5 rounded-none px-2.5 py-1.5 text-[0.8rem]
                {monthlyMode === 'day'
                  ? 'bg-black/5 dark:bg-black/15 text-foreground'
                  : 'text-foreground'}">
              <div class="size-3 shrink-0 rounded-full
                {monthlyMode === 'day' ? 'bg-form-indicator' : 'border border-muted-foreground/40'}">
              </div>
              <span class="truncate">{t("calendar.recurrence.dayOfMonth", getEventDayOfMonth())}</span>
            </button>
            <button onclick={() => setMonthlyMode("ordinal")}
              onfocus={() => { monthlyModeFocusIndex = 1; }}
              onkeydown={(e) => handleRovingKeydown(e, "monthly", 1, 2, "horizontal")}
              data-recurrence-roving="monthly"
              data-roving-index="1"
              tabindex={monthlyModeFocusIndex === 1 ? 0 : -1}
              class="flex min-w-0 items-center gap-2.5 rounded-none px-2.5 py-1.5 text-[0.8rem]
                {monthlyMode === 'ordinal'
                  ? 'bg-black/5 dark:bg-black/15 text-foreground'
                  : 'text-foreground'}">
              <div class="size-3 shrink-0 rounded-full
                {monthlyMode === 'ordinal' ? 'bg-form-indicator' : 'border border-muted-foreground/40'}">
              </div>
              <span class="truncate">{getEventOrdinalWeekday().label}</span>
            </button>
          </div>
        </div>
      {/if}

      <!-- Ends -->
      <div class="flex flex-col gap-1.5">
        <span class="text-[0.733333rem] uppercase tracking-wider text-muted-foreground">{t("calendar.recurrence.endsHeading")}</span>

        <button onclick={() => updateEndType("never")}
          onfocus={() => { endTypeFocusIndex = 0; }}
          onkeydown={(e) => handleRovingKeydown(e, "end", 0, 3, "vertical")}
          data-recurrence-roving="end"
          data-roving-index="0"
          tabindex={endTypeFocusIndex === 0 ? 0 : -1}
          class="flex items-center gap-2.5 rounded-none px-2.5 py-1.5 text-[0.8rem] text-foreground">
          <div class="size-3 shrink-0 rounded-full
            {recEndType === 'never' ? 'bg-form-indicator' : 'border border-muted-foreground/40'}">
          </div>
          <span>{t("calendar.recurrence.never")}</span>
        </button>

        <div class="flex items-center gap-2.5 rounded-none px-2.5 py-1.5 text-[0.8rem]">
          <button onclick={() => updateEndType("until")}
            onfocus={() => { endTypeFocusIndex = 1; }}
            onkeydown={(e) => handleRovingKeydown(e, "end", 1, 3, "vertical")}
            data-recurrence-roving="end"
            data-roving-index="1"
            tabindex={endTypeFocusIndex === 1 ? 0 : -1}
            class="flex w-14 items-center gap-2.5 text-foreground">
            <div class="size-3 shrink-0 rounded-full
              {recEndType === 'until' ? 'bg-form-indicator' : 'border border-muted-foreground/40'}">
            </div>
            <span>{t("calendar.recurrence.on")}</span>
          </button>
          <div class="relative">
            <input bind:this={endDateInput}
              type="text"
              bind:value={dateText}
              onclick={() => { if (recEndType !== "until") updateEndType("until"); pickerOpen = !pickerOpen; }}
              onblur={parseDateInput}
              onkeydown={handleDateInputKeydown}
              class="w-31 rounded bg-black/5 px-2.5 py-0.5 text-[0.8rem] outline-none dark:bg-black/15
                {recEndType === 'until' ? 'text-event-panel-input-text' : 'text-muted-foreground'}
                {pickerOpen ? 'ring-1 ring-primary/60' : 'hover:bg-black/5 dark:hover:bg-black/15'}" />

            {#if pickerOpen}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="fixed inset-0 z-60" onclick={() => { pickerOpen = false; }}></div>
              <div class="fixed z-61 w-60 rounded-lg bg-popover p-2 shadow-lg ring-1 ring-border/60"
                use:positionPicker>
                <MiniDatePicker
                  selectedDate={recEndDate || defaultUntilDate()}
                  small
                  highlightToday={false}
                  activeHighlight="primary"
                  onSelect={selectCalendarDay}
                  onCancel={cancelCalendarDay}
                />
              </div>
            {/if}
          </div>
        </div>

        <div class="flex items-center gap-2.5 rounded-none px-2.5 py-1.5 text-[0.8rem]">
          <button onclick={() => updateEndType("count")}
            onfocus={() => { endTypeFocusIndex = 2; }}
            onkeydown={(e) => handleRovingKeydown(e, "end", 2, 3, "vertical")}
            data-recurrence-roving="end"
            data-roving-index="2"
            tabindex={endTypeFocusIndex === 2 ? 0 : -1}
            class="flex w-14 items-center gap-2.5 text-foreground">
            <div class="size-3 shrink-0 rounded-full
              {recEndType === 'count' ? 'bg-form-indicator' : 'border border-muted-foreground/40'}">
            </div>
            <span>{t("calendar.recurrence.after")}</span>
          </button>
          <div class="flex items-center gap-2">
            <input type="number" value={endCountDraft}
              onfocus={() => { if (recEndType !== "count") updateEndType("count"); }}
              oninput={(e) => { endCountDraft = e.currentTarget.value; }}
              onblur={commitEndCountDraft}
              min={1} max={999}
              class="num-input w-11 rounded bg-black/5 px-1 py-0.5 text-center text-[0.8rem] outline-none dark:bg-black/15
                {recEndType === 'count' ? 'text-event-panel-input-text' : 'text-muted-foreground'}"
              onclick={(e) => e.stopPropagation()}
              onkeydown={(e) => handleNumberDraftKeydown(e, commitEndCountDraft, () => { endCountDraft = String(recEndCount); })} />
            <span class="{recEndType === 'count' ? 'text-event-panel-input-text' : 'text-muted-foreground'}">
              {t("calendar.recurrence.countUnit", recEndCount)}
            </span>
          </div>
        </div>
      </div>

      <!-- RDATE indicator -->
      {#if rdate && rdate.length > 0}
        <span class="text-[0.733333rem] italic text-muted-foreground">
          {t("calendar.recurrence.additionalDates", rdate.length)}
        </span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .num-input {
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .num-input::-webkit-inner-spin-button,
  .num-input::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
</style>
