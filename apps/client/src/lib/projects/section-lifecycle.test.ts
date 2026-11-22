import { describe, expect, it } from "vitest";
import { sectionLifecycle, sectionLifecycleChange } from "./section-lifecycle";

const at = "2026-11-22T00:00:00.000Z";

describe("sectionLifecycle", () => {
  it("reads active, hidden, and archived sections from their times", () => {
    expect(sectionLifecycle({})).toBe("active");
    expect(sectionLifecycle({ hiddenAt: at })).toBe("hidden");
    expect(sectionLifecycle({ archivedAt: at })).toBe("archived");
  });

  it("treats a section that carries both times as archived", () => {
    expect(sectionLifecycle({ hiddenAt: at, archivedAt: at })).toBe("archived");
  });
});

describe("sectionLifecycleChange", () => {
  it("expects the state the section was read in", () => {
    expect(sectionLifecycleChange({ id: "section-1", hiddenAt: at }, "archived")).toEqual({
      sectionId: "section-1",
      expected: "hidden",
      next: "archived",
    });
    expect(sectionLifecycleChange({ id: "section-1", archivedAt: at }, "active")).toEqual({
      sectionId: "section-1",
      expected: "archived",
      next: "active",
    });
  });

  it("asks for no move when the section is already there", () => {
    expect(sectionLifecycleChange({ id: "section-1" }, "active")).toBeNull();
    expect(sectionLifecycleChange({ id: "section-1", hiddenAt: at }, "hidden")).toBeNull();
  });
});
