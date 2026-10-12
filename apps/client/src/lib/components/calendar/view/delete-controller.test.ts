import { describe, expect, it, vi } from "vitest";
import { Temporal } from "@js-temporal/polyfill";
import { dismissCalendarDeleteUndo, type CalendarEditPreview, type CalendarCommitReceipt } from "$lib/api/calendar-edit";
import type { CalendarEditDraft } from "./native-edit-controller.svelte";
import type { CalendarEvent } from "$lib/calendar/types";
import { CalendarViewDeleteController } from "./delete-controller";

vi.mock("$lib/api/calendar-edit", () => ({ dismissCalendarDeleteUndo: vi.fn(async () => {}) }));

function event(id: string, recurringParentId?: string): CalendarEvent {
  return { id, recurringParentId, title: id, start: "2026-07-12 09:00", end: "2026-07-12 10:00",
    timezone: "UTC", calendarId: "local" };
}

function fixture() {
  const input: CalendarEditDraft = { vaultId: "vault", vaultGeneration: 2, sessionKey: 7,
    edit: { kind: "delete", selection: { templateId: "source", recurrenceDate: "2026-07-12", scope: "all" }, stopActive: false },
    window: { windowStartDate: "2026-07-11", windowEndDate: "2026-07-18", renderZone: "UTC", includeTotalEventCount: false } };
  const review: CalendarEditPreview = { vaultId: "vault", vaultGeneration: 2, commandId: "delete",
    sourceId: "source", editedId: "source", reviewRevision: "a".repeat(64), changed: true,
    scope: { effectiveScope: "all", selectedStarted: false, selectedHasHistory: false, selectedActive: false },
    deletion: { outcome: "mixed", requiresActiveStop: true, historyOnly: false },
    window: { sourceEvents: [], windowEvents: [event("survivor", "source")], diagnostics: [], totalEventCount: null },
    previewedIds: new Set(), editingId: undefined };
  const receipt: CalendarCommitReceipt = { commandId: "delete", editedId: "selected", changed: true, preservedIds: [],
    undoReviewRevision: "b".repeat(64), undoAvailableForMs: 4_000 };
  const nativeEditor = {
    commitReviewed: vi.fn(async () => receipt), undoDeletion: vi.fn(async () => ({ ...receipt, commandId: "undo" })),
    pendingKind: "delete" as "delete" | "undo_delete", retainedPreview: review,
    retryPending: vi.fn(async () => receipt), acceptedResultAgeMs: 1_000, acknowledgeResult: vi.fn(),
    acceptsContext: vi.fn(() => true),
  };
  const calendarStore = { acceptNativeEdit: vi.fn(), refreshWindow: vi.fn(async () => {}) };
  const toasts = { showDeletePendingToast: vi.fn(() => "toast"), showDeleteUndoToast: vi.fn(),
    dismissDeleteToastIfPending: vi.fn(), dismissDeleteUndoToast: vi.fn(),
    showSavePendingToast: vi.fn(() => "error"), showSaveErrorToast: vi.fn() };
  const setCommitState = vi.fn(), closeSession = vi.fn();
  let mounted = true, current = true;
  const controller = new CalendarViewDeleteController({ calendarStore, nativeEditor, toasts,
    getWindow: () => ({ start: Temporal.PlainDate.from("2026-07-11"), end: Temporal.PlainDate.from("2026-07-18") }),
    getEvents: () => [event("source"), event("removed", "source"), event("unrelated")],
    canPresent: () => mounted, isCurrent: () => current, setCommitState, closeSession,
    pendingLabel: () => "pending", outcomeLabel: () => "completed", errorLabel: (error) => String(error) });
  return { input, review, receipt, nativeEditor, calendarStore, toasts, setCommitState, closeSession, controller,
    unmount: () => { mounted = false; }, changeSelection: () => { current = false; } };
}

describe("native Calendar deletion presentation", () => {
  it("freezes only the native family and uses the remaining native opportunity for semantic Undo", async () => {
    const f = fixture();
    await f.controller.execute(f.input, f.review, true);
    expect(f.nativeEditor.commitReviewed).toHaveBeenCalledExactlyOnceWith(f.input, f.review, true);
    expect(f.setCommitState.mock.calls[0][0].frozenEvents.map((row: CalendarEvent) => row.id)).toEqual(["unrelated", "survivor"]);
    expect(f.calendarStore.acceptNativeEdit).toHaveBeenCalledOnce();
    expect(f.calendarStore.refreshWindow).toHaveBeenCalledOnce();
    expect(f.closeSession).toHaveBeenCalledOnce();
    expect(f.toasts.showDeleteUndoToast).toHaveBeenCalledWith("toast", "completed", expect.any(Function), 3_000);
    await f.controller.undoCurrent();
    expect(f.nativeEditor.undoDeletion).toHaveBeenCalledExactlyOnceWith(f.review, f.receipt);
    expect(f.calendarStore.refreshWindow).toHaveBeenCalledTimes(2);
    expect(f.nativeEditor.acknowledgeResult.mock.calls.map(([id]) => id)).toEqual(["delete", "undo"]);
  });

  it("retains acceptance after refresh failure and acknowledges only a successful refresh retry", async () => {
    const f = fixture();
    f.calendarStore.refreshWindow.mockRejectedValueOnce(new Error("refresh unavailable"));
    await f.controller.execute(f.input, f.review, false);
    expect(f.nativeEditor.acknowledgeResult).not.toHaveBeenCalled();
    expect(f.toasts.showDeleteUndoToast).not.toHaveBeenCalled();
    expect(f.toasts.showSaveErrorToast).toHaveBeenCalledWith("error", expect.stringContaining("refresh unavailable"));
    await f.controller.retry();
    expect(f.nativeEditor.commitReviewed).toHaveBeenCalledOnce();
    expect(f.nativeEditor.retryPending).toHaveBeenCalledOnce();
    expect(f.nativeEditor.acknowledgeResult).toHaveBeenCalledExactlyOnceWith("delete");
  });

  it("keeps a changed selection open and does not renew an expired opportunity after delayed refresh", async () => {
    const f = fixture();
    f.calendarStore.refreshWindow.mockImplementationOnce(async () => {
      f.changeSelection(); f.nativeEditor.acceptedResultAgeMs = 5_000;
    });
    await f.controller.execute(f.input, f.review, false);
    expect(f.closeSession).not.toHaveBeenCalled();
    expect(f.toasts.showDeleteUndoToast).toHaveBeenCalledWith("toast", "completed", undefined, -1_000);
    await f.controller.undoCurrent();
    expect(f.nativeEditor.undoDeletion).not.toHaveBeenCalled();
  });

  it("leaves an unmounted accepted operation available for receipt and refresh recovery", async () => {
    const f = fixture();
    f.nativeEditor.commitReviewed.mockImplementationOnce(async () => { f.unmount(); return f.receipt; });
    await f.controller.execute(f.input, f.review, false);
    expect(f.calendarStore.refreshWindow).not.toHaveBeenCalled();
    expect(f.nativeEditor.acknowledgeResult).not.toHaveBeenCalled();
    expect(f.toasts.showDeleteUndoToast).not.toHaveBeenCalled();
  });

  it("dismisses only the accepted deletion and presents Undo failure explicitly", async () => {
    const f = fixture();
    await f.controller.execute(f.input, f.review, false);
    await f.controller.dismissUndo();
    expect(dismissCalendarDeleteUndo).toHaveBeenCalledWith({ vaultId: "vault", vaultGeneration: 2, deleteCommandId: "delete" });
    await f.controller.undoCurrent();
    expect(f.nativeEditor.undoDeletion).not.toHaveBeenCalled();
    const another = fixture();
    another.nativeEditor.undoDeletion.mockRejectedValueOnce(new Error("Undo expired"));
    await another.controller.execute(another.input, another.review, false);
    await another.controller.undoCurrent();
    expect(another.toasts.showSaveErrorToast).toHaveBeenCalledWith("error", expect.stringContaining("Undo expired"));
  });
});
