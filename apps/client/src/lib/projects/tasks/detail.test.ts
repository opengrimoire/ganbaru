import { describe, expect, it } from "vitest";
import {
  projectTaskDetailCustomFieldDirty,
  projectTaskDetailCustomFieldDrafts,
  projectTaskDetailMergeSavedCustomFieldDraft,
  projectTaskDetailCustomFieldRawDraft,
  projectTaskDetailCustomFieldSaveDraft,
  projectTaskDetailDraftDirty,
  projectTaskDetailDraftFromTask,
  projectTaskDetailDraftPatch,
  projectTaskDetailDraftsEqual,
  projectTaskDetailEventLinkDateRange,
  projectTaskNewDetailDraft,
  projectTaskNewDetailDraftHasExtras,
} from "./detail";
import type {
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldType,
  ProjectCustomFieldValue,
  ProjectTask,
} from "$lib/projects/types";

function field(id: string, fieldType: ProjectCustomFieldType): ProjectCustomField {
  return {
    id,
    projectId: "project-a",
    name: id,
    fieldType,
    sortOrder: 1000,
    icon: null,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
  };
}

function option(id: string, fieldId = "field-select"): ProjectCustomFieldOption {
  return {
    id,
    fieldId,
    name: id,
    sortOrder: 1000,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
  };
}

function task(overrides: Partial<ProjectTask> = {}): ProjectTask {
  return {
    id: "task-a",
    projectId: "project-a",
    sectionId: "section-a",
    statusId: "status-a",
    title: "Prepare launch",
    description: "Write notes",
    priority: "normal",
    taskType: "task",
    sectionSortOrder: 1000,
    statusSortOrder: 1000,
    estimateMinutes: 30,
    startDate: "2026-06-21",
    dueDate: "2026-06-22",
    targetEndDate: "2026-06-23",
    blockerReason: "Waiting for review",
    milestone: false,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
    ...overrides,
  };
}

describe("project task detail helpers", () => {
  it("builds and compares base task detail drafts", () => {
    const sourceTask = task();
    const draft = projectTaskDetailDraftFromTask(sourceTask);

    expect(draft).toMatchObject({
      title: "Prepare launch",
      estimateMinutes: "30",
      startDate: "2026-06-21",
      dueDate: "2026-06-22",
      targetEndDate: "2026-06-23",
      changeReason: "",
    });
    expect(projectTaskDetailDraftDirty(sourceTask, draft)).toBe(false);
    expect(projectTaskDetailDraftDirty(sourceTask, { ...draft, title: "Updated" })).toBe(true);
  });

  it("starts a new task draft empty in the given placement", () => {
    const draft = projectTaskNewDetailDraft({ sectionId: "section-a", statusId: "status-a" });

    expect(draft).toMatchObject({
      title: "",
      description: "",
      sectionId: "section-a",
      statusId: "status-a",
      priority: "normal",
      taskType: "task",
      milestone: false,
    });
    expect(projectTaskDetailDraftPatch(draft)).toEqual({ ok: false, reason: "title-required" });
  });

  it("compares detail drafts field by field", () => {
    const draft = projectTaskNewDetailDraft({ sectionId: "section-a", statusId: "status-a" });

    expect(projectTaskDetailDraftsEqual(draft, { ...draft })).toBe(true);
    expect(projectTaskDetailDraftsEqual(draft, { ...draft, description: "Notes" })).toBe(false);
    expect(projectTaskDetailDraftsEqual(draft, { ...draft, milestone: true })).toBe(false);
  });

  it("requires a follow-up update only for fields task creation does not accept", () => {
    const initial = projectTaskNewDetailDraft({ sectionId: "section-a", statusId: "status-a" });
    const placed = { ...initial, title: "Plan", sectionId: "section-b", statusId: "status-b" };

    expect(projectTaskNewDetailDraftHasExtras(initial, placed)).toBe(false);
    expect(projectTaskNewDetailDraftHasExtras(initial, { ...placed, priority: "high" })).toBe(true);
    expect(projectTaskNewDetailDraftHasExtras(initial, { ...placed, dueDate: "2026-10-08" })).toBe(true);
    expect(projectTaskNewDetailDraftHasExtras(initial, { ...placed, changeReason: "Scope" })).toBe(true);
  });

  it("builds validated base task update patches", () => {
    expect(projectTaskDetailDraftPatch({
      ...projectTaskDetailDraftFromTask(task()),
      title: " Updated title ",
      description: " Updated description ",
      estimateMinutes: "45",
      blockerReason: " ",
      changeReason: " Scope changed ",
    })).toEqual({
      ok: true,
      patch: {
        title: "Updated title",
        description: "Updated description",
        sectionId: "section-a",
        statusId: "status-a",
        priority: "normal",
        taskType: "task",
        estimateMinutes: 45,
        startDate: "2026-06-21",
        startTime: undefined,
        dueDate: "2026-06-22",
        dueTime: undefined,
        targetEndDate: "2026-06-23",
        blockerReason: undefined,
        milestone: false,
        changeReason: "Scope changed",
      },
    });
  });

  it("edits start and due times with their dates, dropping a time whose date is cleared", () => {
    const timed = task({ startTime: "09:00", dueTime: "17:30" });
    const draft = projectTaskDetailDraftFromTask(timed);
    expect(draft).toMatchObject({ startTime: "09:00", dueTime: "17:30" });
    expect(projectTaskDetailDraftDirty(timed, draft)).toBe(false);
    expect(projectTaskDetailDraftDirty(timed, { ...draft, dueTime: "18:00" })).toBe(true);

    const patch = projectTaskDetailDraftPatch({ ...draft, startTime: "", dueTime: "18:00" });
    expect(patch.ok && patch.patch).toMatchObject({ startTime: undefined, dueTime: "18:00" });
    expect(patch.ok && "startTime" in patch.patch).toBe(true);

    const cleared = { ...draft, dueDate: "" };
    expect(projectTaskDetailDraftPatch(cleared)).toMatchObject({ ok: true, patch: { dueDate: undefined, dueTime: undefined } });
    expect(projectTaskDetailDraftDirty(task({ startTime: "09:00", dueDate: undefined }), cleared)).toBe(false);
  });

  it("reports invalid base task drafts", () => {
    expect(projectTaskDetailDraftPatch({
      ...projectTaskDetailDraftFromTask(task()),
      title: " ",
    })).toEqual({ ok: false, reason: "title-required" });
    expect(projectTaskDetailDraftPatch({
      ...projectTaskDetailDraftFromTask(task()),
      estimateMinutes: "0",
    })).toEqual({ ok: false, reason: "invalid-estimate" });
    expect(projectTaskDetailDraftPatch({
      ...projectTaskDetailDraftFromTask(task()),
      dueDate: "2026-02-30",
    })).toEqual({ ok: false, reason: "invalid-date" });
  });

  it("builds custom field drafts from stored values", () => {
    const textField = field("field-text", "text");
    const selectField = field("field-select", "select");
    const drafts = projectTaskDetailCustomFieldDrafts({
      fields: [textField, selectField],
      valueForField: (entry): ProjectCustomFieldValue | undefined =>
        entry.id === textField.id
          ? {
            taskId: "task-a",
            fieldId: textField.id,
            textValue: "Launch notes",
            updatedAt: "2026-06-21T00:00:00.000Z",
          }
          : undefined,
      optionValuesForField: (entry) => entry.id === selectField.id ? [option("option-a")] : [],
    });

    expect(drafts.textDrafts).toEqual({ "field-text": "Launch notes", "field-select": "" });
    expect(drafts.selectDrafts).toEqual({ "field-text": "none", "field-select": "option-a" });
  });

  it("detects custom field draft changes by field type", () => {
    const numberField = field("field-number", "number");
    const multiField = field("field-multi", "multi_select");
    const drafts = projectTaskDetailCustomFieldDrafts({
      fields: [numberField, multiField],
      valueForField: (entry): ProjectCustomFieldValue | undefined =>
        entry.id === numberField.id
          ? {
            taskId: "task-a",
            fieldId: numberField.id,
            numberValue: 3,
            updatedAt: "2026-06-21T00:00:00.000Z",
          }
          : undefined,
      optionValuesForField: (entry) =>
        entry.id === multiField.id ? [option("option-a", multiField.id), option("option-b", multiField.id)] : [],
    });

    expect(projectTaskDetailCustomFieldDirty({
      field: numberField,
      drafts,
      value: {
        taskId: "task-a",
        fieldId: numberField.id,
        numberValue: 3,
        updatedAt: "2026-06-21T00:00:00.000Z",
      },
      optionValues: [],
    })).toBe(false);

    drafts.multiDrafts[multiField.id] = ["option-a"];
    expect(projectTaskDetailCustomFieldDirty({
      field: multiField,
      drafts,
      value: undefined,
      optionValues: [option("option-a", multiField.id), option("option-b", multiField.id)],
    })).toBe(true);
  });

  it("parses custom field save drafts", () => {
    const dateField = field("field-date", "date");
    const selectField = field("field-select", "select");
    const drafts = projectTaskDetailCustomFieldDrafts({
      fields: [dateField, selectField],
      valueForField: () => undefined,
      optionValuesForField: () => [],
    });
    drafts.dateDrafts[dateField.id] = "2026-06-21";
    drafts.selectDrafts[selectField.id] = "option-a";

    expect(projectTaskDetailCustomFieldSaveDraft({ field: dateField, drafts })).toEqual({
      ok: true,
      value: {
        textValue: null,
        numberValue: null,
        dateValue: "2026-06-21",
        checkboxValue: null,
        optionIds: [],
      },
    });
    expect(projectTaskDetailCustomFieldSaveDraft({ field: selectField, drafts })).toEqual({
      ok: true,
      value: {
        textValue: null,
        numberValue: null,
        dateValue: null,
        checkboxValue: null,
        optionIds: ["option-a"],
      },
    });
  });

  it("reports invalid custom field save drafts", () => {
    const numberField = field("field-number", "number");
    const dateField = field("field-date", "date");
    const drafts = projectTaskDetailCustomFieldDrafts({
      fields: [numberField, dateField],
      valueForField: () => undefined,
      optionValuesForField: () => [],
    });
    drafts.numberDrafts[numberField.id] = "not a number";
    drafts.dateDrafts[dateField.id] = "2026-02-30";

    expect(projectTaskDetailCustomFieldSaveDraft({ field: numberField, drafts })).toEqual({
      ok: false,
      reason: "invalid-number",
    });
    expect(projectTaskDetailCustomFieldSaveDraft({ field: dateField, drafts })).toEqual({
      ok: false,
      reason: "invalid-date",
    });
  });

  it("merges only the saved custom field draft", () => {
    const savedField = field("field-number", "number");
    const otherField = field("field-other", "text");
    const drafts = projectTaskDetailCustomFieldDrafts({
      fields: [savedField, otherField],
      valueForField: () => undefined,
      optionValuesForField: () => [],
    });
    drafts.numberDrafts[savedField.id] = "03.50";
    drafts.textDrafts[otherField.id] = "Unsaved elsewhere";
    const submittedRawDraft = projectTaskDetailCustomFieldRawDraft({
      field: savedField,
      drafts,
    });

    const merged = projectTaskDetailMergeSavedCustomFieldDraft({
      field: savedField,
      drafts,
      saved: {
        textValue: null,
        numberValue: 3.5,
        dateValue: null,
        checkboxValue: null,
        optionIds: [],
      },
      submittedRawDraft,
      requestGeneration: 2,
      latestRequestGeneration: 2,
    });

    expect(merged.numberDrafts[savedField.id]).toBe("3.5");
    expect(merged.textDrafts[otherField.id]).toBe("Unsaved elsewhere");
    expect(merged.textDrafts).toBe(drafts.textDrafts);
  });

  it("preserves a newer same-field edit while an earlier save resolves", () => {
    const savedField = field("field-number", "number");
    const drafts = projectTaskDetailCustomFieldDrafts({
      fields: [savedField],
      valueForField: () => undefined,
      optionValuesForField: () => [],
    });
    drafts.numberDrafts[savedField.id] = "03.50";
    const submittedRawDraft = projectTaskDetailCustomFieldRawDraft({ field: savedField, drafts });
    drafts.numberDrafts[savedField.id] = "4";

    const merged = projectTaskDetailMergeSavedCustomFieldDraft({
      field: savedField,
      drafts,
      saved: {
        textValue: null,
        numberValue: 3.5,
        dateValue: null,
        checkboxValue: null,
        optionIds: [],
      },
      submittedRawDraft,
      requestGeneration: 1,
      latestRequestGeneration: 1,
    });

    expect(merged).toBe(drafts);
    expect(merged.numberDrafts[savedField.id]).toBe("4");
  });

  it("ignores an older overlapping save even when its raw value matches again", () => {
    const savedField = field("field-number", "number");
    const drafts = projectTaskDetailCustomFieldDrafts({
      fields: [savedField],
      valueForField: () => undefined,
      optionValuesForField: () => [],
    });
    drafts.numberDrafts[savedField.id] = "03.50";
    const submittedRawDraft = projectTaskDetailCustomFieldRawDraft({ field: savedField, drafts });

    const merged = projectTaskDetailMergeSavedCustomFieldDraft({
      field: savedField,
      drafts,
      saved: {
        textValue: null,
        numberValue: 3.5,
        dateValue: null,
        checkboxValue: null,
        optionIds: [],
      },
      submittedRawDraft,
      requestGeneration: 1,
      latestRequestGeneration: 2,
    });

    expect(merged).toBe(drafts);
    expect(merged.numberDrafts[savedField.id]).toBe("03.50");
  });

  it("validates event link date ranges", () => {
    expect(projectTaskDetailEventLinkDateRange({
      startDateDraft: "2026-06-21",
      endDateDraft: "2026-06-22",
    })).toEqual({
      ok: true,
      startDate: "2026-06-21",
      endDate: "2026-06-22",
    });
    expect(projectTaskDetailEventLinkDateRange({
      startDateDraft: "not-a-date",
      endDateDraft: "",
    })).toEqual({ ok: false, reason: "invalid-date" });
    expect(projectTaskDetailEventLinkDateRange({
      startDateDraft: "2026-06-22",
      endDateDraft: "2026-06-21",
    })).toEqual({ ok: false, reason: "invalid-range" });
  });
});
