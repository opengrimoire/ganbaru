import { describe, expect, it } from "vitest";
import {
  projectDraftDirty,
  projectDraftFromProject,
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
    sortOrder: 1000,
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
  it("builds a trimmed project update and preserves sort order inside the same group", () => {
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
      nextSortOrderForGroup: () => 5000,
    });

    expect(result).toMatchObject({
      ok: true,
      value: {
        id: "project-1",
        groupId: "group-1",
        name: "Renamed",
        sortOrder: 1000,
        defaultEventName: "Session",
        defaultEventDurationMinutes: 60,
        focusPlaylistId: "focus-new",
        breakPlaylistId: null,
      },
    });
  });

  it("uses the provided group sort order when the group changes", () => {
    const currentProject = project();
    const draft = updateDraft(projectDraftFromProject(currentProject), {
      groupId: "group-2",
    });

    const result = projectUpdateFromDraft({
      project: currentProject,
      draft,
      visibleGroupIds: new Set(["group-1", "group-2"]),
      nextSortOrderForGroup: () => 5000,
    });

    expect(result).toMatchObject({
      ok: true,
      value: { groupId: "group-2", sortOrder: 5000 },
    });
  });

  it("preserves the built-in Routine project identity", () => {
    const currentProject = project({
      id: "project-routine-eat",
      groupId: "group-routine",
      name: "Comer",
      sortOrder: 20,
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
      nextSortOrderForGroup: () => 5000,
    });

    expect(result).toMatchObject({
      ok: true,
      value: {
        groupId: "group-routine",
        name: "Comer",
        sortOrder: 20,
        icon: "lucide:utensils",
        status: "hidden",
      },
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
      nextSortOrderForGroup: () => 5000,
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
      nextSortOrderForGroup: () => 5000,
    })).toEqual({ ok: false, error: "name_required" });
    expect(projectUpdateFromDraft({
      project: currentProject,
      draft: updateDraft(cleanDraft, { groupId: "missing-group" }),
      visibleGroupIds: new Set(["group-1"]),
      nextSortOrderForGroup: () => 5000,
    })).toEqual({ ok: false, error: "group_required" });
    expect(projectUpdateFromDraft({
      project: currentProject,
      draft: updateDraft(cleanDraft, { defaultEventDurationMinutes: "0" }),
      visibleGroupIds: new Set(["group-1"]),
      nextSortOrderForGroup: () => 5000,
    })).toEqual({ ok: false, error: "invalid_duration" });
  });
});
