import { beforeEach, describe, expect, it, vi } from "vitest";

type AppliedListener = (tables: string[]) => void;
type WindowListener = (event: { payload: unknown }) => void;

const transport = vi.hoisted(() => ({
  applied: [] as AppliedListener[],
  windows: [] as WindowListener[],
}));

vi.mock("$lib/api/sync", () => ({
  listenSyncApplied: (listener: AppliedListener) => {
    transport.applied.push(listener);
    return Promise.resolve(() => undefined);
  },
}));

vi.mock("$lib/windows/sync-transport", () => ({
  emitWindowSync: () => Promise.resolve(),
  listenWindowSync: (_event: string, listener: WindowListener) => {
    transport.windows.push(listener);
    return Promise.resolve(() => undefined);
  },
}));

async function startedWindowSync(reload = vi.fn(() => Promise.resolve())) {
  const { initCalendarWindowSync } = await import("./window-sync");
  initCalendarWindowSync(reload);
  return reload;
}

describe("calendar window sync", () => {
  beforeEach(() => {
    vi.resetModules();
    transport.applied.length = 0;
    transport.windows.length = 0;
  });

  it("reloads once for each applied notice that names Calendar rows", async () => {
    const reload = await startedWindowSync();

    transport.applied[0]?.(["calendar_events", "icalendar_components"]);

    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("reloads for applied Focus history, which Calendar views render", async () => {
    const reload = await startedWindowSync();

    transport.applied[0]?.(["focus"]);

    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("ignores applied notices for other domains", async () => {
    const reload = await startedWindowSync();

    transport.applied[0]?.(["quick_notes", "vault_preferences"]);

    expect(reload).not.toHaveBeenCalled();
  });

  it("subscribes once however many stores ask", async () => {
    await startedWindowSync();
    await startedWindowSync();

    expect(transport.applied).toHaveLength(1);
    expect(transport.windows).toHaveLength(1);
  });

  it("logs a failed reload instead of leaving the rejection unhandled", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    await startedWindowSync(vi.fn(() => Promise.reject(new Error("read failed"))));

    transport.applied[0]?.(["calendars"]);
    await Promise.resolve();

    expect(warn).toHaveBeenCalledWith("calendar window sync reload failed", expect.any(Error));
    warn.mockRestore();
  });
});
