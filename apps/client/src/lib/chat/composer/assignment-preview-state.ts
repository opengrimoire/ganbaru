import type { ChatAssignmentTargetRead } from "$lib/chat/contracts";

/**
 * The assignment targets the composer previews for the mentioned agent. Targets are device-local folders, so the
 * preview is off unless an agent is mentioned in a channel this device runs work for. A read carries its request
 * token, so a result for an earlier mention, channel, or thread never lands.
 */
export type AssignmentPreviewState =
  | { phase: "off" }
  | { phase: "loading"; request: number }
  | { phase: "ready"; targets: ChatAssignmentTargetRead[] };

export type AssignmentPreviewEvent =
  /** No agent is mentioned or this device runs no work; any read in flight is dropped. */
  | { type: "disabled" }
  /** A read with token `request` started for the current mention. */
  | { type: "requested"; request: number }
  /** The read with token `request` returned `targets`, or failed when `targets` is empty after an error. */
  | { type: "loaded"; request: number; targets: ChatAssignmentTargetRead[] }
  /** The composer cleared after a delivery. */
  | { type: "cleared" };

export const ASSIGNMENT_PREVIEW_OFF: AssignmentPreviewState = { phase: "off" };

/**
 * Apply one preview event. A result whose token is not the read in flight returns the same state object, so the
 * caller can tell a stale result by identity.
 */
export function transitionAssignmentPreview(
  state: AssignmentPreviewState,
  event: AssignmentPreviewEvent,
): AssignmentPreviewState {
  switch (event.type) {
    case "disabled":
      return state.phase === "off" ? state : ASSIGNMENT_PREVIEW_OFF;
    case "requested":
      return { phase: "loading", request: event.request };
    case "loaded":
      return state.phase === "loading" && state.request === event.request
        ? { phase: "ready", targets: event.targets }
        : state;
    case "cleared":
      return state.phase === "ready" && state.targets.length > 0 ? { phase: "ready", targets: [] } : state;
  }
}

/** The targets to check blockers against; none while a read runs. */
export function previewTargets(state: AssignmentPreviewState): ChatAssignmentTargetRead[] {
  return state.phase === "ready" ? state.targets : [];
}
