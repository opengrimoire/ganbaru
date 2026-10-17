// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import CollectionQuickAdd from "./CollectionQuickAdd.svelte";

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
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("Write failed");
    const input = document.querySelector<HTMLInputElement>("input")!;
    expect(input.value).toBe("Keep me");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await tick();
    expect(document.querySelector("input")).toBeNull();
    expect(document.querySelector('[role="alert"]')).toBeNull();
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });
});
