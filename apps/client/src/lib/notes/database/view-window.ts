import type {
  NotesDataSourceBoardGroup,
  NotesDataSourceBoardView,
  NotesPage,
} from "$lib/notes/types";

/** Hard ceiling for hydrated database rows retained by one mounted view. */
export const MAX_RETAINED_DATABASE_ROWS = 200;

/** Merge a keyset page into an existing row window without duplicate rows. */
export function mergeNotesDatabaseRows(
  current: readonly NotesPage[],
  next: readonly NotesPage[],
  limit = MAX_RETAINED_DATABASE_ROWS,
): NotesPage[] {
  const rows = new Map(current.map((row) => [row.id, row]));
  for (const row of next) rows.set(row.id, row);
  const merged = [...rows.values()];
  return merged.length > limit ? merged.slice(merged.length - limit) : merged;
}

/** Merge a board keyset page while preserving configured empty groups. */
export function mergeNotesDatabaseBoardWindow(
  current: NotesDataSourceBoardView,
  next: NotesDataSourceBoardView,
): NotesDataSourceBoardView {
  const groups = new Map<string, NotesDataSourceBoardGroup>();
  for (const group of [...current.groups, ...next.groups]) {
    const existing = groups.get(group.id);
    groups.set(group.id, {
      ...group,
      rows: mergeNotesDatabaseRows(existing?.rows ?? [], group.rows),
    });
  }
  const mergedGroups = [...groups.values()];
  const retainedIds = new Set(
    mergedGroups
      .flatMap((group) => group.rows)
      .slice(-MAX_RETAINED_DATABASE_ROWS)
      .map((row) => row.id),
  );
  return {
    ...next,
    groups: mergedGroups.map((group) => ({
      ...group,
      rows: group.rows.filter((row) => retainedIds.has(row.id)),
    })),
  };
}
