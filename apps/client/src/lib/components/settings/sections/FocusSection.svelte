<script lang="ts">
  import type { Snippet } from "svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import {
    DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
    DEFAULT_FOCUS_BREAK_END_ESC_PRESSES,
    DEFAULT_FOCUS_BREAK_END_WARNING_SECONDS,
    DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT,
    DEFAULT_FOCUS_BREAK_FINISHED_REPEAT_SECONDS,
    DEFAULT_FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES,
    FOCUS_BREAK_END_ESC_PRESS_OPTIONS,
    FOCUS_BREAK_EXTENSION_LIMIT_OPTIONS,
    FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS,
    FOCUS_BREAK_SOUND_INTERVAL_SECONDS_OPTIONS,
    FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_OPTIONS,
  } from "$lib/stores/preference-options";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";

  let { backgroundExecutionSettings }: { backgroundExecutionSettings?: Snippet } = $props();

  const preferences = getPreferences();
  const pomodoro = getPomodoro();
  const { t } = getLocalization();

  type SelectOption = { value: string; label: string };

  const DISABLED_SELECT_VALUE = "disabled";

  const idleThresholdOptions = $derived<SelectOption[]>(
    FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS.map((minutes) => ({
      value: String(minutes),
      label: t("settings.focus.minutesShort", minutes),
    })),
  );

  const breakFinishedRepeatOptions = $derived<SelectOption[]>(
    FOCUS_BREAK_SOUND_INTERVAL_SECONDS_OPTIONS.map((seconds) => {
      if (seconds === 0) return { value: String(seconds), label: t("settings.focus.optionNone") };
      if (seconds === 60) return { value: String(seconds), label: t("settings.focus.everyMinute") };
      return { value: String(seconds), label: t("settings.focus.everySeconds", seconds) };
    }),
  );

  const breakEndWarningOptions = $derived<SelectOption[]>(
    FOCUS_BREAK_SOUND_INTERVAL_SECONDS_OPTIONS.map((seconds) => {
      if (seconds === 0) return { value: String(seconds), label: t("settings.focus.optionNone") };
      if (seconds === 60) return { value: String(seconds), label: t("settings.focus.oneMinuteBefore") };
      return { value: String(seconds), label: t("settings.focus.secondsBefore", seconds) };
    }),
  );

  const breakEndEscPressOptions = $derived<SelectOption[]>([
    {
      value: DISABLED_SELECT_VALUE,
      label: t("settings.focus.optionDisabled"),
    },
    ...FOCUS_BREAK_END_ESC_PRESS_OPTIONS.map((presses) => ({
      value: String(presses),
      label: t("settings.focus.escPresses", presses),
    })),
  ]);

  const pausedFocusWarningOptions = $derived<SelectOption[]>(
    FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_OPTIONS.map((minutes) => {
      if (minutes === 0) return { value: String(minutes), label: t("settings.focus.optionNone") };
      return { value: String(minutes), label: t("settings.focus.everyMinutes", minutes) };
    }),
  );

  let showDisableIdlePauseConfirm = $state(false);
  let showDisableBreakEndConfirm = $state(false);
  let showDisableBreakExtensionConfirm = $state(false);

  function handleIdleThresholdChange(value: string): void {
    const minutes = Number(value);
    if (!Number.isFinite(minutes)) return;
    preferences.setFocusIdleThresholdMinutes(minutes);
    pomodoro.setActiveIdleThresholdMinutes(preferences.focusIdleThresholdMinutes);
  }

  function resetIdleThreshold(): void {
    preferences.resetFocusIdleThresholdMinutes();
    pomodoro.setActiveIdleThresholdMinutes(DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES);
  }

  function handleBreakFinishedRepeatChange(value: string): void {
    const seconds = Number(value);
    if (!Number.isFinite(seconds)) return;
    preferences.setFocusBreakFinishedRepeatSeconds(seconds);
  }

  function handleBreakEndWarningChange(value: string): void {
    const seconds = Number(value);
    if (!Number.isFinite(seconds)) return;
    preferences.setFocusBreakEndWarningSeconds(seconds);
  }

  function handleBreakEndEscPressChange(value: string): void {
    if (value === DISABLED_SELECT_VALUE) {
      if (preferences.focusBreakEndEscPresses === null) return;
      showDisableBreakEndConfirm = true;
      return;
    }
    const presses = Number(value);
    if (!Number.isFinite(presses)) return;
    preferences.setFocusBreakEndEscPresses(presses);
    showDisableBreakEndConfirm = false;
  }

  function handleBreakExtensionLimitChange(value: string): void {
    if (value === DISABLED_SELECT_VALUE) {
      if (preferences.focusBreakExtensionLimit === null) return;
      showDisableBreakExtensionConfirm = true;
      return;
    }
    const limit = Number(value);
    if (!Number.isFinite(limit)) return;
    preferences.setFocusBreakExtensionLimit(limit);
    showDisableBreakExtensionConfirm = false;
  }

  function handlePausedFocusWarningChange(value: string): void {
    const minutes = Number(value);
    if (!Number.isFinite(minutes)) return;
    preferences.setFocusPauseNotificationIntervalMinutes(minutes);
  }

  function handleIdlePauseDefaultChange(checked: boolean): void {
    if (checked) {
      preferences.setFocusIdlePauseOnEventCreate(true);
      return;
    }
    showDisableIdlePauseConfirm = true;
  }

  function confirmDisableIdlePauseDefault(): void {
    preferences.setFocusIdlePauseOnEventCreate(false);
    showDisableIdlePauseConfirm = false;
  }

  function cancelDisableIdlePauseDefault(): void {
    showDisableIdlePauseConfirm = false;
  }

  function confirmDisableBreakEnd(): void {
    preferences.setFocusBreakEndEscPresses(null);
    showDisableBreakEndConfirm = false;
  }

  function cancelDisableBreakEnd(): void {
    showDisableBreakEndConfirm = false;
  }

  function confirmDisableBreakExtension(): void {
    preferences.setFocusBreakExtensionLimit(null);
    showDisableBreakExtensionConfirm = false;
  }

  function cancelDisableBreakExtension(): void {
    showDisableBreakExtensionConfirm = false;
  }

  const breakExtensionLimitOptions = $derived<SelectOption[]>([
    {
      value: DISABLED_SELECT_VALUE,
      label: t("settings.focus.optionDisabled"),
    },
    ...FOCUS_BREAK_EXTENSION_LIMIT_OPTIONS.map((limit) => ({
      value: String(limit),
      label: t("settings.focus.times", limit),
    })),
  ]);
</script>

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.focus.idleDetectionHeading")}</h2>
    <div class="flex flex-col gap-3">
      <Select
        label={t("settings.focus.idleThreshold")}
        description={t("settings.focus.idleThresholdDescription")}
        value={String(preferences.focusIdleThresholdMinutes)}
        options={idleThresholdOptions}
        onChange={handleIdleThresholdChange}
        canReset={preferences.focusIdleThresholdMinutes !== DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES}
        onReset={resetIdleThreshold}
      />
      <SwitchField
        label={t("settings.focus.idlePauseDefault")}
        description={t("settings.focus.idlePauseDefaultDescription")}
        checked={preferences.focusIdlePauseOnEventCreate}
        onChange={handleIdlePauseDefaultChange}
      />
    </div>
  </section>

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.focus.notificationHeading")}</h2>
    <div class="flex flex-col gap-3">
      <Select
        label={t("settings.focus.pausedFocusWarning")}
        description={t("settings.focus.pausedFocusWarningDescription")}
        value={String(preferences.focusPauseNotificationIntervalMinutes)}
        options={pausedFocusWarningOptions}
        onChange={handlePausedFocusWarningChange}
        canReset={preferences.focusPauseNotificationIntervalMinutes !== DEFAULT_FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES}
        onReset={() => preferences.resetFocusPauseNotificationIntervalMinutes()}
      />
      {@render backgroundExecutionSettings?.()}
    </div>
  </section>

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.focus.breakScreenHeading")}</h2>
    <div class="flex flex-col gap-3">
        <Select
          label={t("settings.focus.endBreakEarly")}
          description={t("settings.focus.endBreakEarlyDescription")}
          value={preferences.focusBreakEndEscPresses === null
            ? DISABLED_SELECT_VALUE
            : String(preferences.focusBreakEndEscPresses)}
          options={breakEndEscPressOptions}
          onChange={handleBreakEndEscPressChange}
          canReset={preferences.focusBreakEndEscPresses !== DEFAULT_FOCUS_BREAK_END_ESC_PRESSES}
          onReset={() => preferences.resetFocusBreakEndEscPresses()}
        />
        <Select
          label={t("settings.focus.extendBreak")}
          description={t("settings.focus.extendBreakDescription")}
          value={preferences.focusBreakExtensionLimit === null
            ? DISABLED_SELECT_VALUE
            : String(preferences.focusBreakExtensionLimit)}
          options={breakExtensionLimitOptions}
          onChange={handleBreakExtensionLimitChange}
          canReset={preferences.focusBreakExtensionLimit !== DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT}
          onReset={() => preferences.resetFocusBreakExtensionLimit()}
        />
        <Select
          label={t("settings.focus.repeatAfterBreakEnds")}
          description={t("settings.focus.repeatAfterBreakEndsDescription")}
          value={String(preferences.focusBreakFinishedRepeatSeconds)}
          options={breakFinishedRepeatOptions}
          onChange={handleBreakFinishedRepeatChange}
          canReset={preferences.focusBreakFinishedRepeatSeconds !== DEFAULT_FOCUS_BREAK_FINISHED_REPEAT_SECONDS}
          onReset={() => preferences.resetFocusBreakFinishedRepeatSeconds()}
        />
        <Select
          label={t("settings.focus.warningBeforeBreakEnds")}
          description={t("settings.focus.warningBeforeBreakEndsDescription")}
          value={String(preferences.focusBreakEndWarningSeconds)}
          options={breakEndWarningOptions}
          onChange={handleBreakEndWarningChange}
          canReset={preferences.focusBreakEndWarningSeconds !== DEFAULT_FOCUS_BREAK_END_WARNING_SECONDS}
          onReset={() => preferences.resetFocusBreakEndWarningSeconds()}
        />
    </div>
  </section>
</div>

{#if showDisableIdlePauseConfirm}
  <ConfirmDialog
    title={t("settings.focus.idlePauseConfirmTitle")}
    message={t("settings.focus.idlePauseConfirmMessage")}
    confirmLabel={t("settings.focus.turnOff")}
    cancelLabel={t("settings.focus.keepOn")}
    onConfirm={confirmDisableIdlePauseDefault}
    onCancel={cancelDisableIdlePauseDefault}
  />
{/if}

{#if showDisableBreakEndConfirm}
  <ConfirmDialog
    title={t("settings.focus.disableBreakEndTitle")}
    message={t("settings.focus.disableBreakEndMessage")}
    confirmLabel={t("settings.focus.disable")}
    cancelLabel={t("settings.focus.keepCurrent")}
    onConfirm={confirmDisableBreakEnd}
    onCancel={cancelDisableBreakEnd}
  />
{/if}

{#if showDisableBreakExtensionConfirm}
  <ConfirmDialog
    title={t("settings.focus.disableBreakExtensionTitle")}
    message={t("settings.focus.disableBreakExtensionMessage")}
    confirmLabel={t("settings.focus.disable")}
    cancelLabel={t("settings.focus.keepCurrent")}
    onConfirm={confirmDisableBreakExtension}
    onCancel={cancelDisableBreakExtension}
  />
{/if}
