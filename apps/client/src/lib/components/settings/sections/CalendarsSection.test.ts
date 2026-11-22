// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Calendar } from "$lib/calendar/types";
import CalendarsSection from "./CalendarsSection.svelte";

const store = vi.hoisted(() => ({
  list: [] as Calendar[],
  countEvents: vi.fn((calendarId: string): Promise<number> => Promise.resolve(calendarId === "cal-work" ? 3 : 0)),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

vi.mock("$lib/stores/calendars.svelte", () => ({
  getCalendars: () => ({
    get list() {
      return store.list;
    },
    countEvents: store.countEvents,
  }),
}));

vi.mock("$lib/stores/calendar.svelte", () => ({
  getCalendar: () => ({}),
}));

vi.mock("$lib/stores/preferences.svelte", () => ({
  getPreferences: () => ({
    calendarTimeFormat: "24h",
    calendarDimPastEvents: false,
  }),
}));

vi.mock("$lib/stores/calendar-zoom.svelte", () => ({
  getCalendarZoom: () => ({
    zoomPercent: 100,
    isDefault: true,
    reset: () => {},
    setZoomPercent: () => {},
  }),
}));

function calendar(id: string, name: string): Calendar {
  return { id, name, color: "#3366ff", source: "local", visible: true, readOnly: false };
}

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

async function settle(): Promise<void> {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve();
    flushSync();
  }
}

describe("Calendars settings section", () => {
  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
    store.list = [calendar("cal-work", "Work"), calendar("cal-home", "Home")];
    store.countEvents.mockClear();
  });

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    target.remove();
  });

  it("counts each calendar's events once on open instead of recounting in a loop", async () => {
    component = mount(CalendarsSection, { target, props: { fileTransfersAvailable: false } });
    flushSync();
    await settle();

    expect(target.textContent).toContain("3");
    expect(store.countEvents.mock.calls.length).toBe(2);
  });
});
