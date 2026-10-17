import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyCalendarEventProjectAssignments,
  registerCalendarEventProjectAssignmentHandler,
} from "./calendar-assignments";

const assignment = { eventId: "event-1", projectId: "project-1" };

describe("calendar event project assignments", () => {
  const cleanups: Array<() => void> = [];

  afterEach(() => {
    for (const cleanup of cleanups.splice(0)) cleanup();
  });

  it("does nothing when no Calendar store is registered", async () => {
    await expect(applyCalendarEventProjectAssignments([assignment])).resolves.toBeUndefined();
  });

  it("forwards assignments to the registered Calendar handler", async () => {
    const handler = vi.fn(async () => undefined);
    cleanups.push(registerCalendarEventProjectAssignmentHandler(handler));

    await applyCalendarEventProjectAssignments([assignment]);

    expect(handler).toHaveBeenCalledWith([assignment]);
  });

  it("skips the handler for an empty assignment list", async () => {
    const handler = vi.fn(async () => undefined);
    cleanups.push(registerCalendarEventProjectAssignmentHandler(handler));

    await applyCalendarEventProjectAssignments([]);

    expect(handler).not.toHaveBeenCalled();
  });

  it("propagates handler failures to the caller", async () => {
    cleanups.push(registerCalendarEventProjectAssignmentHandler(async () => {
      throw new Error("calendar unavailable");
    }));

    await expect(applyCalendarEventProjectAssignments([assignment])).rejects.toThrow("calendar unavailable");
  });

  it("keeps a newer registration when a replaced handler unregisters", async () => {
    const first = vi.fn(async () => undefined);
    const second = vi.fn(async () => undefined);
    const unregisterFirst = registerCalendarEventProjectAssignmentHandler(first);
    cleanups.push(registerCalendarEventProjectAssignmentHandler(second));

    unregisterFirst();
    await applyCalendarEventProjectAssignments([assignment]);

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith([assignment]);
  });
});
