<script lang="ts">
  import { getNavigation } from "$lib/stores/navigation.svelte";
  import { availableAppViews, type View } from "$lib/navigation";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { createAppWorkspaceLocationAdapter } from "$lib/workspace-tabs/app-location-adapter";
  import { getCalendar } from "$lib/stores/calendar.svelte";
  import { getCalendars } from "$lib/stores/calendars.svelte";
  import { getDistractions } from "$lib/stores/distractions.svelte";
  import { DISTRACTIONS_USAGE_REFRESH_INTERVAL_MS, getDistractionsUsage } from "$lib/stores/distractions-usage.svelte";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";
  import { buildDesktopFocusNotificationCopy, type DesktopFocusNotificationCopy } from "$lib/api/focus";
  import { getNotes, getNotesWorkspace } from "$lib/stores/notes.svelte";
  import { getProjects, getProjectsStore, type ProjectsStore } from "$lib/stores/projects.svelte";
  import type { WorkspacePaneScope } from "$lib/workspace-tabs/model";
  import { getChat, getChatStore, resetChatForVault } from "$lib/stores/chat.svelte";
  import { getZoom } from "$lib/stores/zoom.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { getUpdateManager } from "$lib/stores/updates.svelte";
  import { UPDATE_AUTO_CHECK_INTERVAL_MS } from "$lib/stores/update-policy";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { parseNotesLinkHash } from "$lib/notes/links/block-link";
  import {
    listPendingNotesMentionNotifications,
    markNotesMentionNotificationsDelivered,
  } from "$lib/api/notes";
  import { getNotesNotificationSchedule } from "$lib/notes/collaboration/notification-schedule.svelte";
  import type {
    NotesMentionNotification,
    NotesMentionNotificationKind,
  } from "$lib/notes/types";
  import { MAIN_WINDOW_LABEL, createWorkspaceTabWindows, parseTabWindowSearch } from "$lib/windows/tab-windows";
  import { flushBeforeWindowClose } from "$lib/windows/close-flush";
  import { ensureDbUrl } from "$lib/api/db";
  import { prepareDesktopWorkspace } from "$lib/windows/desktop-workspace-readiness";
  import "$lib/stores/app-session";
  import { Temporal } from "@js-temporal/polyfill";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { appNavigationShortcut, hasShortcutModifier } from "$lib/keyboard-shortcuts";
  import { isAppKeyboardIsolated } from "$lib/modal-focus";
  import TitleBar from "$lib/components/title-bar/TitleBar.svelte";
  import { getAppCloseCoordinator } from "$lib/components/title-bar/shortcut-controller.svelte";
  import WindowResizeHandles from "$lib/components/WindowResizeHandles.svelte";
  import CalendarView from "$lib/components/calendar/CalendarView.svelte";
  import MusicPlaybackHost from "$lib/components/music/MusicPlaybackHost.svelte";
  import MusicSoundscapeCoordinator from "$lib/components/music/soundscape/MusicSoundscapeCoordinator.svelte";
  import NotesView from "$lib/components/notes/NotesView.svelte";
  import ProjectsView from "$lib/components/projects/ProjectsView.svelte";
  import ProjectsSidebar from "$lib/components/projects/sidebar/ProjectsSidebar.svelte";
  import ProjectDashboardView from "$lib/components/projects/views/ProjectDashboardView.svelte";
  import ProjectGanttView from "$lib/components/projects/views/ProjectGanttView.svelte";
  import ProjectKanbanView from "$lib/components/projects/views/ProjectKanbanView.svelte";
  import ProjectListView from "$lib/components/projects/list/ProjectListView.svelte";
  import type { ProjectViewComponents } from "$lib/components/projects/view-components";
  import ChatWorkspace from "$lib/components/chat/ChatWorkspace.svelte";
  import WorkspaceSplitView from "$lib/components/workspace-tabs/WorkspaceSplitView.svelte";
  import { workspaceShownPanes } from "$lib/workspace-tabs/shown-panes";
  import type { WorkspaceSplitArea } from "$lib/workspace-tabs/split-layout";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import TooltipHost from "$lib/components/ui/TooltipHost.svelte";
  import UpdateNotificationToast from "$lib/components/updates/UpdateNotificationToast.svelte";
  import { formatEventNotificationBody } from "$lib/calendar/event-notifications";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    firstMarkTime,
    mark as perfMark,
    setShellStartupMs,
  } from "$lib/stores/perf-log.svelte";
  import type { MemoryReport, StartupMemorySnapshot } from "$lib/diagnostics/memory-report";
  import { shouldUseKeyboardFocusIntent } from "$lib/utils";
  import {
    createLifecycleScheduler,
  } from "$lib/scheduling/lifecycle-scheduler";
  import {
    createEventNotificationScheduler,
    createNotesNotificationScheduler,
    createProjectTaskReminderScheduler,
  } from "$lib/scheduling/notification-schedulers";
  import { onMount } from "svelte";
  import { getNotesProjectHistoryScheduler } from "$lib/notes/history/project-history-scheduler";
  import { activeVaultIdentity, onActiveVaultIdentityChange } from "$lib/vault/active-vault";
  import { listenForNotesDatabaseChanges } from "$lib/notes/database/window-sync";
  import { listUpcomingProjectTaskReminders } from "$lib/api/project-task-reminders";
  import { getProjectTaskReminderSchedule } from "$lib/projects/task-reminder-schedule.svelte";
  import { projectTaskReminderNotificationText } from "$lib/projects/task-reminder-notification";
  import type { ProjectChatIntegration, ProjectHostedViewType } from "$lib/projects/types";
  import { projectHostablePlaces, projectPlaceShortcut } from "$lib/projects/places";
  import type { QuickNotesTrashPurgeRuntime } from "$lib/quick-notes/trash-purge-runtime";

  perfMark("boot.script-start");

  const appWindow = getCurrentWindow();
  const appClose = getAppCloseCoordinator();
  const isMainWindow = appWindow.label === MAIN_WINDOW_LABEL;
  const nav = getNavigation();
  const calendar = getCalendar();
  const calendars = getCalendars();
  const distractions = getDistractions();
  const distractionsUsage = getDistractionsUsage();
  const music = getMusicPlayer();
  const pomodoro = getPomodoro();
  const notes = getNotes();
  const secondaryNotes = getNotesWorkspace("secondary");
  const projects = getProjects();
  const secondaryProjects = getProjectsStore("secondary");
  const chat = getChat();
  const secondaryChat = getChatStore("secondary");
  const zoom = getZoom();
  const preferences = getPreferences();
  const settingsLauncher = getSettingsLauncher();
  const updates = getUpdateManager();
  const viewport = getViewport();
  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);
  const workspaceTabs = getWorkspaceTabs();
  workspaceTabs.connect({
    adapter: createAppWorkspaceLocationAdapter(
      { projects, notes, chat, secondary: { projects: secondaryProjects, notes: secondaryNotes, chat: secondaryChat } },
      t,
    ),
    ...(platformHasCapability(BUILD_PLATFORM_PROFILE, "window.detached-views")
      ? { windows: createWorkspaceTabWindows(appWindow.label, availableAppViews()) }
      : {}),
    initialTab: isMainWindow ? null : parseTabWindowSearch(window.location.search, availableAppViews()),
    flushPendingEdits: flushBeforeWindowClose,
    requestWindowClose: () => appClose.request(),
  });
  const notesNotificationSchedule = getNotesNotificationSchedule();
  const projectTaskReminderSchedule = getProjectTaskReminderSchedule();
  const notesProjectHistoryScheduler = getNotesProjectHistoryScheduler();
  const projectViewComponents = {
    list: ProjectListView,
    kanban: ProjectKanbanView,
    calendar: CalendarView,
    gantt: ProjectGanttView,
    dashboard: ProjectDashboardView,
  } satisfies ProjectViewComponents;
  const projectChatIntegration: ProjectChatIntegration = {
    listWorkingFolders: (projectId) => chat.workingFolders
      .filter((entry) => (
        entry.workingFolder.projectId === projectId
        && entry.workingFolder.archivedAt === null
      ))
      .map((entry) => ({
        id: entry.workingFolder.id,
        displayName: entry.workingFolder.displayName,
      })),
    ensureLoaded: () => chat.ensureLoaded(),
    // Selects in the stores of the pane Chat shows in, which may be the other side of a split.
    openProject: async (projectId, workingFolderId) => {
      const scope = workspaceTabs.navigationScope({ kind: "chat", projectId, item: null });
      const target = scope === "secondary" ? secondaryChat : chat;
      await target.ensureLoaded();
      if (workingFolderId) {
        target.selectWorkingFolder(workingFolderId);
      } else {
        await projectsIn(scope).selectProject(projectId);
        await target.syncProjectSelection(projectId);
      }
      workspaceTabs.navigateIn("chat", scope);
    },
  };
  let unlistenCalendarNotificationOpen: UnlistenFn | null = null;
  let unlistenNotesNotificationOpen: UnlistenFn | null = null;
  let unlistenProjectTaskNotificationOpen: UnlistenFn | null = null;
  let unlistenDistractionsDesktopSettingsOpen: UnlistenFn | null = null;
  let unlistenDistractionsLimitsSettingsOpen: UnlistenFn | null = null;
  const NOTES_NOTIFICATION_BODY_MAX_CHARS = 180;
  const AUTOMATIC_UPDATE_CHECK_DELAY_MS = 3_000;
  const FOCUS_NOTIFICATION_COPY_RETRY_MS = 1_000;

  interface NotesNotificationOpenPayload {
    page_id: string;
    block_id?: string | null;
  }

  let isMaximized = $state(true);
  type BenchmarkOverlayComponent = typeof import("$lib/components/diagnostics/BenchmarkOverlay.svelte").default;
  type IdleOverlayComponent = typeof import("$lib/components/pomodoro/IdleOverlay.svelte").default;
  let BenchmarkOverlay = $state<BenchmarkOverlayComponent | null>(null);
  let IdleOverlay = $state<IdleOverlayComponent | null>(null);
  let loadingBenchmarkOverlay: Promise<void> | null = null;
  let loadingIdleOverlay: Promise<void> | null = null;
  let devtoolsToggleInFlight = false;
  function ensureBenchmarkOverlay(): Promise<void> {
    if (BenchmarkOverlay) return Promise.resolve();
    loadingBenchmarkOverlay ??= import("$lib/components/diagnostics/BenchmarkOverlay.svelte")
      .then((module) => {
        BenchmarkOverlay = module.default;
      })
      .finally(() => {
        loadingBenchmarkOverlay = null;
      });
    return loadingBenchmarkOverlay;
  }

  async function afterAnimationFrames(count: number): Promise<void> {
    for (let i = 0; i < count; i++) {
      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => resolve());
      });
    }
  }

  async function startNormalCalendarBoot(): Promise<void> {
    calendars.load().catch((e) => console.error("Failed to load calendars:", e));
    if (isMainWindow) {
      await pomodoro.cleanupOrphans().catch((e) => console.warn("Failed to clean up orphans:", e));
    }
    try {
      await calendar.load();
    } catch (e) {
      console.error("Failed to load calendar:", e);
    }
  }

  async function startBenchmarkOrNormalCalendarBoot(): Promise<void> {
    let benchmarkClaimedBoot = false;
    try {
      const stateJson = await invoke<string | null>("read_benchmark_state");
      if (stateJson) {
        // Resolve the DB URL while the benchmark state is still pending.
        // The runner flips the state to running before scenario setup, and
        // running states intentionally fall back to the user DB on a fresh
        // boot so interrupted benchmarks cannot keep touching benchmark data.
        await ensureDbUrl();
        await ensureBenchmarkOverlay();
        await afterAnimationFrames(2);
        const { getBenchmarkRunner } = await import("$lib/stores/benchmark-runner.svelte");
        benchmarkClaimedBoot = await getBenchmarkRunner().checkAndResume();
      }
    } catch (e) {
      console.error("benchmark resume failed:", e);
    } finally {
      if (!benchmarkClaimedBoot) {
        await startNormalCalendarBoot();
        // Benchmark relaunches skip this so restored windows do not run beside every sample.
        if (isMainWindow) void workspaceTabs.restoreWindows();
      }
    }
  }
  function loadIdleOverlay(): Promise<void> {
    if (IdleOverlay) return Promise.resolve();
    loadingIdleOverlay ??= import("$lib/components/pomodoro/IdleOverlay.svelte")
      .then((module) => {
        IdleOverlay = module.default;
      })
      .finally(() => {
        loadingIdleOverlay = null;
      });
    return loadingIdleOverlay;
  }

  function parseNotesNotificationOpenPayload(
    payload: unknown,
  ): NotesNotificationOpenPayload | null {
    if (!payload || typeof payload !== "object") return null;
    const record = payload as Record<string, unknown>;
    if (typeof record.page_id !== "string") return null;
    const blockId = record.block_id;
    if (blockId !== null && blockId !== undefined && typeof blockId !== "string") return null;
    return {
      page_id: record.page_id,
      block_id: blockId,
    };
  }

  function notesHashForNotification(payload: NotesNotificationOpenPayload): string {
    const params = new URLSearchParams({ page: payload.page_id });
    if (payload.block_id) params.set("block", payload.block_id);
    return `#notes?${params.toString()}`;
  }

  function openNotesNotification(payload: NotesNotificationOpenPayload): void {
    const nextHash = notesHashForNotification(payload);
    nav.navigate("notes");
    if (window.location.hash === nextHash) {
      window.dispatchEvent(new HashChangeEvent("hashchange"));
      return;
    }
    window.location.hash = nextHash;
  }

  function projectTaskNotificationProjectId(payload: unknown): string | null {
    if (!payload || typeof payload !== "object") return null;
    const projectId = (payload as Record<string, unknown>).project_id;
    return typeof projectId === "string" && projectId.length > 0 ? projectId : null;
  }

  async function openProjectTaskNotification(projectId: string): Promise<void> {
    try {
      await projects.selectProject(projectId);
      projects.showProjectView();
      nav.navigate("projects");
    } catch (error: unknown) {
      console.error("Failed to open the project of a task reminder", error);
    }
  }

  /**
   * Time spent before App.svelte could emit `boot.script-start`. The Rust
   * command is process-spawn anchored, while performance.now is anchored to
   * the WebKit document. Adding the script-start mark produces the baseline
   * used by the launch table and benchmark output.
   */
  let shellStartupMs = $state<number | null>(null);
  let startupMemorySnapshot = $state<StartupMemorySnapshot>({ status: "pending" });

  onMount(() => {
    perfMark("boot.app-mount");
    let disposed = false;
    let unlistenDatabaseChanges: (() => void) | null = null;
    void listenForNotesDatabaseChanges().then((unlisten) => {
      if (disposed) unlisten();
      else unlistenDatabaseChanges = unlisten;
    }).catch((error: unknown) => console.error("Failed to listen for Notes database changes", error));
    const automaticUpdateCheckTimerId = isMainWindow
      ? setTimeout(() => {
        void updates.checkAutomatically({ kind: "startup" });
      }, AUTOMATIC_UPDATE_CHECK_DELAY_MS)
      : null;
    const automaticUpdateCheckIntervalId = isMainWindow
      ? setInterval(() => {
        void updates.checkAutomatically({ kind: "periodic" });
      }, UPDATE_AUTO_CHECK_INTERVAL_MS + AUTOMATIC_UPDATE_CHECK_DELAY_MS + 1_000)
      : null;
    const handleMusicAssignmentInspection = (event: Event) => {
      if (!(event instanceof CustomEvent) || event.detail?.ready === true) return;
      // A shown Calendar inspects the event itself; a Calendar beside the active tab only takes focus.
      if (shownKinds.includes("calendar")) {
        if (nav.current !== "calendar") nav.navigate("calendar");
        return;
      }
      const eventId = typeof event.detail?.eventId === "string" ? event.detail.eventId : null;
      if (!eventId) return;
      nav.navigate("calendar");
      window.setTimeout(() => {
        window.dispatchEvent(new CustomEvent("ganbaru:inspect-music-assignment", {
          detail: { eventId, ready: true },
        }));
      }, 0);
    };
    window.addEventListener("ganbaru:inspect-music-assignment", handleMusicAssignmentInspection);
    if (isMainWindow) {
      notesProjectHistoryScheduler.setEnabled(true);
      listen("calendar-notification-open", () => {
        if (disposed) return;
        nav.navigate("calendar");
      })
        .then((unlisten) => {
          if (disposed) {
            unlisten();
            return;
          }
          unlistenCalendarNotificationOpen = unlisten;
        })
        .catch((e) => console.error("Failed to listen for calendar notification opens:", e));
      listen<unknown>("notes-notification-open", (event) => {
        if (disposed) return;
        const payload = parseNotesNotificationOpenPayload(event.payload);
        if (!payload) return;
        openNotesNotification(payload);
      })
        .then((unlisten) => {
          if (disposed) {
            unlisten();
            return;
          }
          unlistenNotesNotificationOpen = unlisten;
        })
        .catch((e) => console.error("Failed to listen for Notes notification opens:", e));
      listen<unknown>("project-task-notification-open", (event) => {
        if (disposed) return;
        const projectId = projectTaskNotificationProjectId(event.payload);
        if (projectId) void openProjectTaskNotification(projectId);
      })
        .then((unlisten) => {
          if (disposed) {
            unlisten();
            return;
          }
          unlistenProjectTaskNotificationOpen = unlisten;
        })
        .catch((e) => console.error("Failed to listen for project task notification opens:", e));
      listen("distractions-open-desktop-settings", () => {
        if (disposed) return;
        settingsLauncher.open("distractions", { distractionsTab: "desktop" });
      })
        .then((unlisten) => {
          if (disposed) {
            unlisten();
            return;
          }
          unlistenDistractionsDesktopSettingsOpen = unlisten;
        })
        .catch((e) => console.error("Failed to listen for distraction settings opens:", e));
      listen("distractions-open-limits-settings", () => {
        if (disposed) return;
        settingsLauncher.open("distractions", { distractionsTab: "limits" });
      })
        .then((unlisten) => {
          if (disposed) {
            unlisten();
            return;
          }
          unlistenDistractionsLimitsSettingsOpen = unlisten;
        })
        .catch((e) => console.error("Failed to listen for distraction limit settings opens:", e));
    }
    const unsubscribeHistoryVault = isMainWindow
      ? onActiveVaultIdentityChange((previousVaultId, nextVaultId) => {
          notesProjectHistoryScheduler.switchVault();
          resetChatForVault();
          syncQuickNotesTrashPurge();
          projectTaskReminderScheduler.invalidate();
          if (!nextVaultId) return;
          const projectsRequest = previousVaultId ? projects.load() : projects.ensureLoaded();
          void projectsRequest.then(() => Promise.all([
            previousVaultId ? notes.load() : notes.ensureLoaded(),
            // The secondary workspace shares the primary's page lists, so it only drops the previous vault's pages.
            previousVaultId ? secondaryNotes.selectPage(null) : null,
            chat.prewarmForProject(projects.selectedProjectId),
          ])).catch((error) => {
            console.error("core workspace preload failed", error);
          });
        })
      : null;

    if (isMainWindow) {
      void startQuickNotesTrashPurgeJob();
      void prepareDesktopWorkspace()
        .catch((error) => {
          console.error("core workspace preload failed", error);
        });
    }

    // Valid benchmark boots are claimed before normal calendar hydration so
    // the measured window is the scenario anchor, not today's normal window.
    startBenchmarkOrNormalCalendarBoot().catch((e) =>
      console.error("calendar boot failed:", e),
    );
    appWindow.isMaximized().then((v) => (isMaximized = v));
    invoke<number>("startup_elapsed_ms").then((ms) => {
      const scriptStartMs = firstMarkTime("boot.script-start") ?? 0;
      const nextShellStartupMs = Math.max(0, Math.round(ms - performance.now() + scriptStartMs));
      shellStartupMs = nextShellStartupMs;
      setShellStartupMs(nextShellStartupMs);
    });
    const startupMemoryTimerId = setTimeout(() => {
      invoke<MemoryReport>("memory_report")
        .then((report) => {
          startupMemorySnapshot = { status: "ready", report };
        })
        .catch((e) => {
          startupMemorySnapshot = {
            status: "failed",
            message: e instanceof Error ? e.message : String(e),
          };
        });
    }, 10_000);

    // Prevent native webview scaling from modified wheel input.
    const blockNativeWheelScale = (e: WheelEvent) => { if (hasShortcutModifier(e)) e.preventDefault(); };
    const blockNativeContextMenu = (e: MouseEvent) => {
      e.preventDefault();
    };
    document.addEventListener("wheel", blockNativeWheelScale, { passive: false, capture: true });
    document.addEventListener("contextmenu", blockNativeContextMenu, { capture: true });

    const root = document.documentElement;
    const navigateToNotesHash = () => {
      if (parseNotesLinkHash(window.location.hash)) nav.navigate("notes");
    };
    const markPointerFocus = () => {
      root.dataset.focusIntent = "pointer";
    };
    const markKeyboardFocus = (event: KeyboardEvent) => {
      if (shouldUseKeyboardFocusIntent(event)) {
        root.dataset.focusIntent = "keyboard";
      }
    };
    markPointerFocus();
    navigateToNotesHash();
    document.addEventListener("pointerdown", markPointerFocus, { capture: true });
    document.addEventListener("keydown", markKeyboardFocus, { capture: true });
    window.addEventListener("hashchange", navigateToNotesHash);

    // Track device time zone changes (travel, OS-level update) and reload
    // Calendar events so wall-clock strings reflect the new zone. The zone is
    // re-resolved on visibility change and window focus, which also catch
    // every scheduler up after suspend.
    let knownZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const checkZone = () => {
      const current = Intl.DateTimeFormat().resolvedOptions().timeZone;
      if (current !== knownZone) {
        knownZone = current;
        calendar.load().catch((e) => console.error("Failed to reload calendar after zone change:", e));
      }
    };
    const onVisibility = () => {
      if (document.visibilityState !== "visible") return;
      checkZone();
      resumeLifecycleSchedulers();
    };
    const onFocus = () => {
      checkZone();
      resumeLifecycleSchedulers();
    };
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("focus", onFocus);

    return () => {
      disposed = true;
      unlistenDatabaseChanges?.();
      unlistenCalendarNotificationOpen?.();
      unlistenCalendarNotificationOpen = null;
      unlistenNotesNotificationOpen?.();
      unlistenNotesNotificationOpen = null;
      unlistenProjectTaskNotificationOpen?.();
      unlistenProjectTaskNotificationOpen = null;
      unlistenDistractionsDesktopSettingsOpen?.();
      unlistenDistractionsDesktopSettingsOpen = null;
      unlistenDistractionsLimitsSettingsOpen?.();
      unlistenDistractionsLimitsSettingsOpen = null;
      document.removeEventListener("wheel", blockNativeWheelScale, { capture: true });
      document.removeEventListener("contextmenu", blockNativeContextMenu, { capture: true });
      document.removeEventListener("pointerdown", markPointerFocus, { capture: true });
      document.removeEventListener("keydown", markKeyboardFocus, { capture: true });
      window.removeEventListener("hashchange", navigateToNotesHash);
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("ganbaru:inspect-music-assignment", handleMusicAssignmentInspection);
      if (automaticUpdateCheckTimerId) clearTimeout(automaticUpdateCheckTimerId);
      if (automaticUpdateCheckIntervalId) clearInterval(automaticUpdateCheckIntervalId);
      clearTimeout(startupMemoryTimerId);
      unsubscribeHistoryVault?.();
      if (isMainWindow) {
        void notesProjectHistoryScheduler.shutdown().catch((error) => {
          console.error("Notes project history shutdown flush failed", error);
        });
      }
      disposeLifecycleSchedulers();
    };
  });

  let pendingFocusNotificationCopy: DesktopFocusNotificationCopy | null = null;
  let publishingFocusNotificationCopy = false;
  let focusNotificationCopyActive = true;
  let focusNotificationCopyRetry: ReturnType<typeof setTimeout> | null = null;

  onMount(() => () => {
    focusNotificationCopyActive = false;
    pendingFocusNotificationCopy = null;
    if (focusNotificationCopyRetry !== null) clearTimeout(focusNotificationCopyRetry);
  });

  async function publishFocusNotificationCopy(): Promise<void> {
    if (publishingFocusNotificationCopy) return;
    publishingFocusNotificationCopy = true;
    try {
      while (pendingFocusNotificationCopy && focusNotificationCopyActive) {
        const copy = pendingFocusNotificationCopy;
        pendingFocusNotificationCopy = null;
        try {
          await invoke("focus_notification_copy", { copy });
        } catch (error) {
          console.error("Failed to configure native Focus notification language:", error);
          if (focusNotificationCopyActive) {
            pendingFocusNotificationCopy ??= copy;
            focusNotificationCopyRetry = setTimeout(() => {
              focusNotificationCopyRetry = null;
              void publishFocusNotificationCopy();
            }, FOCUS_NOTIFICATION_COPY_RETRY_MS);
          }
          break;
        }
      }
    } finally {
      publishingFocusNotificationCopy = false;
    }
  }

  $effect(() => {
    if (!isMainWindow) return;
    if (focusNotificationCopyRetry !== null) {
      clearTimeout(focusNotificationCopyRetry);
      focusNotificationCopyRetry = null;
    }
    pendingFocusNotificationCopy = buildDesktopFocusNotificationCopy(t);
    void publishFocusNotificationCopy();
  });

  $effect(() => {
    let cleanup: (() => void) | undefined;
    let disposed = false;
    appWindow.onResized(() => {
      if (disposed) return;
      appWindow.isMaximized().then((value) => {
        if (!disposed) isMaximized = value;
      });
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
        return;
      }
      cleanup = unlisten;
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  });

  // Runs before the first paint so a remembered Notes or Chat view opens without showing a task view first.
  $effect.pre(() => {
    projects.hostableViewTypes = projectHostablePlaces(availableAppViews());
  });
  /** The Projects store a pane scope shows through. */
  function projectsIn(scope: WorkspacePaneScope): ProjectsStore {
    return scope === "secondary" ? secondaryProjects : projects;
  }
  /** Workspace a Projects pane hosts in place of a task view; hosting needs an open project to scope it. */
  function projectHostedPlace(scope: WorkspacePaneScope): ProjectHostedViewType | null {
    const store = projectsIn(scope);
    const place = store.hostedPlace;
    return place && store.selectedProject && store.selectedGroup ? place : null;
  }
  /** The area the split view shows in, which decides how the tab menu offers to arrange a split. */
  let splitArea = $state<WorkspaceSplitArea>({ width: 0, height: 0 });
  /** The kinds of the active tab and the tab shown beside it, if any. */
  const shownKinds = $derived<readonly View[]>(
    workspaceTabs.partnerTab ? [nav.current, workspaceTabs.partnerTab.location.kind] : [nav.current],
  );
  const shownPanes = $derived.by(() => {
    const partner = workspaceTabs.partnerTab;
    return workspaceShownPanes(
      { id: workspaceTabs.activeTabId, kind: nav.current },
      partner ? { id: partner.id, kind: partner.location.kind } : null,
      partner !== null && workspaceTabs.split?.tabIds[0] === partner.id,
      { scopeOf: workspaceTabs.paneScope, hostedViewIn: projectHostedPlace },
    );
  });

  const suspendInfo = $derived(pomodoro.suspendedAway);
  const idleInfo = $derived(pomodoro.idlePaused);

  $effect(() => {
    if (idleInfo && !idleInfo.nativeOverlay) void loadIdleOverlay();
  });

  function formatAwayDuration(totalSeconds: number): string {
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    if (hours > 0 && minutes > 0) return t("focusDialog.awayHoursMinutes", hours, minutes);
    if (hours > 0) return t("focusDialog.awayHours", hours);
    if (minutes > 0) return t("focusDialog.awayMinutes", minutes);
    return t("focusDialog.awaySeconds", totalSeconds);
  }

  const distractionsUsageScheduler = createLifecycleScheduler({
    run: async (context) => {
      await distractionsUsage.refresh(context);
      return context.isCurrent() ? context.now() + DISTRACTIONS_USAGE_REFRESH_INTERVAL_MS : null;
    },
    onError: (error) => {
      console.warn("Failed to refresh desktop usage presentation:", error);
    },
  });

  $effect(() => {
    const _limits = distractions.usageLimits;
    const enabled = isMainWindow
      && distractions.limitsEnabled
      && distractions.usageLimits.some((limit) => limit.enabled);
    const wasEnabled = distractionsUsageScheduler.isEnabled();
    distractionsUsageScheduler.setEnabled(enabled);
    if (enabled && wasEnabled) distractionsUsageScheduler.invalidate();
  });

  function toggleDevtools(): void {
    if (!import.meta.env.DEV || devtoolsToggleInFlight) return;
    devtoolsToggleInFlight = true;
    invoke<boolean>("toggle_devtools")
      .catch((error) => {
        console.warn("Failed to toggle DevTools:", error);
      })
      .finally(() => {
        devtoolsToggleInFlight = false;
      });
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.isComposing) return;
    if (
      import.meta.env.DEV
      && (e.key === "F12" || (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "i"))
    ) {
      e.preventDefault();
      e.stopImmediatePropagation();
      toggleDevtools();
      return;
    }
    if (isAppKeyboardIsolated()) return;

    const action = appNavigationShortcut(e);
    if (!action) {
      handleProjectPlaceShortcut(e);
      return;
    }
    if (action.type !== "settings" && (suspendInfo || idleInfo)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (action.type === "settings") {
      if (settingsLauncher.isOpen) settingsLauncher.close();
      else settingsLauncher.open();
    } else if (action.type === "tab") {
      workspaceTabs.activateAt(action.index);
    } else if (action.type === "relative-tab") {
      workspaceTabs.activateRelative(action.direction);
    } else if (action.type === "new-tab") {
      workspaceTabs.requestLauncher();
    } else if (action.type === "reopen-tab") {
      workspaceTabs.reopen();
    } else {
      workspaceTabs.close(workspaceTabs.activeTabId);
    }
  }

  /**
   * Switches project views from a hosted workspace and holds the shortcut while the app is suspended or idle.
   * ProjectsView handles it otherwise, so its settings close guard applies.
   */
  function handleProjectPlaceShortcut(e: KeyboardEvent): void {
    if (nav.current !== "projects") return;
    const scope = workspaceTabs.paneScope(workspaceTabs.activeTabId);
    const store = projectsIn(scope);
    const view = projectPlaceShortcut(e, store.viewsForProject(store.selectedProjectId));
    if (!view) return;
    const paused = Boolean(suspendInfo || idleInfo);
    if (!paused && !projectHostedPlace(scope)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (!paused) store.selectView(view);
  }

  // Notes mention notifications
  function notesMentionNotificationSchedulerEnabled(): boolean {
    return preferences.notesMentionNotificationsEnabled
      && (
        preferences.notesReminderNotificationsEnabled
        || preferences.notesUserMentionNotificationsEnabled
        || preferences.notesTaskMentionNotificationsEnabled
      );
  }

  function notesMentionNotificationTitle(kind: NotesMentionNotificationKind): string {
    if (kind === "reminder") return t("notes.notification.reminderTitle");
    if (kind === "task_mention") return t("notes.notification.taskMentionTitle");
    return t("notes.notification.userMentionTitle");
  }

  function compactNotificationText(value: string): string {
    return value.replace(/\s+/gu, " ").trim();
  }

  function truncateNotificationText(value: string): string {
    const compact = compactNotificationText(value);
    if (compact.length <= NOTES_NOTIFICATION_BODY_MAX_CHARS) return compact;
    return `${compact.slice(0, NOTES_NOTIFICATION_BODY_MAX_CHARS - 3).trimEnd()}...`;
  }

  function notesMentionNotificationBody(notification: NotesMentionNotification): string {
    const pageTitle = notification.page_title.trim() || t("notes.untitled");
    if (preferences.notesNotificationIncludeContent) {
      if (notification.source_type === "page_property") {
        return truncateNotificationText(t("notes.notification.propertyBody", notification.plain_text, pageTitle));
      }
      const sourceText = truncateNotificationText(
        notification.source_plain_text || notification.plain_text,
      );
      if (sourceText) return sourceText;
    }
    return t("notes.notification.privateBody", pageTitle);
  }

  async function deliverNotesMentionNotification(
    notification: NotesMentionNotification,
  ): Promise<void> {
    await invoke("show_notes_notification", {
      title: notesMentionNotificationTitle(notification.kind),
      body: notesMentionNotificationBody(notification),
      pageId: notification.page_id,
      blockId: notification.block_id,
      playSound: true,
    });
    await markNotesMentionNotificationsDelivered({ ids: [notification.id] });
  }

  const notesNotificationScheduler = createNotesNotificationScheduler({
    listPending: listPendingNotesMentionNotifications,
    getPreferences: () => ({
      mentionNotificationsEnabled: preferences.notesMentionNotificationsEnabled,
      reminderNotificationsEnabled: preferences.notesReminderNotificationsEnabled,
      userMentionNotificationsEnabled: preferences.notesUserMentionNotificationsEnabled,
      taskMentionNotificationsEnabled: preferences.notesTaskMentionNotificationsEnabled,
    }),
    deliver: deliverNotesMentionNotification,
    dismiss: async (notification) => {
      await markNotesMentionNotificationsDelivered({ ids: [notification.id] });
    },
    onError: (error) => {
      console.error("[notes notifications] check failed:", error);
    },
    onDeliveryError: (error) => {
      console.error("[notes notifications] failed:", error);
    },
  });

  $effect(() => {
    const _enabled = preferences.notesMentionNotificationsEnabled;
    const _reminders = preferences.notesReminderNotificationsEnabled;
    const _users = preferences.notesUserMentionNotificationsEnabled;
    const _tasks = preferences.notesTaskMentionNotificationsEnabled;
    const _version = notesNotificationSchedule.version;
    const enabled = isMainWindow && notesMentionNotificationSchedulerEnabled();
    const wasEnabled = notesNotificationScheduler.isEnabled();
    notesNotificationScheduler.setEnabled(enabled);
    if (enabled && wasEnabled) notesNotificationScheduler.invalidate();
  });

  const chatScheduledMessageScheduler = createLifecycleScheduler({
    errorRetryMs: 60_000,
    run: async (context) => {
      const result = await chat.dispatchDueScheduledMessages();
      if (!context.isCurrent() || !result.nextDispatchAt) return null;
      const deadline = Date.parse(result.nextDispatchAt);
      return Number.isFinite(deadline) ? deadline : null;
    },
    onError: (error) => {
      console.error("[chat scheduled messages] dispatch failed:", error);
    },
  });

  $effect(() => {
    const _version = chat.scheduledMessagesVersion;
    const enabled = isMainWindow && chat.loaded;
    const wasEnabled = chatScheduledMessageScheduler.isEnabled();
    chatScheduledMessageScheduler.setEnabled(enabled);
    if (enabled && wasEnabled) chatScheduledMessageScheduler.invalidate();
  });

  // Event notifications
  const eventNotificationScheduler = createEventNotificationScheduler({
    getEvents: () => {
      const today = Temporal.Now.plainDateISO();
      return calendar.eventsInWindow(
        today.subtract({ days: 1 }),
        today.add({ days: 7 }),
      );
    },
    deliver: async ({ event }) => {
      const now = new Date();
      const title = event.title.trim() || t("calendar.notification.titleFallback");
      const body = formatEventNotificationBody(event, now, { t, locale });
      await invoke("show_event_notification", { title, body, openCalendar: true });
    },
    onError: (error) => {
      console.error("[notifications] failed:", error);
    },
  });

  // Recompute the exact next deadline whenever the calendar window changes.
  $effect(() => {
    const _v = calendar.indexVersion;
    const enabled = isMainWindow && calendar.loaded;
    const wasEnabled = eventNotificationScheduler.isEnabled();
    eventNotificationScheduler.setEnabled(enabled);
    if (enabled && wasEnabled) eventNotificationScheduler.invalidate();
  });

  // Projects task date reminders
  const projectTaskReminderScheduler = createProjectTaskReminderScheduler({
    listUpcoming: listUpcomingProjectTaskReminders,
    deliver: async ({ reminder }) => {
      const text = projectTaskReminderNotificationText(reminder, {
        startTitle: (task) => t("projects.taskReminder.startTitle", task),
        dueTitle: (task) => t("projects.taskReminder.dueTitle", task),
        untitledTask: t("projects.taskReminder.untitledTask"),
        body: (project, when) => t("projects.taskReminder.body", project, when),
      }, locale, preferences.calendarTimeFormat);
      await invoke("show_project_task_notification", {
        title: text.title,
        body: text.body,
        projectId: reminder.projectId,
        taskId: reminder.taskId,
        playSound: true,
      });
    },
    onError: (error) => {
      console.error("[project task reminders] failed:", error);
    },
  });

  $effect(() => {
    const _version = projectTaskReminderSchedule.version;
    const enabled = isMainWindow && projects.loaded;
    const wasEnabled = projectTaskReminderScheduler.isEnabled();
    projectTaskReminderScheduler.setEnabled(enabled);
    if (enabled && wasEnabled) projectTaskReminderScheduler.invalidate();
  });

  let quickNotesTrashPurge: QuickNotesTrashPurgeRuntime | null = null;
  let lifecycleSchedulersDisposed = false;

  /** Loads the Quick notes trash purge job outside the startup bundle. */
  async function startQuickNotesTrashPurgeJob(): Promise<void> {
    try {
      const module = await import("$lib/quick-notes/trash-purge-runtime");
      if (lifecycleSchedulersDisposed || quickNotesTrashPurge) return;
      quickNotesTrashPurge = module.startQuickNotesTrashPurge();
      syncQuickNotesTrashPurge();
    } catch (error: unknown) {
      console.error("Failed to start the Quick notes trash purge", error);
    }
  }

  /** Runs the purge for the active vault, and stops it while no vault is open. */
  function syncQuickNotesTrashPurge(): void {
    const scheduler = quickNotesTrashPurge?.scheduler;
    if (!scheduler) return;
    const enabled = activeVaultIdentity() !== null;
    const wasEnabled = scheduler.isEnabled();
    scheduler.setEnabled(enabled);
    if (enabled && wasEnabled) scheduler.invalidate();
  }

  function resumeLifecycleSchedulers(): void {
    eventNotificationScheduler.resume();
    notesNotificationScheduler.resume();
    projectTaskReminderScheduler.resume();
    chatScheduledMessageScheduler.resume();
    notesProjectHistoryScheduler.resume();
    distractionsUsageScheduler.resume();
    quickNotesTrashPurge?.scheduler.resume();
    music.resumeSnapshotScheduler();
  }

  function disposeLifecycleSchedulers(): void {
    lifecycleSchedulersDisposed = true;
    eventNotificationScheduler.dispose();
    notesNotificationScheduler.dispose();
    projectTaskReminderScheduler.dispose();
    chatScheduledMessageScheduler.dispose();
    distractionsUsageScheduler.dispose();
    quickNotesTrashPurge?.dispose();
    quickNotesTrashPurge = null;
  }
</script>

<svelte:window onkeydowncapture={handleKeydown} />

<div
  class="app-shell h-screen w-screen"
  data-size-class={viewport.sizeClass}
>
  <div class="flex h-full flex-col overflow-hidden bg-sidebar">
    <TitleBar {shellStartupMs} {startupMemorySnapshot} {ensureBenchmarkOverlay} {splitArea} />
    <main class="content-panel flex-1 min-h-0 overflow-hidden">
      <WorkspaceSplitView
        panes={shownPanes}
        split={workspaceTabs.split}
        focusedTabId={workspaceTabs.activeTabId}
        onFocusPane={workspaceTabs.focusPane}
        onResize={(ratio) => workspaceTabs.resizeSplit(workspaceTabs.activeTabId, ratio)}
        bind:area={splitArea}
      >
        {#snippet content(pane)}
          {#snippet hostedProjectSidebar(onProjectSelected: () => void)}
            <ProjectsSidebar
              onSelectView={(view) => projectsIn(pane.scope).selectView(view)}
              {onProjectSelected}
            />
          {/snippet}
          {#if pane.view === "calendar"}
            <CalendarView />
          {:else if pane.view === "projects"}
            <ProjectsView
              viewComponents={projectViewComponents}
              projectChat={projectChatIntegration}
            />
          {:else if pane.view === "notes"}
            <NotesView projectSidebar={pane.hosted ? hostedProjectSidebar : undefined} />
          {:else}
            <ChatWorkspace projectSidebar={pane.hosted ? hostedProjectSidebar : undefined} />
          {/if}
        {/snippet}
      </WorkspaceSplitView>
    </main>
  </div>

  {#if suspendInfo}
    <ConfirmDialog
      title={t("focusDialog.resumeTitle")}
      message={t("focusDialog.awayMessage", formatAwayDuration(suspendInfo.awaySeconds))}
      confirmLabel={t("focusDialog.resume")}
      cancelLabel={t("focusDialog.stopSessionCancel")}
      danger={false}
      onConfirm={() => { void pomodoro.dismissSuspend(true); }}
      onCancel={() => { void pomodoro.dismissSuspend(false); }}
    />
  {/if}

  {#if idleInfo && !idleInfo.nativeOverlay && IdleOverlay}
    {@const Idle = IdleOverlay}
    <Idle
      idleSeconds={idleInfo.idleSeconds}
      nativeOverlay={idleInfo.nativeOverlay}
      focusFailed={idleInfo.focusFailed}
      onResume={() => pomodoro.dismissIdle(true)}
      onVisible={() => pomodoro.reportIdleVisibility()}
    />
  {/if}

  {#if isMainWindow}
    <UpdateNotificationToast />
  {/if}

  {#if BenchmarkOverlay}
    {@const Overlay = BenchmarkOverlay}
    <Overlay />
  {/if}

  {#if isMainWindow}
    {#await import("$lib/components/vault/VaultOwnershipPrompt.svelte") then module}
      {@const VaultOwnershipPrompt = module.default}
      <VaultOwnershipPrompt
        platform="desktop"
        closeConfirmationOpen={appClose.confirmationOpen}
        onRequestClose={() => appClose.request()}
      />
    {/await}
  {/if}

  <MusicPlaybackHost decodes={isMainWindow} />
  {#if isMainWindow}
    <MusicSoundscapeCoordinator />
  {/if}
  <TooltipHost />
  <WindowResizeHandles disabled={isMaximized || !!idleInfo} />
</div>
