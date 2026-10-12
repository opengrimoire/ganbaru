import { FALLBACK_COLOR_INDEX, type EventColor } from "$lib/calendar/types";
import {
  emptyProjectDraft,
  projectDraftDirty,
  projectDraftFromProject,
  projectUpdateFromDraft,
  type ProjectDraft,
  type ProjectDraftError,
} from "./project-draft";
import {
  prioritySaveDrafts,
  priorityDraftDirty,
  statusSaveDrafts,
  statusDraftDirty,
  tagSaveDrafts,
  tagDraftDirty,
} from "./collection-drafts";
import {
  customFieldOptionSaveDrafts,
  customFieldSaveDrafts,
  customFieldDraftDirty,
  newCustomFieldOptionDraftRows,
  customFieldOptionDraftDirty,
  type NewCustomFieldDraft,
  type NewCustomFieldOptionDraft,
  type CustomFieldDraftError,
} from "./custom-field-drafts";
import { projectHasLockedSystemIdentity } from "$lib/projects/system-defaults";
import { getLocalization } from "$lib/i18n/translator.svelte";
import { getProjects } from "$lib/stores/projects.svelte";
import {
  PROJECT_TAG_DEFAULT_COLOR,
  type ProjectCustomFieldType,
  type Project,
  type ProjectCustomField,
  type ProjectCustomFieldOption,
  type ProjectPriorityConfig,
  type ProjectStatus,
  type ProjectStatusCategory,
  type ProjectTag,
} from "$lib/projects/types";
import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";

export interface ProjectSettingsSessionState {
  projectDraftId: string | null;
  projectDraftUpdatedAt: string | null;
  projectDraft: ProjectDraft;
  projectSettingsSaving: boolean;
  projectSettingsError: string | null;
  statusNameDrafts: Record<string, string>;
  statusCategoryDrafts: Record<string, ProjectStatusCategory>;
  statusColorDrafts: Record<string, EventColor>;
  priorityNameDrafts: Record<string, string>;
  priorityColorDrafts: Record<string, EventColor>;
  tagNameDrafts: Record<string, string>;
  tagColorDrafts: Record<string, EventColor>;
  customFieldNameDrafts: Record<string, string>;
  customFieldOptionNameDrafts: Record<string, string>;
  customFieldOptionDraftRowsByField: Record<string, NewCustomFieldOptionDraft[]>;
  customFieldCreateDraftRows: NewCustomFieldDraft[];
  newStatusName: string;
  newStatusCategory: ProjectStatusCategory;
  newStatusColor: EventColor;
  newPriorityName: string;
  newPriorityColor: EventColor;
  newTagName: string;
  newTagColor: EventColor;
  newCustomFieldName: string;
  newCustomFieldType: ProjectCustomFieldType;
  newCustomFieldOptionDrafts: Record<string, string>;
  newCustomFieldOptionRows: NewCustomFieldOptionDraft[];
  newCustomFieldOptionName: string;
}

export interface ProjectSettingsSessionCollections {
  statuses: readonly ProjectStatus[];
  priorities: readonly ProjectPriorityConfig[];
  tags: readonly ProjectTag[];
  customFields: readonly ProjectCustomField[];
  optionsForField: (fieldId: string) => readonly ProjectCustomFieldOption[];
}

export interface ProjectSettingsSessionCreateColors {
  status: EventColor;
  priority: EventColor;
  tag: EventColor;
}

export interface ProjectSettingsSessionOptions {
  onRevealInactive: () => void;
  projects?: ReturnType<typeof getProjects>;
  translate?: ReturnType<typeof getLocalization>["t"];
}

export interface ProjectSettingsMusicUpdate {
  assignments: MusicContextAssignmentDraft[];
  updatedAt: number;
}

/**
 * Owns the complete project settings draft session independently from section rendering.
 */
export function createProjectSettingsSession(options: ProjectSettingsSessionOptions) {
  const projects = options.projects ?? getProjects();
  const t = options.translate ?? getLocalization().t;
  const state = $state<ProjectSettingsSessionState>({
    projectDraftId: null,
    projectDraftUpdatedAt: null,
    projectDraft: emptyProjectDraft(),
    projectSettingsSaving: false,
    projectSettingsError: null,
    statusNameDrafts: {},
    statusCategoryDrafts: {},
    statusColorDrafts: {},
    priorityNameDrafts: {},
    priorityColorDrafts: {},
    tagNameDrafts: {},
    tagColorDrafts: {},
    customFieldNameDrafts: {},
    customFieldOptionNameDrafts: {},
    customFieldOptionDraftRowsByField: {},
    customFieldCreateDraftRows: [],
    newStatusName: "",
    newStatusCategory: "active",
    newStatusColor: 8,
    newPriorityName: "",
    newPriorityColor: 13,
    newTagName: "",
    newTagColor: PROJECT_TAG_DEFAULT_COLOR,
    newCustomFieldName: "",
    newCustomFieldType: "text",
    newCustomFieldOptionDrafts: {},
    newCustomFieldOptionRows: [],
    newCustomFieldOptionName: "",
  });

  function load(
    project: Project,
    collections: ProjectSettingsSessionCollections,
    createColors?: ProjectSettingsSessionCreateColors,
  ): void {
    state.projectDraftId = project.id;
    state.projectDraftUpdatedAt = project.updatedAt;
    state.projectDraft = projectDraftFromProject(project);
    state.projectSettingsError = null;
    state.statusNameDrafts = Object.fromEntries(
      collections.statuses.map((status) => [status.id, status.name]),
    );
    state.statusCategoryDrafts = Object.fromEntries(
      collections.statuses.map((status) => [status.id, status.category]),
    );
    state.statusColorDrafts = Object.fromEntries(
      collections.statuses.map((status) => [status.id, status.color]),
    );
    state.priorityNameDrafts = Object.fromEntries(
      collections.priorities.map((priority) => [priority.id, priority.name]),
    );
    state.priorityColorDrafts = Object.fromEntries(
      collections.priorities.map((priority) => [priority.id, priority.color]),
    );
    state.tagNameDrafts = Object.fromEntries(
      collections.tags.map((tag) => [tag.id, tag.name]),
    );
    state.tagColorDrafts = Object.fromEntries(
      collections.tags.map((tag) => [tag.id, tag.color ?? FALLBACK_COLOR_INDEX]),
    );
    state.customFieldNameDrafts = Object.fromEntries(
      collections.customFields.map((field) => [field.id, field.name]),
    );
    state.customFieldOptionNameDrafts = Object.fromEntries(
      collections.customFields.flatMap((field) =>
        collections.optionsForField(field.id).map((option) => [option.id, option.name]),
      ),
    );
    state.customFieldOptionDraftRowsByField = {};
    state.customFieldCreateDraftRows = [];
    state.newStatusName = "";
    state.newStatusCategory = "active";
    state.newStatusColor = createColors?.status ?? state.newStatusColor;
    state.newPriorityName = "";
    state.newPriorityColor = createColors?.priority ?? state.newPriorityColor;
    state.newTagName = "";
    state.newTagColor = createColors?.tag ?? state.newTagColor;
    state.newCustomFieldName = "";
    state.newCustomFieldType = "text";
    state.newCustomFieldOptionDrafts = {};
    state.newCustomFieldOptionRows = [];
    state.newCustomFieldOptionName = "";
  }

  function synchronizeLockedIdentity(project: Project): void {
    if (!projectHasLockedSystemIdentity(project)) return;
    if (state.projectDraftId !== project.id || state.projectDraft.name === project.name) return;
    state.projectDraft.name = project.name;
    state.projectDraft.groupId = project.groupId;
  }

  function discard(
    project: Project,
    collections: ProjectSettingsSessionCollections,
    createColors?: ProjectSettingsSessionCreateColors,
  ): void {
    load(project, collections, createColors);
  }

  function dirty(project: Project, collections: ProjectSettingsSessionCollections): boolean {
    const statusState = {
      nameDrafts: state.statusNameDrafts,
      categoryDrafts: state.statusCategoryDrafts,
      colorDrafts: state.statusColorDrafts,
      fallbackColor: FALLBACK_COLOR_INDEX,
    };
    const priorityState = {
      nameDrafts: state.priorityNameDrafts,
      colorDrafts: state.priorityColorDrafts,
      fallbackColor: FALLBACK_COLOR_INDEX,
    };
    const tagState = {
      nameDrafts: state.tagNameDrafts,
      colorDrafts: state.tagColorDrafts,
      fallbackColor: FALLBACK_COLOR_INDEX,
    };
    return projectDraftDirty(project, state.projectDraft)
      || collections.statuses.some((status) => statusDraftDirty(status, statusState))
      || collections.priorities.some((priority) => priorityDraftDirty(priority, priorityState))
      || collections.tags.some((tag) => tagDraftDirty(tag, tagState))
      || collections.customFields.some((field) =>
        customFieldDraftDirty(field, state.customFieldNameDrafts)
      )
      || collections.customFields.some((field) =>
        collections.optionsForField(field.id).some((option) =>
          customFieldOptionDraftDirty(option, state.customFieldOptionNameDrafts)
        )
      )
      || collections.customFields.some((field) =>
        newCustomFieldOptionDraftRows(
          state.customFieldOptionDraftRowsByField,
          field.id,
        ).length > 0
      )
      || state.customFieldCreateDraftRows.length > 0;
  }

  function customFieldDraftErrorMessage(error: CustomFieldDraftError): string {
    if (error === "name_required") return t("projects.customFields.nameRequired");
    if (error === "name_exists") return t("projects.customFields.nameExists");
    if (error === "option_name_exists") return t("projects.customFields.optionNameExists");
    return t("projects.customFields.optionNameRequired");
  }

  function setCustomFieldDraftError(error: CustomFieldDraftError): void {
    state.projectSettingsError = customFieldDraftErrorMessage(error);
  }

  function projectDraftErrorMessage(error: ProjectDraftError): string {
    if (error === "name_required") return t("projects.settings.nameRequired");
    if (error === "group_required") return t("projects.settings.groupRequired");
    return t("projects.settings.invalidDuration");
  }

  function nextProjectSortOrderForGroup(groupId: string, excludeProjectId: string): number {
    return Math.max(
      0,
      ...projects.projectsForGroupIncludingInactive(groupId)
        .filter((project) => project.id !== excludeProjectId)
        .map((project) => project.sortOrder),
    ) + 1000;
  }

  /** Validate the complete draft before committing folder, collection, project, and playlist changes. */
  async function save(
    project: Project,
    visibleGroupIds: ReadonlySet<string>,
    collections: ProjectSettingsSessionCollections,
    musicUpdate?: ProjectSettingsMusicUpdate,
    commitDraft?: () => Promise<void>,
    onProjectSaved?: () => void,
  ): Promise<boolean> {
    const shouldUpdateProject = projectDraftDirty(project, state.projectDraft)
      || Boolean(musicUpdate);
    const projectUpdateResult = shouldUpdateProject
      ? projectUpdateFromDraft({
          project,
          draft: state.projectDraft,
          visibleGroupIds,
          nextSortOrderForGroup: nextProjectSortOrderForGroup,
        })
      : null;
    if (projectUpdateResult && !projectUpdateResult.ok) {
      state.projectSettingsError = projectDraftErrorMessage(projectUpdateResult.error);
      return false;
    }
    const statusResult = statusSaveDrafts(collections.statuses, {
      nameDrafts: state.statusNameDrafts,
      categoryDrafts: state.statusCategoryDrafts,
      colorDrafts: state.statusColorDrafts,
      fallbackColor: FALLBACK_COLOR_INDEX,
    });
    if (!statusResult.ok) {
      state.projectSettingsError = t("projects.settings.statusNameRequired");
      return false;
    }
    const priorityResult = prioritySaveDrafts(collections.priorities, {
      nameDrafts: state.priorityNameDrafts,
      colorDrafts: state.priorityColorDrafts,
      fallbackColor: FALLBACK_COLOR_INDEX,
    });
    if (!priorityResult.ok) {
      state.projectSettingsError = t("projects.settings.priorityNameRequired");
      return false;
    }
    const tagResult = tagSaveDrafts(collections.tags, {
      nameDrafts: state.tagNameDrafts,
      colorDrafts: state.tagColorDrafts,
      fallbackColor: FALLBACK_COLOR_INDEX,
    });
    if (!tagResult.ok) {
      state.projectSettingsError = tagResult.error === "name_exists"
        ? t("projects.settings.tagNameExists")
        : t("projects.settings.tagNameRequired");
      return false;
    }
    const customFieldResult = customFieldSaveDrafts({
      fields: collections.customFields,
      fieldNameDrafts: state.customFieldNameDrafts,
      createDraftRows: state.customFieldCreateDraftRows,
    });
    if (!customFieldResult.ok) {
      state.projectSettingsError = customFieldDraftErrorMessage(customFieldResult.error);
      return false;
    }
    const customFieldOptionResult = customFieldOptionSaveDrafts({
      fields: collections.customFields,
      fieldOptions: collections.optionsForField,
      optionNameDrafts: state.customFieldOptionNameDrafts,
      optionCreateDraftRowsByField: state.customFieldOptionDraftRowsByField,
    });
    if (!customFieldOptionResult.ok) {
      state.projectSettingsError = customFieldDraftErrorMessage(customFieldOptionResult.error);
      return false;
    }

    const shouldRevealInactive = shouldUpdateProject && state.projectDraft.status !== "active";
    state.projectSettingsSaving = true;
    state.projectSettingsError = null;
    try {
      await commitDraft?.();
      if (projectUpdateResult?.ok) {
        await projects.updateProject({
          ...projectUpdateResult.value,
          ...(musicUpdate ? {
            musicAssignments: musicUpdate.assignments,
            musicAssignmentsUpdatedAt: musicUpdate.updatedAt,
          } : {}),
        });
        onProjectSaved?.();
      }
      for (const draft of statusResult.drafts) {
        await projects.updateStatus(draft.status, {
          name: draft.name,
          category: draft.category,
          color: draft.color,
        });
        state.statusNameDrafts = { ...state.statusNameDrafts, [draft.status.id]: draft.name };
        state.statusCategoryDrafts = {
          ...state.statusCategoryDrafts,
          [draft.status.id]: draft.category,
        };
        state.statusColorDrafts = { ...state.statusColorDrafts, [draft.status.id]: draft.color };
      }
      for (const draft of priorityResult.drafts) {
        await projects.updatePriority(draft.priority, { name: draft.name, color: draft.color });
        state.priorityNameDrafts = {
          ...state.priorityNameDrafts,
          [draft.priority.id]: draft.name,
        };
        state.priorityColorDrafts = {
          ...state.priorityColorDrafts,
          [draft.priority.id]: draft.color,
        };
      }
      for (const draft of tagResult.drafts) {
        await projects.updateTag(draft.tag, { name: draft.name, color: draft.color });
        state.tagNameDrafts = { ...state.tagNameDrafts, [draft.tag.id]: draft.name };
        state.tagColorDrafts = { ...state.tagColorDrafts, [draft.tag.id]: draft.color };
      }
      for (const draft of customFieldResult.value.updates) {
        await projects.updateCustomField(draft.field, { name: draft.name });
        state.customFieldNameDrafts = {
          ...state.customFieldNameDrafts,
          [draft.field.id]: draft.name,
        };
      }
      for (const draft of customFieldResult.value.creates) {
        const createdField = await projects.addCustomField(project.id, draft.name, draft.fieldType);
        if (createdField) {
          state.customFieldNameDrafts = {
            ...state.customFieldNameDrafts,
            [createdField.id]: createdField.name,
          };
          for (const optionName of draft.optionNames) {
            const createdOption = await projects.addCustomFieldOption(createdField.id, optionName);
            if (createdOption) {
              state.customFieldOptionNameDrafts = {
                ...state.customFieldOptionNameDrafts,
                [createdOption.id]: createdOption.name,
              };
            }
          }
        }
        state.customFieldCreateDraftRows = state.customFieldCreateDraftRows.filter(
          (entry) => entry.id !== draft.draftId,
        );
      }
      for (const draft of customFieldOptionResult.value.updates) {
        await projects.updateCustomFieldOption(draft.option, { name: draft.name });
        state.customFieldOptionNameDrafts = {
          ...state.customFieldOptionNameDrafts,
          [draft.option.id]: draft.name,
        };
      }
      for (const draft of customFieldOptionResult.value.creates) {
        const createdOption = await projects.addCustomFieldOption(draft.field.id, draft.name);
        if (createdOption) {
          state.customFieldOptionNameDrafts = {
            ...state.customFieldOptionNameDrafts,
            [createdOption.id]: createdOption.name,
          };
        }
        state.customFieldOptionDraftRowsByField = {
          ...state.customFieldOptionDraftRowsByField,
          [draft.field.id]: (state.customFieldOptionDraftRowsByField[draft.field.id] ?? [])
            .filter((entry) => entry.id !== draft.draftId),
        };
      }
      if (shouldRevealInactive) options.onRevealInactive();
      return true;
    } catch (error) {
      state.projectSettingsError = t(
        "projects.settings.saveFailed",
        error instanceof Error ? error.message : String(error),
      );
      return false;
    } finally {
      state.projectSettingsSaving = false;
    }
  }

  return {
    state,
    load,
    discard,
    synchronizeLockedIdentity,
    dirty,
    setCustomFieldDraftError,
    save,
  };
}
