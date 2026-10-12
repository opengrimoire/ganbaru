import { describe, expect, it, vi } from "vitest";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { ProjectCustomField, ProjectSavedTaskView, ProjectTaskViewRequest } from "$lib/projects/types";
import { defaultProjectListPresentation } from "$lib/projects/list/presentation";
import { PROJECT_TASK_FILTER_DEFAULTS } from "$lib/projects/list/view";
import {
  ProjectTaskQueryController,
  normalizeProjectFilterDate,
  repairProjectTaskQueryReferences,
} from "./task-query-controller.svelte";

type ControllerInput = ConstructorParameters<typeof ProjectTaskQueryController>[0];

/** Isolate query preferences and their native write boundary from the rest of the project store. */
function createController(activeView: "list" | "kanban" = "list") {
  const requests: Array<{ request: ProjectTaskViewRequest; append: boolean }> = [];
  const selection = { projectId: "project-1", mutationRevision: 0, loadError: null as string | null };
  const saveTaskListColumns = vi.fn<ControllerInput["projects"]["saveTaskListColumns"]>().mockResolvedValue(undefined);
  const applySectionCollapseState = vi.fn<ControllerInput["projects"]["applySectionCollapseState"]>().mockResolvedValue(undefined);
  const saveTaskListPresentation = vi.fn<ControllerInput["projects"]["saveTaskListPresentation"]>().mockResolvedValue(undefined);
  const fields: ProjectCustomField[] = [];
  const createdField: ProjectCustomField = { id: "created", projectId: "project-1", name: "Points", fieldType: "number", sortOrder: 100, icon: null, createdAt: "", updatedAt: "" };
  const addCustomField = vi.fn<ControllerInput["projects"]["addCustomField"]>().mockResolvedValue(createdField);
  const duplicateCustomField = vi.fn<ControllerInput["projects"]["duplicateCustomField"]>().mockResolvedValue(createdField);
  const loadTaskView = vi.fn<ControllerInput["projects"]["loadTaskView"]>().mockImplementation(async (request, append = false) => { selection.loadError = null; requests.push({request, append}); });
  const projects = {
    get selectedProject() { return { id: selection.projectId }; },
    get taskMutationRevision() { return selection.mutationRevision; },
    get taskViewError() { return selection.loadError; },
    activeView,
    saveTaskListColumns,
    applySectionCollapseState,
    saveTaskListPresentation,
    addCustomField,
    duplicateCustomField,
    customFieldsForProject: () => fields,
    prioritiesForProject: () => [],
    isProjectDataLoaded: () => true,
    viewPreferences: [],
    taskViewLoading: false,
    taskViewPage: activeView === "list"
      ? { projectId: "project-1", view: "list", nextCursor: "list-cursor", columnCounts: [], columnCalculations: [{ column: "estimate", total: 1000, filled: 900, sum: 5000 }] }
      : { columnCounts: [{ statusId: "status-1", nextCursor: "column-cursor" }] },
    sectionsForProject: () => [{ id: "section-1" }],
    sectionsForProjectIncludingInactive: () => [{ id: "section-1" }],
    loadTaskView,
  } as unknown as ControllerInput["projects"];
  const controller = new ProjectTaskQueryController({
    projects,
    calendar: { sourceEvents: [] } as unknown as ControllerInput["calendar"],
    translate: ((key: string, ...parameters: unknown[]) => parameters.length ? `${key}: ${String(parameters[0])}` : key) as Translate,
  });
  return { controller, requests, selection, saveTaskListColumns, applySectionCollapseState, saveTaskListPresentation, addCustomField, duplicateCustomField, fields, createdField, loadTaskView };
}

/** Describe a saved view that changes columns and section collapse state together. */
function savedView(): ProjectSavedTaskView {
  return {
    ...PROJECT_TASK_FILTER_DEFAULTS,
    customFieldFilters: [],
    id: "view-1",
    projectId: "project-1",
    name: "Weekly work",
    viewId: "list",
    collapsedSectionIds: ["section-1"],
    showArchivedTasks: false,
    visibleColumns: ["due"],
    updatedAt: "",
  };
}

describe("Project task query controller policy", () => {
  it("normalizes valid dates and rejects incomplete dates", () => {
    expect(normalizeProjectFilterDate(" 2026-07-12 ")).toBe("2026-07-12");
    expect(normalizeProjectFilterDate("2026-02-30")).toBeUndefined();
    expect(normalizeProjectFilterDate(" ")).toBeUndefined();
  });

  it("repairs every reference whose project collection disappeared", () => {
    expect(repairProjectTaskQueryReferences({
      sectionFilter: "removed-section",
      tagFilter: "removed-tag",
      customFieldFilters: [
        { fieldId: "kept-field", mode: "filled" },
        { fieldId: "removed-field", mode: "empty" },
      ],
      sortMode: "custom:removed-field",
      sortDirection: "desc",
      sectionIds: new Set(["section-1"]),
      tagIds: new Set(["tag-1"]),
      fieldIds: new Set(["kept-field"]),
      optionIds: new Set(),
    })).toEqual({
      sectionFilter: "all",
      tagFilter: "all",
      customFieldFilters: [{ fieldId: "kept-field", mode: "filled" }],
      sortMode: "manual",
      sortDirection: "asc",
    });
  });

  it("keeps special no-tag filters and live custom sorting", () => {
    expect(repairProjectTaskQueryReferences({
      sectionFilter: "section-1",
      tagFilter: "none",
      customFieldFilters: [{ fieldId: "field-1", mode: "option", optionId: "option-1" }],
      sortMode: "custom:field-1",
      sortDirection: "desc",
      sectionIds: new Set(["section-1"]),
      tagIds: new Set(),
      fieldIds: new Set(["field-1"]),
      optionIds: new Set(["option-1"]),
    })).toMatchObject({
      sectionFilter: "section-1",
      tagFilter: "none",
      sortMode: "custom:field-1",
      sortDirection: "desc",
    });
  });

  it("builds the backend request from canonical state without local-only grouping", () => {
    const { controller } = createController();
    controller.groupBy = "priority";
    controller.dueRangeStart = " 2026-07-01 ";

    const request = controller.request();
    expect(request).toMatchObject({
      projectId: "project-1",
      view: "list",
      pageSize: 100,
      dueRangeStart: "2026-07-01",
      visibleSectionIds: ["section-1"],
    });
    expect(request).not.toHaveProperty("groupBy");
  });

  it("keeps list and Kanban continuation cursors in their distinct request fields", async () => {
    const list = createController("list");
    list.controller.loadNextList(["selected-task"]);
    await vi.waitFor(() => expect(list.requests).toHaveLength(1));
    expect(list.requests[0]).toMatchObject({
      append: true,
      request: { cursor: "list-cursor", columnCursors: {} },
    });

    const kanban = createController("kanban");
    kanban.controller.loadNextKanban(["selected-task"]);
    await vi.waitFor(() => expect(kanban.requests).toHaveLength(1));
    expect(kanban.requests[0]).toMatchObject({
      append: true,
      request: { columnCursors: { "status-1": "column-cursor" } },
    });
    expect(kanban.requests[0].request).not.toHaveProperty("cursor");
  });

  it("restores columns and preserves the view name draft when saving fails, then allows a retry", async () => {
    const { controller, saveTaskListColumns } = createController();
    controller.listColumns = ["status", "due"];
    controller.savedViewNameDraft = "Unsaved view name";
    let rejectWrite!: (error: unknown) => void;
    saveTaskListColumns.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectWrite = reject; }));
    const saving = controller.toggleColumn("status");
    expect(controller.listColumns).toEqual(["due"]);
    expect(controller.listColumnsSaving).toBe(true);
    await controller.toggleColumn("priority");
    expect(saveTaskListColumns).toHaveBeenCalledOnce();
    rejectWrite(new Error("Disk is full"));
    await expect(saving).resolves.toBeUndefined();
    expect(controller.listColumns).toEqual(["status", "due"]);
    expect(controller.listColumnsSaving).toBe(false);
    expect(controller.listColumnsError).toBe("projects.columns.saveFailed: Disk is full");
    expect(controller.savedViewNameDraft).toBe("Unsaved view name");

    await controller.toggleColumn("status");
    expect(saveTaskListColumns).toHaveBeenLastCalledWith("project-1", ["due"]);
    expect(controller.listColumns).toEqual(["due"]);
    expect(controller.listColumnsError).toBeNull();
  });

  it("keeps a newer project's columns and pending write when an earlier project write fails", async () => {
    const { controller, selection, saveTaskListColumns } = createController();
    let rejectEarlier!: (error: unknown) => void;
    let finishCurrent!: () => void;
    saveTaskListColumns
      .mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectEarlier = reject; }))
      .mockImplementationOnce(() => new Promise<void>((resolve) => { finishCurrent = resolve; }));
    const earlier = controller.toggleColumn("status");
    selection.projectId = "project-2";
    controller.listColumns = ["due"];
    const current = controller.toggleColumn("priority");
    rejectEarlier("Earlier write failed");
    await earlier;
    expect(controller.listColumns).toEqual(["due", "priority"]);
    expect(controller.listColumnsSaving).toBe(true);
    expect(controller.listColumnsError).toBeNull();
    finishCurrent();
    await current;
    expect(controller.listColumnsSaving).toBe(false);
    expect(controller.listColumns).toEqual(["due", "priority"]);
  });

  it("reports a saved view column failure and skips its dependent section changes", async () => {
    const { controller, saveTaskListColumns, applySectionCollapseState } = createController();
    controller.listColumns = ["status"];
    controller.savedViewNameDraft = "Retained draft";
    saveTaskListColumns.mockRejectedValueOnce(new Error("Preference write failed"));
    await expect(controller.applySavedView(savedView())).resolves.toBeUndefined();
    expect(controller.listColumns).toEqual(["status"]);
    expect(controller.listColumnsError).toBe("projects.columns.saveFailed: Preference write failed");
    expect(controller.savedViewError).toBe("projects.savedViews.applyFailed: Preference write failed");
    expect(applySectionCollapseState).not.toHaveBeenCalled();
    expect(controller.savedViewNameDraft).toBe("Retained draft");
  });

  it("waits for saved view columns before applying its section collapse state", async () => {
    const { controller, saveTaskListColumns, applySectionCollapseState } = createController();
    let finishWrite!: () => void;
    saveTaskListColumns.mockImplementationOnce(() => new Promise<void>((resolve) => { finishWrite = resolve; }));
    const applying = controller.applySavedView(savedView());
    expect(controller.listColumnsSaving).toBe(true);
    expect(applySectionCollapseState).not.toHaveBeenCalled();
    finishWrite();
    await applying;
    expect(applySectionCollapseState).toHaveBeenCalledWith("project-1", ["section-1"]);
    expect(controller.listColumns).toEqual(["due"]);
    expect(controller.listColumnsSaving).toBe(false);
    expect(controller.savedViewError).toBeNull();
  });

  it("rolls back failed presentation writes, blocks competing changes, and permits retry", async () => {
    const { controller, saveTaskListPresentation } = createController();
    let rejectWrite!: (error: unknown) => void;
    saveTaskListPresentation.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectWrite = reject; }));
    const next = { ...defaultProjectListPresentation(), frozenThrough: "due" as const, wrappedColumns: ["name" as const] };
    const saving = controller.savePresentation(next);
    expect(controller.listPresentation.frozenThrough).toBe("due");
    expect(controller.presentationSaving).toBe(true);
    await controller.savePresentation({ ...next, frozenThrough: "priority" });
    expect(saveTaskListPresentation).toHaveBeenCalledOnce();
    rejectWrite(new Error("Read-only vault"));
    await saving;
    expect(controller.listPresentation).toEqual(defaultProjectListPresentation());
    expect(controller.presentationError).toBe("projects.columns.presentationFailed: Read-only vault");
    expect(controller.presentationSaving).toBe(false);
    await controller.savePresentation(next);
    expect(controller.listPresentation).toEqual(next);
    expect(controller.presentationError).toBeNull();
  });

  it("keeps a created schema recoverable when inserting its visible column fails", async () => {
    const { controller, saveTaskListColumns, addCustomField } = createController();
    controller.listColumns = ["status", "due"];
    saveTaskListColumns.mockRejectedValueOnce(new Error("Preference write failed"));
    await controller.addColumnProperty("Points", "number", "status");
    expect(addCustomField).toHaveBeenCalledWith("project-1", "Points", "number", null);
    expect(saveTaskListColumns).toHaveBeenCalledWith("project-1", ["status", "custom:created", "due"]);
    expect(controller.listColumns).toEqual(["status", "due"]);
    expect(controller.listColumnsError).toBe("projects.columns.saveFailed: Preference write failed");
    expect(controller.propertySaving).toBe(false);
    await controller.toggleColumn("custom:created");
    expect(controller.listColumns).toEqual(["status", "due", "custom:created"]);
  });

  it("reserves column writes until property creation and its visibility write finish", async () => {
    const { controller, addCustomField, createdField, saveTaskListColumns, applySectionCollapseState } = createController();
    let finishCreation!: (field: ProjectCustomField) => void;
    let finishColumns!: () => void;
    addCustomField.mockImplementationOnce(() => new Promise<ProjectCustomField>((resolve) => { finishCreation = resolve; }));
    saveTaskListColumns.mockImplementationOnce(() => new Promise<void>((resolve) => { finishColumns = resolve; }));
    controller.listColumns = ["status", "due"];
    const creating = controller.addColumnProperty("Points", "number", "status");
    expect(controller.propertySaving).toBe(true);
    await controller.toggleColumn("due");
    await controller.moveColumn("due", -1);
    await controller.applySavedView(savedView());
    expect(saveTaskListColumns).not.toHaveBeenCalled();
    expect(applySectionCollapseState).not.toHaveBeenCalled();
    expect(controller.listColumns).toEqual(["status", "due"]);
    finishCreation(createdField);
    await vi.waitFor(() => expect(saveTaskListColumns).toHaveBeenCalledOnce());
    expect(saveTaskListColumns).toHaveBeenCalledWith("project-1", ["status", "custom:created", "due"]);
    expect(controller.propertySaving).toBe(true);
    await controller.toggleColumn("due");
    await controller.moveColumn("due", -1);
    expect(saveTaskListColumns).toHaveBeenCalledOnce();
    finishColumns();
    await creating;
    await controller.moveColumn("due", -1);
    expect(saveTaskListColumns).toHaveBeenLastCalledWith("project-1", ["status", "due", "custom:created"]);
  });

  it("places a column at any list index and skips writes that keep the order or name hidden columns", async () => {
    const { controller, saveTaskListColumns } = createController();
    controller.listColumns = ["status", "due", "priority", "custom:created"];
    await controller.placeColumn("custom:created", 0);
    expect(saveTaskListColumns).toHaveBeenLastCalledWith("project-1", ["custom:created", "status", "due", "priority"]);
    expect(controller.listColumns).toEqual(["custom:created", "status", "due", "priority"]);
    await controller.placeColumn("custom:created", 99);
    expect(controller.listColumns).toEqual(["status", "due", "priority", "custom:created"]);
    saveTaskListColumns.mockClear();
    await controller.placeColumn("due", 1);
    await controller.placeColumn("assignee", 0);
    await controller.moveColumn("status", -1);
    await controller.moveColumn("custom:created", 1);
    expect(saveTaskListColumns).not.toHaveBeenCalled();
    expect(controller.listColumns).toEqual(["status", "due", "priority", "custom:created"]);
  });

  it("duplicates through the schema boundary and inserts the fresh property next to its source", async () => {
    const { controller, duplicateCustomField, createdField, fields, addCustomField } = createController();
    const source = { ...createdField, id: "source", name: "Original" };
    fields.push(source);
    controller.listColumns = ["status", "custom:source", "due"];
    await controller.addColumnProperty("Original copy", "number", "custom:source", "right", { source });
    expect(duplicateCustomField).toHaveBeenCalledWith(source, "Original copy");
    expect(addCustomField).not.toHaveBeenCalled();
    expect(controller.listColumns).toEqual(["status", "custom:source", "custom:created", "due"]);
    await controller.addColumnProperty(" original ", "number", "name");
    expect(controller.propertyError).toContain("projects.columns.nameExists");
    expect(addCustomField).not.toHaveBeenCalled();
  });

  it("inserts a created property on the requested side of its anchor and reports the outcome", async () => {
    const left = createController();
    left.controller.listColumns = ["status", "due"];
    await expect(left.controller.addColumnProperty("Points", "number", "due", "left")).resolves.toBe(true);
    expect(left.controller.listColumns).toEqual(["status", "custom:created", "due"]);

    const afterName = createController();
    afterName.controller.listColumns = ["status", "due"];
    await expect(afterName.controller.addColumnProperty("Points", "number", "name")).resolves.toBe(true);
    expect(afterName.controller.listColumns).toEqual(["custom:created", "status", "due"]);
    await expect(afterName.controller.addColumnProperty("  ", "number", "status")).resolves.toBe(false);
    expect(afterName.controller.propertyError).toContain("projects.columns.nameRequired");
  });

  it("withholds old calculations as soon as the canonical query changes", async () => {
    const { controller } = createController();
    expect(controller.columnCalculations).toBeUndefined();
    controller.loadCurrent([], null);
    await vi.waitFor(() => expect(controller.columnCalculations?.[0].total).toBe(1000));
    controller.search = "Changed query";
    expect(controller.columnCalculations).toBeUndefined();
    controller.loadCurrent([], null);
    await vi.waitFor(() => expect(controller.columnCalculations?.[0].sum).toBe(5000));
  });

  it("loads an unchanged query once, then refreshes after a successful mutation or project change", async () => {
    const {controller, requests, selection} = createController();
    controller.loadCurrent(["two", "one"], "one");
    await vi.waitFor(() => expect(requests).toHaveLength(1));
    controller.loadCurrent(["one", "two"], null);
    expect(requests).toHaveLength(1);
    selection.mutationRevision += 1;
    expect(controller.columnCalculations).toBeUndefined();
    controller.loadCurrent(["one", "two"], null);
    await vi.waitFor(() => expect(requests).toHaveLength(2));
    selection.projectId = "project-2";
    controller.loadCurrent([], null);
    await vi.waitFor(() => expect(requests).toHaveLength(3));
    expect(requests[2].request.projectId).toBe("project-2");
  });

  it("reloads the same query when the user explicitly retries a failed continuation", async () => {
    const {controller, requests, selection, loadTaskView} = createController();
    const log = vi.spyOn(console, "error").mockImplementation(() => undefined);
    controller.loadCurrent([], null);
    await vi.waitFor(() => expect(requests).toHaveLength(1));
    loadTaskView.mockImplementationOnce(async () => { selection.loadError = "Continuation failed"; throw new Error(selection.loadError); });
    controller.loadNextList([]);
    await vi.waitFor(() => expect(log).toHaveBeenCalledOnce());
    expect(controller.loadError).toBe("Continuation failed");
    expect(controller.columnCalculations).toBeUndefined();
    controller.retryCurrent([], null);
    await vi.waitFor(() => expect(requests).toHaveLength(2));
    expect(requests[1].request).toEqual(requests[0].request);
    expect(controller.loadError).toBeNull();
    log.mockRestore();
  });

  it("keeps current summaries when an older query resolves after the newer one", async () => {
    const {controller, loadTaskView} = createController();
    let finishOld!: () => void;
    let finishNew!: () => void;
    loadTaskView.mockImplementationOnce(() => new Promise<void>((resolve) => { finishOld = resolve; }))
      .mockImplementationOnce(() => new Promise<void>((resolve) => { finishNew = resolve; }));
    controller.loadCurrent([], null);
    controller.search = "New query";
    controller.loadCurrent([], null);
    finishNew();
    await vi.waitFor(() => expect(controller.columnCalculations?.[0].total).toBe(1000));
    finishOld();
    await Promise.resolve();
    expect(controller.columnCalculations?.[0].total).toBe(1000);
  });

  it("preserves the current project's presentation when an earlier project's write fails", async () => {
    const {controller, selection, saveTaskListPresentation} = createController();
    let rejectOld!: (error: unknown) => void;
    let finishCurrent!: () => void;
    saveTaskListPresentation.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectOld = reject; }))
      .mockImplementationOnce(() => new Promise<void>((resolve) => { finishCurrent = resolve; }));
    const earlier = controller.savePresentation({...defaultProjectListPresentation(), frozenThrough: "due"});
    selection.projectId = "project-2";
    controller.listPresentation = defaultProjectListPresentation();
    const current = controller.savePresentation({...defaultProjectListPresentation(), frozenThrough: "name"});
    rejectOld(new Error("Old project's write failed"));
    await earlier;
    expect(controller.listPresentation.frozenThrough).toBe("name");
    expect(controller.presentationSaving).toBe(true);
    expect(controller.presentationError).toBeNull();
    finishCurrent();
    await current;
    expect(controller.presentationSaving).toBe(false);
  });
});
