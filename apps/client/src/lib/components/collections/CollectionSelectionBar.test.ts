// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import SelectionBar from "./CollectionSelectionBarHarness.test.svelte";

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

async function mountBar(): Promise<ReturnType<typeof vi.fn>> {
  const onClear = vi.fn();
  component = mount(SelectionBar, { target: document.body, props: { onClear } });
  await tick();
  return onClear;
}

function press(testId: string): void {
  document.querySelector(`[data-testid="${testId}"]`)?.dispatchEvent(new Event("pointerdown", { bubbles: true }));
}

describe("collection selection bar outside presses", () => {
  it("clears the selection when a press lands outside the collection", async () => {
    const onClear = await mountBar();
    press("outside");
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("keeps the selection for presses on rows and on the bar itself", async () => {
    const onClear = await mountBar();
    press("inside");
    press("action");
    expect(onClear).not.toHaveBeenCalled();
  });

  it("keeps the selection for presses on floating surfaces and while a modal dialog is open", async () => {
    const onClear = await mountBar();
    press("floating");
    const dialog = document.createElement("div");
    dialog.setAttribute("aria-modal", "true");
    document.body.append(dialog);
    press("outside");
    expect(onClear).not.toHaveBeenCalled();
  });
});
