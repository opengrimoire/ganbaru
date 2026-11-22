import {
  failAsyncRequest,
  resolveAsyncRequest,
  startAsyncRequest,
  type AsyncResource,
} from "$lib/utils/async-resource";

/** One `.ics` entry the import command read: a basename and its UTF-8 body. */
export interface CalendarImportEntry {
  readonly name: string;
  readonly contents: string;
}

/** How the import names its entries to the backend: one picked file, or one entry of a picked `.zip`. */
export type CalendarImportSourceKind = "import-file" | "import-zip-entry";

/** Counters folded over every entry an import has committed so far. */
export interface CalendarImportTotals {
  readonly added: number;
  readonly updated: number;
  readonly skippedOlder: number;
  /** Calendars that received events, one per committed entry. */
  readonly calendars: number;
  readonly warnings: readonly string[];
}

/** What one entry added to the totals. */
export type CalendarImportEntryResult = CalendarImportTotals;

/**
 * The lifecycle of one Calendar import. `importing` names the entry in flight; `done` and `failed` keep the totals
 * of the entries committed before the import ended, so a failure after earlier entries still reports what landed.
 */
export type CalendarImportPhase =
  | { readonly phase: "idle" }
  | { readonly phase: "picking" }
  | {
    readonly phase: "importing";
    /** One-based position of the entry in flight. */
    readonly index: number;
    readonly total: number;
    readonly label: string;
    readonly totals: CalendarImportTotals;
  }
  | { readonly phase: "done"; readonly entryCount: number; readonly totals: CalendarImportTotals }
  | { readonly phase: "failed"; readonly totals: CalendarImportTotals; readonly error: unknown };

/** What the import needs from the picker and the Calendar stores. */
export interface CalendarImportDependencies {
  /** Opens the picker and reads its entries, or resolves `null` when the user cancels. */
  pick(): Promise<readonly CalendarImportEntry[] | null>;
  /** Parses and commits one entry. */
  importEntry(entry: CalendarImportEntry, sourceKind: CalendarImportSourceKind): Promise<CalendarImportEntryResult>;
  /** Receives every phase the import moves through, ending with the returned one. */
  onPhase(phase: CalendarImportPhase): void;
}

/** Totals before any entry is committed. */
export const EMPTY_CALENDAR_IMPORT_TOTALS: CalendarImportTotals = {
  added: 0,
  updated: 0,
  skippedOlder: 0,
  calendars: 0,
  warnings: [],
};

/**
 * Totals after folding in one entry.
 *
 * @param totals - The totals so far.
 * @param entry - What the entry added.
 */
export function addCalendarImportTotals(
  totals: CalendarImportTotals,
  entry: CalendarImportEntryResult,
): CalendarImportTotals {
  return {
    added: totals.added + entry.added,
    updated: totals.updated + entry.updated,
    skippedOlder: totals.skippedOlder + entry.skippedOlder,
    calendars: totals.calendars + entry.calendars,
    warnings: entry.warnings.length > 0 ? [...totals.warnings, ...entry.warnings] : totals.warnings,
  };
}

/**
 * Whether an import is running, so a second one cannot start.
 *
 * @param phase - The current phase.
 */
export function isCalendarImportBusy(phase: CalendarImportPhase): boolean {
  return phase.phase === "picking" || phase.phase === "importing";
}

/**
 * Runs one import: picks the entries, commits them in order, and ends in `idle` when the picker is cancelled,
 * `done` after every entry, or `failed` at the first entry that throws.
 *
 * @param deps - The picker, the entry import, and the phase listener.
 * @returns The final phase.
 */
export async function runCalendarImport(deps: CalendarImportDependencies): Promise<CalendarImportPhase> {
  const settle = (phase: CalendarImportPhase): CalendarImportPhase => {
    deps.onPhase(phase);
    return phase;
  };
  deps.onPhase({ phase: "picking" });
  let totals = EMPTY_CALENDAR_IMPORT_TOTALS;
  try {
    const entries = await deps.pick();
    if (!entries) return settle({ phase: "idle" });
    const sourceKind: CalendarImportSourceKind = entries.length > 1 ? "import-zip-entry" : "import-file";
    for (const [position, entry] of entries.entries()) {
      deps.onPhase({ phase: "importing", index: position + 1, total: entries.length, label: entry.name, totals });
      totals = addCalendarImportTotals(totals, await deps.importEntry(entry, sourceKind));
    }
    return settle({ phase: "done", entryCount: entries.length, totals });
  } catch (error) {
    return settle({ phase: "failed", totals, error });
  }
}

/** Event counts per calendar id. */
export type CalendarEventCounts = Readonly<Record<string, number>>;

/** Reads and writes the counts resource a component renders. */
export interface CalendarEventCountsDependencies {
  read(): AsyncResource<CalendarEventCounts>;
  write(resource: AsyncResource<CalendarEventCounts>): void;
  /** Ids of the calendars to count, read when a refresh starts. */
  calendarIds(): readonly string[];
  countEvents(calendarId: string): Promise<number>;
}

/**
 * Reloads the event counts of every calendar. Each refresh supersedes the one in flight, so counts read before a
 * calendar was added or removed never replace newer counts.
 *
 * @param deps - The counts resource and the count query.
 */
export async function refreshCalendarEventCounts(deps: CalendarEventCountsDependencies): Promise<void> {
  const { resource, token } = startAsyncRequest(deps.read());
  deps.write(resource);
  try {
    const next: Record<string, number> = {};
    for (const calendarId of deps.calendarIds()) {
      next[calendarId] = await deps.countEvents(calendarId);
    }
    deps.write(resolveAsyncRequest(deps.read(), token, next));
  } catch (error) {
    deps.write(failAsyncRequest(deps.read(), token, error instanceof Error ? error.message : String(error)));
    throw error;
  }
}
