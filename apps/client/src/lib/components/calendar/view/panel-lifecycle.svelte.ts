import type { CalendarEvent, EventSurfaceStatus } from "$lib/calendar/types";
import type { PanelAnchor } from "$lib/components/calendar/edit-session.svelte";

export type EventPanelComponent = typeof import("$lib/components/calendar/event-panel/EventPanel.svelte").default;

export type ParkedPanelSnapshot =
  | {
      mode: "create";
      sessionKey: number;
      start: string;
      end: string;
      initialCreateData: Partial<CalendarEvent>;
      anchor: PanelAnchor;
      initialAllDay: boolean;
    }
  | {
      mode: "edit";
      sessionKey: number;
      event: CalendarEvent;
      recurringScopeEnabled: boolean;
      anchor: PanelAnchor;
      detailsLoaded: boolean;
      readOnly: boolean;
      allowDeleteWhenReadOnly: boolean;
      allowPomodoroWhenReadOnly: boolean;
      skipInlineDeleteConfirm: boolean;
      endActiveEventAvailable: boolean;
      inlineEndEventConfirm: boolean;
    };

export interface CalendarViewPanelLifecycleOptions {
  importPanel: () => Promise<{ default: EventPanelComponent }>;
  mark: (name: string, detail: Record<string, string | number>) => void;
  afterRender: (callback: () => void) => void;
  afterPaint: (callback: () => void) => void;
}

/** Owns lazy EventPanel loading, request identity, parking, and paint marks. */
export class CalendarViewPanelLifecycle {
  component = $state<EventPanelComponent | null>(null);
  parkedSnapshot = $state<ParkedPanelSnapshot | null>(null);
  pendingEditEventId = $state<string>();
  surfaceStatus = $state<EventSurfaceStatus>();
  surfaceStatusEventId = $state<string>();

  private loading: Promise<void> | null = null;
  private requestId = 0;

  constructor(private readonly options: CalendarViewPanelLifecycleOptions) {}

  get currentRequestId(): number {
    return this.requestId;
  }

  load(): Promise<void> {
    if (this.component) return Promise.resolve();
    this.loading ??= this.options.importPanel()
      .then((module) => {
        this.component = module.default;
      })
      .finally(() => {
        this.loading = null;
      });
    return this.loading;
  }

  beginOpen(mode: "create" | "edit", state: "open" | "unpark" | "switch"): number {
    const requestId = ++this.requestId;
    this.options.mark("panel.start", {
      mode,
      state,
      module: this.component ? "loaded" : "cold",
      request: requestId,
    });
    return requestId;
  }

  ensureReady(requestId: number): Promise<void> | undefined {
    if (this.component) {
      this.options.mark("panel.module-ready", { request: requestId });
      return undefined;
    }
    return this.load().then(() => {
      if (this.isCurrent(requestId)) {
        this.options.mark("panel.module-ready", { request: requestId });
      }
    });
  }

  isCurrent(requestId: number): boolean {
    return requestId === this.requestId;
  }

  /** Release interaction state after the current panel open fails. */
  recoverFailedOpen(requestId: number): boolean {
    if (!this.isCurrent(requestId)) return false;
    this.invalidate();
    this.surfaceStatus = undefined;
    this.surfaceStatusEventId = undefined;
    return true;
  }

  markDetailsReady(requestId: number, found: boolean): void {
    if (this.isCurrent(requestId)) {
      this.options.mark("panel.details-ready", { found: found ? 1 : 0, request: requestId });
    }
  }

  markStateOpen(requestId: number): void {
    this.options.mark("panel.state-open", { request: requestId });
  }

  markPaintDone(requestId: number): void {
    this.options.afterRender(() => {
      if (!this.isCurrent(requestId)) return;
      this.options.mark("panel.flush-done", { request: requestId });
      this.options.afterPaint(() => {
        if (this.isCurrent(requestId)) {
          this.options.mark("panel.paint-done", { request: requestId });
        }
      });
    });
  }

  invalidate(): void {
    this.requestId += 1;
    this.pendingEditEventId = undefined;
  }

  park(snapshot: ParkedPanelSnapshot | null): void {
    if (snapshot) this.parkedSnapshot = snapshot;
    this.pendingEditEventId = undefined;
    this.surfaceStatus = undefined;
    this.surfaceStatusEventId = undefined;
  }

  setSurfaceStatus(status: EventSurfaceStatus | undefined, eventId: string | undefined): void {
    this.surfaceStatus = status;
    this.surfaceStatusEventId = eventId;
  }
}
