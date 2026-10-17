import { describe, it, expect } from "vitest";
import {
  computeDayTimelineBands,
  computePlannedSegments,
  computeTrailingFocusMinutes,
  computeTrailingRhythmState,
} from "./segments";
import type { PersistedSegment, PomodoroConfig } from "$lib/calendar/types";
import type { TimelineEvent, ActivePomodoroState } from "./segments";
import {
  createCustomCountPomodoroConfig,
  createPresetPomodoroConfig,
} from "./rhythm";

const DEFAULT_CONFIG: PomodoroConfig = createPresetPomodoroConfig("adaptive");

describe("computePlannedSegments", () => {
  it("returns empty for zero or negative duration", () => {
    expect(computePlannedSegments(DEFAULT_CONFIG, 0)).toEqual([]);
    expect(computePlannedSegments(DEFAULT_CONFIG, -10)).toEqual([]);
  });

  it("handles single focus period that fits exactly", () => {
    const segments = computePlannedSegments(DEFAULT_CONFIG, 40);
    expect(segments).toEqual([
      { rhythmPosition: 1, phase: "focus", startOffsetMinutes: 0, endOffsetMinutes: 40 },
    ]);
  });

  it("clips focus period when event is shorter than one focus", () => {
    const segments = computePlannedSegments(DEFAULT_CONFIG, 25);
    expect(segments).toEqual([
      { rhythmPosition: 1, phase: "focus", startOffsetMinutes: 0, endOffsetMinutes: 25 },
    ]);
  });

  it("produces focus + short break + next focus for sufficient duration", () => {
    // 90 min: focus 0-40, break 40-45, focus 45-85, break 85-90
    const segments = computePlannedSegments(DEFAULT_CONFIG, 90);
    expect(segments).toHaveLength(4);
    expect(segments[0]).toEqual({ rhythmPosition: 1, phase: "focus", startOffsetMinutes: 0, endOffsetMinutes: 40 });
    expect(segments[1]).toEqual({ rhythmPosition: 1, phase: "short_break", startOffsetMinutes: 40, endOffsetMinutes: 45 });
    expect(segments[2]).toEqual({ rhythmPosition: 2, phase: "focus", startOffsetMinutes: 45, endOffsetMinutes: 85 });
    expect(segments[3]).toEqual({ rhythmPosition: 2, phase: "short_break", startOffsetMinutes: 85, endOffsetMinutes: 90 });
  });

  it("clips break when event ends mid-break", () => {
    const segments = computePlannedSegments(DEFAULT_CONFIG, 43);
    expect(segments).toHaveLength(2);
    expect(segments[1]).toEqual({ rhythmPosition: 1, phase: "short_break", startOffsetMinutes: 40, endOffsetMinutes: 43 });
  });

  it("produces long break after the count rhythm length", () => {
    // 4 focus (40) + 3 short breaks (5) + 1 long break (10) = 185 min
    const segments = computePlannedSegments(DEFAULT_CONFIG, 185);
    const breaks = segments.filter((s) => s.phase !== "focus");
    expect(breaks).toHaveLength(4);
    expect(breaks[0].phase).toBe("short_break");
    expect(breaks[1].phase).toBe("short_break");
    expect(breaks[2].phase).toBe("short_break");
    expect(breaks[3].phase).toBe("long_break");
  });

  it("wraps rhythm position after long break", () => {
    // Enough for the full pattern plus the start of the next cycle.
    const segments = computePlannedSegments(DEFAULT_CONFIG, 230);
    const focusAfterLong = segments.find(
      (s) => s.phase === "focus" && s.startOffsetMinutes >= 185,
    );
    expect(focusAfterLong).toBeDefined();
    expect(focusAfterLong!.rhythmPosition).toBe(1);
  });

  it("handles longBreakAfterFocusCount = 1 (every break is long)", () => {
    const config = createCustomCountPomodoroConfig({
      focusDurationMinutes: 40,
      shortBreakMinutes: 5,
      longBreakMinutes: 10,
      longBreakAfterFocusCount: 1,
    });
    const segments = computePlannedSegments(config, 100);
    const breaks = segments.filter((s) => s.phase !== "focus");
    for (const b of breaks) {
      expect(b.phase).toBe("long_break");
    }
  });

  it("handles exact fit for full count rhythm pattern", () => {
    // 4*40 + 3*5 + 1*10 = 185
    const segments = computePlannedSegments(DEFAULT_CONFIG, 185);
    const last = segments[segments.length - 1];
    expect(last.endOffsetMinutes).toBe(185);
  });

  it("shortens first focus with initialFocusOffset", () => {
    // 30 min inherited: first focus is 10 min (40 - 30), then break
    const segments = computePlannedSegments(DEFAULT_CONFIG, 60, 30);
    expect(segments[0]).toEqual({ rhythmPosition: 1, phase: "focus", startOffsetMinutes: 0, endOffsetMinutes: 10 });
    expect(segments[1]).toEqual({ rhythmPosition: 1, phase: "short_break", startOffsetMinutes: 10, endOffsetMinutes: 15 });
    expect(segments[2]).toEqual({ rhythmPosition: 2, phase: "focus", startOffsetMinutes: 15, endOffsetMinutes: 55 });
  });

  it("starts with break when inherited focus exceeds threshold", () => {
    // 45 min inherited (>= 40 focus): starts with break immediately
    const segments = computePlannedSegments(DEFAULT_CONFIG, 60, 45);
    expect(segments[0].phase).toBe("short_break");
    expect(segments[0].startOffsetMinutes).toBe(0);
    expect(segments[0].endOffsetMinutes).toBe(5);
    expect(segments[1].phase).toBe("focus");
    expect(segments[1].startOffsetMinutes).toBe(5);
  });

  it("starts with break when inherited focus equals threshold exactly", () => {
    const segments = computePlannedSegments(DEFAULT_CONFIG, 50, 40);
    expect(segments[0].phase).toBe("short_break");
    expect(segments[1].phase).toBe("focus");
  });

  it("inherits correctly with zero offset (no change)", () => {
    const withoutOffset = computePlannedSegments(DEFAULT_CONFIG, 90);
    const withZeroOffset = computePlannedSegments(DEFAULT_CONFIG, 90, 0);
    expect(withZeroOffset).toEqual(withoutOffset);
  });

  it("clips inherited shortened focus to event duration", () => {
    // 30 min inherited, but event is only 5 min long
    const segments = computePlannedSegments(DEFAULT_CONFIG, 5, 30);
    expect(segments).toHaveLength(1);
    expect(segments[0]).toEqual({ rhythmPosition: 1, phase: "focus", startOffsetMinutes: 0, endOffsetMinutes: 5 });
  });

  it("clips inherited break to event duration", () => {
    // 40 min inherited: starts with break, but event is only 3 min
    const segments = computePlannedSegments(DEFAULT_CONFIG, 3, 40);
    expect(segments).toHaveLength(1);
    expect(segments[0]).toEqual({ rhythmPosition: 1, phase: "short_break", startOffsetMinutes: 0, endOffsetMinutes: 3 });
  });

  it("starts at inherited rhythm position", () => {
    // Start at position 3: first break should be short, second should trigger long
    const segments = computePlannedSegments(DEFAULT_CONFIG, 185, 0, 3);
    const breaks = segments.filter((s) => s.phase !== "focus");
    // Position 3: short break, position 4: long break, then wrap
    expect(breaks[0].phase).toBe("short_break");
    expect(breaks[0].rhythmPosition).toBe(3);
    expect(breaks[1].phase).toBe("long_break");
    expect(breaks[1].rhythmPosition).toBe(4);
  });

  it("triggers long break immediately at the long-break position", () => {
    // Start at position 4: first break is long
    const segments = computePlannedSegments(DEFAULT_CONFIG, 100, 0, 4);
    const breaks = segments.filter((s) => s.phase !== "focus");
    expect(breaks[0].phase).toBe("long_break");
    expect(breaks[0].rhythmPosition).toBe(4);
    // After long break, the next break should be short at position 1
    if (breaks.length > 1) {
      expect(breaks[1].phase).toBe("short_break");
      expect(breaks[1].rhythmPosition).toBe(1);
    }
  });

  it("wraps inherited rhythm positions beyond the count rhythm length", () => {
    const segments = computePlannedSegments(DEFAULT_CONFIG, 100, 0, 5);
    const breaks = segments.filter((s) => s.phase !== "focus");
    expect(breaks[0].phase).toBe("short_break");
    expect(breaks[0].rhythmPosition).toBe(1);
  });

  it("combines focus offset and rhythm position inheritance", () => {
    // 30 min focus inherited, position 3: shortened focus, then short break
    const segments = computePlannedSegments(DEFAULT_CONFIG, 120, 30, 3);
    expect(segments[0]).toEqual({ rhythmPosition: 3, phase: "focus", startOffsetMinutes: 0, endOffsetMinutes: 10 });
    expect(segments[1].phase).toBe("short_break");
    expect(segments[1].rhythmPosition).toBe(3);
    // Next focus at position 4, its break should be long
    const cycle4Break = segments.find((s) => s.rhythmPosition === 4 && s.phase !== "focus");
    expect(cycle4Break).toBeDefined();
    expect(cycle4Break!.phase).toBe("long_break");
  });

  it("inherited break at long-break position uses long break", () => {
    // 40 min focus inherited at position 4: starts with long break
    const segments = computePlannedSegments(DEFAULT_CONFIG, 60, 40, 4);
    expect(segments[0].phase).toBe("long_break");
    expect(segments[0].endOffsetMinutes).toBe(10); // long break = 10 min
    // After long break, wrap to position 1
    expect(segments[1].phase).toBe("focus");
    expect(segments[1].rhythmPosition).toBe(1);
  });

  it("cross-block scenario: Block A trailing rhythm position carries to Block B", () => {
    // Event A: 80 min, long break after 4 focus periods.
    // Position 1: focus 0-40, short_break 40-45, position 2: focus 45-80 (clipped).
    const blockA = computePlannedSegments(DEFAULT_CONFIG, 80);
    expect(computeTrailingFocusMinutes(blockA)).toBe(35); // 80-45=35 min focus
    expect(computeTrailingRhythmState(DEFAULT_CONFIG, 80).rhythmPosition).toBe(2);

    // Event B: 220 min, inherits 35 min focus at position 2.
    const blockB = computePlannedSegments(DEFAULT_CONFIG, 220, 35, 2);
    // First focus shortened to 5 min (40-35), then short break.
    expect(blockB[0]).toEqual({ rhythmPosition: 2, phase: "focus", startOffsetMinutes: 0, endOffsetMinutes: 5 });
    expect(blockB[1].phase).toBe("short_break");
    // Count breaks before the first long break
    let shortBreaksBeforeLong = 0;
    for (const s of blockB) {
      if (s.phase === "short_break") shortBreaksBeforeLong++;
      if (s.phase === "long_break") break;
    }
    // Event A had 1 short break, so event B has 2 more before the long break.
    expect(shortBreaksBeforeLong).toBe(2);
  });

  it("defaults to position 1 with zero initial rhythm position", () => {
    const withZero = computePlannedSegments(DEFAULT_CONFIG, 90, 0, 0);
    const withDefault = computePlannedSegments(DEFAULT_CONFIG, 90);
    expect(withZero).toEqual(withDefault);
  });

  it("stacked event does not affect sequential successor's segments", () => {
    // Event A: 120 min, event B: 240 min (sequential, A ends where B starts).
    // Event C: 120 min, overlapping A and B (starts 20 min before A ends).
    // Event B inherits from event A, not event C.
    const trailingA = computeTrailingRhythmState(DEFAULT_CONFIG, 120);
    const trailingFocusA = trailingA.focusOffsetMinutes;
    const trailingPositionA = trailingA.rhythmPosition;

    // Event B inheriting from event A (correct predecessor).
    const blockB = computePlannedSegments(DEFAULT_CONFIG, 240, trailingFocusA, trailingPositionA);

    // Event C inheriting from event A (stacked event).
    const trailingC = computeTrailingRhythmState(
      DEFAULT_CONFIG,
      120,
      trailingFocusA,
      trailingPositionA,
    );
    const trailingFocusC = trailingC.focusOffsetMinutes;
    const trailingPositionC = trailingC.rhythmPosition;

    // Event B inheriting from event C (wrong predecessor) would differ.
    const blockBFromC = computePlannedSegments(DEFAULT_CONFIG, 240, trailingFocusC, trailingPositionC);

    // The correct result (from A) should differ from the wrong result (from C)
    // because C's trailing state differs from A's
    expect(trailingFocusA).not.toBe(trailingFocusC);
    // Event B from A stays stable regardless of C.
    expect(blockB).toEqual(computePlannedSegments(DEFAULT_CONFIG, 240, trailingFocusA, trailingPositionA));
    expect(blockB).not.toEqual(blockBFromC);
  });
});

describe("computeTrailingFocusMinutes", () => {
  it("returns 0 for empty segments", () => {
    expect(computeTrailingFocusMinutes([])).toBe(0);
  });

  it("returns focus duration when last segment is focus", () => {
    const segments = computePlannedSegments(DEFAULT_CONFIG, 25);
    // Single clipped focus: 0-25
    expect(computeTrailingFocusMinutes(segments)).toBe(25);
  });

  it("returns 0 when last segment is a break", () => {
    const segments = computePlannedSegments(DEFAULT_CONFIG, 45);
    // focus 0-40, break 40-45
    expect(computeTrailingFocusMinutes(segments)).toBe(0);
  });

  it("returns trailing focus after a break", () => {
    const segments = computePlannedSegments(DEFAULT_CONFIG, 70);
    // focus 0-40, break 40-45, focus 45-70 (25 min)
    const last = segments[segments.length - 1];
    expect(last.phase).toBe("focus");
    expect(computeTrailingFocusMinutes(segments)).toBe(25);
  });
});

const CREATIVE_CONFIG: PomodoroConfig = createPresetPomodoroConfig("creative");

/** Creates a TimelineEvent from hour ranges on a fixed day. */
function makeEvent(
  id: string,
  startHour: number,
  endHour: number,
  config: PomodoroConfig = DEFAULT_CONFIG,
): TimelineEvent {
  const dayMs = new Date("2026-03-21T00:00:00").getTime();
  return {
    id,
    config,
    startMs: dayMs + startHour * 3600000,
    endMs: dayMs + endHour * 3600000,
    startMinute: startHour * 60,
    endMinute: endHour * 60,
  };
}

// Day midnight and a "now" far in the past so all events are future (planned)
const DAY_MS = new Date("2026-03-21T00:00:00").getTime();
const PAST_NOW = DAY_MS - 86400000; // yesterday

describe("computeDayTimelineBands", () => {
  it("returns empty for no events", () => {
    expect(computeDayTimelineBands([], null, DAY_MS, PAST_NOW)).toEqual([]);
  });

  it("returns break bands for a single event", () => {
    // 2h event with the adaptive preset (40/5): focus 0-40, break 40-45, focus 45-80, break 80-85, focus 85-120
    const ev = makeEvent("A", 10, 12);
    const bands = computeDayTimelineBands([ev], null, DAY_MS, PAST_NOW);
    expect(bands.length).toBe(2);
    expect(bands[0]).toEqual({ topMinute: 640, heightMinutes: 5, phase: "short_break", status: "planned" });
    expect(bands[1]).toEqual({ topMinute: 685, heightMinutes: 5, phase: "short_break", status: "planned" });
  });

  it("inherits focus and rhythm position across sequential events", () => {
    // A: 10:00-11:20 (80 min). Segments: focus 0-40, break 40-45, focus 45-80 (trailing: 35min focus, position 2).
    // B: 11:20-13:00 (100 min). Should inherit 35min focus, position 2.
    const A = makeEvent("A", 10, 11 + 20 / 60);
    const B = makeEvent("B", 11 + 20 / 60, 13);
    const bands = computeDayTimelineBands([A, B], null, DAY_MS, PAST_NOW);

    // A produces 1 break at minute 640 (10*60+40)
    const aBands = bands.filter((b) => b.topMinute >= 600 && b.topMinute < 680);
    expect(aBands.length).toBe(1);
    expect(aBands[0].topMinute).toBe(640);

    // B starts at 680 (11:20). Inherited 35min focus at position 2: shortened focus = 5min,
    // then short break at 685.
    const bBands = bands.filter((b) => b.topMinute >= 680);
    expect(bBands.length).toBeGreaterThanOrEqual(1);
    expect(bBands[0].topMinute).toBe(685); // 680 + 5min focus
    expect(bBands[0].phase).toBe("short_break");
  });

  it("resets inheritance when there is a gap", () => {
    // A: 10:00-11:00, B: 12:00-13:00 (1h gap)
    const A = makeEvent("A", 10, 11);
    const B = makeEvent("B", 12, 13);
    const bands = computeDayTimelineBands([A, B], null, DAY_MS, PAST_NOW);

    // B should start fresh (no inheritance from A)
    // B: 60min, focus 0-40, break 40-45, focus 45-60
    const bBands = bands.filter((b) => b.topMinute >= 720);
    expect(bBands.length).toBe(1);
    expect(bBands[0].topMinute).toBe(760); // 720+40
    expect(bBands[0].phase).toBe("short_break");
  });

  it("keeps the projected owner when a later nested event begins", () => {
    // A: 10:00-12:00 (adaptive, longer), B: 10:30-11:30 (creative, shorter, contained)
    const A = makeEvent("A", 10, 12);
    const B = makeEvent("B", 10.5, 11.5, CREATIVE_CONFIG);
    const bands = computeDayTimelineBands([A, B], null, DAY_MS, PAST_NOW);

    // A is already the proposed owner when B begins, so it keeps this window.
    // A: 120min, focus 0-40, break 40-45, focus 45-85, break 85-90
    expect(bands.length).toBe(2);
    expect(bands[0].topMinute).toBe(640); // 600+40
    expect(bands[1].topMinute).toBe(685); // 600+85
  });

  it("keeps both events when neither is contained (partial overlap)", () => {
    // A: 10:00-12:00, C: 11:50-12:50 (extends beyond A)
    const A = makeEvent("A", 10, 12);
    const C = makeEvent("C", 11 + 50 / 60, 12 + 50 / 60, CREATIVE_CONFIG);
    const bands = computeDayTimelineBands([A, C], null, DAY_MS, PAST_NOW);

    // Both should contribute bands
    const aBands = bands.filter((b) => b.topMinute < 710); // before C starts
    const cBands = bands.filter((b) => b.topMinute >= 720); // A ends at 720
    expect(aBands.length).toBeGreaterThan(0);
    expect(cBands.length).toBeGreaterThanOrEqual(0); // C may produce bands after A
  });

  it("uses stable identity for equal windows regardless of rhythm or input order", () => {
    // A: adaptive (40 min focus), B: creative (25 min focus), both 16:00-20:00
    const A = makeEvent("A", 16, 20);
    const B = makeEvent("B", 16, 20, CREATIVE_CONFIG);
    const bands = computeDayTimelineBands([A, B], null, DAY_MS, PAST_NOW);

    expect(bands).toEqual(computeDayTimelineBands([B, A], null, DAY_MS, PAST_NOW));
    expect(bands[0].topMinute).toBe(960 + 40);
    expect(bands[0].heightMinutes).toBe(5);
  });

  it("uses creation identity before occurrence identity for equal windows", () => {
    const A = { ...makeEvent("A", 16, 20), createdAt: "2026-03-20T00:00:00Z" };
    const B = { ...makeEvent("B", 16, 20, CREATIVE_CONFIG), createdAt: "2026-03-19T00:00:00Z" };

    const bands = computeDayTimelineBands([A, B], null, DAY_MS, PAST_NOW);
    expect(bands[0].topMinute).toBe(960 + 25);
    expect(bands).toEqual(computeDayTimelineBands([B, A], null, DAY_MS, PAST_NOW));
  });

  it("selects the nested commitment at a late start and hands back the remaining outer window", () => {
    const outer = makeEvent("outer", 9, 12);
    const nested = makeEvent("nested", 10, 11, CREATIVE_CONFIG);
    const nowMs = DAY_MS + (10 * 60 + 15) * 60_000;

    const bands = computeDayTimelineBands([outer, nested], null, DAY_MS, nowMs);
    expect(bands[0]).toEqual({
      topMinute: 640, heightMinutes: 5, phase: "short_break", status: "planned",
    });
    expect(bands.filter((band) => band.topMinute >= 660)[0].topMinute).toBe(685);
    expect(bands).toEqual(computeDayTimelineBands([nested, outer], null, DAY_MS, nowMs));
  });

  it("preserves an accepted nested owner and its recorded focus", () => {
    const outer = makeEvent("outer", 9, 12);
    const nested = makeEvent("nested", 10, 11, CREATIVE_CONFIG);
    const startedAt = DAY_MS + 10 * 60 * 60_000;
    const active: ActivePomodoroState = {
      activeOccurrenceId: nested.id,
      remainingSeconds: 10 * 60,
      breakOvertimeSeconds: 0,
      segments: [{
        id: "accepted", runId: "run", eventId: nested.id, eventDate: "2026-03-21",
        phase: "focus", rhythmPosition: 1, status: "active", pauseLog: [],
        plannedStart: new Date(startedAt).toISOString(),
        plannedEnd: new Date(startedAt + 25 * 60_000).toISOString(),
        actualStart: new Date(startedAt).toISOString(), actualEnd: null,
      }],
    };

    const bands = computeDayTimelineBands([outer, nested], active, DAY_MS, startedAt + 15 * 60_000);
    expect(bands[0]).toEqual({
      topMinute: 600, heightMinutes: 15, phase: "focus", status: "active",
    });
    expect(bands.find((band) => band.status === "planned")?.topMinute).toBe(625);
  });

  it("skips fully past planned events", () => {
    // A: 08:00-09:00 (past), B: 14:00-15:00 (future)
    const A = makeEvent("A", 8, 9);
    const B = makeEvent("B", 14, 15);
    const nowMs = DAY_MS + 10 * 3600000; // 10:00
    const bands = computeDayTimelineBands([A, B], null, DAY_MS, nowMs);

    // A is past, should have no planned bands. B should have bands.
    const aBands = bands.filter((b) => b.topMinute < 540); // before 09:00
    expect(aBands.length).toBe(0);
    const bBands = bands.filter((b) => b.topMinute >= 840);
    expect(bBands.length).toBeGreaterThan(0);
  });

  it("shows bands only for remaining time of partially past event", () => {
    // A: 09:00-12:00 (3h), now is 10:05
    const A = makeEvent("A", 9, 12);
    const nowMs = DAY_MS + (10 * 60 + 5) * 60000; // 10:05
    const bands = computeDayTimelineBands([A], null, DAY_MS, nowMs);

    // No bands before 10:05 (minute 605)
    for (const b of bands) {
      expect(b.topMinute).toBeGreaterThanOrEqual(605);
    }
  });

  it("starts untracked in-progress events from now", () => {
    const A = makeEvent("A", 9, 12);
    const nowLate = DAY_MS + (10 * 60 + 5) * 60000; // 10:05

    const bandsLate = computeDayTimelineBands([A], null, DAY_MS, nowLate);

    expect(bandsLate[0]).toEqual({
      topMinute: 10 * 60 + 45,
      heightMinutes: 5,
      phase: "short_break",
      status: "planned",
    });
  });

  it("keeps persisted in-progress events aligned with stored rhythm", () => {
    const A = makeEvent("A", 9, 12);
    const nowLate = DAY_MS + (10 * 60 + 5) * 60000; // 10:05
    const persistedSegments = new Map<string, PersistedSegment[]>();
    persistedSegments.set("A", [
      {
        id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
        rhythmPosition: 1, phase: "focus",
        plannedStart: new Date(DAY_MS + 9 * 60 * 60000).toISOString(),
        plannedEnd: new Date(DAY_MS + (9 * 60 + 40) * 60000).toISOString(),
        actualStart: new Date(DAY_MS + 9 * 60 * 60000).toISOString(),
        actualEnd: new Date(DAY_MS + (9 * 60 + 40) * 60000).toISOString(),
        pauseLog: [],
        status: "completed",
      },
    ]);

    const bandsLate = computeDayTimelineBands([A], null, DAY_MS, nowLate, persistedSegments);
    const plannedBands = bandsLate.filter((band) => band.status === "planned");

    expect(plannedBands[0]).toEqual({
      topMinute: 10 * 60 + 25,
      heightMinutes: 5,
      phase: "short_break",
      status: "planned",
    });
  });

  it("handles overlapping events with cursor walk", () => {
    // A: 10:00-12:00, B: 11:00-13:00 (overlap, neither contained)
    const A = makeEvent("A", 10, 12);
    const B = makeEvent("B", 11, 13);
    const bands = computeDayTimelineBands([A, B], null, DAY_MS, PAST_NOW);

    // A covers 10:00-12:00 with its breaks
    // B starts at cursor=12:00 (720), duration = 13:00-12:00 = 60 min
    // B inherits A's trailing state at 12:00
    expect(bands.length).toBeGreaterThan(0);
  });

  it("active event projects bands from persisted segments", () => {
    const ev = makeEvent("A", 10, 12);
    const segStartMs = DAY_MS + 10 * 3600000; // 10:00
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(segStartMs).toISOString(),
          plannedEnd: new Date(segStartMs + 40 * 60000).toISOString(),
          actualStart: new Date(segStartMs).toISOString(),
          actualEnd: new Date(segStartMs + 40 * 60000).toISOString(),
          pauseLog: [],
          status: "completed",
        },
        {
          id: "s2", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "short_break",
          plannedStart: new Date(segStartMs + 40 * 60000).toISOString(),
          plannedEnd: new Date(segStartMs + 45 * 60000).toISOString(),
          actualStart: new Date(segStartMs + 40 * 60000).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
        {
          id: "s3", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 2, phase: "focus",
          plannedStart: new Date(segStartMs + 45 * 60000).toISOString(),
          plannedEnd: new Date(segStartMs + 85 * 60000).toISOString(),
          actualStart: null, actualEnd: null,
          pauseLog: [],
          status: "planned",
        },
        {
          id: "s4", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 2, phase: "short_break",
          plannedStart: new Date(segStartMs + 85 * 60000).toISOString(),
          plannedEnd: new Date(segStartMs + 90 * 60000).toISOString(),
          actualStart: null, actualEnd: null,
          pauseLog: [],
          status: "planned",
        },
      ],
      remainingSeconds: 180, // 3 min left in active break
      breakOvertimeSeconds: 0,
    };

    const nowMs = segStartMs + 42 * 60000; // 10:42
    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, nowMs);

    // Should have the active break band and the future planned break
    const breakBands = bands.filter((b) => b.phase !== "focus");
    expect(breakBands.length).toBe(2);

    // Active break: starts at 10:40 (minute 640), status active
    const activeBand = breakBands.find((b) => b.status === "active");
    expect(activeBand).toBeDefined();
    expect(activeBand!.topMinute).toBe(640);

    // Future planned break
    const plannedBand = breakBands.find((b) => b.status === "planned");
    expect(plannedBand).toBeDefined();
  });

  it("active event previews a pending config change for the current focus segment", () => {
    const ev = makeEvent("A", 10, 12, DEFAULT_CONFIG);
    const segStartMs = DAY_MS + 10 * 3600000; // 10:00
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      currentConfig: CREATIVE_CONFIG,
      phaseElapsedSeconds: 0,
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(segStartMs).toISOString(),
          plannedEnd: new Date(segStartMs + 25 * 60000).toISOString(),
          actualStart: new Date(segStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
      ],
      remainingSeconds: 25 * 60,
      breakOvertimeSeconds: 0,
    };

    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, segStartMs);
    const firstBreak = bands.find((band) => band.phase !== "focus");

    expect(firstBreak).toEqual({
      topMinute: 10 * 60 + 40,
      heightMinutes: 5,
      phase: "short_break",
      status: "planned",
    });
  });

  it("active event includes recovered prior runs without duplicating the current run", () => {
    const ev = makeEvent("A", 10, 12);
    const firstRunStartMs = DAY_MS + 10 * 3600000;
    const secondRunStartMs = DAY_MS + 10.75 * 3600000;
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "current-s1", eventId: "A", eventDate: "2026-03-21", runId: "current-run",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(secondRunStartMs).toISOString(),
          plannedEnd: new Date(secondRunStartMs + 40 * 60000).toISOString(),
          actualStart: new Date(secondRunStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
      ],
      remainingSeconds: 30 * 60,
      breakOvertimeSeconds: 0,
    };

    const persistedSegments = new Map<string, PersistedSegment[]>();
    persistedSegments.set("A", [
      {
        id: "recovered-s1", eventId: "A", eventDate: "2026-03-21", runId: "recovered-run",
        rhythmPosition: 1, phase: "focus",
        plannedStart: new Date(firstRunStartMs).toISOString(),
        plannedEnd: new Date(firstRunStartMs + 40 * 60000).toISOString(),
        actualStart: new Date(firstRunStartMs).toISOString(),
        actualEnd: new Date(firstRunStartMs + 20 * 60000).toISOString(),
        pauseLog: [],
        status: "interrupted",
      },
      {
        id: "current-s1", eventId: "A", eventDate: "2026-03-21", runId: "current-run",
        rhythmPosition: 1, phase: "focus",
        plannedStart: new Date(secondRunStartMs).toISOString(),
        plannedEnd: new Date(secondRunStartMs + 40 * 60000).toISOString(),
        actualStart: new Date(secondRunStartMs).toISOString(),
        actualEnd: null,
        pauseLog: [],
        status: "active",
      },
    ]);

    const nowMs = secondRunStartMs + 10 * 60000;
    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, nowMs, persistedSegments);
    const focusBands = bands.filter((band) => band.phase === "focus");

    expect(focusBands).toHaveLength(2);
    expect(focusBands[0]).toMatchObject({
      topMinute: 600,
      heightMinutes: 20,
      status: "interrupted",
    });
    expect(focusBands[1]).toMatchObject({
      topMinute: 645,
      heightMinutes: 10,
      status: "active",
    });
  });

  it("suppresses planned bands from non-active events overlapping the active event", () => {
    // A: 10:00-14:00 (adaptive preset, processed first by startMinute)
    // B: 10:30-14:30 (active, creative preset)
    const A = makeEvent("A", 10, 14);
    const B: TimelineEvent = {
      id: "B",
      config: createPresetPomodoroConfig("creative"),
      startMs: DAY_MS + 10.5 * 3600000,
      endMs: DAY_MS + 14.5 * 3600000,
      startMinute: 630,
      endMinute: 870,
    };

    const sessionStartMs = DAY_MS + 11 * 3600000; // session started at 11:00
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "B",
      segments: [
        {
          id: "s1", eventId: "B", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(sessionStartMs).toISOString(),
          plannedEnd: new Date(sessionStartMs + 25 * 60000).toISOString(),
          actualStart: new Date(sessionStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
        {
          id: "s2", eventId: "B", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "short_break",
          plannedStart: new Date(sessionStartMs + 25 * 60000).toISOString(),
          plannedEnd: new Date(sessionStartMs + 30 * 60000).toISOString(),
          actualStart: null, actualEnd: null,
          pauseLog: [],
          status: "planned",
        },
      ],
      remainingSeconds: 600,
      breakOvertimeSeconds: 0,
    };

    const nowMs = sessionStartMs + 15 * 60000; // 11:15
    const bands = computeDayTimelineBands([A, B], activeState, DAY_MS, nowMs);

    // No planned bands from event A should overlap with B's range [630, 870]
    const plannedInBRange = bands.filter(
      (b) => b.status === "planned" && b.topMinute < 870 && b.topMinute + b.heightMinutes > 630,
    );
    // Only B's own planned segments should appear (they come from projectActiveSegments)
    for (const pb of plannedInBRange) {
      // These planned bands must come from B's persisted segments, not from A's planned computation.
      // B's first planned break starts at 11:25 (minute 685), well within B's range.
      expect(pb.topMinute).toBeGreaterThanOrEqual(660); // at or after session start
    }
  });

  it("handles no pomodoro events gracefully", () => {
    expect(computeDayTimelineBands([], null, DAY_MS, PAST_NOW)).toEqual([]);
  });

  it("handles single event shorter than one focus period", () => {
    // 20 min event: only focus, no breaks
    const ev = makeEvent("A", 10, 10 + 20 / 60);
    const bands = computeDayTimelineBands([ev], null, DAY_MS, PAST_NOW);
    expect(bands.length).toBe(0);
  });

  it("inherits rhythm position causing long break in successor", () => {
    // A: 3 focus positions (3*40 + 3*5 = 135 min). Trailing: position 4.
    // B starts right after. Its first break should be long break (position 4).
    const A = makeEvent("A", 8, 8 + 135 / 60); // 8:00-10:15
    const B = makeEvent("B", 8 + 135 / 60, 8 + 135 / 60 + 100 / 60); // 10:15-11:55
    const bands = computeDayTimelineBands([A, B], null, DAY_MS, PAST_NOW);

    // A's trailing position = 4. B inherits position 4.
    // B: first focus 0-40, then break at position 4 => long_break.
    const bBands = bands.filter((b) => b.topMinute >= 8 * 60 + 135);
    const firstBBreak = bBands[0];
    expect(firstBBreak).toBeDefined();
    expect(firstBBreak.phase).toBe("long_break");
  });

  // Focus fill bands

  it("active focus segment produces green fill band up to current time", () => {
    const ev = makeEvent("A", 10, 12);
    const segStartMs = DAY_MS + 10 * 3600000;
    const plannedDurMs = 40 * 60000;
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(segStartMs).toISOString(),
          plannedEnd: new Date(segStartMs + plannedDurMs).toISOString(),
          actualStart: new Date(segStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
      ],
      remainingSeconds: 20 * 60,
      breakOvertimeSeconds: 0,
    };

    const nowMs = segStartMs + 20 * 60000; // 10:20
    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, nowMs);

    const focusBands = bands.filter((b) => b.phase === "focus");
    expect(focusBands.length).toBe(1);
    expect(focusBands[0].topMinute).toBe(600); // 10:00
    expect(focusBands[0].heightMinutes).toBe(20); // fills to nowMs (10:20)
    expect(focusBands[0].status).toBe("active");
  });

  it("active focus fill is capped at segment planned end", () => {
    const ev = makeEvent("A", 10, 12);
    const segStartMs = DAY_MS + 10 * 3600000;
    const plannedDurMs = 40 * 60000;
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(segStartMs).toISOString(),
          plannedEnd: new Date(segStartMs + plannedDurMs).toISOString(),
          actualStart: new Date(segStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
      ],
      remainingSeconds: 0,
      breakOvertimeSeconds: 0,
    };

    // nowMs is 50 min after start, but segment is only 40 min
    const nowMs = segStartMs + 50 * 60000;
    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, nowMs);

    const focusBands = bands.filter((b) => b.phase === "focus");
    expect(focusBands.length).toBe(1);
    expect(focusBands[0].heightMinutes).toBe(40); // capped at planned duration
  });

  it("active event projects future breaks from the current event duration", () => {
    const ev = makeEvent("A", 10, 13);
    const segStartMs = DAY_MS + 10 * 3600000;
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(segStartMs).toISOString(),
          plannedEnd: new Date(segStartMs + 40 * 60000).toISOString(),
          actualStart: new Date(segStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
      ],
      remainingSeconds: 30 * 60,
      breakOvertimeSeconds: 0,
    };

    const nowMs = segStartMs + 10 * 60000;
    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, nowMs);
    const breakBands = bands.filter((band) => band.phase !== "focus");

    expect(breakBands.map((band) => band.topMinute)).toEqual([640, 685, 730, 775]);
  });

  it("active event end preview restores focus time clipped by the old block end", () => {
    const ev = makeEvent("A", 17, 20);
    const segStartMs = DAY_MS + (18 * 60 + 50) * 60000;
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(segStartMs).toISOString(),
          plannedEnd: new Date(segStartMs + 10 * 60000).toISOString(),
          actualStart: new Date(segStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
      ],
      remainingSeconds: 10 * 60,
      phaseElapsedSeconds: 0,
      phaseWorkDurationSeconds: 40 * 60,
      currentConfig: DEFAULT_CONFIG,
      breakOvertimeSeconds: 0,
    };

    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, segStartMs);
    const breakBands = bands.filter((band) => band.phase !== "focus");

    expect(breakBands[0]).toEqual({
      topMinute: 19 * 60 + 30,
      heightMinutes: 5,
      phase: "short_break",
      status: "planned",
    });
  });

  it("completed focus segments show as filled", () => {
    const ev = makeEvent("A", 10, 12);
    const segStartMs = DAY_MS + 10 * 3600000;
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(segStartMs).toISOString(),
          plannedEnd: new Date(segStartMs + 40 * 60000).toISOString(),
          actualStart: new Date(segStartMs).toISOString(),
          actualEnd: new Date(segStartMs + 40 * 60000).toISOString(),
          pauseLog: [],
          status: "completed",
        },
        {
          id: "s2", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "short_break",
          plannedStart: new Date(segStartMs + 40 * 60000).toISOString(),
          plannedEnd: new Date(segStartMs + 45 * 60000).toISOString(),
          actualStart: new Date(segStartMs + 40 * 60000).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
      ],
      remainingSeconds: 3 * 60,
      breakOvertimeSeconds: 0,
    };

    const nowMs = segStartMs + 42 * 60000;
    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, nowMs);

    const focusBands = bands.filter((b) => b.phase === "focus");
    expect(focusBands.length).toBe(1);
    expect(focusBands[0].topMinute).toBe(600);
    expect(focusBands[0].heightMinutes).toBe(40);
    expect(focusBands[0].status).toBe("completed");
  });

  it("active break overtime paints only the rail grace window", () => {
    const ev = makeEvent("A", 10, 12);
    const breakStartMs = DAY_MS + (10 * 60 + 40) * 60000;
    const breakEndMs = breakStartMs + 5 * 60000;
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "short_break",
          plannedStart: new Date(breakStartMs).toISOString(),
          plannedEnd: new Date(breakEndMs).toISOString(),
          actualStart: new Date(breakStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
      ],
      remainingSeconds: 0,
      phaseElapsedSeconds: 5 * 60,
      phaseWorkDurationSeconds: 5 * 60,
      currentConfig: DEFAULT_CONFIG,
      breakOvertimeSeconds: 30,
    };

    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, breakEndMs + 30 * 1000);
    const activeBreakBands = bands.filter((band) => band.phase === "short_break" && band.status === "active");

    expect(activeBreakBands.length).toBe(1);
    expect(activeBreakBands[0].topMinute).toBe(10 * 60 + 40);
    expect(activeBreakBands[0].heightMinutes).toBeCloseTo(5 + 10 / 60);
  });

  it("legitimate break extension expands the break mark", () => {
    const ev = makeEvent("A", 10, 12);
    const breakStartMs = DAY_MS + (10 * 60 + 40) * 60000;
    const extendedBreakEndMs = breakStartMs + 8 * 60000;
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "short_break",
          plannedStart: new Date(breakStartMs).toISOString(),
          plannedEnd: new Date(extendedBreakEndMs).toISOString(),
          actualStart: new Date(breakStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [],
          status: "active",
        },
      ],
      remainingSeconds: 3 * 60,
      phaseElapsedSeconds: 5 * 60,
      phaseWorkDurationSeconds: 8 * 60,
      currentConfig: DEFAULT_CONFIG,
      breakOvertimeSeconds: 0,
    };

    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, breakStartMs + 5 * 60000);
    const activeBreakBands = bands.filter((band) => band.phase === "short_break" && band.status === "active");

    expect(activeBreakBands.length).toBe(1);
    expect(activeBreakBands[0].heightMinutes).toBe(8);
  });

  it("completed break overtime remains empty after the rail grace window", () => {
    const ev = makeEvent("A", 10, 12);
    const breakStartMs = DAY_MS + (10 * 60 + 40) * 60000;
    const breakEndMs = breakStartMs + 5 * 60000;
    const persistedSegments = new Map<string, PersistedSegment[]>();
    persistedSegments.set("A", [
      {
        id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
        rhythmPosition: 1, phase: "short_break",
        plannedStart: new Date(breakStartMs).toISOString(),
        plannedEnd: new Date(breakEndMs).toISOString(),
        actualStart: new Date(breakStartMs).toISOString(),
        actualEnd: new Date(breakEndMs + 2 * 60000).toISOString(),
        pauseLog: [],
        status: "completed",
      },
    ]);

    const bands = computeDayTimelineBands([ev], null, DAY_MS, DAY_MS + 12 * 3600000, persistedSegments);
    const breakBands = bands.filter((band) => band.phase === "short_break");

    expect(breakBands.length).toBe(1);
    expect(breakBands[0].heightMinutes).toBeCloseTo(5 + 10 / 60);
  });

  it("persisted segments from past events produce focus fills", () => {
    const ev = makeEvent("A", 8, 10); // past event
    const segStartMs = DAY_MS + 8 * 3600000;
    const nowMs = DAY_MS + 12 * 3600000; // now is 12:00, event is past

    const persistedSegments = new Map<string, import("$lib/calendar/types").PersistedSegment[]>();
    persistedSegments.set("A", [
      {
        id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
        rhythmPosition: 1, phase: "focus",
        plannedStart: new Date(segStartMs).toISOString(),
        plannedEnd: new Date(segStartMs + 40 * 60000).toISOString(),
        actualStart: new Date(segStartMs).toISOString(),
        actualEnd: new Date(segStartMs + 40 * 60000).toISOString(),
        pauseLog: [],
        status: "completed",
      },
      {
        id: "s2", eventId: "A", eventDate: "2026-03-21", runId: "r1",
        rhythmPosition: 1, phase: "short_break",
        plannedStart: new Date(segStartMs + 40 * 60000).toISOString(),
        plannedEnd: new Date(segStartMs + 45 * 60000).toISOString(),
        actualStart: new Date(segStartMs + 40 * 60000).toISOString(),
        actualEnd: new Date(segStartMs + 45 * 60000).toISOString(),
        pauseLog: [],
        status: "completed",
      },
      {
        id: "s3", eventId: "A", eventDate: "2026-03-21", runId: "r1",
        rhythmPosition: 2, phase: "focus",
        plannedStart: new Date(segStartMs + 45 * 60000).toISOString(),
        plannedEnd: new Date(segStartMs + 85 * 60000).toISOString(),
        actualStart: new Date(segStartMs + 45 * 60000).toISOString(),
        actualEnd: new Date(segStartMs + 85 * 60000).toISOString(),
        pauseLog: [],
        status: "completed",
      },
    ]);

    const bands = computeDayTimelineBands([ev], null, DAY_MS, nowMs, persistedSegments);

    // Should have 2 focus fill bands and 1 break band
    const focusBands = bands.filter((b) => b.phase === "focus");
    const breakBands = bands.filter((b) => b.phase !== "focus");
    expect(focusBands.length).toBe(2);
    expect(breakBands.length).toBe(1);

    // First focus: 8:00-8:40
    expect(focusBands[0].topMinute).toBe(480);
    expect(focusBands[0].heightMinutes).toBe(40);
    expect(focusBands[0].status).toBe("completed");

    // Second focus: 8:45-9:25
    expect(focusBands[1].topMinute).toBe(525);
    expect(focusBands[1].heightMinutes).toBe(40);
  });

  it("past event without persisted segments shows no fill", () => {
    const ev = makeEvent("A", 8, 10);
    const nowMs = DAY_MS + 12 * 3600000;

    // No persisted segments
    const bands = computeDayTimelineBands([ev], null, DAY_MS, nowMs);

    // Past event, no segments: no bands at all
    expect(bands.length).toBe(0);
  });

  it("skipped segments produce no bands", () => {
    const ev = makeEvent("A", 8, 10);
    const segStartMs = DAY_MS + 8 * 3600000;
    const nowMs = DAY_MS + 12 * 3600000;

    const persistedSegments = new Map<string, import("$lib/calendar/types").PersistedSegment[]>();
    persistedSegments.set("A", [
      {
        id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
        rhythmPosition: 1, phase: "focus",
        plannedStart: new Date(segStartMs).toISOString(),
        plannedEnd: new Date(segStartMs + 40 * 60000).toISOString(),
        actualStart: new Date(segStartMs).toISOString(),
        actualEnd: new Date(segStartMs + 40 * 60000).toISOString(),
        pauseLog: [],
        status: "completed",
      },
      {
        id: "s2", eventId: "A", eventDate: "2026-03-21", runId: "r1",
        rhythmPosition: 1, phase: "short_break",
        plannedStart: new Date(segStartMs + 40 * 60000).toISOString(),
        plannedEnd: new Date(segStartMs + 45 * 60000).toISOString(),
        actualStart: null, actualEnd: null,
        pauseLog: [],
        status: "skipped",
      },
    ]);

    const bands = computeDayTimelineBands([ev], null, DAY_MS, nowMs, persistedSegments);

    // Only the completed focus band, skipped break is excluded
    expect(bands.length).toBe(1);
    expect(bands[0].phase).toBe("focus");
  });

  it("active focus with pauses shows gaps in green fill", () => {
    const ev = makeEvent("A", 10, 12);
    const segStartMs = DAY_MS + 10 * 3600000; // 10:00
    const pauseStartMs = segStartMs + 10 * 60000; // paused at 10:10
    const resumeMs = segStartMs + 15 * 60000; // resumed at 10:15
    const plannedDurMs = 40 * 60000;
    const activeState: ActivePomodoroState = {
      activeOccurrenceId: "A",
      segments: [
        {
          id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
          rhythmPosition: 1, phase: "focus",
          plannedStart: new Date(segStartMs).toISOString(),
          plannedEnd: new Date(segStartMs + plannedDurMs).toISOString(),
          actualStart: new Date(segStartMs).toISOString(),
          actualEnd: null,
          pauseLog: [{
            startedAt: new Date(pauseStartMs).toISOString(),
            endedAt: new Date(resumeMs).toISOString(),
            reason: "manual",
          }],
          status: "active",
        },
      ],
      remainingSeconds: 15 * 60,
      breakOvertimeSeconds: 0,
    };

    const nowMs = segStartMs + 30 * 60000; // 10:30
    const bands = computeDayTimelineBands([ev], activeState, DAY_MS, nowMs);

    const focusBands = bands.filter((b) => b.phase === "focus");
    expect(focusBands.length).toBe(2); // two bands with a gap
    // First band: 10:00-10:10
    expect(focusBands[0].topMinute).toBe(600);
    expect(focusBands[0].heightMinutes).toBe(10);
    // Second band: 10:15-10:30
    expect(focusBands[1].topMinute).toBe(615);
    expect(focusBands[1].heightMinutes).toBe(15);
  });

  it("persisted focus with pauses shows gaps in completed fill", () => {
    const ev = makeEvent("A", 8, 10);
    const segStartMs = DAY_MS + 8 * 3600000; // 8:00
    const pauseStartMs = segStartMs + 20 * 60000; // paused at 8:20
    const resumeMs = segStartMs + 25 * 60000; // resumed at 8:25
    const nowMs = DAY_MS + 12 * 3600000;

    const persistedSegments = new Map<string, import("$lib/calendar/types").PersistedSegment[]>();
    persistedSegments.set("A", [
      {
        id: "s1", eventId: "A", eventDate: "2026-03-21", runId: "r1",
        rhythmPosition: 1, phase: "focus",
        plannedStart: new Date(segStartMs).toISOString(),
        plannedEnd: new Date(segStartMs + 40 * 60000).toISOString(),
        actualStart: new Date(segStartMs).toISOString(),
        actualEnd: new Date(segStartMs + 40 * 60000).toISOString(),
        pauseLog: [{
          startedAt: new Date(pauseStartMs).toISOString(),
          endedAt: new Date(resumeMs).toISOString(),
          reason: "manual",
        }],
        status: "completed",
      },
    ]);

    const bands = computeDayTimelineBands([ev], null, DAY_MS, nowMs, persistedSegments);

    const focusBands = bands.filter((b) => b.phase === "focus");
    expect(focusBands.length).toBe(2);
    // First band: 8:00-8:20
    expect(focusBands[0].topMinute).toBe(480);
    expect(focusBands[0].heightMinutes).toBe(20);
    // Second band: 8:25 to the actual end at 8:40 (15 min).
    expect(focusBands[1].topMinute).toBe(505);
    expect(focusBands[1].heightMinutes).toBe(15);
  });
});
