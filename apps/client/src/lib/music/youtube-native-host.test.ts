import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  bounds: vi.fn(),
  post: vi.fn(),
  close: vi.fn(),
  calls: [] as string[],
  channels: [] as { onmessage: (value: unknown) => void }[],
}));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    constructor(public onmessage: (value: unknown) => void) { mocks.channels.push(this); }
  },
}));
vi.mock("$lib/api/music", () => ({
  openNativeYouTubeHost: mocks.open,
  setNativeYouTubeHostBounds: mocks.bounds,
  postNativeYouTubeHostMessage: mocks.post,
  closeNativeYouTubeHost: mocks.close,
}));
import {
  createNativeYouTubeHost,
  HIDDEN_NATIVE_YOUTUBE_BOUNDS,
  parseNativeYouTubeHostNotice,
} from "./youtube-native-host";

const URL_A = "https://appassets.androidplatform.net/youtube-player.html?token=a";
const URL_B = "https://appassets.androidplatform.net/youtube-player.html?token=b";

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve: () => void = () => undefined;
  const promise = new Promise<void>((done) => { resolve = () => done(); });
  return { promise, resolve };
}

async function settle(): Promise<void> {
  for (let index = 0; index < 10; index += 1) await Promise.resolve();
}

describe("parseNativeYouTubeHostNotice", () => {
  it("returns the page object for the matching instance", () => {
    expect(parseNativeYouTubeHostNotice(
      { instance: "playback", data: "{\"type\":\"ganbaru-youtube-ready\",\"token\":\"t\"}" },
      "playback",
    )).toEqual({ type: "ganbaru-youtube-ready", token: "t" });
  });

  it("rejects other instances, non-string data, invalid JSON, and non-object payloads", () => {
    for (const value of [
      null,
      "text",
      { instance: "resolver", data: "{}" },
      { instance: "playback", data: { type: "x" } },
      { instance: "playback", data: "{" },
      { instance: "playback", data: "[1]" },
      { instance: "playback", data: "null" },
      { instance: "playback", data: "\"text\"" },
    ]) {
      expect(parseNativeYouTubeHostNotice(value, "playback")).toBeNull();
    }
  });
});

describe("createNativeYouTubeHost", () => {
  beforeEach(() => {
    mocks.calls.length = 0;
    mocks.channels.length = 0;
    for (const [name, mock] of Object.entries({ open: mocks.open, bounds: mocks.bounds, post: mocks.post, close: mocks.close })) {
      mock.mockReset();
      mock.mockImplementation(async (instance: string) => { mocks.calls.push(`${name}:${instance}`); });
    }
  });

  it("delivers only current-session notices from the page", async () => {
    const onData = vi.fn();
    const host = createNativeYouTubeHost("playback", onData);
    await host.open(URL_A, HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    const first = mocks.channels[0];
    first.onmessage({ instance: "playback", data: "{\"type\":\"a\"}" });
    await host.open(URL_B, HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    first.onmessage({ instance: "playback", data: "{\"type\":\"stale\"}" });
    mocks.channels[1].onmessage({ instance: "playback", data: "{\"type\":\"b\"}" });
    expect(onData.mock.calls).toEqual([[{ type: "a" }], [{ type: "b" }]]);
  });

  it("runs operations in call order even when an earlier command is slow", async () => {
    const gate = deferred();
    mocks.open.mockImplementationOnce(async () => {
      await gate.promise;
      mocks.calls.push("open:resolver");
    });
    const host = createNativeYouTubeHost("resolver", vi.fn());
    const opened = host.open(URL_A, HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    host.post({ type: "ganbaru-youtube-command" });
    const closed = host.close();
    await settle();
    expect(mocks.calls).toEqual([]);
    gate.resolve();
    await Promise.all([opened, closed]);
    await settle();
    expect(mocks.calls).toEqual(["open:resolver", "post:resolver", "close:resolver"]);
  });

  it("keeps later operations running after a failed command", async () => {
    mocks.open.mockRejectedValueOnce(new Error("unsupported"));
    const host = createNativeYouTubeHost("playback", vi.fn());
    await expect(host.open(URL_A, HIDDEN_NATIVE_YOUTUBE_BOUNDS)).rejects.toThrow("unsupported");
    await host.setBounds(HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    expect(mocks.calls).toEqual(["bounds:playback"]);
  });

  it("stops a replaced controller from moving, messaging, or closing the newer page", async () => {
    const older = createNativeYouTubeHost("resolver", vi.fn());
    const newer = createNativeYouTubeHost("resolver", vi.fn());
    await older.open(URL_A, HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    await newer.open(URL_B, HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    mocks.calls.length = 0;
    await older.setBounds(HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    older.post({ type: "ganbaru-youtube-command" });
    await older.close();
    await settle();
    expect(mocks.calls).toEqual([]);
    await newer.close();
    expect(mocks.calls).toEqual(["close:resolver"]);
  });

  it("ignores commands before open and after close", async () => {
    const host = createNativeYouTubeHost("playback", vi.fn());
    await host.setBounds(HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    host.post({ type: "ganbaru-youtube-command" });
    await host.close();
    await host.open(URL_A, HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    await host.close();
    await host.setBounds(HIDDEN_NATIVE_YOUTUBE_BOUNDS);
    host.post({ type: "ganbaru-youtube-command" });
    await settle();
    expect(mocks.calls).toEqual(["open:playback", "close:playback"]);
  });
});
