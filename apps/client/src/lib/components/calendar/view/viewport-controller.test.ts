import { describe, expect, it } from "vitest";
import { CalendarViewViewportController } from "./viewport-controller.svelte";

describe("CalendarViewViewportController", () => {
  it("preserves the local timezone while reordering removable rows", () => {
    const controller = new CalendarViewViewportController(() => "America/Monterrey");
    controller.addTimezone("UTC");
    controller.addTimezone("Asia/Tokyo");
    controller.removeTimezone(0);
    expect(controller.timezones[0]).toBe("America/Monterrey");
    controller.reorderTimezone(2, 1);
    expect(controller.timezones).toEqual(["America/Monterrey", "Asia/Tokyo", "UTC"]);
  });

  it("deduplicates rows and enforces the five-timezone cap", () => {
    const controller = new CalendarViewViewportController(() => "UTC");
    for (const timezone of ["UTC", "A", "B", "C", "D", "E"]) controller.addTimezone(timezone);
    expect(controller.timezones).toEqual(["UTC", "A", "B", "C", "D"]);
  });
});
