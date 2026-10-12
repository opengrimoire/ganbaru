import { describe, expect, it, vi } from "vitest";
import type { getLocalization } from "$lib/i18n/translator.svelte";
import type { getProjects } from "$lib/stores/projects.svelte";
import { createProjectSettingsSession } from "./session.svelte";
import type { Project, ProjectStatus } from "$lib/projects/types";

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

function status(): ProjectStatus {
  return {
    id: "status-1",
    projectId: "project-1",
    name: "Open",
    category: "active",
    color: 8,
    sortOrder: 1000,
    terminal: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

const emptyCollections = {
  statuses: [],
  priorities: [],
  tags: [],
  customFields: [],
  optionsForField: () => [],
};

function translate(): ReturnType<typeof getLocalization>["t"] {
  return ((key: string, ...args: Array<string | number>) =>
    [key, ...args].join(":")) as ReturnType<typeof getLocalization>["t"];
}

describe("project settings session", () => {
  it("validates all fields before committing folder or structural changes", async () => {
    const commit = vi.fn(async () => {});
    const updateProject = vi.fn(async () => {});
    const session = createProjectSettingsSession({ projects: { projectsForGroupIncludingInactive: () => [], updateProject } as unknown as ReturnType<typeof getProjects>, translate: translate(), onRevealInactive: () => {} });
    const current = project();
    session.load(current, emptyCollections);
    session.state.projectDraft.name = " ";
    expect(await session.save(current, new Set(["group-1"]), emptyCollections, undefined, commit)).toBe(false);
    expect(commit).not.toHaveBeenCalled();
    expect(updateProject).not.toHaveBeenCalled();
    session.state.projectDraft.name = "Renamed";
    expect(await session.save(current, new Set(["group-1"]), emptyCollections, undefined, commit)).toBe(true);
    expect(commit).toHaveBeenCalledOnce();
    expect(commit.mock.invocationCallOrder[0]).toBeLessThan(updateProject.mock.invocationCallOrder[0]);
  });

  it("reloads and discards one owning draft without retaining stale project state", () => {
    const projects = {} as ReturnType<typeof getProjects>;
    const session = createProjectSettingsSession({
      projects,
      translate: translate(),
      onRevealInactive: () => undefined,
    });
    const first = project();
    const second = project({ id: "project-2", name: "Second", updatedAt: "2026-01-02T00:00:00Z" });

    session.load(first, emptyCollections);
    session.state.projectDraft.name = "Changed";
    expect(session.dirty(first, emptyCollections)).toBe(true);

    session.discard(first, emptyCollections);
    expect(session.state.projectDraft.name).toBe("Project");
    expect(session.dirty(first, emptyCollections)).toBe(false);

    session.load(second, emptyCollections);
    expect(session.state.projectDraftId).toBe("project-2");
    expect(session.state.projectDraft.name).toBe("Second");
  });

  it("keeps successful earlier writes clean when a later sequential save fails", async () => {
    const updateProject = vi.fn(async () => undefined);
    const updateStatus = vi.fn(async () => { throw new Error("status failed"); });
    const projects = {
      projectsForGroupIncludingInactive: () => [],
      updateProject,
      updateStatus,
    } as unknown as ReturnType<typeof getProjects>;
    const session = createProjectSettingsSession({
      projects,
      translate: translate(),
      onRevealInactive: () => undefined,
    });
    const currentProject = project();
    const currentStatus = status();
    const collections = { ...emptyCollections, statuses: [currentStatus] };
    session.load(currentProject, collections);
    session.state.projectDraft.name = "Renamed";
    session.state.statusNameDrafts[currentStatus.id] = "In progress";

    await session.save(currentProject, new Set(["group-1"]), collections);

    expect(updateProject).toHaveBeenCalledOnce();
    expect(updateStatus).toHaveBeenCalledOnce();
    expect(session.state.statusNameDrafts[currentStatus.id]).toBe("In progress");
    expect(session.state.projectSettingsSaving).toBe(false);
    expect(session.state.projectSettingsError).toContain("status failed");
  });

  it("restores the locked Routine identity while retaining editable defaults", () => {
    const session = createProjectSettingsSession({
      projects: {} as ReturnType<typeof getProjects>,
      translate: translate(),
      onRevealInactive: () => undefined,
    });
    const routine = project({ id: "project-routine-eat", groupId: "group-routine", name: "Eating" });
    session.load(routine, emptyCollections);
    session.state.projectDraft.name = "Renamed";
    session.state.projectDraft.groupId = "group-2";
    session.state.projectDraft.defaultEventName = "Lunch";

    session.synchronizeLockedIdentity(routine);

    expect(session.state.projectDraft.name).toBe("Eating");
    expect(session.state.projectDraft.groupId).toBe("group-routine");
    expect(session.state.projectDraft.defaultEventName).toBe("Lunch");
  });

  it("saves soundtrack-only edits through the project transaction", async () => {
    const updateProject = vi.fn(async () => undefined);
    const projects = {
      projectsForGroupIncludingInactive: () => [],
      updateProject,
    } as unknown as ReturnType<typeof getProjects>;
    const session = createProjectSettingsSession({
      projects,
      translate: translate(),
      onRevealInactive: () => undefined,
    });
    const currentProject = project();
    session.load(currentProject, emptyCollections);
    const saved = await session.save(currentProject, new Set(["group-1"]), emptyCollections, {
      assignments: [{
        phase: "focus",
        behavior: "play-automatically",
        playlistId: "playlist-1",
        soundscapeId: null,
        soundscapeBehavior: "inherit",
        provenanceKind: "explicit",
        provenanceId: null,
      }],
      updatedAt: 1_700_000_000_000,
    });

    expect(saved).toBe(true);
    expect(updateProject).toHaveBeenCalledWith(expect.objectContaining({
      musicAssignments: [expect.objectContaining({ phase: "focus", playlistId: "playlist-1" })],
      musicAssignmentsUpdatedAt: 1_700_000_000_000,
    }));
  });
});
