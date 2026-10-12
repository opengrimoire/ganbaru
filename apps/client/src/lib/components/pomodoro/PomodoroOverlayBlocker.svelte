<script lang="ts">
  import { onMount } from "svelte";
  import { emit, listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    POMODORO_OVERLAY_BLOCKER_ACTION_EVENT,
    parsePomodoroBlockedScreenState,
    pomodoroBlockedScreenPalette,
    type PomodoroOverlayBlockerAction,
    type PomodoroBlockedScreenState,
  } from "$lib/pomodoro/blocked-screen";

  interface PomodoroOverlayStateChangedPayload {
    state?: unknown;
  }

  function initialScreenState(): PomodoroBlockedScreenState {
    const params = new URLSearchParams(window.location.search);
    return parsePomodoroBlockedScreenState(params.get("screenState"));
  }

  function payloadScreenState(
    payload: PomodoroOverlayStateChangedPayload,
  ): PomodoroBlockedScreenState {
    return parsePomodoroBlockedScreenState(
      typeof payload.state === "string" ? payload.state : null,
    );
  }

  let screenState = $state<PomodoroBlockedScreenState>(initialScreenState());
  const colors = $derived(pomodoroBlockedScreenPalette(screenState));
  const screenStyle = $derived(`--blocked-screen-bg: ${colors.background};`);

  function blockEvent(event: Event): void {
    event.preventDefault();
    event.stopPropagation();
    if ("stopImmediatePropagation" in event) event.stopImmediatePropagation();
  }

  function emitBlockerAction(payload: PomodoroOverlayBlockerAction): void {
    emit(POMODORO_OVERLAY_BLOCKER_ACTION_EVENT, payload).catch((error) => {
      console.warn("Failed to forward pomodoro blocker action:", error);
    });
  }

  function handleClick(event: MouseEvent): void {
    blockEvent(event);
    emitBlockerAction({ kind: "pointer" });
  }

  function handleKeydown(event: KeyboardEvent): void {
    blockEvent(event);
    emitBlockerAction({
      kind: "keydown",
      code: event.code,
      key: event.key,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      shiftKey: event.shiftKey,
    });
  }

  onMount(() => {
    const appWindow = getCurrentWindow();
    const unlistenPromise = listen<PomodoroOverlayStateChangedPayload>(
      "pomodoro-overlay-state-changed",
      (event) => {
        screenState = payloadScreenState(event.payload);
      },
    ).catch((error) => {
      console.warn("Failed to listen for pomodoro overlay state changes:", error);
      return null;
    });
    const reinforce = (): void => {
      appWindow.setAlwaysOnTop(true).catch(() => {});
      appWindow.setFullscreen(true).catch(() => {});
    };
    reinforce();
    const timerIds = [
      setTimeout(reinforce, 100),
      setTimeout(reinforce, 500),
      setTimeout(reinforce, 1000),
    ];
    window.addEventListener("click", handleClick, true);
    window.addEventListener("keydown", handleKeydown, true);
    window.addEventListener("contextmenu", blockEvent, true);
    return () => {
      void unlistenPromise.then((unlisten) => {
        unlisten?.();
      });
      for (const id of timerIds) clearTimeout(id);
      window.removeEventListener("click", handleClick, true);
      window.removeEventListener("keydown", handleKeydown, true);
      window.removeEventListener("contextmenu", blockEvent, true);
    };
  });

  $effect(() => {
    document.documentElement.style.backgroundColor = colors.background;
    document.body.style.backgroundColor = colors.background;
    document.getElementById("app")?.style.setProperty("background-color", colors.background);
  });
</script>

<div class="blocked-screen fixed inset-0 h-screen w-screen select-none" style={screenStyle}></div>

<style>
  :global(html),
  :global(body),
  :global(#app) {
    background: var(--blocked-screen-bg, #000) !important;
  }

  .blocked-screen {
    background: var(--blocked-screen-bg);
  }
</style>
