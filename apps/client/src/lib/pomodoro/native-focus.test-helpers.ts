import type { FocusProjection, FocusSnapshot } from "./native-focus";
import { DEFAULT_POMODORO_CONFIG } from "./rhythm";

/** Minimal stopped native state for boundary and transport regression tests. */
export function focusProjection(revision = 1, vaultGeneration = 1): FocusProjection {
  return {
    vaultId: "vault", vaultGeneration, error: null,
    snapshot: {
      revision, observedAtMs: 100_000, mode: "stopped", run: null, segment: null,
      changedSegments: [], phaseDeadlineMs: null, remainingMs: 0, elapsedMs: 0,
      completedFocusCount: 0, skipNextBreak: false, focusExtensionUsed: false,
      breakExtensionMs: 0, dismissedOccurrenceId: null, automaticAdmissionSuppressed: false,
      activitySourceUnavailable: false, effectiveIdleTimeoutMinutes: null,
    },
  };
}

/** An idle-paused focus phase whose warning has not been shown, for idle evidence tests. */
export function idlePausedFocusSnapshot(detectedAtMs: number): FocusSnapshot & { mode: "idle_pause" } {
  const stopped = focusProjection().snapshot;
  if (!stopped) throw new Error("Missing fixture state");
  return {
    ...stopped, mode: "idle_pause", idleStartedAtMs: 40_000, idleDetectedAtMs: detectedAtMs,
    idleOverlayVisibleAtMs: null, pausedPromptsDismissed: false,
    run: { id: "run", eventId: "event", occurrenceId: "event", eventDate: "2026-10-02",
      title: null, startedAtMs: 30_000, plannedStartMs: 30_000, plannedEndMs: 130_000, endedAtMs: null,
      inheritedFocusMs: 0, inheritedPhaseMs: 0, configuration: DEFAULT_POMODORO_CONFIG },
    segment: { id: "phase", runId: "run", eventId: "event", eventDate: "2026-10-02",
      phase: "focus", rhythmPosition: 1, plannedStartMs: 30_000, plannedEndMs: 130_000,
      actualStartMs: 30_000, actualEndMs: null, chosenDurationMs: 100_000, status: "active", endReason: null,
      pauses: [{ startedAtMs: 40_000, endedAtMs: null, reason: "idle" }] },
  };
}
