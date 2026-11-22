import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TabDepartures } from "./tab-departures.svelte";

const TIMEOUT_MS = 1_000;

interface Released {
  id: string;
  origin: string;
}

function createDepartures(): { departures: TabDepartures<string>; released: Released[] } {
  const released: Released[] = [];
  const departures = new TabDepartures<string>({
    timeoutMs: TIMEOUT_MS,
    release: (id, origin, remove) => {
      remove();
      released.push({ id, origin });
    },
  });
  return { departures, released };
}

/** A move the test settles by hand. */
function deferredMove(): { promise: Promise<void>; reject: (error: Error) => void } {
  let reject: (error: Error) => void = () => {};
  const promise = new Promise<void>((_resolve, rejectMove) => {
    reject = rejectMove;
  });
  return { promise, reject };
}

async function flushMicrotasks(): Promise<void> {
  for (let round = 0; round < 3; round += 1) await Promise.resolve();
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("TabDepartures", () => {
  it("returns the first of two torn-off tabs to its own origin when its move fails", async () => {
    const { departures, released } = createDepartures();
    const first = deferredMove();
    const second = deferredMove();
    departures.hold("a", first.promise, "origin a");
    departures.hold("b", second.promise, "origin b");

    first.reject(new Error("window failed"));
    await flushMicrotasks();

    expect(released).toEqual([{ id: "a", origin: "origin a" }]);
    expect(departures.has("a")).toBe(false);
    expect(departures.has("b")).toBe(true);

    vi.advanceTimersByTime(TIMEOUT_MS);
    expect(released).toEqual([{ id: "a", origin: "origin a" }, { id: "b", origin: "origin b" }]);
  });

  it("brings back every tab that has not arrived in time, each with its origin", () => {
    const { departures, released } = createDepartures();
    departures.hold("a", null, "origin a");
    vi.advanceTimersByTime(TIMEOUT_MS / 2);
    departures.hold("b", null, "origin b");

    vi.advanceTimersByTime(TIMEOUT_MS / 2);
    expect(released).toEqual([{ id: "a", origin: "origin a" }]);
    vi.advanceTimersByTime(TIMEOUT_MS / 2);
    expect(released).toEqual([{ id: "a", origin: "origin a" }, { id: "b", origin: "origin b" }]);
  });

  it("ignores a late failure from an earlier hold of the same tab", async () => {
    const { departures, released } = createDepartures();
    const stale = deferredMove();
    departures.hold("a", stale.promise, "first origin");
    departures.hold("a", null, "second origin");

    stale.reject(new Error("window failed"));
    await flushMicrotasks();
    expect(released).toEqual([]);
    expect(departures.has("a")).toBe(true);

    vi.advanceTimersByTime(TIMEOUT_MS);
    expect(released).toEqual([{ id: "a", origin: "second origin" }]);
  });

  it("forgets tabs that reached their new window without bringing them back", () => {
    const { departures, released } = createDepartures();
    departures.hold("a", null, "origin a");
    departures.hold("b", null, "origin b");

    departures.forgetGone((id) => id === "b");
    expect(departures.ids).toEqual(["b"]);

    vi.advanceTimersByTime(TIMEOUT_MS);
    expect(released).toEqual([{ id: "b", origin: "origin b" }]);
  });

  it("releases nothing once disposed", () => {
    const { departures, released } = createDepartures();
    departures.hold("a", null, "origin a");
    departures.dispose();

    vi.advanceTimersByTime(TIMEOUT_MS);
    expect(released).toEqual([]);
  });
});
