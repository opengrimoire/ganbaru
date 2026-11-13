import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatQuietStateRead } from "$lib/chat/contracts";
import { CHAT_QUIET_ERROR_RETRY_MS, ChatQuietStatus } from "./quiet-status.svelte";

const notQuiet: ChatQuietStateRead = { quiet: false, reason: null, endsAt: null };

describe("ChatQuietStatus", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-12T10:00:00Z"));
    vi.stubGlobal("window", new EventTarget());
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("is unknown until the first read completes", async () => {
    let finish: (read: ChatQuietStateRead) => void = () => {};
    const read = vi.fn(() => new Promise<ChatQuietStateRead>((resolve) => {
      finish = resolve;
    }));
    const status = new ChatQuietStatus({ read });
    const listener = vi.fn();
    const release = status.use(listener);
    await vi.advanceTimersByTimeAsync(0);
    expect(status.state).toBeNull();

    finish(notQuiet);
    await vi.advanceTimersByTimeAsync(0);

    expect(status.state).toEqual({ quiet: false, endsAtMs: null });
    expect(listener).toHaveBeenCalledTimes(1);
    release();
  });

  it("stays unknown after a failed read and retries after the bounded delay", async () => {
    const read = vi.fn<() => Promise<ChatQuietStateRead>>()
      .mockRejectedValueOnce(new Error("database busy"))
      .mockResolvedValue(notQuiet);
    const status = new ChatQuietStatus({ read });
    const listener = vi.fn();
    const release = status.use(listener);
    await vi.advanceTimersByTimeAsync(0);
    expect(status.state).toBeNull();
    expect(listener).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(CHAT_QUIET_ERROR_RETRY_MS - 1);
    expect(read).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);

    expect(read).toHaveBeenCalledTimes(2);
    expect(status.state).toEqual({ quiet: false, endsAtMs: null });
    expect(listener).toHaveBeenCalledTimes(1);
    release();
  });

  it("forgets a known state when a later read fails", async () => {
    const read = vi.fn<() => Promise<ChatQuietStateRead>>()
      .mockResolvedValueOnce(notQuiet)
      .mockRejectedValueOnce(new Error("database busy"));
    const status = new ChatQuietStatus({ read });
    const release = status.use();
    await vi.advanceTimersByTimeAsync(0);
    expect(status.state).toEqual({ quiet: false, endsAtMs: null });

    status.refresh();
    await vi.advanceTimersByTimeAsync(0);

    expect(status.state).toBeNull();
    release();
  });
});
