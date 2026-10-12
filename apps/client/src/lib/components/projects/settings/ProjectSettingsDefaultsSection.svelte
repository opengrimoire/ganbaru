<script lang="ts">
  import EventColorPicker from "$lib/components/calendar/EventColorPicker.svelte";
  import type { EventColor } from "$lib/calendar/types";
  import { commitIntegerDraft, panelInputKeydown } from "$lib/calendar/event-panel-utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
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
    type PomodoroPresetKey,
  } from "$lib/pomodoro/rhythm";
  import {
    PROJECT_DEFAULT_CUSTOM_POMODORO,
    projectPomodoroSummaryLabel,
    type ProjectDefaultIdleSettingsSource,
    type ProjectDefaultPomodoroMode,
  } from "$lib/projects/default-pomodoro";
  import {
    isProjectCustomDurationInputShape,
    PROJECT_DURATION_PRESET_MINUTES,
    projectCustomDurationDraftFromMinutes,
    projectDurationMinutesFromCustomInput,
    projectDurationPresetFromMinutes,
    type ProjectDefaultEventTimeMode,
    type ProjectDurationPresetValue,
    type ProjectDurationUnit,
  } from "$lib/projects/settings/duration";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { moveTextInputCaretToPointer } from "$lib/utils/text-input-caret";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import {
    FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS,
    type FocusIdleThresholdMinutes,
  } from "$lib/stores/preference-options";
  import type { Theme } from "$lib/themes";
  import ProjectSettingsSectionHeading from "./ProjectSettingsSectionHeading.svelte";
  import ProjectSettingsPlaylistRows from "./ProjectSettingsPlaylistRows.svelte";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";

  let {
    theme,
    pomodoroOptions,
    pomodoroPresetLabel,
    projectColorDraft = $bindable<EventColor | undefined>(),
    projectDefaultEventNameDraft = $bindable<string>(),
    projectDefaultEventNamePlaceholder,
    projectEventTimeModeDraft = $bindable<ProjectDefaultEventTimeMode>(),
    projectDurationDraft = $bindable<string>(),
    projectPomodoroModeDraft = $bindable<ProjectDefaultPomodoroMode>(),
    projectPomodoroPresetDraft = $bindable<PomodoroPresetKey>(),
    projectPomodoroFocusDraft = $bindable<number>(),
    projectPomodoroShortBreakDraft = $bindable<number>(),
    projectPomodoroLongBreakDraft = $bindable<number>(),
    projectPomodoroLongBreakAfterFocusDraft = $bindable<number>(),
    projectIdleSettingsSourceDraft = $bindable<ProjectDefaultIdleSettingsSource>(),
    projectIdlePauseEnabledDraft = $bindable<boolean>(),
    projectIdleThresholdMinutesDraft = $bindable<FocusIdleThresholdMinutes>(),
    musicAssignments,
    musicPlaylists,
    onMusicAssignmentsChange,
    loadingMusicPlaylists = false,
    musicAssignmentsError = null,
    onRetryMusicAssignments,
    musicAssignmentsDisabled = false,
    musicAssignmentsAvailable = true,
    idleDetectionAvailable = true,
  }: {
    theme: Theme;
    pomodoroOptions: readonly PomodoroPresetKey[];
    pomodoroPresetLabel: (preset: PomodoroPresetKey) => string;
    projectColorDraft: EventColor | undefined;
    projectDefaultEventNameDraft: string;
    projectDefaultEventNamePlaceholder?: string;
    projectEventTimeModeDraft: ProjectDefaultEventTimeMode;
    projectDurationDraft: string;
    projectPomodoroModeDraft: ProjectDefaultPomodoroMode;
    projectPomodoroPresetDraft: PomodoroPresetKey;
    projectPomodoroFocusDraft: number;
    projectPomodoroShortBreakDraft: number;
    projectPomodoroLongBreakDraft: number;
    projectPomodoroLongBreakAfterFocusDraft: number;
    projectIdleSettingsSourceDraft: ProjectDefaultIdleSettingsSource;
    projectIdlePauseEnabledDraft: boolean;
    projectIdleThresholdMinutesDraft: FocusIdleThresholdMinutes;
    musicAssignments: readonly MusicContextAssignmentDraft[];
    musicPlaylists: readonly MusicPlaylistSummary[];
    onMusicAssignmentsChange: (assignments: MusicContextAssignmentDraft[]) => void;
    loadingMusicPlaylists?: boolean;
    musicAssignmentsError?: string | null;
    onRetryMusicAssignments: () => Promise<void> | void;
    musicAssignmentsDisabled?: boolean;
    musicAssignmentsAvailable?: boolean;
    idleDetectionAvailable?: boolean;
  } = $props();

  const { t } = getLocalization();

  type SelectOption = { value: string; label: string; summary?: string };
  type PomodoroSelectValue = "none" | PomodoroPresetKey | "custom";
  type PomodoroCustomField = {
    label: string;
    compactLabel: string;
    value: string;
    setDraft: (value: string) => void;
    commit: () => void;
    restore: () => void;
    maxLength: number;
    slotClass: "duration-summary-slot" | "cycle-summary-slot";
  };

  const CUSTOM_DURATION_SLOT_MAX = 99;
  const CUSTOM_CYCLE_SLOT_MAX = 9;

  let durationPreset = $state<ProjectDurationPresetValue>("default");
  let customDurationValue = $state("");
  let customDurationUnit = $state<ProjectDurationUnit>("hours");
  let lastSyncedDurationDraft = $state("");
  let lastSyncedEventTimeMode = $state<ProjectDefaultEventTimeMode>("timed");
  let pomodoroFocusInputDraft = $state(String(PROJECT_DEFAULT_CUSTOM_POMODORO.focusDurationMinutes));
  let pomodoroShortBreakInputDraft = $state(String(PROJECT_DEFAULT_CUSTOM_POMODORO.shortBreakMinutes));
  let pomodoroLongBreakInputDraft = $state(String(PROJECT_DEFAULT_CUSTOM_POMODORO.longBreakMinutes));
  let pomodoroLongBreakAfterInputDraft = $state(String(PROJECT_DEFAULT_CUSTOM_POMODORO.longBreakAfterFocusCount));

  const durationPresetOptions = $derived<SelectOption[]>([
    { value: "default", label: t("projects.settings.defaultDurationNone") },
    { value: "10", label: t("projects.settings.durationMinutes", 10) },
    { value: "15", label: t("projects.settings.durationMinutes", 15) },
    { value: "30", label: t("projects.settings.durationMinutes", 30) },
    { value: "60", label: t("projects.settings.durationHours", 1) },
    { value: "120", label: t("projects.settings.durationHours", 2) },
    { value: "180", label: t("projects.settings.durationHours", 3) },
    { value: "240", label: t("projects.settings.durationHours", 4) },
    { value: "all_day", label: t("projects.settings.defaultDurationAllDay") },
    { value: "custom", label: t("projects.settings.durationCustom") },
  ]);
  const durationUnitOptions = $derived<SelectOption[]>([
    { value: "hours", label: t("projects.settings.durationUnitHours") },
    { value: "minutes", label: t("projects.settings.durationUnitMinutes") },
  ]);
  const idleThresholdOptions = $derived<SelectOption[]>(
    FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS.map((minutes) => ({
      value: String(minutes),
      label: t("settings.focus.minutesShort", minutes),
    })),
  );
  const pomodoroSelectValue = $derived<PomodoroSelectValue>(
    projectPomodoroModeDraft === "none"
      ? "none"
      : projectPomodoroModeDraft === "custom"
        ? "custom"
        : projectPomodoroPresetDraft,
  );
  const pomodoroSelectOptions = $derived<SelectOption[]>([
    { value: "none", label: t("common.none") },
    ...pomodoroOptions.map((preset) => ({
      value: preset,
      label: pomodoroPresetLabel(preset),
      summary: projectPomodoroSummaryLabel(COUNT_PRESET_RHYTHMS[preset]),
    })),
    {
      value: "custom",
      label: t("calendar.pomodoro.custom"),
    },
  ]);
  const pomodoroCustomFields = $derived<PomodoroCustomField[]>([
    {
      label: t("calendar.pomodoro.focus"),
      compactLabel: t("calendar.pomodoro.focusCompact"),
      value: pomodoroFocusInputDraft,
      setDraft: (value: string) => {
        pomodoroFocusInputDraft = sanitizePomodoroNumberDraft(value, 2);
      },
      commit: commitPomodoroFocusDraft,
      restore: () => {
        pomodoroFocusInputDraft = formatPomodoroDurationSlot(projectPomodoroFocusDraft);
      },
      maxLength: 2,
      slotClass: "duration-summary-slot",
    },
    {
      label: t("calendar.pomodoro.shortBreak"),
      compactLabel: t("calendar.pomodoro.shortBreakCompact"),
      value: pomodoroShortBreakInputDraft,
      setDraft: (value: string) => {
        pomodoroShortBreakInputDraft = sanitizePomodoroNumberDraft(value, 2);
      },
      commit: commitPomodoroShortBreakDraft,
      restore: () => {
        pomodoroShortBreakInputDraft = formatPomodoroDurationSlot(projectPomodoroShortBreakDraft);
      },
      maxLength: 2,
      slotClass: "duration-summary-slot",
    },
    {
      label: t("calendar.pomodoro.longBreak"),
      compactLabel: t("calendar.pomodoro.longBreakCompact"),
      value: pomodoroLongBreakInputDraft,
      setDraft: (value: string) => {
        pomodoroLongBreakInputDraft = sanitizePomodoroNumberDraft(value, 2);
      },
      commit: commitPomodoroLongBreakDraft,
      restore: () => {
        pomodoroLongBreakInputDraft = formatPomodoroDurationSlot(projectPomodoroLongBreakDraft);
      },
      maxLength: 2,
      slotClass: "duration-summary-slot",
    },
    {
      label: t("calendar.pomodoro.longBreakAfter"),
      compactLabel: t("calendar.pomodoro.cycleCompact"),
      value: pomodoroLongBreakAfterInputDraft,
      setDraft: (value: string) => {
        pomodoroLongBreakAfterInputDraft = sanitizePomodoroNumberDraft(value, 1);
      },
      commit: commitPomodoroLongBreakAfterDraft,
      restore: () => {
        pomodoroLongBreakAfterInputDraft = formatPomodoroCycleSlot(projectPomodoroLongBreakAfterFocusDraft);
      },
      maxLength: 1,
      slotClass: "cycle-summary-slot",
    },
  ]);

  $effect(() => {
    pomodoroFocusInputDraft = formatPomodoroDurationSlot(projectPomodoroFocusDraft);
  });

  $effect(() => {
    pomodoroShortBreakInputDraft = formatPomodoroDurationSlot(projectPomodoroShortBreakDraft);
  });

  $effect(() => {
    pomodoroLongBreakInputDraft = formatPomodoroDurationSlot(projectPomodoroLongBreakDraft);
  });

  $effect(() => {
    pomodoroLongBreakAfterInputDraft = formatPomodoroCycleSlot(projectPomodoroLongBreakAfterFocusDraft);
  });

  function parseStoredDurationDraft(value: string): number | null | "custom" {
    const trimmed = value.trim();
    if (!trimmed) return null;
    const parsed = Number(value.trim());
    return Number.isFinite(parsed) && parsed > 0
      ? Math.round(parsed)
      : "custom";
  }

  function syncDurationControlsFromDraft(value: string, timeMode = projectEventTimeModeDraft): void {
    if (timeMode === "all_day") {
      durationPreset = "all_day";
      customDurationValue = "";
      customDurationUnit = "hours";
      lastSyncedDurationDraft = value;
      lastSyncedEventTimeMode = timeMode;
      return;
    }
    const minutes = parseStoredDurationDraft(value);
    durationPreset = minutes === "custom" ? "custom" : projectDurationPresetFromMinutes(minutes);
    if (durationPreset === "custom") {
      if (typeof minutes === "number") {
        const draft = projectCustomDurationDraftFromMinutes(minutes);
        customDurationValue = draft.value;
        customDurationUnit = draft.unit;
      } else {
        customDurationValue = "";
        customDurationUnit = "hours";
      }
    } else {
      customDurationValue = "";
      customDurationUnit = "hours";
    }
    lastSyncedDurationDraft = value;
    lastSyncedEventTimeMode = timeMode;
  }

  syncDurationControlsFromDraft(projectDurationDraft, projectEventTimeModeDraft);

  $effect(() => {
    if (
      projectDurationDraft !== lastSyncedDurationDraft
      || projectEventTimeModeDraft !== lastSyncedEventTimeMode
    ) {
      syncDurationControlsFromDraft(projectDurationDraft, projectEventTimeModeDraft);
    }
  });

  function setProjectDurationDraft(value: string): void {
    projectDurationDraft = value;
    lastSyncedDurationDraft = value;
  }

  function isDurationPresetValue(value: string): value is ProjectDurationPresetValue {
    return value === "default"
      || value === "all_day"
      || value === "custom"
      || value in PROJECT_DURATION_PRESET_MINUTES;
  }

  function isDurationUnit(value: string): value is ProjectDurationUnit {
    return value === "minutes" || value === "hours";
  }

  function setDurationPreset(value: string): void {
    if (!isDurationPresetValue(value)) return;
    durationPreset = value;
    if (value === "all_day") {
      projectEventTimeModeDraft = "all_day";
      customDurationValue = "";
      customDurationUnit = "hours";
      setProjectDurationDraft("");
      return;
    }
    projectEventTimeModeDraft = "timed";
    if (value === "custom") {
      customDurationValue = "";
      customDurationUnit = "hours";
      setProjectDurationDraft("custom");
      return;
    }
    if (value === "default") {
      setProjectDurationDraft("");
      return;
    }
    setProjectDurationDraft(String(PROJECT_DURATION_PRESET_MINUTES[value]));
  }

  function syncCustomDurationDraft(): void {
    projectEventTimeModeDraft = "timed";
    const minutes = projectDurationMinutesFromCustomInput(customDurationValue, customDurationUnit);
    setProjectDurationDraft(minutes === null ? "custom" : String(minutes));
  }

  function setCustomDurationValue(input: HTMLInputElement): void {
    const normalized = input.value.trim().replace(",", ".");
    if (!isProjectCustomDurationInputShape(normalized)) {
      input.value = customDurationValue;
      return;
    }
    customDurationValue = normalized;
    syncCustomDurationDraft();
  }

  function setCustomDurationUnit(value: string): void {
    if (!isDurationUnit(value)) return;
    customDurationUnit = value;
    syncCustomDurationDraft();
  }

  function setPomodoroPreset(value: string): void {
    if (value === "none") {
      projectPomodoroModeDraft = "none";
      return;
    }
    if (value === "custom") {
      projectPomodoroModeDraft = "custom";
      return;
    }
    if (pomodoroOptions.includes(value as PomodoroPresetKey)) {
      projectPomodoroModeDraft = "preset";
      projectPomodoroPresetDraft = value as PomodoroPresetKey;
    }
  }

  function isFocusIdleThresholdMinutes(value: number): value is FocusIdleThresholdMinutes {
    return FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS.some((option) => option === value);
  }

  function setUseGlobalIdleSettings(checked: boolean): void {
    projectIdleSettingsSourceDraft = checked ? "global" : "custom";
  }

  function setIdleThresholdMinutes(value: string): void {
    const minutes = Number(value);
    if (Number.isInteger(minutes) && isFocusIdleThresholdMinutes(minutes)) {
      projectIdleThresholdMinutesDraft = minutes;
    }
  }

  function commitPomodoroFocusDraft(): void {
    const result = commitIntegerDraft(
      pomodoroFocusInputDraft,
      projectPomodoroFocusDraft,
      MIN_FOCUS_MINUTES,
      Math.min(MAX_FOCUS_MINUTES, CUSTOM_DURATION_SLOT_MAX),
    );
    pomodoroFocusInputDraft = formatPomodoroDurationSlot(result.value);
    if (result.committed) projectPomodoroFocusDraft = result.value;
  }

  function commitPomodoroShortBreakDraft(): void {
    const result = commitIntegerDraft(
      pomodoroShortBreakInputDraft,
      projectPomodoroShortBreakDraft,
      MIN_SHORT_BREAK_MINUTES,
      Math.min(MAX_SHORT_BREAK_MINUTES, CUSTOM_DURATION_SLOT_MAX),
    );
    pomodoroShortBreakInputDraft = formatPomodoroDurationSlot(result.value);
    if (result.committed) projectPomodoroShortBreakDraft = result.value;
  }

  function commitPomodoroLongBreakDraft(): void {
    const result = commitIntegerDraft(
      pomodoroLongBreakInputDraft,
      projectPomodoroLongBreakDraft,
      MIN_LONG_BREAK_MINUTES,
      Math.min(MAX_LONG_BREAK_MINUTES, CUSTOM_DURATION_SLOT_MAX),
    );
    pomodoroLongBreakInputDraft = formatPomodoroDurationSlot(result.value);
    if (result.committed) projectPomodoroLongBreakDraft = result.value;
  }

  function commitPomodoroLongBreakAfterDraft(): void {
    const result = commitIntegerDraft(
      pomodoroLongBreakAfterInputDraft,
      projectPomodoroLongBreakAfterFocusDraft,
      MIN_RHYTHM_POSITIONS,
      Math.min(MAX_RHYTHM_POSITIONS, CUSTOM_CYCLE_SLOT_MAX),
    );
    pomodoroLongBreakAfterInputDraft = formatPomodoroCycleSlot(result.value);
    if (result.committed) projectPomodoroLongBreakAfterFocusDraft = result.value;
  }

  function formatPomodoroDurationSlot(value: number): string {
    return String(value);
  }

  function formatPomodoroCycleSlot(value: number): string {
    return String(value);
  }

  function sanitizePomodoroNumberDraft(value: string, maxLength: number): string {
    return value.replace(/\D/g, "").slice(0, maxLength);
  }

  function handlePomodoroNumberKeydown(event: KeyboardEvent, commit: () => void, restore: () => void): void {
    if (!event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
      if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        commit();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        restore();
        return;
      }
    }
    panelInputKeydown(event);
  }
</script>

<section class="flex flex-col gap-1.5">
  <ProjectSettingsSectionHeading label={t("projects.settings.defaults")} />
  <div class="flex flex-col gap-1.5">
    <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
      <span class="min-w-0 flex-1 text-[0.866667rem] text-foreground">{t("projects.settings.defaultEventName")}</span>
      <input
        bind:value={projectDefaultEventNameDraft}
        placeholder={projectDefaultEventNamePlaceholder ?? t("common.none")}
        aria-label={t("projects.settings.defaultEventName")}
        class="field w-44 min-w-0 px-2.5 text-left font-medium text-foreground max-[480px]:w-full"
        onpointerdown={moveTextInputCaretToPointer}
      />
    </div>

    <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
      <div class="min-w-0 flex-1 text-[0.866667rem] text-foreground">{t("projects.settings.color")}</div>
      <EventColorPicker
        color={projectColorDraft}
        {theme}
        ariaLabel={t("projects.settings.selectColor")}
        displayLabel
        class="w-44 max-[480px]:w-full"
        onSelect={(color) => {
          projectColorDraft = color;
        }}
      />
    </div>

    <Select
      label={t("projects.settings.defaultDuration")}
      value={durationPreset}
      options={durationPresetOptions}
      onChange={setDurationPreset}
      class="w-44"
    />

    {#if durationPreset === "custom"}
      <div class="flex justify-end px-1 py-1">
        <div class="flex w-44 min-w-0 items-center gap-1.5 max-[480px]:w-full">
          <input
            value={customDurationValue}
            inputmode="decimal"
            aria-label={t("projects.settings.customDurationValue")}
            class="field min-w-0 flex-1 px-2.5 text-left font-medium text-foreground"
            onpointerdown={moveTextInputCaretToPointer}
            oninput={(event) => {
              setCustomDurationValue(event.currentTarget);
            }}
          />
          <Select
            inline
            value={customDurationUnit}
            options={durationUnitOptions}
            onChange={setCustomDurationUnit}
            ariaLabel={t("projects.settings.customDurationUnit")}
            class="w-24 shrink-0 max-[480px]:w-28 max-[480px]:flex-none"
          />
        </div>
      </div>
    {/if}

    <Select
      label={t("projects.settings.defaultPomodoro")}
      value={pomodoroSelectValue}
      options={pomodoroSelectOptions}
      onChange={setPomodoroPreset}
      showSelectedSummary={false}
      showActiveCheck={false}
      alignOptionSummaryEnd
      popoverAlign="end"
      class="w-44"
    />

    {#if projectPomodoroModeDraft === "custom"}
      <div class="flex justify-end px-1 py-1">
        <div class="project-pomodoro-rhythm flex h-7 w-56 min-w-0 items-center justify-end gap-1.5 text-[0.733333rem] text-muted-foreground max-[480px]:w-full max-[480px]:justify-start">
          {#each pomodoroCustomFields as field, fieldIndex}
            <span>{field.compactLabel}</span>
            <label class="contents">
              <input
                type="text"
                inputmode="numeric"
                value={field.value}
                maxlength={field.maxLength}
                aria-label={field.label}
                oninput={(event) => field.setDraft(event.currentTarget.value)}
                onblur={field.commit}
                onkeydown={(event) => handlePomodoroNumberKeydown(event, field.commit, field.restore)}
                class="project-pomodoro-number {field.slotClass} bg-transparent px-0 text-right text-[0.733333rem] text-foreground outline-none"
              />
            </label>
            {#if fieldIndex < pomodoroCustomFields.length - 1}
              <span>/</span>
            {/if}
          {/each}
        </div>
      </div>
    {/if}

    {#if idleDetectionAvailable}
    <SwitchField
      label={t("projects.settings.useGlobalIdleSettings")}
      checked={projectIdleSettingsSourceDraft === "global"}
      onChange={setUseGlobalIdleSettings}
    />

    {#if projectIdleSettingsSourceDraft === "custom"}
      <SwitchField
        label={t("projects.settings.idlePauseDefault")}
        checked={projectIdlePauseEnabledDraft}
        onChange={(checked) => {
          projectIdlePauseEnabledDraft = checked;
        }}
      />

      <Select
        label={t("projects.settings.idleThreshold")}
        value={String(projectIdleThresholdMinutesDraft)}
        options={idleThresholdOptions}
        onChange={setIdleThresholdMinutes}
        class="w-44"
      />
    {/if}
    {/if}

    {#if musicAssignmentsAvailable}
      <div class="flex min-w-0 flex-col gap-1.5">
        {#if musicAssignmentsError}
          <Notice class="px-1 py-1 text-xs" message={musicAssignmentsError} onRetry={onRetryMusicAssignments} />
        {/if}
        <ProjectSettingsPlaylistRows
          assignments={musicAssignments}
          playlists={musicPlaylists}
          onChange={onMusicAssignmentsChange}
          loadingPlaylists={loadingMusicPlaylists}
          disabled={musicAssignmentsDisabled}
        />
      </div>
    {/if}
  </div>
</section>

<style>
  .project-pomodoro-number {
    -moz-appearance: textfield;
    appearance: textfield;
    font-variant-numeric: tabular-nums;
  }

  .project-pomodoro-number::-webkit-inner-spin-button,
  .project-pomodoro-number::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }

  .project-pomodoro-rhythm {
    font-variant-numeric: tabular-nums;
  }

  .duration-summary-slot {
    width: 2ch;
  }

  .cycle-summary-slot {
    width: 1ch;
  }
</style>
