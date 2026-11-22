import { describe, expect, it } from "vitest";

import { idleAsyncResource, type AsyncResource } from "$lib/utils/async-resource";
import {
  EMPTY_CALENDAR_IMPORT_TOTALS,
  isCalendarImportBusy,
  refreshCalendarEventCounts,
  runCalendarImport,
  type CalendarEventCounts,
  type CalendarImportEntry,
  type CalendarImportPhase,
} from "./calendar-import-flow";

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

const entries: CalendarImportEntry[] = [
  { name: "work.ics", contents: "A" },
  { name: "home.ics", contents: "B" },
];

function entryResult(added: number, warnings: string[] = []) {
  return { ...EMPTY_CALENDAR_IMPORT_TOTALS, added, calendars: 1, warnings };
}

describe("runCalendarImport", () => {
  it("returns to idle without importing when the picker is cancelled", async () => {
    const phases: CalendarImportPhase[] = [];
    const outcome = await runCalendarImport({
      pick: async () => null,
      importEntry: async () => entryResult(1),
      onPhase: (phase) => phases.push(phase),
    });
    expect(outcome).toEqual({ phase: "idle" });
    expect(phases.map((phase) => phase.phase)).toEqual(["picking", "idle"]);
  });

  it("names each entry in flight and folds every entry into the done totals", async () => {
    const phases: CalendarImportPhase[] = [];
    const kinds: string[] = [];
    const outcome = await runCalendarImport({
      pick: async () => entries,
      importEntry: async (entry, kind) => {
        kinds.push(kind);
        return entryResult(entry.name === "work.ics" ? 2 : 3, [`${entry.name} warning`]);
      },
      onPhase: (phase) => phases.push(phase),
    });
    expect(kinds).toEqual(["import-zip-entry", "import-zip-entry"]);
    expect(phases.filter((phase) => phase.phase === "importing")).toMatchObject([
      { index: 1, total: 2, label: "work.ics" },
      { index: 2, total: 2, label: "home.ics" },
    ]);
    expect(outcome).toEqual({
      phase: "done",
      entryCount: 2,
      totals: { added: 5, updated: 0, skippedOlder: 0, calendars: 2, warnings: ["work.ics warning", "home.ics warning"] },
    });
  });

  it("imports a single picked file as a file rather than a zip entry", async () => {
    const kinds: string[] = [];
    await runCalendarImport({
      pick: async () => [entries[0]],
      importEntry: async (_entry, kind) => {
        kinds.push(kind);
        return entryResult(1);
      },
      onPhase: () => {},
    });
    expect(kinds).toEqual(["import-file"]);
  });

  it("keeps the totals of entries committed before a later entry fails", async () => {
    const failure = new Error("parse failed");
    const outcome = await runCalendarImport({
      pick: async () => entries,
      importEntry: async (entry) => {
        if (entry.name === "home.ics") throw failure;
        return entryResult(4);
      },
      onPhase: () => {},
    });
    expect(outcome).toMatchObject({ phase: "failed", error: failure, totals: { added: 4, calendars: 1 } });
  });

  it("settles as failed when the picker itself fails", async () => {
    const phases: CalendarImportPhase[] = [];
    const outcome = await runCalendarImport({
      pick: async () => {
        throw new Error("picker failed");
      },
      importEntry: async () => entryResult(1),
      onPhase: (phase) => phases.push(phase),
    });
    expect(outcome).toMatchObject({ phase: "failed", totals: EMPTY_CALENDAR_IMPORT_TOTALS });
    expect(isCalendarImportBusy(phases.at(-1) ?? { phase: "idle" })).toBe(false);
  });

  it("reports busy only while picking or importing", () => {
    expect(isCalendarImportBusy({ phase: "picking" })).toBe(true);
    expect(
      isCalendarImportBusy({ phase: "importing", index: 1, total: 1, label: "a", totals: EMPTY_CALENDAR_IMPORT_TOTALS }),
    ).toBe(true);
    expect(isCalendarImportBusy({ phase: "idle" })).toBe(false);
    expect(isCalendarImportBusy({ phase: "done", entryCount: 0, totals: EMPTY_CALENDAR_IMPORT_TOTALS })).toBe(false);
  });
});

describe("refreshCalendarEventCounts", () => {
  function countsHarness(ids: { current: string[] }, countEvents: (id: string) => Promise<number>) {
    let resource: AsyncResource<CalendarEventCounts> = idleAsyncResource({});
    return {
      deps: {
        read: () => resource,
        write: (next: AsyncResource<CalendarEventCounts>) => {
          resource = next;
        },
        calendarIds: () => ids.current,
        countEvents,
      },
      current: () => resource,
    };
  }

  it("keeps newer counts when an older refresh settles after them", async () => {
    const ids = { current: ["work", "home"] };
    const slow = deferred<number>();
    let calls = 0;
    const harness = countsHarness(ids, async (id) => {
      calls += 1;
      if (calls === 1) return slow.promise;
      return id === "work" ? 7 : 0;
    });

    const older = refreshCalendarEventCounts(harness.deps);
    ids.current = ["work"];
    await refreshCalendarEventCounts(harness.deps);
    expect(harness.current()).toEqual({ status: "idle", data: { work: 7 } });

    slow.resolve(99);
    await older;
    expect(harness.current()).toEqual({ status: "idle", data: { work: 7 } });
  });

  it("keeps the previous counts and records the error when counting fails", async () => {
    const ids = { current: ["work"] };
    let fail = false;
    const harness = countsHarness(ids, async () => {
      if (fail) throw new Error("count failed");
      return 3;
    });
    await refreshCalendarEventCounts(harness.deps);
    fail = true;
    await expect(refreshCalendarEventCounts(harness.deps)).rejects.toThrow("count failed");
    expect(harness.current().data).toEqual({ work: 3 });
    expect(harness.current().status).toBe("failed");
  });
});
