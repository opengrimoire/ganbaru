import { describe, expect, it } from "vitest";
import { ProjectConflictError, mapProjectConflicts, toProjectConflictError } from "./project-conflicts";

const device = { deviceId: "device-a", deviceLabel: "Phone", ownDevice: false };

describe("project conflict mapping", () => {
  it("maps field and dependency conflicts", () => {
    const read = mapProjectConflicts({
      projectId: "project-1",
      fields: [{
        target: "checklist_item",
        id: "item-1",
        field: "title",
        versions: [{ version: "ab", device, editedAtMs: 5, displayed: true, text: "Buy milk" }],
      }],
      dependencies: [{
        key: "k",
        blockingTaskId: "t1",
        blockedTaskId: "t2",
        blockingTaskTitle: "One",
        blockedTaskTitle: "Two",
        wouldCycle: true,
        device,
        addedAtMs: 9,
      }],
    });
    expect(read.fields[0]?.target).toBe("checklist_item");
    expect(read.fields[0]?.versions[0]?.text).toBe("Buy milk");
    expect(read.dependencies[0]?.wouldCycle).toBe(true);
  });

  it("rejects a field the native side does not define", () => {
    expect(() => mapProjectConflicts({
      projectId: "p",
      fields: [{ target: "task", id: "t", field: "notes", versions: [] }],
      dependencies: [],
    })).toThrow("project conflicts.fields[0].field has an unsupported value");
  });

  it("rejects a malformed read", () => {
    expect(() => mapProjectConflicts({ projectId: "p", fields: {}, dependencies: [] })).toThrow();
    expect(() => mapProjectConflicts(null)).toThrow();
  });
});

describe("project conflict errors", () => {
  it("turns known native codes into typed errors", () => {
    for (const code of ["resolved", "would_cycle", "failed"] as const) {
      const error = toProjectConflictError({ code, message: "text" });
      expect(error).toBeInstanceOf(ProjectConflictError);
      expect(error).toMatchObject({ code, message: "text" });
    }
  });

  it("passes other rejections through", () => {
    const unknownCode = { code: "other", message: "text" };
    expect(toProjectConflictError(unknownCode)).toBe(unknownCode);
    expect(toProjectConflictError("plain")).toBe("plain");
  });
});
