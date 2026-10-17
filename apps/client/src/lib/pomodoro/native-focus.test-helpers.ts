import type { FocusProjection } from "./native-focus";

/** Minimal stopped native state for boundary and transport regression tests. */
export function focusProjection(revision = 1, vaultGeneration = 1): FocusProjection {
  return {
    vaultId: "vault", vaultGeneration, error: null,
    snapshot: {
      revision, observedAtMs: 100_000, mode: "stopped", run: null, segment: null,
      changedSegments: [], phaseDeadlineMs: null, remainingMs: 0, elapsedMs: 0,
      completedFocusCount: 0, skipNextBreak: false, focusExtensionUsed: false,
      breakExtensionMs: 0, dismissedOccurrenceId: null, automaticAdmissionSuppressed: false,
      pausedPromptsDismissed: false, idleStartedAtMs: null, idleDetectedAtMs: null, idleOverlayVisibleAtMs: null,
      focusFailedAtMs: null, suspendStartedAtMs: null, suspendReturnedAtMs: null,
      returnStartedAtMs: null, activitySourceUnavailable: false, effectiveIdleTimeoutMinutes: null,
    },
  };
}
