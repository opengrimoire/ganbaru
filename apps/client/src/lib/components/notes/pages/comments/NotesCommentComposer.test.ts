// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesPage } from "$lib/notes/types";
import NotesCommentComposer from "./NotesCommentComposer.svelte";

const editor = vi.hoisted(() => ({
  store: {
    allPages: [] as NotesPage[],
    localUser: null,
  },
}));

vi.mock("$lib/components/notes/editor-context", () => ({
  getNotesEditor: () => editor.store,
}));

describe("NotesCommentComposer", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  function textbox(): HTMLElement {
    const element = target?.querySelector<HTMLElement>('[role="textbox"]');
    if (!element) throw new Error("Comment text field is missing");
    return element;
  }

  it("keeps text typed while the comment was posting", async () => {
    let finishPost: (posted: boolean) => void = () => undefined;
    const onSubmit = vi.fn(() => new Promise<boolean>((resolve) => { finishPost = resolve; }));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesCommentComposer, {
      target,
      props: { label: "Comment", placeholder: "Add a comment", initialRichText: [createRichText("First")], onSubmit },
    });
    await tick();

    target.querySelector<HTMLButtonElement>('button[aria-label="Send"]')?.click();
    await tick();
    expect(onSubmit).toHaveBeenCalledTimes(1);

    textbox().dispatchEvent(new InputEvent("beforeinput", { bubbles: true, cancelable: true, inputType: "insertText", data: " more" }));
    await tick();
    finishPost(true);
    await vi.waitFor(() => expect(target?.querySelector('button[aria-label="Send"]')?.hasAttribute("aria-busy")).toBe(false));
    await tick();

    expect(textbox().textContent).toBe("First more");
  });

  it("clears the posted text when nothing changed during the post", async () => {
    const onSubmit = vi.fn(() => Promise.resolve(true));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesCommentComposer, {
      target,
      props: { label: "Comment", placeholder: "Add a comment", initialRichText: [createRichText("First")], onSubmit },
    });
    await tick();

    target.querySelector<HTMLButtonElement>('button[aria-label="Send"]')?.click();
    await vi.waitFor(() => expect(textbox().textContent).toBe(""));
  });
});
