import { getConfigKey, setConfigKey } from "$lib/vault/config";

/** How a database view tab presents itself in the view switcher. */
export const NOTES_DATABASE_VIEW_TAB_DISPLAYS = ["text_and_icon", "text", "icon"] as const;
export type NotesDatabaseViewTabDisplay = typeof NOTES_DATABASE_VIEW_TAB_DISPLAYS[number];

export const DEFAULT_NOTES_DATABASE_VIEW_TAB_DISPLAY: NotesDatabaseViewTabDisplay = "text_and_icon";
const CONFIG_KEY = "notes.databaseViewTabDisplay";

function isNotesDatabaseViewTabDisplay(value: unknown): value is NotesDatabaseViewTabDisplay {
  return typeof value === "string" && (NOTES_DATABASE_VIEW_TAB_DISPLAYS as readonly string[]).includes(value);
}

/**
 * Read one view's tab display from the stored per-view map, falling back to text and icon for missing or invalid entries.
 *
 * @param stored Untrusted value of the per-view map from vault config.
 * @param viewId Saved database view.
 */
export function readNotesDatabaseViewTabDisplay(stored: unknown, viewId: string): NotesDatabaseViewTabDisplay {
  if (!stored || typeof stored !== "object" || Array.isArray(stored)) return DEFAULT_NOTES_DATABASE_VIEW_TAB_DISPLAY;
  const value: unknown = (stored as Record<string, unknown>)[viewId];
  return isNotesDatabaseViewTabDisplay(value) ? value : DEFAULT_NOTES_DATABASE_VIEW_TAB_DISPLAY;
}

/**
 * Return the per-view map with one view's display replaced, keeping only valid entries so the map stays small.
 * The default display removes the entry.
 *
 * @param stored Untrusted value of the per-view map from vault config.
 * @param viewId Saved database view.
 * @param display Display to store, or null to forget the view.
 */
export function withNotesDatabaseViewTabDisplay(
  stored: unknown,
  viewId: string,
  display: NotesDatabaseViewTabDisplay | null,
): Record<string, NotesDatabaseViewTabDisplay> {
  const next: Record<string, NotesDatabaseViewTabDisplay> = {};
  if (stored && typeof stored === "object" && !Array.isArray(stored)) {
    for (const [id, value] of Object.entries(stored)) if (id !== viewId && isNotesDatabaseViewTabDisplay(value)) next[id] = value;
  }
  if (display && display !== DEFAULT_NOTES_DATABASE_VIEW_TAB_DISPLAY) next[viewId] = display;
  return next;
}

let revision = $state(0);

/** Personal tab presentation for saved database views, kept in vault config rather than in the shared view record. */
export const notesDatabaseViewTabDisplays = {
  get(viewId: string): NotesDatabaseViewTabDisplay {
    void revision;
    return readNotesDatabaseViewTabDisplay(getConfigKey<unknown>(CONFIG_KEY, undefined), viewId);
  },
  set(viewId: string, display: NotesDatabaseViewTabDisplay | null): void {
    const next = withNotesDatabaseViewTabDisplay(getConfigKey<unknown>(CONFIG_KEY, undefined), viewId, display);
    setConfigKey(CONFIG_KEY, Object.keys(next).length ? next : undefined);
    revision += 1;
  },
};
