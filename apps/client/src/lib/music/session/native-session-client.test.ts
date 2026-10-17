import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NativeMusicSnapshot } from "./native-session";

const mocks = vi.hoisted(() => ({
  command: vi.fn(), snapshot: vi.fn(), start: vi.fn(), host: vi.fn(), observe: vi.fn(),
  subscribe: vi.fn(), readFrame: vi.fn(), acknowledge: vi.fn(), unsubscribe: vi.fn(),
  channels: [] as { onmessage: (value: unknown) => void }[],
  frames: new Map<string, unknown>(),
}));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    constructor(public onmessage: (value: unknown) => void) { mocks.channels.push(this); }
  },
}));
vi.mock("$lib/api/music-session", () => ({
  musicSessionCommand: mocks.command, musicSessionStart: mocks.start,
  musicSessionSnapshot: mocks.snapshot, musicSessionObserve: mocks.observe, musicSessionHost: mocks.host,
  musicSessionSubscribe: mocks.subscribe, musicSessionAcknowledge: mocks.acknowledge, musicSessionUnsubscribe: mocks.unsubscribe,
  musicSessionReadFrame: mocks.readFrame,
}));
import { NativeMusicSessionClient } from "./native-session-client";

function snapshot(revision = 1, generation = 1): NativeMusicSnapshot {
  return { soundscapeVersion: null, sessionId: "session", revision, generation, queueRevision: 1, currentIndex: null,
    currentSource: null, backend: null, status: "idle", positionMs: 0, durationMs: null,
    volume: 1, muted: false, rate: 1, order: "in-order", repeatMode: "off", owner: "manual",
    context: null, playlistId: null, queueName: "", canPrevious: false, canNext: false,
    issue: null, error: null, reviewCheckpointId: null, queue: [] };
}

function frame(sequence: number, value = snapshot(), effects: unknown[] = [], effectsExpireAtMs = Date.now() + 5_000) {
  return { subscriptionId: mocks.subscribe.mock.calls.at(-1)?.[0] as unknown, sequence, snapshot: value, effects, effectsExpireAtMs };
}

function send(channel: { onmessage: (value: unknown) => void } | undefined, value: ReturnType<typeof frame>): void {
  mocks.frames.set(`${String(value.subscriptionId)}:${value.sequence}`, value);
  channel?.onmessage({ kind: "update", subscriptionId: value.subscriptionId, sequence: value.sequence });
}

function receive(value: ReturnType<typeof frame>): void { send(mocks.channels.at(-1), value); }

describe("native Music command projection", () => {
  beforeEach(() => {
    vi.resetAllMocks(); mocks.channels.length = 0; mocks.frames.clear();
    mocks.host.mockResolvedValue(undefined); mocks.unsubscribe.mockResolvedValue(undefined); mocks.acknowledge.mockResolvedValue(undefined);
    mocks.readFrame.mockImplementation(async (id: string, sequence: number) => mocks.frames.get(`${id}:${sequence}`));
    mocks.subscribe.mockImplementation(async (_id: string, channel: { onmessage: (value: unknown) => void }) => {
      send(channel, frame(1));
    });
  });

  it("retries an uncertain action with exactly the same receipt identity", async () => {
    const apply = vi.fn();
    const client = new NativeMusicSessionClient({ snapshot: apply, effect: async () => undefined, error: vi.fn() });
    await client.connect();
    await vi.waitFor(() => expect(apply).toHaveBeenCalledTimes(1));
    mocks.command.mockRejectedValueOnce(new Error("response lost")).mockResolvedValueOnce(snapshot(2));
    await client.command({ kind: "next" });
    expect(mocks.command).toHaveBeenCalledTimes(2);
    expect(mocks.command.mock.calls[0]?.[0]).toEqual(mocks.command.mock.calls[1]?.[0]);
    expect(apply).toHaveBeenLastCalledWith(snapshot(2));
  });

  it("resolves a retained uncertain action before sending a different intent", async () => {
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect: async () => undefined, error: vi.fn() });
    mocks.command.mockRejectedValueOnce(new Error("lost")).mockRejectedValueOnce(new Error("still lost"));
    await expect(client.command({ kind: "next" })).rejects.toThrow("still lost");
    const pending: unknown = mocks.command.mock.calls[0]?.[0];
    mocks.command.mockResolvedValueOnce(snapshot(2)).mockResolvedValueOnce(snapshot(3));
    await client.command({ kind: "pause" });
    expect(mocks.command.mock.calls[2]?.[0]).toEqual(pending);
    expect(mocks.command.mock.calls[3]?.[0]).toMatchObject({ intent: { kind: "pause" } });
  });

  it("does not replay pending or queued old-vault intent after reset", async () => {
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect: async () => undefined, error: vi.fn() });
    let resolveOld: ((value: NativeMusicSnapshot) => void) | undefined;
    mocks.command.mockImplementationOnce(() => new Promise<NativeMusicSnapshot>((resolve) => { resolveOld = resolve; }));
    const old = client.command({ kind: "next" });
    const queued = client.command({ kind: "play" });
    const oldResult = expect(old).rejects.toThrow("vault changed");
    const queuedResult = expect(queued).rejects.toThrow("vault changed");
    await Promise.resolve(); await Promise.resolve();
    client.reset();
    mocks.command.mockResolvedValueOnce(snapshot(1));
    await client.command({ kind: "pause" });
    resolveOld?.(snapshot(2));
    await oldResult; await queuedResult;
    expect(mocks.command).toHaveBeenCalledTimes(2);
    expect(mocks.command.mock.calls[1]?.[0]).toMatchObject({ intent: { kind: "pause" } });
  });

  it("drops old observations and effects without replacing a newer projection", async () => {
    const apply = vi.fn(); const effect = vi.fn(async () => undefined);
    const client = new NativeMusicSessionClient({ snapshot: apply, effect, error: vi.fn() });
    await client.connect();
    receive(frame(2, snapshot(4, 3)));
    receive(frame(3, snapshot(2, 2), [{ kind: "pause", generation: 2 }]));
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledWith(expect.any(String), 3));
    expect(apply).toHaveBeenCalledTimes(2);
    expect(apply).toHaveBeenLastCalledWith(snapshot(4, 3));
    expect(effect).not.toHaveBeenCalled();
    client.reset();
    receive(frame(4, snapshot(10, 10)));
    await Promise.resolve(); await Promise.resolve();
    expect(apply).toHaveBeenCalledTimes(2);
  });

  it("consumes browser effects in order before acknowledging their frame", async () => {
    let finish: (() => void) | undefined;
    const effect = vi.fn().mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; })).mockResolvedValue(undefined);
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect, error: vi.fn() });
    await client.connect();
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledTimes(1));
    receive(frame(2, snapshot(2), [{ kind: "pause", generation: 1 }, { kind: "seek", generation: 1, positionMs: 200 }]));
    await vi.waitFor(() => expect(effect).toHaveBeenCalledTimes(1));
    expect(mocks.acknowledge).toHaveBeenCalledTimes(1);
    finish?.();
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledWith(expect.any(String), 2));
    expect(effect.mock.calls).toEqual([[{ kind: "pause", generation: 1 }], [{ kind: "seek", generation: 1, positionMs: 200 }]]);
  });

  it("retries a lost acknowledgement on heartbeat without replaying browser effects", async () => {
    const effect = vi.fn(async () => undefined); const error = vi.fn();
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect, error });
    await client.connect();
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledTimes(1));
    mocks.acknowledge.mockRejectedValueOnce(new Error("ACK response lost"));
    receive(frame(2, snapshot(2), [{ kind: "play", generation: 1 }]));
    await vi.waitFor(() => expect(error).toHaveBeenCalledTimes(1));
    const pending: unknown = mocks.acknowledge.mock.calls.at(-1);
    await client.host(true);
    expect(mocks.acknowledge.mock.calls.at(-1)).toEqual(pending);
    expect(effect).toHaveBeenCalledTimes(1);
  });

  it("acknowledges a duplicate frame without applying its effects again", async () => {
    const effect = vi.fn(async () => undefined);
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect, error: vi.fn() });
    await client.connect();
    const value = frame(2, snapshot(2), [{ kind: "play", generation: 1 }]);
    receive(value); receive(value);
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledTimes(3));
    expect(effect).toHaveBeenCalledTimes(1);
  });

  it("stops expired browser work instead of starting playback after suspension", async () => {
    const effect = vi.fn(async () => undefined);
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect, error: vi.fn() });
    await client.connect();
    receive(frame(2, snapshot(2, 3), [{ kind: "play", generation: 3 }], Date.now() - 1));
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledWith(expect.any(String), 2));
    expect(effect.mock.calls).toEqual([[{ kind: "stop", generation: 3 }]]);
  });

  it("cleans up a subscription whose reply arrives after the vault resets", async () => {
    let finish: (() => void) | undefined;
    mocks.subscribe.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; }));
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect: async () => undefined, error: vi.fn() });
    const pending = client.connect();
    const id: unknown = mocks.subscribe.mock.calls[0]?.[0];
    client.reset();
    await client.connect();
    finish?.();
    await pending;
    expect(mocks.unsubscribe.mock.calls).toEqual([[id], [id]]);
    expect(mocks.host).not.toHaveBeenCalledWith(id, true);
  });

  it("disconnects malformed or skipped frames before executing browser work", async () => {
    const effect = vi.fn(async () => undefined); const error = vi.fn();
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect, error });
    await client.connect();
    receive(frame(3, snapshot(2), [{ kind: "play", generation: 1 }]));
    await vi.waitFor(() => expect(error).toHaveBeenCalledTimes(1));
    expect(effect).not.toHaveBeenCalled();
    expect(mocks.unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("replaces an expired native subscription when the WebView becomes available", async () => {
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect: async () => undefined, error: vi.fn() });
    await client.connect();
    mocks.host.mockRejectedValueOnce({ code: "not-found", message: "subscription expired" });
    await client.host(true);
    expect(mocks.subscribe).toHaveBeenCalledTimes(2);
    expect(mocks.subscribe.mock.calls[0]?.[0]).not.toEqual(mocks.subscribe.mock.calls[1]?.[0]);
    expect(mocks.unsubscribe).toHaveBeenCalledWith(mocks.subscribe.mock.calls[0]?.[0]);
  });

  it("revokes browser availability and stops the decoder after an adapter failure", async () => {
    const error = vi.fn();
    const effect = vi.fn().mockRejectedValueOnce(new Error("decoder failed")).mockResolvedValue(undefined);
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect, error });
    await client.connect();
    receive(frame(2, snapshot(2), [{ kind: "play", generation: 1 }]));
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledWith(expect.any(String), 2));
    expect(effect).toHaveBeenLastCalledWith({ kind: "stop", generation: 1 });
    expect(mocks.host).toHaveBeenLastCalledWith(expect.any(String), false);
    expect(error).toHaveBeenCalledTimes(1);
  });

  it("retries a lost frame read on heartbeat before acknowledging or applying effects", async () => {
    const effect = vi.fn(async () => undefined); const error = vi.fn();
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect, error });
    await client.connect();
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledTimes(1));
    mocks.readFrame.mockRejectedValueOnce(new Error("frame response lost"));
    receive(frame(2, snapshot(2), [{ kind: "play", generation: 1 }]));
    await vi.waitFor(() => expect(error).toHaveBeenCalledTimes(1));
    expect(effect).not.toHaveBeenCalled();
    expect(mocks.acknowledge).toHaveBeenCalledTimes(1);
    await client.host(true);
    expect(effect).toHaveBeenCalledTimes(1);
    expect(mocks.readFrame.mock.calls.at(-2)).toEqual(mocks.readFrame.mock.calls.at(-1));
    expect(mocks.acknowledge).toHaveBeenLastCalledWith(expect.any(String), 2);
  });

  it("rejects a retained frame that does not match the subscription notice", async () => {
    const effect = vi.fn(async () => undefined); const error = vi.fn();
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect, error });
    await client.connect();
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledTimes(1));
    mocks.readFrame.mockResolvedValueOnce({ ...frame(2), subscriptionId: "other" });
    receive(frame(2, snapshot(2), [{ kind: "play", generation: 1 }]));
    await vi.waitFor(() => expect(error).toHaveBeenCalledTimes(1));
    expect(effect).not.toHaveBeenCalled();
    expect(mocks.unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("interrupts old browser work on context invalidation before the in-flight frame finishes", async () => {
    let finish: (() => void) | undefined;
    const effect = vi.fn().mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; })).mockResolvedValue(undefined);
    const resetProjection = vi.fn();
    const client = new NativeMusicSessionClient({ snapshot: vi.fn(), effect, error: vi.fn(), resetProjection });
    await client.connect();
    await vi.waitFor(() => expect(mocks.acknowledge).toHaveBeenCalledTimes(1));
    receive(frame(2, snapshot(2), [{ kind: "pause", generation: 1 }]));
    await vi.waitFor(() => expect(effect).toHaveBeenCalledTimes(1));
    const id: unknown = mocks.subscribe.mock.calls[0]?.[0];
    mocks.channels[0]?.onmessage({ kind: "invalidate", subscriptionId: id });
    expect(effect).toHaveBeenLastCalledWith({ kind: "stop", generation: 1 });
    expect(mocks.unsubscribe).toHaveBeenCalledWith(id);
    expect(resetProjection).toHaveBeenCalledTimes(1);
    finish?.();
    await Promise.resolve(); await Promise.resolve();
    expect(mocks.acknowledge).toHaveBeenCalledTimes(1);
    mocks.channels[0]?.onmessage({ kind: "update", subscriptionId: id, sequence: 3 });
    await Promise.resolve();
    expect(mocks.readFrame).toHaveBeenCalledTimes(2);
  });
});
