import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import {
  layoutMonthDayEvents,
  MONTH_EVENT_CHIP_HEIGHT_PX,
  MONTH_EVENT_ROW_GAP_PX,
  type MonthDayLayoutOptions,
  type MonthMoreLabelFormatter,
} from "./month-event-layout";
import { calendar as enCalendar } from "$lib/i18n/messages/en/calendar";
import { calendar as esCalendar } from "$lib/i18n/messages/es/calendar";

function catalogMoreLabel(catalog: typeof enCalendar | typeof esCalendar): MonthMoreLabelFormatter {
  return (hiddenCount, variant) => variant === "full"
    ? catalog.moreEvents(String(hiddenCount))
    : catalog.moreEventsCompact(String(hiddenCount));
}

/** Layout options whose defaults come from the English catalog. */
type CatalogLayoutOption = "formatMoreLabel" | "untitledLabel";

function layoutDay(
  events: readonly CalendarEvent[],
  options: Omit<MonthDayLayoutOptions, CatalogLayoutOption> & Partial<Pick<MonthDayLayoutOptions, CatalogLayoutOption>>,
) {
  return layoutMonthDayEvents(events, {
    formatMoreLabel: catalogMoreLabel(enCalendar),
    untitledLabel: enCalendar.event.noTitle,
    ...options,
  });
}

function evt(id: string, title: string, overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id,
    title,
    start: "2026-03-13 09:00",
    end: "2026-03-13 10:00",
    timezone: "UTC",
    calendarId: "local",
    ...overrides,
  };
}

function eventItemByTitle(layoutItems: ReturnType<typeof layoutMonthDayEvents>["items"], title: string) {
  return layoutItems.find((item) => item.kind === "event" && item.event.title === title);
}

describe("layoutMonthDayEvents", () => {
  it("packs short events into the same row when they fit", () => {
    const layout = layoutDay([
      evt("1", "Sleep"),
      evt("2", "Breakfast"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    expect(layout.hiddenCount).toBe(0);
    expect(layout.items).toHaveLength(2);
    expect(layout.items.map((item) => item.row)).toEqual([0, 0]);
    expect(layout.items[1].leftPx).toBeGreaterThan(layout.items[0].leftPx);
  });

  it("expands the last event in each row to the row edge", () => {
    const layout = layoutDay([
      evt("1", "Sleep"),
      evt("2", "Breakfast"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    const last = layout.items[1];
    expect(last.kind).toBe("event");
    expect(last.leftPx + last.widthPx).toBe(100);
  });

  it("uses spare row width on earlier readable labels before expanding the last event", () => {
    const layout = layoutDay([
      evt("1", "Dormir"),
      evt("2", "Desayuno"),
      evt("3", "Ejercicio"),
    ], {
      cellWidthPx: 220,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    const items = layout.items.filter((item) => item.kind === "event");
    const desayuno = items.find((item) => item.event.title === "Desayuno");
    const ejercicio = items.find((item) => item.event.title === "Ejercicio");

    expect(items).toHaveLength(3);
    expect(desayuno?.kind).toBe("event");
    expect(ejercicio?.kind).toBe("event");
    expect(desayuno?.widthPx).toBeGreaterThan(56);
    if (ejercicio?.kind === "event") {
      expect(ejercicio.leftPx + ejercicio.widthPx).toBe(220);
    }
  });

  it("uses full-width rows when every event fits vertically", () => {
    const layout = layoutDay([
      evt("1", "A"),
      evt("2", "B"),
      evt("3", "C"),
    ], {
      cellWidthPx: 140,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX * 3 + MONTH_EVENT_ROW_GAP_PX * 2,
    });

    expect(layout.hiddenCount).toBe(0);
    expect(layout.rowCount).toBe(3);
    expect(layout.items.map((item) => item.row)).toEqual([0, 1, 2]);
    expect(layout.items.every((item) => item.leftPx === 0)).toBe(true);
    expect(layout.items.every((item) => item.widthPx === 140)).toBe(true);
  });

  it("reserves month chip width for meeting detail icons", () => {
    const plain = layoutDay([
      evt("1", "Meet"),
      evt("2", "Next"),
    ], {
      cellWidthPx: 130,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });
    const withMeetingIcons = layoutDay([
      evt("1", "Meet", { hasCallLink: true, location: "Room A" }),
      evt("2", "Next"),
    ], {
      cellWidthPx: 130,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    const plainMeet = eventItemByTitle(plain.items, "Meet");
    const meetingMeet = eventItemByTitle(withMeetingIcons.items, "Meet");

    expect(plainMeet?.kind).toBe("event");
    expect(meetingMeet?.kind).toBe("event");
    if (plainMeet?.kind === "event" && meetingMeet?.kind === "event") {
      expect(meetingMeet.widthPx).toBeGreaterThan(plainMeet.widthPx + 18);
    }
  });

  it("does not reserve month chip width for repeat alone", () => {
    const plain = layoutDay([
      evt("1", "Repeat"),
      evt("2", "Next"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });
    const recurring = layoutDay([
      evt("1", "Repeat", {
        recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
      }),
      evt("2", "Next"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    const plainRepeat = eventItemByTitle(plain.items, "Repeat");
    const recurringRepeat = eventItemByTitle(recurring.items, "Repeat");

    expect(plainRepeat?.kind).toBe("event");
    expect(recurringRepeat?.kind).toBe("event");
    if (plainRepeat?.kind === "event" && recurringRepeat?.kind === "event") {
      expect(recurringRepeat.widthPx).toBe(plainRepeat.widthPx);
    }
  });

  it("moves a long event to the next row when the current row is full", () => {
    const layout = layoutDay([
      evt("1", "Sleep"),
      evt("2", "Breakfast"),
      evt("3", "Deep planning review"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX * 2 + MONTH_EVENT_ROW_GAP_PX,
    });

    expect(layout.hiddenCount).toBe(0);
    expect(layout.items.map((item) => item.row)).toEqual([0, 0, 1]);
    expect(layout.items[2].topPx).toBe(MONTH_EVENT_CHIP_HEIGHT_PX + MONTH_EVENT_ROW_GAP_PX);
  });

  it("uses the half-row cap for packing before expanding the last event", () => {
    const layout = layoutDay([
      evt("1", "This is a very long event title that cannot fit naturally"),
      evt("2", "Another very long event title that still shares a row"),
    ], {
      cellWidthPx: 200,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    expect(layout.items).toHaveLength(2);
    expect(layout.items.map((item) => item.row)).toEqual([0, 0]);
    expect(layout.items[0].widthPx).toBeLessThanOrEqual(99);
  });

  it("uses available height to limit rows without shrinking chips", () => {
    const layout = layoutDay([
      evt("1", "Sleep"),
      evt("2", "Breakfast"),
      evt("3", "Meditate"),
      evt("4", "Review"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    expect(layout.rowCount).toBe(1);
    expect(layout.items.every((item) => item.heightPx === MONTH_EVENT_CHIP_HEIGHT_PX)).toBe(true);
    expect(layout.items.every((item) => item.topPx === 0)).toBe(true);
    expect(layout.hiddenCount).toBeGreaterThan(0);
  });

  it("adds a more chip when events overflow", () => {
    const layout = layoutDay([
      evt("1", "Sleep"),
      evt("2", "Breakfast"),
      evt("3", "Meditate"),
      evt("4", "Review"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    const more = layout.items.find((item) => item.kind === "more");
    expect(more).toBeDefined();
    expect(more?.hiddenCount).toBe(3);
    expect(more?.label).toBe("+3 more");
  });

  it("uses the compact more label when the full label cannot fit", () => {
    const layout = layoutDay([
      evt("1", "A"),
      evt("2", "B"),
      evt("3", "C"),
    ], {
      cellWidthPx: 34,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    const more = layout.items.find((item) => item.kind === "more");
    expect(more?.kind).toBe("more");
    expect(more?.hiddenCount).toBe(3);
    expect(more?.label).toBe("+3");
  });

  it("renders the localized more label from the catalog", () => {
    const layout = layoutDay([
      evt("1", "Sleep"),
      evt("2", "Breakfast"),
      evt("3", "Meditate"),
      evt("4", "Review"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
      formatMoreLabel: catalogMoreLabel(esCalendar),
    });

    const more = layout.items.find((item) => item.kind === "more");
    expect(more?.label).toBe("+3 más");
  });

  it("measures the formatted label when choosing between full and compact variants", () => {
    const events = [evt("1", "Sleep"), evt("2", "Breakfast"), evt("3", "Meditate"), evt("4", "Review")];
    const options = { cellWidthPx: 100, availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX };
    const longLabel: MonthMoreLabelFormatter = (count, variant) =>
      variant === "full" ? `+${count} more events are hidden here` : `+${count}`;

    const short = layoutDay(events, options).items.find((item) => item.kind === "more");
    const long = layoutDay(events, { ...options, formatMoreLabel: longLabel })
      .items.find((item) => item.kind === "more");

    expect(short?.kind === "more" && short.label).toBe("+3 more");
    expect(long?.kind === "more" && long.label).toMatch(/^\+\d+$/u);
  });

  it("measures untitled events with the localized fallback label", () => {
    const options = { cellWidthPx: 200, availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX };
    const untitled = layoutDay([evt("1", "  "), evt("2", "Next")], {
      ...options,
      untitledLabel: esCalendar.event.noTitle,
    });
    const titled = layoutDay([evt("1", esCalendar.event.noTitle), evt("2", "Next")], options);
    const shortFallback = layoutDay([evt("1", "  "), evt("2", "Next")], { ...options, untitledLabel: "-" });

    expect(untitled.items[0].widthPx).toBe(titled.items[0].widthPx);
    expect(untitled.items[0].widthPx).toBeGreaterThan(shortFallback.items[0].widthPx);
  });

  it("keeps the more chip compact while expanding the previous event", () => {
    const layout = layoutDay([
      evt("1", "A"),
      evt("2", "B"),
      evt("3", "C"),
      evt("4", "D"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    const eventItem = layout.items.find((item) => item.kind === "event");
    const more = layout.items.find((item) => item.kind === "more");
    expect(eventItem?.kind).toBe("event");
    expect(more?.kind).toBe("more");
    expect(more?.widthPx).toBeLessThan(60);
    if (eventItem?.kind === "event" && more?.kind === "more") {
      expect(more.leftPx + more.widthPx).toBe(100);
      expect(eventItem.leftPx + eventItem.widthPx).toBe(more.leftPx - 2);
    }
  });

  it("keeps readable events before the more chip when the row has enough space", () => {
    const layout = layoutDay([
      evt("1", "Compras"),
      evt("2", "Comer"),
      evt("3", "Cenar"),
      evt("4", "Poner a lavar la ropa"),
    ], {
      cellWidthPx: 190,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    const eventTitles = layout.items
      .filter((item) => item.kind === "event")
      .map((item) => item.event.title);
    const compras = eventItemByTitle(layout.items, "Compras");
    const comer = eventItemByTitle(layout.items, "Comer");
    const more = layout.items.find((item) => item.kind === "more");

    expect(eventTitles).toEqual(["Compras", "Comer", "Cenar"]);
    expect(compras?.widthPx).toBeGreaterThan(52);
    expect(comer?.widthPx).toBeGreaterThan(40);
    expect(more?.kind).toBe("more");
    expect(more?.hiddenCount).toBe(1);
  });

  it("hides another event before leaving earlier labels compressed beside the more chip", () => {
    const layout = layoutDay([
      evt("1", "Compras"),
      evt("2", "Comer"),
      evt("3", "Cenar"),
      evt("4", "Poner a lavar la ropa"),
    ], {
      cellWidthPx: 155,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    const eventTitles = layout.items
      .filter((item) => item.kind === "event")
      .map((item) => item.event.title);
    const more = layout.items.find((item) => item.kind === "more");

    expect(eventTitles).toEqual(["Compras", "Comer"]);
    expect(more?.kind).toBe("more");
    expect(more?.hiddenCount).toBe(2);
  });

  it("replaces visible chips until the more chip fits", () => {
    const layout = layoutDay([
      evt("1", "A"),
      evt("2", "B"),
      evt("3", "C"),
    ], {
      cellWidthPx: 60,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    expect(layout.items).toHaveLength(1);
    expect(layout.items[0].kind).toBe("more");
    expect(layout.hiddenCount).toBe(3);
  });

  it("returns no visible items when no legible row fits", () => {
    const layout = layoutDay([
      evt("1", "Sleep"),
    ], {
      cellWidthPx: 100,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX - 1,
    });

    expect(layout.rowCount).toBe(0);
    expect(layout.items).toEqual([]);
    expect(layout.hiddenCount).toBe(1);
  });

  it("clamps chip width inside very narrow cells", () => {
    const layout = layoutDay([
      evt("1", "Sleep"),
    ], {
      cellWidthPx: 20,
      availableHeightPx: MONTH_EVENT_CHIP_HEIGHT_PX,
    });

    expect(layout.items).toHaveLength(1);
    expect(layout.items[0].widthPx).toBeLessThanOrEqual(20);
  });
});
