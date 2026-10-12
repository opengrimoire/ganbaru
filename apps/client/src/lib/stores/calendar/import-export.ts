import { invoke } from "@tauri-apps/api/core";
import type { Calendar, CalendarEvent } from "$lib/calendar/types";
import type { IcsImportSummary, IcsPreservationPayload } from "$lib/calendar/ics/types";
import { calendarDisplayName } from "$lib/calendar/display";
import { dbUrl } from "$lib/api/db";
import {
  buildBulkImportPayload,
  type CalendarBulkImportResult,
  type CalendarImportSourceKind,
} from "./bulk-import";
import { localTimezone, nowIso } from "./event-payloads";
import { hydrateFullEvent } from "./event-hydration";
import { parseCalendarExportSnapshot } from "./export-snapshot";

export interface CalendarBulkImportOptions {
  refreshWindow?: boolean;
  preservation?: IcsPreservationPayload | null;
  sourceName?: string;
  sourceKind?: CalendarImportSourceKind;
}

export interface CalendarBulkImportStoreResult {
  summary: IcsImportSummary;
  applied: boolean;
  added: number;
  refreshWindow: boolean;
}

export async function bulkImportCalendarEvents(
  events: CalendarEvent[],
  targetCalendarId: string,
  options: CalendarBulkImportOptions = {},
): Promise<CalendarBulkImportStoreResult> {
  const now = nowIso();
  const fallbackZone = localTimezone();
  const payload = buildBulkImportPayload(
    events,
    targetCalendarId,
    now,
    fallbackZone,
    () => crypto.randomUUID(),
    options.preservation ?? null,
    options.sourceName ?? "",
    options.sourceKind ?? "import-file",
  );
  const result = await invoke<CalendarBulkImportResult>("calendar_bulk_import", {
    dbUrl: dbUrl(),
    payload,
  });

  return {
    summary: {
      added: result.added,
      updated: result.updated,
      skippedOlder: result.skippedOlder,
      warnings: result.warnings,
    },
    applied: result.applied.length > 0,
    added: result.added,
    refreshWindow: options.refreshWindow ?? true,
  };
}

export async function exportCalendarAsIcs(
  calendar: Calendar,
): Promise<string> {
  const renderZone = localTimezone();
  const response = await invoke<unknown>("calendar_load_export_snapshot", {
    dbUrl: dbUrl(),
    calendarId: calendar.id,
  });
  const snapshot = parseCalendarExportSnapshot(response, calendar.id);
  const calendarEvents = snapshot.events.map((rows) => {
    const event = hydrateFullEvent(rows, renderZone);
    if (!event) throw new Error("Missing Calendar export event");
    return event;
  });
  const currentCalendar = {
    ...calendar,
    ...snapshot.calendar,
    sourceUrl: snapshot.calendar.source_url ?? undefined,
  };
  if (snapshot.metadata.mixed_methods) {
    console.warn(
      "iCalendar export used METHOD:PUBLISH because this calendar contains mixed preserved METHOD values.",
    );
  }
  const { serializeCalendarToIcs } = await import("$lib/calendar/ics/serializer");
  return serializeCalendarToIcs(
    { ...currentCalendar, name: calendarDisplayName(currentCalendar) },
    calendarEvents,
    renderZone,
    snapshot.timezones,
    snapshot.passthrough_components,
    snapshot.metadata.method ?? undefined,
  );
}
