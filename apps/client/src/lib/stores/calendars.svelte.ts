import { invoke } from "@tauri-apps/api/core";
import { dbUrl, ensureDbUrl } from "$lib/api/db";
import type { Calendar } from "$lib/calendar/types";

interface DbCalendar {
  id: string;
  name: string;
  color: string;
  source: string;
  visible: number;
  read_only: number;
  source_url: string | null;
  last_synced: string | null;
  created_at: string;
  updated_at: string;
}

function mapRow(row: DbCalendar): Calendar {
  return {
    id: row.id,
    name: row.name,
    color: row.color,
    source: row.source,
    visible: row.visible === 1,
    readOnly: row.read_only === 1,
    sourceUrl: row.source_url ?? undefined,
    lastSynced: row.last_synced ?? undefined,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function nowIso(): string {
  return new Date().toISOString();
}

let calendars = $state<Calendar[]>([]);

export function getCalendars() {
  return {
    get list(): Calendar[] {
      return calendars;
    },

    get visibleIds(): Set<string> {
      return new Set(calendars.filter((c) => c.visible).map((c) => c.id));
    },

    async load() {
      const rows = await invoke<DbCalendar[]>("calendar_list_calendars", { dbUrl: await ensureDbUrl() });
      calendars = rows.map(mapRow);
    },

    async toggleVisibility(id: string) {
      const calendar = calendars.find((c) => c.id === id);
      if (!calendar) return;
      await invoke("calendar_set_visibility", {
        dbUrl: dbUrl(),
        id,
        visible: !calendar.visible,
        updatedAt: nowIso(),
      });
      calendars = calendars.map((c) =>
        c.id === id ? { ...c, visible: !c.visible } : c,
      );
    },

    async add(calendar: Omit<Calendar, "id" | "visible" | "readOnly"> & { id?: string; visible?: boolean; readOnly?: boolean }): Promise<Calendar> {
      const id = calendar.id ?? crypto.randomUUID();
      const now = nowIso();
      await invoke("calendar_add_calendar", {
        dbUrl: dbUrl(),
        calendar: {
          id,
          name: calendar.name,
          color: calendar.color,
          source: calendar.source,
          visible: calendar.visible ?? true,
          readOnly: calendar.readOnly ?? false,
          sourceUrl: calendar.sourceUrl ?? null,
          createdAt: now,
          updatedAt: now,
        },
      });
      const entry: Calendar = {
        id,
        name: calendar.name,
        color: calendar.color,
        source: calendar.source,
        visible: calendar.visible ?? true,
        readOnly: calendar.readOnly ?? false,
        sourceUrl: calendar.sourceUrl,
        lastSynced: calendar.lastSynced,
        createdAt: now,
        updatedAt: now,
      };
      calendars = [...calendars, entry];
      return entry;
    },

    async remove(id: string) {
      await invoke("calendar_remove_calendar", { dbUrl: dbUrl(), id });
      calendars = calendars.filter((c) => c.id !== id);
    },

    /**
     * Find an existing imported calendar that originated from `filename`, or
     * create a new one. Used by the .ics import flow so that re-importing the
     * same file keeps a single calendar grouping (whose deletion cascades to
     * every event from that file).
     */
    async findOrCreateImported(filename: string): Promise<Calendar> {
      const row = await invoke<DbCalendar | null>("calendar_find_imported_calendar", {
        dbUrl: dbUrl(),
        filename,
      });
      if (row) {
        const calendar = mapRow(row);
        if (!calendars.some((c) => c.id === calendar.id)) {
          calendars = [...calendars, calendar];
        }
        return calendar;
      }
      const baseName = filename.replace(/\.ics$/i, "");
      return this.add({
        name: baseName,
        color: "",
        source: "ics",
        sourceUrl: filename,
      });
    },

    /**
     * Count events in a calendar without loading their full rows (used by the
     * settings panel listing).
     */
    async countEvents(calendarId: string): Promise<number> {
      return invoke<number>("calendar_count_events", { dbUrl: dbUrl(), calendarId });
    },

    isReadOnly(calendarId: string): boolean {
      return calendars.find((c) => c.id === calendarId)?.readOnly ?? false;
    },
  };
}
