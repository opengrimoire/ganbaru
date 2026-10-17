import { describe, expect, it } from "vitest";
import { parseNativeMusicEffect, parseNativeMusicFrame, parseNativeMusicNotice, parseNativeMusicSnapshot, parseNativeMusicSource } from "./native-session";

const source = { kind: "local-file", identity: "root:music/árbol.flac", originalInput: "árbol.flac", title: "Árbol", path: null, artworkPath: null, startMs: 20, endMs: 90 };
const entry = { source, itemId: "item", membershipId: "membership", backend: "native-audio", availability: "available",
  enabled: true, weight: "normal", snoozedUntil: null, snoozedIndefinitely: false, embeddingBlocked: false, bound: false,
  phaseAllowed: true, skipRanges: [{ startMs: 30, endMs: 40 }], volume: null, rate: null };
const snapshot = { sessionId: "session", revision: 2, generation: 1, queueRevision: 1, currentIndex: 0, currentSource: source,
  backend: "native-audio", status: "paused", positionMs: 20, durationMs: 100, volume: 0.7, muted: false, rate: 1,
  order: "mix", repeatMode: "all", owner: "manual", context: null, playlistId: "playlist", queueName: "Music",
  canPrevious: false, canNext: true, issue: "interrupted", error: null, reviewCheckpointId: null, queue: [entry] };

describe("native Music response validation", () => {
  it("retains native Calendar ownership and validates background state versions", () => {
    const context = { activationKey: "calendar:event", eventId: "event", eventTitle: "Event",
      phase: "focus", behavior: "inherit", assignmentSource: "event-override", playlistId: null,
      state: "kept", issue: null };
    expect(parseNativeMusicSnapshot({ ...snapshot, owner: "calendar-event", context, soundscapeVersion: 7 })).toMatchObject({
      owner: "calendar-event", soundscapeVersion: 7,
    });
    expect(parseNativeMusicSnapshot({ ...snapshot, context }).soundscapeVersion).toBeNull();
    expect(() => parseNativeMusicSnapshot({ ...snapshot, context, soundscapeVersion: -1 })).toThrow();
    expect(() => parseNativeMusicSnapshot({ ...snapshot, context, soundscapeVersion: Number.MAX_SAFE_INTEGER + 1 })).toThrow();
  });
  it("retains unavailable portable sources without inventing device bindings", () => {
    const parsed = parseNativeMusicSnapshot(snapshot);
    expect(parsed.queue?.[0]).toMatchObject({ bound: false, source: { path: "", title: "Árbol" } });
    expect(parseNativeMusicSnapshot({ ...snapshot, queue: null }).queue).toBeNull();
    expect(parsed.order).toBe("mix");
    expect(parseNativeMusicSnapshot({ ...snapshot, issue: "restored" }).issue).toBe("restored");
  });

  it("rejects oversized queues and malformed external values before projection", () => {
    expect(() => parseNativeMusicSnapshot({ ...snapshot, queue: Array.from({ length: 10_001 }, () => entry) })).toThrow("queue");
    expect(() => parseNativeMusicSnapshot({ ...snapshot, generation: Number.MAX_SAFE_INTEGER + 1 })).toThrow("integer");
    expect(() => parseNativeMusicSnapshot({ ...snapshot, rate: Number.NaN })).toThrow("number");
    expect(() => parseNativeMusicSnapshot({ ...snapshot, owner: "foreign-device" })).toThrow("enum");
    expect(() => parseNativeMusicSource({ ...source, kind: "youtube-video", videoId: null })).toThrow("text");
  });

  it("keeps library artist and album and treats their absence as unknown", () => {
    expect(parseNativeMusicSource({ ...source, artist: "Artista", album: "Álbum" })).toMatchObject({ artist: "Artista", album: "Álbum" });
    expect(parseNativeMusicSource({ ...source, artist: null })).toMatchObject({ artist: null, album: null });
    expect(() => parseNativeMusicSource({ ...source, artist: 7 })).toThrow("text");
    expect(() => parseNativeMusicSource({ ...source, album: "x".repeat(16_385) })).toThrow("text");
  });

  it("preserves source generations and explicit browser load intent", () => {
    expect(parseNativeMusicEffect({ kind: "load", sessionId: "session", generation: 4, source: { ...source, kind: "youtube-video", videoId: "video", playlistId: null },
      backend: "browser", positionMs: 20, autoplay: false, volume: 0.5, muted: true, rate: 1 })).toMatchObject({ generation: 4, autoplay: false, muted: true });
    expect(() => parseNativeMusicEffect({ kind: "seek", generation: 4, positionMs: -1 })).toThrow("integer");
    expect(() => parseNativeMusicEffect({ kind: "advance-native-queue", generation: 4 })).toThrow("enum");
  });

  it("rejects unbounded effects and malformed stream identity, sequence, or snapshot", () => {
    const frame = { subscriptionId: "subscription", sequence: 1, snapshot, effects: [{ kind: "pause", generation: 1 }], effectsExpireAtMs: 1_000 };
    expect(parseNativeMusicFrame(frame)).toMatchObject({ subscriptionId: "subscription", sequence: 1 });
    expect(() => parseNativeMusicFrame({ ...frame, effects: Array.from({ length: 33 }, () => frame.effects[0]) })).toThrow("effects");
    expect(() => parseNativeMusicFrame({ ...frame, sequence: 0 })).toThrow("sequence");
    expect(() => parseNativeMusicFrame({ ...frame, subscriptionId: null })).toThrow("text");
    expect(() => parseNativeMusicFrame({ ...frame, effectsExpireAtMs: -1 })).toThrow("integer");
    expect(() => parseNativeMusicFrame({ ...frame, snapshot: { ...snapshot, volume: 10 } })).toThrow("volume");
    expect(parseNativeMusicNotice({ kind: "update", subscriptionId: "subscription", sequence: 1 })).toEqual({ kind: "update", subscriptionId: "subscription", sequence: 1 });
    expect(parseNativeMusicNotice({ kind: "invalidate", subscriptionId: "subscription" })).toEqual({ kind: "invalidate", subscriptionId: "subscription" });
    expect(() => parseNativeMusicNotice({ kind: "update", subscriptionId: "a".repeat(129), sequence: 1 })).toThrow("notice");
    expect(() => parseNativeMusicNotice({ kind: "update", subscriptionId: "subscription", sequence: 0 })).toThrow("notice");
  });
});
