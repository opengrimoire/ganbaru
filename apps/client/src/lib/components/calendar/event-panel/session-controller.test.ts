import { describe, expect, it, vi } from "vitest";
import { WINDOW_TEXT_CHARS } from "$lib/calendar/db-rows";
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
    orderKey: "a0",
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
  it("replaces a shortened window title and location with the full event and blocks their edit until then", () => {
    const shortened = "T".repeat(WINDOW_TEXT_CHARS);
    const opened = event("long", { title: shortened, location: shortened,
      windowTitleShortened: true, windowLocationShortened: true });
    const { session } = controller();
    session.initializeEdit(opened);
    const token = session.beginHeavyLoad(opened.id);
    expect(session.titleEditBlocked).toBe(true);
    expect(session.locationEditBlocked).toBe(true);
    session.completeHeavyLoad(token, event("long", {
      title: `${shortened} and the rest`,
      location: `${shortened} and the place`,
    }));
    expect(session.title).toBe(`${shortened} and the rest`);
    expect(session.location).toBe(`${shortened} and the place`);
    expect(session.titleEditBlocked).toBe(false);
    expect(session.locationEditBlocked).toBe(false);
  });

  it("keeps restored text out of the dirty state of an untouched event", () => {
    const shortened = "T".repeat(WINDOW_TEXT_CHARS);
    const opened = event("long", { title: shortened, windowTitleShortened: true });
    const { session } = controller();
    session.initializeEdit(opened);
    const token = session.beginHeavyLoad(opened.id);
    session.completeHeavyLoad(token, event("long", { title: `${shortened} and the rest` }));
    expect(session.heavyPayload().title).toBe(`${shortened} and the rest`);
    expect(session.heavyPayload()).not.toHaveProperty("location");
  });

  it("does not block a title or location that a window never shortens", () => {
    const whole = "T".repeat(WINDOW_TEXT_CHARS - 1);
    const { session } = controller();
    session.initializeEdit(event("short", { title: whole, location: whole }));
    expect(session.titleEditBlocked).toBe(false);
    expect(session.locationEditBlocked).toBe(false);
  });

  it("blocks only the values the native window reports as shortened, whatever their length", () => {
    const whole = "T".repeat(WINDOW_TEXT_CHARS);
    const { session } = controller();
    session.initializeEdit(event("whole", { title: whole, location: whole }));
    expect(session.titleEditBlocked).toBe(false);
    expect(session.locationEditBlocked).toBe(false);
    session.initializeEdit(event("reported", { title: "Short", windowTitleShortened: true }));
    expect(session.titleEditBlocked).toBe(true);
    expect(session.locationEditBlocked).toBe(false);
  });

  it("takes the current full text when a remote edit changed the start of a shortened value before the late load", () => {
    const stale = "A".repeat(WINDOW_TEXT_CHARS);
    const opened = event("long", { title: stale, location: stale,
      windowTitleShortened: true, windowLocationShortened: true });
    const { session } = controller();
    session.initializeEdit(opened);
    const token = session.beginHeavyLoad(opened.id);
    const remote = event("long", { title: "B".repeat(WINDOW_TEXT_CHARS * 2), location: "C".repeat(WINDOW_TEXT_CHARS * 2) });
    session.completeHeavyLoad(token, remote);
    expect(session.title).toBe(remote.title);
    expect(session.location).toBe(remote.location);
    expect(session.titleEditBlocked).toBe(false);
    expect(session.heavyPayload().title).toBe(remote.title);
  });

  it("keeps a differing occurrence title when the parent event replaces the details", () => {
    const shortened = "T".repeat(WINDOW_TEXT_CHARS);
    const opened = event("instance", { title: shortened, recurringParentId: "parent" });
    const { session } = controller();
    session.initializeEdit(opened);
    const token = session.beginHeavyLoad("parent");
    session.completeHeavyLoad(token, event("parent", { title: "Series title" }));
    expect(session.title).toBe(shortened);
    expect(session.titleEditBlocked).toBe(false);
  });

  it("stays blocked while the full event has not arrived", () => {
    const shortened = "T".repeat(WINDOW_TEXT_CHARS);
    const opened = event("long", { title: shortened, windowTitleShortened: true });
    const { session } = controller();
    session.initializeEdit(opened);
    session.beginHeavyLoad(opened.id);
    expect(session.titleEditBlocked).toBe(true);
    session.initializeCreate({ start: opened.start, end: opened.end }, opened.start, opened.end, false);
    expect(session.titleEditBlocked).toBe(false);
  });

  it("reports a failed full load that leaves a shortened title blocked and recovers on retry", () => {
    const shortened = "T".repeat(WINDOW_TEXT_CHARS);
    const opened = event("instance", { title: shortened, recurringParentId: "parent", windowTitleShortened: true });
    const { session } = controller();
    session.initializeEdit(opened);
    const attempt = session.heavyLoadAttempt;
    const failed = session.beginHeavyLoad("parent");
    expect(session.heavyLoadFailed).toBe(false);

    expect(session.failHeavyLoad(failed)).toBe(true);
    expect(session.heavyLoadFailed).toBe(true);
    expect(session.titleEditBlocked).toBe(true);

    session.retryHeavyLoad();
    expect(session.heavyLoadAttempt).toBe(attempt + 1);
    expect(session.lastFullKey).toBe("");
    const retried = session.beginHeavyLoad("parent");
    expect(session.heavyLoadFailed).toBe(false);
    session.completeHeavyLoad(retried, event("parent", { title: `${shortened} and the rest` }));
    expect(session.title).toBe(`${shortened} and the rest`);
    expect(session.titleEditBlocked).toBe(false);
  });

  it("ignores a failure from a load the panel has moved past", () => {
    const { session } = controller();
    session.initializeEdit(event("opened"));
    const stale = session.beginHeavyLoad("opened");
    session.beginHeavyLoad("next");
    expect(session.failHeavyLoad(stale)).toBe(false);
    expect(session.heavyLoadFailed).toBe(false);
    const current = session.beginHeavyLoad("next");
    session.failHeavyLoad(current);
    session.cancelHeavyLoad();
    expect(session.heavyLoadFailed).toBe(false);
  });

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
