import { describe, expect, it, vi } from "vitest";
import {
  loadProjectOptionalComponent,
  projectOptionalComponentHasLoaded,
} from "./component-registry.mobile";

vi.mock("@tauri-apps/api/window", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tauri-apps/api/window")>();
  return {
    ...actual,
    getCurrentWindow: () => ({ label: "main" }),
  };
});

vi.mock("./ProjectToolbarPanels.svelte", () => ({ default: vi.fn() }));
vi.mock("./ProjectBulkActionController.svelte", () => ({ default: vi.fn() }));

describe("mobile Project component registry", () => {
  it("loads shared list support surfaces as cached mobile components", async () => {
    const firstToolbar = loadProjectOptionalComponent("toolbar");
    const secondToolbar = loadProjectOptionalComponent("toolbar");

    expect(secondToolbar).toBe(firstToolbar);
    await expect(firstToolbar).resolves.toMatchObject({ kind: "toolbar" });
    await expect(loadProjectOptionalComponent("bulk-actions"))
      .resolves.toMatchObject({ kind: "bulk-actions" });
    expect(projectOptionalComponentHasLoaded("toolbar")).toBe(true);
    expect(projectOptionalComponentHasLoaded("bulk-actions")).toBe(true);
  });

  it("keeps the keyboard-oriented task finder out of the mobile composition", async () => {
    await expect(loadProjectOptionalComponent("task-finder"))
      .rejects.toThrow("unavailable in the mobile composition");
  });
});
