import type { CalendarEvent, RecurringScope } from "$lib/calendar/types";
import type { EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import type { PanelSaveData } from "$lib/components/calendar/event-panel/payloads";
import type { CalendarEditIntent } from "$lib/api/calendar-edit";
import type { getCalendar } from "$lib/stores/calendar.svelte";
import type { computeViewWindow } from "$lib/calendar/utils";
import { buildNativeCalendarCreate, buildNativeCalendarEdit } from "./native-edit-intent";
import type {
  CalendarEditContext, CalendarEditDraft, NativeCalendarEditController,
} from "./native-edit-controller.svelte";

type EditState = Extract<EditSessionState, { mode: "edit" }>;
let directSessionKey = 0;

export interface CalendarPanelPersistResult {
  saveRefreshedVisibleWindow: boolean;
}

/** Task linking belongs to the Project command boundary. */
export function calendarDataOnly(data: PanelSaveData): PanelSaveData {
  const { linkedTaskIds: _linkedTaskIds, ...calendarData } = data;
  return calendarData;
}

export interface CalendarViewCommitServiceOptions {
  calendarStore: Pick<ReturnType<typeof getCalendar>, "acceptNativeEdit">;
  nativeEditor: Pick<NativeCalendarEditController, "commit">;
  directEditor: Pick<NativeCalendarEditController, "commit">;
  getSessionState: () => EditSessionState;
  getBaseline: () => Partial<CalendarEvent>;
  getScope: () => RecurringScope;
  getViewWindow: () => ReturnType<typeof computeViewWindow>;
  getContext: () => CalendarEditContext | null;
  getRenderZone: () => string;
}

/** Serialize user drafts; native services own creation and existing-event commits. */
export class CalendarViewCommitService {
  constructor(private readonly options: CalendarViewCommitServiceOptions) {}

  /** Build the same immutable intent for visible preview and accepted Save. */
  editDraft(
    changes: Partial<CalendarEvent>, scope?: RecurringScope,
    action?: CalendarEditIntent["action"],
  ): CalendarEditDraft {
    const state = this.options.getSessionState();
    if (state.mode === "create") {
      if (action && action !== "save") throw new Error("Calendar action requires an existing occurrence");
      const context = this.options.getContext();
      if (!context) throw new Error("Native Calendar context is unavailable; retry after loading");
      const window = this.options.getViewWindow();
      const renderZone = this.options.getRenderZone();
      return { ...context, sessionKey: state.sessionKey,
        edit: buildNativeCalendarCreate({ start: state.start, end: state.end, changes, renderZone }),
        window: { windowStartDate: window.start.toString(), windowEndDate: window.end.toString(),
          renderZone, includeTotalEventCount: false },
      };
    }
    if (state.mode !== "edit") throw new Error("Calendar has no selected occurrence to edit");
    return this.draft(state, state.sessionKey, this.options.getBaseline(), changes,
      scope ?? this.options.getScope(), action);
  }

  private draft(
    state: Pick<EditState, "instanceEvent" | "templateId">, sessionKey: number,
    baseline: Partial<CalendarEvent>, changes: Partial<CalendarEvent>, scope: RecurringScope,
    action?: CalendarEditIntent["action"],
  ): CalendarEditDraft {
    const context = this.options.getContext();
    if (!context) throw new Error("Native Calendar context is unavailable; retry after loading");
    const window = this.options.getViewWindow();
    const renderZone = this.options.getRenderZone();
    return { ...context, sessionKey,
      edit: buildNativeCalendarEdit({ state, baseline, changes, scope, renderZone, action }),
      window: { windowStartDate: window.start.toString(), windowEndDate: window.end.toString(),
        renderZone, includeTotalEventCount: false },
    };
  }

  /** Creation and edits share native review, receipts and coupled Focus persistence. */
  async persist(
    data: PanelSaveData, scope?: RecurringScope,
    settings: { action?: CalendarEditIntent["action"] } = {},
  ): Promise<CalendarPanelPersistResult> {
    const state = this.options.getSessionState();
    const changes = calendarDataOnly(data);
    if (state.mode === "closed") return { saveRefreshedVisibleWindow: false };
    await this.options.nativeEditor.commit(this.editDraft(changes, scope, settings.action));
    this.options.calendarStore.acceptNativeEdit();
    return { saveRefreshedVisibleWindow: false };
  }

  /** An immediate drag shares native protection, Focus ownership and retry receipts. */
  async persistDirect(event: CalendarEvent, original: CalendarEvent): Promise<void> {
    directSessionKey -= 1;
    if (!Number.isSafeInteger(directSessionKey)) throw new Error("Calendar drag identity exhausted");
    await this.options.directEditor.commit(this.draft({ instanceEvent: original,
      templateId: original.recurringParentId ?? original.id }, directSessionKey, original,
      { start: event.start, end: event.end }, "this"));
    this.options.calendarStore.acceptNativeEdit();
  }
}
