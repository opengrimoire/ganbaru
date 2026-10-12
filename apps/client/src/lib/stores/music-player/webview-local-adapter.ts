import type { LocalBackendKind } from "$lib/api/media-player";
import {
  localMediaSeekTargetMs,
  normalizeLocalPlayableStartMs,
  type PlaybackSnapshot,
  type PlaybackStatus,
} from "$lib/music/playback";
import type { MusicSource } from "$lib/music/sources";

const PLAYABLE_START_KICK_MS = 650;
const PLAYABLE_START_NUDGE_MS = 1_000;
const PLAYABLE_START_MAX_NUDGES = 12;
const HAVE_CURRENT_DATA_READY_STATE = 2;

export function nextPlayableStartCandidateMs(
  currentMs: number,
  seekableStartMs: number,
  seekableEndCandidateMs: number,
): number {
  if (seekableStartMs > currentMs + 100) return seekableStartMs;
  if (seekableEndCandidateMs > currentMs + 100) return seekableEndCandidateMs;
  return currentMs + PLAYABLE_START_NUDGE_MS;
}

export function canNudgePlayableStart(
  readyState: number,
  kickCount: number,
): boolean {
  return readyState < HAVE_CURRENT_DATA_READY_STATE
    && kickCount < PLAYABLE_START_MAX_NUDGES;
}

export interface MusicWebviewLocalState {
  currentSource: MusicSource | null;
  snapshot: PlaybackSnapshot;
  playerError: string | null;
  muted: boolean;
  localMediaElement: HTMLMediaElement | null;
  localMediaSrc: string | null;
  localHasVideo: boolean;
  localBackendKind: LocalBackendKind;
  localVideoReady: boolean;
}

interface MusicWebviewLocalAdapterContext {
  state: MusicWebviewLocalState;
  effectiveVolume(): number;
  updateExternalControls(): void;
  persist(force?: boolean): Promise<void>;
}

export interface MusicWebviewLocalAdapter {
  register(element: HTMLMediaElement | null): void;
  prepare(resumeMs: number, playableStartMs: number): void;
  configure(): void;
  play(): Promise<void>;
  pause(): void;
  stop(): void;
  seek(positionMs: number): void;
  applyVolume(): void;
  reset(): void;
  handleLoadedMetadata(event: Event): void;
  handleLoadedData(event: Event): void;
  handleTimeUpdate(event: Event): void;
  handlePlay(event: Event): void;
  handlePause(event: Event): void;
  handleEnded(event: Event): Promise<void>;
  handleError(event: Event): void;
}

/** Owns WebView local media events, volume, seeking, and playable-start recovery. */
export function createMusicWebviewLocalAdapter(
  context: MusicWebviewLocalAdapterContext,
): MusicWebviewLocalAdapter {
  const state = context.state;
  let pendingResumeMs = 0;
  let playableStartMs = 0;
  let kickTimeoutId: number | null = null;
  let kickCount = 0;
  let ignoreNextPause = false;
  let isPauseSilenced = false;

  function register(element: HTMLMediaElement | null): void {
    if (state.localMediaElement === element) return;
    state.localMediaElement = element;
    if (element) configure();
  }

  function prepare(resumeMs: number, nextPlayableStartMs: number): void {
    pendingResumeMs = Math.max(0, resumeMs);
    playableStartMs = normalizeLocalPlayableStartMs(nextPlayableStartMs);
  }

  function effectiveElementVolume(): number {
    return isPauseSilenced ? 0 : context.effectiveVolume();
  }

  function applyVolume(): void {
    const element = state.localMediaElement;
    if (!element) return;
    const volume = effectiveElementVolume();
    element.muted = volume === 0;
    element.volume = volume;
  }

  function silence(element: HTMLMediaElement): void {
    isPauseSilenced = true;
    element.muted = true;
    element.volume = 0;
  }

  function restoreVolume(element: HTMLMediaElement): void {
    if (element !== state.localMediaElement) return;
    isPauseSilenced = false;
    applyVolume();
  }

  function scheduleVolumeRestore(element: HTMLMediaElement): void {
    if (typeof window === "undefined") return;
    const restoreIfPlaying = () => {
      if (!element.paused) restoreVolume(element);
    };
    window.setTimeout(restoreIfPlaying, 0);
    window.setTimeout(restoreIfPlaying, 150);
  }

  function rawDurationMs(element: HTMLMediaElement): number | null {
    return Number.isFinite(element.duration) && element.duration > 0
      ? Math.round(element.duration * 1_000)
      : null;
  }

  function positionMs(element: HTMLMediaElement): number {
    return Number.isFinite(element.currentTime) && element.currentTime > 0
      ? Math.round(element.currentTime * 1_000)
      : 0;
  }

  function snapshotFromElement(
    element: HTMLMediaElement,
    status: PlaybackStatus,
  ): PlaybackSnapshot {
    return {
      ...state.snapshot,
      status,
      positionMs: positionMs(element),
      durationMs: rawDurationMs(element),
      rate: element.playbackRate,
      error: status === "error" ? state.snapshot.error : null,
    };
  }

  function seekableStartMs(element: HTMLMediaElement): number | null {
    if (element.seekable.length === 0) return null;
    try {
      const start = element.seekable.start(0);
      return Number.isFinite(start) ? Math.round(start * 1_000) : null;
    } catch {
      return null;
    }
  }

  function seekableEndMs(element: HTMLMediaElement): number | null {
    if (element.seekable.length === 0) return null;
    try {
      const end = element.seekable.end(element.seekable.length - 1);
      return Number.isFinite(end) ? Math.round(end * 1_000) : null;
    } catch {
      return null;
    }
  }

  function seekElement(element: HTMLMediaElement, position: number): void {
    try {
      element.currentTime = localMediaSeekTargetMs(position, playableStartMs) / 1_000;
    } catch {
      pendingResumeMs = position;
    }
  }

  function updatePlayableStart(element: HTMLMediaElement): void {
    const startMs = normalizeLocalPlayableStartMs(seekableStartMs(element));
    if (startMs > 0 || playableStartMs === 0) playableStartMs = startMs;
  }

  function alignToPlayableStart(element: HTMLMediaElement): void {
    updatePlayableStart(element);
    if (playableStartMs > 0 && element.currentTime * 1_000 < playableStartMs) {
      seekElement(element, 0);
    }
  }

  function clearKick(): void {
    if (kickTimeoutId === null || typeof window === "undefined") return;
    window.clearTimeout(kickTimeoutId);
    kickTimeoutId = null;
  }

  function seekableEndCandidate(element: HTMLMediaElement): number {
    const durationMs = rawDurationMs(element);
    const endMs = seekableEndMs(element);
    if (durationMs === null || endMs === null) return 0;
    if (endMs <= 1_000 || endMs >= durationMs - 1_000) return 0;
    return normalizeLocalPlayableStartMs(endMs);
  }

  function nextStartCandidate(element: HTMLMediaElement): number {
    const currentMs = positionMs(element);
    const startMs = normalizeLocalPlayableStartMs(seekableStartMs(element));
    const endCandidate = seekableEndCandidate(element);
    return nextPlayableStartCandidateMs(currentMs, startMs, endCandidate);
  }

  function scheduleKick(element: HTMLMediaElement, resetCount = true): void {
    clearKick();
    if (resetCount) kickCount = 0;
    if (typeof window === "undefined") return;
    kickTimeoutId = window.setTimeout(() => {
      kickTimeoutId = null;
      if (element !== state.localMediaElement || element.paused) return;
      if (!canNudgePlayableStart(element.readyState, kickCount)) return;
      const startMs = nextStartCandidate(element);
      if (startMs <= 0) return;
      kickCount += 1;
      playableStartMs = startMs;
      seekElement(element, startMs);
      void element.play().catch(() => null);
      state.snapshot = snapshotFromElement(element, "playing");
      scheduleKick(element, false);
    }, PLAYABLE_START_KICK_MS);
  }

  function configure(): void {
    const element = state.localMediaElement;
    if (!element) return;
    ignoreNextPause = false;
    applyVolume();
    element.playbackRate = state.snapshot.rate;
    element.load();
  }

  async function play(): Promise<void> {
    const element = state.localMediaElement;
    if (!element) {
      state.playerError = "Media is still loading.";
      state.snapshot = {
        ...state.snapshot,
        status: "error",
        error: state.playerError,
      };
      return;
    }
    try {
      restoreVolume(element);
      element.playbackRate = state.snapshot.rate;
      alignToPlayableStart(element);
      await element.play();
      scheduleKick(element);
      restoreVolume(element);
      scheduleVolumeRestore(element);
      state.snapshot = snapshotFromElement(element, "playing");
      state.playerError = null;
      context.updateExternalControls();
    } catch (error) {
      state.playerError = error instanceof Error ? error.message : String(error);
      state.snapshot = {
        ...state.snapshot,
        status: "error",
        error: state.playerError,
      };
    }
  }

  function pause(): void {
    const element = state.localMediaElement;
    if (!element) return;
    silence(element);
    element.pause();
    state.snapshot = snapshotFromElement(element, "paused");
    context.updateExternalControls();
  }

  function stop(): void {
    const element = state.localMediaElement;
    if (!element) {
      state.snapshot = { ...state.snapshot, status: "idle", positionMs: 0 };
      return;
    }
    ignoreNextPause = true;
    silence(element);
    element.pause();
    seekElement(element, 0);
    state.snapshot = snapshotFromElement(element, "idle");
    context.updateExternalControls();
  }

  function seek(position: number): void {
    const element = state.localMediaElement;
    if (!element) {
      state.snapshot = { ...state.snapshot, positionMs: position };
      return;
    }
    seekElement(element, position);
    state.snapshot = snapshotFromElement(element, state.snapshot.status);
  }

  function elementFromEvent(event: Event): HTMLMediaElement | null {
    if (!(event.currentTarget instanceof HTMLMediaElement)) return null;
    return event.currentTarget === state.localMediaElement ? event.currentTarget : null;
  }

  function handleLoadedMetadata(event: Event): void {
    const element = elementFromEvent(event);
    if (!element || state.currentSource?.kind !== "local-file") return;
    updatePlayableStart(element);
    const durationMs = rawDurationMs(element);
    const resumeMs = durationMs === null
      ? pendingResumeMs
      : Math.min(pendingResumeMs, durationMs);
    pendingResumeMs = 0;
    if (resumeMs > 0 || playableStartMs > 0) seekElement(element, resumeMs);
    state.snapshot = snapshotFromElement(
      element,
      state.snapshot.status === "loading" ? "ready" : state.snapshot.status,
    );
    state.playerError = null;
    context.updateExternalControls();
    void context.persist(true);
  }

  function handleLoadedData(event: Event): void {
    const element = elementFromEvent(event);
    if (!element || state.currentSource?.kind !== "local-file") return;
    updatePlayableStart(element);
    alignToPlayableStart(element);
    kickCount = 0;
    state.localVideoReady = true;
  }

  function handleTimeUpdate(event: Event): void {
    const element = elementFromEvent(event);
    if (!element || state.currentSource?.kind !== "local-file") return;
    state.snapshot = snapshotFromElement(element, state.snapshot.status);
    void context.persist();
  }

  function handlePlay(event: Event): void {
    const element = elementFromEvent(event);
    if (!element || state.currentSource?.kind !== "local-file") return;
    scheduleKick(element);
    restoreVolume(element);
    scheduleVolumeRestore(element);
    state.snapshot = snapshotFromElement(element, "playing");
    state.playerError = null;
    context.updateExternalControls();
    void context.persist();
  }

  function handlePause(event: Event): void {
    const element = elementFromEvent(event);
    if (!element || state.currentSource?.kind !== "local-file") return;
    if (ignoreNextPause) {
      ignoreNextPause = false;
      return;
    }
    if (state.snapshot.status === "idle" || element.ended) return;
    state.snapshot = snapshotFromElement(element, "paused");
    context.updateExternalControls();
    void context.persist();
  }

  async function handleEnded(event: Event): Promise<void> {
    const element = elementFromEvent(event);
    if (!element || state.currentSource?.kind !== "local-file") return;
    state.snapshot = snapshotFromElement(element, "ended");
    context.updateExternalControls();
    await context.persist(true);
  }

  function errorMessage(element: HTMLMediaElement): string {
    switch (element.error?.code) {
      case 1: return "Local playback was interrupted.";
      case 2: return "The local media file could not be read.";
      case 3: return "The WebView could not decode this media file.";
      case 4: return "This file or codec is not supported by the current WebView player.";
      default: return "Local playback failed.";
    }
  }

  function handleError(event: Event): void {
    const element = elementFromEvent(event);
    if (!element || state.currentSource?.kind !== "local-file") return;
    state.playerError = errorMessage(element);
    state.snapshot = {
      ...snapshotFromElement(element, "error"),
      error: state.playerError,
    };
    context.updateExternalControls();
    void context.persist(true);
  }

  function reset(): void {
    const element = state.localMediaElement;
    if (element) {
      ignoreNextPause = true;
      silence(element);
      element.pause();
      element.removeAttribute("src");
      element.load();
    }
    isPauseSilenced = false;
    state.localMediaSrc = null;
    state.localHasVideo = false;
    state.localBackendKind = "none";
    state.localVideoReady = false;
    clearKick();
    kickCount = 0;
    pendingResumeMs = 0;
    playableStartMs = 0;
  }

  return {
    register,
    prepare,
    configure,
    play,
    pause,
    stop,
    seek,
    applyVolume,
    reset,
    handleLoadedMetadata,
    handleLoadedData,
    handleTimeUpdate,
    handlePlay,
    handlePause,
    handleEnded,
    handleError,
  };
}
