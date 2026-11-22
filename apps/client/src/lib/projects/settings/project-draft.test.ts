import { describe, expect, it } from "vitest";
import {
  projectDraftDirty,
  projectDraftFromProject,
  projectLifecycleChangeFromDraft,
  projectUpdateFromDraft,
  type ProjectDraft,
} from "./project-draft";
import type { Project } from "$lib/projects/types";

function project(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-1",
    groupId: "group-1",
    name: "Project",
    icon: "lucide:folder",
    color: 8,
    orderKey: "a0",
    status: "active",
    defaultEventName: null,
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: 60,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "adaptive",
    defaultPomodoroFocusMinutes: undefined,
    defaultPomodoroShortBreakMinutes: undefined,
    defaultPomodoroLongBreakMinutes: undefined,
    defaultPomodoroLongBreakAfterFocusCount: undefined,
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: true,
    defaultIdleThresholdMinutes: 5,
    focusPlaylistId: undefined,
    breakPlaylistId: undefined,
    workEnvironmentId: undefined,
    blockerRulesetId: undefined,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function updateDraft(
  draft: ProjectDraft,
  overrides: Partial<ProjectDraft>,
): ProjectDraft {
  return { ...draft, ...overrides };
}

describe("projectDraftFromProject", () => {
  it("loads nullable project fields into editable string drafts", () => {
    expect(projectDraftFromProject(project())).toMatchObject({
      groupId: "group-1",
      name: "Project",
      defaultEventName: "",
      defaultEventDurationMinutes: "60",
      defaultPomodoroPresetKey: "adaptive",
      focusPlaylistId: "",
      breakPlaylistId: "",
    });
  });
});

describe("projectDraftDirty", () => {
  it("treats a loaded project draft as clean", () => {
    const currentProject = project();

    expect(projectDraftDirty(
      currentProject,
      projectDraftFromProject(currentProject),
    )).toBe(false);
  });

  it("detects project identity and default changes", () => {
    const currentProject = project();
    const draft = updateDraft(projectDraftFromProject(currentProject), {
      name: "Renamed",
      defaultEventDurationMinutes: "90",
    });

    expect(projectDraftDirty(currentProject, draft)).toBe(true);
  });

  it("ignores locked identity drafts for built-in Routine projects", () => {
    const currentProject = project({
      id: "project-routine-eat",
      groupId: "group-routine",
      name: "Eating",
    });
    const draft = updateDraft(projectDraftFromProject(currentProject), {
      name: "Renamed",
      groupId: "group-2",
    });

    expect(projectDraftDirty(currentProject, draft)).toBe(false);
  });
});

describe("projectUpdateFromDraft", () => {
  it("builds a trimmed project update and keeps the project in place inside the same group", () => {
    const currentProject = project({ focusPlaylistId: "focus-old" });
    const draft = updateDraft(projectDraftFromProject(currentProject), {
      name: "  Renamed  ",
      defaultEventName: "  Session  ",
      focusPlaylistId: "  focus-new  ",
      breakPlaylistId: " ",
    });

    const result = projectUpdateFromDraft({
      project: currentProject,
      draft,
      visibleGroupIds: new Set(["group-1"]),
    });

    expect(result).toMatchObject({
      ok: true,
      value: {
        id: "project-1",
        groupId: "group-1",
        name: "Renamed",
        placement: undefined,
        defaultEventName: "Session",
        defaultEventDurationMinutes: 60,
        focusPlaylistId: "focus-new",
        breakPlaylistId: null,
      },
    });
  });

  it("appends the project to the end of a new group", () => {
    const currentProject = project();
    const draft = updateDraft(projectDraftFromProject(currentProject), {
      groupId: "group-2",
    });

    const result = projectUpdateFromDraft({
      project: currentProject,
      draft,
      visibleGroupIds: new Set(["group-1", "group-2"]),
    });

    expect(result).toMatchObject({
      ok: true,
      value: { groupId: "group-2", placement: { kind: "end" } },
    });
  });

  it("preserves the built-in Routine project identity", () => {
    const currentProject = project({
      id: "project-routine-eat",
      groupId: "group-routine",
      name: "Comer",
      orderKey: "a0",
    });
    const draft = updateDraft(projectDraftFromProject(currentProject), {
      name: "Renamed",
      groupId: "group-2",
      icon: "lucide:utensils",
      status: "hidden",
    });

    const result = projectUpdateFromDraft({
      project: currentProject,
      draft,
      visibleGroupIds: new Set(["group-routine", "group-2"]),
    });

    expect(result).toMatchObject({
      ok: true,
      value: {
        groupId: "group-routine",
        name: "Comer",
        placement: undefined,
        icon: "lucide:utensils",
      },
    });
    expect(result.ok && "status" in result.value).toBe(false);
    expect(projectLifecycleChangeFromDraft(currentProject.id, draft)).toEqual({
      projectId: "project-routine-eat",
      expected: "active",
      next: "hidden",
    });
  });

  it("asks for a lifecycle move from the opened status only when the status changed", () => {
    const opened = projectDraftFromProject(project({ status: "hidden" }));
    expect(projectLifecycleChangeFromDraft("project-1", opened)).toBeNull();

    const refreshed = project({ status: "archived" });
    expect(projectDraftDirty(refreshed, opened)).toBe(false);

    const shown = updateDraft(opened, { status: "active" });
    expect(projectDraftDirty(refreshed, shown)).toBe(true);
    expect(projectLifecycleChangeFromDraft("project-1", shown)).toEqual({
      projectId: "project-1",
      expected: "hidden",
      next: "active",
    });
  });

  it("writes custom pomodoro values only when custom mode is selected", () => {
    const currentProject = project();
    const draft = updateDraft(projectDraftFromProject(currentProject), {
      defaultPomodoroMode: "custom",
      defaultPomodoroFocusMinutes: 45,
      defaultPomodoroShortBreakMinutes: 10,
      defaultPomodoroLongBreakMinutes: 20,
      defaultPomodoroLongBreakAfterFocusCount: 3,
    });

    const result = projectUpdateFromDraft({
      project: currentProject,
      draft,
      visibleGroupIds: new Set(["group-1"]),
    });

    expect(result).toMatchObject({
      ok: true,
      value: {
        defaultPomodoroMode: "custom",
        defaultPomodoroPresetKey: null,
        defaultPomodoroFocusMinutes: 45,
        defaultPomodoroShortBreakMinutes: 10,
        defaultPomodoroLongBreakMinutes: 20,
        defaultPomodoroLongBreakAfterFocusCount: 3,
      },
    });
  });

  it("rejects invalid project drafts", () => {
    const currentProject = project();
    const cleanDraft = projectDraftFromProject(currentProject);

    expect(projectUpdateFromDraft({
      project: currentProject,
      draft: updateDraft(cleanDraft, { name: " " }),
      visibleGroupIds: new Set(["group-1"]),
    })).toEqual({ ok: false, error: "name_required" });
    expect(projectUpdateFromDraft({
      project: currentProject,
      draft: updateDraft(cleanDraft, { groupId: "missing-group" }),
      visibleGroupIds: new Set(["group-1"]),
    })).toEqual({ ok: false, error: "group_unavailable" });
    expect(projectUpdateFromDraft({
      project: currentProject,
      draft: updateDraft(cleanDraft, { defaultEventDurationMinutes: "0" }),
      visibleGroupIds: new Set(["group-1"]),
    })).toEqual({ ok: false, error: "invalid_duration" });
  });

  it("moves a project out of every group and back into a visible one", () => {
    const grouped = project();
    const ungroupedResult = projectUpdateFromDraft({
      project: grouped,
      draft: updateDraft(projectDraftFromProject(grouped), { groupId: null }),
      visibleGroupIds: new Set(["group-1"]),
    });
    expect(ungroupedResult).toMatchObject({ ok: true, value: { groupId: null, placement: { kind: "end" } } });

    const ungrouped = project({ groupId: null });
    expect(projectDraftDirty(ungrouped, projectDraftFromProject(ungrouped))).toBe(false);
    expect(projectUpdateFromDraft({
      project: ungrouped,
      draft: projectDraftFromProject(ungrouped),
      visibleGroupIds: new Set(),
    })).toMatchObject({ ok: true, value: { groupId: null, placement: undefined } });
    expect(projectUpdateFromDraft({
      project: ungrouped,
      draft: updateDraft(projectDraftFromProject(ungrouped), { groupId: "group-1" }),
      visibleGroupIds: new Set(["group-1"]),
    })).toMatchObject({ ok: true, value: { groupId: "group-1", placement: { kind: "end" } } });
  });
});
