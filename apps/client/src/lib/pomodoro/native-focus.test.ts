import { describe, expect, it } from "vitest";
import { FOCUS_VISUAL_INTERVAL_MS, focusDisplayElapsedSeconds, focusDisplayRemainingSeconds, focusNeedsVisualClock, focusPausedPulseAmount, focusProjectionIsCurrent, focusSnapshotAgeMs, parseFocusCommandResult, parseFocusNotice, parseFocusProjection } from "./native-focus";
import { DEFAULT_POMODORO_CONFIG } from "./rhythm";
import { focusProjection, idlePausedFocusSnapshot } from "./native-focus.test-helpers";

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
    const idle = idlePausedFocusSnapshot(99_000);
    const observedAtMs = idle.observedAtMs;
    const detectedAtMs = idle.idleDetectedAtMs;
    const value = { ...focusProjection(), snapshot: { ...idle, idleOverlayVisibleAtMs: observedAtMs } };
    expect(parseFocusProjection(value)).toEqual(value);
    const altered = (idleOverlayVisibleAtMs: unknown) => ({
      ...value, snapshot: { ...value.snapshot, idleOverlayVisibleAtMs },
    });
    expect(() => parseFocusProjection(altered(observedAtMs + 1))).toThrow();
    expect(() => parseFocusProjection(altered(detectedAtMs - 1))).toThrow();
    expect(() => parseFocusProjection(altered("visible"))).toThrow();
    expect(() => parseFocusProjection({ ...value, snapshot: { ...value.snapshot, idleDetectedAtMs: null } })).toThrow();
  });

  it("accepts each mode's facts only beside that mode", () => {
    const idle = idlePausedFocusSnapshot(99_000);
    expect(parseFocusProjection({ ...focusProjection(), snapshot: idle }).snapshot).toEqual(idle);
    const { idleStartedAtMs: _started, ...withoutStart } = idle;
    expect(() => parseFocusProjection({ ...focusProjection(), snapshot: withoutStart })).toThrow();
    expect(() => parseFocusProjection({ ...focusProjection(), snapshot: { ...idle, mode: "return_wait" } })).toThrow();
    expect(() => parseFocusProjection({ ...focusProjection(), snapshot: { ...idle, mode: "paused" } })).toThrow();
    const waiting = { ...idle, mode: "return_wait", returnStartedAtMs: 95_000, pausedPromptsDismissed: true };
    expect(parseFocusProjection({ ...focusProjection(), snapshot: waiting }).snapshot?.mode).toBe("return_wait");
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

  it("counts down from native observation so a cached snapshot matches a fresh one", () => {
    const cached = focusProjection().snapshot;
    if (!cached) throw new Error("Missing fixture state");
    cached.mode = "running";
    cached.remainingMs = 600_000;
    const fresh = { ...cached, observedAtMs: 112_000, remainingMs: 588_000 };
    const wallNowMs = 112_500;
    expect(focusDisplayRemainingSeconds(cached, focusSnapshotAgeMs(cached, wallNowMs)))
      .toBe(focusDisplayRemainingSeconds(fresh, focusSnapshotAgeMs(fresh, wallNowMs)));
    expect(focusDisplayRemainingSeconds(cached, focusSnapshotAgeMs(cached, wallNowMs))).toBe(588);
    expect(focusSnapshotAgeMs(cached, 90_000)).toBe(0);
    expect(focusSnapshotAgeMs(null, wallNowMs)).toBe(0);
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
    expect(focusNeedsVisualClock({ ...state, mode: "manual_pause", pausedPromptsDismissed: false })).toBe(false);
    expect(focusNeedsVisualClock({ ...state, mode: "idle_failed", idleStartedAtMs: 95_000, idleDetectedAtMs: 96_000,
      focusFailedAtMs: 95_000, pausedPromptsDismissed: false })).toBe(true);
    expect(focusNeedsVisualClock({ ...state, mode: "return_wait", returnStartedAtMs: 95_000, pausedPromptsDismissed: false })).toBe(true);
  });
});

describe("focusPausedPulseAmount", () => {
  it("rests, fades fully, and returns once per cycle, stepping on the visual clock", () => {
    const steps = Array.from({ length: 44 }, (_, step) => focusPausedPulseAmount(step * FOCUS_VISUAL_INTERVAL_MS));
    const cycle = steps.slice(0, 22);
    expect(cycle[0]).toBe(0);
    expect(Math.max(...cycle)).toBe(1);
    expect(cycle.at(-1)).toBe(0);
    expect(steps.slice(22)).toEqual(cycle);
    expect(focusPausedPulseAmount(FOCUS_VISUAL_INTERVAL_MS - 1)).toBe(cycle[0]);
    for (const amount of steps) expect(amount).toBeGreaterThanOrEqual(0);
  });
});
