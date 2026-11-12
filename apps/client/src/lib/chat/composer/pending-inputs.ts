import type {
  ChatAssignmentInputRead,
  ChatFollowupRouting,
  ChatWorkAssignmentState,
} from "$lib/chat/contracts";

/** Why a reply addressed to work still shows above the thread composer. */
export type PendingInputStatus = "pending" | "failed" | "editedAfterDelivery";

/** One reply the thread composer lists with its status. */
export interface PendingInputRow {
  input: ChatAssignmentInputRead;
  status: PendingInputStatus;
}

/**
 * Reads the status a reply shows with, or `null` when it needs no attention: delivered replies the
 * run received as they read now, and replies the run ignored.
 *
 * @param input The reply addressed to the work.
 * @returns `pending` while it waits, `failed` when it could not reach the run, and
 *   `editedAfterDelivery` when its current text differs from the delivered revision.
 */
export function pendingInputStatus(input: ChatAssignmentInputRead): PendingInputStatus | null {
  switch (input.deliveryState) {
    case "pending": return "pending";
    case "failed": return "failed";
    case "delivered":
      return input.deliveredRevisionId !== null && input.deliveredRevisionId !== input.currentRevisionId
        ? "editedAfterDelivery"
        : null;
    case "ignored": return null;
  }
}

/**
 * Lists the replies the thread composer shows, in delivery order.
 *
 * @param inputs Every input of the thread's work.
 * @returns The replies that wait, failed, or changed after the run received them.
 */
export function pendingInputRows(inputs: readonly ChatAssignmentInputRead[]): PendingInputRow[] {
  return inputs
    .flatMap((input) => {
      const status = pendingInputStatus(input);
      return status ? [{ input, status }] : [];
    })
    .sort((left, right) => left.input.ordinal - right.input.ordinal);
}

/**
 * The steer or queue choice a reply to active work starts with: the one the user made for this
 * destination, otherwise steering a teammate in the middle of a turn and queueing behind anything else.
 *
 * @param chosen The choice the user made for the destination, if any.
 * @param state The state of the thread's work.
 */
export function defaultFollowupRouting(
  chosen: ChatFollowupRouting | null,
  state: ChatWorkAssignmentState,
): ChatFollowupRouting {
  return chosen ?? (state === "working" ? "steer" : "queue");
}

/**
 * Whether a reply started its own work, whose context already carries its text: it cannot be edited,
 * and cancelling it stops that work.
 *
 * @param input The reply addressed to the work.
 */
export function pendingInputStartsWork(input: ChatAssignmentInputRead): boolean {
  return input.routingKind === "follow_up";
}
