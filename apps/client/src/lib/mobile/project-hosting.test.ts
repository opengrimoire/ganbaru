import { describe, expect, it } from "vitest";
import { mobileWorkspaceSurface } from "./project-hosting";

describe("mobileWorkspaceSurface", () => {
  it("shows the selected project's open Notes or Chat view inside the Projects tab", () => {
    expect(mobileWorkspaceSurface("projects", { hostedPlace: "notes", projectSelected: true })).toBe("notes");
    expect(mobileWorkspaceSurface("projects", { hostedPlace: "chat", projectSelected: true })).toBe("chat");
  });

  it("keeps the Projects workspace while a task view is open or nothing scopes the hosted view", () => {
    expect(mobileWorkspaceSurface("projects", { hostedPlace: null, projectSelected: true })).toBe("projects");
    expect(mobileWorkspaceSurface("projects", { hostedPlace: "notes", projectSelected: false })).toBe("projects");
    expect(mobileWorkspaceSurface("projects", null)).toBe("projects");
  });

  it("leaves other tabs on their own workspace", () => {
    expect(mobileWorkspaceSurface("calendar", { hostedPlace: "chat", projectSelected: true })).toBe("calendar");
    expect(mobileWorkspaceSurface("notes", { hostedPlace: "chat", projectSelected: true })).toBe("notes");
    expect(mobileWorkspaceSurface("chat", { hostedPlace: "notes", projectSelected: true })).toBe("chat");
  });
});
