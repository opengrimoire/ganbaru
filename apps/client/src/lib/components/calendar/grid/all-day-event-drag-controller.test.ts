// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";

vi.mock("svelte", async (importOriginal) => ({
  ...(await importOriginal<typeof import("svelte")>()),
  onDestroy: vi.fn(),
}));

const { createAllDayEventDragController } = await import("./all-day-event-drag-controller.svelte");

const COLUMN_WIDTH = 100;
const DAYS = [new Date(2026, 10, 2), new Date(2026, 10, 3), new Date(2026, 10, 4), new Date(2026, 10, 5)];

function allDayEvent(id: string, date: string): CalendarEvent {
  return { id, title: id, start: `${date} 00:00`, end: `${date} 00:00`, allDay: true } as CalendarEvent;
}

function columnBounds(): DOMRect[] {
  return DAYS.map((_, index) => DOMRect.fromRect({ x: index * COLUMN_WIDTH, y: 0, width: COLUMN_WIDTH, height: 20 }));
}

/** A pointer press on the middle of a chip, away from its resize edges. */
function press(column: number): PointerEvent {
  const target = document.createElement("div");
  return { clientX: column * COLUMN_WIDTH + 50, clientY: 10, pointerType: "mouse", target } as unknown as PointerEvent;
}

function pointer(type: string, column: number): void {
  window.dispatchEvent(new MouseEvent(type, { clientX: column * COLUMN_WIDTH + 50, clientY: 10 }));
}

describe("all-day event drag controller", () => {
  let events: CalendarEvent[];
  let updates: CalendarEvent[];
  let settleUpdate: () => void;

  beforeEach(() => {
    events = [allDayEvent("a", "2026-11-02"), allDayEvent("b", "2026-11-03")];
    updates = [];
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  function controller(onEventUpdate?: (event: CalendarEvent) => Promise<void>) {
    return createAllDayEventDragController({
      events: () => events,
      days: () => DAYS,
      getColumnBounds: columnBounds,
      getPositionedEvents: () => [],
      onEventUpdate: onEventUpdate ?? ((event) => {
        updates.push(event);
        return new Promise<void>((resolve) => { settleUpdate = resolve; });
      }),
      mobileLayout: () => false,
    });
  }

  it("ignores a press on another event while a released drag is still saving", async () => {
    const drag = controller();
    drag.handleDragStart("a", press(0));
    pointer("pointermove", 2);
    pointer("pointerup", 2);
    expect(updates.map((event) => [event.id, event.start])).toEqual([["a", "2026-11-04 00:00"]]);

    drag.handleDragStart("b", press(1));
    expect(drag.dragState?.eventId).toBe("a");
    pointer("pointerup", 1);
    expect(updates).toHaveLength(1);

    settleUpdate();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(drag.dragState).toBeNull();
    expect(drag.allDayDragPreview).toBeNull();

    drag.handleDragStart("b", press(1));
    pointer("pointermove", 3);
    pointer("pointerup", 3);
    expect(updates.map((event) => [event.id, event.start])).toEqual([
      ["a", "2026-11-04 00:00"],
      ["b", "2026-11-05 00:00"],
    ]);
  });
});
