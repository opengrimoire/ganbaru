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
/** Pending visibility write per calendar, so toggles of one calendar reach the database in click order. */
const visibilityWrites = new Map<string, Promise<unknown>>();
/** Latest toggle number per calendar with writes in flight. */
const visibilityToggles = new Map<string, number>();
/** Last visibility the database confirmed per calendar with writes in flight. */
const storedVisibility = new Map<string, boolean>();

/** Shows `visible` for calendar `id`. */
function setVisible(id: string, visible: boolean): void {
  calendars = calendars.map((c) => (c.id === id ? { ...c, visible } : c));
}

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

    /**
     * Flips a calendar's visibility at once and writes it after any earlier toggle of the same calendar, so every
     * click counts and the stored value ends as the last one shown. A failed write restores the last stored value
     * unless a newer toggle has already replaced it.
     *
     * @param id Calendar whose visibility flips.
     */
    async toggleVisibility(id: string) {
      const calendar = calendars.find((c) => c.id === id);
      if (!calendar) return;
      const visible = !calendar.visible;
      const toggle = (visibilityToggles.get(id) ?? 0) + 1;
      visibilityToggles.set(id, toggle);
      if (!storedVisibility.has(id)) storedVisibility.set(id, calendar.visible);
      setVisible(id, visible);
      const previous = visibilityWrites.get(id) ?? Promise.resolve();
      const write = previous.catch(() => undefined).then(() => invoke("calendar_set_visibility", {
        dbUrl: dbUrl(),
        id,
        visible,
        updatedAt: nowIso(),
      }));
      visibilityWrites.set(id, write);
      try {
        await write;
        storedVisibility.set(id, visible);
      } catch (error: unknown) {
        if (visibilityToggles.get(id) === toggle) setVisible(id, storedVisibility.get(id) ?? !visible);
        throw error;
      } finally {
        if (visibilityToggles.get(id) === toggle) {
          visibilityWrites.delete(id);
          visibilityToggles.delete(id);
          storedVisibility.delete(id);
        }
      }
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
