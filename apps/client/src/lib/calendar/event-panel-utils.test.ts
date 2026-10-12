import { describe, expect, it } from "vitest";
import {
  commitTimeDraft,
  displayTimeDraft,
  moveRovingIndex,
  projectAllDayDefaultForSelection,
  projectDefaultEventTitleForSelection,
  normalizeTimeDraft,
  projectDurationDefaultForSelection,
  restoreTimeDraft,
  sanitizeTimeDraftInput,
} from "./event-panel-utils";

describe("normalizeTimeDraft", () => {
  it("normalizes shorthand hours and compact minutes", () => {
    expect(normalizeTimeDraft("9")).toBe("09:00");
    expect(normalizeTimeDraft("09")).toBe("09:00");
    expect(normalizeTimeDraft("930")).toBe("09:30");
    expect(normalizeTimeDraft("0930")).toBe("09:30");
    expect(normalizeTimeDraft("93")).toBe("09:30");
    expect(normalizeTimeDraft("9:30")).toBe("09:30");
    expect(normalizeTimeDraft("9:3")).toBe("09:30");
    expect(normalizeTimeDraft("2359")).toBe("23:59");
  });

  it("rejects empty, partial, impossible, and nonnumeric drafts", () => {
    expect(normalizeTimeDraft("")).toBeNull();
    expect(normalizeTimeDraft("12:")).toBeNull();
    expect(normalizeTimeDraft("24:00")).toBeNull();
    expect(normalizeTimeDraft("2360")).toBeNull();
    expect(normalizeTimeDraft("nope")).toBeNull();
  });

  it("normalizes 12-hour drafts with explicit meridiem", () => {
    expect(normalizeTimeDraft("7am", "12h", "08:00")).toBe("07:00");
    expect(normalizeTimeDraft("7:30pm", "12h", "08:00")).toBe("19:30");
    expect(normalizeTimeDraft("12am", "12h", "08:00")).toBe("00:00");
    expect(normalizeTimeDraft("12:30pm", "12h", "08:00")).toBe("12:30");
  });

  it("normalizes 12-hour drafts without meridiem to the nearest period", () => {
    expect(normalizeTimeDraft("6", "12h", "17:30")).toBe("18:00");
    expect(normalizeTimeDraft("10", "12h", "09:30")).toBe("10:00");
    expect(normalizeTimeDraft("12:30", "12h", "09:30")).toBe("12:30");
    expect(normalizeTimeDraft("12", "12h", "00:30")).toBe("00:00");
  });
});

describe("sanitizeTimeDraftInput", () => {
  it("keeps only digits and one colon", () => {
    expect(sanitizeTimeDraftInput("1a2b:3c4")).toBe("12:34");
    expect(sanitizeTimeDraftInput("a9-3p0")).toBe("930");
    expect(sanitizeTimeDraftInput("1::2")).toBe("1:2");
  });

  it("optionally keeps a normalized meridiem suffix", () => {
    expect(sanitizeTimeDraftInput("7:30 pm", true)).toBe("7:30pm");
    expect(sanitizeTimeDraftInput("7p", true)).toBe("7pm");
    expect(sanitizeTimeDraftInput("p", true)).toBe("pm");
  });
});

describe("displayTimeDraft", () => {
  it("adds the visual colon as soon as the typed prefix identifies the split", () => {
    expect(displayTimeDraft("12")).toBe("12");
    expect(displayTimeDraft("93")).toBe("9:3");
    expect(displayTimeDraft("25")).toBe("2:5");
    expect(displayTimeDraft("26")).toBe("02:06");
    expect(displayTimeDraft("125")).toBe("12:5");
    expect(displayTimeDraft("126")).toBe("1:26");
    expect(displayTimeDraft("236")).toBe("2:36");
    expect(displayTimeDraft("1200")).toBe("12:00");
    expect(displayTimeDraft("2359")).toBe("23:59");
  });

  it("treats compact pasted values as complete when possible", () => {
    expect(displayTimeDraft("930")).toBe("9:30");
    expect(displayTimeDraft("930", true)).toBe("9:30");
    expect(displayTimeDraft("125", true)).toBe("1:25");
    expect(displayTimeDraft("12")).toBe("12");
  });

  it("preserves explicitly typed colon drafts", () => {
    expect(displayTimeDraft("9:30")).toBe("9:30");
    expect(displayTimeDraft("12:")).toBe("12:");
  });

  it("ignores non-time characters before formatting", () => {
    expect(displayTimeDraft("9p3")).toBe("9:3");
    expect(displayTimeDraft("1a2b:3c4")).toBe("12:34");
    expect(displayTimeDraft("abc")).toBe("");
  });

  it("formats 12-hour editable drafts with meridiem suffixes", () => {
    expect(displayTimeDraft("730pm", false, "12h")).toBe("7:30pm");
    expect(displayTimeDraft("13", false, "12h")).toBe("1:3");
    expect(displayTimeDraft("26", false, "12h")).toBe("2:06");
    expect(displayTimeDraft("12:30am", false, "12h")).toBe("12:30am");
  });
});

describe("commitTimeDraft", () => {
  it("commits valid drafts and keeps invalid drafts from mutating time", () => {
    expect(commitTimeDraft("930", "08:00")).toEqual({ value: "09:30", committed: true });
    expect(commitTimeDraft("24:00", "08:00")).toEqual({ value: "08:00", committed: false });
  });

  it("commits 12-hour drafts and infers missing meridiem from the previous time", () => {
    expect(commitTimeDraft("6:30", "17:30", "12h")).toEqual({ value: "18:30", committed: true });
    expect(commitTimeDraft("6:30am", "17:30", "12h")).toEqual({ value: "06:30", committed: true });
  });

  it("restores the canonical value when editing is cancelled", () => {
    expect(restoreTimeDraft("14:30")).toBe("14:30");
    expect(restoreTimeDraft("14:30", "12h")).toBe("2:30pm");
    expect(restoreTimeDraft("00:00", "12h")).toBe("12am");
  });
});

describe("moveRovingIndex", () => {
  it("moves through horizontal lists and respects boundaries", () => {
    expect(moveRovingIndex({ currentIndex: 0, itemCount: 3, key: "ArrowLeft", orientation: "horizontal" })).toBe(0);
    expect(moveRovingIndex({ currentIndex: 0, itemCount: 3, key: "ArrowRight", orientation: "horizontal" })).toBe(1);
    expect(moveRovingIndex({ currentIndex: 2, itemCount: 3, key: "ArrowRight", orientation: "horizontal" })).toBe(2);
  });

  it("moves through vertical lists and handles Home and End", () => {
    expect(moveRovingIndex({ currentIndex: 1, itemCount: 4, key: "ArrowUp", orientation: "vertical" })).toBe(0);
    expect(moveRovingIndex({ currentIndex: 1, itemCount: 4, key: "ArrowDown", orientation: "vertical" })).toBe(2);
    expect(moveRovingIndex({ currentIndex: 1, itemCount: 4, key: "Home", orientation: "vertical" })).toBe(0);
    expect(moveRovingIndex({ currentIndex: 1, itemCount: 4, key: "End", orientation: "vertical" })).toBe(3);
  });

  it("moves through fixed-column grids", () => {
    expect(moveRovingIndex({ currentIndex: 0, itemCount: 8, key: "ArrowDown", orientation: "grid", columns: 4 })).toBe(4);
    expect(moveRovingIndex({ currentIndex: 5, itemCount: 8, key: "ArrowUp", orientation: "grid", columns: 4 })).toBe(1);
    expect(moveRovingIndex({ currentIndex: 7, itemCount: 8, key: "ArrowDown", orientation: "grid", columns: 4 })).toBe(7);
    expect(moveRovingIndex({ currentIndex: 0, itemCount: 8, key: "End", orientation: "grid", columns: 4 })).toBe(7);
  });
});

describe("projectDefaultEventTitleForSelection", () => {
  it("uses the project default event name when the current title is blank", () => {
    expect(projectDefaultEventTitleForSelection({
      currentTitle: "",
      defaultEventName: "Reading",
    })).toBe("Reading");
    expect(projectDefaultEventTitleForSelection({
      currentTitle: "   ",
      defaultEventName: "Exercise",
    })).toBe("Exercise");
  });

  it("keeps the current title when it already has text", () => {
    expect(projectDefaultEventTitleForSelection({
      currentTitle: "Existing title",
      defaultEventName: "Reading",
    })).toBe("Existing title");
  });

  it("does not fall back to the project name when no default event name is stored", () => {
    expect(projectDefaultEventTitleForSelection({
      currentTitle: "",
      defaultEventName: null,
    })).toBe("");
    expect(projectDefaultEventTitleForSelection({
      currentTitle: "   ",
      defaultEventName: null,
    })).toBe("");
  });
});

describe("projectDurationDefaultForSelection", () => {
  it("applies project duration defaults to create and inactive edit selections", () => {
    expect(projectDurationDefaultForSelection({
      mode: "create",
      allDay: false,
      activeEdit: false,
      defaultEventTimeMode: "timed",
      defaultEventDurationMinutes: 90,
    })).toBe(90);
    expect(projectDurationDefaultForSelection({
      mode: "edit",
      allDay: false,
      activeEdit: false,
      defaultEventTimeMode: "timed",
      defaultEventDurationMinutes: 45,
    })).toBe(45);
  });

  it("skips duration defaults for all-day and active edit selections", () => {
    expect(projectDurationDefaultForSelection({
      mode: "create",
      allDay: true,
      activeEdit: false,
      defaultEventTimeMode: "timed",
      defaultEventDurationMinutes: 90,
    })).toBeNull();
    expect(projectDurationDefaultForSelection({
      mode: "edit",
      allDay: false,
      activeEdit: true,
      defaultEventTimeMode: "timed",
      defaultEventDurationMinutes: 90,
    })).toBeNull();
  });

  it("skips duration defaults for all-day project defaults", () => {
    expect(projectDurationDefaultForSelection({
      mode: "create",
      allDay: false,
      activeEdit: false,
      defaultEventTimeMode: "all_day",
      defaultEventDurationMinutes: null,
    })).toBeNull();
  });

  it("keeps the current duration when the project has no duration default", () => {
    expect(projectDurationDefaultForSelection({
      mode: "edit",
      allDay: false,
      activeEdit: false,
      defaultEventTimeMode: "timed",
      defaultEventDurationMinutes: null,
    })).toBeNull();
  });
});

describe("projectAllDayDefaultForSelection", () => {
  it("applies all-day project defaults to timed create selections", () => {
    expect(projectAllDayDefaultForSelection({
      mode: "create",
      allDay: false,
      activeEdit: false,
      defaultEventTimeMode: "all_day",
      defaultEventDurationMinutes: null,
    })).toBe(true);
  });

  it("does not apply all-day defaults to all-day or active edit selections", () => {
    expect(projectAllDayDefaultForSelection({
      mode: "create",
      allDay: true,
      activeEdit: false,
      defaultEventTimeMode: "all_day",
      defaultEventDurationMinutes: null,
    })).toBe(false);
    expect(projectAllDayDefaultForSelection({
      mode: "edit",
      allDay: false,
      activeEdit: true,
      defaultEventTimeMode: "all_day",
      defaultEventDurationMinutes: null,
    })).toBe(false);
  });
});
