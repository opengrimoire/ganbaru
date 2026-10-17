import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CalendarCommitFailure, type CalendarEditPreview, type CalendarPreviewRequest } from "$lib/api/calendar-edit";
import { NativeCalendarEditController, type CalendarEditDraft } from "./native-edit-controller.svelte";

const authority = { vaultId: "vault", vaultGeneration: 2 };

function draft(title = "Latest", sessionKey = 1): CalendarEditDraft {
  return { ...authority, sessionKey,
    edit: { selection: { templateId: "source", recurrenceDate: "2026-05-15", scope: "this" },
      draft: { fields: [{ field: "title", value: title }] } },
    window: { windowStartDate: "2026-05-11", windowEndDate: "2026-05-17", renderZone: "UTC", includeTotalEventCount: false },
  };
}

function preview(request: CalendarPreviewRequest): CalendarEditPreview {
  return { ...authority, commandId: request.commandId, sourceId: "source", editedId: request.commandId,
    reviewRevision: "a".repeat(64), changed: true,
    scope: { effectiveScope: "this", selectedActive: false, selectedStarted: false, selectedHasHistory: false },
    window: { sourceEvents: [], windowEvents: [], totalEventCount: null, diagnostics: [] },
    previewedIds: new Set(), editingId: undefined,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

function identifiers() {
  let count = 0;
  return () => `edit-${++count}`;
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

describe("native Calendar edit lifetime", () => {
  it("keeps deletion confirmation bound to the original review and retries its exact stop intent", async () => {
    const input: CalendarEditDraft = { ...draft(), edit: { kind: "delete",
      selection: { templateId: "source", recurrenceDate: "2026-05-15", scope: "all" }, stopActive: false } };
    const load = vi.fn(async (request: CalendarPreviewRequest) => ({ ...preview(request),
      deletion: { outcome: "archive" as const, requiresActiveStop: true, historyOnly: false } }));
    const accepted = { commandId: "edit-1", editedId: "source", changed: true, preservedIds: [],
      undoReviewRevision: "b".repeat(64), undoAvailableForMs: 3_000 };
    const write = vi.fn().mockRejectedValueOnce(new Error("Lost deletion response")).mockResolvedValue(accepted);
    const controller = new NativeCalendarEditController(() => authority, load, write, identifiers());
    const reviewed = await controller.reviewDeletion(input);
    await expect(controller.commitReviewed(input, reviewed, true)).rejects.toThrow("Lost deletion response");
    expect(write.mock.calls[0][0]).toMatchObject({ commandId: reviewed.commandId, reviewRevision: reviewed.reviewRevision,
      edit: { kind: "delete", stopActive: true } });
    expect(controller.pendingKind).toBe("delete");
    controller.reset();
    await expect(controller.retryPending()).resolves.toEqual(accepted);
    expect(write.mock.calls[1][0]).toEqual(write.mock.calls[0][0]);
    expect(controller.resultPending).toBe(true);
    controller.reset();
    await expect(controller.retryPending()).resolves.toEqual(accepted);
    expect(write).toHaveBeenCalledTimes(2);
    expect(load).toHaveBeenCalledOnce();
    controller.acknowledgeResult("another");
    expect(controller.resultPending).toBe(true);
    controller.acknowledgeResult(accepted.commandId);
    expect(controller.resultPending).toBe(false);
  });

  it("rejects a changed deletion selection after confirmation without refreshing consent onto another run", async () => {
    const input: CalendarEditDraft = { ...draft(), edit: { kind: "delete",
      selection: { templateId: "source", recurrenceDate: "2026-05-15", scope: "all" }, stopActive: false } };
    const load = vi.fn(async (request: CalendarPreviewRequest) => ({ ...preview(request),
      deletion: { outcome: "archive" as const, requiresActiveStop: true, historyOnly: false } }));
    const write = vi.fn();
    const controller = new NativeCalendarEditController(() => authority, load, write, identifiers());
    const reviewed = await controller.reviewDeletion(input);
    controller.update({ ...input, sessionKey: 2 });
    await expect(controller.commitReviewed(input, reviewed, true)).rejects.toThrow("selection changed");
    expect(write).not.toHaveBeenCalled();
    expect(load).toHaveBeenCalledOnce();
  });

  it("retries an accepted Undo across expiry and remount without requesting another preview", async () => {
    const input: CalendarEditDraft = { ...draft(), edit: { kind: "delete",
      selection: { templateId: "source", recurrenceDate: "2026-05-15", scope: "all" }, stopActive: false } };
    const load = vi.fn(async (request: CalendarPreviewRequest) => ({ ...preview(request),
      deletion: { outcome: "delete" as const, requiresActiveStop: false, historyOnly: false } }));
    const write = vi.fn().mockRejectedValueOnce(new Error("Lost Undo response"))
      .mockResolvedValue({ commandId: "edit-2", editedId: "source", changed: true, preservedIds: [] });
    const controller = new NativeCalendarEditController(() => authority, load, write, identifiers());
    const review = await controller.reviewDeletion(input);
    const deletion = { commandId: "delete", editedId: "source", changed: true, preservedIds: [], undoReviewRevision: "b".repeat(64) };
    await expect(controller.undoDeletion(review, deletion)).rejects.toThrow("Lost Undo response");
    controller.reset();
    await vi.advanceTimersByTimeAsync(10_000);
    await controller.retryPending();
    expect(write.mock.calls[1][0]).toEqual(write.mock.calls[0][0]);
    expect(write.mock.calls[0][0]).toMatchObject({ reviewRevision: deletion.undoReviewRevision,
      edit: { kind: "undo_delete", deleteCommandId: "delete" } });
    expect(controller.pendingKind).toBe("undo_delete");
    expect(load).toHaveBeenCalledOnce();
  });
  it("retains the exact reviewed creation after a lost Save response and refresh-only retry", async () => {
    const input: CalendarEditDraft = { ...draft(), edit: { kind: "create", draft: {
      timing: { startTime: "2026-03-08T02:00", endTime: "2026-03-08T02:30", timezone: "America/New_York" },
      recurrence: { kind: "set", value: "FREQ=DAILY;COUNT=3" },
    } } };
    const receipt = { commandId: "edit-1", editedId: "native-created", changed: true, preservedIds: [] };
    const load = vi.fn(async (request: CalendarPreviewRequest) => preview(request));
    const write = vi.fn().mockRejectedValueOnce(new Error("Response lost")).mockResolvedValue(receipt);
    const controller = new NativeCalendarEditController(() => authority, load, write, identifiers());
    await expect(controller.commit(input)).rejects.toThrow("Response lost");
    expect(controller.uncertain).toBe(true);
    expect(controller.pendingAction).toBeUndefined();
    await expect(controller.commit(input)).resolves.toEqual(receipt);
    expect(write.mock.calls[1][0]).toEqual(write.mock.calls[0][0]);
    expect(write.mock.calls[1][0].edit).toEqual(input.edit);
    await expect(controller.commit(input)).resolves.toEqual(receipt);
    expect(write).toHaveBeenCalledTimes(2);
    expect(load).toHaveBeenCalledOnce();
  });

  it("gives a changed creation draft a separate preview identity while retaining the uncertain original", async () => {
    const input: CalendarEditDraft = { ...draft(), edit: { kind: "create", draft: { fields: [{ field: "title", value: "Original" }] } } };
    const load = vi.fn(async (request: CalendarPreviewRequest) => preview(request));
    const write = vi.fn().mockRejectedValue(new Error("Response lost"));
    const controller = new NativeCalendarEditController(() => authority, load, write, identifiers());
    await expect(controller.commit(input)).rejects.toThrow("Response lost");
    const original = write.mock.calls[0][0];
    const changed: CalendarEditDraft = { ...input,
      edit: { kind: "create", draft: { fields: [{ field: "title", value: "Changed" }] } } };
    const changedPreview = await controller.review(changed);
    expect(changedPreview.commandId).not.toBe(original.commandId);
    await expect(controller.commit(changed)).rejects.toThrow("previous Save is unresolved");
    await expect(controller.retryPending()).rejects.toThrow("Response lost");
    expect(write.mock.calls[1][0]).toEqual(original);
  });

  it("hides a previous contour immediately when input changes before the update effect", async () => {
    const controller = new NativeCalendarEditController(() => authority,
      async (request) => preview(request), undefined, identifiers());
    const input = draft("Reviewed");
    const result = await controller.review(input);
    expect(controller.previewFor(input)).toBe(result);
    expect(controller.previewFor(draft("New title"))).toBeNull();
    expect(controller.previewFor({ ...input, vaultGeneration: 3 })).toBeNull();
    expect(controller.previewFor(draft("Reviewed", 2))).toBeNull();
  });

  it("debounces rapid input and retains only the latest draft behind an active preview", async () => {
    const first = deferred<CalendarEditPreview>();
    const load = vi.fn(async (request: CalendarPreviewRequest) => preview(request))
      .mockImplementationOnce(() => first.promise);
    const controller = new NativeCalendarEditController(() => authority, load, undefined, identifiers());
    controller.update(draft("Before debounce"));
    controller.update(draft("First admitted"));
    await vi.advanceTimersByTimeAsync(200);
    expect(load).toHaveBeenCalledOnce();
    for (let index = 0; index < 50; index++) controller.update(draft(`Pending ${index}`));
    await vi.advanceTimersByTimeAsync(1000);
    expect(load).toHaveBeenCalledOnce();
    first.resolve(preview(load.mock.calls[0][0]));
    await vi.advanceTimersByTimeAsync(0);
    expect(controller.preview).toBeNull();
    await vi.advanceTimersByTimeAsync(200);
    expect(load).toHaveBeenCalledTimes(2);
    expect(load.mock.calls[1][0].edit).toEqual(draft("Pending 49").edit);
    expect(controller.preview?.commandId).toBe(load.mock.calls[1][0].commandId);
  });

  it("rejects a late same-vault response after its native generation changes", async () => {
    let current = { ...authority };
    const delayed = deferred<CalendarEditPreview>();
    const load = vi.fn((_request: CalendarPreviewRequest) => delayed.promise);
    const controller = new NativeCalendarEditController(() => current, load, undefined, identifiers());
    const pending = controller.review(draft());
    current = { ...authority, vaultGeneration: 3 };
    delayed.resolve(preview(load.mock.calls[0][0]));
    await expect(pending).rejects.toThrow("vault changed");
    expect(controller.preview).toBeNull();
    expect(load).toHaveBeenCalledOnce();
  });

  it("cannot save a newer session using an older review still in flight", async () => {
    const delayed = deferred<CalendarEditPreview>();
    const load = vi.fn((_request: CalendarPreviewRequest) => delayed.promise);
    const write = vi.fn();
    const controller = new NativeCalendarEditController(() => authority, load, write, identifiers());
    const saving = controller.commit(draft("Old", 1));
    controller.update(draft("New", 2));
    delayed.resolve(preview(load.mock.calls[0][0]));
    await expect(saving).rejects.toThrow("editor or vault changed");
    expect(write).not.toHaveBeenCalled();
  });

  it("retains the exact request across lost replies, panel close and native generation changes", async () => {
    let current = { ...authority };
    const load = vi.fn(async (request: CalendarPreviewRequest) => preview(request));
    const receipt = { commandId: "edit-1", editedId: "edited", changed: true, preservedIds: [] };
    const write = vi.fn().mockRejectedValueOnce(new Error("Lost response")).mockResolvedValueOnce(receipt);
    const controller = new NativeCalendarEditController(() => current, load, write, identifiers());
    await expect(controller.commit(draft())).rejects.toThrow("Lost response");
    const original = structuredClone(write.mock.calls[0][0]);
    expect(controller.uncertain).toBe(true);
    expect(controller.matchesPending(draft())).toBe(true);
    expect(controller.matchesPending(draft("Changed"))).toBe(false);
    expect(controller.matchesPending(draft("Latest", 2))).toBe(false);
    await expect(controller.commit(draft("Changed"))).rejects.toThrow("original draft");
    controller.reset();
    current = { ...authority, vaultGeneration: 3 };
    await expect(controller.retryPending()).resolves.toEqual(receipt);
    expect(write.mock.calls[1][0]).toEqual(original);
    expect(load).toHaveBeenCalledOnce();
    expect(controller.uncertain).toBe(false);
  });

  it("keeps an uncertain request while a different vault is selected", async () => {
    let current = { ...authority };
    const load = vi.fn(async (request: CalendarPreviewRequest) => preview(request));
    const write = vi.fn().mockRejectedValue(new CalendarCommitFailure("unknown", "Owner stopped"));
    const controller = new NativeCalendarEditController(() => current, load, write, identifiers());
    await expect(controller.commit(draft())).rejects.toThrow("Owner stopped");
    current = { vaultId: "other", vaultGeneration: 3 };
    await expect(controller.retryPending()).rejects.toThrow("original Calendar vault");
    expect(controller.uncertain).toBe(true);
    expect(write).toHaveBeenCalledOnce();
  });

  it("resolves the original receipt after execution context disappears in a read-only vault", async () => {
    let current: typeof authority | null = { ...authority };
    let activeVault: string | null = "vault";
    const load = vi.fn(async (request: CalendarPreviewRequest) => preview(request));
    const receipt = { commandId: "edit-1", editedId: "edited", changed: true, preservedIds: [] };
    const write = vi.fn().mockRejectedValueOnce(new Error("Lost reply")).mockResolvedValueOnce(receipt);
    const controller = new NativeCalendarEditController(() => current, load, write, identifiers(), () => activeVault);
    await expect(controller.commit(draft())).rejects.toThrow("Lost reply");
    const original = structuredClone(write.mock.calls[0][0]);
    current = null;
    activeVault = "other";
    await expect(controller.retryPending()).rejects.toThrow("original Calendar vault");
    activeVault = "vault";
    await expect(controller.retryPending()).resolves.toEqual(receipt);
    expect(write.mock.calls[1][0]).toEqual(original);
    expect(load).toHaveBeenCalledOnce();
    expect(controller.uncertain).toBe(false);
    expect(controller.previewFor(draft())).toBeNull();
  });

  it("refreshes review with a new command after a confirmed rejection", async () => {
    const load = vi.fn(async (request: CalendarPreviewRequest) => preview(request));
    const write = vi.fn().mockRejectedValueOnce(new CalendarCommitFailure("rejected", "Source changed"))
      .mockResolvedValueOnce({ commandId: "edit-2", editedId: "edited", changed: true, preservedIds: [] });
    const controller = new NativeCalendarEditController(() => authority, load, write, identifiers());
    await expect(controller.commit(draft())).rejects.toThrow("Source changed");
    expect(controller.uncertain).toBe(false);
    await controller.commit(draft());
    expect(load).toHaveBeenCalledTimes(2);
    expect(write.mock.calls[1][0].commandId).not.toBe(write.mock.calls[0][0].commandId);
  });

  it("does not write again when refreshing the calendar after a confirmed Save failed", async () => {
    const load = vi.fn(async (request: CalendarPreviewRequest) => preview(request));
    const receipt = { commandId: "edit-1", editedId: "edited", changed: true, preservedIds: [] };
    const write = vi.fn().mockResolvedValue(receipt);
    const controller = new NativeCalendarEditController(() => authority, load, write, identifiers());
    await expect(controller.commit(draft())).resolves.toEqual(receipt);
    await expect(controller.commit(draft())).resolves.toEqual(receipt);
    expect(write).toHaveBeenCalledOnce();
    expect(load).toHaveBeenCalledOnce();
  });
});
