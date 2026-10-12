import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import { createPresetPomodoroConfig } from "$lib/pomodoro/rhythm";
import {
  mapPomodoroSegmentRows,
  parsePomodoroSegmentRows,
  pomodoroSegmentSnapshotKey,
  visiblePomodoroEventIds,
  type DbPomodoroSegmentRow,
} from "./pomodoro-rail-segments";

const pomodoroConfig = createPresetPomodoroConfig("deep");

function event(id: string, pomodoro = true): CalendarEvent {
  return {
    id,
    title: id,
    start: "2026-05-04 09:00",
    end: "2026-05-04 10:00",
    timezone: "UTC",
    calendarId: "local",
    pomodoroConfig: pomodoro ? pomodoroConfig : undefined,
  };
}

function row(eventId: string, eventDate: string): DbPomodoroSegmentRow {
  return {
    id: `${eventId}-${eventDate}`,
    event_id: eventId,
    event_date: eventDate,
    run_id: "run-1",
    rhythm_position: 1,
    phase: "focus",
    planned_start: `${eventDate}T09:00:00Z`,
    planned_end: `${eventDate}T09:25:00Z`,
    actual_start: `${eventDate}T09:00:00Z`,
    actual_end: `${eventDate}T09:25:00Z`,
    pauses: [],
    status: "completed",
  };
}

describe("visiblePomodoroEventIds", () => {
  it("returns sorted visible pomodoro ids only", () => {
    expect(visiblePomodoroEventIds([
      event("b"),
      event("a", false),
      event("a::2026-05-04"),
      event("b"),
    ])).toEqual(["a::2026-05-04", "b"]);
  });
});

describe("pomodoroSegmentSnapshotKey", () => {
  it("is stable for the same segment version and id set", () => {
    expect(pomodoroSegmentSnapshotKey(3, ["b", "a"])).toBe("3|a,b");
  });
});

describe("mapPomodoroSegmentRows", () => {
  it("uses the native occurrence identity when the device day differs", () => {
    const mapped = mapPomodoroSegmentRows(
      [row("event-root::2026-05-04", "2026-05-03")],
      ["event-root::2026-05-04"],
    );

    expect(mapped.get("event-root::2026-05-04")?.[0]).toMatchObject({
      eventId: "event-root::2026-05-04",
      eventDate: "2026-05-03",
      phase: "focus",
    });
  });

  it("keeps anchor history separate and excludes invisible occurrences", () => {
    const mapped = mapPomodoroSegmentRows(
      [row("event-root", "2026-05-03"), row("event-root::2026-05-04", "2026-05-03"),
        row("event-root::2026-05-05", "2026-05-04")],
      ["event-root", "event-root::2026-05-04"],
    );
    expect(mapped.size).toBe(2);
    expect(mapped.get("event-root")).toHaveLength(1);
    expect(mapped.get("event-root::2026-05-04")).toHaveLength(1);
  });
});

describe("native Focus history validation", () => {
  it("preserves native occurrence identity, device date, and nullable pause endings", () => {
    const segment = row("source::2026-05-04", "2026-05-03");
    segment.pauses = [{ startedAt: "2026-05-03T09:10:00Z", endedAt: null, reason: "manual" }];
    expect(parsePomodoroSegmentRows([segment], [segment.event_id])).toEqual([segment]);
  });

  it.each([
    { phase: "work" }, { status: "failed" }, { rhythm_position: 1.5 },
    { event_date: "2026-02-30" }, { planned_start: "2026-05-04 09:00" },
    { pauses: [{ startedAt: "2026-05-04T09:10:00Z", endedAt: null, reason: "unknown" }] },
  ])("rejects malformed native evidence %j", (invalid) => {
    expect(() => parsePomodoroSegmentRows([{ ...row("source", "2026-05-04"), ...invalid }], ["source"]))
      .toThrow("Invalid native Focus history row");
  });

  it("rejects foreign occurrences, duplicate segments, and invalid root responses", () => {
    const segment = row("source", "2026-05-04");
    expect(() => parsePomodoroSegmentRows([segment], ["other"])).toThrow();
    expect(() => parsePomodoroSegmentRows([segment, segment], ["source"])).toThrow();
    expect(() => parsePomodoroSegmentRows({ rows: [] }, ["source"])).toThrow();
  });

  it("enforces aggregate row and pause budgets before rendering", () => {
    const segment = row("source", "2026-05-04");
    expect(() => parsePomodoroSegmentRows(Array.from({ length: 10_001 }, () => segment), ["source"]))
      .toThrow("size");
    const pauses = Array.from({ length: 5001 }, () => ({
      startedAt: "2026-05-04T09:10:00Z", endedAt: null, reason: "manual" as const,
    }));
    expect(() => parsePomodoroSegmentRows([
      { ...segment, id: "one", pauses }, { ...segment, id: "two", pauses },
    ], ["source"])).toThrow("pause limit");
  });

  it("accepts a maximum-length native qualified occurrence but enforces UTF-8 bytes", () => {
    const id = `${"a".repeat(1024)}::2026-05-04`;
    const segment = { ...row(id, "2026-05-04"), id: "segment" };
    expect(parsePomodoroSegmentRows([segment], [id])).toEqual([segment]);
    const oversized = "é".repeat(519);
    expect(() => parsePomodoroSegmentRows([{ ...segment, event_id: oversized }], [oversized])).toThrow();
  });
});
