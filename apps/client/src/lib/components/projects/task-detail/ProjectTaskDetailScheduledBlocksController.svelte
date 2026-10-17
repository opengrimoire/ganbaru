<script lang="ts">
  import type { MaybePromise } from "$lib/utils";
  import type { CalendarEvent } from "$lib/calendar/types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    eventLinkCandidateEvents,
    linkedEventRowsForTask,
    projectTaskDetailEventLinkDateRange,
  } from "$lib/projects/tasks/detail";
  import type { ProjectLinkableEvent, ProjectTask } from "$lib/projects/types";
  import ProjectTaskDetailScheduledBlocksSection from "./ProjectTaskDetailScheduledBlocksSection.svelte";

  type EventDatePickerTarget = "start" | "end";

  let {
    task,
    taskEventIds,
    allProjectEvents,
    todayDate,
    searchLinkableEvents,
    onLinkEvent,
    onUnlinkEvent,
  }: {
    task: ProjectTask;
    taskEventIds: readonly string[];
    allProjectEvents: readonly CalendarEvent[];
    todayDate: string;
    searchLinkableEvents: (
      projectId: string,
      taskId: string,
      query: string,
      startDate?: string,
      endDate?: string,
      limit?: number,
    ) => Promise<ProjectLinkableEvent[]>;
    onLinkEvent: (task: ProjectTask, event: ProjectLinkableEvent) => MaybePromise<boolean>;
    onUnlinkEvent: (task: ProjectTask, event: ProjectLinkableEvent) => MaybePromise;
  } = $props();

  const { t } = getLocalization();

  let search = $state("");
  let startDate = $state("");
  let endDate = $state("");
  let searchResults = $state<ProjectLinkableEvent[]>([]);
  let searchPending = $state(false);
  let searchError = $state<string | null>(null);
  let datePickerTarget = $state<EventDatePickerTarget | null>(null);
  let searchRunId = 0;

  const events = $derived(linkedEventRowsForTask({
    linkedEventIds: taskEventIds,
    eventLinkResults: searchResults,
    allProjectEvents,
  }));
  const candidates = $derived(eventLinkCandidateEvents({
    taskEventIds,
    eventLinkResults: searchResults,
  }));

  $effect(() => {
    const query = search;
    const startDateDraft = startDate;
    const endDateDraft = endDate;
    const range = projectTaskDetailEventLinkDateRange({
      startDateDraft,
      endDateDraft,
    });
    if (!range.ok) {
      searchRunId++;
      searchPending = false;
      searchError = range.reason === "invalid-range"
        ? t("projects.detail.eventLinkInvalidDateRange")
        : t("projects.detail.invalidDate");
      searchResults = [];
      return;
    }

    const runId = ++searchRunId;
    searchPending = true;
    searchError = null;
    const timeoutId = setTimeout(() => {
      void searchLinkableEvents(task.projectId, task.id, query, range.startDate, range.endDate, 12)
        .then((results) => {
          if (runId !== searchRunId) return;
          searchResults = results;
        })
        .catch((error) => {
          if (runId !== searchRunId) return;
          searchError = t(
            "projects.detail.eventLinkSearchFailed",
            error instanceof Error ? error.message : String(error),
          );
          searchResults = [];
        })
        .finally(() => {
          if (runId === searchRunId) {
            searchPending = false;
          }
        });
    }, query.trim() ? 150 : 0);

    return () => {
      clearTimeout(timeoutId);
    };
  });

  function toggleDatePicker(target: EventDatePickerTarget): void {
    datePickerTarget = datePickerTarget === target ? null : target;
  }

  function clearDate(target: EventDatePickerTarget): void {
    if (target === "start") {
      startDate = "";
    } else {
      endDate = "";
    }
    if (datePickerTarget === target) datePickerTarget = null;
  }

  function selectDate(date: string): void {
    if (datePickerTarget === "start") {
      startDate = date;
    } else if (datePickerTarget === "end") {
      endDate = date;
    }
    datePickerTarget = null;
  }

  async function linkEvent(task: ProjectTask, event: ProjectLinkableEvent): Promise<void> {
    const linked = await onLinkEvent(task, event);
    if (linked) search = "";
  }
</script>

<ProjectTaskDetailScheduledBlocksSection
  {task}
  {events}
  {candidates}
  {search}
  searchPending={searchPending}
  searchError={searchError}
  startDate={startDate}
  endDate={endDate}
  {todayDate}
  startPickerOpen={datePickerTarget === "start"}
  endPickerOpen={datePickerTarget === "end"}
  onSearchChange={(value) => { search = value; }}
  onToggleStartPicker={() => toggleDatePicker("start")}
  onToggleEndPicker={() => toggleDatePicker("end")}
  onClearStartDate={() => clearDate("start")}
  onClearEndDate={() => clearDate("end")}
  onSelectDate={selectDate}
  onCancelDatePicker={() => { datePickerTarget = null; }}
  onLinkEvent={linkEvent}
  {onUnlinkEvent}
/>
