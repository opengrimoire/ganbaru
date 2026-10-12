import { describe, expect, it } from "vitest";
import {
  formatNotesActivityDate,
  formatNotesActivityTime,
  type NotesActivityTimeLabels,
} from "./activity";

const labels: NotesActivityTimeLabels = {
  justNow: "just now",
  minutesAgo: (minutes) => `${minutes}m ago`,
  hoursAgo: (hours) => `${hours}h ago`,
};

describe("notes page activity formatting", () => {
  const now = new Date(2026, 6, 9, 12, 0, 0);

  it("formats very recent edits as just now", () => {
    expect(
      formatNotesActivityTime(new Date(now.getTime() - 30_000), {
        locale: "en-US",
        now,
        labels,
      }),
    ).toBe("just now");
  });

  it("formats full elapsed minutes without decimals", () => {
    expect(
      formatNotesActivityTime(new Date(now.getTime() - 2.8 * 60_000), {
        locale: "en-US",
        now,
        labels,
      }),
    ).toBe("2m ago");
  });

  it("formats full elapsed hours before the 24 hour cutoff", () => {
    expect(
      formatNotesActivityTime(new Date(now.getTime() - 14.5 * 60 * 60_000), {
        locale: "en-US",
        now,
        labels,
      }),
    ).toBe("14h ago");
  });

  it("formats older timestamps as a short date without the year when possible", () => {
    expect(
      formatNotesActivityTime(new Date(2026, 6, 8, 11, 0, 0), {
        locale: "en-US",
        now,
        labels,
      }),
    ).toBe("Jul 8");
  });

  it("includes the year for dates from another year", () => {
    expect(formatNotesActivityDate(new Date(2025, 6, 8, 11, 0, 0), "en-US", now))
      .toBe("Jul 8, 2025");
  });
});
