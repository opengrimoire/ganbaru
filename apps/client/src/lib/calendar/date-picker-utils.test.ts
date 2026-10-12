import { describe, expect, it } from "vitest";
import {
  buildCalendarGrid,
  clampDatePickerActiveDate,
  datePickerMonthNavigationTarget,
} from "./date-picker-utils";

describe("clampDatePickerActiveDate", () => {
  it("keeps an enabled roving date when the selection precedes the minimum", () => {
    expect(clampDatePickerActiveDate("2026-08-10", "2026-08-15")).toBe("2026-08-15");
    expect(clampDatePickerActiveDate("2026-08-20", "2026-08-15")).toBe("2026-08-20");
    expect(clampDatePickerActiveDate("2026-08-10")).toBe("2026-08-10");
  });
});

describe("datePickerMonthNavigationTarget", () => {
  it("uses the minimum day within its month and rejects earlier months", () => {
    expect(datePickerMonthNavigationTarget("2026-09-10", 2026, 8, "2026-08-15"))
      .toBe("2026-08-15");
    expect(datePickerMonthNavigationTarget("2026-09-20", 2026, 8, "2026-08-15"))
      .toBe("2026-08-20");
    expect(datePickerMonthNavigationTarget("2026-08-15", 2026, 7, "2026-08-15"))
      .toBeNull();
  });
});

describe("buildCalendarGrid", () => {
  it("starts weeks on Monday and fills outside-month days", () => {
    const days = buildCalendarGrid(2021, 5, "2021-05-12");

    expect(days).toHaveLength(42);
    expect(days[0]).toMatchObject({
      day: 26,
      dateStr: "2021-04-26",
      currentMonth: false,
    });
    expect(days[5]).toMatchObject({
      day: 1,
      dateStr: "2021-05-01",
      currentMonth: true,
    });
  });

  it("marks only the selected date", () => {
    const days = buildCalendarGrid(2024, 1, "2024-01-15");

    expect(days.filter((day) => day.selected).map((day) => day.dateStr)).toEqual([
      "2024-01-15",
    ]);
  });
});
