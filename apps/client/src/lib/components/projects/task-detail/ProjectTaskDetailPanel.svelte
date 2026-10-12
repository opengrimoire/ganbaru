<script lang="ts">
  import { Temporal } from "@js-temporal/polyfill";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import {
    selectDateRangeEnd,
    selectDateRangeStart,
  } from "$lib/calendar/date-range-selection";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import type {
    ProjectChecklistItem,
    ProjectCustomField,
    ProjectCustomFieldOption,
    ProjectTag,
    ProjectLinkableEvent,
    ProjectPriority,
    ProjectStatus,
    ProjectTask,
    ProjectTaskType,
  } from "$lib/projects/types";
  import type { ProjectTaskUpdatePatch } from "$lib/projects/snapshot/update-payloads";
  import { getCalendar } from "$lib/stores/calendar.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import type { ProjectTaskModalLayout } from "$lib/projects/toolbar";
  import {
    canCreateTag as canCreateTaskTag,
    dependencyCandidateTasks as buildDependencyCandidateTasks,
    parentTaskCandidateTasks as buildParentTaskCandidateTasks,
    projectTaskDetailCustomFieldDirty,
    projectTaskDetailCustomFieldDrafts,
    projectTaskDetailMergeSavedCustomFieldDraft,
    projectTaskDetailCustomFieldRawDraft,
    projectTaskDetailCustomFieldSaveDraft,
    projectTaskDetailDraftDirty,
    projectTaskDetailDraftFromTask,
    projectTaskDetailDraftPatch,
    projectTaskDetailDraftsEqual,
    projectTaskNewDetailDraft,
    projectTaskNewDetailDraftHasExtras,
    tagCandidateTags as buildTagCandidateTags,
    type ProjectTaskDetailDraft,
    type ProjectTaskDetailCustomFieldDrafts,
  } from "$lib/projects/tasks/detail";
  import {
    applyProjectTaskDraftRelations,
    emptyProjectTaskDraftRelations,
    moveProjectTaskDraftEntry,
    projectTaskDraftChecklistItems,
    projectTaskDraftDependencies,
    projectTaskDraftHasTagName,
    projectTaskDraftPreview,
    projectTaskDraftRelationsChanged,
    projectTaskDraftSubtaskTasks,
    projectTaskDraftTags,
    removeProjectTaskDraftTag,
    type ProjectTaskDraftCustomFieldValue,
    type ProjectTaskDraftRelations,
    type ProjectTaskDraftRelationWriter,
  } from "$lib/projects/tasks/draft-relations";
  import ProjectTaskDetailChecklistSection from "./ProjectTaskDetailChecklistSection.svelte";
  import ProjectTaskDetailCustomFieldsSection from "./ProjectTaskDetailCustomFieldsSection.svelte";
  import ProjectTaskDetailDescriptionSection from "./ProjectTaskDetailDescriptionSection.svelte";
  import ProjectTaskDetailDependenciesSection from "./ProjectTaskDetailDependenciesSection.svelte";
  import ProjectTaskDetailFooter from "./ProjectTaskDetailFooter.svelte";
  import ProjectTaskDetailHeader from "./ProjectTaskDetailHeader.svelte";
  import ProjectTaskDetailHistorySection from "./ProjectTaskDetailHistorySection.svelte";
  import ProjectTaskDetailParentSection from "./ProjectTaskDetailParentSection.svelte";
  import ProjectTaskDetailPlanningSection from "./ProjectTaskDetailPlanningSection.svelte";
  import ProjectTaskDetailPropertiesSection from "./ProjectTaskDetailPropertiesSection.svelte";
  import ProjectTaskDetailScheduledBlocksController from "./ProjectTaskDetailScheduledBlocksController.svelte";
  import ProjectTaskDetailSubtasksSection from "./ProjectTaskDetailSubtasksSection.svelte";
  import ProjectTaskDetailTagsSection from "./ProjectTaskDetailTagsSection.svelte";

  let {
    taskId,
    layout = "modal",
    showArchivedTasks,
    showInactiveSections,
    onClose,
    onOpenTask,
    onShowArchivedTasks,
    draftProjectId = null,
    onCreated,
  }: {
    /** Existing task to edit, or null while composing a new task draft. */
    taskId: string | null;
    layout?: ProjectTaskModalLayout;
    showArchivedTasks: boolean;
    showInactiveSections: boolean;
    onClose: () => void;
    onOpenTask: (taskId: string) => void;
    onShowArchivedTasks: () => void;
    /** Project that receives the new task when no task ID is given. */
    draftProjectId?: string | null;
    /** Called after a draft is created so the parent can show the persisted task. */
    onCreated?: (task: ProjectTask) => void;
  } = $props();

  const projects = getProjects();
  const calendar = getCalendar();
  const theme = getTheme();
  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "system.android-back",
  );

  let detailDraftTaskId = $state<string | null>(null);
  let detailDialog = $state<HTMLDivElement | null>(null);
  let detailDraftUpdatedAt = $state<string | null>(null);
  let detailTitle = $state("");
  let detailDescription = $state("");
  let detailSectionId = $state("");
  let detailStatusId = $state("");
  let detailPriority = $state<ProjectPriority>("normal");
  let detailTaskType = $state<ProjectTaskType>("task");
  let detailEstimateMinutes = $state("");
  let detailStartDate = $state("");
  let detailStartTime = $state("");
  let detailDueDate = $state("");
  let detailDueTime = $state("");
  let detailTargetEndDate = $state("");
  let detailBlockerReason = $state("");
  let detailChangeReason = $state("");
  let detailMilestone = $state(false);
  let detailSaving = $state(false);
  let detailError = $state<string | null>(null);
  let detailScrollContainer: HTMLDivElement | undefined = $state();
  let discardCloseConfirmOpen = $state(false);
  let datePickerTarget: DetailDateTarget | null = $state(null);
  let customFieldDatePickerTarget = $state<string | null>(null);
  let pendingTaskOpenId = $state<string | null>(null);
  let subtaskDraft = $state("");
  let checklistDraft = $state("");
  let tagDraft = $state("");
  let checklistTitleDrafts = $state<Record<string, string>>({});
  let customFieldTextDrafts = $state<Record<string, string>>({});
  let customFieldNumberDrafts = $state<Record<string, string>>({});
  let customFieldDateDrafts = $state<Record<string, string>>({});
  let customFieldCheckboxDrafts = $state<Record<string, boolean>>({});
  let customFieldSelectDrafts = $state<Record<string, string>>({});
  let customFieldMultiDrafts = $state<Record<string, string[]>>({});
  const customFieldSaveGenerations = new Map<string, number>();
  const customFieldSaveQueues = new Map<string, Promise<void>>();
  let dependencySearch = $state("");
  let parentTaskSearch = $state("");

  type DetailDateTarget = "start" | "due" | "target";

  let newTaskInitialDraft = $state<ProjectTaskDetailDraft | null>(null);
  let draftRelations = $state<ProjectTaskDraftRelations>(emptyProjectTaskDraftRelations());
  let draftTaskId = $state(crypto.randomUUID());
  let draftStartedAt = $state(Temporal.Now.instant().toString());
  /** Error to show once the task created from a draft is loaded for editing. */
  let createdTaskError = $state<{ taskId: string; message: string } | null>(null);

  const selectedTask = $derived(taskId ? projects.taskById(taskId) : undefined);
  const draftMode = $derived(!taskId && draftProjectId !== null);
  const draftTask = $derived(
    draftMode && draftProjectId
      ? projectTaskDraftPreview({
          id: draftTaskId,
          projectId: draftProjectId,
          draft: currentDetailDraft(),
          parentTaskId: draftRelations.parentTaskId,
          timestamp: draftStartedAt,
        })
      : undefined,
  );
  /** Persisted task, or the stand-in for an unsaved draft, shown by the detail sections. */
  const sectionTask = $derived(selectedTask ?? draftTask);
  const selectedProjectId = $derived(selectedTask?.projectId ?? (draftMode ? draftProjectId : null));
  const allProjectSections = $derived(projects.sectionsForProjectIncludingInactive(selectedProjectId));
  const sections = $derived.by(() =>
    showInactiveSections ? allProjectSections : projects.sectionsForProject(selectedProjectId)
  );
  const visibleSectionIds = $derived.by(() => new Set(sections.map((section) => section.id)));
  const statuses = $derived(projects.statusesForProject(selectedProjectId));
  const priorities = $derived(projects.prioritiesForProject(selectedProjectId));
  const activeProjectTasks = $derived.by(() =>
    projects.tasksForProject(selectedProjectId).filter((task) => visibleSectionIds.has(task.sectionId))
  );
  const allProjectTasksWithArchived = $derived.by(() =>
    projects.tasksForProjectIncludingArchived(selectedProjectId)
      .filter((task) => visibleSectionIds.has(task.sectionId))
  );
  const allProjectTasks = $derived(showArchivedTasks ? allProjectTasksWithArchived : activeProjectTasks);
  const projectCustomFields = $derived(projects.customFieldsForProject(selectedProjectId));
  const allProjectEvents = $derived.by(() => {
    if (!selectedProjectId) return [];
    return calendar.sourceEvents
      .filter((event) => event.projectId === selectedProjectId)
      .sort((a, b) => a.start.localeCompare(b.start));
  });
  const detailDirty = $derived.by(() => {
    if (draftMode) {
      return newTaskInitialDraft !== null
        && !projectTaskDetailDraftsEqual(newTaskInitialDraft, currentDetailDraft());
    }
    if (!selectedTask) return false;
    return projectTaskDetailDraftDirty(selectedTask, currentDetailDraft());
  });
  const detailCanSubmit = $derived(draftMode ? detailTitle.trim().length > 0 : detailDirty);
  const detailHasUnsavedEdits = $derived.by(() => {
    if (draftMode) {
      return detailDirty
        || projectTaskDraftRelationsChanged(draftRelations)
        || draftCustomFieldsChanged();
    }
    if (!selectedTask) return false;
    return detailDirty
      || projectCustomFields.some((field) => customFieldValueDirty(selectedTask, field))
      || projects.checklistItemsForTask(selectedTask.id).some(
        (item) => (checklistTitleDrafts[item.id] ?? item.title) !== item.title,
      );
  });
  const todayDate = $derived(Temporal.Now.plainDateISO().toString());

  $effect(() => {
    if (!selectedTask) return;
    if (
      detailDraftTaskId !== selectedTask.id
      || (!detailHasUnsavedEdits && detailDraftUpdatedAt !== selectedTask.updatedAt)
    ) {
      loadTaskDetailDraft(selectedTask);
    }
  });

  $effect(() => {
    if (!draftMode || !draftProjectId || newTaskInitialDraft) return;
    const sectionId = projects.defaultSection(draftProjectId)?.id;
    const statusId = projects.defaultStatus(draftProjectId)?.id;
    if (!sectionId || !statusId) return;
    loadNewTaskDraft(draftProjectId, projectTaskNewDetailDraft({ sectionId, statusId }));
  });

  function statusForTask(task: ProjectTask): ProjectStatus | undefined {
    return projects.statusById(task.statusId);
  }

  function taskById(taskId: string): ProjectTask | undefined {
    return allProjectTasks.find((task) => task.id === taskId);
  }

  function blockedByDependencies(task: ProjectTask) {
    if (draftMode) return projectTaskDraftDependencies(draftRelations, task.id, draftStartedAt);
    return projects.dependenciesBlockingTask(task.id);
  }

  function blocksDependencies(task: ProjectTask) {
    if (draftMode) return [];
    return projects.dependenciesBlockedByTask(task.id);
  }

  function checklistItemsForTask(task: ProjectTask): ProjectChecklistItem[] {
    if (draftMode) return projectTaskDraftChecklistItems(draftRelations, task.id, draftStartedAt);
    return projects.checklistItemsForTask(task.id);
  }

  function eventIdsForTask(task: ProjectTask): string[] {
    if (draftMode) return draftRelations.eventIds;
    return projects.eventLinksForTask(task.id).map((link) => link.eventId);
  }

  function historyForTask(task: ProjectTask) {
    if (draftMode) return [];
    return projects.taskChangeEventsForTask(task.id).slice(0, 8);
  }

  function searchLinkableEvents(
    projectId: string,
    taskId: string,
    query: string,
    startDate?: string,
    endDate?: string,
    limit?: number,
  ): Promise<ProjectLinkableEvent[]> {
    // A draft has no persisted identity yet, so search as an unlinked task.
    const searchTaskId = draftMode ? "" : taskId;
    return projects.searchLinkableEvents(projectId, searchTaskId, query, startDate, endDate, limit);
  }

  function dependencyCandidateTasks(task: ProjectTask): ProjectTask[] {
    return buildDependencyCandidateTasks({
      task,
      allProjectTasks,
      blockedByDependencies: blockedByDependencies(task),
      dependencySearch,
    });
  }

  function taskHasAnySubtasks(task: ProjectTask): boolean {
    if (draftMode) return draftRelations.subtasks.length > 0;
    return projects.subtasksForTaskIncludingArchived(task.id).length > 0;
  }

  function parentTaskCandidateTasks(task: ProjectTask): ProjectTask[] {
    return buildParentTaskCandidateTasks({
      task,
      activeProjectTasks,
      parentTaskSearch,
    });
  }

  function tagsForTask(task: ProjectTask): ProjectTag[] {
    if (draftMode) {
      return projectTaskDraftTags({
        relations: draftRelations,
        projectTags: projects.tagsForProject(task.projectId),
        projectId: task.projectId,
        timestamp: draftStartedAt,
      });
    }
    return projects.tagsForTask(task.id);
  }

  function tagCandidateTags(task: ProjectTask): ProjectTag[] {
    const tags = draftMode
      ? projects.tagsForProject(task.projectId).filter((tag) => !draftRelations.tagIds.includes(tag.id))
      : projects.unlinkedTagsForTask(task);
    return buildTagCandidateTags({ tags, tagDraft });
  }

  function canCreateTag(task: ProjectTask): boolean {
    const projectTags = projects.tagsForProject(task.projectId);
    return canCreateTaskTag({ projectTags, tagDraft })
      && (!draftMode || !projectTaskDraftHasTagName(draftRelations, projectTags, tagDraft));
  }

  function customFieldOptions(field: ProjectCustomField): ProjectCustomFieldOption[] {
    return projects.customFieldOptionsForField(field.id);
  }

  function currentDetailDraft(): ProjectTaskDetailDraft {
    return {
      title: detailTitle,
      description: detailDescription,
      sectionId: detailSectionId,
      statusId: detailStatusId,
      priority: detailPriority,
      taskType: detailTaskType,
      estimateMinutes: detailEstimateMinutes,
      startDate: detailStartDate,
      startTime: detailStartTime,
      dueDate: detailDueDate,
      dueTime: detailDueTime,
      targetEndDate: detailTargetEndDate,
      blockerReason: detailBlockerReason,
      changeReason: detailChangeReason,
      milestone: detailMilestone,
    };
  }

  function currentCustomFieldDrafts(): ProjectTaskDetailCustomFieldDrafts {
    return {
      textDrafts: customFieldTextDrafts,
      numberDrafts: customFieldNumberDrafts,
      dateDrafts: customFieldDateDrafts,
      checkboxDrafts: customFieldCheckboxDrafts,
      selectDrafts: customFieldSelectDrafts,
      multiDrafts: customFieldMultiDrafts,
    };
  }

  function customFieldValueDirty(task: ProjectTask, field: ProjectCustomField): boolean {
    // Draft values are written on creation, so they never offer a separate save.
    if (draftMode) return false;
    return projectTaskDetailCustomFieldDirty({
      field,
      drafts: currentCustomFieldDrafts(),
      value: projects.customFieldValueForTask(task.id, field.id),
      optionValues: projects.customFieldOptionValuesForTask(task.id, field.id),
    });
  }

  function draftCustomFieldDirty(field: ProjectCustomField): boolean {
    return projectTaskDetailCustomFieldDirty({
      field,
      drafts: currentCustomFieldDrafts(),
      value: undefined,
      optionValues: [],
    });
  }

  function draftCustomFieldsChanged(): boolean {
    return projectCustomFields.some(draftCustomFieldDirty);
  }

  /**
   * Parses the custom field values set on a draft. Returns the first invalid
   * reason instead when a value cannot be saved.
   */
  function draftCustomFieldValues():
    | { ok: true; values: ProjectTaskDraftCustomFieldValue[] }
    | { ok: false; message: string } {
    const values: ProjectTaskDraftCustomFieldValue[] = [];
    for (const field of projectCustomFields.filter(draftCustomFieldDirty)) {
      const draft = projectTaskDetailCustomFieldSaveDraft({ field, drafts: currentCustomFieldDrafts() });
      if (!draft.ok) {
        return {
          ok: false,
          message: draft.reason === "invalid-number"
            ? t("projects.customFields.invalidNumber")
            : t("projects.detail.invalidDate"),
        };
      }
      values.push({ fieldId: field.id, ...draft.value });
    }
    return { ok: true, values };
  }

  function subtasksForTask(parent: ProjectTask): ProjectTask[] {
    if (draftMode) return projectTaskDraftSubtaskTasks(draftRelations, parent);
    return showArchivedTasks
      ? projects.subtasksForTaskIncludingArchived(parent.id)
      : projects.subtasksForTask(parent.id);
  }

  function applyDetailDraft(detailDraft: ProjectTaskDetailDraft): void {
    detailTitle = detailDraft.title;
    detailDescription = detailDraft.description;
    detailSectionId = detailDraft.sectionId;
    detailStatusId = detailDraft.statusId;
    detailPriority = detailDraft.priority;
    detailTaskType = detailDraft.taskType;
    detailEstimateMinutes = detailDraft.estimateMinutes;
    detailStartDate = detailDraft.startDate;
    detailStartTime = detailDraft.startTime;
    detailDueDate = detailDraft.dueDate;
    detailDueTime = detailDraft.dueTime;
    detailTargetEndDate = detailDraft.targetEndDate;
    detailBlockerReason = detailDraft.blockerReason;
    detailChangeReason = detailDraft.changeReason;
    detailMilestone = detailDraft.milestone;
    detailError = null;
    datePickerTarget = null;
  }

  function loadNewTaskDraft(projectId: string, detailDraft: ProjectTaskDetailDraft): void {
    newTaskInitialDraft = detailDraft;
    draftRelations = emptyProjectTaskDraftRelations();
    draftTaskId = crypto.randomUUID();
    draftStartedAt = Temporal.Now.instant().toString();
    applyDetailDraft(detailDraft);
    loadRelationDrafts({}, projectTaskDetailCustomFieldDrafts({
      fields: projects.customFieldsForProject(projectId),
      valueForField: () => undefined,
      optionValuesForField: () => [],
    }));
  }

  function loadTaskDetailDraft(task: ProjectTask): void {
    detailDraftTaskId = task.id;
    detailDraftUpdatedAt = task.updatedAt;
    applyDetailDraft(projectTaskDetailDraftFromTask(task));
    if (createdTaskError?.taskId === task.id) detailError = createdTaskError.message;
    createdTaskError = null;
    loadRelationDrafts(
      Object.fromEntries(projects.checklistItemsForTask(task.id).map((item) => [item.id, item.title])),
      projectTaskDetailCustomFieldDrafts({
        fields: projects.customFieldsForProject(task.projectId),
        valueForField: (field) => projects.customFieldValueForTask(task.id, field.id),
        optionValuesForField: (field) => projects.customFieldOptionValuesForTask(task.id, field.id),
      }),
    );
  }

  function loadRelationDrafts(
    checklistTitles: Record<string, string>,
    customFieldDrafts: ProjectTaskDetailCustomFieldDrafts,
  ): void {
    subtaskDraft = "";
    checklistDraft = "";
    tagDraft = "";
    checklistTitleDrafts = checklistTitles;
    customFieldTextDrafts = customFieldDrafts.textDrafts;
    customFieldNumberDrafts = customFieldDrafts.numberDrafts;
    customFieldDateDrafts = customFieldDrafts.dateDrafts;
    customFieldCheckboxDrafts = customFieldDrafts.checkboxDrafts;
    customFieldSelectDrafts = customFieldDrafts.selectDrafts;
    customFieldMultiDrafts = customFieldDrafts.multiDrafts;
    dependencySearch = "";
    parentTaskSearch = "";
    datePickerTarget = null;
    customFieldDatePickerTarget = null;
  }

  function closeTaskDetailImmediately(): void {
    if (selectedTask) loadTaskDetailDraft(selectedTask);
    newTaskInitialDraft = null;
    onClose();
  }

  function requestTaskDetailClose(): void {
    if (detailHasUnsavedEdits) {
      pendingTaskOpenId = null;
      discardCloseConfirmOpen = true;
      return;
    }
    closeTaskDetailImmediately();
  }

  $effect(() => {
    if (!androidSystemBackAvailable) return;
    return mobileBackStack.activate({
      handle: requestTaskDetailClose,
    });
  });

  $effect(() => {
    if (!androidSystemBackAvailable || !datePickerTarget) return;
    return mobileBackStack.activate({
      handle: () => {
        datePickerTarget = null;
      },
    });
  });

  $effect(() => {
    if (!androidSystemBackAvailable || !customFieldDatePickerTarget) return;
    return mobileBackStack.activate({
      handle: () => {
        customFieldDatePickerTarget = null;
      },
    });
  });

  $effect(() => {
    if ((!selectedTask && !draftMode) || !detailDialog || discardCloseConfirmOpen) return;
    return activateModalFocus(detailDialog);
  });

  function confirmDiscardTaskDetail(): void {
    discardCloseConfirmOpen = false;
    const nextTaskId = pendingTaskOpenId;
    pendingTaskOpenId = null;
    if (selectedTask) loadTaskDetailDraft(selectedTask);
    if (nextTaskId) {
      onOpenTask(nextTaskId);
      return;
    }
    newTaskInitialDraft = null;
    onClose();
  }

  function cancelDiscardTaskDetail(): void {
    discardCloseConfirmOpen = false;
    pendingTaskOpenId = null;
  }

  function openTaskDetail(task: ProjectTask): void {
    if (detailHasUnsavedEdits) {
      pendingTaskOpenId = task.id;
      discardCloseConfirmOpen = true;
      return;
    }
    onOpenTask(task.id);
  }

  function handleTaskDetailKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.defaultPrevented || discardCloseConfirmOpen) return;
    if (detailDialog?.querySelector("[data-app-floating-surface]")) return;
    event.preventDefault();
    event.stopPropagation();
    requestTaskDetailClose();
  }

  function setDetailDateValue(target: DetailDateTarget, value: string): void {
    if (target === "start") {
      detailStartDate = value;
      if (!value) detailStartTime = "";
      return;
    }
    if (target === "due") {
      detailDueDate = value;
      if (!value) detailDueTime = "";
      return;
    }
    if (target === "target") {
      detailTargetEndDate = value;
      return;
    }
  }

  function toggleDetailDatePicker(target: DetailDateTarget): void {
    datePickerTarget = datePickerTarget === target ? null : target;
    customFieldDatePickerTarget = null;
  }

  function selectDetailDate(dateStr: string): void {
    if (!datePickerTarget) return;
    if (datePickerTarget === "start") {
      const nextRange = selectDateRangeStart({
        selectedDate: dateStr,
        startDate: detailStartDate || undefined,
        endDate: detailDueDate || undefined,
      });
      detailStartDate = nextRange.startDate ?? "";
      detailDueDate = nextRange.endDate ?? "";
      datePickerTarget = null;
      return;
    }
    if (datePickerTarget === "due") {
      const nextRange = selectDateRangeEnd({
        selectedDate: dateStr,
        startDate: detailStartDate || undefined,
        endDate: detailDueDate || undefined,
      });
      detailStartDate = nextRange.startDate ?? "";
      detailDueDate = nextRange.endDate ?? "";
      datePickerTarget = null;
      return;
    }
    setDetailDateValue(datePickerTarget, dateStr);
    datePickerTarget = null;
  }

  function clearDetailDate(target: DetailDateTarget): void {
    setDetailDateValue(target, "");
    if (datePickerTarget === target) datePickerTarget = null;
  }

  function selectCustomFieldDate(dateStr: string): void {
    if (!customFieldDatePickerTarget) return;
    customFieldDateDrafts = {
      ...customFieldDateDrafts,
      [customFieldDatePickerTarget]: dateStr,
    };
    customFieldDatePickerTarget = null;
  }

  function clearCustomFieldDate(fieldId: string): void {
    customFieldDateDrafts = {
      ...customFieldDateDrafts,
      [fieldId]: "",
    };
    if (customFieldDatePickerTarget === fieldId) customFieldDatePickerTarget = null;
  }

  function toggleCustomFieldMultiOption(field: ProjectCustomField, option: ProjectCustomFieldOption): void {
    const current = customFieldMultiDrafts[field.id] ?? [];
    customFieldMultiDrafts = {
      ...customFieldMultiDrafts,
      [field.id]: current.includes(option.id)
        ? current.filter((optionId) => optionId !== option.id)
        : [...current, option.id],
    };
  }

  async function saveTaskCustomField(task: ProjectTask, field: ProjectCustomField): Promise<void> {
    if (draftMode) return;
    detailError = null;
    const requestKey = `${task.id}:${field.id}`;
    const requestGeneration = (customFieldSaveGenerations.get(requestKey) ?? 0) + 1;
    customFieldSaveGenerations.set(requestKey, requestGeneration);
    const submittedDrafts = currentCustomFieldDrafts();
    const submittedRawDraft = projectTaskDetailCustomFieldRawDraft({
      field,
      drafts: submittedDrafts,
    });
    let saveRequest: Promise<void> | null = null;
    try {
      const draft = projectTaskDetailCustomFieldSaveDraft({
        field,
        drafts: submittedDrafts,
      });
      if (!draft.ok) {
        detailError = draft.reason === "invalid-number"
          ? t("projects.customFields.invalidNumber")
          : t("projects.detail.invalidDate");
        return;
      }
      const previousSave = customFieldSaveQueues.get(requestKey) ?? Promise.resolve();
      saveRequest = previousSave
        .catch(() => undefined)
        .then(() => projects.saveCustomFieldValue({
          taskId: task.id,
          fieldId: field.id,
          ...draft.value,
        }));
      customFieldSaveQueues.set(requestKey, saveRequest);
      await saveRequest;
      if (selectedTask?.id !== task.id) return;
      const mergedDrafts = projectTaskDetailMergeSavedCustomFieldDraft({
        field,
        drafts: currentCustomFieldDrafts(),
        saved: draft.value,
        submittedRawDraft,
        requestGeneration,
        latestRequestGeneration: customFieldSaveGenerations.get(requestKey) ?? 0,
      });
      customFieldTextDrafts = mergedDrafts.textDrafts;
      customFieldNumberDrafts = mergedDrafts.numberDrafts;
      customFieldDateDrafts = mergedDrafts.dateDrafts;
      customFieldCheckboxDrafts = mergedDrafts.checkboxDrafts;
      customFieldSelectDrafts = mergedDrafts.selectDrafts;
      customFieldMultiDrafts = mergedDrafts.multiDrafts;
    } catch (error) {
      if (
        customFieldSaveGenerations.get(requestKey) !== requestGeneration
        || selectedTask?.id !== task.id
      ) return;
      detailError = t(
        "projects.customFields.valueSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      if (saveRequest && customFieldSaveQueues.get(requestKey) === saveRequest) {
        customFieldSaveQueues.delete(requestKey);
      }
    }
  }

  async function submitSubtask(parent: ProjectTask): Promise<void> {
    const statusId = projects.defaultStatus(parent.projectId)?.id ?? parent.statusId;
    if (draftMode) {
      const title = subtaskDraft.trim();
      if (!title) return;
      draftRelations = {
        ...draftRelations,
        subtasks: [...draftRelations.subtasks, { id: crypto.randomUUID(), title, statusId }],
      };
      subtaskDraft = "";
      return;
    }
    await projects.addTask(parent.projectId, subtaskDraft, parent.sectionId, statusId, parent.id);
    subtaskDraft = "";
  }

  async function toggleSubtaskComplete(subtask: ProjectTask): Promise<void> {
    if (!draftMode) {
      await projects.toggleTaskDone(subtask);
      return;
    }
    const target = statusForTask(subtask)?.terminal
      ? projects.reopenStatus(subtask.projectId)
      : projects.doneStatus(subtask.projectId);
    if (!target) return;
    draftRelations = {
      ...draftRelations,
      subtasks: draftRelations.subtasks.map((entry) =>
        entry.id === subtask.id ? { ...entry, statusId: target.id } : entry),
    };
  }

  function removeDraftSubtask(subtask: ProjectTask): void {
    draftRelations = {
      ...draftRelations,
      subtasks: draftRelations.subtasks.filter((entry) => entry.id !== subtask.id),
    };
  }

  async function submitChecklistItem(task: ProjectTask): Promise<void> {
    if (draftMode) {
      const title = checklistDraft.trim();
      if (!title) return;
      const id = crypto.randomUUID();
      draftRelations = {
        ...draftRelations,
        checklist: [...draftRelations.checklist, { id, title, completed: false }],
      };
      checklistTitleDrafts = { ...checklistTitleDrafts, [id]: title };
      checklistDraft = "";
      return;
    }
    await projects.addChecklistItem(task.id, checklistDraft);
    checklistDraft = "";
  }

  async function setChecklistItemCompleted(item: ProjectChecklistItem, completed: boolean): Promise<void> {
    if (!draftMode) {
      await projects.setChecklistItemCompleted(item, completed);
      return;
    }
    draftRelations = {
      ...draftRelations,
      checklist: draftRelations.checklist.map((entry) =>
        entry.id === item.id ? { ...entry, completed } : entry),
    };
  }

  async function saveChecklistItem(item: ProjectChecklistItem): Promise<void> {
    const title = (checklistTitleDrafts[item.id] ?? item.title).trim();
    if (!title) {
      detailError = t("projects.detail.checklistItemTitleRequired");
      return;
    }
    detailError = null;
    if (draftMode) {
      draftRelations = {
        ...draftRelations,
        checklist: draftRelations.checklist.map((entry) =>
          entry.id === item.id ? { ...entry, title } : entry),
      };
    } else {
      await projects.updateChecklistItem(item, { title });
    }
    checklistTitleDrafts = { ...checklistTitleDrafts, [item.id]: title };
  }

  async function deleteChecklistItem(item: ProjectChecklistItem): Promise<void> {
    if (!draftMode) {
      await projects.removeChecklistItem(item.id);
      return;
    }
    draftRelations = {
      ...draftRelations,
      checklist: draftRelations.checklist.filter((entry) => entry.id !== item.id),
    };
  }

  async function attachExistingTag(task: ProjectTask, tag: ProjectTag): Promise<void> {
    detailError = null;
    if (draftMode) {
      draftRelations = { ...draftRelations, tagIds: [...draftRelations.tagIds, tag.id] };
      tagDraft = "";
      return;
    }
    try {
      await projects.linkTaskTag(task.id, tag.id);
      tagDraft = "";
    } catch (error) {
      detailError = t(
        "projects.detail.tagSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function submitTaskTag(task: ProjectTask): Promise<void> {
    const name = tagDraft.trim();
    if (!name) {
      detailError = t("projects.detail.tagNameRequired");
      return;
    }
    detailError = null;
    if (draftMode) {
      const existing = projects.tagsForProject(task.projectId)
        .find((tag) => tag.name.trim().toLowerCase() === name.toLowerCase());
      if (existing) {
        if (!draftRelations.tagIds.includes(existing.id)) {
          draftRelations = { ...draftRelations, tagIds: [...draftRelations.tagIds, existing.id] };
        }
      } else if (!projectTaskDraftHasTagName(draftRelations, [], name)) {
        draftRelations = { ...draftRelations, newTagNames: [...draftRelations.newTagNames, name] };
      }
      tagDraft = "";
      return;
    }
    try {
      await projects.addAndLinkTaskTag(task, name);
      tagDraft = "";
    } catch (error) {
      detailError = t(
        "projects.detail.tagSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function detachTaskTag(task: ProjectTask, tag: ProjectTag): Promise<void> {
    detailError = null;
    if (draftMode) {
      draftRelations = removeProjectTaskDraftTag(draftRelations, tag.id);
      return;
    }
    try {
      await projects.unlinkTaskTag(task.id, tag.id);
    } catch (error) {
      detailError = t(
        "projects.detail.tagSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function moveChecklistItemInDetail(item: ProjectChecklistItem, direction: -1 | 1): Promise<void> {
    detailError = null;
    if (draftMode) {
      draftRelations = {
        ...draftRelations,
        checklist: moveProjectTaskDraftEntry(draftRelations.checklist, item.id, direction),
      };
      return;
    }
    await projects.moveChecklistItem(item, direction);
  }

  async function moveSubtaskInDetail(task: ProjectTask, direction: -1 | 1): Promise<void> {
    detailError = null;
    if (draftMode) {
      draftRelations = {
        ...draftRelations,
        subtasks: moveProjectTaskDraftEntry(draftRelations.subtasks, task.id, direction),
      };
      return;
    }
    await projects.moveSubtask(task, direction);
  }

  async function promoteSubtaskFromDetail(task: ProjectTask): Promise<void> {
    detailError = null;
    if (draftMode) {
      draftRelations = { ...draftRelations, parentTaskId: null };
      return;
    }
    try {
      await projects.promoteSubtask(task);
    } catch (error) {
      detailError = t(
        "projects.detail.promoteFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function demoteTaskFromDetail(task: ProjectTask, parentTask: ProjectTask): Promise<void> {
    detailError = null;
    if (draftMode) {
      // Subtasks live in their parent's section.
      draftRelations = { ...draftRelations, parentTaskId: parentTask.id };
      detailSectionId = parentTask.sectionId;
      parentTaskSearch = "";
      return;
    }
    try {
      await projects.demoteTaskToSubtask(task, parentTask);
      parentTaskSearch = "";
    } catch (error) {
      detailError = t(
        "projects.detail.demoteFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function addBlockingDependency(blockingTask: ProjectTask, blockedTask: ProjectTask): Promise<void> {
    detailError = null;
    if (draftMode) {
      if (!draftRelations.blockedByTaskIds.includes(blockingTask.id)) {
        draftRelations = {
          ...draftRelations,
          blockedByTaskIds: [...draftRelations.blockedByTaskIds, blockingTask.id],
        };
      }
      dependencySearch = "";
      return;
    }
    try {
      await projects.addTaskDependency(blockingTask.id, blockedTask.id);
      dependencySearch = "";
    } catch (error) {
      detailError = t(
        "projects.detail.dependencySaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function removeDependency(dependencyId: string): Promise<void> {
    detailError = null;
    if (draftMode) {
      // Draft dependencies are identified by their blocking task.
      draftRelations = {
        ...draftRelations,
        blockedByTaskIds: draftRelations.blockedByTaskIds.filter((id) => id !== dependencyId),
      };
      return;
    }
    try {
      await projects.removeTaskDependency(dependencyId);
    } catch (error) {
      detailError = t(
        "projects.detail.dependencySaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function linkExistingEvent(task: ProjectTask, event: ProjectLinkableEvent): Promise<boolean> {
    detailError = null;
    if (draftMode) {
      if (!draftRelations.eventIds.includes(event.id)) {
        draftRelations = { ...draftRelations, eventIds: [...draftRelations.eventIds, event.id] };
      }
      return true;
    }
    try {
      await projects.linkTaskEvent(task.id, event.id, "scheduled");
      return true;
    } catch (error) {
      detailError = t(
        "projects.detail.eventLinkSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
      return false;
    }
  }

  async function unlinkExistingEvent(task: ProjectTask, event: ProjectLinkableEvent): Promise<void> {
    detailError = null;
    if (draftMode) {
      draftRelations = {
        ...draftRelations,
        eventIds: draftRelations.eventIds.filter((id) => id !== event.id),
      };
      return;
    }
    try {
      await projects.unlinkTaskEvent(task.id, event.id);
    } catch (error) {
      detailError = t(
        "projects.detail.eventLinkSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function archiveTaskFromDetail(task: ProjectTask): Promise<void> {
    detailError = null;
    detailSaving = true;
    try {
      await projects.archiveTasks([task]);
    } catch (error) {
      detailError = t("projects.detail.archiveFailed", error instanceof Error ? error.message : String(error));
    } finally {
      detailSaving = false;
    }
  }

  async function restoreTaskFromDetail(task: ProjectTask): Promise<void> {
    detailError = null;
    detailSaving = true;
    onShowArchivedTasks();
    try {
      await projects.restoreTasks([task]);
    } catch (error) {
      detailError = t("projects.detail.restoreFailed", error instanceof Error ? error.message : String(error));
    } finally {
      detailSaving = false;
    }
  }

  function detailSaveErrorMessage(error: unknown): string {
    const message = error instanceof Error ? error.message : String(error);
    return message === t("projects.detail.invalidEstimate")
      || message === t("projects.detail.invalidDate")
      ? message
      : t("projects.detail.saveFailed", message);
  }

  async function createTaskFromDraft(
    projectId: string,
    initialDraft: ProjectTaskDetailDraft,
    patch: ProjectTaskUpdatePatch,
  ): Promise<void> {
    const customFieldValues = draftCustomFieldValues();
    if (!customFieldValues.ok) {
      detailError = customFieldValues.message;
      return;
    }
    const draft = currentDetailDraft();
    const relations = draftRelationsWithTitleDrafts();
    let created: ProjectTask | undefined;
    const errors: string[] = [];
    detailSaving = true;
    detailError = null;
    try {
      created = await projects.addTask(
        projectId,
        draft.title,
        draft.sectionId,
        draft.statusId,
        relations.parentTaskId ?? undefined,
      );
      if (!created) return;
      if (projectTaskNewDetailDraftHasExtras(initialDraft, draft)) {
        await projects.updateTask(created, patch);
      }
    } catch (error) {
      if (!created) {
        detailError = t("projects.tasks.createFailed", error instanceof Error ? error.message : String(error));
        detailSaving = false;
        return;
      }
      // The task exists, so keep the unsaved fields on it for a retry with Save changes.
      detailDraftTaskId = created.id;
      detailDraftUpdatedAt = created.updatedAt;
      errors.push(detailSaveErrorMessage(error));
    }
    if (!created) {
      detailSaving = false;
      return;
    }
    const relationFailures = await applyProjectTaskDraftRelations({
      task: created,
      relations,
      customFieldValues: customFieldValues.values,
      writer: draftRelationWriter(),
    });
    detailSaving = false;
    if (relationFailures.length > 0) {
      errors.push(t("projects.detail.draftRelationsFailed", relationFailures.join("; ")));
    }
    if (errors.length > 0) {
      const message = errors.join(" ");
      detailError = message;
      // Loading the created task resets the form unless its unsaved fields were kept above.
      if (detailDraftTaskId !== created.id) createdTaskError = { taskId: created.id, message };
    }
    newTaskInitialDraft = null;
    onCreated?.(created);
  }

  /** Applies checklist titles edited in place but not yet confirmed. */
  function draftRelationsWithTitleDrafts(): ProjectTaskDraftRelations {
    return {
      ...draftRelations,
      checklist: draftRelations.checklist.map((item) => ({
        ...item,
        title: checklistTitleDrafts[item.id]?.trim() || item.title,
      })),
    };
  }

  function draftRelationWriter(): ProjectTaskDraftRelationWriter {
    return {
      addChecklistItem: async (taskId, title, completed) => {
        const item = await projects.addChecklistItem(taskId, title);
        if (item && completed) await projects.setChecklistItemCompleted(item, true);
      },
      addSubtask: async (parent, title, statusId) => {
        await projects.addTask(parent.projectId, title, parent.sectionId, statusId, parent.id);
      },
      linkTag: (taskId, tagId) => projects.linkTaskTag(taskId, tagId),
      addAndLinkTag: (task, name) => projects.addAndLinkTaskTag(task, name),
      addDependency: (blockingTaskId, blockedTaskId) =>
        projects.addTaskDependency(blockingTaskId, blockedTaskId),
      linkEvent: (taskId, eventId) => projects.linkTaskEvent(taskId, eventId, "scheduled"),
      saveCustomFieldValue: (value) => projects.saveCustomFieldValue(value),
    };
  }

  async function saveTaskDetail(): Promise<void> {
    const patch = projectTaskDetailDraftPatch(currentDetailDraft());
    if (!patch.ok) {
      if (patch.reason === "title-required") {
        detailError = t("projects.detail.titleRequired");
      } else if (patch.reason === "invalid-estimate") {
        detailError = t("projects.detail.invalidEstimate");
      } else if (patch.reason === "invalid-date") {
        detailError = t("projects.detail.invalidDate");
      }
      return;
    }
    if (draftMode) {
      if (draftProjectId && newTaskInitialDraft) {
        await createTaskFromDraft(draftProjectId, newTaskInitialDraft, patch.patch);
      }
      return;
    }
    if (!selectedTask) return;
    detailSaving = true;
    detailError = null;
    try {
      await projects.updateTask(selectedTask, patch.patch);
      detailChangeReason = "";
      detailDraftUpdatedAt = selectedTask.updatedAt;
    } catch (error) {
      detailError = detailSaveErrorMessage(error);
    } finally {
      detailSaving = false;
    }
  }
</script>

<svelte:window onkeydown={handleTaskDetailKeydown} />

{#if selectedTask || draftMode}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="surface-backdrop fixed inset-0 z-70 flex items-center justify-center p-3"
      style={androidSystemBackAvailable
        ? "padding: calc(var(--safe-area-top) + 0.75rem) calc(var(--safe-area-right) + 0.75rem) calc(var(--safe-area-bottom) + 0.75rem) calc(var(--safe-area-left) + 0.75rem)"
        : undefined}
      onclick={requestTaskDetailClose}
    >
    <div
      bind:this={detailDialog}
      class={cn(
        "task-detail-dialog",
        "surface-dialog flex max-h-full max-w-full min-h-0 flex-col overflow-hidden",
        layout === "fullscreen" && androidSystemBackAvailable && "h-full w-full",
        layout === "fullscreen" && !androidSystemBackAvailable && "h-[calc(100dvh-1rem)] w-[calc(100vw-1rem)]",
        layout === "sheet" && "h-[min(88dvh,48rem)] w-[calc(100vw-1rem)] max-w-6xl",
        layout === "modal" && "h-[min(92dvh,62rem)] w-[min(78rem,calc(100vw-2rem))]",
      )}
      data-floating-root
      role="dialog"
      data-mobile={androidSystemBackAvailable || undefined}
      aria-modal="true"
      aria-label={draftMode ? t("projects.detail.newTitle") : t("projects.detail.title")}
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
    >
      <ProjectTaskDetailHeader
        task={selectedTask ?? null}
        projectName={selectedProjectId ? projects.projectById(selectedProjectId)?.name ?? "" : ""}
        title={detailTitle}
        onTitleChange={(value) => { detailTitle = value; }}
        onClose={requestTaskDetailClose}
      />

      <form class="flex min-h-0 flex-1 flex-col" onsubmit={(event) => { event.preventDefault(); void saveTaskDetail(); }}>
        <div class="relative min-h-0 flex-1">
          <div bind:this={detailScrollContainer} use:scrollEdgeFadeAction class="task-detail-scroll hide-scrollbar h-full overflow-y-auto overscroll-contain">
            {#key selectedTask?.id ?? "draft"}
              <div class="task-detail-workspace">
                <div class="task-detail-content">
                  <ProjectTaskDetailDescriptionSection
                    value={detailDescription}
                    onChange={(value) => { detailDescription = value; }}
                  />

                  {#if sectionTask}
                  {@const selectedTaskHistory = historyForTask(sectionTask)}
                  {@const selectedTaskChecklist = checklistItemsForTask(sectionTask)}
                  {@const selectedTaskSubtasks = subtasksForTask(sectionTask)}
                  {@const selectedTaskBlockedBy = blockedByDependencies(sectionTask)}
                  {@const selectedTaskBlocks = blocksDependencies(sectionTask)}
                  {@const selectedTaskDependencyCandidates = dependencyCandidateTasks(sectionTask)}
                  {@const selectedTaskEventIds = eventIdsForTask(sectionTask)}
                  <ProjectTaskDetailChecklistSection
                    task={sectionTask}
                    items={selectedTaskChecklist}
                    titleDrafts={checklistTitleDrafts}
                    draft={checklistDraft}
                    onTitleDraftChange={(itemId, value) => {
                      checklistTitleDrafts = {
                        ...checklistTitleDrafts,
                        [itemId]: value,
                      };
                    }}
                    onDraftChange={(value) => { checklistDraft = value; }}
                    onToggleCompleted={setChecklistItemCompleted}
                    onSaveItem={saveChecklistItem}
                    onMoveItem={moveChecklistItemInDetail}
                    onDeleteItem={deleteChecklistItem}
                    onSubmitItem={submitChecklistItem}
                  />

                  <ProjectTaskDetailSubtasksSection
                    task={sectionTask}
                    subtasks={selectedTaskSubtasks}
                    theme={theme.current}
                    draft={subtaskDraft}
                    {statusForTask}
                    onDraftChange={(value) => { subtaskDraft = value; }}
                    onSubmitSubtask={submitSubtask}
                    onToggleComplete={toggleSubtaskComplete}
                    onOpenTask={openTaskDetail}
                    onPromoteSubtask={promoteSubtaskFromDetail}
                    onMoveSubtask={moveSubtaskInDetail}
                    onRemoveSubtask={draftMode ? removeDraftSubtask : undefined}
                  />

                  <ProjectTaskDetailDependenciesSection
                    task={sectionTask}
                    blockedByDependencies={selectedTaskBlockedBy}
                    blocksDependencies={selectedTaskBlocks}
                    dependencyCandidates={selectedTaskDependencyCandidates}
                    dependencySearch={dependencySearch}
                    {taskById}
                    onDependencySearchChange={(value) => { dependencySearch = value; }}
                    onAddBlockingDependency={addBlockingDependency}
                    onRemoveDependency={removeDependency}
                  />

                  <ProjectTaskDetailScheduledBlocksController
                    task={sectionTask}
                    taskEventIds={selectedTaskEventIds}
                    {allProjectEvents}
                    {todayDate}
                    {searchLinkableEvents}
                    onLinkEvent={linkExistingEvent}
                    onUnlinkEvent={unlinkExistingEvent}
                  />

                  <ProjectTaskDetailHistorySection events={selectedTaskHistory} />
                  {/if}

                </div>
                <aside class="task-detail-properties" aria-label={t("projects.detail.properties")}>
                  <h2 class="text-[0.866667rem] font-semibold">{t("projects.detail.properties")}</h2>
                  <ProjectTaskDetailPropertiesSection
                    {statuses}
                    {sections}
                    {priorities}
                    theme={theme.current}
                    statusId={detailStatusId}
                    sectionId={detailSectionId}
                    priority={detailPriority}
                    taskType={detailTaskType}
                    onStatusChange={(value) => { detailStatusId = value; }}
                    onSectionChange={(value) => { detailSectionId = value; }}
                    onPriorityChange={(value) => { detailPriority = value; }}
                    onTaskTypeChange={(value) => { detailTaskType = value; }}
                  />

                  <ProjectTaskDetailPlanningSection
                    milestone={detailMilestone}
                    onMilestoneChange={(value) => { detailMilestone = value; }}
                    estimateMinutes={detailEstimateMinutes}
                    startDate={detailStartDate}
                    startTime={detailStartTime}
                    dueDate={detailDueDate}
                    dueTime={detailDueTime}
                    targetEndDate={detailTargetEndDate}
                    blockerReason={detailBlockerReason}
                    changeReason={detailChangeReason}
                    {todayDate}
                    startPickerOpen={datePickerTarget === "start"}
                    duePickerOpen={datePickerTarget === "due"}
                    targetPickerOpen={datePickerTarget === "target"}
                    onEstimateMinutesChange={(value) => { detailEstimateMinutes = value; }}
                    onBlockerReasonChange={(value) => { detailBlockerReason = value; }}
                    onChangeReasonChange={(value) => { detailChangeReason = value; }}
                    onToggleStartPicker={() => toggleDetailDatePicker("start")}
                    onToggleDuePicker={() => toggleDetailDatePicker("due")}
                    onToggleTargetPicker={() => toggleDetailDatePicker("target")}
                    onClearStartDate={() => clearDetailDate("start")}
                    onClearDueDate={() => clearDetailDate("due")}
                    onClearTargetEndDate={() => clearDetailDate("target")}
                    onSelectDate={selectDetailDate}
                    onStartTimeChange={(time) => { detailStartTime = time ?? ""; }}
                    onDueTimeChange={(time) => { detailDueTime = time ?? ""; }}
                    onCancelDatePicker={() => { datePickerTarget = null; }}
                  />

                  {#if sectionTask}
                  {@const selectedTaskTags = tagsForTask(sectionTask)}
                  {@const selectedTaskTagCandidates = tagCandidateTags(sectionTask)}
                  {@const selectedTaskParent = sectionTask.parentTaskId ? taskById(sectionTask.parentTaskId) : undefined}
                  <ProjectTaskDetailParentSection
                    task={sectionTask}
                    parentTask={selectedTaskParent}
                    parentCandidates={parentTaskCandidateTasks(sectionTask)}
                    parentSearch={parentTaskSearch}
                    hasSubtasks={taskHasAnySubtasks(sectionTask)}
                    onParentSearchChange={(value) => { parentTaskSearch = value; }}
                    onPromoteSubtask={promoteSubtaskFromDetail}
                    onDemoteTask={demoteTaskFromDetail}
                  />

                  <ProjectTaskDetailTagsSection
                    task={sectionTask}
                    tags={selectedTaskTags}
                    candidates={selectedTaskTagCandidates}
                    draft={tagDraft}
                    canCreate={canCreateTag(sectionTask)}
                    theme={theme.current}
                    onDraftChange={(value) => { tagDraft = value; }}
                    onAttachTag={attachExistingTag}
                    onSubmitTag={submitTaskTag}
                    onDetachTag={detachTaskTag}
                  />

                  {#if projectCustomFields.length > 0}
                    <ProjectTaskDetailCustomFieldsSection
                      task={sectionTask}
                      fields={projectCustomFields}
                      textDrafts={customFieldTextDrafts}
                      numberDrafts={customFieldNumberDrafts}
                      dateDrafts={customFieldDateDrafts}
                      checkboxDrafts={customFieldCheckboxDrafts}
                      selectDrafts={customFieldSelectDrafts}
                      multiDrafts={customFieldMultiDrafts}
                      datePickerTarget={customFieldDatePickerTarget}
                      {todayDate}
                      {customFieldOptions}
                      {customFieldValueDirty}
                      onTextDraftChange={(fieldId, value) => {
                        customFieldTextDrafts = {
                          ...customFieldTextDrafts,
                          [fieldId]: value,
                        };
                      }}
                      onNumberDraftChange={(fieldId, value) => {
                        customFieldNumberDrafts = {
                          ...customFieldNumberDrafts,
                          [fieldId]: value,
                        };
                      }}
                      onCheckboxDraftChange={(fieldId, value) => {
                        customFieldCheckboxDrafts = {
                          ...customFieldCheckboxDrafts,
                          [fieldId]: value,
                        };
                      }}
                      onSelectDraftChange={(fieldId, value) => {
                        customFieldSelectDrafts = {
                          ...customFieldSelectDrafts,
                          [fieldId]: value,
                        };
                      }}
                      onToggleMultiOption={toggleCustomFieldMultiOption}
                      onSaveField={saveTaskCustomField}
                      onToggleDatePicker={(fieldId) => {
                        customFieldDatePickerTarget = customFieldDatePickerTarget === fieldId ? null : fieldId;
                        datePickerTarget = null;
                      }}
                      onClearDate={clearCustomFieldDate}
                      onSelectDate={selectCustomFieldDate}
                      onCancelDatePicker={() => { customFieldDatePickerTarget = null; }}
                    />
                  {/if}
                  {/if}
                </aside>
              </div>
            {/key}
          </div>
          <CalendarScrollbar
            scrollContainer={detailScrollContainer}
            stickyTop={8}
            stickyBottom={8}
            wheelPassthrough
          />
        </div>

        {#if detailError}
          <div role="alert" class="shrink-0 border-t border-destructive/20 bg-destructive/10 px-5 py-2 text-[0.8rem] text-destructive">
            {detailError}
          </div>
        {/if}

        <ProjectTaskDetailFooter
          task={selectedTask ?? null}
          saving={detailSaving}
          dirty={detailCanSubmit}
          onArchive={archiveTaskFromDetail}
          onRestore={restoreTaskFromDetail}
        />
      </form>
    </div>
    </div>
    {#if discardCloseConfirmOpen}
      <ConfirmDialog
        title={draftMode ? t("projects.detail.discardDraftTitle") : t("calendar.view.discardUnsavedTitle")}
        message={draftMode ? t("projects.detail.discardDraftMessage") : t("calendar.view.changesLost")}
        confirmLabel={t("calendar.view.discard")}
        cancelLabel={t("common.cancel")}
        onConfirm={confirmDiscardTaskDetail}
        onCancel={cancelDiscardTaskDetail}
      />
    {/if}
{/if}

<style>
  .task-detail-scroll {
    container: task-detail / inline-size;
  }

  .task-detail-workspace {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(20rem, 0.65fr);
    min-height: 100%;
    align-items: start;
  }

  .task-detail-content {
    display: grid;
    min-width: 0;
    align-content: start;
    gap: 1.75rem;
    padding: 1.5rem 2rem 2rem;
  }

  .task-detail-properties {
    display: grid;
    min-width: 0;
    align-content: start;
    gap: 1rem;
    align-self: stretch;
    padding: 1.5rem;
    border-left: 1px solid var(--border);
    background: color-mix(in oklab, var(--muted) 20%, var(--card));
  }

  .task-detail-dialog :global(.task-property-row) {
    display: grid;
    grid-template-columns: minmax(6.5rem, 0.85fr) minmax(0, 1.15fr);
    gap: 0.75rem;
    align-items: center;
    min-height: 2rem;
    font-size: calc(0.8rem * var(--type-scale, 1));
    color: var(--muted-foreground);
  }

  .task-detail-dialog :global(:is(input, textarea, button, select):focus) {
    outline: none !important;
    box-shadow: none !important;
  }

  .task-detail-dialog :global(button:focus-visible) {
    background-color: var(--accent);
  }

  .task-detail-dialog :global(button:focus-visible) {
    color: var(--accent-foreground);
  }

  .task-detail-content :global(.task-detail-section + .task-detail-section) {
    border-top: 1px solid color-mix(in oklab, var(--border) 65%, transparent);
    padding-top: 1.25rem;
  }

  @container task-detail (max-width: 54rem) {
    .task-detail-workspace {
      grid-template-columns: minmax(0, 1fr);
    }
    .task-detail-content {
      display: contents;
    }
    .task-detail-workspace {
      gap: 1.5rem;
      padding: 1.25rem;
    }
    .task-detail-properties {
      grid-row: 2;
      border-left: 0;
      border-bottom: 1px solid var(--border);
    }
  }

  .task-detail-dialog[data-mobile="true"] :global(button),
  .task-detail-dialog[data-mobile="true"] :global(input:not([type="checkbox"])),
  .task-detail-dialog[data-mobile="true"] :global(select),
  .task-detail-dialog[data-mobile="true"] :global(textarea) {
    min-height: 3rem;
  }
</style>
