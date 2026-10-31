<script lang="ts">
  import { onBackButtonPress } from "@tauri-apps/api/app";
  import { onMount, untrack } from "svelte";
  import { notesHeldEditNotice, reportNotesHeldEditFailure } from "$lib/stores/notes/held-edit-notice.svelte";
  import { COMPACT_IDENTITY_ICON_SIZE, COMPACT_IDENTITY_ICON_STROKE_WIDTH } from "$lib/icon-sizing";
  import MobileNavigation from "$lib/components/mobile/MobileNavigation.svelte";
  import MobileTopBar from "$lib/components/mobile/MobileTopBar.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { ensureDbUrl } from "$lib/api/db";
  import { setSyncForeground } from "$lib/api/sync";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    MobileBackListenerController,
    resolveMobileBackAction,
  } from "$lib/mobile/back";
  import {
    mobileNavigationPresentation,
    type MobileNavigationPresentation,
    mobileTopBarPanelGeometry,
  } from "$lib/mobile/layout";
  import {
    classifyLoadFailure,
    recoverLoadFailure,
    type LoadFailure,
  } from "$lib/module-load-recovery";
  import { revealFocusedTextEntry } from "$lib/mobile/keyboard";
  import { workspaceTabKindIcon } from "$lib/workspace-tabs/presentation";
  import { createDeferredWorkspaceLocationAdapter } from "$lib/mobile/workspace-location.svelte";
  import { provideMobileIdentitySlot } from "$lib/mobile/identity-slot";
  import { MobilePersistenceLifecycleController } from "$lib/mobile/persistence-lifecycle";
  import { mobileWorkspaceSurface, type MobileProjectHostingState } from "$lib/mobile/project-hosting";
  import { availableAppViews, type View } from "$lib/navigation";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { flushQuickNoteEditors } from "$lib/quick-notes/persistence";
  import { getCalendar } from "$lib/stores/calendar.svelte";
  import { getCalendars } from "$lib/stores/calendars.svelte";
  import { getNavigation } from "$lib/stores/navigation.svelte";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { mark as perfMark } from "$lib/stores/perf-log.svelte";
  import { getZoom } from "$lib/stores/zoom.svelte";
  import { flushConfig } from "$lib/vault/config";

  type CalendarComponent = typeof import("$lib/components/calendar/CalendarView.svelte").default;
  type ProjectsComponent = typeof import("$lib/components/projects/ProjectsView.svelte").default;
  type NotesComponent = typeof import("$lib/components/notes/NotesView.svelte").default;
  type ChatComponent = typeof import("$lib/components/chat/ChatWorkspace.svelte").default;
  type QuickNotesComponent = typeof import("$lib/components/quick-notes/QuickNotesPanel.svelte").default;
  type SettingsComponent = typeof import("$lib/components/settings/SettingsModal.svelte").default;
  type MusicComponent = typeof import("$lib/components/music/MusicPanel.svelte").default;
  type MusicPlaybackHostComponent = typeof import("$lib/components/music/MusicPlaybackHost.svelte").default;
  type PomodoroMenuComponent = typeof import("$lib/components/pomodoro/PomodoroMenuContent.svelte").default;
  type LinkedDeviceControlComponent = typeof import("$lib/components/vault/handoff/LinkedDeviceControl.svelte").default;
  type TabsPanelComponent = typeof import("$lib/components/mobile/MobileTabsPanel.svelte").default;
  type ProjectViewComponents = import("$lib/components/projects/view-components").ProjectViewComponents;
  type ProjectViewSwitcherComponent = typeof import("$lib/components/projects/ProjectMobileViewSwitcher.svelte").default;
  type WorkspaceLocationAdapter = import("$lib/stores/workspace-tabs.svelte").WorkspaceLocationAdapter;
  type ProjectsStore = ReturnType<typeof import("$lib/stores/projects.svelte").getProjects>;
  type NotesStore = ReturnType<typeof import("$lib/stores/notes.svelte").getNotes>;
  type QuickNotesTrashPurgeRuntime = import("$lib/quick-notes/trash-purge-runtime").QuickNotesTrashPurgeRuntime;
  type DeferredSurface = Exclude<View, "calendar"> | "settings" | "quickNotes" | "music";
  interface CalendarNotificationScheduler {
    reconcile(): Promise<void>;
    takeAction(): Promise<string | null>;
  }
  interface PomodoroScheduleScheduler {
    reconcile(): Promise<void>;
  }

  perfMark("boot.mobile-shell-script");

  const nav = getNavigation();
  const viewport = getViewport();
  const settingsLauncher = getSettingsLauncher();
  const calendar = getCalendar();
  const calendars = getCalendars();
  const pomodoro = getPomodoro();
  const mobileBackStack = getMobileBackStack();
  getZoom().reapply();
  const localization = getLocalization();
  const { t } = localization;
  const androidSystemBackAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "system.android-back",
  );
  const musicAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "view.music");
  const mobileMusicPlayerChromeHeight = 132;
  const mobileMusicMediaHeightRatio = 9 / 16;
  const mobileMusicStackedMediaShare = 0.65;

  let showPomodoro = $state(false);
  let identitySlotElement = $state<HTMLElement | null>(null);
  provideMobileIdentitySlot({ get target() { return identitySlotElement; } });
  let showSettings = $state(false);
  let showQuickNotes = $state(false);
  let showMusic = $state(false);
  let musicPanelStyle = $state("");
  let musicPlaylistPanelStyle = $state("");
  let quickNotesPanelStyle = $state("");
  let nestedRouteOpen = $state(false);
  let loadError = $state<LoadFailure | null>(null);
  let backendReady = $state(false);
  let initializingWorkspace = $state(false);
  let CalendarSurface = $state<CalendarComponent | null>(null);
  let ProjectsSurface = $state<ProjectsComponent | null>(null);
  let NotesSurface = $state<NotesComponent | null>(null);
  let ChatSurface = $state<ChatComponent | null>(null);
  let ProjectViewSwitcherSurface = $state<ProjectViewSwitcherComponent | null>(null);
  let projectsStore = $state.raw<ProjectsStore | null>(null);
  let QuickNotesSurface = $state<QuickNotesComponent | null>(null);
  let SettingsSurface = $state<SettingsComponent | null>(null);
  let MusicSurface = $state<MusicComponent | null>(null);
  let MusicPlaybackHostSurface = $state<MusicPlaybackHostComponent | null>(null);
  let musicPanelMounted = $state(false);
  let PomodoroMenuSurface = $state<PomodoroMenuComponent | null>(null);
  let LinkedDeviceControlSurface = $state<LinkedDeviceControlComponent | null>(null);
  let notesStore = $state.raw<NotesStore | null>(null);
  let notesSurfaceMounted = $state(false);
  let projectViewComponents = $state.raw<ProjectViewComponents | null>(null);
  let deferredLoadErrors = $state<Partial<Record<DeferredSurface, LoadFailure>>>({});
  const deferredSurfaceLoads = new Map<DeferredSurface, Promise<void>>();
  let deferredPreparationStarted = false;
  let mobileAppDisposed = false;
  let stopMusicPreload = (): void => undefined;
  let removePomodoroBackLayer = (): void => undefined;
  let removeSettingsBackLayer = (): void => undefined;
  let removeQuickNotesBackLayer = (): void => undefined;
  let removeMusicBackLayer = (): void => undefined;
  let calendarNotificationScheduler = $state.raw<CalendarNotificationScheduler | null>(null);
  let calendarNotificationSchedulerLoad: Promise<void> | null = null;
  let calendarNotificationSchedulerDisposed = false;
  let pomodoroScheduleScheduler = $state.raw<PomodoroScheduleScheduler | null>(null);
  let quickNotesTrashPurge = $state.raw<QuickNotesTrashPurgeRuntime | null>(null);
  let quickNotesTrashPurgeLoad: Promise<void> | null = null;
  let showTabs = $state(false);
  let TabsPanelSurface = $state<TabsPanelComponent | null>(null);
  let markBackendReady = (): void => undefined;
  /** Settles once the database is ready, so restoring a tab never loads a store before it. */
  const backendReadyOnce = new Promise<void>((resolve) => {
    markBackendReady = resolve;
  });

  const workspaceTabs = getWorkspaceTabs();
  workspaceTabs.connect({
    adapter: createDeferredWorkspaceLocationAdapter(loadWorkspaceLocationAdapter),
    // The tabs panel offers no close on the only tab, and Back from the first tab leaves the app.
    requestWindowClose: () => undefined,
  });

  const navigationPresentation = $derived(
    mobileNavigationPresentation(viewport.layoutWidth),
  );
  const useNavigationRail = $derived(navigationPresentation === "rail");
  const bottomNavigationVisible = $derived(!useNavigationRail && !viewport.keyboardOpen);
  const suspendInfo = $derived(pomodoro.suspendedAway);
  const suspendDecisionOpen = $derived(suspendInfo !== null);
  const modalOpen = $derived(
    showSettings || showQuickNotes || showMusic || suspendDecisionOpen,
  );
  const CalendarIcon = workspaceTabKindIcon("calendar");
  const currentTitle = $derived(t(`titleBar.tab.${nav.current}`));
  /** Workspace shown for the current tab; the Projects tab can host the selected project's Notes or Chat. */
  const workspaceSurface = $derived(mobileWorkspaceSurface(nav.current, projectHostingState(nav.current)));
  const projectHostingActive = $derived(workspaceSurface !== nav.current);
  const currentDeferredSurface = $derived(
    workspaceSurface === "calendar" ? null : workspaceSurface,
  );
  const firstTabActive = $derived(workspaceTabs.activeTabId === workspaceTabs.tabs[0]?.id);
  const shouldInterceptSystemBack = $derived(
    androidSystemBackAvailable && (nestedRouteOpen
      || mobileBackStack.hasActiveLayer
      || !firstTabActive),
  );
  const backListenerController = new MobileBackListenerController(
    (handler) => onBackButtonPress(handler),
    handleSystemBack,
    (error) => {
      console.error("Failed to update Android back handling", error);
    },
  );
  const persistenceLifecycle = new MobilePersistenceLifecycleController({
    documentTarget: document,
    windowTarget: window,
    flushers: {
      config: flushConfig,
      notes: flushMountedNotes,
      quickNotes: flushQuickNoteEditors,
    },
    onError: (label, error) => {
      if (!reportNotesHeldEditFailure(error)) console.error(`Failed to flush mobile ${label} persistence`, error);
    },
  });

  /** Reads the Projects state that decides hosting, only for the Projects tab and once the store has loaded. */
  function projectHostingState(view: View): MobileProjectHostingState | null {
    if (view !== "projects" || !projectsStore) return null;
    return {
      hostedPlace: projectsStore.hostedPlace,
      projectSelected: Boolean(projectsStore.selectedProject && projectsStore.selectedGroup),
    };
  }

  /** Loads the Projects, Notes, and Chat stores that tab locations describe, once the database is ready. */
  async function loadWorkspaceLocationAdapter(): Promise<WorkspaceLocationAdapter> {
    await backendReadyOnce;
    await prepareDeferredSurface("projects");
    const projects = projectsStore;
    if (!projects) throw new Error("The Projects store did not load, so tab locations cannot be restored");
    const [adapterModule, notesModule, chatModule] = await Promise.all([
      import("$lib/workspace-tabs/app-location-adapter"),
      import("$lib/stores/notes.svelte"),
      import("$lib/stores/chat.svelte"),
    ]);
    return adapterModule.createAppWorkspaceLocationAdapter(
      { projects, notes: notesModule.getNotes(), chat: chatModule.getChat() },
      t,
    );
  }

  /**
   * Opens the tabs panel once its chunk loads, or closes it when it is open; a failed load leaves the bar as it was,
   * so a tap retries.
   */
  function toggleTabs(): void {
    if (showTabs) {
      showTabs = false;
      return;
    }
    if (TabsPanelSurface) {
      showTabs = true;
      return;
    }
    import("$lib/components/mobile/MobileTabsPanel.svelte")
      .then((module) => {
        if (mobileAppDisposed) return;
        TabsPanelSurface = module.default;
        showTabs = true;
      })
      .catch((error: unknown) => {
        console.error("Failed to load the tabs panel", error);
      });
  }

  /**
   * Saves Notes edits when the app hides, once Notes has ever mounted, even while another tab is on screen. A held edit
   * refuses the flush, and the lifecycle error handler shows the held notice.
   */
  function flushMountedNotes(): Promise<void> {
    if (!notesSurfaceMounted || !notesStore) return Promise.resolve();
    return notesStore.flushPendingWrites();
  }

  function rootPixelValue(property: string): number {
    const value = Number.parseFloat(
      getComputedStyle(document.documentElement).getPropertyValue(property),
    );
    return Number.isFinite(value) ? Math.max(0, value) : 0;
  }

  function utilityPanelStyle(
    triggerSelector: string,
    desiredWidth: number,
    desiredHeight: number | ((panelWidth: number) => number),
  ): string {
    const trigger = document.querySelector<HTMLElement>(triggerSelector);
    if (!trigger) return "";
    const triggerRect = trigger.getBoundingClientRect();
    const visualViewport = window.visualViewport;
    const viewportOffsetLeft = visualViewport?.offsetLeft ?? 0;
    const viewportOffsetTop = visualViewport?.offsetTop ?? 0;
    const viewportWidth = visualViewport?.width ?? window.innerWidth;
    const viewportHeight = visualViewport?.height ?? window.innerHeight;
    const safeAreaLeft = rootPixelValue("--safe-area-left");
    const safeAreaRight = rootPixelValue("--safe-area-right");
    const safeAreaTop = rootPixelValue("--safe-area-top");
    const safeAreaBottom = rootPixelValue("--safe-area-bottom");
    const geometry = mobileTopBarPanelGeometry({
      anchorLeft: triggerRect.left,
      anchorWidth: triggerRect.width,
      anchorBottom: triggerRect.bottom,
      desiredWidth,
      desiredHeight: viewportHeight,
      viewportLeft: viewportOffsetLeft + safeAreaLeft,
      viewportWidth: Math.max(0, viewportWidth - safeAreaLeft - safeAreaRight),
      viewportTop: viewportOffsetTop + safeAreaTop,
      viewportHeight: Math.max(0, viewportHeight - safeAreaTop - safeAreaBottom),
    });
    const fittedHeight = typeof desiredHeight === "function"
      ? desiredHeight(geometry.width)
      : desiredHeight;
    return [
      `left:${Math.round(geometry.left)}px`,
      `top:${Math.round(geometry.top)}px`,
      `width:${Math.round(geometry.width)}px`,
      `height:${Math.round(Math.min(geometry.height, Math.max(0, fittedHeight)))}px`,
    ].join(";");
  }

  function updateUtilityPanelPositions(): void {
    quickNotesPanelStyle = utilityPanelStyle(
      "[data-mobile-quick-notes-trigger]",
      760,
      680,
    );
    musicPanelStyle = utilityPanelStyle(
      "[data-mobile-music-trigger]",
      1000,
      (panelWidth) => mobileMusicPlayerChromeHeight
        + panelWidth * mobileMusicMediaHeightRatio,
    );
    musicPlaylistPanelStyle = utilityPanelStyle(
      "[data-mobile-music-trigger]",
      1000,
      (panelWidth) => mobileMusicPlayerChromeHeight
        + (panelWidth * mobileMusicMediaHeightRatio) / mobileMusicStackedMediaShare,
    );
  }

  async function ensureCalendarNotificationScheduler(): Promise<void> {
    if (calendarNotificationScheduler || calendarNotificationSchedulerDisposed) return;
    if (calendarNotificationSchedulerLoad) return calendarNotificationSchedulerLoad;
    calendarNotificationSchedulerLoad = (async () => {
      const [module, pomodoroScheduleModule] = await Promise.all([
        import("$lib/scheduling/mobile-calendar-notifications"),
        import("$lib/scheduling/mobile-pomodoro-schedule"),
      ]);
      if (calendarNotificationSchedulerDisposed) return;
      calendarNotificationScheduler = new module.MobileCalendarNotificationScheduler(
        t,
        () => localization.locale,
      );
      pomodoroScheduleScheduler = new pomodoroScheduleModule.MobilePomodoroScheduleScheduler(t);
      await Promise.all([
        calendarNotificationScheduler.reconcile(),
        pomodoroScheduleScheduler.reconcile(),
      ]);
      const eventId = await calendarNotificationScheduler.takeAction();
      if (eventId) nav.navigate("calendar");
    })().catch((error: unknown) => {
      console.error("Failed to initialize Android Calendar notifications", error);
    }).finally(() => {
      calendarNotificationSchedulerLoad = null;
    });
    return calendarNotificationSchedulerLoad;
  }

  /** Loads the Quick notes trash purge job outside the shell bundle. */
  function ensureQuickNotesTrashPurge(): void {
    if (quickNotesTrashPurge || quickNotesTrashPurgeLoad || mobileAppDisposed) return;
    quickNotesTrashPurgeLoad = import("$lib/quick-notes/trash-purge-runtime")
      .then((module) => {
        if (!mobileAppDisposed) quickNotesTrashPurge = module.startQuickNotesTrashPurge();
      })
      .catch((error: unknown) => {
        console.error("Failed to start the Quick notes trash purge", error);
      })
      .finally(() => {
        quickNotesTrashPurgeLoad = null;
      });
  }

  function afterAnimationFrames(count: number): Promise<void> {
    return new Promise((resolve) => {
      const advance = (remaining: number): void => {
        if (remaining === 0) {
          resolve();
          return;
        }
        requestAnimationFrame(() => advance(remaining - 1));
      };
      advance(count);
    });
  }

  /**
   * Recovers the Focus run without blocking startup or resume; the native projection reports
   * a failed recovery to the Focus surface.
   */
  async function recoverFocusRun(): Promise<void> {
    try {
      await pomodoro.recoverMobileRun();
    } catch (error) {
      console.warn("Failed to recover the mobile Focus run", error);
    }
  }

  async function prepareCriticalSurfaces(): Promise<void> {
    const [calendarModule, pomodoroMenuModule, linkedDeviceControlModule] = await Promise.all([
      import("$lib/components/calendar/CalendarView.svelte"),
      import("$lib/components/pomodoro/PomodoroMenuContent.svelte"),
      import("$lib/components/vault/handoff/LinkedDeviceControl.svelte"),
    ]);
    if (mobileAppDisposed) return;
    CalendarSurface = calendarModule.default;
    PomodoroMenuSurface = pomodoroMenuModule.default;
    LinkedDeviceControlSurface = linkedDeviceControlModule.default;
    perfMark("boot.mobile-critical-surfaces-ready");
  }

  function deferredSurfaceReady(surface: DeferredSurface): boolean {
    if (surface === "projects") {
      return ProjectsSurface !== null && projectViewComponents !== null && ProjectViewSwitcherSurface !== null;
    }
    if (surface === "notes") return NotesSurface !== null;
    if (surface === "chat") return ChatSurface !== null;
    if (surface === "settings") return SettingsSurface !== null;
    if (surface === "quickNotes") return QuickNotesSurface !== null;
    return MusicSurface !== null && MusicPlaybackHostSurface !== null;
  }

  $effect(() => {
    if (showMusic && MusicSurface) musicPanelMounted = true;
  });

  async function prepareDeferredSurfaceNow(surface: DeferredSurface): Promise<void> {
    if (surface === "projects") {
      const [
        storeModule,
        projectsModule,
        listModule,
        dashboardModule,
        kanbanModule,
        ganttModule,
        calendarModule,
        switcherModule,
        placesModule,
      ] = await Promise.all([
        import("$lib/stores/projects.svelte"),
        import("$lib/components/projects/ProjectsView.svelte"),
        import("$lib/components/projects/list/ProjectListView.svelte"),
        import("$lib/components/projects/views/ProjectDashboardView.svelte"),
        import("$lib/components/projects/views/ProjectKanbanView.svelte"),
        import("$lib/components/projects/views/ProjectGanttView.svelte"),
        import("$lib/components/calendar/CalendarView.svelte"),
        import("$lib/components/projects/ProjectMobileViewSwitcher.svelte"),
        import("$lib/projects/places"),
      ]);
      if (mobileAppDisposed) return;
      ProjectsSurface = projectsModule.default;
      ProjectViewSwitcherSurface = switcherModule.default;
      projectViewComponents = {
        list: listModule.default,
        dashboard: dashboardModule.default,
        kanban: kanbanModule.default,
        calendar: calendarModule.default,
        gantt: ganttModule.default,
      };
      const store = storeModule.getProjects();
      store.hostableViewTypes = placesModule.projectHostablePlaces(availableAppViews());
      projectsStore = store;
      void store.ensureLoaded().catch((error) => {
        console.warn("Mobile Projects preload failed", error);
      });
      return;
    }
    if (surface === "notes") {
      const [storeModule, module] = await Promise.all([
        import("$lib/stores/notes.svelte"),
        import("$lib/components/notes/NotesView.svelte"),
      ]);
      if (mobileAppDisposed) return;
      notesStore = storeModule.getNotes();
      NotesSurface = module.default;
      void notesStore.ensureLoaded().catch((error) => {
        console.warn("Mobile Notes preload failed", error);
      });
      return;
    }
    if (surface === "chat") {
      const [chatStoreModule, projectsStoreModule, module] = await Promise.all([
        import("$lib/stores/chat.svelte"),
        import("$lib/stores/projects.svelte"),
        import("$lib/components/chat/ChatWorkspace.svelte"),
      ]);
      if (mobileAppDisposed) return;
      ChatSurface = module.default;
      void chatStoreModule.getChat()
        .prewarmForProject(projectsStoreModule.getProjects().selectedProjectId)
        .catch((error) => {
          console.warn("Mobile Chat preload failed", error);
        });
      return;
    }
    if (surface === "settings") {
      const module = await import("$lib/components/settings/SettingsModal.svelte");
      if (!mobileAppDisposed) SettingsSurface = module.default;
      return;
    }
    if (surface === "quickNotes") {
      const [preloadModule, module] = await Promise.all([
        import("$lib/quick-notes/initial-snapshot"),
        import("$lib/components/quick-notes/QuickNotesPanel.svelte"),
      ]);
      if (mobileAppDisposed) return;
      QuickNotesSurface = module.default;
      void preloadModule.preloadQuickNotesInitialSnapshot().catch((error) => {
        console.warn("Mobile Quick notes preload failed", error);
      });
      return;
    }
    const [panelModule, playbackHostModule, preloadModule] = await Promise.all([
      import("$lib/components/music/MusicPanel.svelte"),
      import("$lib/components/music/MusicPlaybackHost.svelte"),
      import("$lib/music/first-use-preload"),
    ]);
    if (mobileAppDisposed) return;
    MusicSurface = panelModule.default;
    MusicPlaybackHostSurface = playbackHostModule.default;
    stopMusicPreload();
    stopMusicPreload = preloadModule.startMusicFirstUsePreload();
  }

  function prepareDeferredSurface(surface: DeferredSurface): Promise<void> {
    if (deferredSurfaceReady(surface)) return Promise.resolve();
    const existing = deferredSurfaceLoads.get(surface);
    if (existing) return existing;
    delete deferredLoadErrors[surface];
    const request = prepareDeferredSurfaceNow(surface)
      .then(() => {
        if (!mobileAppDisposed) perfMark(`boot.mobile-${surface}-ready`);
      })
      .catch((error: unknown) => {
        if (mobileAppDisposed) return;
        deferredLoadErrors[surface] = classifyLoadFailure(error);
        console.error(`Failed to prepare the mobile ${surface} surface`, error);
      })
      .finally(() => {
        deferredSurfaceLoads.delete(surface);
      });
    deferredSurfaceLoads.set(surface, request);
    return request;
  }

  async function prepareDeferredSurfacesAfterPaint(): Promise<void> {
    if (deferredPreparationStarted) return;
    deferredPreparationStarted = true;
    await afterAnimationFrames(2);
    if (mobileAppDisposed) return;
    perfMark("boot.mobile-calendar-presented");
    void synchronizeMobileDistractions();
    void ensureCalendarNotificationScheduler();
    const batches: readonly (readonly DeferredSurface[])[] = [
      ["projects"],
      ["settings"],
      ["quickNotes"],
      ["notes"],
      ["chat"],
      ["music"],
    ];
    for (const batch of batches) {
      await Promise.all(batch.map((surface) => prepareDeferredSurface(surface)));
      await afterAnimationFrames(1);
      if (mobileAppDisposed) return;
    }
    perfMark("boot.mobile-standard-surfaces-ready");
  }

  async function initializeWorkspace(): Promise<void> {
    if (initializingWorkspace) return;
    initializingWorkspace = true;
    backendReady = false;
    loadError = null;
    try {
      const databaseReady = ensureDbUrl();
      const criticalSurfaces = prepareCriticalSurfaces();
      await databaseReady;
      perfMark("boot.mobile-database-ready");
      await Promise.all([
        criticalSurfaces,
        recoverFocusRun(),
        calendars.load(),
        calendar.load(),
      ]);
      perfMark("boot.mobile-calendar-data-ready");
      backendReady = true;
      markBackendReady();
      perfMark("boot.mobile-shell-ready");
      void prepareDeferredSurfacesAfterPaint();
    } catch (error) {
      loadError = classifyLoadFailure(error);
      console.error("Failed to initialize the mobile workspace", error);
    } finally {
      initializingWorkspace = false;
    }
  }

  async function synchronizeMobileDistractions(): Promise<void> {
    try {
      const [storeModule, usageModule, mobileModule] = await Promise.all([
        import("$lib/stores/distractions.svelte"),
        import("$lib/stores/distractions-usage.svelte"),
        import("$lib/scheduling/mobile-distractions"),
      ]);
      await storeModule.getDistractions().publishMobileRules();
      await usageModule.getDistractionsUsage().refresh();
      const target = await mobileModule.takeMobileDistractionsNotificationAction();
      if (target) settingsLauncher.open("distractions", { distractionsTab: target });
    } catch (error) {
      console.warn("Failed to synchronize Android Distractions", error);
    }
  }

  /** Retries startup and returns the attempt so the notice can follow it; a document reload returns nothing. */
  function retryWorkspaceLoad(): Promise<void> | undefined {
    const failure = loadError;
    if (!failure || initializingWorkspace) return undefined;
    let attempt: Promise<void> | undefined;
    recoverLoadFailure(failure, () => {
      attempt = initializeWorkspace();
    });
    return attempt;
  }

  /** Retries a deferred surface and returns the attempt so the notice can follow it. */
  function retryDeferredSurface(surface: DeferredSurface): Promise<void> | undefined {
    const failure = deferredLoadErrors[surface];
    if (!failure) return undefined;
    let attempt: Promise<void> | undefined;
    recoverLoadFailure(failure, () => {
      attempt = prepareDeferredSurface(surface);
    });
    return attempt;
  }

  function clearNestedRoute(): void {
    if (window.location.hash.length === 0) {
      nestedRouteOpen = false;
      return;
    }
    const previousUrl = window.location.href;
    const nextUrl = new URL(previousUrl);
    nextUrl.hash = "";
    window.history.replaceState(window.history.state, "", nextUrl);
    nestedRouteOpen = false;
    window.dispatchEvent(new HashChangeEvent("hashchange", {
      oldURL: previousUrl,
      newURL: nextUrl.href,
    }));
  }

  function formatAwayDuration(totalSeconds: number): string {
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    if (hours > 0 && minutes > 0) return t("focusDialog.awayHoursMinutes", hours, minutes);
    if (hours > 0) return t("focusDialog.awayHours", hours);
    if (minutes > 0) return t("focusDialog.awayMinutes", minutes);
    return t("focusDialog.awaySeconds", totalSeconds);
  }

  function stopSuspendedSession(): void {
    void pomodoro.dismissSuspend(false);
  }

  function closePomodoro(): void {
    removePomodoroBackLayer();
    removePomodoroBackLayer = () => undefined;
    showPomodoro = false;
  }

  function openPomodoro(): void {
    closeSettings();
    closeQuickNotes();
    closeMusic();
    showPomodoro = true;
    removePomodoroBackLayer();
    removePomodoroBackLayer = mobileBackStack.activate({ handle: closePomodoro });
  }

  function closeSettings(): void {
    removeSettingsBackLayer();
    removeSettingsBackLayer = () => undefined;
    showSettings = false;
    if (settingsLauncher.isOpen) settingsLauncher.close();
  }

  function openSettings(): void {
    closePomodoro();
    closeQuickNotes();
    closeMusic();
    showSettings = true;
    void prepareDeferredSurface("settings");
    removeSettingsBackLayer();
    removeSettingsBackLayer = mobileBackStack.activate({ handle: closeSettings });
  }

  $effect(() => {
    if (!settingsLauncher.isOpen || showSettings) return;
    openSettings();
  });

  function closeQuickNotes(): void {
    removeQuickNotesBackLayer();
    removeQuickNotesBackLayer = () => undefined;
    showQuickNotes = false;
  }

  function openQuickNotes(): void {
    if (showQuickNotes || !backendReady) return;
    closePomodoro();
    closeSettings();
    closeMusic();
    updateUtilityPanelPositions();
    showQuickNotes = true;
    void prepareDeferredSurface("quickNotes");
    removeQuickNotesBackLayer();
    removeQuickNotesBackLayer = mobileBackStack.activate({ handle: closeQuickNotes });
  }

  function closeMusic(): void {
    removeMusicBackLayer();
    removeMusicBackLayer = () => undefined;
    showMusic = false;
  }

  function openMusic(): void {
    if (!musicAvailable || showMusic || !backendReady) return;
    closePomodoro();
    closeSettings();
    closeQuickNotes();
    updateUtilityPanelPositions();
    showMusic = true;
    void prepareDeferredSurface("music");
    removeMusicBackLayer();
    removeMusicBackLayer = mobileBackStack.activate({ handle: closeMusic });
  }

  function handleSystemBack(): void {
    const action = resolveMobileBackAction({
      featureLayerOpen: mobileBackStack.hasActiveLayer,
      nestedRouteOpen,
      firstTabActive,
    });
    if (action === "consume-feature-layer") {
      mobileBackStack.consume();
    } else if (action === "close-nested-route") {
      clearNestedRoute();
    } else if (action === "activate-first-tab") {
      const firstTab = workspaceTabs.tabs[0];
      if (firstTab) workspaceTabs.activate(firstTab.id);
    } else {
      void backListenerController.setEnabled(false);
    }
  }

  // Once the keyboard opens or grows, keep the caret above it after the WebView finishes its own focus scrolling.
  $effect(() => {
    if (!viewport.keyboardOpen) return;
    void viewport.height;
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => revealFocusedTextEntry());
    });
    return () => cancelAnimationFrame(frame);
  });

  $effect(() => {
    const root = document.documentElement;
    root.style.setProperty("--mobile-bottom-chrome-h", bottomNavigationVisible ? "var(--mobile-bottom-nav-h)" : "0px");
    return () => root.style.removeProperty("--mobile-bottom-chrome-h");
  });

  onMount(() => {
    mobileAppDisposed = false;
    const detachPersistenceLifecycle = persistenceLifecycle.attach();
    let schedulerResume: Promise<void> | null = null;
    const syncNestedRoute = (): void => {
      nestedRouteOpen = window.location.hash.length > 0;
    };
    const handleMusicAssignmentInspection = (event: Event): void => {
      if (!(event instanceof CustomEvent) || event.detail?.ready === true || nav.current === "calendar") return;
      const eventId = typeof event.detail?.eventId === "string" ? event.detail.eventId : null;
      if (!eventId) return;
      nav.navigate("calendar");
      window.setTimeout(() => {
        window.dispatchEvent(new CustomEvent("ganbaru:inspect-music-assignment", {
          detail: { eventId, ready: true },
        }));
      }, 0);
    };
    const resumeLifecycleSchedulers = (): void => {
      if (document.visibilityState !== "visible" || !backendReady) return;
      quickNotesTrashPurge?.scheduler.resume();
      if (schedulerResume) return;
      schedulerResume = (async () => {
        await recoverFocusRun();
        await Promise.all([
          calendarNotificationScheduler?.reconcile(),
          pomodoroScheduleScheduler?.reconcile(),
          synchronizeMobileDistractions(),
        ]);
        const eventId = await calendarNotificationScheduler?.takeAction();
        if (eventId) nav.navigate("calendar");
      })().catch((error) => {
        console.error("Failed to resume mobile focus schedulers", error);
      }).finally(() => {
        schedulerResume = null;
      });
    };
    const reportSyncForeground = (): void => {
      setSyncForeground(document.visibilityState === "visible").catch((error) => {
        console.error("Failed to report the app visibility to sync", error);
      });
    };
    const handleVisibilityChange = (): void => {
      reportSyncForeground();
      if (document.visibilityState === "hidden") {
        pomodoro.prepareForMobileBackground();
        return;
      }
      resumeLifecycleSchedulers();
    };
    syncNestedRoute();
    window.addEventListener("hashchange", syncNestedRoute);
    window.addEventListener("popstate", syncNestedRoute);
    window.addEventListener("ganbaru:inspect-music-assignment", handleMusicAssignmentInspection);
    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("focus", resumeLifecycleSchedulers);
    reportSyncForeground();

    void initializeWorkspace();

    return () => {
      mobileAppDisposed = true;
      window.removeEventListener("hashchange", syncNestedRoute);
      window.removeEventListener("popstate", syncNestedRoute);
      window.removeEventListener("ganbaru:inspect-music-assignment", handleMusicAssignmentInspection);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("focus", resumeLifecycleSchedulers);
      removePomodoroBackLayer();
      removeSettingsBackLayer();
      removeQuickNotesBackLayer();
      removeMusicBackLayer();
      detachPersistenceLifecycle();
      stopMusicPreload();
      void persistenceLifecycle.flush();
      void backListenerController.dispose();
      calendarNotificationSchedulerDisposed = true;
      calendarNotificationScheduler = null;
      pomodoroScheduleScheduler = null;
      quickNotesTrashPurge?.dispose();
      quickNotesTrashPurge = null;
    };
  });

  $effect(() => {
    const ready = backendReady;
    const purge = quickNotesTrashPurge;
    if (!purge) {
      if (ready) ensureQuickNotesTrashPurge();
      return;
    }
    purge.scheduler.setEnabled(ready);
  });

  $effect(() => {
    void backListenerController.setEnabled(shouldInterceptSystemBack);
  });

  $effect(() => {
    const _calendarVersion = calendar.indexVersion;
    const _notificationLocale = localization.locale;
    if (!backendReady || !calendar.loaded) return;
    void calendarNotificationScheduler?.reconcile();
    void pomodoroScheduleScheduler?.reconcile();
  });

  // Prepare whatever the active tab shows, including a Notes or Chat view hosted in a project tab.
  $effect(() => {
    if (!backendReady || !currentDeferredSurface) return;
    void prepareDeferredSurface(currentDeferredSurface);
  });

  // A Notes link route belongs to the Notes surface, so leaving it for another tab or view drops the route.
  $effect(() => {
    if (workspaceSurface !== "notes") untrack(clearNestedRoute);
  });

  $effect(() => {
    if (workspaceSurface !== "notes" || !NotesSurface || !notesStore) return;
    notesSurfaceMounted = true;
  });

  $effect(() => {
    if (!showQuickNotes && !showMusic) return;
    updateUtilityPanelPositions();
    const visualViewport = window.visualViewport;
    window.addEventListener("resize", updateUtilityPanelPositions);
    visualViewport?.addEventListener("resize", updateUtilityPanelPositions);
    visualViewport?.addEventListener("scroll", updateUtilityPanelPositions);
    return () => {
      window.removeEventListener("resize", updateUtilityPanelPositions);
      visualViewport?.removeEventListener("resize", updateUtilityPanelPositions);
      visualViewport?.removeEventListener("scroll", updateUtilityPanelPositions);
    };
  });
</script>

{#snippet hostedProjectViews()}
  {#if ProjectViewSwitcherSurface && projectsStore?.selectedProjectId}
    <ProjectViewSwitcherSurface
      projectId={projectsStore.selectedProjectId}
      onSelectView={(view) => projectsStore?.selectView(view)}
    />
  {/if}
{/snippet}

{#snippet tabBar(presentation: MobileNavigationPresentation)}
  <MobileNavigation
    tabs={workspaceTabs.tabs}
    activeTabId={workspaceTabs.activeTabId}
    tabsOpen={showTabs}
    {presentation}
    onSelect={(tabId) => workspaceTabs.activate(tabId)}
    onMove={(tabId, targetId, placement) => workspaceTabs.move(tabId, targetId, placement)}
    onOpenTabs={toggleTabs}
  />
{/snippet}

{#snippet calendarIdentityIcon()}
  <CalendarIcon size={COMPACT_IDENTITY_ICON_SIZE} strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH} />
{/snippet}

{#snippet loadingRing()}
  <svg
    viewBox="0 0 48 48"
    class="ganbaru-loading-ring size-12 text-foreground"
    aria-hidden="true"
  >
    <circle
      class="ganbaru-loading-ring-stroke"
      cx="24"
      cy="24"
      r="18"
      fill="none"
      stroke="currentColor"
      stroke-width="3"
      stroke-linecap="round"
    />
  </svg>
{/snippet}

{#snippet deferredSurfacePlaceholder(
  surface: DeferredSurface,
  title: string,
  onClose: (() => void) | null = null,
)}
  {#if deferredLoadErrors[surface]}
    {@const failure = deferredLoadErrors[surface]}
    <div class="flex h-full w-full p-6">
      <Notice
        layout="fill"
        class="text-sm"
        title={t("common.viewLoadFailed", title)}
        message={failure.message}
        onRetry={() => retryDeferredSurface(surface)}
        onDismiss={onClose ?? undefined}
      />
    </div>
  {:else}
    <div class="flex h-full w-full flex-col items-center justify-center gap-3 p-6 text-center">
      <div role="status" aria-label={t("common.loading")}>
        {@render loadingRing()}
      </div>
    </div>
  {/if}
{/snippet}

<div
  class="mobile-app-shell mobile-viewport-height flex w-screen flex-col overflow-hidden text-foreground"
  data-size-class={viewport.sizeClass}
  style="background-color: var(--cal-bg); width: calc(100vw * var(--mobile-interface-scale-inverse, 1)); height: calc(100vh * var(--mobile-interface-scale-inverse, 1)); height: calc(100dvh * var(--mobile-interface-scale-inverse, 1)); padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left);"
>
  {#if !backendReady && !loadError}
    <section
      class="grid min-h-0 flex-1 place-items-center"
      role="status"
      aria-label={t("common.loading")}
    >
      {@render loadingRing()}
    </section>
  {:else}
  <div
    class="flex min-h-0 flex-1 flex-col"
    inert={modalOpen}
    aria-hidden={modalOpen ? "true" : undefined}
  >
    {#if PomodoroMenuSurface && LinkedDeviceControlSurface}
    <MobileTopBar
      pomodoroTime={pomodoro.formattedTime}
      pomodoroActive={pomodoro.isActive}
      pomodoroRemainingSeconds={pomodoro.remainingSeconds}
      pomodoroTotalSeconds={pomodoro.totalSecondsForPhase}
      pomodoroPaused={pomodoro.isActive
        && pomodoro.phase === "focus"
        && !pomodoro.isRunning
        && !pomodoro.suspendedAway
        && !pomodoro.idlePaused}
      pomodoroPausedPulseAmount={pomodoro.pausedPulseAmount}
      pomodoroOpen={showPomodoro}
      {PomodoroMenuSurface}
      {LinkedDeviceControlSurface}
      quickNotesOpen={showQuickNotes}
      quickNotesDisabled={!backendReady}
      musicOpen={showMusic}
      musicDisabled={!backendReady}
      musicVisible={musicAvailable}
      onTogglePomodoro={() => {
        if (showPomodoro) closePomodoro();
        else openPomodoro();
      }}
      onClosePomodoro={closePomodoro}
      onOpenQuickNotes={openQuickNotes}
      onOpenMusic={openMusic}
      onOpenSettings={openSettings}
      bind:identityTarget={identitySlotElement}
      identityLabel={nav.current === "calendar" ? currentTitle : null}
      identityIcon={nav.current === "calendar" ? calendarIdentityIcon : undefined}
    />
    {/if}

    <div class="flex min-h-0 flex-1 overflow-hidden">
      {#if useNavigationRail}
        {@render tabBar("rail")}
      {/if}

      <main class="relative min-h-0 min-w-0 flex-1 overflow-hidden" style="background-color: var(--cal-bg);">
        {#if loadError}
          <div class="flex h-full p-6">
            <Notice
              layout="fill"
              class="text-sm"
              title={t("common.viewLoadFailed", currentTitle)}
              message={loadError.message}
              onRetry={retryWorkspaceLoad}
            />
          </div>
        {:else if nav.current === "calendar" && CalendarSurface}
          <CalendarSurface initialViewMode="day" mobileLayout />
        {:else if workspaceSurface === "projects" && ProjectsSurface && projectViewComponents}
          <ProjectsSurface
            mobileLayout
            viewComponents={projectViewComponents}
          />
        {:else if workspaceSurface === "notes" && NotesSurface}
          <NotesSurface mobileLayout projectViewSwitcher={projectHostingActive ? hostedProjectViews : undefined} />
        {:else if workspaceSurface === "chat" && ChatSurface}
          <ChatSurface projectViewSwitcher={projectHostingActive ? hostedProjectViews : undefined} />
        {:else if currentDeferredSurface}
          {@render deferredSurfacePlaceholder(currentDeferredSurface, t(`titleBar.tab.${workspaceSurface}`))}
        {:else}
          <div class="flex h-full items-center justify-center">
            {@render loadingRing()}
          </div>
        {/if}
        {#if showTabs && TabsPanelSurface}
          <TabsPanelSurface tabs={workspaceTabs} onClose={() => { showTabs = false; }} />
        {/if}
      </main>
    </div>
    {#if bottomNavigationVisible}
      {@render tabBar("bottom")}
    {/if}
  </div>

  {#if MusicPlaybackHostSurface}
    <MusicPlaybackHostSurface />
  {/if}

  {#if musicPanelMounted && MusicSurface}
    <div hidden={!showMusic} inert={!showMusic || suspendDecisionOpen} aria-hidden={!showMusic || suspendDecisionOpen ? "true" : undefined}>
      <MusicSurface
        visible={showMusic}
        presentation="mobile"
        mobilePlayerPanelStyle={musicPanelStyle}
        mobilePlaylistPanelStyle={musicPlaylistPanelStyle}
        onClose={closeMusic}
      />
    </div>
  {:else if showMusic}
    <div
      class="fixed z-50"
      inert={suspendDecisionOpen}
      aria-hidden={suspendDecisionOpen ? "true" : undefined}
      style="background-color: var(--cal-bg); left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left);"
    >
      {@render deferredSurfacePlaceholder("music", t("music.title"), closeMusic)}
    </div>
  {/if}

  {#if showSettings && SettingsSurface}
    <div inert={suspendDecisionOpen} aria-hidden={suspendDecisionOpen ? "true" : undefined}>
      <SettingsSurface
        presentation="mobile"
        initialSection={settingsLauncher.targetSection}
        initialDistractionsTab={settingsLauncher.targetDistractionsTab}
        initialContactsTab={settingsLauncher.targetContactsTab}
        initialChatSubsection={settingsLauncher.targetChatSubsection}
        initialChatTeammateId={settingsLauncher.targetChatTeammateId}
        initialChatChannelId={settingsLauncher.targetChatChannelId}
        initialChatCreateTeammate={settingsLauncher.targetChatCreateTeammate}
        onClose={closeSettings}
      />
    </div>
  {:else if showSettings}
    <div
      class="fixed z-50 bg-background"
      inert={suspendDecisionOpen}
      aria-hidden={suspendDecisionOpen ? "true" : undefined}
      style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left);"
    >
      {@render deferredSurfacePlaceholder("settings", t("settings.title"), closeSettings)}
    </div>
  {/if}

  {#if showQuickNotes && QuickNotesSurface}
    <div inert={suspendDecisionOpen} aria-hidden={suspendDecisionOpen ? "true" : undefined}>
      <QuickNotesSurface mobileLayout mobilePanelStyle={quickNotesPanelStyle} onClose={closeQuickNotes} />
    </div>
  {:else if showQuickNotes}
    <div
      class="fixed z-50"
      inert={suspendDecisionOpen}
      aria-hidden={suspendDecisionOpen ? "true" : undefined}
      style="background-color: var(--cal-bg); left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left);"
    >
      {@render deferredSurfacePlaceholder("quickNotes", t("quickNotes.title"), closeQuickNotes)}
    </div>
  {/if}

  {#if suspendInfo}
    <ConfirmDialog
      title={t("focusDialog.resumeTitle")}
      message={t("focusDialog.awayMessage", formatAwayDuration(suspendInfo.awaySeconds))}
      confirmLabel={t("focusDialog.resume")}
      cancelLabel={t("focusDialog.stopSessionCancel")}
      danger={false}
      onConfirm={() => { void pomodoro.dismissSuspend(true); }}
      onCancel={stopSuspendedSession}
      onDismiss={() => undefined}
    />
  {/if}
  {/if}

  {#await import("$lib/components/vault/FlaggedRollbackToast.svelte") then module}
    {@const FlaggedRollbackToast = module.default}
    <FlaggedRollbackToast />
  {/await}
  {#if notesHeldEditNotice.current}
    {#await import("$lib/components/notes/conflicts/NotesHeldEditToast.svelte") then module}
      {@const NotesHeldEditToast = module.default}
      <NotesHeldEditToast />
    {/await}
  {/if}
</div>
