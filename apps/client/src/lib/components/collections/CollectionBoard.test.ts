// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import Board from "./CollectionBoardHarness.test.svelte";

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

/** Dispatch a local board drag without accepting data from external drags. */
function drag(node: Element, type: string): void {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, { clientY: { value: 1 }, dataTransfer: { value: { setData: vi.fn(), effectAllowed: "none", dropEffect: "none" } } });
  node.dispatchEvent(event);
}

describe("shared Kanban movement", () => {
  it("ignores external and same-group drops and submits a local move once", async () => {
    let complete: () => void = () => {};
    const onMove = vi.fn(() => new Promise<void>((resolve) => { complete = resolve; }));
    component = mount(Board, { target: document.body, props: { onMove } });
    const source = document.querySelector('section[aria-label="Todo"]')!;
    const destination = document.querySelector('section[aria-label="Done"]')!;
    drag(destination, "drop");
    expect(onMove).not.toHaveBeenCalled();
    drag(source.querySelector("button")!, "dragstart");
    drag(source, "dragover");
    drag(source, "drop");
    expect(onMove).not.toHaveBeenCalled();
    drag(destination, "dragover");
    drag(destination, "drop");
    drag(destination, "drop");
    expect(onMove).toHaveBeenCalledTimes(1);
    expect(onMove.mock.calls[0]).toEqual([{ id: "Task" }, { id: "Done", rows: [] }, null, "after"]);
    complete();
    await tick();
  });

  it("shows a failed move and restores the drag control for retry", async () => {
    const onMove = vi.fn().mockRejectedValue(new Error("Status update failed"));
    component = mount(Board, { target: document.body, props: { onMove } });
    const handle = document.querySelector<HTMLButtonElement>('button[draggable="true"]')!;
    const destination = document.querySelector('section[aria-label="Done"]')!;
    drag(handle, "dragstart");
    drag(destination, "dragover");
    drag(destination, "drop");
    await tick();
    await tick();
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("Status update failed");
    expect(handle.disabled).toBe(false);
  });
});
