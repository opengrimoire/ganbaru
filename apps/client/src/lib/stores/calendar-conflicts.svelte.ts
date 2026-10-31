import { loadCalendarConflicts, type CalendarFieldConflict } from "$lib/api/calendar-conflicts";
import { conflictedEventIds, eventConflicts } from "$lib/calendar/conflicts";

/**
 * Open Calendar sync conflicts, read once for every event. Views call {@link ensure} from an effect that also reads
 * {@link revision}, so they read again after {@link invalidate}, which marks the read as stale.
 */
function createCalendarConflictsStore() {
  let revision = $state(0);
  let conflicts = $state.raw<readonly CalendarFieldConflict[]>([]);
  const eventIds = $derived(conflictedEventIds(conflicts));
  let loadedRevision = -1;
  let inFlight: { revision: number; request: Promise<void> } | undefined;

  async function load(requestedRevision: number): Promise<void> {
    try {
      const read = await loadCalendarConflicts();
      if (requestedRevision < loadedRevision) return;
      conflicts = read;
      loadedRevision = requestedRevision;
    } catch (error: unknown) {
      console.warn("Calendar conflicts could not be loaded", error);
    } finally {
      if (inFlight?.revision === requestedRevision) inFlight = undefined;
    }
  }

  return {
    /** Bumped by every {@link invalidate}. */
    get revision(): number {
      return revision;
    },
    /** Every open conflict last read; empty before the first read. */
    get all(): readonly CalendarFieldConflict[] {
      return conflicts;
    },
    /** Ids of the events with an open conflict, for badges. */
    get eventIds(): ReadonlySet<string> {
      return eventIds;
    },
    /** Whether an event, or the series an occurrence belongs to, has an open conflict. */
    isConflicted(event: { id: string; recurringParentId?: string }): boolean {
      return eventIds.has(event.recurringParentId ?? event.id);
    },
    /** The open conflicts of one event. */
    forEvent(eventId: string | null | undefined): CalendarFieldConflict[] {
      return eventId ? eventConflicts(conflicts, eventId) : [];
    },
    /** Reads the conflicts unless the read is current or in flight. */
    ensure(): Promise<void> {
      const current = revision;
      if (loadedRevision === current) return Promise.resolve();
      if (inFlight?.revision === current) return inFlight.request;
      const request = load(current);
      inFlight = { revision: current, request };
      return request;
    },
    /** Marks the read as stale after a resolution or after sync changed rows. */
    invalidate(): void {
      revision += 1;
    },
  };
}

export const calendarConflicts = createCalendarConflictsStore();
