<script lang="ts">
  import type { MaybePromise } from "$lib/utils";
  import Select from "$lib/components/ui/Select.svelte";
  import Trash2 from "@lucide/svelte/icons/trash";
  import { formatDateTime, formatList } from "$lib/i18n/formatters";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectLinkableEvent, ProjectTask } from "$lib/projects/types";
  import ProjectTaskDetailDateField from "./ProjectTaskDetailDateField.svelte";
  import ProjectSettingsSectionHeading from "$lib/components/projects/settings/ProjectSettingsSectionHeading.svelte";


  let {
    task,
    events,
    candidates,
    search,
    searchPending,
    searchError,
    startDate,
    endDate,
    todayDate,
    startPickerOpen,
    endPickerOpen,
    onSearchChange,
    onToggleStartPicker,
    onToggleEndPicker,
    onClearStartDate,
    onClearEndDate,
    onSelectDate,
    onCancelDatePicker,
    onLinkEvent,
    onUnlinkEvent,
  }: {
    task: ProjectTask;
    events: ProjectLinkableEvent[];
    candidates: ProjectLinkableEvent[];
    search: string;
    searchPending: boolean;
    searchError: string | null;
    startDate: string;
    endDate: string;
    todayDate: string;
    startPickerOpen: boolean;
    endPickerOpen: boolean;
    onSearchChange: (value: string) => void;
    onToggleStartPicker: () => void;
    onToggleEndPicker: () => void;
    onClearStartDate: () => void;
    onClearEndDate: () => void;
    onSelectDate: (date: string) => void;
    onCancelDatePicker: () => void;
    onLinkEvent: (task: ProjectTask, event: ProjectLinkableEvent) => MaybePromise;
    onUnlinkEvent: (task: ProjectTask, event: ProjectLinkableEvent) => MaybePromise;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;

  /** Format event times in the active app locale, preserving invalid source text for diagnosis. */
  function eventDateLabel(value: string): string {
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : formatDateTime(localization.locale, date, {
      dateStyle: "medium", timeStyle: "short",
    });
  }

  /** Include the date and existing task links when choosing between similarly named events. */
  function eventOptionSummary(event: ProjectLinkableEvent): string {
    const otherTasks = event.linkedTasks
      .filter((linkedTask) => linkedTask.taskId !== task.id)
      .map((linkedTask) => linkedTask.title);
    const date = eventDateLabel(event.start);
    return otherTasks.length > 0
      ? `${date} · ${t("projects.detail.linkedToTasks", formatList(localization.locale, otherTasks))}`
      : date;
  }
</script>

<section class="task-detail-section grid min-w-0 content-start gap-3">
  <ProjectSettingsSectionHeading label={t("projects.detail.scheduledBlocks")} count={events.length} inlineCount />
  <div class="grid gap-1">
    {#each events as event (event.id)}
      <div class="grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-2 rounded-md bg-muted/30 px-2 py-1.5">
        <div class="min-w-0">
          <span class="block truncate text-[0.8rem]">{event.title || t("calendar.event.noTitle")}</span>
          <span class="block truncate text-[0.733333rem] text-muted-foreground">{eventDateLabel(event.start)}</span>
        </div>
        <span class="text-[0.733333rem] text-muted-foreground">{eventDateLabel(event.end)}</span>
        <button
          type="button"
          class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
          aria-label={t("projects.actions.unlinkScheduledBlock")}
          title={t("projects.actions.unlinkScheduledBlock")}
          onclick={() => { void onUnlinkEvent(task, event); }}
        >
          <Trash2 size={13} strokeWidth={1.75} />
        </button>
      </div>
    {:else}
      <div class="px-1 py-1 text-[0.8rem] text-muted-foreground">
        {t("projects.detail.noScheduledBlocks")}
      </div>
    {/each}
  </div>
  {#if searchError}
    <Notice class="px-1 text-[0.8rem]" message={searchError} />
  {/if}
  <div class="grid gap-1">
    <div class="grid gap-2 min-[760px]:grid-cols-2">
      <ProjectTaskDetailDateField
        label={t("projects.detail.eventLinkStartDate")}
        value={startDate}
        noDateLabel={t("projects.detail.noDate")}
        clearLabel={t("projects.detail.clearDate", t("projects.detail.eventLinkStartDate"))}
        pickerOpen={startPickerOpen}
        selectedDate={startDate || todayDate}
        highlightMode="none"
        onToggle={onToggleStartPicker}
        onClear={onClearStartDate}
        onSelect={onSelectDate}
        onCancel={onCancelDatePicker}
      />
      <ProjectTaskDetailDateField
        label={t("projects.detail.eventLinkEndDate")}
        value={endDate}
        noDateLabel={t("projects.detail.noDate")}
        clearLabel={t("projects.detail.clearDate", t("projects.detail.eventLinkEndDate"))}
        pickerOpen={endPickerOpen}
        selectedDate={endDate || todayDate}
        highlightMode="none"
        onToggle={onToggleEndPicker}
        onClear={onClearEndDate}
        onSelect={onSelectDate}
        onCancel={onCancelDatePicker}
      />
    </div>
    <Select inline appearance="quiet" contentAlign="start" class="w-full" value=""
      triggerLabel={t("projects.detail.linkExistingBlock")} ariaLabel={t("projects.detail.linkExistingBlock")}
      options={searchPending ? [] : candidates.map((event) => ({ value: event.id, label: event.title || t("calendar.event.noTitle"), summary: eventOptionSummary(event) }))}
      searchPlaceholder={t("projects.detail.linkExistingBlockPlaceholder")}
      searchValue={search} onSearchChange={onSearchChange}
      emptyLabel={searchPending ? t("projects.detail.searchingBlocks") : t("projects.detail.noLinkableBlocks")}
      onChange={(value) => {
        const event = candidates.find((candidate) => candidate.id === value);
        if (event) void onLinkEvent(task, event);
      }} />
  </div>
</section>
