import type { TabDragGrab } from "$lib/windows/tab-windows";
import type { WorkspaceTab, WorkspaceTabPlacement } from "./model";

/**
 * A tab drag under way in this vault, started in this window or another. Every window draws the tab while the
 * drag passes over it, and whichever window holds the tab moves it along its strip.
 */
export interface LiveTabDrag extends TabDragGrab {
  /** Window the drag started in, or null when this window cannot share tabs with others. */
  sourceLabel: string | null;
  tab: WorkspaceTab;
}

/** Where a tab this window asked for goes once it arrives, or null to keep it at the end. */
export type TabDropTarget = { id: string; placement: WorkspaceTabPlacement } | null;

/**
 * The drag whose source is in this window. Its source must stay on screen until the drag ends, so a window
 * whose last tab left during it only hides, and closes once the drag ends.
 */
export type OwnTabDrag =
  | { phase: "none" }
  | { phase: "dragging"; tabId: string; leftEmpty: boolean }
  /** The drag ended, or was forgotten, after the window's last tab left; the window closes at the next drag end. */
  | { phase: "closePending" };

/**
 * A tab this window asked for. A tab hovered over the strip is forgotten when its drag ends, while a dropped
 * tab keeps its place until it arrives or waits too long.
 */
export type TabArrival =
  | { phase: "none" }
  | { phase: "hovering"; tabId: string; target: TabDropTarget }
  | { phase: "dropped"; tabId: string; target: TabDropTarget };

/** Tab drag state of one window. The drawn drag and this window's own drag are separate, since another window's drag can replace the drawn one while this window's drag source still runs. */
export interface TabDragState {
  /** The drag this window draws, from this window or another. */
  live: LiveTabDrag | null;
  own: OwnTabDrag;
  arrival: TabArrival;
  /** When this window last asked for a dragged tab, so it asks again only now and then. */
  lastClaim: { tabId: string; at: number } | null;
}

export type TabDragEvent =
  /** This window started dragging one of its tabs. */
  | { type: "begun"; drag: LiveTabDrag }
  /** Another window started a drag in this vault. */
  | { type: "remoteBegun"; drag: LiveTabDrag }
  /** This window's last tab left during its own drag. */
  | { type: "leftEmpty" }
  /** This window's drag source for `tabId` ended. */
  | { type: "sourceEnded"; tabId: string }
  /** This window heard the drag of `tabId` end. */
  | { type: "settled"; tabId: string }
  /** This window asked for `tabId`, at `askedAt` when it sent the request and null when it held off. */
  | { type: "claimed"; tabId: string; target: TabDropTarget; dropped: boolean; askedAt: number | null }
  /** The tab this window asked for arrived. */
  | { type: "arrived"; tabId: string }
  /** A dropped tab did not arrive in time. */
  | { type: "arrivalTimedOut"; tabId: string }
  /** Forget any drag under way, as on a vault change or disconnect. */
  | { type: "reset" }
  /** The window closed after its last tab left. */
  | { type: "closed" };

const NO_OWN_DRAG: OwnTabDrag = { phase: "none" };
const NO_ARRIVAL: TabArrival = { phase: "none" };

export const IDLE_TAB_DRAG: TabDragState = { live: null, own: NO_OWN_DRAG, arrival: NO_ARRIVAL, lastClaim: null };

/** How long a window waits before it asks again for a tab dragged over its strip. */
export const CLAIM_INTERVAL_MS = 150;

/** The target a tab this window asked for goes to when `tabId` arrives, or null. */
export function arrivalTarget(state: TabDragState, tabId: string): TabDropTarget {
  return state.arrival.phase !== "none" && state.arrival.tabId === tabId ? state.arrival.target : null;
}

/** Whether a drag whose source is in this window runs. */
export function ownsTabDrag(state: TabDragState): boolean {
  return state.own.phase === "dragging";
}

/** Whether a claim for `tabId` at `now` asks the holding window again; a drop always asks. */
export function shouldAskForTab(state: TabDragState, tabId: string, dropped: boolean, now: number): boolean {
  return dropped || state.lastClaim?.tabId !== tabId || now - state.lastClaim.at >= CLAIM_INTERVAL_MS;
}

/** End this window's drag source for `tabId`, keeping a pending close when its last tab left during it. */
function endOwnDrag(own: OwnTabDrag, tabId: string): OwnTabDrag {
  if (own.phase !== "dragging" || own.tabId !== tabId) return own;
  return own.leftEmpty ? { phase: "closePending" } : NO_OWN_DRAG;
}

function withOwn(state: TabDragState, own: OwnTabDrag): TabDragState {
  return own === state.own ? state : { ...state, own };
}

/** Apply one tab drag event; events that do not apply return the same state object. */
export function transitionTabDrag(state: TabDragState, event: TabDragEvent): TabDragState {
  switch (event.type) {
    case "begun":
      return {
        ...state,
        live: event.drag,
        own: {
          phase: "dragging",
          tabId: event.drag.tab.id,
          leftEmpty: state.own.phase === "closePending" || (state.own.phase === "dragging" && state.own.leftEmpty),
        },
        lastClaim: null,
      };
    case "remoteBegun":
      return { ...state, live: event.drag };
    case "leftEmpty":
      if (state.own.phase !== "dragging" || state.own.leftEmpty) return state;
      return { ...state, own: { ...state.own, leftEmpty: true } };
    case "sourceEnded":
      return withOwn(state, endOwnDrag(state.own, event.tabId));
    case "settled": {
      const keepArrival = state.arrival.phase !== "hovering" || state.arrival.tabId !== event.tabId;
      return {
        live: null,
        own: endOwnDrag(state.own, event.tabId),
        arrival: keepArrival ? state.arrival : NO_ARRIVAL,
        lastClaim: null,
      };
    }
    case "claimed":
      return {
        ...state,
        arrival: { phase: event.dropped ? "dropped" : "hovering", tabId: event.tabId, target: event.target },
        lastClaim: event.askedAt === null ? state.lastClaim : { tabId: event.tabId, at: event.askedAt },
      };
    case "arrived": {
      const arrivalMatches = state.arrival.phase !== "none" && state.arrival.tabId === event.tabId;
      const claimMatches = state.lastClaim?.tabId === event.tabId;
      if (!arrivalMatches && !claimMatches) return state;
      return {
        ...state,
        arrival: arrivalMatches ? NO_ARRIVAL : state.arrival,
        lastClaim: claimMatches ? null : state.lastClaim,
      };
    }
    case "arrivalTimedOut":
      if (state.arrival.phase === "none" || state.arrival.tabId !== event.tabId) return state;
      return { ...state, arrival: NO_ARRIVAL };
    case "reset": {
      const own: OwnTabDrag = state.own.phase === "dragging" && state.own.leftEmpty ? { phase: "closePending" }
        : state.own.phase === "closePending" ? state.own : NO_OWN_DRAG;
      return { live: null, own, arrival: NO_ARRIVAL, lastClaim: null };
    }
    case "closed":
      return state.own.phase === "closePending" ? withOwn(state, NO_OWN_DRAG) : state;
  }
}
