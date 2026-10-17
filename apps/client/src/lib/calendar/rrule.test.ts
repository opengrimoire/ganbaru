import { describe, it, expect } from "vitest";
import {
  recurrenceToRrule,
  rruleToRecurrence,
  formatRecurrenceLabel,
  recurrenceConfigsEqual,
} from "./rrule";
import type { RecurrenceConfig } from "./types";

describe("recurrenceToRrule", () => {
  it("serializes simple daily", () => {
    const config: RecurrenceConfig = { frequency: "daily", interval: 1, end: { type: "never" } };
    expect(recurrenceToRrule(config)).toBe("FREQ=DAILY");
  });

  it("serializes interval", () => {
    const config: RecurrenceConfig = { frequency: "daily", interval: 3, end: { type: "never" } };
    expect(recurrenceToRrule(config)).toBe("FREQ=DAILY;INTERVAL=3");
  });

  it("serializes weekdays", () => {
    const config: RecurrenceConfig = {
      frequency: "weekly",
      interval: 1,
      weekdays: ["MO", "WE", "FR"],
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=WEEKLY;BYDAY=MO,WE,FR");
  });

  it("serializes count end", () => {
    const config: RecurrenceConfig = {
      frequency: "weekly",
      interval: 2,
      end: { type: "count", count: 10 },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=WEEKLY;INTERVAL=2;COUNT=10");
  });

  it("serializes until end", () => {
    const config: RecurrenceConfig = {
      frequency: "monthly",
      interval: 1,
      end: { type: "until", date: "2026-06-15" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=MONTHLY;UNTIL=20260615");
  });

  it("serializes ordinal weekdays", () => {
    const config: RecurrenceConfig = {
      frequency: "monthly",
      interval: 1,
      ordinalWeekdays: [{ day: "TU", ordinal: 2 }],
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=MONTHLY;BYDAY=2TU");
  });

  it("serializes negative ordinal weekday", () => {
    const config: RecurrenceConfig = {
      frequency: "monthly",
      interval: 1,
      ordinalWeekdays: [{ day: "FR", ordinal: -1 }],
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=MONTHLY;BYDAY=-1FR");
  });

  it("serializes ordinalWeekdays over simple weekdays", () => {
    const config: RecurrenceConfig = {
      frequency: "monthly",
      interval: 1,
      weekdays: ["MO"],
      ordinalWeekdays: [{ day: "TU", ordinal: 3 }],
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=MONTHLY;BYDAY=3TU");
  });

  it("serializes BYMONTHDAY", () => {
    const config: RecurrenceConfig = {
      frequency: "monthly",
      interval: 1,
      byMonthDay: [15],
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=MONTHLY;BYMONTHDAY=15");
  });

  it("serializes BYMONTH", () => {
    const config: RecurrenceConfig = {
      frequency: "yearly",
      interval: 1,
      byMonth: [3, 9],
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=YEARLY;BYMONTH=3,9");
  });

  it("serializes BYSETPOS", () => {
    const config: RecurrenceConfig = {
      frequency: "monthly",
      interval: 1,
      weekdays: ["MO", "TU", "WE", "TH", "FR"],
      bySetPos: [-1],
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe(
      "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
    );
  });

  it("serializes BYYEARDAY", () => {
    const config: RecurrenceConfig = {
      frequency: "yearly",
      interval: 1,
      byYearDay: [1, 100],
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=YEARLY;BYYEARDAY=1,100");
  });

  it("serializes BYWEEKNO", () => {
    const config: RecurrenceConfig = {
      frequency: "yearly",
      interval: 1,
      byWeekNo: [20],
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=YEARLY;BYWEEKNO=20");
  });

  it("serializes WKST when not MO", () => {
    const config: RecurrenceConfig = {
      frequency: "weekly",
      interval: 1,
      wkst: "SU",
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=WEEKLY;WKST=SU");
  });

  it("omits WKST when MO (default)", () => {
    const config: RecurrenceConfig = {
      frequency: "weekly",
      interval: 1,
      wkst: "MO",
      end: { type: "never" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=WEEKLY");
  });

  it("serializes UNTIL with datetime precision", () => {
    const config: RecurrenceConfig = {
      frequency: "daily",
      interval: 1,
      end: { type: "until", date: "2026-06-15T23:59:59Z" },
    };
    expect(recurrenceToRrule(config)).toBe("FREQ=DAILY;UNTIL=20260615T235959Z");
  });
});

describe("rruleToRecurrence", () => {
  it("parses simple daily", () => {
    const config = rruleToRecurrence("FREQ=DAILY");
    expect(config.frequency).toBe("daily");
    expect(config.interval).toBe(1);
    expect(config.weekdays).toBeUndefined();
    expect(config.end).toEqual({ type: "never" });
  });

  it("parses interval", () => {
    const config = rruleToRecurrence("FREQ=DAILY;INTERVAL=3");
    expect(config.interval).toBe(3);
  });

  it("parses BYDAY", () => {
    const config = rruleToRecurrence("FREQ=WEEKLY;BYDAY=MO,WE,FR");
    expect(config.weekdays).toEqual(["MO", "WE", "FR"]);
  });

  it("parses COUNT", () => {
    const config = rruleToRecurrence("FREQ=WEEKLY;COUNT=10");
    expect(config.end).toEqual({ type: "count", count: 10 });
  });

  it("parses UNTIL in YYYYMMDD format", () => {
    const config = rruleToRecurrence("FREQ=MONTHLY;UNTIL=20260615");
    expect(config.end).toEqual({ type: "until", date: "2026-06-15" });
  });

  it("falls back to repeatUntil column", () => {
    const config = rruleToRecurrence("FREQ=DAILY", "2026-04-01");
    expect(config.end).toEqual({ type: "until", date: "2026-04-01" });
  });

  it("prefers UNTIL over repeatUntil column", () => {
    const config = rruleToRecurrence("FREQ=DAILY;UNTIL=20260615", "2026-04-01");
    expect(config.end).toEqual({ type: "until", date: "2026-06-15" });
  });

  it("parses weekday BYDAY lists", () => {
    const config = rruleToRecurrence("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR");
    expect(config.frequency).toBe("weekly");
    expect(config.weekdays).toEqual(["MO", "TU", "WE", "TH", "FR"]);
    expect(config.interval).toBe(1);
  });

  it("parses ordinal BYDAY", () => {
    const config = rruleToRecurrence("FREQ=MONTHLY;BYDAY=2TU");
    expect(config.ordinalWeekdays).toEqual([{ day: "TU", ordinal: 2 }]);
    expect(config.weekdays).toBeUndefined();
  });

  it("parses negative ordinal BYDAY", () => {
    const config = rruleToRecurrence("FREQ=MONTHLY;BYDAY=-1FR");
    expect(config.ordinalWeekdays).toEqual([{ day: "FR", ordinal: -1 }]);
  });

  it("parses mixed ordinal and simple BYDAY", () => {
    const config = rruleToRecurrence("FREQ=MONTHLY;BYDAY=2TU,FR");
    expect(config.ordinalWeekdays).toEqual([
      { day: "TU", ordinal: 2 },
      { day: "FR" },
    ]);
    expect(config.weekdays).toBeUndefined();
  });

  it("parses BYMONTHDAY", () => {
    const config = rruleToRecurrence("FREQ=MONTHLY;BYMONTHDAY=15,28");
    expect(config.byMonthDay).toEqual([15, 28]);
  });

  it("parses BYMONTH", () => {
    const config = rruleToRecurrence("FREQ=YEARLY;BYMONTH=3,9");
    expect(config.byMonth).toEqual([3, 9]);
  });

  it("parses BYSETPOS", () => {
    const config = rruleToRecurrence("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1");
    expect(config.bySetPos).toEqual([-1]);
    expect(config.weekdays).toEqual(["MO", "TU", "WE", "TH", "FR"]);
  });

  it("parses BYYEARDAY", () => {
    const config = rruleToRecurrence("FREQ=YEARLY;BYYEARDAY=1,100,200");
    expect(config.byYearDay).toEqual([1, 100, 200]);
  });

  it("parses BYWEEKNO", () => {
    const config = rruleToRecurrence("FREQ=YEARLY;BYWEEKNO=20,40");
    expect(config.byWeekNo).toEqual([20, 40]);
  });

  it("parses WKST", () => {
    const config = rruleToRecurrence("FREQ=WEEKLY;WKST=SU");
    expect(config.wkst).toBe("SU");
  });

  it("ignores invalid WKST", () => {
    const config = rruleToRecurrence("FREQ=WEEKLY;WKST=XX");
    expect(config.wkst).toBeUndefined();
  });

  it("parses UNTIL with datetime", () => {
    const config = rruleToRecurrence("FREQ=DAILY;UNTIL=20260615T235959Z");
    expect(config.end).toEqual({ type: "until", date: "2026-06-15T23:59:59Z" });
  });
});

describe("RRULE round-trips for BY* rules", () => {
  it("round-trips ordinal BYDAY", () => {
    const rrule = "FREQ=MONTHLY;BYDAY=2TU";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips negative ordinal BYDAY", () => {
    const rrule = "FREQ=MONTHLY;BYDAY=-1FR";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips BYMONTHDAY", () => {
    const rrule = "FREQ=MONTHLY;BYMONTHDAY=15,28";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips BYMONTH", () => {
    const rrule = "FREQ=YEARLY;BYMONTH=3,9";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips BYSETPOS with weekdays", () => {
    const rrule = "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips BYYEARDAY", () => {
    const rrule = "FREQ=YEARLY;BYYEARDAY=1,100";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips BYWEEKNO", () => {
    const rrule = "FREQ=YEARLY;BYWEEKNO=20";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips WKST=SU", () => {
    const rrule = "FREQ=WEEKLY;WKST=SU";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips UNTIL with datetime", () => {
    const rrule = "FREQ=DAILY;UNTIL=20260615T235959Z";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips Google Calendar monthly 2nd Tuesday pattern", () => {
    const rrule = "FREQ=MONTHLY;BYDAY=2TU;COUNT=12";
    const config = rruleToRecurrence(rrule);
    expect(recurrenceToRrule(config)).toBe(rrule);
  });

  it("round-trips Outlook yearly BYMONTH+BYDAY pattern", () => {
    const rrule = "FREQ=YEARLY;BYMONTH=11;BYDAY=4TH";
    const config = rruleToRecurrence(rrule);
    expect(config.frequency).toBe("yearly");
    expect(config.byMonth).toEqual([11]);
    expect(config.ordinalWeekdays).toEqual([{ day: "TH", ordinal: 4 }]);
    // Serializer emits BYDAY before BYMONTH
    expect(recurrenceToRrule(config)).toBe("FREQ=YEARLY;BYDAY=4TH;BYMONTH=11");
  });
});

describe("recurrenceConfigsEqual", () => {
  it("treats selector order and default week start as equivalent", () => {
    expect(recurrenceConfigsEqual(
      {
        frequency: "weekly",
        interval: 1,
        weekdays: ["FR", "MO"],
        wkst: "MO",
        end: { type: "never" },
      },
      {
        frequency: "weekly",
        interval: 1,
        weekdays: ["MO", "FR"],
        end: { type: "never" },
      },
    )).toBe(true);
  });

  it("distinguishes real recurrence changes", () => {
    expect(recurrenceConfigsEqual(
      {
        frequency: "weekly",
        interval: 1,
        weekdays: ["MO", "FR"],
        end: { type: "never" },
      },
      {
        frequency: "weekly",
        interval: 1,
        weekdays: ["MO"],
        end: { type: "never" },
      },
    )).toBe(false);
  });
});

describe("formatRecurrenceLabel", () => {
  it("formats daily", () => {
    expect(
      formatRecurrenceLabel({ frequency: "daily", interval: 1, end: { type: "never" } }),
    ).toBe("Daily");
  });

  it("formats every N days", () => {
    expect(
      formatRecurrenceLabel({ frequency: "daily", interval: 3, end: { type: "never" } }),
    ).toBe("Every 3 days");
  });

  it("formats weekly with weekdays", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "weekly",
        interval: 1,
        weekdays: ["MO", "WE", "FR"],
        end: { type: "never" },
      }),
    ).toBe("Weekly on Mon, Wed, Fri");
  });

  it("formats every N weeks with weekdays", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "weekly",
        interval: 2,
        weekdays: ["MO", "WE"],
        end: { type: "never" },
      }),
    ).toBe("Every 2 weeks on Mon, Wed");
  });

  it("appends until date", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "daily",
        interval: 1,
        end: { type: "until", date: "2026-03-17" },
      }),
    ).toBe("Daily, ends Mar 17");
  });

  it("appends count", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "weekly",
        interval: 1,
        end: { type: "count", count: 5 },
      }),
    ).toBe("Weekly, 5 times");
  });

  it("formats monthly", () => {
    expect(
      formatRecurrenceLabel({ frequency: "monthly", interval: 1, end: { type: "never" } }),
    ).toBe("Monthly");
  });

  it("formats yearly", () => {
    expect(
      formatRecurrenceLabel({ frequency: "yearly", interval: 1, end: { type: "never" } }),
    ).toBe("Yearly");
  });

  it("formats monthly ordinal weekday", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "monthly",
        interval: 1,
        ordinalWeekdays: [{ day: "TU", ordinal: 2 }],
        end: { type: "never" },
      }),
    ).toBe("Monthly on the 2nd Tue");
  });

  it("formats monthly last weekday", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "monthly",
        interval: 1,
        ordinalWeekdays: [{ day: "FR", ordinal: -1 }],
        end: { type: "never" },
      }),
    ).toBe("Monthly on the last Fri");
  });

  it("formats monthly BYMONTHDAY", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "monthly",
        interval: 1,
        byMonthDay: [15],
        end: { type: "never" },
      }),
    ).toBe("Monthly on the 15th");
  });

  it("formats monthly BYMONTHDAY multiple", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "monthly",
        interval: 1,
        byMonthDay: [1, 15],
        end: { type: "never" },
      }),
    ).toBe("Monthly on the 1st, 15th");
  });

  it("formats BYSETPOS", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "monthly",
        interval: 1,
        weekdays: ["MO", "TU", "WE", "TH", "FR"],
        bySetPos: [-1],
        end: { type: "never" },
      }),
    ).toBe("Monthly on Mon, Tue, Wed, Thu, Fri (last occurrence)");
  });

  it("formats ordinal weekday with count end", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "monthly",
        interval: 1,
        ordinalWeekdays: [{ day: "TU", ordinal: 2 }],
        end: { type: "count", count: 12 },
      }),
    ).toBe("Monthly on the 2nd Tue, 12 times");
  });

  it("formats every 2 months ordinal weekday", () => {
    expect(
      formatRecurrenceLabel({
        frequency: "monthly",
        interval: 2,
        ordinalWeekdays: [{ day: "MO", ordinal: 1 }],
        end: { type: "never" },
      }),
    ).toBe("Every 2 months on the 1st Mon");
  });
});
