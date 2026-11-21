/** A device linking action that holds the panel's controls while it runs. */
export type HandoffAction = "status" | "network" | "pair" | "approval" | "bootstrap" | "unlink" | "recover" | "linkFolder";

/** What the panel reports as running: one action, or the pairing invitation being created. */
export type HandoffBusy = HandoffAction | "invite";

/**
 * The work in flight in the device linking panel. Creating a pairing invitation is its own axis because Settings
 * keeps the other controls usable while it runs, so an action may start beside it. Each running variant carries
 * the request that started it, so only that request's end clears it.
 */
export interface HandoffActivity {
  invite: { phase: "idle" } | { phase: "creating"; request: number };
  action: { phase: "idle" } | { phase: "running"; kind: HandoffAction; request: number };
}

export type HandoffActivityEvent =
  /** Creating the invitation numbered `request` started; it replaces any creation still running. */
  | { type: "inviteStarted"; request: number }
  /** The invitation creation numbered `request` ended, whether it succeeded or failed. */
  | { type: "inviteFinished"; request: number }
  /** The action numbered `request` started; it replaces any action still running. */
  | { type: "actionStarted"; kind: HandoffAction; request: number }
  /** The action numbered `request` ended, whether it succeeded or failed. */
  | { type: "actionFinished"; request: number };

export const IDLE_HANDOFF_ACTIVITY: HandoffActivity = { invite: { phase: "idle" }, action: { phase: "idle" } };

/**
 * The activity before the panel mounts: without a cached status it shows the status loading state until the first
 * status read, numbered above zero, replaces this placeholder.
 */
export function initialHandoffActivity(statusCached: boolean): HandoffActivity {
  if (statusCached) return IDLE_HANDOFF_ACTIVITY;
  return { invite: { phase: "idle" }, action: { phase: "running", kind: "status", request: 0 } };
}

/**
 * Apply one activity event. The end of a request that is no longer running returns the same state object, so
 * callers can tell an ignored move by identity.
 */
export function transitionHandoffActivity(state: HandoffActivity, event: HandoffActivityEvent): HandoffActivity {
  switch (event.type) {
    case "inviteStarted":
      return { ...state, invite: { phase: "creating", request: event.request } };
    case "inviteFinished":
      if (state.invite.phase !== "creating" || state.invite.request !== event.request) return state;
      return { ...state, invite: { phase: "idle" } };
    case "actionStarted":
      return { ...state, action: { phase: "running", kind: event.kind, request: event.request } };
    case "actionFinished":
      if (state.action.phase !== "running" || state.action.request !== event.request) return state;
      return { ...state, action: { phase: "idle" } };
  }
}

/** The running action, else the invitation being created, else `null`. */
export function handoffBusy(state: HandoffActivity): HandoffBusy | null {
  if (state.action.phase === "running") return state.action.kind;
  return state.invite.phase === "creating" ? "invite" : null;
}

/** Whether a pairing invitation is being created, whatever else runs beside it. */
export function handoffInviting(state: HandoffActivity): boolean {
  return state.invite.phase === "creating";
}

/** The confirmation dialog the panel shows; an unlink names the device it would unlink. */
export type HandoffConfirmation =
  | { kind: "network" }
  | { kind: "networkRevoke" }
  | { kind: "unlink"; deviceId: string }
  | { kind: "recover" }
  | { kind: "firstCopy" };
