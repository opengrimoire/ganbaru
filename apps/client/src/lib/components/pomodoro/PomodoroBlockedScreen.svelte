<script lang="ts">
  import { onMount } from "svelte";
  import {
    formatBreakExtensionHint,
    formatBreakEndEarlyShortcut,
    formatBlockedScreenDateTime,
    formatBlockedScreenDuration,
    formatBreakExtensionShortcut,
    isBlockedScreenAcknowledgementState,
    pomodoroBlockedScreenPalette,
    pomodoroBlockedScreenCopy,
    shouldShowBlockedScreenDateTime,
    type PomodoroBlockedScreenState,
  } from "$lib/pomodoro/blocked-screen";
  import {
    DEFAULT_FOCUS_BREAK_END_ESC_PRESSES,
    DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT,
    type FocusBreakEndEscPresses,
    type FocusBreakExtensionLimit,
  } from "$lib/stores/preference-options";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);

  let {
    state: screenState,
    seconds,
    extensionMinutes = 0,
    maxExtensionMinutes = DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT,
    escPresses = 0,
    breakEndEscPresses = DEFAULT_FOCUS_BREAK_END_ESC_PRESSES,
  }: {
    state: PomodoroBlockedScreenState;
    seconds: number;
    extensionMinutes?: number;
    maxExtensionMinutes?: FocusBreakExtensionLimit;
    escPresses?: number;
    breakEndEscPresses?: FocusBreakEndEscPresses;
  } = $props();

  let now = $state(new Date());
  const copy = $derived(pomodoroBlockedScreenCopy(screenState, t));
  const colors = $derived(pomodoroBlockedScreenPalette(screenState));
  const timerLabel = $derived(formatBlockedScreenDuration(seconds));
  const showDateTime = $derived(shouldShowBlockedScreenDateTime(screenState));
  const dateTimeLabel = $derived(formatBlockedScreenDateTime(now, locale));
  const extensionShortcutLabel = $derived(formatBreakExtensionShortcut());
  const acknowledgementState = $derived(isBlockedScreenAcknowledgementState(screenState));
  const showTimer = $derived(!acknowledgementState);
  const screenStyle = $derived(
    `--blocked-screen-bg: ${colors.background}; --blocked-main-text: ${colors.mainText}; --blocked-muted-text: ${colors.mutedText}; --blocked-subtle-text: ${colors.subtleText};`,
  );
  const showExtensionHint = $derived.by(() => {
    if (screenState !== "break_countdown") return false;
    return formatBreakExtensionHint(extensionMinutes, maxExtensionMinutes, undefined, t) !== null;
  });
  const skipHintKey = $derived.by(() => {
    if (screenState !== "break_countdown") return null;
    return formatBreakEndEarlyShortcut(escPresses, breakEndEscPresses);
  });

  onMount(() => {
    const intervalId = setInterval(() => {
      now = new Date();
    }, 1000);

    return () => {
      clearInterval(intervalId);
    };
  });

  $effect(() => {
    document.documentElement.style.backgroundColor = colors.background;
    document.body.style.backgroundColor = colors.background;
    document.getElementById("app")?.style.setProperty("background-color", colors.background);
  });
</script>

<div
  class="blocked-screen fixed inset-0 flex h-screen w-screen select-none flex-col items-center justify-center"
  style={screenStyle}
>
  {#if showDateTime}
    <p class="blocked-date-time blocked-main absolute inset-x-0 top-8 mx-auto max-w-[calc(100vw-3rem)] px-6 text-center">
      {dateTimeLabel}
    </p>
  {/if}

  <div class="flex flex-col items-center gap-8 px-6 text-center">
    {#if copy.title}
      <p
        class={acknowledgementState
          ? "blocked-finished-title blocked-main"
          : "blocked-kicker blocked-muted font-medium uppercase tracking-wide"}
      >
        {copy.title}
      </p>
    {/if}

    {#if showTimer}
      <p class="blocked-timer blocked-main tabular-nums">
        {timerLabel}
      </p>
    {/if}

    {#if copy.status}
      <p class="blocked-status blocked-muted">
        {copy.status}
      </p>
    {/if}

    {#if copy.subtitle}
      <p class="blocked-subtitle blocked-muted">
        {copy.subtitle}
      </p>
    {/if}
  </div>

  {#if !acknowledgementState}
    <div class="blocked-hints blocked-subtle absolute inset-x-0 bottom-12 flex flex-col items-center gap-3 px-6 text-center">
      {#if screenState === "break_countdown"}
        {#if showExtensionHint}
          <p>
            {t("pomodoroOverlay.press")} <span class="blocked-main">{extensionShortcutLabel}</span> {t("pomodoroOverlay.to")} {t("pomodoroOverlay.extendBreak")}
          </p>
        {/if}
        {#if skipHintKey}
          <p>
            {t("pomodoroOverlay.press")} <span class="blocked-main">{skipHintKey}</span> {t("pomodoroOverlay.to")} {t("pomodoroOverlay.endBreakNow")}
          </p>
        {/if}
      {:else}
        {#if copy.primaryHint}
          <p>
            {t("pomodoroOverlay.press")} <span class="blocked-main">{copy.primaryHint.key}</span> {t("pomodoroOverlay.to")} {copy.primaryHint.label}
          </p>
        {/if}
        {#if copy.secondaryHint}
          <p>
            {t("pomodoroOverlay.press")} <span class="blocked-main">{copy.secondaryHint.key}</span> {t("pomodoroOverlay.to")} {copy.secondaryHint.label}
          </p>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  :global(html),
  :global(body),
  :global(#app) {
    background: var(--blocked-screen-bg, #000) !important;
  }

  .blocked-screen {
    background: var(--blocked-screen-bg);
    color: var(--blocked-main-text);
  }

  .blocked-main {
    color: var(--blocked-main-text);
  }

  .blocked-muted {
    color: var(--blocked-muted-text);
  }

  .blocked-subtle {
    color: var(--blocked-subtle-text);
  }

  .blocked-date-time,
  .blocked-kicker,
  .blocked-subtitle {
    font-size: calc(1.75rem * var(--type-scale));
    line-height: 1.25;
  }

  .blocked-hints {
    font-size: calc(1.3125rem * var(--type-scale));
    line-height: 1.25;
  }

  .blocked-status {
    font-size: calc(2rem * var(--type-scale));
    line-height: 1.2;
  }

  .blocked-timer {
    font-family: inherit;
    font-size: calc(13rem * var(--type-scale));
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0;
    line-height: 0.85;
  }

  .blocked-finished-title {
    font-size: calc(5rem * var(--type-scale));
    font-weight: 700;
    letter-spacing: 0;
    line-height: 0.95;
  }

  @media (max-width: 900px), (max-height: 620px) {
    .blocked-timer {
      font-size: calc(10rem * var(--type-scale));
    }

    .blocked-finished-title {
      font-size: calc(4rem * var(--type-scale));
    }

    .blocked-date-time,
    .blocked-kicker,
    .blocked-subtitle {
      font-size: calc(1.4rem * var(--type-scale));
    }

    .blocked-hints {
      font-size: calc(1.05rem * var(--type-scale));
    }

    .blocked-status {
      font-size: calc(1.6rem * var(--type-scale));
    }
  }

  @media (max-width: 640px), (max-height: 460px) {
    .blocked-timer {
      font-size: calc(7rem * var(--type-scale));
    }

    .blocked-finished-title {
      font-size: calc(3rem * var(--type-scale));
    }

    .blocked-date-time,
    .blocked-kicker,
    .blocked-subtitle {
      font-size: calc(1.1rem * var(--type-scale));
    }

    .blocked-hints {
      font-size: calc(0.825rem * var(--type-scale));
    }

    .blocked-status {
      font-size: calc(1.25rem * var(--type-scale));
    }
  }

  @media (max-width: 420px), (max-height: 320px) {
    .blocked-timer {
      font-size: calc(4.75rem * var(--type-scale));
    }

    .blocked-finished-title {
      font-size: calc(2rem * var(--type-scale));
    }

    .blocked-date-time,
    .blocked-kicker,
    .blocked-subtitle {
      font-size: calc(0.85rem * var(--type-scale));
    }

    .blocked-hints {
      font-size: calc(0.6375rem * var(--type-scale));
    }

    .blocked-status {
      font-size: calc(0.95rem * var(--type-scale));
    }
  }

  @media (max-width: 300px), (max-height: 220px) {
    .blocked-timer {
      font-size: calc(3.25rem * var(--type-scale));
    }

    .blocked-finished-title {
      font-size: calc(1.5rem * var(--type-scale));
    }

    .blocked-date-time,
    .blocked-kicker,
    .blocked-subtitle {
      font-size: calc(0.7rem * var(--type-scale));
    }

    .blocked-hints {
      font-size: calc(0.525rem * var(--type-scale));
    }

    .blocked-status {
      font-size: calc(0.8rem * var(--type-scale));
    }
  }
</style>
