<script lang="ts">
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ProjectTaskDetailDateField from "./ProjectTaskDetailDateField.svelte";

  let {
    milestone,
    onMilestoneChange,
    estimateMinutes,
    startDate,
    startTime,
    dueDate,
    dueTime,
    targetEndDate,
    blockerReason,
    changeReason,
    todayDate,
    startPickerOpen,
    duePickerOpen,
    targetPickerOpen,
    onEstimateMinutesChange,
    onBlockerReasonChange,
    onChangeReasonChange,
    onToggleStartPicker,
    onToggleDuePicker,
    onToggleTargetPicker,
    onClearStartDate,
    onClearDueDate,
    onClearTargetEndDate,
    onSelectDate,
    onStartTimeChange,
    onDueTimeChange,
    onCancelDatePicker,
  }: {
    milestone: boolean;
    onMilestoneChange: (value: boolean) => void;
    estimateMinutes: string;
    startDate: string;
    startTime: string;
    dueDate: string;
    dueTime: string;
    targetEndDate: string;
    blockerReason: string;
    changeReason: string;
    todayDate: string;
    startPickerOpen: boolean;
    duePickerOpen: boolean;
    targetPickerOpen: boolean;
    onEstimateMinutesChange: (value: string) => void;
    onBlockerReasonChange: (value: string) => void;
    onChangeReasonChange: (value: string) => void;
    onToggleStartPicker: () => void;
    onToggleDuePicker: () => void;
    onToggleTargetPicker: () => void;
    onClearStartDate: () => void;
    onClearDueDate: () => void;
    onClearTargetEndDate: () => void;
    onSelectDate: (date: string) => void;
    onStartTimeChange: (time: string | null) => void;
    onDueTimeChange: (time: string | null) => void;
    onCancelDatePicker: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<div class="grid gap-1">
  <ProjectTaskDetailDateField inline
    label={t("projects.detail.startDate")}
    value={startDate}
    time={startTime}
    onTimeChange={onStartTimeChange}
    noDateLabel={t("projects.detail.noDate")}
    clearLabel={t("projects.detail.clearDate", t("projects.detail.startDate"))}
    pickerOpen={startPickerOpen}
    selectedDate={startDate || todayDate}
    rangeStartDate={startDate || undefined}
    rangeEndDate={dueDate || undefined}
    highlightToday={false}
    onToggle={onToggleStartPicker}
    onClear={onClearStartDate}
    onSelect={onSelectDate}
    onCancel={onCancelDatePicker}
  />

  <ProjectTaskDetailDateField inline
    label={t("projects.detail.dueDate")}
    value={dueDate}
    time={dueTime}
    onTimeChange={onDueTimeChange}
    noDateLabel={t("projects.detail.noDate")}
    clearLabel={t("projects.detail.clearDate", t("projects.detail.dueDate"))}
    pickerOpen={duePickerOpen}
    selectedDate={dueDate || todayDate}
    rangeStartDate={startDate || undefined}
    rangeEndDate={dueDate || undefined}
    highlightToday={false}
    onToggle={onToggleDuePicker}
    onClear={onClearDueDate}
    onSelect={onSelectDate}
    onCancel={onCancelDatePicker}
  />

  <ProjectTaskDetailDateField inline
    label={t("projects.detail.targetEndDate")}
    value={targetEndDate}
    noDateLabel={t("projects.detail.noDate")}
    clearLabel={t("projects.detail.clearDate", t("projects.detail.targetEndDate"))}
    pickerOpen={targetPickerOpen}
    selectedDate={targetEndDate || todayDate}
    highlightMode="none"
    onToggle={onToggleTargetPicker}
    onClear={onClearTargetEndDate}
    onSelect={onSelectDate}
    onCancel={onCancelDatePicker}
  />

  <label class="task-property-row">
    <span>{t("projects.detail.estimateMinutes")}</span>
    <input
      value={estimateMinutes}
      inputmode="numeric"
      placeholder={t("common.none")}
      class="min-h-8 min-w-0 rounded-md bg-transparent px-1.5 text-[0.8rem] text-foreground placeholder:text-muted-foreground hover:bg-accent/60"
      oninput={(event) => onEstimateMinutesChange(event.currentTarget.value)}
    />
  </label>
  <label class="task-property-row">
    <span>{t("projects.detail.milestone")}</span>
    <Checkbox checked={milestone} onChange={onMilestoneChange} />
  </label>
  <label class="task-property-row">
    <span>{t("projects.detail.blockerReason")}</span>
    <input
      value={blockerReason}
      placeholder={t("common.none")}
      class="min-h-8 min-w-0 rounded-md bg-transparent px-1.5 text-[0.8rem] text-foreground placeholder:text-muted-foreground hover:bg-accent/60"
      oninput={(event) => onBlockerReasonChange(event.currentTarget.value)}
    />
  </label>
  <label class="task-property-row">
    <span>{t("projects.detail.changeReason")}</span>
    <input
      value={changeReason}
      maxlength="1000"
      placeholder={t("projects.detail.changeReasonPlaceholder")}
      class="min-h-8 min-w-0 rounded-md bg-transparent px-1.5 text-[0.8rem] text-foreground placeholder:text-muted-foreground hover:bg-accent/60"
      oninput={(event) => onChangeReasonChange(event.currentTarget.value)}
    />
  </label>
</div>
