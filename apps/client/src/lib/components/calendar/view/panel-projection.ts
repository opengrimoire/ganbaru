import type { CalendarEvent } from "$lib/calendar/types";
import type { EditSessionState, PanelAnchor } from "$lib/components/calendar/edit-session.svelte";
import type { ParkedPanelSnapshot } from "./panel-lifecycle.svelte";

/** Resolve an editor baseline from persisted occurrences, preserving native geometry. */
export function resolvePersistedCalendarPanelEvent(
  displayedId: string,
  canonicalEvents: readonly CalendarEvent[],
  fullEvent?: CalendarEvent | null,
): CalendarEvent | undefined {
  const canonical = canonicalEvents.find((event) => event.id === displayedId);
  if (!canonical) return undefined;
  if (!fullEvent) return canonical;
  if (fullEvent.id !== (canonical.recurringParentId ?? canonical.id)) return undefined;
  return { ...fullEvent, ...canonical };
}

export type PanelRenderState =
  | {
      parked: boolean;
      mode: "create";
      sessionKey: number;
      start: string;
      end: string;
      initialCreateData: Partial<CalendarEvent>;
      anchor: PanelAnchor;
      initialAllDay: boolean;
      event?: undefined;
      detailsLoaded: false;
      externalDirty: false;
      readOnly: false;
      allowDeleteWhenReadOnly: false;
      allowPomodoroWhenReadOnly: false;
      skipInlineDeleteConfirm: false;
    }
  | {
      parked: boolean;
      mode: "edit";
      sessionKey: number;
      start: "";
      end: "";
      anchor: PanelAnchor;
      initialAllDay: false;
      event: CalendarEvent;
      recurringScopeEnabled: boolean;
      detailsLoaded: boolean;
      externalDirty: boolean;
      readOnly: boolean;
      allowDeleteWhenReadOnly: boolean;
      allowPomodoroWhenReadOnly: boolean;
      skipInlineDeleteConfirm: boolean;
      endActiveEventAvailable: boolean;
      inlineEndEventConfirm: boolean;
    };

export interface PanelEditProjection {
  selectedActive: boolean;
  recurring: boolean;
  detailsLoaded: boolean;
  locked: boolean;
  allowArchive: boolean;
  allowPomodoroWhenReadOnly: boolean;
  deleteWouldStopSession: boolean;
  endWouldStopProductivity: boolean;
}

export function snapshotCalendarPanel(input: {
  state: EditSessionState;
  changes: Partial<CalendarEvent>;
  panelEvent: CalendarEvent | undefined;
  edit?: PanelEditProjection;
}): ParkedPanelSnapshot | null {
  const { state } = input;
  if (state.mode === "create") {
    return {
      mode: "create",
      sessionKey: state.sessionKey,
      start: state.start,
      end: state.end,
      initialCreateData: { ...input.changes },
      anchor: state.anchor,
      initialAllDay: !!input.changes.allDay,
    };
  }
  if (state.mode !== "edit" || !input.panelEvent || !input.edit) return null;
  return {
    mode: "edit",
    sessionKey: state.sessionKey,
    event: input.panelEvent,
    recurringScopeEnabled: input.edit.recurring && !input.edit.selectedActive,
    anchor: state.anchor,
    detailsLoaded: input.edit.detailsLoaded,
    readOnly: input.edit.locked,
    allowDeleteWhenReadOnly: input.edit.locked
      && (input.edit.allowArchive || input.edit.selectedActive),
    allowPomodoroWhenReadOnly: input.edit.allowPomodoroWhenReadOnly,
    skipInlineDeleteConfirm: input.edit.deleteWouldStopSession,
    endActiveEventAvailable: false,
    inlineEndEventConfirm: false,
  };
}

export function projectCalendarPanel(input: {
  hidden: boolean;
  state: EditSessionState;
  changes: Partial<CalendarEvent>;
  dirty: boolean;
  panelEvent: CalendarEvent | undefined;
  parked: ParkedPanelSnapshot | null;
  endingActiveEvent: boolean;
  edit?: PanelEditProjection;
}): PanelRenderState | null {
  if (input.hidden) return null;
  const { state } = input;
  if (state.mode === "create") {
    return {
      parked: false,
      mode: "create",
      sessionKey: state.sessionKey,
      start: state.start,
      end: state.end,
      initialCreateData: { ...input.changes },
      anchor: state.anchor,
      initialAllDay: !!input.changes.allDay,
      detailsLoaded: false,
      externalDirty: false,
      readOnly: false,
      allowDeleteWhenReadOnly: false,
      allowPomodoroWhenReadOnly: false,
      skipInlineDeleteConfirm: false,
    };
  }
  if (state.mode === "edit" && input.panelEvent && input.edit) {
    const selectedActive = input.endingActiveEvent || input.edit.selectedActive;
    return {
      parked: false,
      mode: "edit",
      sessionKey: state.sessionKey,
      start: "",
      end: "",
      anchor: state.anchor,
      initialAllDay: false,
      event: input.panelEvent,
      recurringScopeEnabled: input.edit.recurring && !selectedActive,
      detailsLoaded: input.edit.detailsLoaded,
      externalDirty: input.dirty,
      readOnly: !input.endingActiveEvent && input.edit.locked,
      allowDeleteWhenReadOnly: !input.endingActiveEvent && input.edit.locked
        && (input.edit.allowArchive || selectedActive),
      allowPomodoroWhenReadOnly: !input.endingActiveEvent
        && input.edit.allowPomodoroWhenReadOnly,
      skipInlineDeleteConfirm: input.edit.deleteWouldStopSession,
      endActiveEventAvailable: selectedActive,
      inlineEndEventConfirm: selectedActive
        && !input.endingActiveEvent
        && !input.edit.endWouldStopProductivity,
    };
  }
  return projectParkedPanel(input.parked);
}

function projectParkedPanel(parked: ParkedPanelSnapshot | null): PanelRenderState | null {
  if (!parked) return null;
  if (parked.mode === "create") {
    return {
      parked: true,
      ...parked,
      detailsLoaded: false,
      externalDirty: false,
      readOnly: false,
      allowDeleteWhenReadOnly: false,
      allowPomodoroWhenReadOnly: false,
      skipInlineDeleteConfirm: false,
    };
  }
  return {
    parked: true,
    mode: "edit",
    sessionKey: parked.sessionKey,
    start: "",
    end: "",
    anchor: parked.anchor,
    initialAllDay: false,
    event: parked.event,
    recurringScopeEnabled: parked.recurringScopeEnabled,
    detailsLoaded: parked.detailsLoaded,
    externalDirty: false,
    readOnly: parked.readOnly,
    allowDeleteWhenReadOnly: parked.allowDeleteWhenReadOnly,
    allowPomodoroWhenReadOnly: parked.allowPomodoroWhenReadOnly,
    skipInlineDeleteConfirm: parked.skipInlineDeleteConfirm,
    endActiveEventAvailable: parked.endActiveEventAvailable,
    inlineEndEventConfirm: parked.inlineEndEventConfirm,
  };
}
