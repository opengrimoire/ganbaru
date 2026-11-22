// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ThemeConflictError, type ThemeConflict } from "$lib/api/themes";
import ThemeConflictBanner from "./ThemeConflictBanner.svelte";

const api = vi.hoisted(() => ({
  getThemeConflict: vi.fn<(id: string) => Promise<ThemeConflict>>(),
}));

vi.mock("$lib/api/themes", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/api/themes")>()),
  getThemeConflict: api.getThemeConflict,
}));

const LOAD_FAILED = "The versions could not be loaded";

let mounted: ReturnType<typeof mount> | null = null;

afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = null;
  document.body.innerHTML = "";
  api.getThemeConflict.mockReset();
});

async function settle(): Promise<void> {
  for (let round = 0; round < 5; round += 1) {
    await Promise.resolve();
    await tick();
  }
}

function retryButton(target: HTMLElement): HTMLButtonElement {
  const button = [...target.querySelectorAll("button")].find((candidate) =>
    /retry|try again/i.test(`${candidate.getAttribute("aria-label") ?? ""} ${candidate.title} ${candidate.textContent ?? ""}`));
  if (!button) throw new Error("retry button not found");
  return button;
}

describe("ThemeConflictBanner", () => {
  it("clears the load failure when a retry finds the conflict already resolved", async () => {
    api.getThemeConflict.mockRejectedValueOnce(new Error("offline"));
    api.getThemeConflict.mockRejectedValueOnce(new ThemeConflictError("resolved", "gone"));
    const target = document.createElement("div");
    document.body.append(target);
    mounted = mount(ThemeConflictBanner, {
      target,
      props: { themeId: "custom", displayName: "Custom", iconLabel: "light", onResolve: async () => {} },
    });
    flushSync();
    await settle();
    expect(target.textContent).toContain(LOAD_FAILED);

    retryButton(target).click();
    await settle();

    expect(api.getThemeConflict).toHaveBeenCalledTimes(2);
    expect(target.textContent).not.toContain(LOAD_FAILED);
  });
});
