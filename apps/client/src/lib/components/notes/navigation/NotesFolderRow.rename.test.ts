// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesFolder } from "$lib/notes/types";
import NotesFolderRow from "./NotesFolderRow.svelte";

const folder: NotesFolder = {
  object: "folder",
  id: "folder-a",
  project_id: "project-a",
  parent_folder_id: null,
  name: "Original",
  created_time: "2026-07-01T12:00:00.000Z",
  last_edited_time: "2026-07-01T12:00:00.000Z",
};

describe("NotesFolderRow rename", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  /** Mounts the row in rename mode with each rename settled by the returned resolvers, in call order. */
  function renderRow(): Array<(renamed: boolean) => void> {
    const settlers: Array<(renamed: boolean) => void> = [];
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesFolderRow, {
      target,
      props: {
        folder,
        depth: 0,
        collapsed: true,
        renameRequestId: 1,
        moveTargets: [],
        onActivate: vi.fn(),
        onToggleCollapsed: vi.fn(),
        onToggleFavorite: vi.fn(),
        onCreatePage: vi.fn(),
        onCreateFolder: vi.fn(),
        onRename: () => new Promise<boolean>((resolve) => { settlers.push(resolve); }),
        onMove: vi.fn(),
        onDelete: vi.fn(),
      },
    });
    flushSync();
    return settlers;
  }

  function renameInput(): HTMLInputElement {
    const input = target?.querySelector<HTMLInputElement>('input[aria-label="Rename folder"]');
    if (!input) throw new Error("Missing rename input");
    return input;
  }

  function type(value: string): void {
    const input = renameInput();
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  }

  function submit(): void {
    renameInput().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    flushSync();
  }

  function reopenRename(): void {
    target?.querySelector<HTMLButtonElement>(".explorer-row-action")?.click();
    flushSync();
    const rename = [...(target?.querySelectorAll<HTMLButtonElement>('[role="menu"] button') ?? [])]
      .find((button) => button.textContent?.trim() === "Rename folder");
    if (!rename) throw new Error("Missing rename menu item");
    rename.click();
    flushSync();
  }

  function visibleName(): string | undefined {
    return target?.querySelector(".explorer-row-label span")?.textContent?.trim();
  }

  it("keeps a newer pending name when an earlier rename fails later", async () => {
    const settlers = renderRow();
    type("First");
    submit();
    reopenRename();
    type("Second");
    submit();
    expect(settlers).toHaveLength(2);
    expect(visibleName()).toBe("Second");

    settlers[0](false);
    await tick();
    await tick();

    expect(visibleName()).toBe("Second");
  });

  it("keeps the text being typed when the previous rename fails", async () => {
    const settlers = renderRow();
    type("First");
    submit();
    reopenRename();
    type("Typing");

    settlers[0](false);
    await tick();
    await tick();

    expect(renameInput().value).toBe("Typing");
  });
});
