/** A committed Calendar event to project assignment returned by a project mutation. */
export interface CalendarEventProjectAssignment {
  eventId: string;
  projectId: string;
}

type CalendarEventProjectAssignmentHandler = (
  assignments: readonly CalendarEventProjectAssignment[],
) => Promise<void>;

let handler: CalendarEventProjectAssignmentHandler | null = null;

/**
 * Register how the loaded Calendar store applies committed project assignments.
 *
 * Projects reaches Calendar through this registration instead of importing the Calendar store, so the
 * Projects store stays independent of Calendar loading. When no Calendar store is loaded, nothing needs
 * updating because Calendar reads the committed assignments from SQLite when it loads.
 *
 * @param next Applies assignments to the loaded Calendar store.
 * @returns A function that removes the registration if it is still current.
 */
export function registerCalendarEventProjectAssignmentHandler(
  next: CalendarEventProjectAssignmentHandler,
): () => void {
  handler = next;
  return () => {
    if (handler === next) handler = null;
  };
}

/**
 * Apply committed project assignments to the loaded Calendar store, if any.
 *
 * @param assignments Assignments committed by a project mutation.
 */
export async function applyCalendarEventProjectAssignments(
  assignments: readonly CalendarEventProjectAssignment[],
): Promise<void> {
  if (assignments.length === 0 || !handler) return;
  await handler(assignments);
}
