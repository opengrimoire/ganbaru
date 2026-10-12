// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { TitleBarShortcutControllerContext } from "./shortcut-controller.svelte";

import ShortcutHost from "./TitleBarShortcutsHarness.test.svelte";

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

describe("shell shortcuts during editing", () => {
  it("captures app commands inside inputs and contenteditable before editor handlers", async () => {
    const context: TitleBarShortcutControllerContext = {
      benchmarkLocked: () => false, themeEditorLocked: () => false,
      ensureBenchmarkOverlay: vi.fn(async () => {}), showResetSequenceConfirmation: vi.fn(),
      closeOtherConfirmations: vi.fn(), requestClose: vi.fn(async () => {}),
      toggleTheme: vi.fn(), openThemeSwitcher: vi.fn(), zoomIn: vi.fn(), zoomOut: vi.fn(),
      resetZoom: vi.fn(), toggleMusic: vi.fn(), toggleDiagnostics: vi.fn(), openShortcutHelp: vi.fn(),
    };
    component = mount(ShortcutHost, { target: document.body, props: { context } });
    await tick();
    const commands = [
      { key: "m", ctrlKey: true, action: context.toggleMusic },
      { key: "l", ctrlKey: true, shiftKey: true, action: context.toggleTheme },
      { key: "t", ctrlKey: true, shiftKey: true, action: context.openThemeSwitcher },
      { key: "d", ctrlKey: true, shiftKey: true, action: context.toggleDiagnostics },
      { key: "=", ctrlKey: true, action: context.zoomIn },
      { key: "F1", action: context.openShortcutHelp },
    ];
    for (const tag of ["input", "div"] as const) {
      const editor = document.createElement(tag);
      if (tag === "div") editor.setAttribute("contenteditable", "true");
      document.body.append(editor);
      const editorHandler = vi.fn((event: Event) => event.stopPropagation());
      editor.addEventListener("keydown", editorHandler);
      for (const { action, ...keys } of commands) {
        vi.mocked(action).mockClear();
        const event = new KeyboardEvent("keydown", { ...keys, bubbles: true, cancelable: true });
        editor.dispatchEvent(event);
        expect(action).toHaveBeenCalledTimes(1);
        expect(event.defaultPrevented).toBe(true);
      }
      expect(editorHandler).not.toHaveBeenCalled();
      for (const keys of [{ key: "Tab" }, { key: "b", ctrlKey: true }, { key: "m", ctrlKey: true, isComposing: true }]) {
        const event = new KeyboardEvent("keydown", { ...keys, bubbles: true, cancelable: true });
        editor.dispatchEvent(event);
        expect(event.defaultPrevented).toBe(false);
      }
      expect(editorHandler).toHaveBeenCalledTimes(3);
    }
  });
});
