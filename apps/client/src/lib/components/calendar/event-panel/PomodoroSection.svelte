<script lang="ts">
  import { tick } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { commitIntegerDraft, moveRovingIndex, panelInputKeydown } from "$lib/calendar/event-panel-utils";
  import Timer from "@lucide/svelte/icons/timer";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import {
    COUNT_PRESET_RHYTHMS,
    MAX_FOCUS_MINUTES,
    MAX_LONG_BREAK_MINUTES,
    MAX_RHYTHM_POSITIONS,
    MAX_SHORT_BREAK_MINUTES,
    MIN_FOCUS_MINUTES,
    MIN_LONG_BREAK_MINUTES,
    MIN_RHYTHM_POSITIONS,
    MIN_SHORT_BREAK_MINUTES,
    summarizeSequencePomodoroRhythm,
    type SequencePomodoroRhythmStep,
  } from "$lib/pomodoro/rhythm";

  type PomodoroPreset = "adaptive" | "creative" | "balanced" | "deep" | "extended" | "custom";
  type BuiltInPomodoroPreset = Exclude<PomodoroPreset, "custom">;
  type CustomRhythmMode = "simple" | "sequence";
  type SummarySlotClass = "duration-summary-slot" | "cycle-summary-slot";

  const CUSTOM_DURATION_SLOT_MAX = 99;
  const CUSTOM_CYCLE_SLOT_MAX = 9;
  const CUSTOM_COUNT_DEFAULT = {
    focusDurationMinutes: 40,
    shortBreakMinutes: 10,
    longBreakMinutes: 10,
    longBreakAfterFocusCount: 4,
  } as const;
  const PRESET_RHYTHMS = COUNT_PRESET_RHYTHMS;
  const PRESET_ORDER: BuiltInPomodoroPreset[] = [
    "adaptive",
    "creative",
    "balanced",
    "deep",
    "extended",
  ];
  const PRESET_ENTRIES = PRESET_ORDER.map((key) => [key, PRESET_RHYTHMS[key]]) as Array<[
    BuiltInPomodoroPreset,
    typeof PRESET_RHYTHMS[BuiltInPomodoroPreset],
  ]>;
  const PRESET_OPTION_COUNT = PRESET_ENTRIES.length + 1;

  let {
    enabled,
    preset = $bindable<PomodoroPreset>("adaptive"),
    focusDurationMinutes = $bindable(40),
    shortBreakMinutes = $bindable(5),
    longBreakMinutes = $bindable(10),
    longBreakAfterFocusCount = $bindable(4),
    customRhythmMode = $bindable<CustomRhythmMode>("simple"),
    sequenceSteps = $bindable<SequencePomodoroRhythmStep[]>([]),
    idleTimeoutEnabled = $bindable(true),
    expanded,
    readonlyInteractive = false,
    idleDetectionAvailable = true,
    onToggle,
    onExpand,
    onChange,
  }: {
    enabled: boolean;
    preset: PomodoroPreset;
    focusDurationMinutes: number;
    shortBreakMinutes: number;
    longBreakMinutes: number;
    longBreakAfterFocusCount: number;
    customRhythmMode: CustomRhythmMode;
    sequenceSteps: SequencePomodoroRhythmStep[];
    idleTimeoutEnabled: boolean;
    expanded: boolean;
    readonlyInteractive?: boolean;
    idleDetectionAvailable?: boolean;
    onToggle: () => void;
    onExpand: () => void;
    onChange: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);
  let sectionEl: HTMLDivElement | undefined = $state();
  let presetFocusIndex = $state(0);
  let focusDurationDraft = $state("40");
  let shortBreakDraft = $state("5");
  let longBreakDraft = $state("10");
  let longBreakAfterDraft = $state("4");

  $effect(() => {
    focusDurationDraft = formatDurationSlot(focusDurationMinutes);
  });

  $effect(() => {
    shortBreakDraft = formatDurationSlot(shortBreakMinutes);
  });

  $effect(() => {
    longBreakDraft = formatDurationSlot(longBreakMinutes);
  });

  $effect(() => {
    longBreakAfterDraft = formatCycleSlot(longBreakAfterFocusCount);
  });

  $effect(() => {
    const index = preset === "custom"
      ? PRESET_ENTRIES.length
      : PRESET_ENTRIES.findIndex(([key]) => key === preset);
    if (index >= 0) presetFocusIndex = index;
  });

  const localizedPresetEntries = $derived(
    PRESET_ENTRIES.map(([key, rhythm]) => ({
      key,
      label: presetLabel(key),
      summarySlots: presetSummarySlots(key, rhythm),
    })),
  );

  const customFields = $derived([
    {
      label: t("calendar.pomodoro.focus"),
      compactLabel: t("calendar.pomodoro.focusCompact"),
      value: focusDurationDraft,
      setDraft: (v: string) => { focusDurationDraft = sanitizeCustomSlotDraft(v, 2); },
      commit: commitFocusDurationDraft,
      restore: () => { focusDurationDraft = formatDurationSlot(focusDurationMinutes); },
      min: MIN_FOCUS_MINUTES,
      max: Math.min(MAX_FOCUS_MINUTES, CUSTOM_DURATION_SLOT_MAX),
      maxLength: 2,
      slotClass: "duration-summary-slot" as SummarySlotClass,
    },
    {
      label: t("calendar.pomodoro.shortBreak"),
      compactLabel: t("calendar.pomodoro.shortBreakCompact"),
      value: shortBreakDraft,
      setDraft: (v: string) => { shortBreakDraft = sanitizeCustomSlotDraft(v, 2); },
      commit: commitShortBreakDraft,
      restore: () => { shortBreakDraft = formatDurationSlot(shortBreakMinutes); },
      min: MIN_SHORT_BREAK_MINUTES,
      max: Math.min(MAX_SHORT_BREAK_MINUTES, CUSTOM_DURATION_SLOT_MAX),
      maxLength: 2,
      slotClass: "duration-summary-slot" as SummarySlotClass,
    },
    {
      label: t("calendar.pomodoro.longBreak"),
      compactLabel: t("calendar.pomodoro.longBreakCompact"),
      value: longBreakDraft,
      setDraft: (v: string) => { longBreakDraft = sanitizeCustomSlotDraft(v, 2); },
      commit: commitLongBreakDraft,
      restore: () => { longBreakDraft = formatDurationSlot(longBreakMinutes); },
      min: MIN_LONG_BREAK_MINUTES,
      max: Math.min(MAX_LONG_BREAK_MINUTES, CUSTOM_DURATION_SLOT_MAX),
      maxLength: 2,
      slotClass: "duration-summary-slot" as SummarySlotClass,
    },
    {
      label: t("calendar.pomodoro.longBreakAfter"),
      compactLabel: t("calendar.pomodoro.cycleCompact"),
      value: longBreakAfterDraft,
      setDraft: (v: string) => { longBreakAfterDraft = sanitizeCustomSlotDraft(v, 1); },
      commit: commitLongBreakAfterDraft,
      restore: () => { longBreakAfterDraft = formatCycleSlot(longBreakAfterFocusCount); },
      min: MIN_RHYTHM_POSITIONS,
      max: Math.min(MAX_RHYTHM_POSITIONS, CUSTOM_CYCLE_SLOT_MAX),
      maxLength: 1,
      slotClass: "cycle-summary-slot" as SummarySlotClass,
    },
  ]);
  const sequenceSummary = $derived(summarizeSequencePomodoroRhythm(sequenceSteps));

  const summary = $derived.by(() => {
    if (!enabled) return "";
    if (preset === "custom") {
      if (customRhythmMode === "sequence") {
        return t(
          "calendar.pomodoro.sequenceSummary",
          formatNumber(locale, sequenceSummary.stepCount),
          sequenceSummary.stepCount === 1,
          formatNumber(locale, sequenceSummary.focusMinutes),
          formatNumber(locale, sequenceSummary.breakMinutes),
        );
      }
      return t("calendar.pomodoro.customSummary", focusDurationMinutes, shortBreakMinutes, longBreakMinutes);
    }
    return presetLabel(preset) ?? t("calendar.pomodoro.custom");
  });
  const readonlyInteractiveClass = $derived(readonlyInteractive ? "readonly-interactive" : "");
  const readonlyInteractiveInputClass = $derived(
    readonlyInteractive ? "readonly-interactive-input" : "",
  );

  function presetLabel(p: BuiltInPomodoroPreset): string {
    if (p === "adaptive") return t("calendar.pomodoro.adaptive");
    if (p === "creative") return t("calendar.pomodoro.creative");
    if (p === "balanced") return t("calendar.pomodoro.balanced");
    if (p === "deep") return t("calendar.pomodoro.deepFocus");
    return t("calendar.pomodoro.extended");
  }

  function presetSummarySlots(
    p: BuiltInPomodoroPreset,
    rhythm: typeof COUNT_PRESET_RHYTHMS[BuiltInPomodoroPreset],
  ): Array<{ label: string; value: string; slotClass: SummarySlotClass }> {
    if (p === "adaptive") return [];
    return [
      {
        label: t("calendar.pomodoro.focusCompact"),
        value: formatPresetDurationSlot(rhythm.focusDurationMinutes),
        slotClass: "duration-summary-slot",
      },
      {
        label: t("calendar.pomodoro.shortBreakCompact"),
        value: formatPresetDurationSlot(rhythm.shortBreakMinutes),
        slotClass: "duration-summary-slot",
      },
      {
        label: t("calendar.pomodoro.longBreakCompact"),
        value: formatPresetDurationSlot(rhythm.longBreakMinutes),
        slotClass: "duration-summary-slot",
      },
      {
        label: t("calendar.pomodoro.cycleCompact"),
        value: formatCycleSlot(rhythm.longBreakAfterFocusCount),
        slotClass: "cycle-summary-slot",
      },
    ];
  }

  function applyPreset(p: PomodoroPreset) {
    const wasCustom = preset === "custom";
    if (p === "custom" && wasCustom) return;
    preset = p;
    if (p !== "custom") {
      const rhythm = PRESET_RHYTHMS[p];
      focusDurationMinutes = rhythm.focusDurationMinutes;
      shortBreakMinutes = rhythm.shortBreakMinutes;
      longBreakMinutes = rhythm.longBreakMinutes;
      longBreakAfterFocusCount = rhythm.longBreakAfterFocusCount;
    } else if (!wasCustom) {
      focusDurationMinutes = CUSTOM_COUNT_DEFAULT.focusDurationMinutes;
      shortBreakMinutes = CUSTOM_COUNT_DEFAULT.shortBreakMinutes;
      longBreakMinutes = CUSTOM_COUNT_DEFAULT.longBreakMinutes;
      longBreakAfterFocusCount = CUSTOM_COUNT_DEFAULT.longBreakAfterFocusCount;
    }
    customRhythmMode = "simple";
    onChange();
  }

  function convertSequenceToSimpleRhythm() {
    customRhythmMode = "simple";
    onChange();
  }

  async function focusPresetButton(index: number) {
    await tick();
    sectionEl
      ?.querySelector<HTMLButtonElement>(`[data-pomodoro-roving="preset"][data-roving-index="${index}"]`)
      ?.focus();
  }

  function handlePresetKeydown(e: KeyboardEvent, index: number) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const nextIndex = moveRovingIndex({
      currentIndex: index,
      itemCount: PRESET_OPTION_COUNT,
      key: e.key,
      orientation: "vertical",
    });
    if (nextIndex === index) return;
    e.preventDefault();
    e.stopPropagation();
    presetFocusIndex = nextIndex;
    void focusPresetButton(nextIndex);
  }

  function commitFocusDurationDraft() {
    const result = commitIntegerDraft(
      focusDurationDraft,
      focusDurationMinutes,
      MIN_FOCUS_MINUTES,
      Math.min(MAX_FOCUS_MINUTES, CUSTOM_DURATION_SLOT_MAX),
    );
    focusDurationDraft = formatDurationSlot(result.value);
    if (!result.committed) return;
    focusDurationMinutes = result.value;
    onChange();
  }

  function commitShortBreakDraft() {
    const result = commitIntegerDraft(
      shortBreakDraft,
      shortBreakMinutes,
      MIN_SHORT_BREAK_MINUTES,
      Math.min(MAX_SHORT_BREAK_MINUTES, CUSTOM_DURATION_SLOT_MAX),
    );
    shortBreakDraft = formatDurationSlot(result.value);
    if (!result.committed) return;
    shortBreakMinutes = result.value;
    onChange();
  }

  function commitLongBreakDraft() {
    const result = commitIntegerDraft(
      longBreakDraft,
      longBreakMinutes,
      MIN_LONG_BREAK_MINUTES,
      Math.min(MAX_LONG_BREAK_MINUTES, CUSTOM_DURATION_SLOT_MAX),
    );
    longBreakDraft = formatDurationSlot(result.value);
    if (!result.committed) return;
    longBreakMinutes = result.value;
    onChange();
  }

  function commitLongBreakAfterDraft() {
    const result = commitIntegerDraft(
      longBreakAfterDraft,
      longBreakAfterFocusCount,
      MIN_RHYTHM_POSITIONS,
      Math.min(MAX_RHYTHM_POSITIONS, CUSTOM_CYCLE_SLOT_MAX),
    );
    longBreakAfterDraft = formatCycleSlot(result.value);
    if (!result.committed) return;
    longBreakAfterFocusCount = result.value;
    onChange();
  }

  function formatDurationSlot(value: number): string {
    return String(value);
  }

  function formatPresetDurationSlot(value: number): string {
    return String(value).padStart(2, "0");
  }

  function formatCycleSlot(value: number): string {
    return String(value);
  }

  function sanitizeCustomSlotDraft(value: string, maxLength: number): string {
    return value.replace(/\D/g, "").slice(0, maxLength);
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

</script>

<div bind:this={sectionEl} class="flex flex-col overflow-hidden rounded-none" style="background-color: var(--panel-contrast);">
  <div class="section-header flex items-stretch">
    <button onclick={onToggle}
      class="flex w-10 shrink-0 items-center justify-center
        {readonlyInteractiveClass}
        {enabled ? 'bg-black/3 text-foreground dark:bg-black/30' : 'text-muted-foreground/50'}">
      <Timer size={14} />
    </button>
    <button onclick={onExpand}
      class="flex flex-1 items-center gap-2.5 px-3 py-2 text-left {readonlyInteractiveClass}">
      <span class="translate-y-[1.13px] text-[0.8rem] {enabled ? 'text-foreground' : 'text-muted-foreground'}">{t("calendar.pomodoro.title")}</span>
      <span class="ml-auto translate-y-[1.13px] truncate text-[0.733333rem] text-muted-foreground">{summary}</span>
    </button>
  </div>
  {#if expanded}
    <div transition:slide={{ duration: 180, easing: cubicOut }} data-section="pomodoro" class="flex flex-col gap-0 p-2.5" style="background-color: var(--panel-bg);">
      {#each localizedPresetEntries as { key, label, summarySlots }, index}
        <button
          onclick={() => applyPreset(key as PomodoroPreset)}
          onfocus={() => { presetFocusIndex = index; }}
          onkeydown={(e) => handlePresetKeydown(e, index)}
          data-pomodoro-roving="preset"
          data-roving-index={index}
          tabindex={presetFocusIndex === index ? 0 : -1}
          class="flex items-center gap-2.5 rounded-none px-3 py-1.5 text-left text-[0.8rem]
            {readonlyInteractiveClass}
            {preset === key ? 'bg-black/5 text-foreground dark:bg-black/15' : 'text-foreground'}"
        >
          <span>{label}</span>
          {#if summarySlots.length > 0}
            <span class="rhythm-summary ml-auto text-[0.733333rem] text-muted-foreground">
              {#each summarySlots as field, fieldIndex}
                <span>{field.label}</span>
                <span class="rhythm-summary-number {field.slotClass}">{field.value}</span>
                {#if fieldIndex < summarySlots.length - 1}
                  <span class="rhythm-summary-separator">/</span>
                {/if}
              {/each}
            </span>
          {/if}
        </button>
      {/each}
      {#if preset === "custom"}
        <div
          class="flex items-center gap-2.5 rounded-none bg-black/5 px-3 py-1.5 text-left text-[0.8rem] text-foreground dark:bg-black/15"
        >
          <button
            onclick={() => applyPreset("custom")}
            onfocus={() => { presetFocusIndex = PRESET_ENTRIES.length; }}
            onkeydown={(e) => handlePresetKeydown(e, PRESET_ENTRIES.length)}
            data-pomodoro-roving="preset"
            data-roving-index={PRESET_ENTRIES.length}
            tabindex={presetFocusIndex === PRESET_ENTRIES.length ? 0 : -1}
            class="text-left {readonlyInteractiveClass}"
          >
            {t("calendar.pomodoro.custom")}
          </button>
          {#if customRhythmMode === "sequence"}
            <span class="ml-auto text-[0.733333rem] text-muted-foreground">
              {t(
                "calendar.pomodoro.sequencePreserved",
                formatNumber(locale, sequenceSteps.length),
                sequenceSteps.length === 1,
              )}
            </span>
            <button
              type="button"
              onclick={convertSequenceToSimpleRhythm}
              class="text-[0.733333rem] text-primary hover:underline {readonlyInteractiveClass}"
            >
              {t("calendar.pomodoro.convertSequenceToSimple")}
            </button>
          {:else}
            <span class="rhythm-summary ml-auto text-[0.733333rem] text-muted-foreground">
              {#each customFields as field, fieldIndex}
                <span>{field.compactLabel}</span>
                <label class="contents">
                  <input type="text" inputmode="numeric" value={field.value} maxlength={field.maxLength}
                    aria-label={field.label}
                    oninput={(e) => field.setDraft(e.currentTarget.value)}
                    onblur={field.commit}
                    class="num-input rhythm-summary-number {field.slotClass} bg-transparent px-0 text-[0.733333rem] text-event-panel-input-text outline-none {readonlyInteractiveInputClass}"
                    onkeydown={(e) => handleNumberDraftKeydown(e, field.commit, field.restore)} />
                </label>
                {#if fieldIndex < customFields.length - 1}
                  <span class="rhythm-summary-separator">/</span>
                {/if}
              {/each}
            </span>
          {/if}
        </div>
      {:else}
        <button
          onclick={() => applyPreset("custom")}
          onfocus={() => { presetFocusIndex = PRESET_ENTRIES.length; }}
          onkeydown={(e) => handlePresetKeydown(e, PRESET_ENTRIES.length)}
          data-pomodoro-roving="preset"
          data-roving-index={PRESET_ENTRIES.length}
          tabindex={presetFocusIndex === PRESET_ENTRIES.length ? 0 : -1}
          class="flex items-center gap-2.5 rounded-none px-3 py-1.5 text-left text-[0.8rem] text-foreground {readonlyInteractiveClass}"
        >
          <span>{t("calendar.pomodoro.custom")}</span>
        </button>
      {/if}
      {#if idleDetectionAvailable}
      <div class="mt-1 border-t border-border/40 px-0 pt-0.5">
        <button
          onclick={() => { idleTimeoutEnabled = !idleTimeoutEnabled; onChange(); }}
          class="flex w-full items-center gap-2.5 rounded-none px-2.5 py-1.5 text-left text-[0.8rem] text-foreground {readonlyInteractiveClass}"
        >
          <div class="size-3 shrink-0
            {idleTimeoutEnabled ? 'bg-form-indicator' : 'border border-muted-foreground/40'}">
          </div>
          <span>{t("calendar.pomodoro.pauseOnInactivity")}</span>
        </button>
      </div>
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
  .rhythm-summary {
    display: grid;
    grid-template-columns: max-content 2ch max-content max-content 2ch max-content max-content 2ch max-content max-content 1ch;
    column-gap: 0.25rem;
    align-items: center;
    font-variant-numeric: tabular-nums;
  }
  .rhythm-summary-separator {
    text-align: center;
  }
  .rhythm-summary-number {
    display: inline-block;
    text-align: right;
  }
  .duration-summary-slot {
    width: 2ch;
  }
  .cycle-summary-slot {
    width: 1ch;
  }
</style>
