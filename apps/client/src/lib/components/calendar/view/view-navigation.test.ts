import { describe, expect, it } from "vitest";
import {
  DEFAULT_DAY_HEADER_RETURN_MODE,
  nextDayHeaderReturnMode,
  type DayHeaderReturnMode,
} from "./view-navigation";

describe("day header return mode", () => {
  it("defaults to week view", () => {
    expect(DEFAULT_DAY_HEADER_RETURN_MODE).toBe("week");
  });

  it("remembers the committed non-day view", () => {
    let mode: DayHeaderReturnMode = DEFAULT_DAY_HEADER_RETURN_MODE;

    mode = nextDayHeaderReturnMode(mode, "workweek");
    expect(mode).toBe("workweek");

    mode = nextDayHeaderReturnMode(mode, "week");
    expect(mode).toBe("week");

    mode = nextDayHeaderReturnMode(mode, "month");
    expect(mode).toBe("month");
  });

  it("keeps the previous return mode when day view commits", () => {
    expect(nextDayHeaderReturnMode("week", "day")).toBe("week");
    expect(nextDayHeaderReturnMode("workweek", "day")).toBe("workweek");
    expect(nextDayHeaderReturnMode("month", "day")).toBe("month");
  });
});
