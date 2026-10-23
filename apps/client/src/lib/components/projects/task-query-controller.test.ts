import { describe, expect, it, vi } from "vitest";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { ProjectCustomField, ProjectTaskViewRequest, ProjectViewPreference } from "$lib/projects/types";
import {
  defaultProjectViewQuery,
  PROJECT_VIEW_QUERY_PREFERENCE_KEY,
  serializeProjectViewQuery,
  type ProjectViewQuery,
} from "$lib/projects/view-query";
import { defaultProjectListPresentation } from "$lib/projects/list/presentation";
import {
  ProjectTaskQueryController,
  normalizeProjectFilterDate,
  repairProjectTaskQueryReferences,
} from "./task-query-controller.svelte";

type ControllerInput = ConstructorParameters<typeof ProjectTaskQueryController>[0];

/** Isolate query preferences and their native write boundary from the rest of the project store. */
function createController(activeView: "list" | "kanban" = "list") {
  const requests: Array<{ request: ProjectTaskViewRequest; append: boolean }> = [];
  const selection = { projectId: "project-1", mutationRevision: 0, loadError: null as string | null, preferencesLoaded: true, viewPreferences: [] as ProjectViewPreference[], viewId: null as string | null };
  const saveViewQuery = vi.fn<ControllerInput["projects"]["saveViewQuery"]>().mockResolvedValue(undefined);
  const saveTaskListColumns = vi.fn<ControllerInput["projects"]["saveTaskListColumns"]>().mockResolvedValue(undefined);
  const saveTaskListPresentation = vi.fn<ControllerInput["projects"]["saveTaskListPresentation"]>().mockResolvedValue(undefined);
  const fields: ProjectCustomField[] = [];
  const createdField: ProjectCustomField = { id: "created", projectId: "project-1", name: "Points", fieldType: "number", orderKey: "100", icon: null, createdAt: "", updatedAt: "" };
  const addCustomField = vi.fn<ControllerInput["projects"]["addCustomField"]>().mockResolvedValue(createdField);
  const duplicateCustomField = vi.fn<ControllerInput["projects"]["duplicateCustomField"]>().mockResolvedValue(createdField);
  const loadTaskView = vi.fn<ControllerInput["projects"]["loadTaskView"]>().mockImplementation(async (request, append = false) => { selection.loadError = null; requests.push({request, append}); });
  const projects = {
    get selectedProject() { return { id: selection.projectId }; },
    get taskMutationRevision() { return selection.mutationRevision; },
    get taskViewError() { return selection.loadError; },
    activeView,
    activeTaskView: activeView,
    get activeProjectView() { return { id: selection.viewId ?? `view-${selection.projectId}`, projectId: selection.projectId, viewType: activeView }; },
    saveTaskListColumns,
    saveTaskListPresentation,
    saveViewQuery,
    isProjectOptionalDataLoaded: () => selection.preferencesLoaded,
    addCustomField,
    duplicateCustomField,
    customFieldsForProject: () => fields,
    prioritiesForProject: () => [],
    tagsForProject: () => [],
    customFieldOptionsForField: () => [],
    isProjectDataLoaded: () => true,
    get viewPreferences() { return selection.viewPreferences; },
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
    querySaveDelayMs: 10,
  });
  controller.syncViewQuery();
  return { controller, requests, selection, saveViewQuery, saveTaskListColumns, saveTaskListPresentation, addCustomField, duplicateCustomField, fields, createdField, loadTaskView };
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

  it("restores columns when saving fails, then allows a retry", async () => {
    const { controller, saveTaskListColumns } = createController();
    controller.listColumns = ["status", "due"];
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

    await controller.toggleColumn("status");
    expect(saveTaskListColumns).toHaveBeenLastCalledWith("view-project-1", ["due"]);
    expect(controller.listColumns).toEqual(["due"]);
    expect(controller.listColumnsError).toBeNull();
  });

  it("keeps a newer view's columns and pending write when an earlier view's write fails", async () => {
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
    expect(saveTaskListColumns).toHaveBeenCalledWith("view-project-1", ["status", "custom:created", "due"]);
    expect(controller.listColumns).toEqual(["status", "due"]);
    expect(controller.listColumnsError).toBe("projects.columns.saveFailed: Preference write failed");
    expect(controller.propertySaving).toBe(false);
    await controller.toggleColumn("custom:created");
    expect(controller.listColumns).toEqual(["status", "due", "custom:created"]);
  });

  it("reserves column writes until property creation and its visibility write finish", async () => {
    const { controller, addCustomField, createdField, saveTaskListColumns } = createController();
    let finishCreation!: (field: ProjectCustomField) => void;
    let finishColumns!: () => void;
    addCustomField.mockImplementationOnce(() => new Promise<ProjectCustomField>((resolve) => { finishCreation = resolve; }));
    saveTaskListColumns.mockImplementationOnce(() => new Promise<void>((resolve) => { finishColumns = resolve; }));
    controller.listColumns = ["status", "due"];
    const creating = controller.addColumnProperty("Points", "number", "status");
    expect(controller.propertySaving).toBe(true);
    await controller.toggleColumn("due");
    await controller.moveColumn("due", -1);
    expect(saveTaskListColumns).not.toHaveBeenCalled();
    expect(controller.listColumns).toEqual(["status", "due"]);
    finishCreation(createdField);
    await vi.waitFor(() => expect(saveTaskListColumns).toHaveBeenCalledOnce());
    expect(saveTaskListColumns).toHaveBeenCalledWith("view-project-1", ["status", "custom:created", "due"]);
    expect(controller.propertySaving).toBe(true);
    await controller.toggleColumn("due");
    await controller.moveColumn("due", -1);
    expect(saveTaskListColumns).toHaveBeenCalledOnce();
    finishColumns();
    await creating;
    await controller.moveColumn("due", -1);
    expect(saveTaskListColumns).toHaveBeenLastCalledWith("view-project-1", ["status", "due", "custom:created"]);
  });

  it("places a column at any list index and skips writes that keep the order or name hidden columns", async () => {
    const { controller, saveTaskListColumns } = createController();
    controller.listColumns = ["status", "due", "priority", "custom:created"];
    await controller.placeColumn("custom:created", 0);
    expect(saveTaskListColumns).toHaveBeenLastCalledWith("view-project-1", ["custom:created", "status", "due", "priority"]);
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
    controller.syncViewQuery();
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

  it("preserves the current view's presentation when an earlier view's write fails", async () => {
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

  it("restores each view's saved query while the task search stays shared", () => {
    const { controller, selection } = createController();
    selection.viewPreferences = [queryPreference("view-b", { statusFilter: "blocked", sortDirection: "desc" }, { showArchivedTasks: true, calendarMode: "month" })];
    controller.search = "invoice";
    selection.viewId = "view-b";
    controller.syncViewQuery();
    expect(controller.queryReady).toBe(true);
    expect(controller.statusFilter).toBe("blocked");
    expect(controller.sortDirection).toBe("desc");
    expect(controller.showArchivedTasks).toBe(true);
    expect(controller.calendarMode).toBe("month");
    expect(controller.search).toBe("invoice");

    selection.viewId = "view-c";
    controller.syncViewQuery();
    expect(controller.statusFilter).toBe("all");
    expect(controller.showArchivedTasks).toBe(false);
    expect(controller.calendarMode).toBeNull();
    expect(controller.search).toBe("invoice");
  });

  it("saves edits once after the quiet period and keeps newer edits when its own write comes back", async () => {
    const { controller, selection, saveViewQuery } = createController();
    controller.persistViewQuery();
    controller.statusFilter = "open";
    controller.persistViewQuery();
    controller.sortDirection = "desc";
    controller.persistViewQuery();
    await vi.waitFor(() => expect(saveViewQuery).toHaveBeenCalledTimes(1));
    const [viewId, saved] = saveViewQuery.mock.calls[0];
    expect(viewId).toBe("view-project-1");
    expect(saved.filters).toMatchObject({ statusFilter: "open", sortDirection: "desc" });

    controller.statusFilter = "done";
    controller.persistViewQuery();
    selection.viewPreferences = [queryPreference("view-project-1", saved.filters)];
    controller.syncViewQuery();
    expect(controller.statusFilter).toBe("done");
    await vi.waitFor(() => expect(saveViewQuery).toHaveBeenCalledTimes(2));
    expect(saveViewQuery.mock.calls[1][1].filters.statusFilter).toBe("done");
  });

  it("saves a pending edit to its own view before another view opens and shows it on return", () => {
    const { controller, selection, saveViewQuery } = createController();
    controller.statusFilter = "blocked";
    controller.persistViewQuery();
    selection.viewId = "view-b";
    controller.syncViewQuery();
    expect(saveViewQuery).toHaveBeenCalledTimes(1);
    expect(saveViewQuery.mock.calls[0][0]).toBe("view-project-1");
    expect(controller.statusFilter).toBe("all");

    selection.viewId = null;
    controller.syncViewQuery();
    expect(controller.statusFilter).toBe("blocked");
  });

  it("applies a query changed elsewhere and drops the overridden local edit", async () => {
    const { controller, selection, saveViewQuery } = createController();
    controller.statusFilter = "open";
    controller.persistViewQuery();
    selection.viewPreferences = [queryPreference("view-project-1", { statusFilter: "done" })];
    controller.syncViewQuery();
    expect(controller.statusFilter).toBe("done");
    controller.persistViewQuery();
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(saveViewQuery).not.toHaveBeenCalled();
  });

  it("loads no tasks until the open view's saved query has loaded", async () => {
    const { controller, selection, requests } = createController();
    selection.preferencesLoaded = false;
    selection.viewId = "view-b";
    controller.syncViewQuery();
    expect(controller.queryReady).toBe(false);
    controller.loadCurrent([], null);
    expect(requests).toHaveLength(0);

    selection.viewPreferences = [queryPreference("view-b", { statusFilter: "blocked" })];
    selection.preferencesLoaded = true;
    controller.syncViewQuery();
    controller.loadCurrent([], null);
    await vi.waitFor(() => expect(requests).toHaveLength(1));
    expect(requests[0].request.statusFilter).toBe("blocked");
  });

  it("keeps saved field references until the project's fields have loaded", () => {
    const kanban = createController("kanban").controller;
    kanban.sortMode = "custom:points";
    kanban.repairDisappearingFields();
    expect(kanban.sortMode).toBe("custom:points");

    const list = createController("list").controller;
    list.sortMode = "custom:points";
    list.repairDisappearingFields();
    expect(list.sortMode).toBe("manual");
  });
});

function queryPreference(
  viewId: string,
  filters: Partial<ProjectViewQuery["filters"]>,
  rest: Partial<Omit<ProjectViewQuery, "filters">> = {},
): ProjectViewPreference {
  const query = defaultProjectViewQuery();
  return {
    projectId: "project-1",
    viewId,
    preferenceKey: PROJECT_VIEW_QUERY_PREFERENCE_KEY,
    preferenceValue: serializeProjectViewQuery({ ...query, ...rest, filters: { ...query.filters, ...filters } }),
    updatedAt: "",
  };
}
