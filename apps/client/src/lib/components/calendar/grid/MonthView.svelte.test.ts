// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import { lightTheme } from "$lib/themes";
import MonthView from "./MonthView.svelte";

const DAY = "2026-11-11";

function timedEvent(id: string, title: string, hour: number): CalendarEvent {
  const start = `${DAY} ${String(hour).padStart(2, "0")}:00`;
  const end = `${DAY} ${String(hour + 1).padStart(2, "0")}:00`;
  return { id, title, start, end, allDay: false } as CalendarEvent;
}

/** Reports a fixed size for every observed element so the month grid lays out chips and a more button. */
class FixedSizeResizeObserver {
  constructor(private readonly callback: ResizeObserverCallback) {}

  observe(target: Element): void {
    const entry = {
      target,
      contentRect: DOMRect.fromRect({ width: 60, height: 420 }),
      contentBoxSize: [{ inlineSize: 60, blockSize: 420 }],
    } as unknown as ResizeObserverEntry;
    queueMicrotask(() => this.callback([entry], this as unknown as ResizeObserver));
  }

  unobserve(): void {}

  disconnect(): void {}
}

describe("MonthView more events dialog", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", FixedSizeResizeObserver);
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    component = undefined;
    target.remove();
    vi.unstubAllGlobals();
  });

  it("drops an event deleted while the dialog is open", async () => {
    const props = $state({
      eventsByDay: new Map<string, CalendarEvent[]>([[DAY, [
        timedEvent("a", "Alpha", 8),
        timedEvent("b", "Bravo", 9),
        timedEvent("c", "Charlie", 10),
        timedEvent("d", "Delta", 11),
      ]]]),
    });
    component = mount(MonthView, {
      target,
      props: {
        anchorDate: new Date(2026, 10, 11),
        get eventsByDay() { return props.eventsByDay; },
        theme: lightTheme,
        onDayClick: vi.fn(),
        onEventClick: vi.fn(),
      },
    });
    flushSync();
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();

    const more = target.querySelector<HTMLButtonElement>(".month-more-surface");
    expect(more).not.toBeNull();
    more!.click();
    flushSync();
    const listed = () => [...document.body.querySelectorAll("[role='dialog'] [data-event-id]")]
      .map((element) => element.getAttribute("data-event-id"));
    expect(listed()).toEqual(["a", "b", "c", "d"]);

    props.eventsByDay = new Map([[DAY, [timedEvent("a", "Alpha", 8), timedEvent("c", "Charlie", 10), timedEvent("d", "Delta", 11)]]]);
    flushSync();
    expect(listed()).toEqual(["a", "c", "d"]);
  });
});
