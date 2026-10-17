import { describe, expect, it, vi } from "vitest";
import { CalendarCommitFailure, type CalendarCommitReceipt, type CalendarEditPreview, type CalendarTaskScheduleIntent } from "$lib/api/calendar-edit";
import { ProjectSchedulingController } from "./controller";

const intent: CalendarTaskScheduleIntent = { kind: "schedule_tasks", projectId: "project",
  tasks: [{ id: "task", revision: 3 }], startTime: "2099-05-10T09:00", timezone: "UTC",
  durationMinutes: 30, globalIdleTimeoutMinutes: 5 };
const window = { windowStartDate: "2099-05-10", windowEndDate: "2099-05-10", renderZone: "UTC", includeTotalEventCount: false };
const identities = [{ taskId: "task", eventId: `calendar-schedule-${"a".repeat(64)}` }];
const receipt: CalendarCommitReceipt = { commandId: "schedule-1", editedId: identities[0]!.eventId,
  changed: true, preservedIds: [], scheduledTasks: identities };
const preview: CalendarEditPreview = { vaultId: "vault", vaultGeneration: 1, commandId: "schedule-1",
  editedId: receipt.editedId, sourceId: receipt.editedId, reviewRevision: "b".repeat(64), changed: true,
  scope: { effectiveScope: "this", selectedStarted: false, selectedHasHistory: false, selectedActive: false },
  window: { sourceEvents: [], windowEvents: [], diagnostics: [], totalEventCount: null },
  previewedIds: new Set(), editingId: undefined, scheduledTasks: identities };

function setup() {
  let vault = "vault";
  const read = vi.fn(async () => structuredClone(preview));
  const write = vi.fn(async () => structuredClone(receipt));
  const controller = new ProjectSchedulingController(() => vault, read, write, () => "schedule-1");
  return { controller, read, write, changeVault: (next: string) => { vault = next; } };
}

describe("native Project scheduling recovery", () => {
  it("sends only semantic selection and retains acceptance until cache acknowledgment", async () => {
    const { controller, read, write } = setup();
    const accepted = await controller.schedule(intent, window);
    expect(accepted).toEqual(receipt);
    accepted.scheduledTasks![0]!.taskId = "changed by a caller";
    expect(read).toHaveBeenCalledWith({ commandId: "schedule-1", edit: intent, window });
    expect(write).toHaveBeenCalledWith({ vaultId: "vault", vaultGeneration: 1,
      commandId: "schedule-1", reviewRevision: "b".repeat(64), edit: intent });
    expect(controller.recoverable).toBe(true);
    expect(await controller.retry()).toEqual(receipt);
    await expect(controller.schedule({ ...intent, durationMinutes: 90 }, window)).rejects.toThrow("accepted schedule");
    expect(await controller.schedule(intent, window)).toEqual(receipt);
    expect(write).toHaveBeenCalledTimes(1);
    controller.acknowledge("different-command");
    expect(controller.recoverable).toBe(true);
    controller.acknowledge(receipt.commandId);
    expect(controller.recoverable).toBe(false);
  });

  it("retries the exact immutable request after a lost response and rejects another selection", async () => {
    const { controller, read, write } = setup();
    write.mockRejectedValueOnce(new Error("lost response"));
    const draft = structuredClone(intent);
    await expect(controller.schedule(draft, window)).rejects.toThrow("lost response");
    draft.tasks[0]!.revision = 9;
    draft.durationMinutes = 90;
    await expect(controller.schedule(draft, window)).rejects.toThrow("unresolved");
    expect(await controller.retry()).toEqual(receipt);
    expect(write.mock.calls[1]).toEqual(write.mock.calls[0]);
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("discards proven rejection and obtains a fresh native review", async () => {
    const { controller, read, write } = setup();
    write.mockRejectedValueOnce(new CalendarCommitFailure("rejected", "stale task"));
    await expect(controller.schedule(intent, window)).rejects.toThrow("stale task");
    expect(controller.recoverable).toBe(false);
    await controller.schedule(intent, window);
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("keeps unknown native results and malformed receipt identities recoverable", async () => {
    for (const result of [new CalendarCommitFailure("unknown", "timeout"), { ...receipt, scheduledTasks: [{ taskId: "other", eventId: receipt.editedId }] }]) {
      const { controller, write } = setup();
      if (result instanceof Error) write.mockRejectedValueOnce(result);
      else write.mockResolvedValueOnce(result);
      await expect(controller.schedule(intent, window)).rejects.toThrow();
      expect(controller.recoverable).toBe(true);
      expect(await controller.retry()).toEqual(receipt);
    }
  });

  it("does not commit a preview from another vault and retains original vault recovery", async () => {
    const { controller, read, write, changeVault } = setup();
    read.mockImplementationOnce(async () => { changeVault("other"); return preview; });
    await expect(controller.schedule(intent, window)).rejects.toThrow("vault changed");
    expect(write).not.toHaveBeenCalled();
    changeVault("vault");
    write.mockRejectedValueOnce(new Error("lost"));
    await expect(controller.schedule(intent, window)).rejects.toThrow("lost");
    changeVault("other");
    await expect(controller.retry()).rejects.toThrow("original scheduling vault");
    expect(controller.recoverable).toBe(true);
    changeVault("vault");
    await controller.retry();
  });

  it("admits only one preview and commit while a request is in flight", async () => {
    const { controller, read } = setup();
    let finish: (value: CalendarEditPreview) => void = () => { throw new Error("missing preview waiter"); };
    read.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const first = controller.schedule(intent, window);
    await expect(controller.schedule(intent, window)).rejects.toThrow("already in progress");
    finish(preview);
    await first;
    expect(read).toHaveBeenCalledTimes(1);
  });
});
