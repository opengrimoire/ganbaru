import { tick } from "svelte";
import type { MusicPlaybackMode, MusicRepeatMode, MusicWeight } from "$lib/music/library/contracts";
import type { MusicActivityPhase, MusicAssignmentBehavior, MusicAssignmentSource } from "$lib/music/context-assignment";
import { emptyMusicSkipBreakdown, type MusicPlaylistSkipReason, type MusicSavedQueueEntry } from "$lib/music/playlists/playback";
import { probeLocalMedia, type LocalBackendKind } from "$lib/api/media-player";
import { confirmLocalMusicSources, pickMediaFolder, registerEmbeddedArtwork, registerMediaFile } from "$lib/api/music";
import { clampRate, clampVolume, formatVolumePercent, MAX_VOLUME, type PlaybackSnapshot } from "$lib/music/playback";
import { formatSourceKind, isYouTubeSource, localFileSourceFromPath, parseMusicSourceInput, sourceDisplayLabel, type MusicSource } from "$lib/music/sources";
import { NativeMusicSessionClient } from "$lib/music/session/native-session-client";
import type { NativeMusicEffect, NativeMusicIntent, NativeMusicQueueEntry, NativeMusicQueueIntent, NativeMusicSnapshot } from "$lib/music/session/native-session";
import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";
import { focusMusicWindow } from "$lib/music/session/platform-controls";
import { getLocalization } from "$lib/i18n/translator.svelte";
import { MusicSurfaceClaims } from "$lib/stores/music-player/surface-claims";
import { initialMusicSnapshot, loadMusicPlayerSettings, persistMusicPlayerSettings } from "$lib/stores/music-player/settings";
import { MusicLoadRuntime } from "$lib/stores/music-player/load-runtime";
import { createMusicHostedMediaController, type MusicStaleVisual } from "$lib/stores/music-player/hosted-media-controller";
import { createDeviceVolume } from "$lib/stores/music-player/device-volume";
import { createMusicExternalControls } from "$lib/stores/music-player/external-controls";
import { createMusicYouTubeAdapter } from "$lib/stores/music-player/youtube-adapter";
import type { NativeYouTubeHost } from "$lib/music/youtube-native-host";
import { createMusicWebviewLocalAdapter } from "$lib/stores/music-player/webview-local-adapter";
import { MusicObservationSender } from "$lib/stores/music-player/observation-sender";

export type { MusicStaleVisual } from "$lib/stores/music-player/hosted-media-controller";
export type MusicPlaybackContextOwner = "manual" | "review" | "calendar-event" | "pomodoro" | "soundscape";

export type MusicPlaybackActionOrigin = "manual" | "context" | "pomodoro-pause" | "system";
export type MusicContextPlaybackState = "playing" | "prepared" | "paused" | "kept" | "unavailable" | "overridden";
export type MusicContextPlaybackIssue = "missing-playlist" | "no-eligible-items" | "offline-only" | "deleted-soundscape" | "activation-failed" | null;

export interface MusicContextPlayback {
  owner: "calendar-event" | "pomodoro";
  activationKey: string;
  eventId: string;
  eventTitle: string;
  displayLabel: string;
  phase: MusicActivityPhase;
  behavior: MusicAssignmentBehavior;
  assignmentSource: MusicAssignmentSource;
  playlistId: string | null;
  playlistName: string | null;
  state: MusicContextPlaybackState;
  issue: MusicContextPlaybackIssue;
}

export interface MusicSavedPlaylistLoadOptions {
  explicitItemId?: string | null;
  structuralSkipped?: Record<MusicPlaylistSkipReason, number>;
  autoplay?: boolean;
  autoRecovery?: boolean;
  avoidItemId?: string | null;
  context?: MusicContextPlayback | null;
}

export interface MusicSourceQueueEntry {
  itemId: string;
  source: MusicSource;
  snoozed: boolean;
}

/** User intent passed to the native source loader from a Music surface. */
export interface LoadSourceOptions {
  autoplay?: boolean;
  resume?: boolean;
  preserveQueue?: boolean;
}

export interface MusicReviewPlaybackCheckpoint { checkpointId: string }

const PROGRESS_MAX_FALLBACK = 1;
const BROWSER_OBSERVATION_MS = 1_000;
const initialPlayerSettings = loadMusicPlayerSettings();

/** Presents the native Music session and runs the browser playback it delegates. */
class MusicPlayerStore {
  sourceInput = $state("");
  currentSource = $state<MusicSource | null>(null);
  parseError = $state<string | null>(null);
  playerError = $state<string | null>(null);
  snapshot = $state<PlaybackSnapshot>(initialMusicSnapshot(initialPlayerSettings));
  queue = $state<MusicSource[]>([]);
  folderScanTruncated = $state(false);
  shuffleEnabled = $state(initialPlayerSettings.shuffleEnabled);
  mixEnabled = $state(initialPlayerSettings.mixEnabled);
  get playbackMode(): MusicPlaybackMode {
    return this.shuffleEnabled ? this.mixEnabled ? "mix" : "shuffle" : "in-order";
  }
  muted = $state(initialPlayerSettings.muted);
  playlistVisible = $state(initialPlayerSettings.playlistVisible);
  shuffleOrder = $state<number[]>([]);
  queueHistory = $state<number[]>([]);
  pendingQueueIndex = $state<number | null>(null);
  youtubeHostUrl = $state<string | null>(null);
  youtubeFrame = $state<HTMLIFrameElement | null>(null);
  youtubeHostToken = $state<string | null>(null);
  youtubeHostReady = $state(false);
  youtubePlaybackStarting = $state(false);
  youtubeKnownDurations = $state<Record<string, number>>({});
  localMediaElement = $state<HTMLMediaElement | null>(null);
  localMediaSrc = $state<string | null>(null);
  localHasVideo = $state(false);
  localBackendKind = $state<LocalBackendKind>("none");
  localVideoReady = $state(false);
  currentArtworkUrl = $state<string | null>(null);
  staleVisual = $state<MusicStaleVisual | null>(null);
  surfaceElement = $state<HTMLElement | null>(null);
  /** Whether this window runs the browser decoders; other windows show the session without its video. */
  decodesBrowserMedia = $state(true);
  sourceActionBusy = $state(false);
  volumeFeedbackId = $state(0);
  /** Platform media volume level that replaces the app volume, or null where the app volume applies. */
  deviceVolumeLevel = $state<number | null>(null);
  contextOwner = $state<MusicPlaybackContextOwner>("manual");
  contextPlayback = $state<MusicContextPlayback | null>(null);
  contextRetryRequest = $state(0);
  manualPlaybackActionVersion = $state(0);
  activePlaylistId = $state<string | null>(null);
  activePlaylistName = $state<string | null>(null);
  activeSourceQueueId = $state<string | null>(null);
  activeQueueItemIds = $state<string[]>([]);
  sourceQueueSnoozedItemIds = $state<string[]>([]);
  activePlaylistRepeatMode = $state<MusicRepeatMode>("off");
  savedQueueEntries = $state<MusicSavedQueueEntry[]>([]);
  savedQueueRecentItemIds = $state<string[]>([]);
  savedQueueSkipBreakdown = $state<Record<MusicPlaylistSkipReason, number>>(emptyMusicSkipBreakdown());
  online = $state(typeof navigator === "undefined" || navigator.onLine);

  private native = $state<NativeMusicSnapshot | null>(null);
  /** Identifies native background state for a read-only presentation refresh. */
  get soundscapeVersion(): number | null { return this.native?.soundscapeVersion ?? null; }
  private entries = $state<NativeMusicQueueEntry[]>([]);
  private initialized = false;
  private isHeartbeatRunning = false;
  private observationTimer: ReturnType<typeof setInterval> | null = null;
  private unsubscribeVault: (() => void) | null = null;
  private unsubscribeDeviceVolume: (() => void) | null = null;
  private isPinningSessionVolume = false;
  private readonly deviceVolume = createDeviceVolume();
  private browserSessionId: string | null = null;
  private browserGeneration = -1;
  private browserSequence = 0;
  private readonly observationSender = new MusicObservationSender(() => this.sendBrowserObservation());
  private nativeArtworkGeneration = -1;
  private readonly loadRuntime = new MusicLoadRuntime();
  private readonly surfaceClaims = new MusicSurfaceClaims((element) => { this.surfaceElement = element; });
  private readonly hostedMediaController = createMusicHostedMediaController({ state: this, loadedTitle: () => this.loadedTitle });
  private readonly client = new NativeMusicSessionClient({
    snapshot: (snapshot) => this.applySnapshot(snapshot), effect: (effect) => this.applyEffect(effect), error: (error) => this.reportError(error),
    resetProjection: () => this.clearNativePresentation(),
  });
  private readonly youtubeAdapter = createMusicYouTubeAdapter({
    state: this, loadRuntime: this.loadRuntime, effectiveVolume: () => this.effectiveVolume(),
    resolvePlaylist: (sources, selectedIndex, autoplay) => this.start({ kind: "sources", sources, selectedIndex, name: sources[0]?.title ?? "YouTube" }, autoplay, false).then(() => undefined),
    persist: () => this.observeBrowser(), updateExternalControls: () => this.externalControls.updateBrowser(),
    onDurationKnown: (id, duration) => { this.youtubeKnownDurations = { ...this.youtubeKnownDurations, [id]: duration }; },
    setPlaybackStarting: (starting) => { this.youtubePlaybackStarting = starting; },
  });
  private readonly webviewLocalAdapter = createMusicWebviewLocalAdapter({
    state: this, effectiveVolume: () => this.effectiveVolume(), updateExternalControls: () => this.externalControls.updateBrowser(),
    persist: () => this.observeBrowser(),
  });
  private readonly externalControls = createMusicExternalControls({
    currentSource: () => this.browserSessionId === this.native?.sessionId && this.native?.backend === "browser" ? this.currentSource : null,
    snapshot: () => this.snapshot,
    title: () => this.loadedTitle,
    sourceKindLabel: () => this.sourceKindLabel,
    artworkUrl: () => this.currentArtworkUrl,
    isBusy: () => this.isBusy,
    canPrevious: () => this.canPlayPreviousTrack,
    canNext: () => this.canPlayNextTrack,
    volume: () => this.volumeControlValue,
    muted: () => this.muted,
    shuffleEnabled: () => this.shuffleEnabled,
    play: () => this.playPlayback(),
    pause: () => this.pausePlayback(),
    togglePlay: () => this.togglePlay(),
    stop: () => this.stopPlayback(),
    previous: () => this.playPreviousTrack(),
    next: () => this.playNextTrack(),
    seekBy: (deltaMs) => this.seekByMs(deltaMs),
    seekTo: (positionMs) => this.seekToMs(positionMs),
    setVolume: (volume) => this.setVolume(volume),
    setRate: (rate) => this.setRate(rate),
    toggleShuffle: () => this.toggleShuffle(),
    inspectAssignment: () => this.inspectContextAssignment(),
    handleWindowMessage: this.youtubeAdapter.handleMessage,
  });
  get isBusy(): boolean {
    return this.snapshot.status === "loading";
  }

  get isPlaying(): boolean {
    return this.snapshot.status === "playing";
  }

  get durationMs(): number {
    return this.snapshot.durationMs ?? 0;
  }

  get progressMax(): number {
    return this.durationMs > 0 ? this.durationMs : PROGRESS_MAX_FALLBACK;
  }

  get progressValue(): number {
    if (this.durationMs <= 0) return 0;
    return Math.min(this.snapshot.positionMs, this.progressMax);
  }

  get loadedTitle(): string {
    return this.currentSource ? sourceDisplayLabel(this.currentSource) : "Nothing loaded";
  }

  get sourceKindLabel(): string {
    return this.currentSource ? formatSourceKind(this.currentSource.kind) : "No source";
  }

  get queuePositionLabel(): string {
    const index = this.currentQueueIndex;
    if (index < 0 || this.queue.length === 0) return "No playlist";
    return `${index + 1} of ${this.queue.length}`;
  }

  get currentQueueIndex(): number {
    return this.native?.currentIndex ?? -1;
  }

  get currentSavedItemId(): string | null {
    return this.entries[this.currentQueueIndex]?.itemId ?? null;
  }

  get highlightedQueueIndex(): number {
    return this.currentQueueIndex;
  }

  get canPlayPreviousTrack(): boolean {
    return this.native?.canPrevious ?? false;
  }

  get canPlayNextTrack(): boolean {
    return this.native?.canNext ?? false;
  }

  get isLocalVideoActive(): boolean {
    return this.currentSource?.kind === "local-file" && this.localHasVideo;
  }

  get volumeMax(): number {
    return MAX_VOLUME;
  }

  get volumeControlValue(): number {
    return this.deviceVolumeLevel ?? clampVolume(this.snapshot.volume);
  }

  get volumePercentLabel(): string {
    return formatVolumePercent(this.volumeControlValue);
  }

  get volumeFeedbackLabel(): string {
    return `Sound ${formatVolumePercent(this.muted ? 0 : this.volumeControlValue)}`;
  }

  get isYouTubeActive(): boolean {
    return this.currentSource ? isYouTubeSource(this.currentSource) : false;
  }

  /** Whether the current source plays through browser decoders that run in another window. */
  get playsInOtherWindow(): boolean {
    return !this.decodesBrowserMedia && this.currentSource !== null && this.native?.backend === "browser";
  }

  /** Connect to the native session; `decodesBrowserMedia` is false in a window that only presents it. */
  init({ decodesBrowserMedia = true }: { decodesBrowserMedia?: boolean } = {}): void {
    if (this.initialized) return;
    this.initialized = true;
    this.decodesBrowserMedia = decodesBrowserMedia;
    this.externalControls.init();
    window.addEventListener("online", this.handleConnectivityChange);
    window.addEventListener("offline", this.handleConnectivityChange);
    document.addEventListener("visibilitychange", this.handleVisibilityChange);
    this.deviceVolumeLevel = this.deviceVolume.read();
    this.unsubscribeDeviceVolume = this.deviceVolume.subscribe(this.refreshDeviceVolume);
    this.unsubscribeVault = onActiveVaultIdentityChange((previous, next) => {
      if (previous !== next && next) {
        this.stopBrowser(); this.client.reset();
        void this.connect().catch((error: unknown) => this.reportError(error));
      }
    });
    void this.connect().catch((error: unknown) => this.reportError(error));
    this.observationTimer = setInterval(() => {
      void this.heartbeat();
      if (this.isYouTubeActive) this.youtubeAdapter.post({ action: "snapshot" });
    }, BROWSER_OBSERVATION_MS);
  }

  private async connect(): Promise<void> {
    await this.client.connect(document.visibilityState !== "hidden");
    await this.client.command({ kind: "online", online: this.online });
  }

  destroy(): void {
    this.initialized = false;
    if (this.observationTimer) clearInterval(this.observationTimer);
    this.observationTimer = null;
    this.client.disconnect(); this.stopBrowser(); this.externalControls.destroy(); this.hostedMediaController.destroy();
    this.unsubscribeVault?.(); this.unsubscribeVault = null;
    this.unsubscribeDeviceVolume?.(); this.unsubscribeDeviceVolume = null;
    window.removeEventListener("online", this.handleConnectivityChange); window.removeEventListener("offline", this.handleConnectivityChange);
    document.removeEventListener("visibilitychange", this.handleVisibilityChange);
  }

  private readonly handleConnectivityChange = (): void => {
    this.online = navigator.onLine;
    void this.command({ kind: "online", online: this.online }, "system").catch((error: unknown) => this.reportError(error));
  };
  private readonly handleVisibilityChange = (): void => { this.refreshDeviceVolume(); void this.heartbeat(); };
  private readonly refreshDeviceVolume = (): void => {
    const level = this.deviceVolume.read();
    if (level === this.deviceVolumeLevel) return;
    this.deviceVolumeLevel = level;
    this.externalControls.updateBrowser();
  };
  /** The device volume scales Music output, so the session keeps the app volume at full while it applies. */
  private get usesDeviceVolume(): boolean { return this.deviceVolumeLevel !== null; }
  private pinSessionVolume(volume: number): void {
    if (!this.usesDeviceVolume || volume === MAX_VOLUME || this.isPinningSessionVolume) return;
    this.isPinningSessionVolume = true;
    void this.command({ kind: "volume", volume: MAX_VOLUME }, "system")
      .catch((error: unknown) => this.reportError(error))
      .finally(() => { this.isPinningSessionVolume = false; });
  }
  private async heartbeat(): Promise<void> {
    if (!this.initialized || this.isHeartbeatRunning) return;
    this.isHeartbeatRunning = true;
    try { await this.client.host(document.visibilityState !== "hidden"); }
    catch (error) { this.reportError(error); }
    finally { this.isHeartbeatRunning = false; }
  }
  private reportError(error: unknown): void {
    this.playerError = error instanceof Error ? error.message
      : typeof error === "object" && error !== null && "message" in error ? String(error.message) : String(error);
  }

  private clearNativePresentation(): void {
    this.stopBrowser();
    this.hostedMediaController.destroy();
    this.native = null; this.entries = []; this.currentSource = null;
    this.queue = []; this.activeQueueItemIds = []; this.savedQueueEntries = [];
    this.activePlaylistId = null; this.activePlaylistName = null; this.activeSourceQueueId = null;
    this.activePlaylistRepeatMode = "off"; this.sourceQueueSnoozedItemIds = []; this.savedQueueRecentItemIds = [];
    this.savedQueueSkipBreakdown = emptyMusicSkipBreakdown();
    this.contextOwner = "manual"; this.contextPlayback = null;
    this.nativeArtworkGeneration = -1; this.currentArtworkUrl = null; this.staleVisual = null;
    this.snapshot = { ...this.snapshot, status: "idle", positionMs: 0, durationMs: null, error: null };
    this.playerError = null; this.sourceActionBusy = false;
    this.externalControls.updateBrowser();
  }

  private applySnapshot(value: NativeMusicSnapshot): void {
    this.native = value;
    if (value.queue) {
      this.entries = value.queue;
      this.queue = value.queue.map((entry) => entry.source);
      this.activeQueueItemIds = value.queue.map((entry) => entry.itemId ?? "");
      this.savedQueueEntries = value.playlistId ? value.queue.map((entry) => ({
        membershipId: entry.membershipId ?? "", itemId: entry.itemId ?? "", identityKey: entry.source.identity, source: entry.source,
        sourceKind: entry.source.kind === "local-file" ? "local-file" : "youtube-video", availability: entry.availability,
        youtubeResolutionState: entry.embeddingBlocked ? "embedding-blocked" : null, weight: entry.weight, enabled: entry.enabled,
        snoozedUntil: entry.snoozedUntil, snoozedIndefinitely: entry.snoozedIndefinitely,
        skipRanges: entry.skipRanges.map((range, index) => ({ ...range, id: String(entry.membershipId) + ":" + index, membershipId: entry.membershipId ?? "", sortOrder: index })),
        volume: entry.volume, rate: entry.rate,
      })) : [];
    }
    this.currentSource = value.currentSource;
    this.snapshot = { status: value.status, positionMs: value.positionMs, durationMs: value.durationMs, volume: value.volume, rate: value.rate, error: value.error };
    const { t } = getLocalization();
    this.playerError = value.error ?? (value.issue === "browser-host-unavailable"
      ? t(this.decodesBrowserMedia ? "music.nativeSession.browserUnavailable" : "music.nativeSession.browserUnavailableOtherWindow")
      : value.issue === "interrupted" ? t("music.nativeSession.interrupted") : null);
    this.muted = value.muted; this.shuffleEnabled = value.order !== "in-order"; this.mixEnabled = value.order === "mix";
    this.pinSessionVolume(value.volume);
    this.activePlaylistId = value.playlistId; this.activePlaylistName = value.queueName || null; this.activePlaylistRepeatMode = value.repeatMode;
    this.contextOwner = value.owner;
    if (value.context) {
      const phaseKey = value.context.phase === "focus" ? "music.assignment.phase.focus"
        : value.context.phase === "short-break" ? "music.assignment.phase.short-break" : "music.assignment.phase.long-break";
      this.contextPlayback = { ...value.context, owner: value.owner === "calendar-event" ? "calendar-event" : "pomodoro", playlistName: value.queueName || null,
        displayLabel: t("music.assignment.context.selectedBy", t(phaseKey), value.context.eventTitle) };
    } else if (value.owner === "manual") this.contextPlayback = null;
    this.sourceQueueSnoozedItemIds = this.entries.filter((entry) => entry.snoozedIndefinitely || (entry.snoozedUntil ?? 0) > Date.now()).flatMap((entry) => entry.itemId ? [entry.itemId] : []);
    this.savedQueueSkipBreakdown = emptyMusicSkipBreakdown();
    for (const entry of this.entries) {
      const reason = !entry.enabled ? "disabled" : !entry.phaseAllowed ? "phase-constraint"
        : entry.snoozedIndefinitely || (entry.snoozedUntil ?? 0) > Date.now() ? "snoozed"
        : !entry.bound ? "unbound-root" : !this.online && entry.source.kind !== "local-file" ? "offline"
        : entry.embeddingBlocked ? "embedding-blocked" : entry.availability !== "available" ? "unavailable" : null;
      if (reason) this.savedQueueSkipBreakdown[reason] += 1;
    }
    if (value.backend === "native-audio" && value.currentSource?.kind === "local-file" && this.nativeArtworkGeneration !== value.generation) {
      this.nativeArtworkGeneration = value.generation;
      void this.loadArtwork(value.currentSource, value.generation).catch((error: unknown) => {
        if (this.native?.sessionId === value.sessionId && this.native.generation === value.generation) this.reportError(error);
      });
    }
    this.externalControls.updateBrowser();
  }

  private async loadArtwork(source: MusicSource, generation: number): Promise<void> {
    if (source.kind !== "local-file") return;
    const hostedGeneration = this.hostedMediaController.nextGeneration();
    this.hostedMediaController.startVisualTransition();
    const artwork = source.artworkPath ? await registerMediaFile(source.artworkPath, hostedGeneration) : await registerEmbeddedArtwork(source.path, hostedGeneration);
    if (this.native?.generation !== generation) {
      if (artwork) await this.hostedMediaController.unregisterGeneration([artwork], hostedGeneration);
      return;
    }
    this.currentArtworkUrl = artwork; this.localHasVideo = false;
    await this.hostedMediaController.retainCurrent(hostedGeneration);
  }

  private stopBrowser(): void {
    this.loadRuntime.begin(); this.youtubeAdapter.destroy(); this.webviewLocalAdapter.reset();
    this.browserSessionId = null; this.youtubeHostUrl = null; this.youtubeHostReady = false; this.youtubeHostToken = null;
    this.externalControls.updateBrowser();
  }

  private async applyEffect(effect: NativeMusicEffect): Promise<void> {
    if (effect.kind === "stop") { this.stopBrowser(); return; }
    if (effect.kind === "load") {
      this.stopBrowser();
      this.browserSessionId = effect.sessionId; this.browserGeneration = effect.generation; this.browserSequence = 0;
      const load = this.loadRuntime.begin();
      this.currentSource = effect.source; this.muted = effect.muted;
      this.snapshot = { ...this.snapshot, status: "loading", positionMs: effect.positionMs, volume: effect.volume, rate: effect.rate, error: null };
      if (isYouTubeSource(effect.source)) {
        await this.youtubeAdapter.load(effect.source, effect.positionMs, load, effect.autoplay);
      } else {
        const source = effect.source;
        const hostedGeneration = this.hostedMediaController.nextGeneration();
        const probe = await probeLocalMedia(source.path);
        const media = await registerMediaFile(source.path, hostedGeneration);
        if (!this.loadRuntime.isCurrent(load)) { await this.hostedMediaController.unregisterGeneration([media], hostedGeneration); return; }
        this.localMediaSrc = media; this.localHasVideo = true; this.localBackendKind = "webview";
        this.webviewLocalAdapter.prepare(effect.positionMs, probe.playableStartMs ?? 0);
        await this.hostedMediaController.retainCurrent(hostedGeneration); await tick();
        if (!this.loadRuntime.isCurrent(load)) return;
        this.webviewLocalAdapter.configure();
        if (effect.autoplay) await this.webviewLocalAdapter.play();
      }
      await this.observeBrowser();
      this.externalControls.updateBrowser();
      return;
    }
    if (effect.generation !== this.browserGeneration || !this.browserSessionId) return;
    if (effect.kind === "settings") {
      this.muted = effect.muted; this.snapshot = { ...this.snapshot, volume: effect.volume, rate: effect.rate };
      if (this.isYouTubeActive) { this.youtubeAdapter.post({ action: "volume", volume: this.effectiveVolume() }); this.youtubeAdapter.post({ action: "rate", rate: effect.rate }); }
      else { this.webviewLocalAdapter.applyVolume(); if (this.localMediaElement) this.localMediaElement.playbackRate = effect.rate; }
    } else if (this.isYouTubeActive) {
      this.youtubeAdapter.post(effect.kind === "seek" ? { action: "seek", positionMs: effect.positionMs } : { action: effect.kind, volume: this.effectiveVolume() });
    } else if (effect.kind === "play") await this.webviewLocalAdapter.play();
    else if (effect.kind === "pause") this.webviewLocalAdapter.pause();
    else if (effect.kind === "seek") this.webviewLocalAdapter.seek(effect.positionMs);
  }

  private observeBrowser(): Promise<void> {
    if (!this.browserSessionId || !this.currentSource) return Promise.resolve();
    return this.observationSender.request();
  }

  private async sendBrowserObservation(): Promise<void> {
    if (!this.browserSessionId || !this.currentSource) return;
    try {
      await this.client.observe({ sessionId: this.browserSessionId, generation: this.browserGeneration, sequence: ++this.browserSequence,
        sourceIdentity: this.currentSource.identity, status: this.snapshot.status, positionMs: Math.max(0, Math.round(this.snapshot.positionMs)),
        durationMs: this.snapshot.durationMs === null ? null : Math.max(0, Math.round(this.snapshot.durationMs)), error: this.snapshot.error });
    } catch (error) { this.reportError(error); }
  }

  private async command(intent: NativeMusicIntent, origin: MusicPlaybackActionOrigin = "manual"): Promise<NativeMusicSnapshot> {
    await this.client.connect();
    if (origin === "manual") this.manualPlaybackActionVersion += 1;
    return this.client.command(intent);
  }
  private async start(queue: NativeMusicQueueIntent, autoplay = true, resume = false): Promise<NativeMusicSnapshot> {
    await this.client.connect();
    this.manualPlaybackActionVersion += 1;
    return this.client.start({ queue, autoplay, resume, order: this.playbackMode, volume: this.usesDeviceVolume ? MAX_VOLUME : this.snapshot.volume, muted: this.muted, rate: this.snapshot.rate });
  }

  async loadFromInput(): Promise<void> {
    const parsed = parseMusicSourceInput(this.sourceInput); this.parseError = parsed.error;
    if (parsed.source) await this.loadSource(parsed.source, { autoplay: true });
  }
  async loadFolder(): Promise<void> {
    this.sourceActionBusy = true;
    try {
      const selected = await pickMediaFolder(); if (!selected) return;
      this.folderScanTruncated = selected.truncated;
      const sources = selected.tracks.map((track) => localFileSourceFromPath(track.path, track.title, track.artworkPath));
      if (sources.length) await this.start({ kind: "sources", sources, selectedIndex: null, name: sources[0]?.title ?? "" });
    } finally { this.sourceActionBusy = false; }
  }
  async loadSource(source: MusicSource, options: LoadSourceOptions = {}): Promise<void> {
    if (source.kind === "local-file" && !(await confirmLocalMusicSources([source.path, ...(source.artworkPath ? [source.artworkPath] : [])]))) return;
    await this.start({ kind: "sources", sources: [source], selectedIndex: 0, name: source.title }, options.autoplay ?? false, options.resume ?? true);
  }
  async loadReviewItem(itemId: string, autoplay: boolean): Promise<void> { await this.start({ kind: "review-item", itemId }, autoplay); }
  async suspendForReview(): Promise<MusicReviewPlaybackCheckpoint> {
    const snapshot = await this.command({ kind: "suspend-review" }, "system");
    if (!snapshot.reviewCheckpointId) throw new Error("Music review checkpoint was not created");
    return { checkpointId: snapshot.reviewCheckpointId };
  }
  async restoreAfterReview(checkpoint: MusicReviewPlaybackCheckpoint): Promise<void> {
    await this.command({ kind: "restore-review", checkpointId: checkpoint.checkpointId }, "system");
  }
  async loadSavedPlaylist(playlistId: string, _name: string, _entries: MusicSavedQueueEntry[], _shuffle: boolean, _repeat: MusicRepeatMode,
    _mix = false, options: MusicSavedPlaylistLoadOptions = {}): Promise<boolean> {
    const result = await this.start({ kind: "saved-playlist", playlistId, explicitItemId: options.explicitItemId ?? null, avoidItemId: options.avoidItemId ?? null }, options.autoplay ?? true);
    if (options.context) this.setContextPlayback(options.context);
    return result.currentIndex !== null;
  }
  async loadSourceQueue(queueId: string, name: string, entries: readonly MusicSourceQueueEntry[], selectedItemId?: string): Promise<boolean> {
    if (!entries.length) return false;
    const result = await this.start({ kind: "library-items", itemIds: entries.map((entry) => entry.itemId), selectedItemId: selectedItemId ?? null, name });
    this.activeSourceQueueId = queueId; return result.currentIndex !== null;
  }
  setContextPlayback(context: MusicContextPlayback): void { this.contextPlayback = context; this.contextOwner = context.owner; }
  clearContextPlayback(): void { this.contextPlayback = null; this.contextOwner = "manual"; }
  /** Resolves whether the retry command succeeded; failures are reported through the player error. */
  async requestContextRetry(): Promise<boolean> {
    this.contextRetryRequest += 1;
    try {
      await this.command({ kind: "retry-context" }, "context");
      return true;
    } catch (error: unknown) {
      this.reportError(error);
      return false;
    }
  }
  inspectContextAssignment(): void {
    if (!this.contextPlayback) return;
    window.dispatchEvent(new CustomEvent("ganbaru:inspect-music-assignment", { detail: { eventId: this.contextPlayback.eventId } }));
    focusMusicWindow();
  }
  async retrySavedPlaylist(): Promise<void> { await this.command({ kind: "refresh" }, "system"); await this.command({ kind: "play" }); }
  reconcileSavedPlaylist(_entries: MusicSavedQueueEntry[], _skipped?: Record<MusicPlaylistSkipReason, number>): void { this.refreshQueue(); }
  detachDeletedPlaylist(_playlistId: string): void { this.refreshQueue(); }
  private refreshQueue(): void { void this.command({ kind: "refresh" }, "system").catch((error: unknown) => this.reportError(error)); }
  async togglePlay(origin: MusicPlaybackActionOrigin = "manual"): Promise<void> { await this.command({ kind: "toggle" }, origin); }
  async playPlayback(origin: MusicPlaybackActionOrigin = "manual"): Promise<void> { await this.command({ kind: "play" }, origin); }
  async pausePlayback(origin: MusicPlaybackActionOrigin = "manual"): Promise<void> { await this.command({ kind: "pause" }, origin); }
  async stopPlayback(origin: MusicPlaybackActionOrigin = "manual"): Promise<void> { await this.command({ kind: "stop" }, origin); }
  async resetPlayer(): Promise<void> { await this.stopPlayback(); }
  async seekToMs(positionMs: number): Promise<void> { await this.command({ kind: "seek", positionMs: Math.max(0, Math.round(positionMs)) }); }
  async seekByMs(deltaMs: number): Promise<void> { await this.command({ kind: "seek-by", deltaMs: Math.round(deltaMs) }); }
  async setVolume(volume: number): Promise<void> {
    if (this.usesDeviceVolume) {
      this.deviceVolumeLevel = this.deviceVolume.set(clampVolume(volume));
      if (this.muted) await this.command({ kind: "muted", muted: false });
      this.externalControls.updateBrowser();
    } else {
      await this.command({ kind: "volume", volume: clampVolume(volume) }); await this.command({ kind: "muted", muted: false });
    }
    this.volumeFeedbackId += 1; this.persistPlayerSettings();
  }
  async toggleMute(): Promise<void> { await this.command({ kind: "muted", muted: !this.muted }); this.volumeFeedbackId += 1; this.persistPlayerSettings(); }
  async adjustVolume(delta: number): Promise<void> { await this.setVolume(this.volumeControlValue + delta); }
  async setRate(rate: number): Promise<void> { await this.command({ kind: "rate", rate: clampRate(rate) }); this.persistPlayerSettings(); }
  async setTransientRate(rate: number): Promise<void> { await this.command({ kind: "rate", rate: clampRate(rate) }, "system"); }
  toggleShuffle(): void { this.setPlaybackMode(this.shuffleEnabled ? "in-order" : "shuffle"); }
  setPlaybackMode(order: MusicPlaybackMode): void {
    void this.command({ kind: "order", order }).then(() => this.persistPlayerSettings()).catch((error: unknown) => this.reportError(error));
  }
  setPlaylistVisible(visible: boolean): void { this.playlistVisible = visible; this.persistPlayerSettings(); }
  async playQueueItem(index: number): Promise<void> { await this.command({ kind: "select", index }); }
  async playNextTrack(): Promise<void> { await this.command({ kind: "next" }); }
  async playPreviousTrack(): Promise<void> { await this.command({ kind: "previous" }); }
  applyCurrentQueueSnooze(_endsAt: number | null): void { this.refreshQueue(); }
  applyQueueItemSnooze(_index: number, _endsAt: number | null): void { this.refreshQueue(); }
  clearCurrentQueueSnooze(): void { this.refreshQueue(); }
  clearQueueItemSnooze(_index: number): void { this.refreshQueue(); }
  applyCurrentQueueWeight(_weight: MusicWeight): void { this.refreshQueue(); }
  applyLibraryMetadata(_itemId: string, _identityKey: string, _title: string, _artworkUrl?: string | null): void { this.refreshQueue(); }
  claimSurface(owner: string, element: HTMLElement, priority = 0): () => void { return this.surfaceClaims.claim(owner, element, priority); }
  registerYouTubeFrame(frame: HTMLIFrameElement | null): void { this.youtubeAdapter.registerFrame(frame); }
  registerYouTubeNativeHost(host: NativeYouTubeHost | null): void { this.youtubeAdapter.registerNativeHost(host); }
  handleYouTubeNativeData(data: unknown): void { this.youtubeAdapter.handleNativeData(data); }
  registerLocalMedia(element: HTMLMediaElement | null): void { this.webviewLocalAdapter.register(element); }
  resumeSnapshotScheduler(): void { void this.heartbeat(); if (this.isYouTubeActive) this.youtubeAdapter.post({ action: "snapshot" }); }
  handleVolumeWheel(event: WheelEvent): void {
    if (event.ctrlKey) return;
    const target = event.target;
    if (target instanceof HTMLElement && (target.closest("[data-music-scrollable='true']")
      || (target.closest("input, textarea, [contenteditable='true']") && !target.closest("[data-music-volume-control='true']")))) return;
    event.preventDefault(); event.stopPropagation();
    const delta = event.deltaY || -event.deltaX;
    if (delta) void this.adjustVolume(delta > 0 ? -0.05 : 0.05).catch((error: unknown) => this.reportError(error));
  }
  private effectiveVolume(): number { return this.muted ? 0 : this.usesDeviceVolume ? MAX_VOLUME : this.volumeControlValue; }
  private persistPlayerSettings(): void {
    persistMusicPlayerSettings({ volume: this.snapshot.volume, rate: this.snapshot.rate, shuffleEnabled: this.shuffleEnabled,
      mixEnabled: this.mixEnabled, muted: this.muted, playlistVisible: this.playlistVisible });
  }
  handleLocalLoadedMetadata(event: Event): void {
    this.webviewLocalAdapter.handleLoadedMetadata(event);
  }

  handleLocalLoadedData(event: Event): void {
    this.webviewLocalAdapter.handleLoadedData(event);
  }

  handleLocalTimeUpdate(event: Event): void {
    this.webviewLocalAdapter.handleTimeUpdate(event);
  }

  handleLocalPlay(event: Event): void {
    this.webviewLocalAdapter.handlePlay(event);
  }

  handleLocalPause(event: Event): void {
    this.webviewLocalAdapter.handlePause(event);
  }

  async handleLocalEnded(event: Event): Promise<void> {
    await this.webviewLocalAdapter.handleEnded(event);
  }

  handleLocalError(event: Event): void {
    this.webviewLocalAdapter.handleError(event);
  }

  handleArtworkLoaded(): void {
    this.hostedMediaController.finishVisualTransitionAfterPaint();
  }

  handleArtworkError(): void {
    this.currentArtworkUrl = null;
    this.hostedMediaController.syncRetention();
  }

}

let store: MusicPlayerStore | null = null;
export function getMusicPlayer(): MusicPlayerStore { return store ??= new MusicPlayerStore(); }
