import { describe, expect, it, vi } from "vitest";
import {
  loadProjectOptionalComponent,
  projectOptionalComponentHasLoaded,
} from "$lib/components/projects/component-registry";

vi.mock("@tauri-apps/api/window", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tauri-apps/api/window")>();
  return {
    ...actual,
    getCurrentWindow: () => ({ label: "main" }),
  };
});

describe("Project component registry", () => {
  it("keeps optional surfaces separate and single-flight", async () => {
    const first = loadProjectOptionalComponent("task-finder");
    const second = loadProjectOptionalComponent("task-finder");

    expect(second).toBe(first);
    await expect(first).resolves.toMatchObject({ kind: "task-finder" });
    expect(projectOptionalComponentHasLoaded("task-finder")).toBe(true);
    expect(projectOptionalComponentHasLoaded("toolbar")).toBe(false);
    expect(projectOptionalComponentHasLoaded("bulk-actions")).toBe(false);
  });
});
