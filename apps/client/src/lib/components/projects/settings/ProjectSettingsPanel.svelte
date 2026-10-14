<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import { getMusicContextAssignments, getMusicPlaylistSummaries } from "$lib/music/platform-library";
  import { FALLBACK_COLOR_INDEX, type EventColor } from "$lib/calendar/types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import type { PomodoroPresetKey } from "$lib/pomodoro/rhythm";
  import {
    PROJECT_POMODORO_PRESET_ORDER,
  } from "$lib/projects/default-pomodoro";
  import {
    projectLifecycleLabel,
  } from "$lib/projects/display";
  import {
    draftPriorityColor,
    draftPriorityName,
    draftStatusCategory,
    draftStatusColor,
    draftStatusName,
    draftTagColor,
    draftTagName,
    tagNameTaken,
    type PriorityDraftState,
    type StatusDraftState,
    type TagDraftState,
  } from "$lib/projects/settings/collection-drafts";
  import {
    PROJECT_LIFECYCLE_STATUSES,
    PROJECT_TAG_DEFAULT_COLOR,
  } from "$lib/projects/types";
  import { createProjectSettingsCustomFieldController } from "$lib/projects/settings/custom-field-controller.svelte";
  import { createProjectSettingsColorAllocator } from "$lib/projects/settings/color-controller";
  import { createProjectSettingsSortableController } from "$lib/projects/settings/sortable-controller.svelte";
  import { createProjectSettingsSession } from "$lib/projects/settings/session.svelte";
  import { createProjectSettingsStructureDraft } from "$lib/projects/settings/structure-draft.svelte";
  import {
    nextProjectSettingsPaletteColor,
    scrollProjectSettingsRowIntoView,
  } from "$lib/projects/settings/ui";
  import type {
    Project,
    ProjectCustomField,
    ProjectCustomFieldOption,
    ProjectTag,
    ProjectLifecycleStatus,
    ProjectPriorityConfig,
    ProjectStatus,
    ProjectStatusCategory,
  } from "$lib/projects/types";
  import {
    completeMusicAssignmentDrafts,
    musicAssignmentDraftsEqual,
    persistedMusicAssignmentDrafts,
  } from "$lib/music/assignment-draft";
  import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import ProjectSettingsCustomFieldsSection from "./ProjectSettingsCustomFieldsSection.svelte";
  import ProjectSettingsDefaultsSection from "./ProjectSettingsDefaultsSection.svelte";
  import ProjectSettingsDeleteDialogs from "./ProjectSettingsDeleteDialogs.svelte";
  import ProjectSettingsIdentitySection from "./ProjectSettingsIdentitySection.svelte";
  import ProjectSettingsMembersSection from "./ProjectSettingsMembersSection.svelte";
  import ProjectSettingsWorkingFoldersSection from "$lib/components/projects/settings/ProjectSettingsWorkingFoldersSection.svelte";
  import { projectHasLockedSystemIdentity } from "$lib/projects/system-defaults";
  import ProjectSettingsPanelShell from "./ProjectSettingsPanelShell.svelte";
  import ProjectSettingsPrioritiesSection from "./ProjectSettingsPrioritiesSection.svelte";
  import ProjectSettingsStatusesSection from "./ProjectSettingsStatusesSection.svelte";
  import ProjectSettingsTagsSection from "./ProjectSettingsTagsSection.svelte";

  let {
    projectId,
    presentation = "side",
    onClose,
    onRevealInactive,
    onDirtyChange,
  }: {
    projectId: string;
    presentation?: "side" | "popover";
    onClose: () => void;
    onRevealInactive: () => void;
    onDirtyChange: (dirty: boolean) => void;
  } = $props();

  const projects = getProjects();
  const theme = getTheme();
  const { t } = getLocalization();
  const musicAssignmentsAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "music.context-assignments",
  );
  const workingFoldersAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "projects.working-folders",
  );
  const idleDetectionAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "pomodoro.native-idle-detection",
  );

  const PROJECT_STATUS_CATEGORIES: ProjectStatusCategory[] = ["not_started", "active", "blocked", "done"];
  const NEW_STATUS_FIRST_COLOR: EventColor = 8;
  const NEW_PRIORITY_FIRST_COLOR: EventColor = 13;
  const NEW_TAG_FIRST_COLOR = PROJECT_TAG_DEFAULT_COLOR;
  type SelectOption = { value: string; label: string };

  const session = createProjectSettingsSession({
    onRevealInactive: () => onRevealInactive(),
  });
  const sessionState = session.state;
  const structure = createProjectSettingsStructureDraft(projects);
  let workingFoldersSection = $state<{ saveDraft: () => Promise<void>; discardDraft: () => void }>();
  let workingFoldersDirty = $state(false);
  let workingFoldersBusy = $state(false);
  let pendingDeleteStatusId = $state<string | null>(null);
  let pendingDeletePriorityId = $state<string | null>(null);
  let pendingDeleteTagId = $state<string | null>(null);
  let settingsScrollElement = $state<HTMLElement | undefined>();
  let newStatusRowElement = $state<HTMLDivElement | undefined>();
  let newPriorityRowElement = $state<HTMLDivElement | undefined>();
  let newTagRowElement = $state<HTMLDivElement | undefined>();
  let newCustomFieldRowElement = $state<HTMLDivElement | undefined>();
  let musicAssignments = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  let savedMusicAssignments = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  let musicPlaylists = $state<MusicPlaylistSummary[]>([]);
  let musicAssignmentsProjectId = $state<string | null>(null);
  let musicAssignmentsLoading = $state(false);
  let musicAssignmentsError = $state<string | null>(null);
  let musicLoadGeneration = 0;

  const selectedProject = $derived(projects.projectById(projectId));
  const selectedProjectId = $derived(selectedProject?.id ?? null);
  const selectedProjectIdentityLocked = $derived(
    selectedProject ? projectHasLockedSystemIdentity(selectedProject) : false,
  );
  const visibleProjectGroups = $derived.by(() => projects.visibleGroups());
  const statuses = $derived(structure.statuses);
  const priorities = $derived(structure.priorities);
  const projectTags = $derived(structure.tags);
  const projectCustomFields = $derived(structure.customFields);
  const projectGroupOptions = $derived<SelectOption[]>(
    visibleProjectGroups.map((group) => ({ value: group.id, label: group.name })),
  );
  const lifecycleOptions = $derived<SelectOption[]>(
    PROJECT_LIFECYCLE_STATUSES.map((status) => ({
      value: status,
      label: projectLifecycleLabel(status, t),
    })),
  );
  const statusCategoryOptions = $derived<SelectOption[]>(
    PROJECT_STATUS_CATEGORIES.map((category) => ({
      value: category,
      label: statusCategoryLabel(category),
    })),
  );
  const pendingDeleteTag = $derived.by(() =>
    pendingDeleteTagId ? projectTags.find((tag) => tag.id === pendingDeleteTagId) : undefined
  );
  const pendingDeleteStatus = $derived.by(() =>
    pendingDeleteStatusId ? statuses.find((status) => status.id === pendingDeleteStatusId) : undefined
  );
  const pendingDeletePriority = $derived.by(() =>
    pendingDeletePriorityId ? priorities.find((priority) => priority.id === pendingDeletePriorityId) : undefined
  );
  const draftReady = $derived(
    Boolean(selectedProject && sessionState.projectDraftId === selectedProject.id),
  );
  const musicAssignmentsDirty = $derived(
    Boolean(selectedProject && musicAssignmentsProjectId === selectedProject.id)
      && !musicAssignmentDraftsEqual(musicAssignments, savedMusicAssignments),
  );
  const settingsDirty = $derived.by(() => selectedProject
    ? session.dirty(selectedProject, sessionCollections()) || musicAssignmentsDirty || structure.dirty || workingFoldersDirty
    : false);
  const customFields = createProjectSettingsCustomFieldController({
    state: sessionState,
    fields: () => projectCustomFields,
    collections: structure,
    translate: t,
    setDraftError: session.setCustomFieldDraftError,
    afterCreateDraft: () => { void scrollToNewCustomFieldRow(); },
  });
  const pendingDeleteCustomField = $derived(customFields.pendingDeleteField());
  const pendingDeleteCustomFieldOption = $derived(customFields.pendingDeleteOption());

  const statusSortable = createProjectSettingsSortableController({
    getEntries: () => statuses,
    moveEntry: structure.moveStatus,
    setError: (error) => { sessionState.projectSettingsError = error; },
    reorderFailedMessage: () => t("projects.settings.statusReorderFailed"),
    saveFailedMessage: (message) => t("projects.settings.statusSaveFailed", message),
  });
  const prioritySortable = createProjectSettingsSortableController({
    getEntries: () => priorities,
    moveEntry: structure.movePriority,
    setError: (error) => { sessionState.projectSettingsError = error; },
    reorderFailedMessage: () => t("projects.settings.priorityReorderFailed"),
    saveFailedMessage: (message) => t("projects.settings.prioritySaveFailed", message),
  });
  const tagSortable = createProjectSettingsSortableController({
    getEntries: () => projectTags,
    moveEntry: structure.moveTag,
    setError: (error) => { sessionState.projectSettingsError = error; },
    reorderFailedMessage: () => t("projects.settings.tagReorderFailed"),
    saveFailedMessage: (message) => t("projects.settings.tagSaveFailed", message),
  });
  const customFieldSortable = createProjectSettingsSortableController({
    getEntries: () => projectCustomFields,
    moveEntry: structure.moveCustomField,
    setError: (error) => { sessionState.projectSettingsError = error; },
    reorderFailedMessage: () => t("projects.customFields.reorderFailed"),
    saveFailedMessage: (message) => t("projects.customFields.saveFailed", message),
  });
  const customFieldOptionSortable = createProjectSettingsSortableController({
    getEntries: () => projectCustomFields.flatMap(customFieldOptions),
    getEntriesForEntry: (option) => structure.optionsForField(option.fieldId),
    moveEntry: structure.moveCustomFieldOption,
    setError: (error) => { sessionState.projectSettingsError = error; },
    reorderFailedMessage: () => t("projects.customFields.optionReorderFailed"),
    saveFailedMessage: (message) => t("projects.customFields.optionSaveFailed", message),
  });

  $effect(() => {
    if (!selectedProject || sessionState.projectSettingsSaving) return;
    if (
      sessionState.projectDraftId !== selectedProject.id
      || (!settingsDirty && sessionState.projectDraftUpdatedAt !== selectedProject.updatedAt)
    ) {
      loadDraft(selectedProject);
    }
  });

  $effect(() => {
    if (!selectedProject || !selectedProjectIdentityLocked) return;
    session.synchronizeLockedIdentity(selectedProject);
  });

  $effect(() => {
    onDirtyChange(settingsDirty);
  });

  onDestroy(() => {
    musicLoadGeneration += 1;
    resetReorderDrags();
    onDirtyChange(false);
  });

  function loadDraft(project: Project): void {
    structure.load(canonicalCollections(project.id));
    session.load(project, sessionCollections(), initialCreateColors());
    pendingDeleteStatusId = null;
    pendingDeletePriorityId = null;
    pendingDeleteTagId = null;
    customFields.resetTransientState();
    resetReorderDrags();
    if (musicAssignmentsAvailable) {
      void loadMusicAssignments(project.id);
    } else {
      musicAssignments = completeMusicAssignmentDrafts([]);
      savedMusicAssignments = completeMusicAssignmentDrafts([]);
      musicPlaylists = [];
      musicAssignmentsProjectId = null;
      musicAssignmentsLoading = false;
      musicAssignmentsError = null;
    }
  }

  async function loadMusicAssignments(projectId: string): Promise<void> {
    const generation = ++musicLoadGeneration;
    musicAssignmentsLoading = true;
    musicAssignmentsError = null;
    const [assignmentsResult, playlistsResult] = await Promise.allSettled([
      getMusicContextAssignments("project-default", projectId),
      getMusicPlaylistSummaries(Date.now(), 0, 500),
    ]);
    if (generation !== musicLoadGeneration || selectedProject?.id !== projectId) return;
    if (assignmentsResult.status === "fulfilled") {
      savedMusicAssignments = completeMusicAssignmentDrafts(assignmentsResult.value);
      musicAssignments = completeMusicAssignmentDrafts(assignmentsResult.value);
      musicAssignmentsProjectId = projectId;
    }
    if (playlistsResult.status === "fulfilled") musicPlaylists = playlistsResult.value;
    const failures = [assignmentsResult, playlistsResult]
      .filter((result): result is PromiseRejectedResult => result.status === "rejected")
      .map((result) => result.reason instanceof Error ? result.reason.message : String(result.reason));
    musicAssignmentsError = failures.length > 0 ? failures.join(" ") : null;
    musicAssignmentsLoading = false;
  }

  function sessionCollections() {
    return {
      statuses,
      priorities,
      tags: projectTags,
      customFields: projectCustomFields,
      optionsForField: structure.optionsForField,
    };
  }

  /** Read canonical collections only when opening, discarding, or completing Save. */
  function canonicalCollections(id: string) {
    return {
      statuses: projects.statusesForProject(id),
      priorities: projects.prioritiesForProject(id),
      tags: projects.tagsForProject(id),
      customFields: projects.customFieldsForProject(id),
      optionsForField: projects.customFieldOptionsForField,
    };
  }

  function initialCreateColors() {
    return {
      status: nextUnusedStatusColor(NEW_STATUS_FIRST_COLOR),
      priority: nextUnusedPriorityColor(NEW_PRIORITY_FIRST_COLOR),
      tag: nextUnusedTagColor(NEW_TAG_FIRST_COLOR),
    };
  }

  function closeProjectSettings(): void {
    onClose();
  }

  function discardProjectSettings(): void {
    if (sessionState.projectSettingsSaving) return;
    if (selectedProject) {
      structure.load(canonicalCollections(selectedProject.id));
      session.discard(selectedProject, sessionCollections(), initialCreateColors());
      musicAssignments = completeMusicAssignmentDrafts(savedMusicAssignments);
      workingFoldersSection?.discardDraft();
      customFields.resetTransientState();
      pendingDeleteStatusId = null;
      pendingDeletePriorityId = null;
      pendingDeleteTagId = null;
      resetReorderDrags();
    }
  }

  function statusCategoryLabel(category: ProjectStatusCategory): string {
    if (category === "active") return t("projects.statusCategory.active");
    if (category === "blocked") return t("projects.statusCategory.blocked");
    if (category === "done") return t("projects.statusCategory.done");
    return t("projects.statusCategory.notStarted");
  }

  function setLifecycleStatus(value: string): void {
    if (PROJECT_LIFECYCLE_STATUSES.includes(value as ProjectLifecycleStatus)) {
      sessionState.projectDraft.status = value as ProjectLifecycleStatus;
    }
  }

  function setStatusCategory(statusId: string, value: string): void {
    if (!PROJECT_STATUS_CATEGORIES.includes(value as ProjectStatusCategory)) return;
    sessionState.statusCategoryDrafts = {
      ...sessionState.statusCategoryDrafts,
      [statusId]: value as ProjectStatusCategory,
    };
  }

  function setNewStatusCategory(value: string): void {
    if (PROJECT_STATUS_CATEGORIES.includes(value as ProjectStatusCategory)) {
      sessionState.newStatusCategory = value as ProjectStatusCategory;
    }
  }

  function pomodoroPresetLabel(preset: PomodoroPresetKey): string {
    if (preset === "creative") return t("projects.pomodoro.creative");
    if (preset === "balanced") return t("projects.pomodoro.balanced");
    if (preset === "deep") return t("projects.pomodoro.deep");
    if (preset === "extended") return t("projects.pomodoro.extended");
    return t("projects.pomodoro.adaptive");
  }

  function statusDraftState(): StatusDraftState {
    return {
      nameDrafts: sessionState.statusNameDrafts,
      categoryDrafts: sessionState.statusCategoryDrafts,
      colorDrafts: sessionState.statusColorDrafts,
      fallbackColor: FALLBACK_COLOR_INDEX,
    };
  }

  function priorityDraftState(): PriorityDraftState {
    return {
      nameDrafts: sessionState.priorityNameDrafts,
      colorDrafts: sessionState.priorityColorDrafts,
      fallbackColor: FALLBACK_COLOR_INDEX,
    };
  }

  function tagDraftState(): TagDraftState {
    return {
      nameDrafts: sessionState.tagNameDrafts,
      colorDrafts: sessionState.tagColorDrafts,
      fallbackColor: FALLBACK_COLOR_INDEX,
    };
  }

  const customFieldOptions = customFields.fieldOptions;
  const customFieldOptionCreateDraftRows = customFields.optionCreateRows;

  function fieldForCustomFieldOption(option: ProjectCustomFieldOption | undefined): ProjectCustomField | undefined {
    return option ? projectCustomFields.find((field) => field.id === option.fieldId) : undefined;
  }

  const setCustomFieldOptionCreateDraftName = customFields.setOptionCreateName;
  const removeCustomFieldOptionCreateDraft = customFields.removeOptionCreate;
  const setCustomFieldCreateDraftName = customFields.setCreateFieldName;
  const setCustomFieldCreateDraftOptionName = customFields.setCreateOptionName;
  const setCustomFieldCreateDraftPendingOptionName = customFields.setCreatePendingOptionName;
  const removeCustomFieldCreateDraft = customFields.removeCreateField;
  const removeCustomFieldCreateDraftOption = customFields.removeCreateOption;

  async function scrollToSettingsRow(rowElement: HTMLElement | undefined): Promise<void> {
    await tick();
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => resolve());
    });
    scrollProjectSettingsRowIntoView(settingsScrollElement, rowElement);
  }

  async function scrollToNewStatusRow(): Promise<void> {
    await scrollToSettingsRow(newStatusRowElement);
  }

  async function scrollToNewPriorityRow(): Promise<void> {
    await scrollToSettingsRow(newPriorityRowElement);
  }

  async function scrollToNewTagRow(): Promise<void> {
    await scrollToSettingsRow(newTagRowElement);
  }

  async function scrollToNewCustomFieldRow(): Promise<void> {
    await scrollToSettingsRow(newCustomFieldRowElement);
  }

  function tagNameDraftValue(tag: ProjectTag): string {
    return draftTagName(tag, tagDraftState());
  }

  function tagColorDraftValue(tag: ProjectTag): EventColor {
    return draftTagColor(tag, tagDraftState());
  }

  const nextUnusedStatusColor = createProjectSettingsColorAllocator({
    entries: () => statuses, color: statusColorDraftValue, fallback: NEW_STATUS_FIRST_COLOR,
  });
  const nextUnusedPriorityColor = createProjectSettingsColorAllocator({
    entries: () => priorities, color: priorityColorDraftValue, fallback: NEW_PRIORITY_FIRST_COLOR,
  });
  const nextUnusedTagColor = createProjectSettingsColorAllocator({
    entries: () => projectTags, color: tagColorDraftValue, fallback: NEW_TAG_FIRST_COLOR,
  });

  function tagNameExists(name: string, ignoredTagId?: string): boolean {
    return tagNameTaken({
      tags: projectTags,
      name,
      ignoredTagId,
    });
  }

  /** Drop any row drag in progress, for when the draft reloads or the panel closes. */
  function resetReorderDrags(): void {
    for (const sortable of [statusSortable, prioritySortable, tagSortable, customFieldSortable, customFieldOptionSortable]) {
      sortable.reset();
    }
  }

  function statusNameDraftValue(status: ProjectStatus): string {
    return draftStatusName(status, statusDraftState());
  }

  function statusCategoryDraftValue(status: ProjectStatus): ProjectStatusCategory {
    return draftStatusCategory(status, statusDraftState());
  }

  function statusColorDraftValue(status: ProjectStatus): EventColor {
    return draftStatusColor(status, statusDraftState());
  }

  function setStatusColor(statusId: string, color: EventColor | undefined): void {
    if (color === undefined) return;
    sessionState.statusColorDrafts = {
      ...sessionState.statusColorDrafts,
      [statusId]: color,
    };
  }

  function setNewStatusColor(color: EventColor | undefined): void {
    if (color !== undefined) sessionState.newStatusColor = color;
  }

  function statusTaskCount(status: ProjectStatus): number {
    return projects
      .tasksForProjectIncludingArchived(status.projectId)
      .filter((task) => task.statusId === status.id)
      .length;
  }

  function statusDeleteDisabled(status: ProjectStatus): boolean {
    return statuses.length <= 1 || statusTaskCount(status) > 0;
  }

  function statusDeleteTitle(status: ProjectStatus): string {
    if (statuses.length <= 1) return t("projects.settings.deleteStatusBlockedLast");
    if (statusTaskCount(status) > 0) return t("projects.settings.deleteStatusBlockedTasks");
    return t("projects.settings.deleteStatus", status.name);
  }

  function requestDeleteStatus(status: ProjectStatus): void {
    if (statusDeleteDisabled(status)) return;
    pendingDeleteStatusId = status.id;
  }

  function cancelDeleteStatus(): void {
    pendingDeleteStatusId = null;
  }

  async function confirmDeleteStatus(): Promise<void> {
    if (!pendingDeleteStatus) return;
    const status = pendingDeleteStatus;
    pendingDeleteStatusId = null;
    sessionState.projectSettingsError = null;
    try {
      structure.removeStatus(status.id);
      const remainingNames = { ...sessionState.statusNameDrafts };
      const remainingCategories = { ...sessionState.statusCategoryDrafts };
      const remainingColors = { ...sessionState.statusColorDrafts };
      delete remainingNames[status.id];
      delete remainingCategories[status.id];
      delete remainingColors[status.id];
      sessionState.statusNameDrafts = remainingNames;
      sessionState.statusCategoryDrafts = remainingCategories;
      sessionState.statusColorDrafts = remainingColors;
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.settings.statusDeleteFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  function priorityNameDraftValue(priority: ProjectPriorityConfig): string {
    return draftPriorityName(priority, priorityDraftState());
  }

  function priorityColorDraftValue(priority: ProjectPriorityConfig): EventColor {
    return draftPriorityColor(priority, priorityDraftState());
  }

  function setPriorityColor(priorityId: string, color: EventColor | undefined): void {
    if (color === undefined) return;
    sessionState.priorityColorDrafts = {
      ...sessionState.priorityColorDrafts,
      [priorityId]: color,
    };
  }

  function setNewPriorityColor(color: EventColor | undefined): void {
    if (color !== undefined) sessionState.newPriorityColor = color;
  }

  function priorityTaskCount(priority: ProjectPriorityConfig): number {
    return projects
      .tasksForProjectIncludingArchived(priority.projectId)
      .filter((task) => task.priority === priority.id)
      .length;
  }

  function priorityDeleteDisabled(priority: ProjectPriorityConfig): boolean {
    return priorities.length <= 1 || priorityTaskCount(priority) > 0;
  }

  function priorityDeleteTitle(priority: ProjectPriorityConfig): string {
    if (priorities.length <= 1) return t("projects.settings.deletePriorityBlockedLast");
    if (priorityTaskCount(priority) > 0) return t("projects.settings.deletePriorityBlockedTasks");
    return t("projects.settings.deletePriority", priority.name);
  }

  function requestDeletePriority(priority: ProjectPriorityConfig): void {
    if (priorityDeleteDisabled(priority)) return;
    pendingDeletePriorityId = priority.id;
  }

  function cancelDeletePriority(): void {
    pendingDeletePriorityId = null;
  }

  async function confirmDeletePriority(): Promise<void> {
    if (!pendingDeletePriority) return;
    const priority = pendingDeletePriority;
    pendingDeletePriorityId = null;
    sessionState.projectSettingsError = null;
    try {
      structure.removePriority(priority.id);
      const remainingNames = { ...sessionState.priorityNameDrafts };
      const remainingColors = { ...sessionState.priorityColorDrafts };
      delete remainingNames[priority.id];
      delete remainingColors[priority.id];
      sessionState.priorityNameDrafts = remainingNames;
      sessionState.priorityColorDrafts = remainingColors;
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.settings.priorityDeleteFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function submitPriority(): Promise<void> {
    if (!selectedProjectId) return;
    const name = sessionState.newPriorityName.trim();
    const createdColor = sessionState.newPriorityColor;
    if (!name) {
      sessionState.projectSettingsError = t("projects.settings.priorityNameRequired");
      return;
    }
    sessionState.projectSettingsError = null;
    try {
      structure.addPriority(selectedProjectId, name, createdColor);
      sessionState.newPriorityName = "";
      sessionState.newPriorityColor = nextUnusedPriorityColor(
        nextProjectSettingsPaletteColor(createdColor, NEW_PRIORITY_FIRST_COLOR),
        createdColor,
      );
      await scrollToNewPriorityRow();
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.settings.prioritySaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function submitStatus(): Promise<void> {
    if (!selectedProjectId) return;
    const name = sessionState.newStatusName.trim();
    const createdColor = sessionState.newStatusColor;
    if (!name) {
      sessionState.projectSettingsError = t("projects.settings.statusNameRequired");
      return;
    }
    sessionState.projectSettingsError = null;
    try {
      structure.addStatus(selectedProjectId, name, sessionState.newStatusCategory, createdColor);
      sessionState.newStatusName = "";
      sessionState.newStatusCategory = "active";
      sessionState.newStatusColor = nextUnusedStatusColor(
        nextProjectSettingsPaletteColor(createdColor, NEW_STATUS_FIRST_COLOR),
        createdColor,
      );
      await scrollToNewStatusRow();
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.settings.statusSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  function setTagColor(tagId: string, color: EventColor | undefined): void {
    if (color === undefined) return;
    sessionState.tagColorDrafts = {
      ...sessionState.tagColorDrafts,
      [tagId]: color,
    };
  }

  function setNewTagColor(color: EventColor | undefined): void {
    if (color !== undefined) sessionState.newTagColor = color;
  }

  async function submitTag(): Promise<void> {
    if (!selectedProjectId) return;
    const name = sessionState.newTagName.trim();
    const createdColor = sessionState.newTagColor;
    if (!name) {
      sessionState.projectSettingsError = t("projects.settings.tagNameRequired");
      return;
    }
    if (tagNameExists(name)) {
      sessionState.projectSettingsError = t("projects.settings.tagNameExists");
      return;
    }
    sessionState.projectSettingsError = null;
    try {
      structure.addTag(selectedProjectId, name, createdColor);
      sessionState.newTagName = "";
      sessionState.newTagColor = nextUnusedTagColor(
        nextProjectSettingsPaletteColor(createdColor, NEW_TAG_FIRST_COLOR),
        createdColor,
      );
      await scrollToNewTagRow();
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.settings.tagSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function moveTagByDirection(tag: ProjectTag, direction: -1 | 1): Promise<void> {
    sessionState.projectSettingsError = null;
    try {
      await structure.moveTag(tag, direction);
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.settings.tagSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  function requestDeleteTag(tag: ProjectTag): void {
    pendingDeleteTagId = tag.id;
  }

  function cancelDeleteTag(): void {
    pendingDeleteTagId = null;
  }

  async function confirmDeleteTag(): Promise<void> {
    if (!pendingDeleteTag) return;
    const tag = pendingDeleteTag;
    pendingDeleteTagId = null;
    sessionState.projectSettingsError = null;
    try {
      structure.removeTag(tag.id);
      const remainingNames = { ...sessionState.tagNameDrafts };
      const remainingColors = { ...sessionState.tagColorDrafts };
      delete remainingNames[tag.id];
      delete remainingColors[tag.id];
      sessionState.tagNameDrafts = remainingNames;
      sessionState.tagColorDrafts = remainingColors;
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.settings.tagDeleteFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  const setNewCustomFieldOptionDraftName = customFields.setNewOptionName;
  const removeNewCustomFieldOptionDraft = customFields.removeNewOption;
  const submitNewCustomFieldOptionDraft = customFields.submitNewOption;
  const submitCustomFieldCreateDraftOption = customFields.submitCreateDraftOption;
  const submitCustomField = customFields.submitField;

  async function moveCustomFieldByDirection(field: ProjectCustomField, direction: -1 | 1): Promise<void> {
    sessionState.projectSettingsError = null;
    try {
      await structure.moveCustomField(field, direction);
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.customFields.saveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  const requestDeleteCustomField = customFields.requestDeleteField;
  const cancelDeleteCustomField = customFields.cancelDeleteField;
  const confirmDeleteCustomField = customFields.removeField;
  const submitCustomFieldOption = customFields.submitOption;

  async function moveCustomFieldOptionByDirection(option: ProjectCustomFieldOption, direction: -1 | 1): Promise<void> {
    sessionState.projectSettingsError = null;
    try {
      await structure.moveCustomFieldOption(option, direction);
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.customFields.optionSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  const requestDeleteCustomFieldOption = customFields.requestDeleteOption;
  const cancelDeleteCustomFieldOption = customFields.cancelDeleteOption;
  const confirmDeleteCustomFieldOption = customFields.removeOption;

  async function moveStatusByDirection(status: ProjectStatus, direction: -1 | 1): Promise<void> {
    sessionState.projectSettingsError = null;
    try {
      await structure.moveStatus(status, direction);
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.settings.statusSaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function movePriorityByDirection(priority: ProjectPriorityConfig, direction: -1 | 1): Promise<void> {
    sessionState.projectSettingsError = null;
    try {
      await structure.movePriority(priority, direction);
    } catch (error) {
      sessionState.projectSettingsError = t(
        "projects.settings.prioritySaveFailed",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  async function saveProjectSettings(): Promise<void> {
    if (!selectedProject || sessionState.projectSettingsSaving || settingsOperationsBusy()) return;
    const savingProjectId = selectedProject.id;
    const folderDraft = workingFoldersSection;
    const musicUpdate = musicAssignmentsDirty
      ? { assignments: persistedMusicAssignmentDrafts(musicAssignments), updatedAt: Date.now() }
      : undefined;
    const saved = await session.save(
      selectedProject,
      new Set(visibleProjectGroups.map((group) => group.id)),
      sessionCollections(),
      musicUpdate,
      async () => {
        await structure.save(sessionState);
        await folderDraft?.saveDraft();
      },
      () => {
        if (musicUpdate) savedMusicAssignments = completeMusicAssignmentDrafts(musicAssignments);
      },
    );
    if (saved) {
      const project = projects.projectById(savingProjectId);
      if (project) {
        structure.load(canonicalCollections(project.id));
        session.load(project, sessionCollections(), initialCreateColors());
      }
    }
  }

  /** Report whether working-folder operations or row moves are pending, which blocks committing the settings snapshot. */
  function settingsOperationsBusy(): boolean {
    return workingFoldersBusy || statusSortable.pending || prioritySortable.pending || tagSortable.pending
      || customFieldSortable.pending || customFieldOptionSortable.pending;
  }

</script>

{#if selectedProject}
<ProjectSettingsPanelShell
  {presentation}
  {draftReady}
  dirty={settingsDirty}
  saving={sessionState.projectSettingsSaving}
  busy={settingsOperationsBusy()}
  error={sessionState.projectSettingsError}
  title={t("projects.settings.title")}
  discardLabel={t("projects.settings.discard")}
  closeLabel={t("projects.settings.close")}
  saveLabel={t("projects.settings.save")}
  onDiscard={discardProjectSettings}
  onClose={closeProjectSettings}
  onSave={() => { void saveProjectSettings(); }}
  bind:scrollElement={settingsScrollElement}
>
          <ProjectSettingsIdentitySection
            bind:projectNameDraft={sessionState.projectDraft.name}
            bind:projectGroupDraft={sessionState.projectDraft.groupId}
            bind:projectStatusDraft={sessionState.projectDraft.status}
            bind:projectIconDraft={sessionState.projectDraft.icon}
            {projectGroupOptions}
            {lifecycleOptions}
            identityLocked={selectedProjectIdentityLocked}
            {setLifecycleStatus}
          />

          <div class="h-px bg-border/70" aria-hidden="true"></div>

          <ProjectSettingsMembersSection projectId={selectedProject.id} projectName={selectedProject.name} />

          <div class="h-px bg-border/70" aria-hidden="true"></div>

          {#if workingFoldersAvailable}
            <ProjectSettingsWorkingFoldersSection
              bind:this={workingFoldersSection}
              projectId={selectedProject.id}
              saving={sessionState.projectSettingsSaving}
              onDirtyChange={(dirty) => { workingFoldersDirty = dirty; }}
              onBusyChange={(busy) => { workingFoldersBusy = busy; }}
            />

            <div class="h-px bg-border/70" aria-hidden="true"></div>
          {/if}

          <ProjectSettingsDefaultsSection
            theme={theme.current}
            pomodoroOptions={PROJECT_POMODORO_PRESET_ORDER}
            {pomodoroPresetLabel}
            projectDefaultEventNamePlaceholder={selectedProjectIdentityLocked ? selectedProject.name : undefined}
            bind:projectColorDraft={sessionState.projectDraft.color}
            bind:projectDefaultEventNameDraft={sessionState.projectDraft.defaultEventName}
            bind:projectEventTimeModeDraft={sessionState.projectDraft.defaultEventTimeMode}
            bind:projectDurationDraft={sessionState.projectDraft.defaultEventDurationMinutes}
            bind:projectPomodoroModeDraft={sessionState.projectDraft.defaultPomodoroMode}
            bind:projectPomodoroPresetDraft={sessionState.projectDraft.defaultPomodoroPresetKey}
            bind:projectPomodoroFocusDraft={sessionState.projectDraft.defaultPomodoroFocusMinutes}
            bind:projectPomodoroShortBreakDraft={sessionState.projectDraft.defaultPomodoroShortBreakMinutes}
            bind:projectPomodoroLongBreakDraft={sessionState.projectDraft.defaultPomodoroLongBreakMinutes}
            bind:projectPomodoroLongBreakAfterFocusDraft={sessionState.projectDraft.defaultPomodoroLongBreakAfterFocusCount}
            bind:projectIdleSettingsSourceDraft={sessionState.projectDraft.defaultIdleSettingsSource}
            bind:projectIdlePauseEnabledDraft={sessionState.projectDraft.defaultIdlePauseEnabled}
            bind:projectIdleThresholdMinutesDraft={sessionState.projectDraft.defaultIdleThresholdMinutes}
            {musicAssignments}
            {musicPlaylists}
            onMusicAssignmentsChange={(assignments) => { musicAssignments = assignments; }}
            loadingMusicPlaylists={musicAssignmentsLoading}
            {musicAssignmentsError}
            musicAssignmentsDisabled={sessionState.projectSettingsSaving || musicAssignmentsProjectId !== selectedProject.id}
            {musicAssignmentsAvailable}
            {idleDetectionAvailable}
            onRetryMusicAssignments={() => (selectedProject ? loadMusicAssignments(selectedProject.id) : undefined)}
          />

          <div class="h-px bg-border/70" aria-hidden="true"></div>

          <ProjectSettingsTagsSection
            theme={theme.current}
            tags={projectTags}
            tagSortable={tagSortable}
            bind:newTagRowElement
            bind:newTagName={sessionState.newTagName}
            bind:newTagColor={sessionState.newTagColor}
            {moveTagByDirection}
            {tagColorDraftValue}
            {setTagColor}
            {tagNameDraftValue}
            setTagNameDraft={(tagId, name) => {
              sessionState.tagNameDrafts = {
                ...sessionState.tagNameDrafts,
                [tagId]: name,
              };
            }}
            {requestDeleteTag}
            {setNewTagColor}
            {submitTag}
          />

          <div class="h-px bg-border/70" aria-hidden="true"></div>

          <ProjectSettingsCustomFieldsSection
            {projectCustomFields}
            {customFieldOptions}
            {customFieldOptionCreateDraftRows}
            bind:customFieldNameDrafts={sessionState.customFieldNameDrafts}
            bind:customFieldOptionNameDrafts={sessionState.customFieldOptionNameDrafts}
            bind:customFieldCreateDraftRows={sessionState.customFieldCreateDraftRows}
            bind:newCustomFieldName={sessionState.newCustomFieldName}
            bind:newCustomFieldType={sessionState.newCustomFieldType}
            bind:newCustomFieldOptionDrafts={sessionState.newCustomFieldOptionDrafts}
            bind:newCustomFieldOptionRows={sessionState.newCustomFieldOptionRows}
            bind:newCustomFieldOptionName={sessionState.newCustomFieldOptionName}
            bind:newCustomFieldRowElement
            customFieldSortable={customFieldSortable}
            customFieldOptionSortable={customFieldOptionSortable}
            {moveCustomFieldByDirection}
            {requestDeleteCustomField}
            {moveCustomFieldOptionByDirection}
            {requestDeleteCustomFieldOption}
            {setCustomFieldOptionCreateDraftName}
            {removeCustomFieldOptionCreateDraft}
            {setCustomFieldCreateDraftName}
            {setCustomFieldCreateDraftOptionName}
            {setCustomFieldCreateDraftPendingOptionName}
            {removeCustomFieldCreateDraft}
            {removeCustomFieldCreateDraftOption}
            {submitCustomFieldOption}
            {submitCustomFieldCreateDraftOption}
            {submitCustomField}
            {setNewCustomFieldOptionDraftName}
            {removeNewCustomFieldOptionDraft}
            {submitNewCustomFieldOptionDraft}
          />

          <div class="h-px bg-border/70" aria-hidden="true"></div>

          <ProjectSettingsStatusesSection
            theme={theme.current}
            {statuses}
            {statusCategoryOptions}
            statusSortable={statusSortable}
            bind:newStatusRowElement
            bind:newStatusName={sessionState.newStatusName}
            bind:newStatusColor={sessionState.newStatusColor}
            bind:newStatusCategory={sessionState.newStatusCategory}
            {moveStatusByDirection}
            {statusColorDraftValue}
            {setStatusColor}
            {statusNameDraftValue}
            setStatusNameDraft={(statusId, name) => {
              sessionState.statusNameDrafts = {
                ...sessionState.statusNameDrafts,
                [statusId]: name,
              };
            }}
            {statusCategoryDraftValue}
            {setStatusCategory}
            {statusDeleteDisabled}
            {statusDeleteTitle}
            {requestDeleteStatus}
            {setNewStatusColor}
            {setNewStatusCategory}
            {submitStatus}
          />

          <div class="h-px bg-border/70" aria-hidden="true"></div>

          <ProjectSettingsPrioritiesSection
            theme={theme.current}
            {priorities}
            prioritySortable={prioritySortable}
            bind:newPriorityRowElement
            bind:newPriorityName={sessionState.newPriorityName}
            bind:newPriorityColor={sessionState.newPriorityColor}
            {movePriorityByDirection}
            {priorityColorDraftValue}
            {setPriorityColor}
            {priorityNameDraftValue}
            setPriorityNameDraft={(priorityId, name) => {
              sessionState.priorityNameDrafts = {
                ...sessionState.priorityNameDrafts,
                [priorityId]: name,
              };
            }}
            {priorityDeleteDisabled}
            {priorityDeleteTitle}
            {requestDeletePriority}
            {setNewPriorityColor}
            {submitPriority}
          />
</ProjectSettingsPanelShell>
{/if}

<ProjectSettingsDeleteDialogs
  {pendingDeleteStatus}
  {pendingDeletePriority}
  {pendingDeleteTag}
  {pendingDeleteCustomField}
  {pendingDeleteCustomFieldOption}
  customFieldForOption={fieldForCustomFieldOption}
  onConfirmDeleteStatus={() => { void confirmDeleteStatus(); }}
  onCancelDeleteStatus={cancelDeleteStatus}
  onConfirmDeletePriority={() => { void confirmDeletePriority(); }}
  onCancelDeletePriority={cancelDeletePriority}
  onConfirmDeleteTag={() => { void confirmDeleteTag(); }}
  onCancelDeleteTag={cancelDeleteTag}
  onConfirmDeleteCustomField={() => { void confirmDeleteCustomField(); }}
  onCancelDeleteCustomField={cancelDeleteCustomField}
  onConfirmDeleteCustomFieldOption={() => { void confirmDeleteCustomFieldOption(); }}
  onCancelDeleteCustomFieldOption={cancelDeleteCustomFieldOption}
/>
