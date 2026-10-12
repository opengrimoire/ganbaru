<script lang="ts">
  import Music from "@lucide/svelte/icons/music";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import PomodoroMenuContent from "$lib/components/pomodoro/PomodoroMenuContent.svelte";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import LinkedDeviceControl from "$lib/components/vault/handoff/LinkedDeviceControl.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";
  import { cn } from "$lib/utils";

  let {
    showPomodoro,
    showMusic,
    quickNotesOpen,
    musicPanelOpen,
    showMenu = $bindable(),
    isMainWindow,
    onMenuOpened,
    onToggleQuickNotes,
    onToggleMusic,
  }: {
    showPomodoro: boolean;
    showMusic: boolean;
    quickNotesOpen: boolean;
    musicPanelOpen: boolean;
    showMenu: boolean;
    isMainWindow: boolean;
    onMenuOpened: () => void;
    onToggleQuickNotes: () => void;
    onToggleMusic: () => void;
  } = $props();

  const musicPlayer = getMusicPlayer();
  const pomodoro = getPomodoro();
  const { t } = getLocalization();

  const TITLE_BAR_ICON_COLOR_CLASS = "text-foreground/68 dark:text-white/76";
  const TITLE_BAR_ICON_STROKE_CLASS = "stroke-foreground/68 dark:stroke-white/76";
  const TITLE_BAR_SUBTLE_STROKE_CLASS = "stroke-foreground/20 dark:stroke-white/20";
  const TITLE_BAR_ICON_STROKE_WIDTH = 1.5;
  const TITLE_BAR_ICON_SIZE = 14;
  const POMODORO_RING_SIZE = TITLE_BAR_ICON_SIZE + 0.5;
  const volumeStep = 0.05;

  const isActive = $derived(pomodoro.isActive);
  const pomodoroPausedPulseActive = $derived(
    isActive && pomodoro.phase === "focus" && !pomodoro.isRunning
      && !pomodoro.suspendedAway && !pomodoro.idlePaused,
  );
  const musicVolumeTooltipLine = $derived(
    t("titleBar.music.volumeTooltip", musicPlayer.volumePercentLabel),
  );
  const pomodoroButtonTooltip = $derived(
    `${isActive ? t("titleBar.pomodoro.remaining", pomodoro.formattedTime) : t("titleBar.control.pomodoro")}\n${musicVolumeTooltipLine}`,
  );
  const musicButtonTooltip = $derived(
    `${t("titleBar.control.music")}\n${musicVolumeTooltipLine}${musicPlayer.contextPlayback && musicPlayer.contextPlayback.state !== "overridden" ? `\n${t("titleBar.music.contextual", t(`music.assignment.phase.${musicPlayer.contextPlayback.phase}`), musicPlayer.contextPlayback.eventTitle)}` : ""}`,
  );

  function toggleMenu(): void {
    const nextOpen = !showMenu;
    showMenu = nextOpen;
    if (nextOpen) onMenuOpened();
  }

  function snappedVolume(value: number): number {
    if (!Number.isFinite(value)) return musicPlayer.volumeControlValue;
    return Number((Math.round(value / volumeStep) * volumeStep).toFixed(2));
  }

  function setTitleBarVolume(value: number): void {
    void musicPlayer.setVolume(snappedVolume(value));
  }

  export function handleVolumeWheel(event: WheelEvent): void {
    event.preventDefault();
    event.stopPropagation();
    if (event.ctrlKey) return;
    const delta = event.deltaY === 0 ? -event.deltaX : event.deltaY;
    if (delta === 0) return;
    setTitleBarVolume(
      musicPlayer.volumeControlValue + (delta > 0 ? -volumeStep : volumeStep),
    );
  }

</script>

    <!-- Pomodoro progress ring with dropdown -->
    {#if showPomodoro}
      <div class="relative">
        <button
          onclick={toggleMenu}
          onwheel={handleVolumeWheel}
          class={cn(
            "titlebar-icon-button flex items-center justify-center rounded-lg transition-colors",
            showMenu ? "bg-sidebar-accent" : "hover:bg-sidebar-accent",
          )}
          title={pomodoroButtonTooltip}
          aria-haspopup="menu"
          aria-expanded={showMenu}
        >
          <PomodoroProgressRing
            active={isActive}
            remainingSeconds={pomodoro.remainingSeconds}
            totalSeconds={pomodoro.totalSecondsForPhase}
            paused={pomodoroPausedPulseActive}
            pausedPulseAmount={pomodoro.pausedPulseAmount}
            size={POMODORO_RING_SIZE}
            trackClass={TITLE_BAR_SUBTLE_STROKE_CLASS}
            progressClass={`${TITLE_BAR_ICON_COLOR_CLASS} ${TITLE_BAR_ICON_STROKE_CLASS}`}
          />
        </button>
        {#if showMenu}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="fixed inset-0 z-40"
            onclick={() => { showMenu = false; }}
            onkeydown={(e) => { if (e.key === "Escape") showMenu = false; }}
            onwheel={handleVolumeWheel}
          ></div>
          <div
            class="surface-floating surface-floating-body absolute right-0 top-9 z-50 w-floating-sm"
            onwheel={handleVolumeWheel}
          >
            <PomodoroMenuContent
              includeMusic={isMainWindow}
              onDismiss={() => { showMenu = false; }}
              onOpenMusic={onToggleMusic}
            />
          </div>
        {/if}
      </div>
    {/if}

    <LinkedDeviceControl
      platform="desktop"
      presentation="desktop"
      onOpened={() => {
        showMenu = false;
        onMenuOpened();
      }}
    />

    <button
      type="button"
      onclick={onToggleQuickNotes}
      class={cn(
        "titlebar-icon-button flex items-center justify-center rounded-lg transition-colors",
        quickNotesOpen ? "bg-sidebar-accent text-foreground" : `${TITLE_BAR_ICON_COLOR_CLASS} hover:bg-sidebar-accent`,
      )}
      title={t("titleBar.control.quickNotes")}
      aria-label={t("titleBar.control.quickNotes")}
      aria-haspopup="dialog"
      aria-expanded={quickNotesOpen}
    >
      <StickyNote size={TITLE_BAR_ICON_SIZE} strokeWidth={TITLE_BAR_ICON_STROKE_WIDTH} />
    </button>

    {#if isMainWindow && showMusic}
      <button
        type="button"
        onclick={onToggleMusic}
        onwheel={handleVolumeWheel}
        class={cn(
          "titlebar-icon-button relative flex items-center justify-center rounded-lg transition-colors",
          musicPanelOpen
            ? "bg-background text-foreground dark:bg-accent dark:text-white"
            : `${TITLE_BAR_ICON_COLOR_CLASS} hover:bg-sidebar-accent`,
        )}
        title={musicButtonTooltip}
        aria-label={t("titleBar.control.music")}
        aria-haspopup="dialog"
        aria-expanded={musicPanelOpen}
      >
        <Music size={TITLE_BAR_ICON_SIZE} strokeWidth={TITLE_BAR_ICON_STROKE_WIDTH} />
        {#if musicPlayer.contextPlayback && musicPlayer.contextPlayback.state !== "overridden"}
          <span class="absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full bg-primary ring-2 ring-sidebar" aria-hidden="true"></span>
        {/if}
      </button>
    {/if}

<style>
  .titlebar-icon-button {
    width: 32px;
    height: 32px;
  }
</style>
