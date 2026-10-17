import type { CalendarEvent } from "$lib/calendar/types";
import type { EditSessionState, PanelAnchor } from "$lib/components/calendar/edit-session.svelte";
import { isPendingCreateEventId } from "$lib/components/calendar/display-events";
import { Temporal } from "@js-temporal/polyfill";

interface CalendarDragSession {
  readonly state: EditSessionState;
  readonly dirty: boolean;
  readonly changes?: Partial<CalendarEvent>;
  openEdit(
    event: CalendarEvent,
    anchor: PanelAnchor,
    fullEvent?: CalendarEvent,
    detailsLoaded?: boolean,
  ): void;
  updateChanges(changes: Partial<CalendarEvent>): void;
}

export interface CalendarDragCommitControllerOptions {
  session: CalendarDragSession;
  isCommitHidden: () => boolean;
  editingId: () => string | undefined;
  visibleEvents: () => CalendarEvent[];
  isRecurring: (event: CalendarEvent) => boolean;
  isActivePomodoroEvent: (event: CalendarEvent) => boolean;
  panelAnchor: (eventId: string) => PanelAnchor;
  loadPanel: () => Promise<void>;
  confirmDiscard: (action: () => Promise<void>) => void;
  getTemplate: (event: CalendarEvent) => CalendarEvent | undefined;
  updateEvent: (event: CalendarEvent) => Promise<void>;
  now?: () => number;
}

/** Coordinates calendar drag and resize commits with open edit sessions. */
export class CalendarDragCommitController {
  lastDragEndTime = 0;

  constructor(private readonly options: CalendarDragCommitControllerOptions) {}

  markInteractionEnd(): void {
    this.lastDragEndTime = (this.options.now ?? Date.now)();
  }

  async handle(event: CalendarEvent): Promise<void> {
    if (this.options.isCommitHidden()) return;
    this.markInteractionEnd();

    if (isPendingCreateEventId(event.id)) {
      if (this.options.session.state.mode === "create") {
        const selectedId = this.options.editingId();
        const original = this.options.visibleEvents().find((candidate) => candidate.id === event.id);
        if (!original && selectedId !== event.id) return;
        if (original && selectedId !== event.id) {
          const state = this.options.session.state;
          const changes = this.options.session.changes;
          const shift = (base: string, before: string, after: string): string => {
            const civil = (value: string) => Temporal.PlainDateTime.from(value.replace(" ", "T"));
            return civil(base).add(civil(before).until(civil(after), { largestUnit: "days" }))
              .toString({ smallestUnit: "minute" }).replace("T", " ");
          };
          this.options.session.updateChanges({
            start: shift(changes?.start ?? state.start, original.start, event.start),
            end: shift(changes?.end ?? state.end, original.end, event.end),
          });
        } else {
          this.applyTimes(event);
        }
      }
      return;
    }

    const state = this.options.session.state;
    if (state.mode === "edit"
      && (state.originalEvent.id === event.id || this.options.editingId() === event.id)) {
      this.applyTimes(event);
      return;
    }

    if (this.options.isActivePomodoroEvent(event) || this.options.isRecurring(event)) {
      const original = this.options.visibleEvents().find((candidate) => candidate.id === event.id);
      if (!original) return;
      const open = async () => {
        await this.options.loadPanel();
        this.options.session.openEdit(original, this.options.panelAnchor(event.id), original);
        this.applyTimes(event);
      };
      if (this.options.session.dirty) {
        this.options.confirmDiscard(open);
        return;
      }
      await open();
      return;
    }

    const template = this.options.getTemplate(event);
    if (template && template.start === event.start && template.end === event.end) return;
    await this.options.updateEvent(event);
  }

  private applyTimes(event: CalendarEvent): void {
    this.options.session.updateChanges({ start: event.start, end: event.end });
  }
}
