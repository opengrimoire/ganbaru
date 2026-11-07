<script lang="ts">
  import { tick, untrack } from "svelte";
  import Clock from "@lucide/svelte/icons/clock";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import {
    CLOCK_DIAL_MINUTE_LABEL_STEP,
    CLOCK_DIAL_RING_RADIUS,
    type ClockDialMode,
    type ClockPeriod,
    type ClockTime,
    type ClockTimeSubject,
    clockDialHand,
    clockDialItems,
    clockPeriod,
    clockTimeFromValue,
    continuousClockRotation,
    displayClockHour,
    formatClockValue,
    parseTypedClockTime,
    pickClockDialTime,
    stepClockTime,
    withClockPeriod,
  } from "$lib/utils/clock-dial";
  import { TOUCH_LONG_PRESS_MS, touchMovedBeyondSlop } from "$lib/utils/touch-hold";
  import PickerDialog from "./PickerDialog.svelte";

  /**
   * The touch time picker: a clock dial in a dialog. A press or drag on the dial picks the hour, then the dial turns
   * to minutes, where a tap lands on the nearest labeled mark and a drag or hold reaches every minute. The header
   * fields switch between the two, and the keyboard button swaps the dial for typed fields. Nothing changes until OK.
   */
  let {
    value,
    fallback = "",
    subject = undefined,
    label = undefined,
    onSelect,
    onCancel,
  }: {
    /** The current `HH:MM` value, or empty when none is set. */
    value: string;
    /** The `HH:MM` value the dial starts on when `value` is empty. */
    fallback?: string;
    /** What the time is for, named in the title, such as the start or end of a range. */
    subject?: ClockTimeSubject | undefined;
    /** Accessible name of the dialog; defaults to the picker title. */
    label?: string | undefined;
    onSelect: (time: string) => void;
    onCancel: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);
  const preferences = getPreferences();
  const format = $derived(preferences.calendarTimeFormat);

  /** The draft time; it starts from the value when the dialog opens and is only reported on OK. */
  let time = $state<ClockTime>(untrack(() => clockTimeFromValue(value, fallback)));
  let mode = $state<ClockDialMode>("hour");
  let typing = $state(false);
  let hourText = $state("");
  let minuteText = $state("");
  let dialElement = $state<HTMLDivElement>();
  let hourInput = $state<HTMLInputElement>();
  let minuteInput = $state<HTMLInputElement>();
  let pressing = $state(false);
  let dragged = $state(false);
  /** Whether the current press picks exact minutes; a plain tap snaps to the labeled marks. */
  let precise = false;
  let pressX = 0;
  let pressY = 0;
  let pointerX = 0;
  let pointerY = 0;
  let holdTimer: ReturnType<typeof setTimeout> | undefined;

  const period = $derived(clockPeriod(time.hour));
  const items = $derived(clockDialItems(mode, format, period));
  const hand = $derived(clockDialHand(time, mode, format));
  const typedTime = $derived(typing ? parseTypedClockTime(hourText, minuteText, format, period) : time);
  const title = $derived.by(() => {
    if (subject === "start") return typing ? t("common.timePicker.enterStartTime") : t("common.timePicker.selectStartTime");
    if (subject === "end") return typing ? t("common.timePicker.enterEndTime") : t("common.timePicker.selectEndTime");
    if (subject === "due") return typing ? t("common.timePicker.enterDueTime") : t("common.timePicker.selectDueTime");
    return typing ? t("common.timePicker.enterTime") : t("common.timePicker.selectTime");
  });

  $effect(() => clearHold);

  let lastRotation = 0;
  /** The hand rotation, continued from the last one so the hand turns the short way. */
  const rotation = $derived.by(() => {
    lastRotation = continuousClockRotation(lastRotation, hand.angle);
    return lastRotation;
  });
  const handStyle = $derived(`transform: rotate(${rotation}deg); --clock-hand-reach: ${CLOCK_DIAL_RING_RADIUS[hand.ring] * 50}%;`);

  function twoDigits(number: number): string {
    return formatNumber(locale, number, { minimumIntegerDigits: 2, useGrouping: false });
  }

  function plainNumber(number: number): string {
    return formatNumber(locale, number, { useGrouping: false });
  }

  function hourLabel(hour: number): string {
    const shown = displayClockHour(hour, format);
    return format === "24h" ? twoDigits(shown) : plainNumber(shown);
  }

  function faceLabel(label: number): string {
    return mode === "minute" ? twoDigits(label) : plainNumber(label);
  }

  /** Percent offsets of a face label from the dial's top left corner. */
  function labelPosition(angle: number, ring: "outer" | "inner"): string {
    const radians = (angle * Math.PI) / 180;
    const radius = CLOCK_DIAL_RING_RADIUS[ring] * 50;
    return `left: ${50 + Math.sin(radians) * radius}%; top: ${50 - Math.cos(radians) * radius}%;`;
  }

  /** The time at the last pointer position, snapping minutes to the labeled marks unless the press is precise. */
  function pointerTime(): ClockTime {
    if (!dialElement) return time;
    const rect = dialElement.getBoundingClientRect();
    return pickClockDialTime({
      dx: pointerX - (rect.left + rect.width / 2),
      dy: pointerY - (rect.top + rect.height / 2),
      radius: rect.width / 2,
      mode,
      format,
      time,
      minuteStep: precise ? 1 : CLOCK_DIAL_MINUTE_LABEL_STEP,
    });
  }

  function clearHold(): void {
    if (holdTimer !== undefined) clearTimeout(holdTimer);
    holdTimer = undefined;
  }

  /** A press held in place switches to exact minutes. */
  function handleHold(): void {
    holdTimer = undefined;
    if (!pressing) return;
    precise = true;
    time = pointerTime();
  }

  function handlePointerDown(event: PointerEvent): void {
    if (event.button !== 0 || !dialElement) return;
    event.preventDefault();
    dialElement.setPointerCapture(event.pointerId);
    pressing = true;
    dragged = false;
    precise = false;
    pressX = pointerX = event.clientX;
    pressY = pointerY = event.clientY;
    clearHold();
    holdTimer = setTimeout(handleHold, TOUCH_LONG_PRESS_MS);
    time = pointerTime();
  }

  function handlePointerMove(event: PointerEvent): void {
    if (!pressing) return;
    pointerX = event.clientX;
    pointerY = event.clientY;
    if (!dragged && touchMovedBeyondSlop(pointerX - pressX, pointerY - pressY)) {
      dragged = true;
      precise = true;
      clearHold();
    }
    time = pointerTime();
  }

  function handlePointerUp(event: PointerEvent): void {
    if (!pressing) return;
    pointerX = event.clientX;
    pointerY = event.clientY;
    time = pointerTime();
    endPress();
    if (mode === "hour") mode = "minute";
  }

  function endPress(): void {
    clearHold();
    pressing = false;
    dragged = false;
    precise = false;
  }

  function handleDialKeydown(event: KeyboardEvent): void {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const step = event.key === "ArrowUp" || event.key === "ArrowRight"
      ? 1
      : event.key === "ArrowDown" || event.key === "ArrowLeft" ? -1 : 0;
    if (step !== 0) {
      event.preventDefault();
      time = stepClockTime(time, mode, format, mode === "minute" && event.shiftKey ? step * CLOCK_DIAL_MINUTE_LABEL_STEP : step);
      return;
    }
    if (event.key === "Enter" && mode === "hour") {
      event.preventDefault();
      mode = "minute";
    }
  }

  function selectPeriod(next: ClockPeriod): void {
    time = withClockPeriod(time, next);
  }

  function typedFields(current: ClockTime): { hour: string; minute: string } {
    const shown = displayClockHour(current.hour, format);
    return {
      hour: format === "24h" ? String(shown).padStart(2, "0") : String(shown),
      minute: String(current.minute).padStart(2, "0"),
    };
  }

  async function toggleTyping(): Promise<void> {
    if (typing) {
      if (typedTime) time = typedTime;
      typing = false;
      mode = "hour";
      return;
    }
    const fields = typedFields(time);
    hourText = fields.hour;
    minuteText = fields.minute;
    typing = true;
    await tick();
    hourInput?.focus();
    hourInput?.select();
  }

  function digitsOnly(text: string): string {
    return text.replace(/\D/g, "").slice(0, 2);
  }

  function handleHourInput(event: Event & { currentTarget: HTMLInputElement }): void {
    hourText = digitsOnly(event.currentTarget.value);
    event.currentTarget.value = hourText;
    if (hourText.length === 2) {
      minuteInput?.focus();
      minuteInput?.select();
    }
  }

  function handleMinuteInput(event: Event & { currentTarget: HTMLInputElement }): void {
    minuteText = digitsOnly(event.currentTarget.value);
    event.currentTarget.value = minuteText;
  }

  function handleTypedKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter") return;
    event.preventDefault();
    confirm();
  }

  function confirm(): void {
    if (!typedTime) return;
    onSelect(formatClockValue(typedTime));
  }
</script>

{#snippet periodToggle()}
  <div class="clock-period" role="group">
    {#each [{ value: "am" as const, text: t("common.timePicker.am") }, { value: "pm" as const, text: t("common.timePicker.pm") }] as option (option.value)}
      <button type="button" aria-pressed={period === option.value} data-selected={period === option.value || undefined}
        onclick={() => selectPeriod(option.value)}>{option.text}</button>
    {/each}
  </div>
{/snippet}

{#snippet faceLabels()}
  {#each items as item (`${item.ring}-${item.value}`)}
    <span class="clock-label" data-ring={item.ring} style={labelPosition(item.angle, item.ring)}>{faceLabel(item.label)}</span>
  {/each}
{/snippet}

<PickerDialog label={label ?? title} onClose={onCancel}>
  <div class="clock-picker px-2 pt-2">
    <p class="mb-5 text-[0.8rem] text-muted-foreground">{title}</p>

    {#if typing}
      <div class="flex items-start justify-center gap-1.5">
        <label class="flex flex-col gap-1.5">
          <input bind:this={hourInput} class="clock-field" data-active="true" data-invalid={!typedTime || undefined}
            type="text" inputmode="numeric" autocomplete="off" maxlength="2" value={hourText}
            aria-label={t("common.timePicker.hour")}
            oninput={handleHourInput} onkeydown={handleTypedKeydown} />
          <span class="text-[0.733333rem] text-muted-foreground">{t("common.timePicker.hour")}</span>
        </label>
        <span class="clock-separator" aria-hidden="true">:</span>
        <label class="flex flex-col gap-1.5">
          <input bind:this={minuteInput} class="clock-field" data-active="true" data-invalid={!typedTime || undefined}
            type="text" inputmode="numeric" autocomplete="off" maxlength="2" value={minuteText}
            aria-label={t("common.timePicker.minute")}
            oninput={handleMinuteInput} onkeydown={handleTypedKeydown} />
          <span class="text-[0.733333rem] text-muted-foreground">{t("common.timePicker.minute")}</span>
        </label>
        {#if format === "12h"}{@render periodToggle()}{/if}
      </div>
    {:else}
      <div class="flex items-center justify-center gap-1.5">
        <button type="button" class="clock-field" data-active={mode === "hour" || undefined} aria-pressed={mode === "hour"}
          aria-label={`${t("common.timePicker.hour")} ${hourLabel(time.hour)}`}
          onclick={() => { mode = "hour"; }}>{hourLabel(time.hour)}</button>
        <span class="clock-separator" aria-hidden="true">:</span>
        <button type="button" class="clock-field" data-active={mode === "minute" || undefined} aria-pressed={mode === "minute"}
          aria-label={`${t("common.timePicker.minute")} ${twoDigits(time.minute)}`}
          onclick={() => { mode = "minute"; }}>{twoDigits(time.minute)}</button>
        {#if format === "12h"}{@render periodToggle()}{/if}
      </div>

      <div
        bind:this={dialElement}
        class="clock-dial mx-auto mt-8"
        data-animated={!dragged || undefined}
        role="slider"
        tabindex="0"
        aria-label={mode === "hour" ? t("common.timePicker.hour") : t("common.timePicker.minute")}
        aria-valuemin={mode === "hour" ? (format === "24h" ? 0 : 1) : 0}
        aria-valuemax={mode === "hour" ? (format === "24h" ? 23 : 12) : 59}
        aria-valuenow={mode === "hour" ? displayClockHour(time.hour, format) : time.minute}
        aria-valuetext={mode === "hour" ? hourLabel(time.hour) : twoDigits(time.minute)}
        onpointerdown={handlePointerDown}
        onpointermove={handlePointerMove}
        onpointerup={handlePointerUp}
        onpointercancel={endPress}
        onlostpointercapture={endPress}
        onkeydown={handleDialKeydown}
      >
        <div class="clock-hand" aria-hidden="true" style={handStyle}>
          <span class="clock-hand-line"></span>
        </div>
        {#key mode}
          <div class="clock-face" aria-hidden="true">{@render faceLabels()}</div>
        {/key}
        <!-- The knob repeats the face in its contrast color, clipped to the knob and turned back upright, so the numbers
             read over the knob as if it sat beneath them. -->
        <div class="clock-hand" aria-hidden="true" style={handStyle}>
          <div class="clock-knob">
            <div class="clock-knob-turn" style="transform: rotate({-rotation}deg);">
              {#key mode}
                <div class="clock-face">{@render faceLabels()}</div>
              {/key}
            </div>
          </div>
        </div>
        <span class="clock-center"></span>
      </div>
    {/if}

    <div class="mt-6 flex items-center gap-1">
      <button type="button" class="flex size-12 items-center justify-center rounded-full text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-label={typing ? t("common.timePicker.useClock") : t("common.timePicker.typeTime")}
        onclick={() => { void toggleTyping(); }}>
        {#if typing}<Clock size={22} strokeWidth={1.75} />{:else}<Keyboard size={22} strokeWidth={1.75} />{/if}
      </button>
      <span class="flex-1"></span>
      <button type="button" class="clock-action text-muted-foreground" onclick={onCancel}>{t("common.cancel")}</button>
      <button type="button" class="clock-action text-foreground" disabled={!typedTime} onclick={confirm}>{t("common.timePicker.ok")}</button>
    </div>
  </div>
</PickerDialog>

<style>
  .clock-picker {
    --clock-tint: color-mix(in srgb, var(--foreground) 8%, transparent);
    --clock-motion: 200ms cubic-bezier(0.2, 0, 0, 1);
  }

  .clock-field {
    display: flex;
    width: 6rem;
    height: 5rem;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-xl);
    background-color: var(--clock-tint);
    color: var(--foreground);
    font-size: 3.4rem;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    text-align: center;
    outline: none;
    transition: background-color var(--clock-motion), color var(--clock-motion);
  }

  .clock-field[data-active] {
    background-color: var(--primary);
    color: var(--primary-foreground);
  }

  .clock-field[data-invalid] {
    background-color: color-mix(in srgb, var(--destructive) 18%, transparent);
    color: var(--foreground);
  }

  .clock-separator {
    display: flex;
    height: 5rem;
    align-items: center;
    color: var(--foreground);
    font-size: 3.4rem;
    line-height: 1;
  }

  .clock-period {
    display: flex;
    height: 5rem;
    flex-direction: column;
    margin-left: 0.5rem;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
  }

  .clock-period button {
    flex: 1;
    min-width: 3.25rem;
    padding-inline: 0.5rem;
    color: var(--muted-foreground);
    font-size: 0.866667rem;
    font-weight: 500;
  }

  .clock-period button + button {
    border-top: 1px solid var(--border);
  }

  .clock-period button[data-selected] {
    background-color: color-mix(in srgb, var(--primary) 22%, transparent);
    color: var(--foreground);
  }

  .clock-dial {
    position: relative;
    width: min(16.5rem, 100%);
    aspect-ratio: 1;
    border-radius: 9999px;
    overflow: hidden;
    background-color: var(--clock-tint);
    outline: none;
    touch-action: none;
    user-select: none;
  }

  .clock-hand {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  .clock-dial[data-animated] .clock-hand {
    transition: transform var(--clock-motion);
  }

  .clock-hand-line {
    position: absolute;
    bottom: 50%;
    left: calc(50% - 1px);
    width: 2px;
    height: var(--clock-hand-reach);
    background-color: var(--primary);
  }

  .clock-dial[data-animated] .clock-hand-line {
    transition: height var(--clock-motion);
  }

  /* The dial is square, so a percentage circle radius is a fraction of its width. The layer fills the dial and turns
     with the hand, so its corners reach past the dial, which clips them to keep them out of the dialog's scroll area. */
  .clock-knob {
    position: absolute;
    inset: 0;
    background-color: var(--primary);
    clip-path: circle(9.375% at 50% calc(50% - var(--clock-hand-reach)));
  }

  .clock-knob-turn {
    position: absolute;
    inset: 0;
  }

  .clock-knob .clock-label {
    color: var(--primary-foreground);
  }

  .clock-dial[data-animated] .clock-knob {
    transition: clip-path var(--clock-motion);
  }

  .clock-dial[data-animated] .clock-knob-turn {
    transition: transform var(--clock-motion);
  }

  .clock-center {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 9999px;
    background-color: var(--primary);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  .clock-face {
    position: absolute;
    inset: 0;
    pointer-events: none;
    animation: clock-face-in var(--clock-motion);
  }

  .clock-label {
    position: absolute;
    color: var(--foreground);
    font-size: 1rem;
    font-variant-numeric: tabular-nums;
    line-height: 1;
    transform: translate(-50%, -50%);
  }

  .clock-label[data-ring="inner"] {
    font-size: 0.933333rem;
  }

  .clock-action {
    min-height: 3rem;
    padding-inline: 1rem;
    border-radius: 9999px;
    font-size: 0.933333rem;
    font-weight: 500;
  }

  .clock-action:hover:not(:disabled) {
    background-color: var(--accent);
  }

  .clock-action:disabled {
    opacity: 0.45;
  }

  @keyframes clock-face-in {
    from {
      opacity: 0;
      transform: scale(0.92);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .clock-dial[data-animated] .clock-hand,
    .clock-dial[data-animated] .clock-hand-line,
    .clock-dial[data-animated] .clock-knob,
    .clock-dial[data-animated] .clock-knob-turn,
    .clock-field {
      transition: none;
    }

    .clock-face {
      animation: none;
    }
  }
</style>
