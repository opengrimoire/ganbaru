import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { focusProjection } from "$lib/pomodoro/native-focus.test-helpers";
import type { FocusRequest } from "$lib/pomodoro/native-focus";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(), action: vi.fn(), read: vi.fn(),
  channels: [] as { onmessage: (value: unknown) => void }[],
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
  Channel: class {
    onmessage: (value: unknown) => void = () => {};
    constructor() { mocks.channels.push(this); }
  },
}));
import { NativeFocusClient, reportIdleOverlayVisible, buildDesktopFocusNotificationCopy } from "./focus";
import type { Translate } from "$lib/i18n/translator.svelte";

const clients: NativeFocusClient[] = [];
function client() {
  const publish = vi.fn();
  const fail = vi.fn();
  const instance = new NativeFocusClient(publish, fail);
  clients.push(instance);
  return { instance, publish, fail };
}

describe("native Focus command connection", () => {
  it("supplies only localized desktop alert text without clocks, configuration, or execution facts", () => {
    const copy = buildDesktopFocusNotificationCopy(((key: string) => `translated:${key}`) as Translate);
    expect(Object.keys(copy).sort()).toEqual([
      "dismissPromptsLabel", "endingWarningTitle", "extendFocusLabel", "pausedReminderBody",
      "pausedReminderTitle", "resumeFocusLabel",
    ]);
    expect(copy.endingWarningTitle).toBe("translated:pomodoroNotification.endingWarningTitle");
    expect(copy.resumeFocusLabel).toBe("translated:pomodoroNotification.resumeFocusLabel");
  });
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.channels.length = 0;
    mocks.read.mockResolvedValue(focusProjection());
    mocks.invoke.mockImplementation(async (command: string, args: unknown) => {
      if (command === "focus_snapshot" || command === "focus_renew_subscription") return mocks.read();
      if (command === "focus_command") return mocks.action(args);
      if (command === "focus_subscribe" || command === "focus_unsubscribe") return undefined;
      throw new Error(`Unexpected Focus command ${command}`);
    });
  });
  afterEach(async () => {
    for (const instance of clients.splice(0)) await instance.dispose();
    vi.useRealTimers();
  });

  it("retries lost replies with one unchanged identity and displayed revision", async () => {
    const { instance, publish } = client();
    await instance.initialize();
    mocks.action.mockRejectedValueOnce(new Error("response lost"))
      .mockResolvedValueOnce({ receipt: focusProjection(2).snapshot, projection: focusProjection(8) });
    await instance.command({ kind: "stop" });
    expect(mocks.action).toHaveBeenCalledTimes(2);
    expect(mocks.action.mock.calls[0]?.[0]).toEqual(mocks.action.mock.calls[1]?.[0]);
    expect(mocks.action.mock.calls[0]?.[0]).toMatchObject({ request: { command: { expectedRevision: 1 } } });
    expect(publish).toHaveBeenLastCalledWith(focusProjection(8));
  });

  it("reports a painted idle episode without supplying a visibility clock or execution intent", async () => {
    mocks.invoke.mockResolvedValue(undefined);
    const scope = { vaultGeneration: 3, runId: "run", segmentId: "segment", idleDetectedAtMs: 100_000 };
    await reportIdleOverlayVisible(scope);
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("focus_idle_overlay_visible", { scope });
  });

  it("keeps queued actions bound to the state displayed before another action completes", async () => {
    const { instance } = client();
    await instance.initialize();
    mocks.action.mockImplementation(async ({ request }: { request: FocusRequest }) => {
      if (request.command.intent.kind === "pause") return { receipt: focusProjection(2).snapshot, projection: focusProjection(2) };
      throw { code: "stale_revision", message: "Displayed revision is stale" };
    });
    const first = instance.command({ kind: "pause" });
    const second = instance.command({ kind: "advance" });
    const rejected = expect(second).rejects.toMatchObject({ code: "stale_revision" });
    await first;
    await rejected;
    expect(mocks.action).toHaveBeenCalledTimes(2);
    expect(mocks.action.mock.calls[1]?.[0]).toMatchObject({ request: { command: { expectedRevision: 1 } } });
  });

  it("refuses a detached control for an older phase before sending a command", async () => {
    const { instance } = client();
    await instance.initialize();
    await expect(instance.command({ kind: "skip_break" }, { vaultGeneration: 1, runId: "old-run", segmentId: "old-phase" })).rejects.toThrow("previous phase");
    expect(mocks.action).not.toHaveBeenCalled();
  });

  it("resolves an exhausted uncertain receipt before sending another intent", async () => {
    const { instance } = client();
    await instance.initialize();
    mocks.action.mockRejectedValueOnce(new Error("lost")).mockRejectedValueOnce(new Error("still lost"));
    await expect(instance.command({ kind: "stop" })).rejects.toThrow("still lost");
    const uncertain: unknown = mocks.action.mock.calls[0]?.[0];
    mocks.action.mockResolvedValueOnce({ receipt: focusProjection(2).snapshot, projection: focusProjection(2) })
      .mockRejectedValueOnce({ code: "stale_revision", message: "New action had an older displayed revision" });
    await expect(instance.command({ kind: "advance" })).rejects.toMatchObject({ code: "stale_revision" });
    expect(mocks.action.mock.calls[2]?.[0]).toEqual(uncertain);
    expect(mocks.action.mock.calls[3]?.[0]).toMatchObject({ request: { command: { intent: { kind: "advance" }, expectedRevision: 1 } } });
  });

  it("clears execution presentation immediately when the current generation becomes unavailable", async () => {
    const { instance, publish, fail } = client();
    await instance.initialize();
    mocks.read.mockRejectedValueOnce(new Error("native owner unavailable"));
    mocks.channels[0]?.onmessage({ vaultGeneration: 1, revision: null, available: false, hasError: true });
    expect(publish).toHaveBeenLastCalledWith({ ...focusProjection(), snapshot: null });
    await vi.waitFor(() => expect(fail).toHaveBeenCalled());
  });

  it("coalesces snapshot requests and rejects an older response without publishing it", async () => {
    const { instance, publish } = client();
    await instance.initialize();
    mocks.read.mockResolvedValueOnce(focusProjection(9));
    await instance.refresh();
    const calls = mocks.read.mock.calls.length;
    mocks.read.mockResolvedValueOnce(focusProjection(3));
    await Promise.all([instance.refresh(), instance.refresh(), instance.refresh()]);
    expect(mocks.read).toHaveBeenCalledTimes(calls + 1);
    expect(publish).toHaveBeenLastCalledWith(focusProjection(9));
  });

  it("discards stale-generation action results after a native invalidation", async () => {
    const { instance, publish } = client();
    await instance.initialize();
    let reply: ((value: unknown) => void) | undefined;
    mocks.action.mockImplementationOnce(() => new Promise<unknown>((resolve) => { reply = resolve; }));
    const action = instance.command({ kind: "stop" });
    await vi.waitFor(() => expect(mocks.action).toHaveBeenCalledTimes(1));
    mocks.read.mockResolvedValue(focusProjection(1, 2));
    mocks.channels[0]?.onmessage({ vaultGeneration: 2, revision: 1, available: true, hasError: false });
    await vi.waitFor(() => expect(publish).toHaveBeenLastCalledWith(focusProjection(1, 2)));
    reply?.({ receipt: focusProjection(2).snapshot, projection: focusProjection(2) });
    await action;
    expect(publish).toHaveBeenLastCalledWith(focusProjection(1, 2));
  });

  it("releases a failed initial subscription and can reconnect", async () => {
    const { instance } = client();
    mocks.read.mockRejectedValueOnce(new Error("initial snapshot failed"));
    await expect(instance.initialize()).rejects.toThrow("initial snapshot failed");
    expect(mocks.invoke).toHaveBeenCalledWith("focus_unsubscribe", { subscriptionId: expect.any(String) });
    await instance.initialize();
    expect(mocks.invoke.mock.calls.filter(([command]) => command === "focus_subscribe")).toHaveLength(2);
  });
});
