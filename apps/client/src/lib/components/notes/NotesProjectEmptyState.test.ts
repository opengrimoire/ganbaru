// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { shortcutParts } from "$lib/keyboard-shortcuts";
import NotesProjectEmptyState from "./NotesProjectEmptyState.svelte";

let component: ReturnType<typeof mount> | undefined;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

async function mountEmptyState() {
  const onCreate = vi.fn();
  component = mount(NotesProjectEmptyState, { target: document.body, props: { onCreate } });
  await tick();
  const emptyState = document.querySelector<HTMLElement>("[data-notes-project-empty]")!;
  return { emptyState, onCreate };
}

describe("Notes project empty state", () => {
  it("creates a note once per click on its action", async () => {
    const { emptyState, onCreate } = await mountEmptyState();
    const button = emptyState.querySelector<HTMLButtonElement>("button")!;
    expect(button.textContent?.trim()).toBe("New note");
    button.click();
    expect(onCreate).toHaveBeenCalledOnce();
  });

  it("shows each part of the platform new note shortcut as a key", async () => {
    const { emptyState } = await mountEmptyState();
    const keys = [...emptyState.querySelectorAll("kbd")].map((key) => key.textContent);
    expect(keys).toEqual(shortcutParts("Mod + N"));
  });
});
