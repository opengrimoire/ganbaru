// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesPage } from "$lib/notes/types";

const loader = vi.hoisted(() => ({
  calls: [] as string[],
  unresolved: new Promise<never>(() => undefined),
}));

vi.mock("$lib/components/notes/component-registry", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/components/notes/component-registry")>();
  return {
    ...actual,
    loadNotesOptionalComponent: (kind: string) => {
      loader.calls.push(kind);
      return loader.unresolved;
    },
  };
});

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
    title: { id: "title", type: "title", title: [createRichText("Page A")] },
  },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

describe("NotesPageRow lazy actions", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    loader.calls.length = 0;
    component = undefined;
    target = undefined;
  });

  it("does not request the destination picker until the move submenu opens", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const { default: NotesPageRow } = await import("./NotesPageRow.svelte");
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
        onRename: vi.fn(),
        onToggleCollapsed: vi.fn(),
        onToggleFavorite: vi.fn(),
        onCreateChild: vi.fn(),
        onDuplicate: vi.fn(),
        moveTargets: [{
          key: "workspace",
          title: "Workspace",
          path: [],
          depth: 0,
          recent: false,
          parent: { type: "workspace", workspace: true },
          pageId: null,
        }],
        onMove: vi.fn(),
        onArchive: vi.fn(),
        onTrash: vi.fn(),
      },
    });
    await tick();
    expect(loader.calls).toEqual([]);

    const actionButton = target.querySelector<HTMLButtonElement>(".explorer-row-action");
    expect(actionButton).not.toBeNull();
    actionButton?.click();
    await tick();
    expect(loader.calls).toEqual([]);
    expect(target.querySelector('[role="menu"]')?.contains(document.activeElement)).toBe(true);
    target.querySelector('[role="menu"]')?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(target.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(actionButton);
    actionButton?.click();
    await tick();

    const moveButton = target.querySelector<HTMLButtonElement>('button[aria-expanded="false"]');
    expect(moveButton).not.toBeNull();
    moveButton?.click();
    await tick();

    expect(loader.calls).toEqual(["destination-picker"]);
    expect(target.querySelector('[aria-busy="true"]')).not.toBeNull();
  });
});
