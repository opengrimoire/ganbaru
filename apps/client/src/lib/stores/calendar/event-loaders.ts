import { invoke } from "@tauri-apps/api/core";
import type { CalendarEvent } from "$lib/calendar/types";
import { dbUrl } from "$lib/api/db";
import { localTimezone } from "./event-payloads";
import {
  hydrateFullEvent,
  hydratePanelEvent,
  type CalendarFullEventRows,
  type CalendarPanelEventRows,
} from "./event-hydration";

const PANEL_EVENT_CACHE_LIMIT = 64;
const panelEventCache = new Map<string, Promise<CalendarEvent | undefined>>();

function rememberPanelEvent(id: string, promise: Promise<CalendarEvent | undefined>): void {
  panelEventCache.set(id, promise);
  while (panelEventCache.size > PANEL_EVENT_CACHE_LIMIT) {
    const first = panelEventCache.keys().next().value;
    if (typeof first !== "string") break;
    panelEventCache.delete(first);
  }
}

export function clearPanelEventCache(): void {
  panelEventCache.clear();
}

export async function loadPanelEvent(id: string): Promise<CalendarEvent | undefined> {
  const cached = panelEventCache.get(id);
  if (cached) return cached;

  const promise = (async () => {
    const renderZone = localTimezone();
    const rows = await invoke<CalendarPanelEventRows>("calendar_load_panel_event", {
      dbUrl: dbUrl(),
      id,
    });
    return hydratePanelEvent(rows, renderZone);
  })().catch((error: unknown) => {
    if (panelEventCache.get(id) === promise) panelEventCache.delete(id);
    throw error;
  });

  rememberPanelEvent(id, promise);
  return promise;
}

export function prefetchPanelEvent(id: string): void {
  void loadPanelEvent(id).catch((error) => {
    console.warn("[calendar] panel event prefetch failed:", error);
  });
}

export async function loadFullEvent(id: string): Promise<CalendarEvent | undefined> {
  const renderZone = localTimezone();
  const rows = await invoke<CalendarFullEventRows>("calendar_load_full_event", {
    dbUrl: dbUrl(),
    id,
  });
  return hydrateFullEvent(rows, renderZone);
}
