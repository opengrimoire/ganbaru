// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { COLLECTION_SAVE_FEEDBACK_DELAY_MS } from "./CollectionSaveIndicator.svelte";
import SaveIndicator from "./CollectionSaveIndicatorHarness.test.svelte";

let component: ReturnType<typeof mount> | undefined;
beforeEach(() => { vi.useFakeTimers(); });
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  vi.useRealTimers();
  document.body.replaceChildren();
});

/** Drive saves through reactive props as the database header does. */
async function setPending(pending: boolean): Promise<void> {
  document.querySelectorAll<HTMLButtonElement>("button")[pending ? 0 : 1].click();
  await tick();
}

describe("delayed collection save feedback", () => {
  it("keeps fast saves invisible and starts a fresh delay for the next save", async () => {
    component = mount(SaveIndicator, { target: document.body });
    await setPending(true);
    await vi.advanceTimersByTimeAsync(COLLECTION_SAVE_FEEDBACK_DELAY_MS - 1);
    expect(document.querySelector('[role="status"]')).toBeNull();
    await setPending(false);
    await vi.advanceTimersByTimeAsync(COLLECTION_SAVE_FEEDBACK_DELAY_MS);
    expect(document.querySelector("svg")).toBeNull();
    await setPending(true);
    await vi.advanceTimersByTimeAsync(COLLECTION_SAVE_FEEDBACK_DELAY_MS - 1);
    expect(document.querySelector("svg")).toBeNull();
    await vi.advanceTimersByTimeAsync(1);
    await tick();
    expect(document.querySelector('[role="status"]')?.getAttribute("aria-label")).toBe("Saving database");
    expect(document.querySelector("svg")).not.toBeNull();
    await setPending(false);
    expect(document.querySelector('[role="status"]')).toBeNull();
  });

  it("cancels the feedback timer when its database is removed", async () => {
    component = mount(SaveIndicator, { target: document.body });
    await setPending(true);
    const timerCount = vi.getTimerCount();
    await unmount(component);
    component = undefined;
    expect(vi.getTimerCount()).toBeLessThan(timerCount);
    await vi.advanceTimersByTimeAsync(COLLECTION_SAVE_FEEDBACK_DELAY_MS);
    expect(document.querySelector('[role="status"]')).toBeNull();
  });
});
