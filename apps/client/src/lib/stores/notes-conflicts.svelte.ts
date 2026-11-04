import {
  loadNotesPageConflicts,
  resolveNotesConflict,
  type NotesConflictResolution,
  type NotesPageConflicts,
} from "$lib/api/notes/conflicts";

/**
 * Sync conflicts of the Notes pages the window shows. Views call {@link ensure} from an effect that also reads
 * {@link revision}, so they read again after {@link invalidate}, which marks every loaded read as stale.
 */
function createNotesConflictsStore() {
  let revision = $state(0);
  let reads = $state<Record<string, NotesPageConflicts>>({});
  const loadedRevisions = new Map<string, number>();
  const inFlight = new Map<string, { revision: number; request: Promise<void> }>();

  async function load(pageId: string, requestedRevision: number): Promise<void> {
    try {
      const read = await loadNotesPageConflicts(pageId);
      if (requestedRevision < (loadedRevisions.get(pageId) ?? 0)) return;
      reads = { ...reads, [pageId]: read };
      loadedRevisions.set(pageId, requestedRevision);
    } catch (error: unknown) {
      console.warn("Notes conflicts could not be loaded", error);
    } finally {
      if (inFlight.get(pageId)?.revision === requestedRevision) inFlight.delete(pageId);
    }
  }

  function invalidate(): void {
    revision += 1;
  }

  return {
    /** Bumped by every {@link invalidate}. */
    get revision(): number {
      return revision;
    },
    /** The conflicts last read for a page, or undefined before the first read. */
    read(pageId: string | null | undefined): NotesPageConflicts | undefined {
      return pageId ? reads[pageId] : undefined;
    },
    /** Reads a page's conflicts unless they are current or being read. */
    ensure(pageId: string): Promise<void> {
      const current = revision;
      if (loadedRevisions.get(pageId) === current) return Promise.resolve();
      const pending = inFlight.get(pageId);
      if (pending?.revision === current) return pending.request;
      const request = load(pageId, current);
      inFlight.set(pageId, { revision: current, request });
      return request;
    },
    /** Resolves a conflict, then marks every read as stale whether it succeeded or not. */
    async resolve(resolution: NotesConflictResolution): Promise<void> {
      try {
        await resolveNotesConflict(resolution);
      } finally {
        invalidate();
      }
    },
    /** Marks every read as stale after a resolution or after sync changed rows. */
    invalidate,
  };
}

export const notesConflicts = createNotesConflictsStore();
