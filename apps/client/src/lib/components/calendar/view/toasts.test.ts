import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createCalendarViewToastController } from "./toasts.svelte";

describe("Calendar view toasts", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("goes straight to the saved outcome when a save settles before the pending delay", () => {
    const toasts = createCalendarViewToastController();
    const id = toasts.showSavePendingToast("Saving");
    expect(toasts.saveToast).toBeNull();
    vi.advanceTimersByTime(200);
    toasts.showSaveSuccessToast(id, "Saved");
    expect(toasts.saveToast).toMatchObject({ pending: false, message: "Saved" });
    vi.advanceTimersByTime(1_000);
    expect(toasts.saveToast).toMatchObject({ pending: false, message: "Saved" });
    toasts.destroy();
  });

  it("shows a slow save as pending once the delay passes", () => {
    const toasts = createCalendarViewToastController();
    toasts.showSavePendingToast("Saving");
    vi.advanceTimersByTime(499);
    expect(toasts.saveToast).toBeNull();
    vi.advanceTimersByTime(1);
    expect(toasts.saveToast).toMatchObject({ pending: true, message: "Saving" });
    toasts.destroy();
  });

  it("shows an immediate failure without waiting for the pending delay", () => {
    const toasts = createCalendarViewToastController();
    const id = toasts.showSavePendingToast("Delete failed");
    toasts.showSaveErrorToast(id, "Delete failed");
    expect(toasts.saveToast).toMatchObject({ pending: false, variant: "error" });
    toasts.destroy();
  });

  it("restarts the delay for each deletion and never reveals a dismissed pending deletion", () => {
    const toasts = createCalendarViewToastController();
    const first = toasts.showDeletePendingToast("Deleting");
    vi.advanceTimersByTime(600);
    expect(toasts.deleteUndoToast).toMatchObject({ id: first, pending: true });
    toasts.showDeleteUndoToast(first, "Deleted");
    const second = toasts.showDeletePendingToast("Deleting");
    expect(toasts.deleteUndoToast).toBeNull();
    toasts.dismissDeleteToastIfPending(second);
    vi.advanceTimersByTime(600);
    expect(toasts.deleteUndoToast).toBeNull();
    toasts.destroy();
  });
});
