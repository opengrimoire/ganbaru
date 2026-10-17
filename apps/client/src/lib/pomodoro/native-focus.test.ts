import { describe, expect, it } from "vitest";
import { focusDisplayElapsedSeconds, focusDisplayRemainingSeconds, focusNeedsVisualClock, focusProjectionIsCurrent, parseFocusCommandResult, parseFocusNotice, parseFocusProjection } from "./native-focus";
import { DEFAULT_POMODORO_CONFIG } from "./rhythm";
import { focusProjection } from "./native-focus.test-helpers";

describe("native Focus response boundaries", () => {
  it("requires complete typed state and an authorized vault identity", () => {
    expect(parseFocusProjection(focusProjection())).toEqual(focusProjection());
    expect(() => parseFocusProjection({ ...focusProjection(), vaultId: null })).toThrow();
    expect(() => parseFocusProjection({ ...focusProjection(), snapshot: { revision: 1 } })).toThrow();
    const malformed = focusProjection();
    expect(() => parseFocusProjection({ ...malformed, snapshot: { ...malformed.snapshot, remainingMs: Number.NaN } })).toThrow();
    expect(() => parseFocusProjection({ ...malformed, snapshot: { ...malformed.snapshot, observedAtMs: Number.MAX_SAFE_INTEGER } })).toThrow();
    expect(() => parseFocusProjection({ ...malformed, snapshot: { ...malformed.snapshot, mode: "running" } })).toThrow();
  });

  it("bounds changed history before examining nested external rows", () => {
    const value = focusProjection();
    expect(() => parseFocusProjection({ ...value, snapshot: { ...value.snapshot, changedSegments: Array.from({ length: 129 }, () => null) } })).toThrow();
    expect(() => parseFocusProjection({ ...value, error: "x".repeat(65_537) })).toThrow();
  });

  it("requires native visibility evidence to follow detection and precede observation", () => {
    const value = focusProjection();
    if (!value.snapshot) throw new Error("Missing fixture state");
    const observedAtMs = value.snapshot.observedAtMs;
    const detectedAtMs = observedAtMs - 1000;
    value.snapshot.idleDetectedAtMs = detectedAtMs;
    value.snapshot.idleOverlayVisibleAtMs = observedAtMs;
    expect(parseFocusProjection(value)).toEqual(value);
    const altered = (idleOverlayVisibleAtMs: unknown) => ({
      ...value, snapshot: { ...value.snapshot, idleOverlayVisibleAtMs },
    });
    expect(() => parseFocusProjection(altered(observedAtMs + 1))).toThrow();
    expect(() => parseFocusProjection(altered(detectedAtMs - 1))).toThrow();
    expect(() => parseFocusProjection(altered("visible"))).toThrow();
    expect(() => parseFocusProjection({ ...value, snapshot: { ...value.snapshot, idleDetectedAtMs: null } })).toThrow();
  });

  it("validates the immutable receipt separately from a later canonical revision", () => {
    const result = { projection: focusProjection(8), receipt: focusProjection(3).snapshot };
    expect(parseFocusCommandResult(result, 2).projection.snapshot?.revision).toBe(8);
    expect(() => parseFocusCommandResult(result, 3)).toThrow("receipt");
    expect(() => parseFocusCommandResult({ ...result, projection: { ...result.projection, vaultId: null } }, 2)).toThrow();
  });

  it("rejects delayed vault, revision, and equal-revision clock responses", () => {
    const previous = focusProjection(5, 3);
    expect(focusProjectionIsCurrent(previous, focusProjection(100, 2))).toBe(false);
    expect(focusProjectionIsCurrent(previous, focusProjection(4, 3))).toBe(false);
    const staleClock = focusProjection(5, 3);
    if (!staleClock.snapshot) throw new Error("Missing fixture state");
    staleClock.snapshot.observedAtMs -= 1;
    expect(focusProjectionIsCurrent(previous, staleClock)).toBe(false);
    expect(focusProjectionIsCurrent(previous, focusProjection(1, 4))).toBe(true);
  });

  it("accepts a bounded generation invalidation and the subsequent vault binding", () => {
    const invalidated = { ...focusProjection(1, 4), vaultId: null, snapshot: null };
    expect(focusProjectionIsCurrent(focusProjection(5, 3), invalidated)).toBe(true);
    expect(focusProjectionIsCurrent(invalidated, focusProjection(1, 4))).toBe(true);
    expect(parseFocusNotice({ vaultGeneration: 4, revision: null, available: false, hasError: false }).available).toBe(false);
    expect(() => parseFocusNotice({ vaultGeneration: -1, revision: null, available: false, hasError: false })).toThrow();
  });

  it("keeps paused work frozen while its visible opportunity is clipped by the event end", () => {
    const state = focusProjection().snapshot;
    if (!state) throw new Error("Missing fixture state");
    state.mode = "manual_pause";
    state.remainingMs = 30_000;
    state.elapsedMs = 10_000;
    state.run = { id: "run", eventId: "event", occurrenceId: "event", eventDate: "2026-10-02",
      title: null, startedAtMs: 90_000, plannedStartMs: 80_000, plannedEndMs: 130_000, endedAtMs: null,
      inheritedFocusMs: 0, inheritedPhaseMs: 0, configuration: DEFAULT_POMODORO_CONFIG };
    state.segment = { id: "phase", runId: "run", eventId: "event", eventDate: "2026-10-02",
      phase: "focus", rhythmPosition: 1, plannedStartMs: 90_000, plannedEndMs: 130_000,
      actualStartMs: 90_000, actualEndMs: null, chosenDurationMs: 70_000, status: "active", endReason: null,
      pauses: [{ startedAtMs: 100_000, endedAtMs: null, reason: "manual" }] };
    expect(focusDisplayRemainingSeconds(state, 10_000)).toBe(20);
    expect(focusDisplayRemainingSeconds(state, 30_000)).toBe(0);
    expect(state.remainingMs).toBe(30_000);
    expect(state.mode).toBe("manual_pause");
  });

  it("caps visual elapsed work without inventing a completed native phase", () => {
    const state = focusProjection().snapshot;
    if (!state) throw new Error("Missing fixture state");
    state.mode = "running";
    state.elapsedMs = 50_000;
    state.segment = { id: "phase", runId: "run", eventId: "event", eventDate: "2026-10-02",
      phase: "focus", rhythmPosition: 1, plannedStartMs: 40_000, plannedEndMs: 100_000,
      actualStartMs: 40_000, actualEndMs: null, chosenDurationMs: 60_000, status: "active", endReason: null, pauses: [] };
    expect(focusDisplayElapsedSeconds(state, 100_000)).toBe(60);
    expect(state.segment.status).toBe("active");
    state.mode = "manual_pause";
    expect(focusDisplayElapsedSeconds(state, 100_000)).toBe(50);
  });

  it("runs the visual clock only while visible counters can change", () => {
    expect(focusNeedsVisualClock(null)).toBe(false);
    const state = focusProjection().snapshot;
    if (!state) throw new Error("Missing fixture state");
    expect(focusNeedsVisualClock(state)).toBe(false);
    state.run = { id: "run", eventId: "event", occurrenceId: "event", eventDate: "2026-10-02",
      title: null, startedAtMs: 90_000, plannedStartMs: 80_000, plannedEndMs: 130_000, endedAtMs: null,
      inheritedFocusMs: 0, inheritedPhaseMs: 0, configuration: DEFAULT_POMODORO_CONFIG };
    expect(focusNeedsVisualClock(state)).toBe(true);
    state.run = { ...state.run, endedAtMs: 120_000 };
    expect(focusNeedsVisualClock(state)).toBe(false);
    expect(focusNeedsVisualClock({ ...state, idleStartedAtMs: 95_000 })).toBe(true);
    expect(focusNeedsVisualClock({ ...state, returnStartedAtMs: 95_000 })).toBe(true);
  });
});
