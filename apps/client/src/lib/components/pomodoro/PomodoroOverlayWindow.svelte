<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";
  import { reportIdleOverlayVisible, type FocusControlScope } from "$lib/api/focus";
  import { hasShortcutModifier } from "$lib/keyboard-shortcuts";
  import {
    DEFAULT_FOCUS_BREAK_END_ESC_PRESSES,
    DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT,
    parseFocusBreakExtensionLimit,
    parseFocusBreakEndEscPresses,
    type FocusBreakEndEscPresses,
    type FocusBreakExtensionLimit,
  } from "$lib/stores/preference-options";
  import {
    POMODORO_OVERLAY_BLOCKER_ACTION_EVENT,
    isBlockedScreenAcknowledgementState,
    isPomodoroCompletionScreenState,
    parsePomodoroOverlayBlockerAction,
    parsePomodoroBlockedScreenState,
    remainingSecondsUntil,
  } from "$lib/pomodoro/blocked-screen";
  import PomodoroBlockedScreen from "./PomodoroBlockedScreen.svelte";
  import type {
    PomodoroOverlayBlockerAction,
    PomodoroBlockedScreenState,
    PomodoroCompletionScreenState,
  } from "$lib/pomodoro/blocked-screen";

  const pomodoro = getPomodoro();
  const controlParams = new URLSearchParams(window.location.search);
  const controlScope: FocusControlScope = {
    vaultGeneration: Number(controlParams.get("focusVaultGeneration")),
    runId: controlParams.get("focusRunId") ?? "",
    segmentId: controlParams.get("focusSegmentId") ?? "",
  };

  type OverlayMode =
    | { kind: "idle"; initialIdleSeconds: number }
    | {
        kind: "break";
        breakEndsAtMs: number;
        breakEndEscPresses: FocusBreakEndEscPresses;
        breakExtensionLimit: FocusBreakExtensionLimit;
      }
    | { kind: "completion"; screenState: PomodoroCompletionScreenState };

  function numberParam(params: URLSearchParams, name: string, fallback: number): number {
    const value = Number(params.get(name));
    return Number.isFinite(value) && value >= 0 ? Math.floor(value) : fallback;
  }

  function breakEndEscPressesParam(params: URLSearchParams): FocusBreakEndEscPresses {
    const raw = params.get("breakEndEscPresses");
    if (raw === null) return DEFAULT_FOCUS_BREAK_END_ESC_PRESSES;
    if (raw === "disabled") return null;
    const value = Number(raw);
    return parseFocusBreakEndEscPresses(value, DEFAULT_FOCUS_BREAK_END_ESC_PRESSES);
  }

  function breakExtensionLimitParam(params: URLSearchParams): FocusBreakExtensionLimit {
    const raw = params.get("breakExtensionLimit");
    if (raw === null) return DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT;
    if (raw === "disabled") return null;
    const value = Number(raw);
    return parseFocusBreakExtensionLimit(value, DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT);
  }

  function overlayModeFromLocation(): OverlayMode {
    const params = new URLSearchParams(window.location.search);
    if (params.get("overlayKind") === "idle") {
      return {
        kind: "idle",
        initialIdleSeconds: numberParam(params, "idleSeconds", 0),
      };
    }

    if (params.get("overlayKind") === "completion") {
      const screenState = parsePomodoroBlockedScreenState(params.get("screenState"));
      return {
        kind: "completion",
        screenState: isPomodoroCompletionScreenState(screenState)
          ? screenState
          : "event_finished",
      };
    }

    const fallbackBreakSeconds = numberParam(params, "breakSeconds", 0);
    return {
      kind: "break",
      breakEndsAtMs: numberParam(
        params,
        "breakEndsAtMs",
        Date.now() + fallbackBreakSeconds * 1000,
      ),
      breakEndEscPresses: breakEndEscPressesParam(params),
      breakExtensionLimit: breakExtensionLimitParam(params),
    };
  }

  const mode = overlayModeFromLocation();
  let seconds = $state(
    mode.kind === "idle"
      ? mode.initialIdleSeconds
      : mode.kind === "break"
        ? remainingSecondsUntil(mode.breakEndsAtMs, Date.now())
        : 0,
  );
  let screenState = $state<PomodoroBlockedScreenState>(
    mode.kind === "idle"
      ? "idle"
      : mode.kind === "break"
        ? "break_countdown"
        : mode.screenState,
  );
  let extensionMinutes = $state(0);
  let escPresses = $state(0);
  let closed = false;
  let actionPending = false;
  const IDLE_VISIBILITY_RETRY_MS = 5000;

  function dispatchAction(action: () => Promise<void>): void {
    if (actionPending || closed) return;
    actionPending = true;
    void action().catch((error: unknown) => {
      console.error("Native Focus overlay action failed:", error);
    }).finally(() => { actionPending = false; });
  }
  const breakEndEscPresses = mode.kind === "break"
    ? mode.breakEndEscPresses
    : DEFAULT_FOCUS_BREAK_END_ESC_PRESSES;
  const breakExtensionLimit = mode.kind === "break"
    ? mode.breakExtensionLimit
    : DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT;

  function closeOverlay(): void {
    if (closed) return;
    closed = true;
    invoke("dismiss_pomodoro_completion", { scope: controlScope }).catch((error: unknown) => {
      closed = false;
      console.warn("Failed to close pomodoro overlay:", error);
    });
  }

  function reinforceFullscreen(): void {
    const appWindow = getCurrentWindow();
    appWindow.setAlwaysOnTop(true).catch(() => {});
    appWindow.setFullscreen(true).catch(() => {});
    appWindow.setFocus().catch(() => {});
  }

  async function acknowledgeBreak(): Promise<void> {
    if (screenState !== "break_finished") return;
    await pomodoro.controlOverlay({ kind: "advance" }, controlScope);
  }

  async function skipBreak(): Promise<void> {
    await pomodoro.controlOverlay({ kind: "skip_break" }, controlScope);
  }

  async function extendBreak(): Promise<void> {
    if (breakExtensionLimit === null) return;
    if (extensionMinutes >= breakExtensionLimit) return;
    await pomodoro.controlOverlay({ kind: "extend_break", seconds: 60 }, controlScope);
    escPresses = 0;
  }

  async function resumeIdle(): Promise<void> {
    await pomodoro.controlOverlay({ kind: "resolve_idle", resume: true }, controlScope);
  }

  function handleKeyCommand(command: {
    code: string;
    key: string;
    ctrlKey: boolean;
    metaKey: boolean;
    shiftKey: boolean;
  }): void {
    if (mode.kind === "completion") {
      closeOverlay();
      return;
    }

    if (mode.kind === "idle") {
      if (command.code === "Space") {
        dispatchAction(resumeIdle);
      }
      return;
    }

    if (screenState === "break_finished") {
      dispatchAction(acknowledgeBreak);
      return;
    }

    if (command.code === "Space" && command.shiftKey && hasShortcutModifier(command)) {
      if (breakExtensionLimit !== null) {
        dispatchAction(extendBreak);
      }
      return;
    }

    if (command.key === "Escape") {
      if (breakEndEscPresses === null) return;
      escPresses = Math.min(breakEndEscPresses, escPresses + 1);
      if (escPresses >= breakEndEscPresses) {
        dispatchAction(skipBreak);
      }
      return;
    }

    escPresses = 0;
  }

  function handleKeydown(event: KeyboardEvent): void {
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation();
    handleKeyCommand({
      code: event.code,
      key: event.key,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      shiftKey: event.shiftKey,
    });
  }

  function handleClick(): void {
    if (mode.kind === "completion") {
      closeOverlay();
      return;
    }

    if (mode.kind === "break" && screenState === "break_finished") {
      dispatchAction(acknowledgeBreak);
    }
  }

  function handleBlockerAction(action: PomodoroOverlayBlockerAction): void {
    if (action.kind === "pointer") {
      if (mode.kind === "completion") {
        closeOverlay();
      } else if (mode.kind === "break" && isBlockedScreenAcknowledgementState(screenState)) {
        dispatchAction(acknowledgeBreak);
      } else {
        reinforceFullscreen();
      }
      return;
    }

    handleKeyCommand(action);
  }

  onMount(() => {
    let painted = false;
    let visibilityAccepted = false;
    let visibilityPending = false;
    let disposed = false;
    let secondPaintFrame = 0;
    const idleDetectedAtMs = Number(controlParams.get("focusIdleDetectedAtMs"));
    async function reportIdleVisibility(): Promise<void> {
      if (mode.kind !== "idle" || !painted || disposed || closed || visibilityAccepted || visibilityPending) return;
      if (!controlParams.has("focusIdleDetectedAtMs")) return;
      visibilityPending = true;
      try {
        if (!await getCurrentWindow().isVisible() || disposed || closed) return;
        await reportIdleOverlayVisible({ ...controlScope, idleDetectedAtMs });
        visibilityAccepted = true;
      } catch (error: unknown) {
        console.warn("Native Focus idle visibility acknowledgement failed:", error);
      } finally {
        visibilityPending = false;
      }
    }
    const firstPaintFrame = requestAnimationFrame(() => {
      secondPaintFrame = requestAnimationFrame(() => {
        painted = true;
        void reportIdleVisibility();
      });
    });
    const visibilityRetryId = mode.kind === "idle"
      ? setInterval(() => { void reportIdleVisibility(); }, IDLE_VISIBILITY_RETRY_MS)
      : null;
    reinforceFullscreen();
    const fullscreenTimerIds = [
      setTimeout(reinforceFullscreen, 100),
      setTimeout(reinforceFullscreen, 500),
      setTimeout(reinforceFullscreen, 1000),
    ];
    const focusIntervalId = setInterval(reinforceFullscreen, 2000);
    const unlistenBlockerActionPromise = listen<unknown>(
      POMODORO_OVERLAY_BLOCKER_ACTION_EVENT,
      (event) => {
        const action = parsePomodoroOverlayBlockerAction(event.payload);
        if (action !== null) handleBlockerAction(action);
      },
    ).catch((error) => {
      console.warn("Failed to listen for pomodoro blocker actions:", error);
      return null;
    });
    window.addEventListener("keydown", handleKeydown, true);

    return () => {
      disposed = true;
      cancelAnimationFrame(firstPaintFrame);
      cancelAnimationFrame(secondPaintFrame);
      if (visibilityRetryId !== null) clearInterval(visibilityRetryId);
      void unlistenBlockerActionPromise.then((unlisten) => {
        unlisten?.();
      });
      for (const id of fullscreenTimerIds) clearTimeout(id);
      clearInterval(focusIntervalId);
      window.removeEventListener("keydown", handleKeydown, true);
    };
  });

  $effect(() => {
    if (mode.kind === "completion") return;
    const snapshot = pomodoro.nativeSnapshot;
    if (!snapshot || snapshot.run?.id !== controlScope.runId || snapshot.segment?.id !== controlScope.segmentId) return;
    if (mode.kind === "idle") {
      screenState = snapshot.mode === "idle_failed" ? "idle_failed" : "idle";
      seconds = pomodoro.idleElapsedSeconds;
    } else {
      screenState = snapshot.mode === "return_wait" ? "break_finished" : "break_countdown";
      seconds = snapshot.mode === "return_wait" ? pomodoro.breakOvertimeSeconds : pomodoro.remainingSeconds;
      extensionMinutes = Math.floor(snapshot.breakExtensionMs / 60_000);
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="h-full w-full" onclick={handleClick}>
  <PomodoroBlockedScreen
    state={screenState}
    {seconds}
    {extensionMinutes}
    maxExtensionMinutes={breakExtensionLimit}
    {escPresses}
    {breakEndEscPresses}
  />
</div>
