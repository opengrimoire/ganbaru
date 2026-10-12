<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { elapsedSecondsSince } from "$lib/pomodoro/blocked-screen";
  import PomodoroBlockedScreen from "./PomodoroBlockedScreen.svelte";

  let {
    idleSeconds,
    nativeOverlay = false,
    focusFailed = false,
    onResume,
    onVisible,
  }: {
    idleSeconds: number;
    nativeOverlay?: boolean;
    focusFailed?: boolean;
    onResume: () => void | Promise<void>;
    onVisible: () => Promise<void>;
  } = $props();

  let elapsed = $state(0);
  let overlayVisibleAtMs = Date.now();
  let tickIntervalId: ReturnType<typeof setInterval> | null = null;
  let wasFullscreen = false;
  const VISIBILITY_RETRY_MS = 5000;

  async function enterFullscreen() {
    const appWindow = getCurrentWindow();
    try {
      wasFullscreen = await appWindow.isFullscreen();
      await appWindow.setAlwaysOnTop(true);
      await appWindow.setFullscreen(true);
      await appWindow.setFocus();
    } catch (e) {
      console.warn("Failed to enter fullscreen for idle overlay:", e);
    }
  }

  async function exitFullscreen() {
    const appWindow = getCurrentWindow();
    try {
      if (!wasFullscreen) {
        await appWindow.setFullscreen(false);
      }
      await appWindow.setAlwaysOnTop(false);
    } catch (e) {
      console.warn("Failed to exit fullscreen:", e);
    }
  }

  function handleResume() {
    void Promise.resolve(onResume()).then(exitFullscreen).catch((error: unknown) => {
      console.error("Native Focus resume failed:", error);
    });
  }

  function afterNextPaint(callback: () => void): () => void {
    let cancelled = false;
    let firstFrameId = 0;
    let secondFrameId = 0;
    firstFrameId = requestAnimationFrame(() => {
      secondFrameId = requestAnimationFrame(() => {
        if (!cancelled) callback();
      });
    });

    return () => {
      cancelled = true;
      cancelAnimationFrame(firstFrameId);
      cancelAnimationFrame(secondFrameId);
    };
  }

  onMount(() => {
    let painted = false;
    let accepted = false;
    let pending = false;
    let disposed = false;
    async function reportVisibility(): Promise<void> {
      if (!painted || accepted || pending || disposed || focusFailed) return;
      pending = true;
      try {
        if (!await getCurrentWindow().isVisible() || disposed) return;
        await onVisible();
        accepted = true;
      } catch (error: unknown) {
        console.warn("Native Focus fallback visibility acknowledgement failed:", error);
      } finally {
        pending = false;
      }
    }
    elapsed = idleSeconds;
    // When the native overlay window is active, it handles fullscreen, sounds,
    // notifications, and key capture. Skip those side effects here.
    const cancelPaintSideEffects = afterNextPaint(() => {
      overlayVisibleAtMs = Date.now();
      elapsed = idleSeconds;
      painted = true;
      void reportVisibility();
      if (!nativeOverlay) void enterFullscreen();
    });

    tickIntervalId = setInterval(() => {
      elapsed = idleSeconds + elapsedSecondsSince(overlayVisibleAtMs, Date.now());
    }, 1000);
    const visibilityRetryId = setInterval(() => { void reportVisibility(); }, VISIBILITY_RETRY_MS);

    function handleKeydown(e: KeyboardEvent) {
      if (nativeOverlay) return; // Native overlay captures keys.
      if (e.code === "Space") {
        e.preventDefault();
        e.stopPropagation();
        handleResume();
      }
    }
    window.addEventListener("keydown", handleKeydown, true);

    return () => {
      disposed = true;
      clearInterval(visibilityRetryId);
      cancelPaintSideEffects();
      window.removeEventListener("keydown", handleKeydown, true);
      if (tickIntervalId !== null) clearInterval(tickIntervalId);
    };
  });
</script>

<div class="fixed inset-0 z-60">
  <PomodoroBlockedScreen
    state={focusFailed ? "idle_failed" : "idle"}
    seconds={elapsed}
  />
</div>
