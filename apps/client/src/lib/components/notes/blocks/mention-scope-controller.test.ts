import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesMentionScope } from "$lib/notes/types";
import { createNotesMentionScopeController } from "./mention-scope-controller.svelte";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((next, fail) => { resolve = next; reject = fail; });
  return { promise, resolve, reject };
}

function scope(taskId: string): NotesMentionScope {
  return { tasks: [{ id: taskId, project_id: "project", title: taskId, completed: false }], events: [] };
}

async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("Notes mention scope controller", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("shares a request already running for the page and serves its scope only to that page", async () => {
    const result = deferred<NotesMentionScope>();
    const load = vi.fn(() => result.promise);
    const controller = createNotesMentionScopeController(load);
    controller.request("page-a");
    controller.request("page-a");
    expect(load).toHaveBeenCalledOnce();
    result.resolve(scope("task-a"));
    await settle();
    expect(controller.read("page-a")).toEqual(scope("task-a"));
    expect(controller.read("page-b")).toEqual({ tasks: [], events: [] });
  });

  it("keeps the shown scope while the same page refreshes", async () => {
    const results = [scope("old"), scope("new")];
    const load = vi.fn(async () => results.shift() ?? scope("none"));
    const controller = createNotesMentionScopeController(load);
    controller.request("page-a");
    await settle();
    controller.request("page-a");
    expect(controller.read("page-a")).toEqual(scope("old"));
    await settle();
    expect(controller.read("page-a")).toEqual(scope("new"));
  });

  it("clears the scope on a page switch and ignores the previous page's late result", async () => {
    const first = deferred<NotesMentionScope>();
    const second = deferred<NotesMentionScope>();
    const load = vi.fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const controller = createNotesMentionScopeController(load);
    controller.request("page-a");
    controller.request("page-b");
    first.resolve(scope("task-a"));
    await settle();
    expect(controller.read("page-b")).toEqual({ tasks: [], events: [] });
    second.resolve(scope("task-b"));
    await settle();
    expect(controller.read("page-b")).toEqual(scope("task-b"));
    expect(controller.read("page-a")).toEqual({ tasks: [], events: [] });
  });

  it("leaves the scope empty when loading fails and drops results after a reset", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const failing = deferred<NotesMentionScope>();
    const stale = deferred<NotesMentionScope>();
    const fresh = deferred<NotesMentionScope>();
    const load = vi.fn()
      .mockReturnValueOnce(failing.promise)
      .mockReturnValueOnce(stale.promise)
      .mockReturnValueOnce(fresh.promise);
    const controller = createNotesMentionScopeController(load);
    controller.request("page-a");
    failing.reject(new Error("offline"));
    await settle();
    expect(controller.read("page-a")).toEqual({ tasks: [], events: [] });
    controller.request("page-a");
    expect(load).toHaveBeenCalledTimes(2);
    controller.reset();
    controller.request("page-a");
    expect(load).toHaveBeenCalledTimes(3);
    stale.resolve(scope("stale"));
    await settle();
    expect(controller.read("page-a")).toEqual({ tasks: [], events: [] });
    fresh.resolve(scope("fresh"));
    await settle();
    expect(controller.read("page-a")).toEqual(scope("fresh"));
  });
});
