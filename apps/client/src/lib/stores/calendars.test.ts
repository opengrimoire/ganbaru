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

  it("counts each of two quick toggles and writes them in click order", async () => {
    const store = getCalendars();
    invokeMock.mockResolvedValue(undefined);
    await store.add({ id: "cal-work", name: "Work", color: "", source: "ics", createdAt: "", updatedAt: "" });

    const replies: (() => void)[] = [];
    invokeMock.mockImplementation(() => new Promise((resolve) => {
      replies.push(() => resolve(undefined));
    }));
    const first = store.toggleVisibility("cal-work");
    expect(store.list.find((calendar) => calendar.id === "cal-work")?.visible).toBe(false);
    const second = store.toggleVisibility("cal-work");
    expect(store.list.find((calendar) => calendar.id === "cal-work")?.visible).toBe(true);
    await vi.waitFor(() => expect(replies).toHaveLength(1));
    replies[0]?.();
    await vi.waitFor(() => expect(replies).toHaveLength(2));
    replies[1]?.();
    await Promise.all([first, second]);

    const written = invokeMock.mock.calls
      .filter(([command]) => command === "calendar_set_visibility")
      .map(([, args]) => (args as { visible: boolean }).visible);
    expect(written).toEqual([false, true]);
    expect(store.list.find((calendar) => calendar.id === "cal-work")?.visible).toBe(true);
  });

  it("restores the stored visibility when the latest toggle fails to save", async () => {
    const store = getCalendars();
    invokeMock.mockResolvedValue(undefined);
    await store.add({ id: "cal-home", name: "Home", color: "", source: "ics", createdAt: "", updatedAt: "" });
    invokeMock.mockRejectedValue(new Error("Database locked"));

    await expect(store.toggleVisibility("cal-home")).rejects.toThrow("Database locked");

    expect(store.list.find((calendar) => calendar.id === "cal-home")?.visible).toBe(true);
  });
});
