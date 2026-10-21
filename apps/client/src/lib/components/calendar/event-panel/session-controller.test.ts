import { describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { Project } from "$lib/projects/types";
import {
  EventPanelSessionController,
  eventPanelHeavyLookupId,
} from "./session-controller.svelte";

function event(id: string, overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id,
    title: `Event ${id}`,
    start: "2026-07-12 23:30",
    end: "2026-07-13 00:30",
    timezone: "America/Monterrey",
    calendarId: "calendar-a",
    ...overrides,
  };
}

function project(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-a",
    groupId: "group-a",
    name: "Project",
    icon: "lucide:folder",
    sortOrder: 0,
    status: "active",
    defaultEventName: "Project focus",
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: 90,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "adaptive",
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: true,
    defaultIdleThresholdMinutes: 3,
    createdAt: "2026-07-12T00:00:00Z",
    updatedAt: "2026-07-12T00:00:00Z",
    ...overrides,
  };
}

function controller(selectedProject?: Project) {
  const onChange = vi.fn();
  return {
    onChange,
    session: new EventPanelSessionController({
      projects: { projectById: (id) => id === selectedProject?.id ? selectedProject : undefined },
      controlsDisabled: () => false,
      lockStartControls: () => false,
      timeFormat: () => "24h",
      mode: () => "create",
      preferences: () => ({ idlePauseEnabled: true, idleThresholdMinutes: 3 }),
      onChange: () => onChange,
    }),
  };
}

describe("EventPanelSessionController", () => {
  it("loads recurring occurrence details from the parent identity", () => {
    expect(eventPanelHeavyLookupId(event("instance", { recurringParentId: "parent" }))).toBe("parent");
    expect(eventPanelHeavyLookupId(event("standalone"))).toBe("standalone");
  });

  it("starts initialization exactly once for each session identity", () => {
    const { session } = controller();
    expect(session.beginInitialization("create:1")).toBe(true);
    session.initialized = true;
    expect(session.beginInitialization("create:1")).toBe(false);
    expect(session.initialized).toBe(true);
    expect(session.beginInitialization("create:2")).toBe(true);
    expect(session.initialized).toBe(false);
  });

  it("rejects stale heavy loads and preserves a project changed during hydration", () => {
    const { session } = controller();
    session.initializeEdit(event("opened", { projectId: "initial" }));
    const stale = session.beginHeavyLoad("opened");
    const current = session.beginHeavyLoad("next");

    expect(session.completeHeavyLoad(stale, event("root", { description: "stale" }))).toBe(false);
    session.projectId = "user-selection";
    expect(session.completeHeavyLoad(current, event("root", {
      description: "loaded",
      projectId: "server-project",
    }))).toBe(true);
    expect(session.description).toBe("loaded");
    expect(session.projectId).toBe("user-selection");
  });

  it("applies project defaults across midnight and emits the normalized payload", () => {
    const selectedProject = project();
    const { session, onChange } = controller(selectedProject);
    session.initializeCreate({}, "2026-07-12 23:30", "2026-07-13 00:00", false);

    session.handleProjectSelect(selectedProject.id);

    expect(session.title).toBe("Project focus");
    expect(session.endDate).toBe("2026-07-13");
    expect(session.endTime).toBe("01:00");
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({
      title: "Project focus",
      projectId: selectedProject.id,
      end: "2026-07-13 01:00",
    }));
  });

  it("keeps the chosen event color for a project created from the picker and offers it on save", () => {
    const created = project({ color: undefined });
    const { session } = controller(created);
    session.initializeCreate({ color: 5 }, "2026-07-12 09:00", "2026-07-12 10:00", false);

    session.handleProjectCreated(created.id);

    expect(session.color).toBe(5);
    expect(session.createdProjectColorForSave({ projectId: created.id, color: 5 })).toBe(5);
    expect(session.createdProjectColorForSave({ projectId: created.id, color: undefined })).toBeNull();
  });

  it("applies an existing project's color and never offers to replace it", () => {
    const existing = project({ color: 2 });
    const { session } = controller(existing);
    session.initializeCreate({ color: 5 }, "2026-07-12 09:00", "2026-07-12 10:00", false);

    session.handleProjectSelect(existing.id);

    expect(session.color).toBe(2);
    expect(session.createdProjectColorForSave({ projectId: existing.id, color: 2 })).toBeNull();
  });
});
