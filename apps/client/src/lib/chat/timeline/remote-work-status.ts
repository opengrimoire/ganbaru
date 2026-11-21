import type { ChatWorkAssignmentState } from "$lib/chat/contracts";

/** The state a work row shows: a replicated work state, or starting while a linked desktop is about to claim it. */
export type RemoteWorkStatus = ChatWorkAssignmentState | "starting";

/** The icon that stands for a status in the step icon column. */
export type RemoteWorkStatusIcon = "queued" | "progress" | "question" | "approval" | "none";

/** What a phone knows of the run a linked desktop executes for the work. */
export interface RemoteWorkLiveView {
  /** Whether rows of the run show above the work row, which already show the work progressing. */
  rowsShown: boolean;
  /** Whether the work is expected to be claimed soon, as right after it was posted here. */
  claimExpected: boolean;
}

/**
 * Returns the status step a work row shows, or `null` when the run's rows above it already show the work in progress.
 * Work still reads as queued here until the desktop's claim syncs, which trails the run it already streams, so queued
 * work whose rows show is working, and queued work about to be claimed is starting.
 */
export function remoteWorkStatus(state: ChatWorkAssignmentState, live: RemoteWorkLiveView): RemoteWorkStatus | null {
  if (state === "queued" || state === "working") {
    if (live.rowsShown) return null;
    if (state === "queued" && live.claimExpected) return "starting";
  }
  return state;
}

/** Returns the icon of a work status: progress while it runs, the reason while it waits, and none once it is done. */
export function remoteWorkStatusIcon(status: RemoteWorkStatus): RemoteWorkStatusIcon {
  switch (status) {
    case "queued": return "queued";
    case "starting":
    case "working": return "progress";
    case "waiting_for_answer": return "question";
    case "waiting_for_approval": return "approval";
    case "ready_for_review":
    case "completed":
    case "failed":
    case "cancelled": return "none";
  }
}
