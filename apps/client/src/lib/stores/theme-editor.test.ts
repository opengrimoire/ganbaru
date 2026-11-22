import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ThemeSaveOutcome } from "$lib/themes/editor-save";

const theme = vi.hoisted(() => ({
  id: "light",
  persistThemeToDb: vi.fn<(id: string) => Promise<ThemeSaveOutcome>>(),
  setEditingTheme: vi.fn(),
  setTheme: vi.fn(),
  discardFreshTheme: vi.fn(),
  restoreThemeFromSnapshot: vi.fn(),
  exportTheme: vi.fn(() => "{}"),
}));

vi.mock("./theme.svelte", () => ({ getTheme: () => theme }));

/** Creates a save the test settles by hand. */
function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((res) => { resolve = res; });
  return { promise, resolve };
}

describe("getThemeEditor", () => {
  beforeEach(async () => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  async function openFresh() {
    const { getThemeEditor } = await import("./theme-editor.svelte");
    const editor = getThemeEditor();
    editor.open("fresh-theme", { createdFresh: true, previousActiveId: "light" });
    return editor;
  }

  it("saves once when Save is pressed twice before the first save lands", async () => {
    const editor = await openFresh();
    const save = deferred<ThemeSaveOutcome>();
    theme.persistThemeToDb.mockReturnValueOnce(save.promise);

    const first = editor.commit();
    const second = editor.commit();
    save.resolve({ kind: "saved" });

    await expect(Promise.all([first, second])).resolves.toEqual([{ kind: "saved" }, { kind: "saved" }]);
    expect(theme.persistThemeToDb).toHaveBeenCalledOnce();
    expect(editor.isActive).toBe(false);
  });

  it("keeps a theme that is being saved when Cancel is pressed meanwhile", async () => {
    const editor = await openFresh();
    const save = deferred<ThemeSaveOutcome>();
    theme.persistThemeToDb.mockReturnValueOnce(save.promise);

    const committing = editor.commit();
    const cancelling = editor.cancel();
    save.resolve({ kind: "saved" });
    await Promise.all([committing, cancelling]);

    expect(theme.discardFreshTheme).not.toHaveBeenCalled();
    expect(theme.setTheme).not.toHaveBeenCalled();
    expect(editor.isActive).toBe(false);
  });

  it("cancels after a save that stopped at a conflict", async () => {
    const { getThemeEditor } = await import("./theme-editor.svelte");
    const editor = getThemeEditor();
    editor.open("custom", { snapshot: "{\"before\":true}", previousActiveId: "light" });
    const save = deferred<ThemeSaveOutcome>();
    theme.persistThemeToDb.mockReturnValueOnce(save.promise);

    const committing = editor.commit();
    const cancelling = editor.cancel();
    save.resolve({ kind: "conflict", groups: [] });
    await Promise.all([committing, cancelling]);

    expect(theme.restoreThemeFromSnapshot).toHaveBeenCalledWith("custom", "{\"before\":true}");
    expect(editor.isActive).toBe(false);
  });

  it("lets Cancel close the editor after a failed save", async () => {
    const editor = await openFresh();
    theme.persistThemeToDb.mockRejectedValueOnce(new Error("database is locked"));

    await expect(editor.commit()).rejects.toThrow("database is locked");
    expect(editor.isActive).toBe(true);
    await editor.cancel();

    expect(theme.discardFreshTheme).toHaveBeenCalledWith("fresh-theme");
    expect(editor.isActive).toBe(false);
  });
});
