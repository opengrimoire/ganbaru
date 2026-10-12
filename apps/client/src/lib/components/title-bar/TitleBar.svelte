<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { clearAssetUrlCache } from "$lib/api/asset-url-cache";
  import { getNavigation } from "$lib/stores/navigation.svelte";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { getZoom } from "$lib/stores/zoom.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import type { TitleBarControlId } from "$lib/stores/preference-options";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { DetachableTabView } from "$lib/navigation";
  import { getDetachedWindows } from "$lib/stores/detached-windows.svelte";
  import { detachableTabViewFromWindowLabel } from "$lib/windows/detached";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getThemeEditor } from "$lib/stores/theme-editor.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { getBenchmarkStatus } from "$lib/stores/benchmark-status.svelte";
  import type { StartupMemorySnapshot } from "$lib/diagnostics/memory-report";
  import type { SectionId } from "$lib/settings/types";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import TitleBarOverlayHost from "./TitleBarOverlayHost.svelte";
  import TitleBarTabs from "./TitleBarTabs.svelte";
  import TitleBarMediaControls from "./TitleBarMediaControls.svelte";
  import TitleBarUtilityControls from "./TitleBarUtilityControls.svelte";
  import { createTitleBarWindowController } from "./window-controller.svelte";
  import { createTitleBarDetachedController } from "./detached-controller.svelte";
  import TitleBarMenus from "./TitleBarMenus.svelte";
  import {
    createTitleBarShortcutController,
    getAppCloseCoordinator,
  } from "./shortcut-controller.svelte";
  import TitleBarWindowControls from "./TitleBarWindowControls.svelte";
  import { flushQuickNoteEditors } from "$lib/quick-notes/persistence";
  import { preloadQuickNotesInitialSnapshot } from "$lib/quick-notes/initial-snapshot";

  let {
    shellStartupMs = null,
    startupMemorySnapshot = { status: "pending" },
    ensureBenchmarkOverlay = async () => {},
  }: {
    shellStartupMs: number | null;
    startupMemorySnapshot: StartupMemorySnapshot;
    ensureBenchmarkOverlay?: () => Promise<void>;
  } = $props();

  const appWindow = getCurrentWindow();
  const isMainWindow = appWindow.label === "main";
  const detachedWindowView = detachableTabViewFromWindowLabel(appWindow.label);
  const nav = getNavigation();
  const pomodoro = getPomodoro();
  const theme = getTheme();
  const zoom = getZoom();
  const preferences = getPreferences();
  const viewport = getViewport();
  const { t } = getLocalization();
  const detachedWindows = getDetachedWindows();
  const benchmarkStatus = getBenchmarkStatus();
  const appClose = getAppCloseCoordinator();

  let showCloseConfirm = $state(false);
  let showPomodoroMenu = $state(false);
  let showQuickNotes = $state(false);
  let showMusicPanel = $state(false);
  let showResetSequenceConfirm = $state(false);
  let showResetConfirm = $state(false);
  let showPerfMenu = $state(false);
  let showTitleBarMenu = $state(false);
  let showTabContextMenu = $state(false);
  let showUtilityOverflowMenu = $state(false);
  let titleBarMenuStyle = $state("");
  let tabContextMenuStyle = $state("");
  let tabContextView = $state<DetachableTabView | null>(null);
  let titleBarTabs: { handleWheel: (event: WheelEvent) => void } | undefined;
  let mediaControls: { handleVolumeWheel: (event: WheelEvent) => void } | undefined;
  const settingsLauncher = getSettingsLauncher();
  const themeEditor = getThemeEditor();
  let perfPinned = $state(false);
  const lockedByBenchmark = $derived(benchmarkStatus.status !== "idle");


  function togglePerfMenu() {
    showPerfMenu = !showPerfMenu;
    showPomodoroMenu = false;
    showUtilityOverflowMenu = false;
    showQuickNotes = false;
    showMusicPanel = false;
  }

  function toggleTheme() {
    if (lockedByThemeEditor) return;
    showPomodoroMenu = false;
    showUtilityOverflowMenu = false;
    showQuickNotes = false;
    showMusicPanel = false;
    theme.toggle();
  }

  function toggleThemeFromShortcut() {
    if (lockedByThemeEditor) return;
    theme.toggle();
  }

  async function toggleQuickNotes(): Promise<void> {
    if (showQuickNotes) {
      showQuickNotes = false;
      return;
    }
    try {
      await preloadQuickNotesInitialSnapshot();
    } catch (error: unknown) {
      console.warn("Quick notes preload failed", error);
    }
    showQuickNotes = true;
    showMusicPanel = false;
    showPomodoroMenu = false;
    showPerfMenu = false;
    showTitleBarMenu = false;
    showUtilityOverflowMenu = false;
    showThemeQuickSwitcher = false;
    settingsLauncher.close();
  }

  function openMusicPanel(): void {
    if (!isMainWindow) return;
    showMusicPanel = true;
    showQuickNotes = false;
    showPomodoroMenu = false;
    showPerfMenu = false;
    showTitleBarMenu = false;
    showUtilityOverflowMenu = false;
    showThemeQuickSwitcher = false;
    settingsLauncher.close();
  }

  function toggleMusicPanel(): void {
    if (showMusicPanel) {
      showMusicPanel = false;
      return;
    }
    openMusicPanel();
  }

  async function openMusicPanelFromTray(): Promise<void> {
    openMusicPanel();
    try {
      await appWindow.show();
      await appWindow.unminimize();
      await appWindow.setFocus();
    } catch (error) {
      console.error("Failed to focus the main window for Music:", error);
    }
  }

  onMount(() => {
    if (!isMainWindow) return;
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    void listen("tray-music-open", () => {
      if (!disposed) void openMusicPanelFromTray();
    })
      .then((nextUnlisten) => {
        if (disposed) {
          nextUnlisten();
          return;
        }
        unlisten = nextUnlisten;
      })
      .catch((error: unknown) => {
        console.error("Failed to listen for Music panel opens:", error);
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  let showThemeQuickSwitcher = $state(false);

  function openThemeQuickSwitcher() {
    if (lockedByThemeEditor) return;
    if (!perfPinned) showPerfMenu = false;
    settingsLauncher.close();
    showPomodoroMenu = false;
    showTitleBarMenu = false;
    showUtilityOverflowMenu = false;
    showQuickNotes = false;
    showMusicPanel = false;
    showThemeQuickSwitcher = true;
  }

  function openSettings(section?: SectionId) {
    showPomodoroMenu = false;
    showUtilityOverflowMenu = false;
    showQuickNotes = false;
    showMusicPanel = false;
    settingsLauncher.open(section);
  }

  async function confirmReset() {
    showResetConfirm = false;
    pomodoro.stopSession();
    clearAssetUrlCache();
    await invoke("reset_database");
  }

  function activateOverflowControl(id: TitleBarControlId) {
    if (overflowControlDisabled(id)) return;
    showPomodoroMenu = false;
    showUtilityOverflowMenu = false;
    if (id === "theme") {
      theme.toggle();
    } else if (id === "performance") {
      togglePerfMenu();
    } else if (id === "settings") {
      openSettings();
    }
  }


  function cancelClose() {
    showCloseConfirm = false;
    appClose.setConfirmationOpen(false);
  }


  // While the floating theme editor is open, the buttons that would navigate
  // away from or disrupt the edit session (theme toggle flips base; settings
  // modal reopens behind the panel) are disabled. Window controls,
  // pomodoro, and the diagnostics monitor stay live because they do not
  // interfere with the edit session.
  const lockedByThemeEditor = $derived(!!themeEditor.editingId);

  const windowController = createTitleBarWindowController({
    window: appWindow,
    detachedWindowView,
    markDetachedViewAttached: (view) => detachedWindows.markAttached(view),
    benchmarkLocked: () => lockedByBenchmark,
    resetConfirmationOpen: () => showResetSequenceConfirm || showResetConfirm,
    requestCloseConfirmation: () => {
      showCloseConfirm = true;
      appClose.setConfirmationOpen(true);
    },
    closeConfirmation: cancelClose,
    ensureBenchmarkOverlay: () => ensureBenchmarkOverlay(),
    beforeClose: flushQuickNoteEditors,
    themeEditOpen: () => !!themeEditor.editingId,
    cancelThemeEdit: () => themeEditor.cancel(),
    reportError: (message, error) => { console.error(message, error); },
  });

  const handleClose = windowController.requestClose;
  const confirmClose = windowController.confirmClose;

  $effect(() => appClose.registerRequestHandler(handleClose));

  createTitleBarShortcutController({
    benchmarkLocked: () => lockedByBenchmark,
    themeEditorLocked: () => lockedByThemeEditor,
    ensureBenchmarkOverlay: () => ensureBenchmarkOverlay(),
    showResetSequenceConfirmation: () => { showResetSequenceConfirm = true; },
    closeOtherConfirmations: () => {
      showCloseConfirm = false;
      appClose.setConfirmationOpen(false);
      showResetConfirm = false;
    },
    requestClose: handleClose,
    toggleTheme: toggleThemeFromShortcut,
    openThemeSwitcher: openThemeQuickSwitcher,
    zoomIn: () => zoom.zoomIn(),
    zoomOut: () => zoom.zoomOut(),
    resetZoom: () => zoom.reset(),
    toggleMusic: () => { if (isMainWindow) toggleMusicPanel(); },
    toggleDiagnostics: togglePerfMenu,
    openShortcutHelp: () => { if (!lockedByThemeEditor) openSettings("shortcuts"); },
  });

  const detachedController = createTitleBarDetachedController({
    window: appWindow,
    isMainWindow,
    detachedWindowView,
    contextView: () => tabContextView,
    closeContextMenu: () => {
      showTabContextMenu = false;
      tabContextView = null;
    },
    currentView: () => nav.current,
    navigate: (view) => nav.navigate(view),
    detachedViews: () => detachedWindows.views,
    markAttached: (view) => detachedWindows.markAttached(view),
    markDetached: (view) => detachedWindows.markDetached(view),
    reportError: (message, error) => { console.error(message, error); },
  });



  const titleBarControls: { id: TitleBarControlId; label: () => string }[] = [
    { id: "pomodoro", label: () => t("titleBar.control.pomodoro") },
    { id: "music", label: () => t("titleBar.control.music") },
    { id: "theme", label: () => t("titleBar.control.theme") },
    { id: "performance", label: () => t("titleBar.control.performance") },
    { id: "settings", label: () => t("titleBar.control.settings") },
  ];

  const TITLE_BAR_MENU_WIDTH = FLOATING_WIDTH.sm;
  const TAB_CONTEXT_MENU_MAX_WIDTH = 260;
  const MENU_EDGE_GAP = 8;
  const themeEditorLockedControlIds = new Set<TitleBarControlId>([
    "theme",
    "settings",
  ]);
  const TITLE_BAR_ICON_COLOR_CLASS = "text-foreground/68 dark:text-white/76";
  const TITLE_BAR_ICON_STROKE_CLASS = "stroke-foreground/68 dark:stroke-white/76";
  const TITLE_BAR_ICON_STROKE_WIDTH = 1.5;
  const TITLE_BAR_ICON_SIZE = 14;

  const compactOverflowIds = $derived.by(() => {
    const ids = new Set<TitleBarControlId>();
    if (viewport.below("regular")) {
      ids.add("theme");
      ids.add("performance");
    }
    return ids;
  });
  const overflowActionControls = $derived(
    titleBarControls.filter((control) =>
      control.id !== "compactTabs"
      && preferences.titleBarVisibility[control.id]
      && compactOverflowIds.has(control.id),
    ),
  );
  function titleBarControlVisible(id: TitleBarControlId): boolean {
    return preferences.titleBarVisibility[id] && !compactOverflowIds.has(id);
  }

  function overflowControlDisabled(id: TitleBarControlId): boolean {
    return lockedByThemeEditor && themeEditorLockedControlIds.has(id);
  }



  function handleModalKeydown(e: KeyboardEvent) {
    if (showPomodoroMenu && e.key === "Escape") {
      showPomodoroMenu = false;
      return;
    }

    if (showUtilityOverflowMenu && e.key === "Escape") {
      showUtilityOverflowMenu = false;
      return;
    }

    if (showTabContextMenu && e.key === "Escape") {
      showTabContextMenu = false;
      tabContextView = null;
      return;
    }

    if (showTitleBarMenu && e.key === "Escape") {
      showTitleBarMenu = false;
      return;
    }

    if (showPerfMenu && e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      showPerfMenu = false;
      return;
    }
  }


  function openTitleBarMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    showPomodoroMenu = false;
    showQuickNotes = false;
    showMusicPanel = false;
    showTabContextMenu = false;
    tabContextView = null;

    const left = Math.min(
      Math.max(MENU_EDGE_GAP, e.clientX),
      Math.max(MENU_EDGE_GAP, window.innerWidth - TITLE_BAR_MENU_WIDTH - MENU_EDGE_GAP),
    );
    const top = Math.max(MENU_EDGE_GAP, e.clientY);
    titleBarMenuStyle = [
      `left: ${left}px`,
      `top: ${top}px`,
      `width: ${TITLE_BAR_MENU_WIDTH}px`,
      `max-height: calc(100vh - ${top + MENU_EDGE_GAP}px)`,
    ].join("; ");
    showTitleBarMenu = true;
  }

  function openTabContextMenu(e: MouseEvent, view: DetachableTabView) {
    e.preventDefault();
    e.stopPropagation();
    showPomodoroMenu = false;
    showQuickNotes = false;
    showMusicPanel = false;
    showTitleBarMenu = false;
    showUtilityOverflowMenu = false;
    showTabContextMenu = true;
    tabContextView = view;

    const left = Math.min(
      Math.max(MENU_EDGE_GAP, e.clientX),
      Math.max(MENU_EDGE_GAP, window.innerWidth - TAB_CONTEXT_MENU_MAX_WIDTH - MENU_EDGE_GAP),
    );
    const top = Math.max(MENU_EDGE_GAP, e.clientY);
    tabContextMenuStyle = [
      `left: ${left}px`,
      `top: ${top}px`,
      "width: max-content",
      "min-width: 196px",
      `max-width: min(${TAB_CONTEXT_MENU_MAX_WIDTH}px, calc(100vw - ${MENU_EDGE_GAP * 2}px))`,
      `max-height: calc(100vh - ${top + MENU_EDGE_GAP}px)`,
    ].join("; ");
  }


  function toggleTitleBarControl(id: TitleBarControlId) {
    preferences.toggleTitleBarControl(id);
    showTitleBarMenu = false;
  }

</script>

<svelte:window onkeydown={handleModalKeydown} />


<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  data-tauri-drag-region
  class="app-title-bar flex w-full shrink-0 select-none items-center bg-sidebar"
  style="height: var(--titlebar-h);"
  onwheel={(event) => titleBarTabs?.handleWheel(event)}
  oncontextmenu={openTitleBarMenu}
  ondragover={detachedController.handleDragOver}
  ondrop={detachedController.handleDrop}
>
  <TitleBarTabs
    bind:this={titleBarTabs}
    {detachedWindowView}
    windowLabel={appWindow.label}
    pomodoroMenuOpen={showPomodoroMenu}
    onVolumeWheel={(event) => mediaControls?.handleVolumeWheel(event)}
    onOpenContextMenu={openTabContextMenu}
  />

  <!-- Draggable spacer -->
  <div class="flex-1"></div>

  <!-- Utility buttons -->
  <div class="flex shrink-0 items-center gap-0.5">
    <TitleBarMediaControls
      bind:this={mediaControls}
      showPomodoro={titleBarControlVisible("pomodoro")}
      showMusic={titleBarControlVisible("music")}
      quickNotesOpen={showQuickNotes}
      musicPanelOpen={showMusicPanel}
      bind:showMenu={showPomodoroMenu}
      {isMainWindow}
      onMenuOpened={() => {
        showPerfMenu = false;
        showTitleBarMenu = false;
        showUtilityOverflowMenu = false;
        showQuickNotes = false;
        showMusicPanel = false;
      }}
      onToggleQuickNotes={() => { void toggleQuickNotes(); }}
      onToggleMusic={toggleMusicPanel}
    />

    <TitleBarUtilityControls
      showTheme={titleBarControlVisible("theme")}
      showPerformance={titleBarControlVisible("performance")}
      showSettings={titleBarControlVisible("settings")}
      themeIsDark={theme.isDark}
      {lockedByThemeEditor}
      bind:showOverflow={showUtilityOverflowMenu}
      overflowControls={overflowActionControls.map((control) => ({
        id: control.id,
        label: control.label(),
        disabled: overflowControlDisabled(control.id),
      }))}
      onToggleTheme={toggleTheme}
      onTogglePerformance={togglePerfMenu}
      onOpenSettings={() => openSettings()}
      onActivateOverflow={activateOverflowControl}
      onOverflowOpened={() => {
        showPomodoroMenu = false;
        showPerfMenu = false;
        showQuickNotes = false;
        showMusicPanel = false;
      }}
    />

  </div>

  <TitleBarWindowControls
    isMaximized={windowController.isMaximized}
    {lockedByBenchmark}
    {isMainWindow}
    onMinimize={() => { void appWindow.minimize(); }}
    onToggleMaximize={() => { void appWindow.toggleMaximize(); }}
    onClose={() => { void handleClose(); }}
  />
</div>

<TitleBarOverlayHost
  bind:showPerformance={showPerfMenu}
  bind:performancePinned={perfPinned}
  bind:showThemeQuickSwitcher
  bind:showQuickNotes
  bind:showMusic={showMusicPanel}
  {shellStartupMs}
  {startupMemorySnapshot}
  {ensureBenchmarkOverlay}
/>

<TitleBarMenus
  bind:showTabContextMenu
  bind:tabContextView
  {tabContextMenuStyle}
  detachedWindow={!!detachedWindowView}
  canDetachTab={detachedWindowView ? true : detachedController.canDetachContextView()}
  bind:showTitleBarMenu
  {titleBarMenuStyle}
  controls={titleBarControls.map((control) => ({
    id: control.id,
    label: control.label(),
  }))}
  onTabContextAction={detachedController.activateContextAction}
  onToggleControl={toggleTitleBarControl}
/>

{#if showResetSequenceConfirm}
  <ConfirmDialog
    title={t("titleBar.resetSequenceTitle")}
    message={t("titleBar.resetSequenceMessage")}
    confirmLabel={t("titleBar.resetSequenceConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => {
      showResetSequenceConfirm = false;
      showResetConfirm = true;
    }}
    onCancel={() => { showResetSequenceConfirm = false; }}
  />
{/if}

{#if showResetConfirm}
  <ConfirmDialog
    title={t("titleBar.resetDatabaseTitle")}
    message={t("titleBar.resetDatabaseMessage")}
    confirmLabel={t("titleBar.resetDatabaseConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirmReset}
    onCancel={() => { showResetConfirm = false; }}
  />
{/if}

{#if showCloseConfirm}
  <ConfirmDialog
    title={t("titleBar.closeAppTitle")}
    message={t("titleBar.closeAppMessage")}
    confirmLabel={t("titleBar.closeAnyway")}
    cancelLabel={t("titleBar.stay")}
    onConfirm={confirmClose}
    onCancel={cancelClose}
  />
{/if}
