import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import { getCalendars } from "./calendars.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

vi.mock("$lib/api/db", () => ({
  dbUrl: () => "sqlite:test",
  ensureDbUrl: () => Promise.resolve("sqlite:test"),
}));

const invokeMock = vi.mocked(invoke);

describe("calendars store", () => {
  afterEach(() => {
    invokeMock.mockReset();
  });

  it("shows the stored visibility when two toggles overlap", async () => {
    const store = getCalendars();
    invokeMock.mockResolvedValue(undefined);
    await store.add({ id: "cal-work", name: "Work", color: "", source: "ics", createdAt: "", updatedAt: "" });

    const replies: (() => void)[] = [];
    invokeMock.mockImplementation(() => new Promise((resolve) => {
      replies.push(() => resolve(undefined));
    }));
    const first = store.toggleVisibility("cal-work");
    const second = store.toggleVisibility("cal-work");
    for (const reply of replies) reply();
    await Promise.all([first, second]);

    const written = invokeMock.mock.calls
      .filter(([command]) => command === "calendar_set_visibility")
      .map(([, args]) => (args as { visible: boolean }).visible);
    expect(written).toEqual([false, false]);
    expect(store.list.find((calendar) => calendar.id === "cal-work")?.visible).toBe(false);
  });
});
