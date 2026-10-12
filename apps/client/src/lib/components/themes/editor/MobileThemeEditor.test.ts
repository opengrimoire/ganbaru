// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MobileThemeEditor from "./MobileThemeEditor.svelte";

const state = vi.hoisted(() => {
  const layers: Array<() => void> = [];
  const editor = {
    editingId: "custom" as string | undefined,
    hasUnsavedChanges: true,
    cancel: vi.fn(async () => {
      editor.editingId = undefined;
    }),
    commit: vi.fn(async () => {
      editor.editingId = undefined;
    }),
  };
  return {
    layers,
    editor,
    backStack: {
      get hasActiveLayer(): boolean {
        return layers.length > 0;
      },
      activate(layer: { handle: () => void }): () => void {
        layers.push(layer.handle);
        return () => {
          const index = layers.lastIndexOf(layer.handle);
          if (index >= 0) layers.splice(index, 1);
        };
      },
      consume(): boolean {
        const handle = layers.at(-1);
        if (!handle) return false;
        handle();
        return true;
      },
    },
  };
});

vi.mock("$lib/stores/theme.svelte", () => ({
  getTheme: () => ({
    registry: {
      custom: { id: "custom", displayName: "Custom", kind: "user" },
    },
    isBuiltin: () => false,
    canResetThemeToSeed: () => false,
    resetThemeToSeed: vi.fn(),
  }),
}));

vi.mock("$lib/stores/theme-editor.svelte", () => ({
  getThemeEditor: () => state.editor,
}));

vi.mock("$lib/stores/mobile-back-stack.svelte", () => ({
  getMobileBackStack: () => state.backStack,
}));

vi.mock("$lib/platform", () => ({
  BUILD_PLATFORM_PROFILE: {
    platform: "android",
    shell: "mobile",
    capabilities: ["system.android-back"],
  },
  platformHasCapability: (_profile: unknown, capability: string) =>
    capability === "system.android-back",
}));

vi.mock("./ThemeEditor.svelte", async () => ({
  default: (await import("./MobileThemeEditorStub.test.svelte")).default,
}));

async function flushEffects(): Promise<void> {
  await tick();
  await new Promise<void>((resolve) => queueMicrotask(resolve));
}

describe("MobileThemeEditor", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    state.layers.splice(0);
    state.editor.editingId = "custom";
    state.editor.hasUnsavedChanges = true;
    state.editor.cancel.mockClear();
    state.editor.commit.mockClear();
  });

  it("fills the visual viewport and exposes mobile-sized session actions", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MobileThemeEditor, { target });
    await flushEffects();

    const panel = target.querySelector<HTMLElement>(".mobile-theme-editor");
    expect(panel?.style.left).toBe("var(--visual-viewport-offset-left)");
    expect(panel?.style.height).toBe("var(--visual-viewport-height)");
    expect(target.querySelector("[data-theme-editor-stub]")).not.toBeNull();
    expect(target.textContent).toContain("Save and apply");
    expect(target.querySelector('[aria-label="Back to themes"]')?.classList)
      .toContain("size-12");
  });

  it("asks before Android Back discards a dirty session", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MobileThemeEditor, { target });
    await flushEffects();

    expect(state.backStack.consume()).toBe(true);
    await flushEffects();
    expect(target.querySelector(".confirm-dialog")).not.toBeNull();
    expect(state.editor.cancel).not.toHaveBeenCalled();

    expect(state.backStack.consume()).toBe(true);
    await flushEffects();
    expect(target.querySelector(".confirm-dialog")).toBeNull();
    expect(state.editor.cancel).not.toHaveBeenCalled();

    expect(state.backStack.consume()).toBe(true);
    await flushEffects();
    target.querySelectorAll<HTMLButtonElement>(".confirm-dialog button").item(1).click();
    await flushEffects();
    expect(state.editor.cancel).toHaveBeenCalledOnce();
  });

  it("cancels a clean read without a confirmation", async () => {
    state.editor.hasUnsavedChanges = false;
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MobileThemeEditor, { target });
    await flushEffects();

    expect(state.backStack.consume()).toBe(true);
    await flushEffects();
    expect(target.querySelector(".confirm-dialog")).toBeNull();
    expect(state.editor.cancel).toHaveBeenCalledOnce();
  });

  it("does not roll back a session while unmounting after Save", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MobileThemeEditor, { target });
    await flushEffects();

    const saveButton = [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes("Save and apply"));
    expect(saveButton).toBeDefined();
    saveButton?.click();
    await flushEffects();
    expect(state.editor.commit).toHaveBeenCalledOnce();

    await unmount(component);
    component = undefined;
    expect(state.editor.cancel).not.toHaveBeenCalled();
  });
});
