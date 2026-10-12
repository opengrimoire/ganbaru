// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import CollectionPropertyNameFieldHarness from "./CollectionPropertyNameFieldHarness.test.svelte";

let component: Record<string, unknown> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

/** Mount the field for the Points property with a resolving rename by default. */
function render(options: { editable?: boolean } = {}) {
  const onRename = vi.fn<(name: string) => Promise<void>>().mockResolvedValue(undefined);
  const mounted = mount(CollectionPropertyNameFieldHarness, { target: document.body, props: { initialPropertyId: "points", initialName: "Points", onRename, ...options } });
  component = mounted;
  flushSync();
  return { show: (propertyId: string, name: string) => { mounted.show(propertyId, name); flushSync(); }, onRename };
}

function input(): HTMLInputElement {
  return document.querySelector<HTMLInputElement>('input[aria-label="Property name"]')!;
}

function edit(value: string): void {
  input().dispatchEvent(new FocusEvent("focus"));
  input().value = value;
  input().dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

function enter(): void {
  input().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
}

describe("Collection property name field", () => {
  it("renames with the trimmed name on Enter", async () => {
    const { onRename } = render();
    edit("  Story points  ");
    enter();
    await vi.waitFor(() => expect(onRename).toHaveBeenCalledExactlyOnceWith("Story points"));
  });

  it("restores the canonical name for a blank or unchanged draft without saving", () => {
    const { onRename } = render();
    edit("   ");
    input().dispatchEvent(new FocusEvent("blur"));
    flushSync();
    expect(input().value).toBe("Points");
    edit(" Points ");
    enter();
    flushSync();
    expect(onRename).not.toHaveBeenCalled();
    expect(input().value).toBe("Points");
  });

  it("keeps a rejected draft with an explicit error and does not retry it when the menu closes", async () => {
    const { onRename } = render();
    onRename.mockRejectedValueOnce(new Error("Disk full"));
    edit("Estimate");
    enter();
    await vi.waitFor(() => expect(document.querySelector('[role="alert"]')?.textContent).toBe("Could not rename property: Disk full"));
    expect(input().value).toBe("Estimate");
    await unmount(component!);
    component = undefined;
    expect(onRename).toHaveBeenCalledOnce();
  });

  it("saves a pending draft when its menu closes before the field blurs", async () => {
    const { onRename } = render();
    edit("Estimate");
    await unmount(component!);
    component = undefined;
    expect(onRename).toHaveBeenCalledExactlyOnceWith("Estimate");
  });

  it("follows canonical renames while pristine and keeps an active draft", () => {
    const { show } = render();
    show("points", "Effort");
    expect(input().value).toBe("Effort");
    edit("Draft");
    show("points", "Remote");
    expect(input().value).toBe("Draft");
  });

  it("resets the draft and error when it starts showing another property", async () => {
    const { show, onRename } = render();
    onRename.mockRejectedValueOnce(new Error("Disk full"));
    edit("Estimate");
    enter();
    await vi.waitFor(() => expect(document.querySelector('[role="alert"]')).not.toBeNull());
    show("due", "Due");
    await tick();
    expect(input().value).toBe("Due");
    expect(document.querySelector('[role="alert"]')).toBeNull();
  });

  it("is read only and never saves when the property cannot be edited", async () => {
    const { onRename } = render({ editable: false });
    expect(input().readOnly).toBe(true);
    edit("Estimate");
    enter();
    await tick();
    expect(onRename).not.toHaveBeenCalled();
  });
});
