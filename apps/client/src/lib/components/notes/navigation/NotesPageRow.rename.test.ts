// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesPage } from "$lib/notes/types";
import NotesPageRow from "./NotesPageRow.svelte";

const page: NotesPage = {
  object: "page",
  id: "page-a",
  created_time: "2026-07-01T12:00:00.000Z",
  last_edited_time: "2026-07-01T12:00:00.000Z",
  parent: { type: "workspace", workspace: true },
  folder_id: null,
  in_trash: false,
  archived: false,
  icon: null,
  cover: null,
  properties: {
    title: { id: "title", type: "title", title: [createRichText("Original")] },
  },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

describe("NotesPageRow rename", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  /** Mounts the row with each rename settled by the returned resolvers, in call order. */
  function renderRow(): Array<(renamed: boolean) => void> {
    const settlers: Array<(renamed: boolean) => void> = [];
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesPageRow, {
      target,
      props: {
        page,
        depth: 0,
        hasChildren: false,
        collapsed: false,
        parentStatus: null,
        favorited: false,
        selected: false,
        onSelect: vi.fn(),
        onRename: () => new Promise<boolean>((resolve) => { settlers.push(resolve); }),
        onToggleCollapsed: vi.fn(),
        onToggleFavorite: vi.fn(),
        onCreateChild: vi.fn(),
        onDuplicate: vi.fn(),
        moveTargets: [],
        onMove: vi.fn(),
        onArchive: vi.fn(),
        onTrash: vi.fn(),
      },
    });
    flushSync();
    return settlers;
  }

  function renameInput(): HTMLInputElement {
    const input = target?.querySelector<HTMLInputElement>('input[aria-label="Rename note"]');
    if (!input) throw new Error("Missing rename input");
    return input;
  }

  function openRename(): void {
    target?.querySelector<HTMLButtonElement>(".explorer-row-action")?.click();
    flushSync();
    const rename = [...(target?.querySelectorAll<HTMLButtonElement>('[role="menu"] button') ?? [])]
      .find((button) => button.textContent?.trim() === "Rename note");
    if (!rename) throw new Error("Missing rename menu item");
    rename.click();
    flushSync();
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

  function visibleTitle(): string | undefined {
    return target?.querySelector(".explorer-row-label span, span.truncate")?.textContent?.trim();
  }

  it("keeps a newer pending title when an earlier rename fails later", async () => {
    const settlers = renderRow();
    openRename();
    type("First");
    submit();
    openRename();
    type("Second");
    submit();
    expect(settlers).toHaveLength(2);
    expect(visibleTitle()).toBe("Second");

    settlers[0](false);
    await tick();
    await tick();

    expect(visibleTitle()).toBe("Second");
  });

  it("keeps the text being typed when the previous rename fails", async () => {
    const settlers = renderRow();
    openRename();
    type("First");
    submit();
    openRename();
    type("Typing");

    settlers[0](false);
    await tick();
    await tick();

    expect(renameInput().value).toBe("Typing");
  });
});
