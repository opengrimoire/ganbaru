<script lang="ts">
  import CollectionCell from "$lib/components/collections/CollectionCell.svelte";
  import { Temporal } from "@js-temporal/polyfill";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Clock from "@lucide/svelte/icons/clock";
  import Eraser from "@lucide/svelte/icons/eraser";
  import Link2 from "@lucide/svelte/icons/link-2";
  import Mail from "@lucide/svelte/icons/mail";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import Phone from "@lucide/svelte/icons/phone";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import UserRound from "@lucide/svelte/icons/user-round";
  import X from "@lucide/svelte/icons/x";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import MiniDatePicker from "$lib/components/ui/MiniDatePicker.svelte";
  import TimePicker from "$lib/components/ui/TimePicker.svelte";
  import ProfileAvatar from "$lib/components/profile/ProfileAvatar.svelte";
  import { formatTimeLabel } from "$lib/calendar/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    projectCustomFieldAllowsMultipleOptions,
    projectCustomFieldInputType,
    projectCustomFieldTextInputMode,
    projectCustomFieldTextValue,
    projectCustomFieldUsesOptions,
    projectCustomFieldUsesTextValue,
  } from "$lib/projects/custom-fields";
  import {
    projectPriorityDisplayColor,
    projectPriorityDisplayLabel,
  } from "$lib/projects/display";
  import { customFieldIdFromTaskListColumn } from "$lib/projects/tasks/list-columns";
  import {
    type ProjectCustomField,
    type ProjectCustomFieldOption,
    type ProjectCustomFieldValue,
    type ProjectCustomFieldValueUpdate,
    type ProjectPriority,
    type ProjectPriorityConfig,
    type ProjectStatus,
    type ProjectTask,
    type ProjectTaskListColumn,
  } from "$lib/projects/types";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import PriorityFlagIcon from "$lib/components/projects/PriorityFlagIcon.svelte";
  import ProjectStatusBadge from "$lib/components/projects/ProjectStatusBadge.svelte";
  import { getProjectListTableContext } from "./table-context";
  import { projectListDateLabel, projectListNumberLabel, projectListTimeLabel } from "$lib/projects/list/presentation";

  let {
    column,
    task,
    status,
    statuses,
    priorities,
    statusMenuOpen,
    priorityMenuOpen,
    startDateMenuOpen,
    dueDateMenuOpen,
    projectCustomFields,
    scheduled,
    blockedByCount,
    blocksCount,
    estimateLabel,
    customFieldDisplayValue,
    customFieldOptions,
    customFieldValue,
    customFieldOptionValues,
    onSaveCustomFieldValue,
    onToggleStatusMenu,
    onSetStatus,
    onTogglePriorityMenu,
    onSetPriority,
    onToggleStartDateMenu,
    onCloseStartDateMenu,
    onSetStartDate,
    onClearStartDate,
    onSetStartTime,
    onClearStartTime,
    onToggleDueDateMenu,
    onCloseDueDateMenu,
    onSetDueDate,
    onClearDueDate,
    onSetDueTime,
    onClearDueTime,
  }: {
    column: ProjectTaskListColumn;
    task: ProjectTask;
    status: ProjectStatus | undefined;
    statuses: ProjectStatus[];
    priorities: ProjectPriorityConfig[];
    statusMenuOpen: boolean;
    priorityMenuOpen: boolean;
    startDateMenuOpen: boolean;
    dueDateMenuOpen: boolean;
    projectCustomFields: ProjectCustomField[];
    scheduled: string | null;
    blockedByCount: number;
    blocksCount: number;
    estimateLabel: (minutes: number) => string;
    customFieldDisplayValue: (task: ProjectTask, field: ProjectCustomField) => string | undefined;
    customFieldOptions: (field: ProjectCustomField) => ProjectCustomFieldOption[];
    customFieldValue: (task: ProjectTask, field: ProjectCustomField) => ProjectCustomFieldValue | undefined;
    customFieldOptionValues: (task: ProjectTask, field: ProjectCustomField) => ProjectCustomFieldOption[];
    onSaveCustomFieldValue: (
      task: ProjectTask,
      field: ProjectCustomField,
      value: Omit<ProjectCustomFieldValueUpdate, "taskId" | "fieldId">,
    ) => Promise<void>;
    onToggleStatusMenu: () => void;
    onSetStatus: (status: ProjectStatus) => void;
    onTogglePriorityMenu: () => void;
    onSetPriority: (priority: ProjectPriority) => void;
    onToggleStartDateMenu: () => void;
    onCloseStartDateMenu: () => void;
    onSetStartDate: (startDate: string) => void;
    onClearStartDate: () => void;
    onSetStartTime: (startTime: string) => void;
    onClearStartTime: () => void;
    onToggleDueDateMenu: () => void;
    onCloseDueDateMenu: () => void;
    onSetDueDate: (dueDate: string) => void;
    onClearDueDate: () => void;
    onSetDueTime: (dueTime: string) => void;
    onClearDueTime: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const theme = getTheme();
  const preferences = getPreferences();
  const context = getProjectListTableContext();
  const FLOATING_PANEL_GAP = 6;
  const FLOATING_PANEL_MARGIN = 8;
  const DATE_PICKER_PANEL_WIDTH = FLOATING_WIDTH.sm;
  const DATE_PICKER_PANEL_MIN_HEIGHT = 180;
  const TIME_PICKER_PANEL_WIDTH = 160;
  const TIME_PICKER_PANEL_HEIGHT = 200;
  const TIME_PICKER_PANEL_MIN_HEIGHT = 96;
  const DEFAULT_TIME_PICKER_ANCHOR = "12:00";

  let timePickerOpen = $state(false);
  let dateTriggerEl: HTMLButtonElement | undefined = $state();
  let timeTriggerEl: HTMLButtonElement | undefined = $state();
  let customFieldPanelOpen = $state(false);
  let customFieldRootEl: HTMLDivElement | undefined = $state();
  let customFieldTriggerEl: HTMLButtonElement | undefined = $state();
  let valueSaveError = $state<string | null>(null);
  let numericDraft = $state<string | null>(null);
  let numericSaving = $state(false);

  const todayDate = $derived.by(() => {
    const now = new Date();
    return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  });
  const ownDateMenuOpen = $derived(column === "start" ? startDateMenuOpen : column === "due" ? dueDateMenuOpen : false);

  $effect(() => {
    if (!ownDateMenuOpen) {
      timePickerOpen = false;
    }
  });

  $effect(() => {
    if (!customFieldPanelOpen) return;
    const handlePointerDown = (event: PointerEvent): void => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (!customFieldRootEl?.contains(target)) {
        customFieldPanelOpen = false;
      }
    };
    window.addEventListener("pointerdown", handlePointerDown, true);
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown, true);
    };
  });

  function dateButtonText(dateValue: string | undefined, timeValue: string | undefined, emptyDateLabel: string): string {
    if (!dateValue) return emptyDateLabel;
    const date = projectListDateLabel(dateValue, context?.query.listPresentation.dateFormats[column] ?? "locale", localization.locale, todayDate);
    if (!timeValue || (column !== "start" && column !== "due")) return date;
    const time = projectListTimeLabel(timeValue, context?.query.listPresentation.timeFormats[column] ?? "locale", localization.locale);
    return time ? `${date} ${time}` : date;
  }

  function emptyCustomFieldPayload(): Omit<ProjectCustomFieldValueUpdate, "taskId" | "fieldId"> {
    return {
      textValue: null,
      numberValue: null,
      dateValue: null,
      checkboxValue: null,
      optionIds: [],
    };
  }

  function customFieldInputValue(field: ProjectCustomField, value: ProjectCustomFieldValue | undefined): string {
    if (projectCustomFieldUsesTextValue(field.fieldType)) return projectCustomFieldTextValue(value);
    if (field.fieldType === "number") return value?.numberValue === undefined ? "" : String(value.numberValue);
    if (field.fieldType === "date") return value?.dateValue ?? "";
    return "";
  }

  async function saveCustomFieldText(
    field: ProjectCustomField,
    nextValue: string,
    input?: HTMLInputElement | HTMLTextAreaElement,
  ): Promise<void> {
    const value = customFieldValue(task, field);
    const previousValue = projectCustomFieldTextValue(value);
    const text = nextValue.trim();
    if (text === previousValue) return;
    input?.setCustomValidity("");
    await onSaveCustomFieldValue(task, field, {
      ...emptyCustomFieldPayload(),
      textValue: text || null,
    });
  }

  async function saveCustomFieldNumber(
    field: ProjectCustomField,
    nextValue: string,
    input?: HTMLInputElement,
  ): Promise<void> {
    const value = customFieldValue(task, field);
    const current = value?.numberValue;
    const text = nextValue.trim();
    const nextNumber = text ? Number(text) : null;
    if (text && !Number.isFinite(nextNumber)) {
      input?.setCustomValidity(t("projects.customFields.invalidNumber"));
      input?.reportValidity();
      return;
    }
    input?.setCustomValidity("");
    if ((current === undefined && nextNumber === null) || current === nextNumber) return;
    numericSaving = true;
    await onSaveCustomFieldValue(task, field, {
      ...emptyCustomFieldPayload(),
      numberValue: nextNumber,
    });
  }

  /** Keep an unsuccessful numeric draft visible and format only a committed canonical value. */
  async function saveFormattedNumber(field: ProjectCustomField, input: HTMLInputElement): Promise<void> {
    if (numericSaving) return;
    numericDraft = input.value;
    valueSaveError = null;
    try {
      await saveCustomFieldNumber(field, numericDraft, input);
      if (!input.validity.valid) return;
      numericDraft = null;
    } catch (error: unknown) {
      valueSaveError = t("projects.customFields.valueSaveFailed", error instanceof Error ? error.message : String(error));
    } finally {
      numericSaving = false;
    }
  }

  async function saveCustomFieldDate(field: ProjectCustomField, nextValue: string | null): Promise<void> {
    const value = customFieldValue(task, field);
    const current = value?.dateValue ?? "";
    let dateValue: string | null = null;
    if (nextValue?.trim()) {
      dateValue = Temporal.PlainDate.from(nextValue.trim()).toString();
    }
    if (current === (dateValue ?? "")) return;
    await onSaveCustomFieldValue(task, field, {
      ...emptyCustomFieldPayload(),
      dateValue,
    });
  }

  async function saveCustomFieldCheckbox(field: ProjectCustomField, checked: boolean): Promise<void> {
    if (customFieldValue(task, field)?.checkboxValue === checked) return;
    await onSaveCustomFieldValue(task, field, {
      ...emptyCustomFieldPayload(),
      checkboxValue: checked,
    });
  }

  async function saveCustomFieldOptions(
    field: ProjectCustomField,
    optionIds: string[],
  ): Promise<void> {
    await onSaveCustomFieldValue(task, field, {
      ...emptyCustomFieldPayload(),
      optionIds,
    });
  }

  function handleCustomFieldInputKeydown(
    event: KeyboardEvent & { currentTarget: HTMLInputElement | HTMLTextAreaElement },
    field: ProjectCustomField,
    value: ProjectCustomFieldValue | undefined,
  ): void {
    if (event.key === "Enter") {
      if (event.currentTarget instanceof HTMLTextAreaElement && !event.ctrlKey && !event.metaKey) return;
      event.preventDefault();
      event.currentTarget.blur();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.currentTarget.value = customFieldInputValue(field, value);
      event.currentTarget.blur();
    }
  }

  function selectedCustomFieldOptionIds(field: ProjectCustomField): string[] {
    return customFieldOptionValues(task, field).map((option) => option.id);
  }

  function customFieldOptionLabel(field: ProjectCustomField): string {
    const labels = customFieldOptionValues(task, field).map((option) => option.name);
    return labels.length > 0 ? labels.join(", ") : t("projects.customFields.emptyValue");
  }

  function customFieldIcon(field: ProjectCustomField) {
    if (field.fieldType === "url") return Link2;
    if (field.fieldType === "phone") return Phone;
    if (field.fieldType === "email") return Mail;
    if (field.fieldType === "person") return UserRound;
    if (field.fieldType === "files") return Paperclip;
    return undefined;
  }

  function positionPanel(
    node: HTMLElement,
    anchor: HTMLElement | undefined,
    options: {
      fallbackWidth: number;
      minHeight: number;
      preferredHeight?: number;
      applyNodeMaxHeight?: boolean;
      maxHeightProperty?: string;
    },
  ): void {
    if (!anchor) return;
    const triggerRect = anchor.getBoundingClientRect();
    const panelWidth = node.offsetWidth || options.fallbackWidth;
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;
    const usableViewportHeight = Math.max(0, viewportHeight - FLOATING_PANEL_MARGIN * 2);
    const preferredHeight = Math.min(
      options.preferredHeight ?? Math.max(node.scrollHeight, node.offsetHeight, options.minHeight),
      usableViewportHeight,
    );
    const minimumHeight = Math.min(options.minHeight, usableViewportHeight);
    const left = Math.max(
      FLOATING_PANEL_MARGIN,
      Math.min(
        triggerRect.left,
        viewportWidth - panelWidth - FLOATING_PANEL_MARGIN,
      ),
    );
    const belowTop = triggerRect.bottom + FLOATING_PANEL_GAP;
    const aboveBottom = triggerRect.top - FLOATING_PANEL_GAP;
    const belowSpace = Math.max(0, viewportHeight - FLOATING_PANEL_MARGIN - belowTop);
    const aboveSpace = Math.max(0, aboveBottom - FLOATING_PANEL_MARGIN);
    const preferBelow = belowSpace >= preferredHeight || belowSpace >= aboveSpace;
    const availableHeight = preferBelow ? belowSpace : aboveSpace;
    const maxHeight = Math.max(
      minimumHeight,
      Math.min(preferredHeight, availableHeight || usableViewportHeight),
    );
    const unclampedTop = preferBelow
      ? belowTop
      : aboveBottom - maxHeight;
    const top = Math.max(
      FLOATING_PANEL_MARGIN,
      Math.min(
        unclampedTop,
        viewportHeight - FLOATING_PANEL_MARGIN - maxHeight,
      ),
    );

    node.style.left = `${Math.round(left)}px`;
    node.style.top = `${Math.round(top)}px`;
    if (options.applyNodeMaxHeight ?? true) {
      node.style.maxHeight = `${Math.round(maxHeight)}px`;
      node.style.overflowY = "auto";
    }
    if (options.maxHeightProperty) {
      node.style.setProperty(options.maxHeightProperty, `${Math.round(maxHeight)}px`);
    }
  }

  function positionDatePickerPanel(node: HTMLElement) {
    function updatePosition(): void {
      positionPanel(node, dateTriggerEl, {
        fallbackWidth: DATE_PICKER_PANEL_WIDTH,
        minHeight: DATE_PICKER_PANEL_MIN_HEIGHT,
      });
    }

    const frame = requestAnimationFrame(updatePosition);
    const resizeObserver = new ResizeObserver(updatePosition);
    resizeObserver.observe(node);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);

    return {
      destroy() {
        cancelAnimationFrame(frame);
        resizeObserver.disconnect();
        window.removeEventListener("resize", updatePosition);
        window.removeEventListener("scroll", updatePosition, true);
      },
    };
  }

  function positionTimePickerPanel(node: HTMLElement) {
    function updatePosition(): void {
      positionPanel(node, timeTriggerEl, {
        fallbackWidth: TIME_PICKER_PANEL_WIDTH,
        minHeight: TIME_PICKER_PANEL_MIN_HEIGHT,
        preferredHeight: TIME_PICKER_PANEL_HEIGHT,
        applyNodeMaxHeight: false,
        maxHeightProperty: "--project-list-time-picker-max-height",
      });
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

  function positionCustomFieldPanel(node: HTMLElement) {
    function updatePosition(): void {
      positionPanel(node, customFieldTriggerEl, {
        fallbackWidth: Math.max(180, customFieldTriggerEl?.offsetWidth ?? 0),
        minHeight: 48,
      });
    }

    const frame = requestAnimationFrame(updatePosition);
    const resizeObserver = new ResizeObserver(updatePosition);
    resizeObserver.observe(node);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);

    return {
      destroy() {
        cancelAnimationFrame(frame);
        resizeObserver.disconnect();
        window.removeEventListener("resize", updatePosition);
        window.removeEventListener("scroll", updatePosition, true);
      },
    };
  }
</script>

<CollectionCell
  class={context?.cellClass(column)} style={context?.cellStyle(column)}
  data-list-status-menu-root={column === "status" ? "true" : undefined}
  data-list-priority-menu-root={column === "priority" ? "true" : undefined}
  data-list-date-menu-root={column === "start" || column === "due" ? "true" : undefined}
>
  {#if column === "status"}
    <button
      type="button"
      data-collection-cell-primary class="absolute inset-0 z-0 cursor-pointer disabled:cursor-not-allowed"
      disabled={Boolean(task.archivedAt)}
      aria-label={status?.name ?? t("projects.list.status")}
      data-app-tooltip-disabled="true"
      aria-haspopup="menu"
      aria-expanded={statusMenuOpen}
      onclick={onToggleStatusMenu}
    ></button>
    <div class="pointer-events-none relative z-10 min-w-0 max-w-full">
      <ProjectStatusBadge
        {status}
        theme={theme.current}
        label={status?.name ?? t("projects.list.status")}
        class="text-[0.733333rem]"
      />
    </div>
      {#if statusMenuOpen}
        <div
          class="surface-floating surface-floating-body absolute left-0 top-full z-30 mt-1 w-44"
          role="menu"
        >
          {#each statuses as nextStatus (nextStatus.id)}
            <button
              type="button"
              class="menu-item justify-between"
              role="menuitemradio"
              aria-checked={task.statusId === nextStatus.id}
              onclick={() => onSetStatus(nextStatus)}
            >
              <ProjectStatusBadge
                status={nextStatus}
                theme={theme.current}
                label={nextStatus.name}
                class="text-[0.733333rem]"
              />
              {#if task.statusId === nextStatus.id}
                <Check size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
              {/if}
            </button>
          {/each}
        </div>
      {/if}
  {:else if column === "priority"}
    {@const selectedPriorityLabel = projectPriorityDisplayLabel(task.priority, priorities, t)}
    {@const selectedPriorityColor = projectPriorityDisplayColor(task.priority, priorities)}
    <button
      type="button"
      data-collection-cell-primary class="absolute inset-0 z-0 cursor-pointer disabled:cursor-not-allowed"
      disabled={Boolean(task.archivedAt)}
      aria-label={selectedPriorityLabel}
      data-app-tooltip-disabled="true"
      aria-haspopup="menu"
      aria-expanded={priorityMenuOpen}
      onclick={onTogglePriorityMenu}
    ></button>
    <div class="pointer-events-none relative z-10 min-w-0 max-w-full">
      <span class="flex min-w-0 items-center gap-1.5 text-foreground">
        <PriorityFlagIcon color={selectedPriorityColor} theme={theme.current} size={13} class="shrink-0" />
        <span class="min-w-0 truncate">{selectedPriorityLabel}</span>
      </span>
    </div>
      {#if priorityMenuOpen}
        <div
          class="surface-floating surface-floating-body absolute left-0 top-full z-30 mt-1 w-44"
          role="menu"
        >
          {#each priorities as priority (priority.id)}
            <button
              type="button"
              class="menu-item justify-between"
              role="menuitemradio"
              aria-checked={task.priority === priority.id}
              onclick={() => onSetPriority(priority.id)}
            >
              <span class="flex min-w-0 items-center gap-1.5 text-foreground">
                <PriorityFlagIcon color={priority.color} theme={theme.current} size={13} class="shrink-0" />
                <span class="min-w-0 truncate">{priority.name}</span>
              </span>
              {#if task.priority === priority.id}
                <Check size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
              {/if}
            </button>
          {/each}
        </div>
      {/if}
  {:else if column === "estimate"}
    {#if task.estimateMinutes !== undefined}
      <span class="truncate rounded border border-border bg-background px-1.5 py-0.5 text-[0.733333rem] text-muted-foreground">
        {estimateLabel(task.estimateMinutes)}
      </span>
    {/if}
  {:else if column === "assignee" || column === "reviewer"}
    {@const personName = preferences.profileDisplayName || t("projects.people.you")}
    <span title={personName}>
      <ProfileAvatar displayName={personName} imagePath={preferences.profileAvatarImagePath} crop={preferences.profileImageCrop} color={preferences.profileColor} size={24} />
    </span>
  {:else if column === "start" || column === "due"}
    {@const dateValue = column === "start" ? task.startDate : task.dueDate}
    {@const timeValue = column === "start" ? task.startTime : task.dueTime}
    {@const dateMenuOpen = column === "start" ? startDateMenuOpen : dueDateMenuOpen}
    {@const dateLabel = column === "start" ? t("projects.columns.start") : t("projects.columns.due")}
    {@const emptyDateLabel = column === "start" ? t("projects.detail.noDate") : t("projects.filters.noDueDate")}
    {@const datePickerAnchor = dateValue || (column === "start" ? task.dueDate : task.startDate) || todayDate}
      <button
        bind:this={dateTriggerEl}
        type="button"
        data-collection-cell-primary class="absolute inset-0 z-0 cursor-pointer disabled:cursor-not-allowed"
        disabled={Boolean(task.archivedAt)}
        aria-label={dateButtonText(dateValue, timeValue, emptyDateLabel)}
        data-app-tooltip-disabled="true"
        aria-haspopup="dialog"
        aria-expanded={dateMenuOpen}
        onclick={() => {
          if (column === "start") onToggleStartDateMenu();
          else onToggleDueDateMenu();
        }}
      ></button>
      <span class={cn("pointer-events-none relative z-10 block min-w-0 truncate", dateValue ? "text-foreground" : "text-muted-foreground")}>
        {dateButtonText(dateValue, timeValue, emptyDateLabel)}
      </span>
      {#if dateMenuOpen}
        <div
          class="surface-floating fixed z-30 w-floating-sm p-2"
          role="dialog"
          aria-label={dateLabel}
          use:positionDatePickerPanel
        >
          <MiniDatePicker
            selectedDate={datePickerAnchor}
            rangeStartDate={task.startDate}
            rangeEndDate={task.dueDate}
            small
            highlightToday={false}
            activeHighlight="primary"
            onSelect={(dateStr) => {
              if (column === "start") onSetStartDate(dateStr);
              else onSetDueDate(dateStr);
            }}
            onCancel={() => {
              if (column === "start") onCloseStartDateMenu();
              else onCloseDueDateMenu();
            }}
          />
          {#if dateValue}
            <div class="mt-2 border-t border-border/70 pt-2">
              <div class="flex items-center gap-1">
                <button
                  bind:this={timeTriggerEl}
                  type="button"
                  class="menu-item min-w-0 flex-1 text-muted-foreground hover:text-foreground"
                  aria-haspopup="dialog"
                  aria-expanded={timePickerOpen}
                  onclick={() => {
                    timePickerOpen = !timePickerOpen;
                  }}
                >
                  <Clock size={13} strokeWidth={1.75} class="shrink-0" />
                  <span class="truncate">
                    {timeValue ? formatTimeLabel(timeValue, preferences.calendarTimeFormat) : t("projects.columns.selectHour")}
                  </span>
                </button>
                {#if timeValue}
                  <button
                    type="button"
                    class="flex size-(--panel-row-height) shrink-0 items-center justify-center rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground"
                    aria-label={t("projects.columns.clearHour")}
                    data-app-tooltip-disabled="true"
                    onclick={() => {
                      if (column === "start") onClearStartTime();
                      else onClearDueTime();
                      timePickerOpen = false;
                    }}
                  >
                    <Eraser size={13} strokeWidth={1.75} />
                  </button>
                {/if}
                <button
                  type="button"
                  class="flex size-(--panel-row-height) shrink-0 items-center justify-center rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground"
                  aria-label={t("projects.detail.clearDate", dateLabel)}
                  data-app-tooltip-disabled="true"
                  onclick={() => {
                    if (column === "start") onClearStartDate();
                    else onClearDueDate();
                    timePickerOpen = false;
                  }}
                >
                  <Trash2 size={13} strokeWidth={1.75} />
                </button>
              </div>
              {#if timePickerOpen}
                <div
                  class="project-list-time-panel surface-floating fixed z-40 w-40 overflow-hidden"
                  role="dialog"
                  aria-label={`${dateLabel} ${t("projects.columns.selectHour")}`}
                  use:positionTimePickerPanel
                >
                  <TimePicker
                    currentTime={timeValue ?? ""}
                    activeTime={timeValue ?? DEFAULT_TIME_PICKER_ANCHOR}
                    scrollTime={timeValue ?? DEFAULT_TIME_PICKER_ANCHOR}
                    emphasizedTime={timeValue ? undefined : null}
                    focusOnOpen
                    onSelect={(time) => {
                      if (column === "start") onSetStartTime(time);
                      else onSetDueTime(time);
                      timePickerOpen = false;
                    }}
                    onCancel={() => {
                      timePickerOpen = false;
                    }}
                  />
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
  {:else if column === "scheduled"}
    {#if scheduled}
      <span class="truncate rounded border border-sky-500/40 bg-sky-500/10 px-1.5 py-0.5 text-[0.733333rem] text-sky-700 dark:text-sky-300">
        {scheduled}
      </span>
    {/if}
  {:else if column === "dependencies"}
    <div class="flex min-w-0 flex-wrap gap-1">
      {#if blockedByCount > 0}
        <span class="rounded border border-destructive/40 bg-destructive/10 px-1.5 py-0.5 text-[0.733333rem] text-destructive">
          {t("projects.list.blockedBy", blockedByCount)}
        </span>
      {/if}
      {#if blocksCount > 0}
        <span class="rounded border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-[0.733333rem] text-amber-700 dark:text-amber-300">
          {t("projects.list.blocks", blocksCount)}
        </span>
      {/if}
    </div>
  {:else}
    {@const customFieldId = customFieldIdFromTaskListColumn(column)}
    {@const customField = customFieldId ? projectCustomFields.find((field) => field.id === customFieldId) : undefined}
    {#if customField}
      {@const customValue = customFieldValue(task, customField)}
      {@const customDisplayValue = customFieldDisplayValue(task, customField)}
      {#if projectCustomFieldUsesTextValue(customField.fieldType)}
        {@const FieldIcon = customFieldIcon(customField)}
        <div class="relative z-10 flex min-w-0 flex-1 items-center gap-1.5">
          {#if FieldIcon}
            <FieldIcon size={13} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
          {/if}
          {#if context?.query.listPresentation.wrappedColumns.includes(column)}
            <textarea data-collection-cell-primary
              value={customFieldInputValue(customField, customValue)}
              class="min-h-9 min-w-0 flex-1 resize-y rounded border border-transparent bg-transparent px-1 py-1 outline-none"
              aria-label={customField.name}
              placeholder={t("projects.customFields.emptyValue")}
              disabled={Boolean(task.archivedAt)}
              onblur={(event) => { void saveCustomFieldText(customField, event.currentTarget.value, event.currentTarget); }}
              onkeydown={(event) => handleCustomFieldInputKeydown(event, customField, customValue)}
            ></textarea>
          {:else}<input data-collection-cell-primary
            type={projectCustomFieldInputType(customField.fieldType)}
            inputmode={projectCustomFieldTextInputMode(customField.fieldType)}
            value={customFieldInputValue(customField, customValue)}
            class="h-7 min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-1 text-foreground outline-none transition-colors placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-60"
            placeholder={t("projects.customFields.emptyValue")}
            title={customDisplayValue}
            disabled={Boolean(task.archivedAt)}
            onblur={(event) => { void saveCustomFieldText(customField, event.currentTarget.value, event.currentTarget); }}
            onkeydown={(event) => handleCustomFieldInputKeydown(event, customField, customValue)}
          />
          {/if}
        </div>
      {:else if customField.fieldType === "number"}
        <input data-collection-cell-primary
          type="text"
          inputmode="decimal"
          value={numericDraft ?? (customValue?.numberValue === undefined ? "" : projectListNumberLabel(customValue.numberValue, context?.query.listPresentation.numberFormats[column] ?? "number", localization.locale))}
          onfocus={() => { numericDraft ??= customFieldInputValue(customField, customValue); }}
          oninput={(event) => {
            numericDraft = event.currentTarget.value;
            event.currentTarget.setCustomValidity("");
            valueSaveError = null;
          }}
          class="relative z-10 h-7 min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-1 text-foreground outline-none transition-colors placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-60"
          placeholder={t("projects.customFields.emptyValue")}
          title={customDisplayValue}
          disabled={Boolean(task.archivedAt) || numericSaving}
          aria-busy={numericSaving}
          onblur={(event) => { void saveFormattedNumber(customField, event.currentTarget); }}
          onkeydown={(event) => handleCustomFieldInputKeydown(event, customField, customValue)}
        />
        {#if valueSaveError}<span role="alert" class="text-xs text-destructive">{valueSaveError}</span>{/if}
      {:else if customField.fieldType === "checkbox"}
        {@const checked = customValue?.checkboxValue ?? false}
        <label class="flex items-center" data-collection-cell-primary>
          <Checkbox
            {checked}
            label={customField.name}
            disabled={Boolean(task.archivedAt)}
            onChange={(next) => { void saveCustomFieldCheckbox(customField, next); }}
          />
        </label>
      {:else if customField.fieldType === "date"}
        <div bind:this={customFieldRootEl} class="relative z-10 flex min-w-0 flex-1 items-center gap-1">
          <button
            bind:this={customFieldTriggerEl} data-collection-cell-primary
            type="button"
            class={cn(
              "flex h-7 min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded-md px-1 text-left disabled:cursor-not-allowed disabled:opacity-60",
              customValue?.dateValue ? "text-foreground" : "text-muted-foreground",
            )}
            aria-haspopup="dialog"
            aria-expanded={customFieldPanelOpen}
            disabled={Boolean(task.archivedAt)}
            onclick={() => { customFieldPanelOpen = !customFieldPanelOpen; }}
          >
            <CalendarDays size={13} strokeWidth={1.75} class="shrink-0" />
            <span class="min-w-0 truncate">{customValue?.dateValue ? projectListDateLabel(customValue.dateValue, context?.query.listPresentation.dateFormats[column] ?? "locale", localization.locale, todayDate) : t("projects.customFields.emptyValue")}</span>
          </button>
          {#if customValue?.dateValue}
            <button
              type="button"
              class="relative z-10 flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-60"
              aria-label={t("projects.detail.clearDate", customField.name)}
              disabled={Boolean(task.archivedAt)}
              onclick={() => { void saveCustomFieldDate(customField, null); }}
            >
              <X size={13} strokeWidth={1.75} />
            </button>
          {/if}
          {#if customFieldPanelOpen}
            <div
              class="surface-floating fixed z-40 w-floating-sm p-2"
              role="dialog"
              aria-label={customField.name}
              use:positionCustomFieldPanel
            >
              <MiniDatePicker
                selectedDate={customValue?.dateValue ?? todayDate}
                small
                highlightMode="none"
                activeHighlight="primary"
                onSelect={(dateStr) => {
                  void saveCustomFieldDate(customField, dateStr);
                  customFieldPanelOpen = false;
                }}
                onCancel={() => { customFieldPanelOpen = false; }}
              />
            </div>
          {/if}
        </div>
      {:else if projectCustomFieldUsesOptions(customField.fieldType)}
        {@const selectedOptionIds = selectedCustomFieldOptionIds(customField)}
        {@const multiple = projectCustomFieldAllowsMultipleOptions(customField.fieldType)}
        <div bind:this={customFieldRootEl} class="relative z-10 min-w-0 flex-1">
          <button
            bind:this={customFieldTriggerEl} data-collection-cell-primary
            type="button"
            class={cn(
              "flex h-7 min-w-0 max-w-full cursor-pointer items-center justify-between gap-2 rounded-md px-1.5 text-left disabled:cursor-not-allowed disabled:opacity-60",
              selectedOptionIds.length > 0 ? "text-foreground" : "text-muted-foreground",
            )}
            aria-haspopup="menu"
            aria-expanded={customFieldPanelOpen}
            disabled={Boolean(task.archivedAt)}
            title={customDisplayValue}
            onclick={() => { customFieldPanelOpen = !customFieldPanelOpen; }}
          >
            <span class="min-w-0 truncate">{customFieldOptionLabel(customField)}</span>
            <ChevronDown size={13} strokeWidth={1.75} class="shrink-0" />
          </button>
          {#if customFieldPanelOpen}
            <div
              class="surface-floating fixed z-40 flex min-w-44 flex-col overflow-hidden"
              role="menu"
              aria-label={customField.name}
              use:positionCustomFieldPanel
            >
              <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 overflow-y-auto">
                {#if selectedOptionIds.length > 0}
                  <button
                    type="button"
                    class="menu-item text-muted-foreground"
                    role="menuitem"
                    onclick={() => {
                      void saveCustomFieldOptions(customField, []);
                      if (!multiple) customFieldPanelOpen = false;
                    }}
                  >
                    <Eraser size={13} strokeWidth={1.75} class="shrink-0" />
                    <span class="truncate">{t("projects.customFields.clearValue")}</span>
                  </button>
                {/if}
                {#each customFieldOptions(customField) as option (option.id)}
                  {@const optionSelected = selectedOptionIds.includes(option.id)}
                  <button
                    type="button"
                    class="menu-item justify-between"
                    role={multiple ? "menuitemcheckbox" : "menuitemradio"}
                    aria-checked={optionSelected}
                    onclick={() => {
                      if (multiple) {
                        void saveCustomFieldOptions(
                          customField,
                          optionSelected
                            ? selectedOptionIds.filter((optionId) => optionId !== option.id)
                            : [...selectedOptionIds, option.id],
                        );
                      } else {
                        void saveCustomFieldOptions(customField, [option.id]);
                        customFieldPanelOpen = false;
                      }
                    }}
                  >
                    <span class="min-w-0 truncate">{option.name}</span>
                    {#if optionSelected}
                      <Check size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
                    {/if}
                  </button>
                {:else}
                  <div class="px-2 py-2 text-muted-foreground">{t("projects.customFields.noOptions")}</div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  {/if}
</CollectionCell>

<style>
  :global(.project-list-time-panel .time-picker-scroll) {
    max-height: var(--project-list-time-picker-max-height, 12.5rem);
  }
</style>
