// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import CollectionQuickAdd from "./CollectionQuickAdd.svelte";

/** jsdom lays nothing out and has no `scrollIntoView`, which the field calls as it takes focus. */
const nativeScrollIntoView = Element.prototype.scrollIntoView;
beforeAll(() => {
  Element.prototype.scrollIntoView = () => undefined;
});
afterAll(() => {
  Element.prototype.scrollIntoView = nativeScrollIntoView;
});

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

describe("shared inline creation", () => {
  it("allows blank Notes creation on each click without opening a draft form", async () => {
    const onCreate = vi.fn();
    component = mount(CollectionQuickAdd, { target: document.body, props: { label: "New page", onCreate } });
    const trigger = document.querySelector<HTMLButtonElement>("button")!;
    trigger.click();
    trigger.click();
    await tick();
    expect(onCreate).toHaveBeenCalledTimes(2);
    expect(document.querySelector("form")).toBeNull();
    expect(trigger.disabled).toBe(false);
  });

  it("requires a name for task creation", async () => {
    const onSubmit = vi.fn().mockResolvedValue(true);
    component = mount(CollectionQuickAdd, { target: document.body, props: { label: "Add task", active: true, draft: "   ", onSubmit } });
    document.querySelector("form")!.dispatchEvent(new Event("submit", { cancelable: true }));
    await tick();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("keeps failed drafts and permits a retry without submitting twice while saving", async () => {
    let resolve: (success: boolean) => void = () => {};
    const onSubmit = vi.fn(() => new Promise<boolean>((done) => { resolve = done; }));
    component = mount(CollectionQuickAdd, { target: document.body, props: { label: "Add row", onSubmit } });
    document.querySelector<HTMLButtonElement>("button")!.click();
    await tick();
    await tick();
    const input = document.querySelector<HTMLInputElement>("input")!;
    expect(document.activeElement).toBe(input);
    input.value = "Draft task";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const submit = () => document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    submit();
    submit();
    await tick();
    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(input.disabled).toBe(true);
    resolve(false);
    await tick();
    await tick();
    expect(input.value).toBe("Draft task");
    submit();
    expect(onSubmit).toHaveBeenCalledTimes(2);
    resolve(true);
    await tick();
    await tick();
    expect(input.value).toBe("");
  });

  it("reports rejected writes and only discards the draft on explicit cancellation", async () => {
    const onSubmit = vi.fn().mockRejectedValue(new Error("Write failed"));
    component = mount(CollectionQuickAdd, { target: document.body, props: { label: "Add card", active: true, draft: "Keep me", onSubmit } });
    document.querySelector("form")!.dispatchEvent(new Event("submit", { cancelable: true }));
    await tick();
    await tick();
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("The item could not be added.");
    const input = document.querySelector<HTMLInputElement>("input")!;
    expect(input.value).toBe("Keep me");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await tick();
    expect(document.querySelector("input")).toBeNull();
    expect(document.querySelector('[role="alert"]')).toBeNull();
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("anchors the submit button at the next column and grows the field with a longer draft", async () => {
    const onSubmit = vi.fn().mockResolvedValue(true);
    component = mount(CollectionQuickAdd, { target: document.body, props: { label: "Add task", active: true, draft: "Plan", contentColumn: 3, indent: "1rem", submitAnchor: "16rem", onSubmit } });
    await tick();
    const field = document.querySelector("form > div")!;
    expect(field.getAttribute("style")).toContain("min-width: max(0px, calc(16rem - 1rem - 0.75rem))");
    const sizer = field.querySelector("span[aria-hidden]")!;
    expect(sizer.textContent).toBe("Plan ");
    const input = field.querySelector("input")!;
    input.value = "Plan the release checklist";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(sizer.textContent).toBe("Plan the release checklist ");
    expect(field.nextElementSibling?.getAttribute("type")).toBe("submit");
  });

  it("keeps the field full width without an anchor", async () => {
    const onSubmit = vi.fn().mockResolvedValue(true);
    component = mount(CollectionQuickAdd, { target: document.body, props: { label: "Add card", active: true, onSubmit } });
    await tick();
    const field = document.querySelector("form > div")!;
    expect(field.getAttribute("style")).toBeNull();
    expect(field.querySelector("span[aria-hidden]")).toBeNull();
  });
});
