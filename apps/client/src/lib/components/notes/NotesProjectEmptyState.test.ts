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

async function mountEmptyState(projectName: string) {
  const onCreate = vi.fn();
  component = mount(NotesProjectEmptyState, { target: document.body, props: { projectName, onCreate } });
  await tick();
  const section = document.querySelector<HTMLElement>("[data-notes-project-empty]")!;
  return { section, onCreate };
}

describe("Notes project empty state", () => {
  it("names the project and labels the region with its heading", async () => {
    const { section } = await mountEmptyState("Routine");
    const heading = section.querySelector("h2")!;
    expect(heading.textContent).toBe("Routine has no notes yet.");
    expect(section.getAttribute("aria-labelledby")).toBe(heading.id);
  });

  it("creates a note once per click on its action", async () => {
    const { section, onCreate } = await mountEmptyState("Social");
    const button = section.querySelector<HTMLButtonElement>("button")!;
    expect(button.textContent?.trim()).toBe("New note");
    button.click();
    expect(onCreate).toHaveBeenCalledOnce();
  });

  it("shows each part of the platform new note shortcut as a key", async () => {
    const { section } = await mountEmptyState("Social");
    const keys = [...section.querySelectorAll("kbd")].map((key) => key.textContent);
    expect(keys).toEqual(shortcutParts("Mod + N"));
  });
});
