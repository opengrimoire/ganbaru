import { NativeFocusClient, reportIdleOverlayVisible, type FocusControlScope } from "$lib/api/focus";
import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
import { focusDisplayElapsedSeconds, focusDisplayRemainingSeconds, focusNeedsVisualClock, focusSegmentForRail, type FocusIntent, type FocusProjection } from "$lib/pomodoro/native-focus";
import { DEFAULT_POMODORO_CONFIG, type PomodoroConfig } from "$lib/pomodoro/rhythm";
import type { PersistedSegment } from "$lib/calendar/types";

const VISUAL_INTERVAL_MS = 180;
const MAX_PRESENTATION_SEGMENTS = 128;
const FOCUS_EXTENSION_SECONDS = 180;
const PAUSE_PULSE = [0, 0, 0, 0, 0, 0.067, 0.25, 0.5, 0.75, 0.933, 1, 1, 1, 1, 1, 1, 0.933, 0.75, 0.5, 0.25, 0.067, 0] as const;
let projection = $state.raw<FocusProjection | null>(null);
let segments = $state<PersistedSegment[]>([]);
let segmentVersion = $state(0);
let visualNow = $state(0);
let receivedAt = 0;
let presentationActive = false;
let visualTimer: ReturnType<typeof setInterval> | null = null;
let connectionError = $state<string | null>(null);

function recordFailure(error: unknown): void {
  connectionError = error instanceof Error ? error.message
    : typeof error === "object" && error !== null && "message" in error && typeof error.message === "string"
      ? error.message : "Native Focus connection failed";
  console.error("Native Focus request failed:", error);
}
function acceptProjection(next: FocusProjection): void {
  const previous = projection;
  projection = next;
  receivedAt = performance.now();
  visualNow = receivedAt;
  connectionError = next.error;
  if (previous?.vaultGeneration !== next.vaultGeneration) segments = [];
  const snapshot = next.snapshot;
  if (snapshot) {
    const mapped = new Map(segments.map((segment) => [segment.id, segment]));
    for (const changed of snapshot.changedSegments) mapped.set(changed.id, focusSegmentForRail(changed, snapshot.run?.occurrenceId ?? ""));
    if (snapshot.segment) mapped.set(snapshot.segment.id, focusSegmentForRail(snapshot.segment, snapshot.run?.occurrenceId ?? ""));
    segments = [...mapped.values()].slice(-MAX_PRESENTATION_SEGMENTS);
  }
  if (previous?.vaultGeneration !== next.vaultGeneration || previous?.snapshot?.revision !== snapshot?.revision) segmentVersion += 1;
  syncVisualClock();
}
const client = new NativeFocusClient(acceptProjection, recordFailure);
/** Tick only while counters can change, so idle sessions do not invalidate dependents several times per second. */
function syncVisualClock(): void {
  const needed = presentationActive && focusNeedsVisualClock(projection?.snapshot);
  if (needed && visualTimer === null) {
    visualNow = performance.now();
    visualTimer = setInterval(() => { visualNow = performance.now(); }, VISUAL_INTERVAL_MS);
  } else if (!needed && visualTimer !== null) {
    clearInterval(visualTimer);
    visualTimer = null;
  }
}
function beginPresentation(): void {
  if (presentationActive) return;
  presentationActive = true;
  visualNow = performance.now();
  syncVisualClock();
  void client.initialize().catch(recordFailure);
}
function command(intent: FocusIntent): Promise<void> { return client.command(intent); }
function dispatch(intent: FocusIntent): void { void command(intent).catch(recordFailure); }
function currentConfig(): PomodoroConfig { return projection?.snapshot?.run?.configuration ?? DEFAULT_POMODORO_CONFIG; }
function isRunning(): boolean { return projection?.snapshot?.mode === "running"; }
function isActive(): boolean {
  const snapshot = projection?.snapshot;
  return Boolean(snapshot?.run && snapshot.run.endedAtMs === null && snapshot.mode !== "stopped" && snapshot.mode !== "expired");
}
function visualElapsedMs(): number { return Math.max(0, visualNow - receivedAt); }
function remainingSeconds(): number {
  const snapshot = projection?.snapshot;
  if (!snapshot) return DEFAULT_POMODORO_CONFIG.rhythm.kind === "count" ? DEFAULT_POMODORO_CONFIG.rhythm.focusDurationMinutes * 60 : 0;
  return focusDisplayRemainingSeconds(snapshot, visualElapsedMs());
}

/** Reactive presentation of accepted native execution. Visual time never writes history. */
export function getPomodoro() {
  beginPresentation();
  return {
    /** Calendar reviews share the native vault context without supplying execution evidence. */
    get vaultContext() {
      return projection?.vaultId ? { vaultId: projection.vaultId, vaultGeneration: projection.vaultGeneration } : null;
    },
    get phase() { return projection?.snapshot?.segment?.phase ?? "focus"; },
    get remainingSeconds() { return remainingSeconds(); },
    get phaseElapsedSeconds() {
      const snapshot = projection?.snapshot;
      return snapshot ? focusDisplayElapsedSeconds(snapshot, visualElapsedMs()) : 0;
    },
    get phaseWorkDurationSeconds() { return (projection?.snapshot?.segment?.chosenDurationMs ?? 0) / 1000; },
    get currentConfig() { return currentConfig(); },
    get isRunning() { return isRunning(); },
    get isActive() { return isActive(); },
    get totalSecondsForPhase() { return (projection?.snapshot?.segment?.chosenDurationMs ?? 0) / 1000; },
    get canAddFocusTime() {
      const snapshot = projection?.snapshot;
      return isActive() && snapshot?.segment?.phase === "focus" && !snapshot.focusExtensionUsed
        && (snapshot.mode === "running" || snapshot.mode === "manual_pause");
    },
    get canPauseResume() {
      const snapshot = projection?.snapshot;
      return isActive() && snapshot?.segment?.phase === "focus" && (snapshot.mode === "running" || snapshot.mode === "manual_pause");
    },
    get pausedPulseAmount() { return projection?.snapshot?.mode === "manual_pause" ? PAUSE_PULSE[Math.floor(visualNow / VISUAL_INTERVAL_MS) % PAUSE_PULSE.length] : 0; },
    get formattedTime() { const seconds = remainingSeconds(); return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`; },
    get activeOccurrenceId() { return isActive() ? projection?.snapshot?.run?.occurrenceId ?? null : null; },
    get activeRunId() { return isActive() ? projection?.snapshot?.run?.id ?? null : null; },
    async setAutomaticAdmissionSuppressed(suppressed: boolean) {
      const generation = projection?.vaultGeneration;
      const vaultId = projection?.vaultId;
      try { await command({ kind: "set_automatic_admission_suppressed", suppressed }); }
      catch (error) {
        if (typeof error !== "object" || error === null || !("code" in error) || error.code !== "stale_revision"
          || projection?.vaultGeneration !== generation || projection?.vaultId !== vaultId) throw error;
        await command({ kind: "set_automatic_admission_suppressed", suppressed });
      }
    },
    get breakOvertimeSeconds() {
      const start = projection?.snapshot?.returnStartedAtMs;
      const observed = projection?.snapshot?.observedAtMs;
      return start !== null && start !== undefined && observed !== undefined ? Math.floor(Math.max(0, observed - start + visualElapsedMs()) / 1000) : 0;
    },
    get idleElapsedSeconds() {
      const snapshot = projection?.snapshot;
      return snapshot?.idleStartedAtMs === null || snapshot?.idleStartedAtMs === undefined ? 0
        : Math.floor(Math.max(0, snapshot.observedAtMs - snapshot.idleStartedAtMs + visualElapsedMs()) / 1000);
    },
    get segments() { return segments; },
    get segmentVersion() { return segmentVersion; },
    get suspendedAway() {
      const snapshot = projection?.snapshot;
      return snapshot?.mode === "suspended" ? { awaySeconds: Math.floor(Math.max(0, (snapshot.suspendReturnedAtMs ?? snapshot.observedAtMs) - (snapshot.suspendStartedAtMs ?? snapshot.observedAtMs)) / 1000) } : null;
    },
    async dismissSuspend(resume: boolean) { await command({ kind: "resolve_suspend", resume }); },
    get idlePaused() {
      const snapshot = projection?.snapshot;
      if (!snapshot || (snapshot.mode !== "idle_pause" && snapshot.mode !== "idle_failed")) return null;
      return {
        idleSeconds: Math.floor(Math.max(0, (snapshot.idleDetectedAtMs ?? snapshot.observedAtMs) - (snapshot.idleStartedAtMs ?? snapshot.observedAtMs)) / 1000),
        nativeOverlay: BUILD_PLATFORM_PROFILE.shell === "desktop" && projection?.error === null,
        idleStartMs: snapshot.idleStartedAtMs ?? snapshot.observedAtMs,
        overlayStartedAtMs: snapshot.idleDetectedAtMs ?? snapshot.observedAtMs,
        focusFailed: snapshot.mode === "idle_failed", focusFailedAtMs: snapshot.focusFailedAtMs,
      };
    },
    async dismissIdle(resume: boolean) { await command({ kind: "resolve_idle", resume }); },
    /** The fallback warning reports visibility without starting its own failure timer. */
    async reportIdleVisibility() {
      const current = projection;
      const snapshot = current?.snapshot;
      if (!current || !snapshot?.run || !snapshot.segment || snapshot.mode !== "idle_pause"
        || snapshot.idleDetectedAtMs === null) return;
      await reportIdleOverlayVisible({
        vaultGeneration: current.vaultGeneration, runId: snapshot.run.id,
        segmentId: snapshot.segment.id, idleDetectedAtMs: snapshot.idleDetectedAtMs,
      });
    },
    /** Native Calendar resolves configuration; editor drafts cannot become execution input. */
    async startScheduledSession() { await command({ kind: "start_scheduled", occurrenceId: null }); },
    setActiveIdleThresholdMinutes(minutes: number) { dispatch({ kind: "set_idle_timeout", minutes }); },
    async stopSession() { await command({ kind: "stop" }); },
    pause() { dispatch({ kind: "pause" }); },
    start() { dispatch({ kind: projection?.snapshot?.mode === "manual_pause" ? "resume" : "advance" }); },
    skip() { dispatch({ kind: "advance" }); },
    addFocusTime(seconds: number = FOCUS_EXTENSION_SECONDS) { dispatch({ kind: "extend_focus", seconds }); },
    async cleanupOrphans() { await client.initialize(); await client.refresh(); },
    async recoverMobileRun() { beginPresentation(); await client.initialize(); return client.refresh(); },
    prepareForMobileBackground() { presentationActive = false; syncVisualClock(); },
    get nativeSnapshot() { return projection?.snapshot ?? null; },
    async controlOverlay(intent: FocusIntent, scope: FocusControlScope) { await client.command(intent, scope); },
    get error() { return connectionError; },
  };
}
