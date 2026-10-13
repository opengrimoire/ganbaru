import { describe, expect, it } from "vitest";
import { resolveProjectSidebarGroupId } from "./sidebar";

describe("resolveProjectSidebarGroupId", () => {
  const groups = ["health", "work", "home"];

  it("lists the browsed group over the selected project's group", () => {
    expect(resolveProjectSidebarGroupId("home", "work", groups)).toBe("home");
  });

  it("falls back to the selected project's group without a browsed group", () => {
    expect(resolveProjectSidebarGroupId(null, "work", groups)).toBe("work");
  });

  it("drops a browsed group that is no longer visible", () => {
    expect(resolveProjectSidebarGroupId("deleted", "work", groups)).toBe("work");
  });

  it("uses the first visible group when neither group is visible", () => {
    expect(resolveProjectSidebarGroupId("deleted", "hidden", groups)).toBe("health");
  });

  it("returns null when there are no visible groups", () => {
    expect(resolveProjectSidebarGroupId("home", "work", [])).toBeNull();
  });
});
