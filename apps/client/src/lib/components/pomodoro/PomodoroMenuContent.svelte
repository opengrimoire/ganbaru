<script lang="ts">
  import ClockPlus from "@lucide/svelte/icons/clock-plus";
  import Coffee from "@lucide/svelte/icons/coffee";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import PauseIcon from "@lucide/svelte/icons/pause";
  import PlayIcon from "@lucide/svelte/icons/play";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import SkipForward from "@lucide/svelte/icons/skip-forward";
  import Loading from "$lib/components/ui/Loading.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { PlaybackStatus } from "$lib/music/playback";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";

  let {
    includeMusic,
    onDismiss,
    onOpenMusic,
  }: {
    includeMusic: boolean;
    onDismiss: () => void;
    onOpenMusic: () => void;
  } = $props();

  const pomodoro = getPomodoro();
  const musicPlayer = getMusicPlayer();
  const { t } = getLocalization();
  let starting = $state(false);
  let startMessage = $state("");
  const MENU_ICON_SIZE = 14;
  const MENU_ICON_STROKE_WIDTH = 1.8;
  const volumeStep = 0.05;

  const isActive = $derived(pomodoro.isActive);
  const pauseResumeLabel = $derived(
    isActive && !pomodoro.isRunning
      ? t("titleBar.pomodoro.resumeFocus")
      : t("titleBar.pomodoro.pauseFocus"),
  );
  const phaseAdvanceLabel = $derived(
    isActive
      ? pomodoro.phase === "focus"
        ? t("titleBar.pomodoro.goToBreakNow")
        : t("titleBar.pomodoro.startFocusNow")
      : t("titleBar.pomodoro.goToBreakNow"),
  );
  const musicStatusText = $derived.by(() => {
    const title = musicPlayer.currentSource ? musicPlayer.loadedTitle.trim() : "";
    if (title) return title;
    if (musicPlayer.currentSource || musicPlayer.snapshot.status !== "idle") {
      return musicStatusLabel(musicPlayer.snapshot.status);
    }
    return t("titleBar.music.noMusicLoaded");
  });
  /** A loading source with no title yet shows a spinner in the status slot instead of a status word. */
  const musicStatusLoading = $derived(
    musicPlayer.snapshot.status === "loading"
      && !(musicPlayer.currentSource && musicPlayer.loadedTitle.trim()),
  );
  const canPlayPauseMusic = $derived(Boolean(musicPlayer.currentSource) && !musicPlayer.isBusy);
  const musicPlayPauseLabel = $derived(
    musicPlayer.isPlaying ? t("titleBar.music.pause") : t("titleBar.music.play"),
  );
  const volumeProgress = $derived(musicPlayer.volumeMax > 0
    ? `${Math.min(100, Math.max(0, (musicPlayer.volumeControlValue / musicPlayer.volumeMax) * 100))}%`
    : "0%");

  function musicStatusLabel(status: PlaybackStatus): string {
    switch (status) {
      case "playing": return t("titleBar.music.status.playing");
      case "paused": return t("titleBar.music.status.paused");
      case "loading": return t("titleBar.music.status.loading");
      case "ready": return t("titleBar.music.status.ready");
      case "ended": return t("titleBar.music.status.ended");
      case "error": return t("titleBar.music.status.error");
      case "idle": return t("titleBar.music.status.idle");
    }
  }

  /** Row styling shared by every action; disabled rows take the shared disabled color from `menu-item`. */
  const MENU_ITEM_CLASS = "menu-item justify-between gap-4 whitespace-nowrap";

  function snappedVolume(value: number): number {
    if (!Number.isFinite(value)) return musicPlayer.volumeControlValue;
    return Number((Math.round(value / volumeStep) * volumeStep).toFixed(2));
  }

  function setVolume(value: number): void {
    void musicPlayer.setVolume(snappedVolume(value));
  }

  function openMusic(): void {
    onDismiss();
    onOpenMusic();
  }

  /** Start an eligible commitment only after native persistence acknowledges it. */
  async function startScheduledSession(): Promise<void> {
    if (starting) return;
    starting = true;
    startMessage = "";
    try {
      await pomodoro.startScheduledSession();
      onDismiss();
    } catch (error) {
      console.warn("Failed to accept scheduled focus session:", error);
      startMessage = typeof error === "object" && error !== null && "code" in error && error.code === "ineligible_commitment"
        ? t("pomodoroNotification.noCommitmentDue") : t("pomodoroNotification.startFailed");
    } finally {
      starting = false;
    }
  }
</script>

{#if isActive}
  <div class="menu-label">
    {pomodoro.phase !== "focus" && pomodoro.remainingSeconds === 0
      ? t("pomodoroNotification.breakCompleteTitle")
      : t("titleBar.pomodoro.left", pomodoro.formattedTime)}
  </div>
{:else}
  <div class="menu-label">
    {t("titleBar.pomodoro.noActiveSession")}
  </div>
  <button
    type="button"
    disabled={starting}
    onclick={() => { void startScheduledSession(); }}
    class={MENU_ITEM_CLASS}
  >
    <span>{t("pomodoroNotification.startScheduledSession")}</span>
    <PlayIcon class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
  </button>
  {#if startMessage}
    <p class="px-2 py-1.5 text-panel-detail text-muted-foreground" role="status">{startMessage}</p>
  {/if}
{/if}

<button
  type="button"
  onclick={() => {
    if (pomodoro.isRunning) pomodoro.pause();
    else pomodoro.start();
    onDismiss();
  }}
  disabled={!pomodoro.canPauseResume}
  class={MENU_ITEM_CLASS}
>
  <span>{pauseResumeLabel}</span>
  {#if isActive && !pomodoro.isRunning}
    <PlayIcon class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
  {:else}
    <PauseIcon class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
  {/if}
</button>

<button
  type="button"
  onclick={() => { pomodoro.addFocusTime(); onDismiss(); }}
  disabled={!pomodoro.canAddFocusTime}
  class={MENU_ITEM_CLASS}
>
  <span>{t("titleBar.pomodoro.extendFocusMinutes", 3)}</span>
  <ClockPlus class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
</button>

<button
  type="button"
  onclick={() => { pomodoro.skip(); onDismiss(); }}
  disabled={!isActive}
  class={MENU_ITEM_CLASS}
>
  <span>{phaseAdvanceLabel}</span>
  {#if pomodoro.phase === "focus" || !isActive}
    <Coffee class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
  {:else}
    <PlayIcon class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
  {/if}
</button>

{#if includeMusic}
  <div role="separator" class="menu-separator"></div>
  <div class="menu-label">
    <span class="block truncate">{#if musicStatusLoading}<Loading layout="inline" label={t("titleBar.music.status.loading")} />{:else}{musicStatusText}{/if}</span>
  </div>
  <button
    type="button"
    onclick={() => { void musicPlayer.togglePlay(); onDismiss(); }}
    disabled={!canPlayPauseMusic}
    class={MENU_ITEM_CLASS}
  >
    <span>{musicPlayPauseLabel}</span>
    {#if musicPlayer.isPlaying}
      <PauseIcon class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
    {:else}
      <PlayIcon class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
    {/if}
  </button>
  <button
    type="button"
    onclick={() => { void musicPlayer.playPreviousTrack(); onDismiss(); }}
    disabled={!musicPlayer.canPlayPreviousTrack}
    class={MENU_ITEM_CLASS}
  >
    <span>{t("titleBar.music.previous")}</span>
    <SkipBack class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
  </button>
  <button
    type="button"
    onclick={() => { void musicPlayer.playNextTrack(); onDismiss(); }}
    disabled={!musicPlayer.canPlayNextTrack}
    class={MENU_ITEM_CLASS}
  >
    <span>{t("titleBar.music.next")}</span>
    <SkipForward class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
  </button>
  <div class="flex min-h-(--panel-row-height) items-center gap-3 px-2 text-foreground">
    <span class="shrink-0">{t("titleBar.music.volume")}</span>
    <input
      type="range"
      min="0"
      max={musicPlayer.volumeMax}
      step={volumeStep}
      value={musicPlayer.volumeControlValue}
      class="pomodoro-menu-volume-slider min-w-0 flex-1"
      style={`--pomodoro-menu-volume-progress: ${volumeProgress};`}
      aria-label={t("titleBar.music.volumeLabel")}
      oninput={(event) => { setVolume(Number(event.currentTarget.value)); }}
    />
  </div>
  <button type="button" onclick={openMusic} class={MENU_ITEM_CLASS}>
    <span>{t("titleBar.music.open")}</span>
    <ExternalLink class="shrink-0 opacity-70" size={MENU_ICON_SIZE} strokeWidth={MENU_ICON_STROKE_WIDTH} />
  </button>
{/if}

<style>
  .pomodoro-menu-volume-slider {
    --pomodoro-menu-volume-thumb-size: 0.5rem;
    --pomodoro-menu-volume-track-height: 0.125rem;
    --pomodoro-menu-volume-track-color: color-mix(in srgb, var(--foreground) 18%, transparent);
    --pomodoro-menu-volume-fill-color: color-mix(in srgb, var(--foreground) 58%, transparent);
    height: var(--pomodoro-menu-volume-thumb-size);
    appearance: none;
    cursor: pointer;
    background: linear-gradient(
      to right,
      var(--pomodoro-menu-volume-fill-color) 0%,
      var(--pomodoro-menu-volume-fill-color) var(--pomodoro-menu-volume-progress),
      var(--pomodoro-menu-volume-track-color) var(--pomodoro-menu-volume-progress),
      var(--pomodoro-menu-volume-track-color) 100%
    ) center / calc(100% - var(--pomodoro-menu-volume-thumb-size)) var(--pomodoro-menu-volume-track-height) no-repeat;
  }

  .pomodoro-menu-volume-slider::-webkit-slider-runnable-track {
    height: var(--pomodoro-menu-volume-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .pomodoro-menu-volume-slider::-webkit-slider-thumb {
    width: var(--pomodoro-menu-volume-thumb-size);
    height: var(--pomodoro-menu-volume-thumb-size);
    margin-top: calc((var(--pomodoro-menu-volume-track-height) - var(--pomodoro-menu-volume-thumb-size)) / 2);
    appearance: none;
    border: 0;
    border-radius: 999px;
    background: var(--foreground);
  }

  .pomodoro-menu-volume-slider::-moz-range-track,
  .pomodoro-menu-volume-slider::-moz-range-progress {
    height: var(--pomodoro-menu-volume-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .pomodoro-menu-volume-slider::-moz-range-thumb {
    width: var(--pomodoro-menu-volume-thumb-size);
    height: var(--pomodoro-menu-volume-thumb-size);
    border: 0;
    border-radius: 999px;
    background: var(--foreground);
  }
</style>
