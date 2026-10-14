<script lang="ts">
  import Settings from "@lucide/svelte/icons/settings";
  import Music from "@lucide/svelte/icons/music";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import { mobileCenteredPanelGeometry } from "$lib/mobile/layout";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  type PomodoroMenuComponent = typeof import("$lib/components/pomodoro/PomodoroMenuContent.svelte").default;
  type LinkedDeviceControlComponent = typeof import("$lib/components/vault/handoff/LinkedDeviceControl.svelte").default;

  let {
    pomodoroTime,
    pomodoroActive,
    pomodoroRemainingSeconds,
    pomodoroTotalSeconds,
    pomodoroPaused,
    pomodoroPausedPulseAmount,
    pomodoroOpen,
    PomodoroMenuSurface,
    LinkedDeviceControlSurface,
    quickNotesOpen,
    quickNotesDisabled,
    musicOpen,
    musicDisabled,
    musicVisible = true,
    onTogglePomodoro,
    onClosePomodoro,
    onOpenQuickNotes,
    onOpenMusic,
    onOpenSettings,
    identityTarget = $bindable(null),
    identityLabel = null,
  }: {
    pomodoroTime: string;
    pomodoroActive: boolean;
    pomodoroRemainingSeconds: number;
    pomodoroTotalSeconds: number;
    pomodoroPaused: boolean;
    pomodoroPausedPulseAmount: number | null;
    pomodoroOpen: boolean;
    PomodoroMenuSurface: PomodoroMenuComponent;
    LinkedDeviceControlSurface: LinkedDeviceControlComponent;
    quickNotesOpen: boolean;
    quickNotesDisabled: boolean;
    musicOpen: boolean;
    musicDisabled: boolean;
    musicVisible?: boolean;
    onTogglePomodoro: () => void;
    onClosePomodoro: () => void;
    onOpenQuickNotes: () => void;
    onOpenMusic: () => void;
    onOpenSettings: () => void;
    /** The leading slot where the active destination places its context switcher. */
    identityTarget?: HTMLElement | null;
    /** Names a destination that has no context switcher, so the leading slot is not left empty. */
    identityLabel?: string | null;
  } = $props();

  const { t } = getLocalization();
  const utilityButtonClass = "group flex h-full w-11 shrink-0 items-center justify-center text-muted-foreground transition-colors disabled:opacity-40";
  const utilityIconClass = "grid h-8 w-8 place-items-center rounded-full transition-colors group-active:bg-accent/70";
  let pomodoroAnchorElement = $state<HTMLDivElement | null>(null);
  let pomodoroTriggerElement = $state<HTMLButtonElement | null>(null);
  let pomodoroMenuStyle = $state("left:50%;width:min(16rem,calc(100vw - 1rem));transform:translateX(-50%)");
  const pomodoroMenuDesiredWidth = FLOATING_WIDTH.sm;
  const pomodoroMenuViewportInset = 8;

  function rootPixelValue(property: string): number {
    const value = Number.parseFloat(
      getComputedStyle(document.documentElement).getPropertyValue(property),
    );
    return Number.isFinite(value) ? Math.max(0, value) : 0;
  }

  function updatePomodoroMenuPosition(): void {
    if (!pomodoroAnchorElement || !pomodoroTriggerElement) return;
    const triggerRect = pomodoroTriggerElement.getBoundingClientRect();
    const anchorRect = pomodoroAnchorElement.getBoundingClientRect();
    const visualViewport = window.visualViewport;
    const viewportOffsetLeft = visualViewport?.offsetLeft ?? 0;
    const viewportWidth = visualViewport?.width ?? window.innerWidth;
    const safeAreaLeft = rootPixelValue("--safe-area-left");
    const safeAreaRight = rootPixelValue("--safe-area-right");
    const geometry = mobileCenteredPanelGeometry({
      anchorLeft: triggerRect.left,
      anchorWidth: triggerRect.width,
      desiredWidth: pomodoroMenuDesiredWidth,
      viewportLeft: viewportOffsetLeft + safeAreaLeft,
      viewportWidth: Math.max(0, viewportWidth - safeAreaLeft - safeAreaRight),
      inset: pomodoroMenuViewportInset,
    });
    pomodoroMenuStyle = [
      `left:${Math.round(geometry.left - anchorRect.left)}px`,
      `width:${Math.round(geometry.width)}px`,
      "transform:none",
    ].join(";");
  }

  function togglePomodoro(): void {
    if (!pomodoroOpen) updatePomodoroMenuPosition();
    onTogglePomodoro();
  }

  $effect(() => {
    if (!pomodoroOpen || !pomodoroAnchorElement || !pomodoroTriggerElement) return;
    updatePomodoroMenuPosition();
    const visualViewport = window.visualViewport;
    window.addEventListener("resize", updatePomodoroMenuPosition);
    visualViewport?.addEventListener("resize", updatePomodoroMenuPosition);
    visualViewport?.addEventListener("scroll", updatePomodoroMenuPosition);
    return () => {
      window.removeEventListener("resize", updatePomodoroMenuPosition);
      visualViewport?.removeEventListener("resize", updatePomodoroMenuPosition);
      visualViewport?.removeEventListener("scroll", updatePomodoroMenuPosition);
    };
  });
</script>

<header
  class="mobile-top-bar flex h-(--mobile-topbar-h) shrink-0 items-center justify-between gap-1 pr-1 pl-2 text-foreground"
  style="background-color: var(--cal-bg);"
>
  <div bind:this={identityTarget} class="flex h-full min-w-0 flex-1 items-center" data-mobile-identity-slot>
    {#if identityLabel}
      <span class="min-w-0 truncate px-2 text-identity font-medium text-foreground" data-mobile-identity-label>{identityLabel}</span>
    {/if}
  </div>
  <div class="flex h-full shrink-0 items-center">
    <div bind:this={pomodoroAnchorElement} class="relative flex h-full shrink-0 items-center">
      <button
        bind:this={pomodoroTriggerElement}
        type="button"
        data-mobile-pomodoro-trigger
        onclick={togglePomodoro}
        aria-label={pomodoroActive
          ? t("titleBar.pomodoro.remaining", pomodoroTime)
          : t("titleBar.control.pomodoro")}
        aria-haspopup="menu"
        aria-expanded={pomodoroOpen}
        class={cn(utilityButtonClass, pomodoroActive && "w-auto px-1", (pomodoroOpen || pomodoroActive) && "text-foreground")}
      >
        <span class={cn(
          utilityIconClass,
          pomodoroActive && "w-auto grid-flow-col gap-1.5 pr-2.5 pl-1.5 text-sm font-medium tabular-nums",
          (pomodoroOpen || pomodoroActive) && "bg-accent/70",
        )}>
          <PomodoroProgressRing
            active={pomodoroActive}
            remainingSeconds={pomodoroRemainingSeconds}
            totalSeconds={pomodoroTotalSeconds}
            paused={pomodoroPaused}
            pausedPulseAmount={pomodoroPausedPulseAmount}
            size={20}
            trackClass="stroke-muted-foreground/35"
            progressClass="text-foreground/76 stroke-foreground/76"
          />
          {#if pomodoroActive}<span aria-hidden="true">{pomodoroTime}</span>{/if}
        </span>
      </button>
      {#if pomodoroOpen}
        <button
          type="button"
          class="fixed inset-0 z-40 h-full w-full bg-transparent"
          aria-label={t("common.close")}
          onclick={onClosePomodoro}
        ></button>
        <div
          role="menu"
          class="surface-floating absolute top-[calc(100%+0.25rem)] z-50 flex max-h-[calc(var(--visual-viewport-height)-var(--safe-area-top)-var(--mobile-topbar-h)-0.75rem)] flex-col overflow-hidden"
          style={pomodoroMenuStyle}
        >
          <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 overflow-y-auto">
            <PomodoroMenuSurface
              includeMusic={musicVisible}
              touch
              onDismiss={onClosePomodoro}
              onOpenMusic={onOpenMusic}
            />
          </div>
        </div>
      {/if}
    </div>
    <div class="flex h-full w-11 justify-center *:w-full">
      <LinkedDeviceControlSurface platform="android" presentation="mobile" onOpened={onClosePomodoro} />
    </div>
    <button
      type="button"
      data-mobile-quick-notes-trigger
      disabled={quickNotesDisabled}
      onclick={onOpenQuickNotes}
      aria-label={t("titleBar.control.quickNotes")}
      aria-haspopup="dialog"
      aria-expanded={quickNotesOpen}
      class={cn(utilityButtonClass, quickNotesOpen && "text-foreground")}
    >
      <span class={cn(utilityIconClass, quickNotesOpen && "bg-accent/70")}><StickyNote size={19} strokeWidth={1.8} aria-hidden="true" /></span>
    </button>
    {#if musicVisible}
      <button
        type="button"
        data-mobile-music-trigger
        disabled={musicDisabled}
        onclick={onOpenMusic}
        aria-label={t("titleBar.control.music")}
        aria-haspopup="dialog"
        aria-expanded={musicOpen}
        class={cn(utilityButtonClass, musicOpen && "text-foreground")}
      >
        <span class={cn(utilityIconClass, musicOpen && "bg-accent/70")}><Music size={19} strokeWidth={1.8} aria-hidden="true" /></span>
      </button>
    {/if}
    <button
      type="button"
      onclick={onOpenSettings}
      aria-label={t("titleBar.control.settings")}
      aria-haspopup="dialog"
      class={utilityButtonClass}
    >
      <span class={utilityIconClass}><Settings size={19} strokeWidth={1.8} aria-hidden="true" /></span>
    </button>
  </div>
</header>
