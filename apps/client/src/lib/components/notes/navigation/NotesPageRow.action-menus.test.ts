// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesFolder, NotesPage } from "$lib/notes/types";
import NotesFolderRow from "./NotesFolderRow.svelte";
import NotesPageRow from "./NotesPageRow.svelte";

const timestamp = "2026-07-01T12:00:00.000Z";
const page: NotesPage = {
  object: "page",
  id: "page-a",
  created_time: timestamp,
  last_edited_time: timestamp,
  parent: { type: "workspace", workspace: true },
  folder_id: null,
  in_trash: false,
  archived: false,
  icon: null,
  cover: null,
  properties: { title: { id: "title", type: "title", title: [createRichText("Page A")] } },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};
const folder: NotesFolder = {
  object: "folder",
  id: "folder-a",
  project_id: "project-a",
  parent_folder_id: null,
  name: "Folder A",
  created_time: timestamp,
  last_edited_time: timestamp,
};

describe("Notes row action menus", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
  });

  async function expectToggle(): Promise<void> {
    const trigger = target.querySelector<HTMLButtonElement>(".explorer-row-action");
    expect(trigger).not.toBeNull();
    for (const expectedOpen of [true, false, true]) {
      trigger?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      trigger?.click();
      await tick();
      expect(target.querySelector('[role="menu"]') !== null).toBe(expectedOpen);
      expect(trigger?.getAttribute("aria-expanded")).toBe(String(expectedOpen));
    }
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await tick();
    expect(target.querySelector('[role="menu"]')).toBeNull();
  }

  function expectOverflowTooltip(): void {
    const label = target.querySelector<HTMLElement>(".explorer-row-label > .truncate");
    if (!label) throw new Error("Notes row label is missing");
    let renderedWidth = 80;
    Object.defineProperties(label, {
      scrollWidth: { configurable: true, get: () => renderedWidth },
      clientWidth: { configurable: true, value: 100 },
    });

    label.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    expect(label.dataset.appTooltip).toBeUndefined();

    renderedWidth = 150;
    label.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    expect(label.dataset.appTooltip).toBe(label.textContent);

    renderedWidth = 100;
    label.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    expect(label.dataset.appTooltip).toBeUndefined();
  }

  it("toggles the page menu and shows its name only when clipped", async () => {
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
        onRename: vi.fn(),
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
    await tick();
    expectOverflowTooltip();
    await expectToggle();
  });

  it("toggles the folder menu and shows its name only when clipped", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesFolderRow, {
      target,
      props: {
        folder,
        depth: 0,
        collapsed: false,
        moveTargets: [],
        onActivate: vi.fn(),
        onToggleCollapsed: vi.fn(),
        onCreatePage: vi.fn(),
        onCreateFolder: vi.fn(),
        onRename: vi.fn(),
        onMove: vi.fn(),
        onDelete: vi.fn(),
      },
    });
    await tick();
    expectOverflowTooltip();
    await expectToggle();
  });
});
