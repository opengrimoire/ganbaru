// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ThemeJsonSection from "./ThemeJsonSection.svelte";

describe("ThemeJsonSection file feedback", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("disables the file action and shows progress while saving", async () => {
    const onSave = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ThemeJsonSection, {
      target,
      props: {
        isBuiltin: true,
        jsonDraft: "{}",
        jsonDirty: false,
        jsonErrors: [],
        jsonNotice: undefined,
        jsonSaving: true,
        fileSaveAvailable: true,
        onCopy: vi.fn(),
        onSave,
        onApply: vi.fn(),
        onReset: vi.fn(),
        onInput: vi.fn(),
      },
    });
    await tick();

    const saveButton = [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes("Saving"));
    expect(saveButton?.disabled).toBe(true);
    expect(saveButton?.getAttribute("aria-busy")).toBe("true");
    expect(saveButton?.querySelector(".animate-spin")).not.toBeNull();

    saveButton?.click();
    expect(onSave).not.toHaveBeenCalled();
  });
});
