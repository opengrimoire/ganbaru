<script lang="ts">
  import type {
    CalendarEvent, CalendarViewMode, EventSurfaceStatus, RecurringScope,
  } from "$lib/calendar/types";
  import {
    computeViewWindow, formatDatePart,
    getWeekDays, getWorkCycleDays,
    getLocalTimezone,
  } from "$lib/calendar/utils";
  import { useCalendar } from "$lib/stores/calendar.svelte";
  import { getCalendars } from "$lib/stores/calendars.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { calendarIdentityEmail } from "$lib/calendar/display";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { useCalendarZoom } from "$lib/stores/calendar-zoom.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import { getWorkspacePaneFocus } from "$lib/workspace-tabs/pane-focus";
  import { getWorkspacePaneScope } from "$lib/workspace-tabs/pane-scope";
  import {
    handlesCalendarMusicInspection,
    registerCalendarMusicInspectionTarget,
  } from "$lib/components/calendar/view/music-inspection";
  import { getNativeCalendarEditController } from "$lib/components/calendar/view/native-edit-controller.svelte";
  import type { CalendarEditDraft } from "$lib/components/calendar/view/native-edit-controller.svelte";
  import type { CalendarDeleteOutcome } from "$lib/api/calendar-edit";
  import { activeVaultIdentity } from "$lib/vault/active-vault";
  import CalendarX from "@lucide/svelte/icons/calendar-x";
  import Plus from "@lucide/svelte/icons/plus";
  import CalendarHeader from "./CalendarHeader.svelte";
  import { portal } from "$lib/utils/portal";
  import WeekView from "$lib/components/calendar/grid/WeekView.svelte";
  import DayView from "$lib/components/calendar/grid/DayView.svelte";
  import MonthView from "$lib/components/calendar/grid/MonthView.svelte";
  import ActionToast from "$lib/components/ui/ActionToast.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { createEditSession } from "./edit-session.svelte";
  import type { EditSessionState, PanelAnchor } from "./edit-session.svelte";
  import { mark as perfMark } from "$lib/stores/perf-log.svelte";
  import { hasOnlyShortcutModifier } from "$lib/keyboard-shortcuts";
  import { getCalendarNavHandle } from "$lib/calendar/nav-handle.svelte";
  import { PENDING_CREATE_ID } from "./display-events";
  import {
    activePomodoroSaveWouldStopSession,
    endActiveEventWouldStopProductivity,
    isActiveTimedCalendarEvent,
  } from "$lib/components/calendar/view/active-event-end";
  import { getCalendarEventEditLock } from "./event-edit-permissions";
  import {
    DEFAULT_DAY_HEADER_RETURN_MODE,
    type DayHeaderReturnMode,
  } from "$lib/components/calendar/view/view-navigation";
  import { createCalendarViewToastController } from "$lib/components/calendar/view/toasts.svelte";
  import { createCalendarViewModelBuilder, calendarViewModelDays } from "$lib/components/calendar/view/model";
  import { createCalendarViewConfirmationController } from "$lib/components/calendar/view/confirmation.svelte";
  import {
    createCalendarOutsideCloseAction,
    panelAnchorFromRect,
    panelAnchorFromRenderedEvent as panelAnchorFromRenderedEventElement,
  } from "$lib/components/calendar/view/panel-dom";
  import { createPersistedPomodoroSegmentsController } from "$lib/components/calendar/view/persisted-segments.svelte";
  import { createCalendarViewTargetController } from "$lib/components/calendar/view/target.svelte";
  import {
    CalendarViewPanelLifecycle,
  } from "$lib/components/calendar/view/panel-lifecycle.svelte";
  import {
    projectCalendarPanel,
    resolvePersistedCalendarPanelEvent,
    snapshotCalendarPanel,
    type PanelEditProjection,
    type PanelRenderState,
  } from "$lib/components/calendar/view/panel-projection";
  import { CalendarDragCommitController } from "$lib/components/calendar/view/drag-commit-controller";
  import {
    CalendarViewCommitService,
  } from "$lib/components/calendar/view/commit-service";
  import type { PanelSaveData } from "$lib/components/calendar/event-panel/payloads";
  import { CalendarViewDeleteController } from "$lib/components/calendar/view/delete-controller";
  import { CalendarViewNavigationController } from "$lib/components/calendar/view/navigation-controller";
  import { CalendarViewViewportController } from "$lib/components/calendar/view/viewport-controller.svelte";
  import { CalendarViewSaveController } from "$lib/components/calendar/view/save-controller.svelte";
  import {
    buildCalendarSaveFreeze,
    projectCalendarDisplay,
    projectCalendarSurfaceStatuses,
    visibleCalendarEvents,
  } from "$lib/components/calendar/view/display-projection";
  import {
    calendarSwipeAxis,
    calendarSwipeDirection,
    calendarSwipeNavigation,
    type CalendarSwipeAxis,
  } from "./mobile-gestures";

  const paneScope = getWorkspacePaneScope();
  const calendarStore = useCalendar();
  const calendarsStore = getCalendars();
  const projects = useProjects();
  const pomodoro = getPomodoro();
  const nativeEditor = getNativeCalendarEditController(() => pomodoro.vaultContext, "panel", paneScope);
  const directEditor = getNativeCalendarEditController(() => pomodoro.vaultContext, "direct", paneScope);
  const deleteEditor = getNativeCalendarEditController(() => pomodoro.vaultContext, "delete", paneScope);
  let viewAlive = true;
  const calendarZoom = useCalendarZoom();
  const theme = getTheme();
  const preferences = getPreferences();
  const mobileBackStack = getMobileBackStack();
  const { t } = getLocalization();
  const calendarBannerClass = "shrink-0 border-b border-border bg-popover px-3 py-2 text-sm";
  const toasts = createCalendarViewToastController();
  const confirm = createCalendarViewConfirmationController({
    defaultYesLabel: () => t("common.yes"),
    defaultNoLabel: () => t("common.no"),
  });
  const requestConfirm = confirm.requestConfirm;
  type CalendarCreateDefaultsInput = {
    start: string;
    end: string;
    allDay?: boolean;
  };
  type CalendarCreateDefaults = Partial<CalendarEvent>;

  const CREATE_CLOSE_GUARD_MS = 500;

  const panelLifecycle = new CalendarViewPanelLifecycle({
    importPanel: () => import("$lib/components/calendar/event-panel/EventPanel.svelte"),
    mark: perfMark,
    afterRender: (callback) => { void tick().then(callback); },
    afterPaint: (callback) => { requestAnimationFrame(callback); },
  });
  let {
    eventFilter,
    createDefaults,
    initialViewMode,
    onViewModeChange,
    mobileLayout = false,
    headerTarget = null,
  }: {
    eventFilter?: (event: CalendarEvent) => boolean;
    createDefaults?: (input: CalendarCreateDefaultsInput) => CalendarCreateDefaults;
    initialViewMode?: CalendarViewMode;
    onViewModeChange?: (mode: CalendarViewMode) => void;
    mobileLayout?: boolean;
    /** A host toolbar slot that shows the navigation and view controls in place of their own row. */
    headerTarget?: HTMLElement | null;
  } = $props();

  function closeSession() {
    const snapshot = snapshotCurrentPanel();
    panelLifecycle.park(snapshot);
    session.close();
  }

  function handlePanelSurfaceStatusChange(status: EventSurfaceStatus | undefined) {
    const editState = session.state;
    if (editState.mode === "create") {
      panelLifecycle.setSurfaceStatus(status, PENDING_CREATE_ID);
      return;
    }
    if (editState.mode === "edit") {
      panelLifecycle.setSurfaceStatus(status, editState.originalEvent.id);
      return;
    }
    panelLifecycle.setSurfaceStatus(undefined, undefined);
  }

  async function waitForFrames(count: number): Promise<void> {
    for (let i = 0; i < count; i++) {
      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => resolve());
      });
    }
  }

  const initialAnchorDate = new Date();

  function getInitialViewMode(): CalendarViewMode {
    return initialViewMode ?? (mobileLayout ? "day" : preferences.calendarViewMode);
  }

  let viewMode: CalendarViewMode = $state(getInitialViewMode());
  let dayHeaderReturnMode: DayHeaderReturnMode = $state(DEFAULT_DAY_HEADER_RETURN_MODE);
  let anchorDate: Date = $state(initialAnchorDate);
  const viewportController = new CalendarViewViewportController(getLocalTimezone);

  function getFocusIdleDefaults() {
    return {
      pauseWhenIdle: preferences.focusIdlePauseOnEventCreate,
      thresholdMinutes: preferences.focusIdleThresholdMinutes,
    };
  }

  const session = createEditSession(getFocusIdleDefaults);

  $effect(() => {
    if (session.state.mode === "create" || session.state.mode === "edit") {
      void panelLifecycle.load();
    }
  });

  // Display events are a pure overlay with no store mutation. While edits are
  // saving, preview computation is suppressed to prevent a flash: the store
  // updates before the session closes, so the preview would briefly conflict.
  let suppressEditPreview = $state(false);
  let panelCommitHidden = $state(false);
  let saveDisplayFreeze = $state<CalendarEvent[] | null>(null);

  // Visible viewport window. Navigation commits this target only after the
  // matching event and rail snapshots are ready.
  const viewWindow = $derived(computeViewWindow(anchorDate, viewMode));
  const multiDayRangeDays = $derived.by(() => {
    if (viewMode === "week") return getWeekDays(anchorDate);
    if (viewMode === "workweek") return getWorkCycleDays(anchorDate);
    return [];
  });

  // Keep the headless handle's cached view mode in sync so external drivers
  // (the benchmark harness) read the current value without polling. Only the primary pane drives the handle.
  $effect(() => {
    if (paneScope === "primary") getCalendarNavHandle().reportViewMode(viewMode);
  });

  function visibleStoreEventsForWindow(window: typeof viewWindow): CalendarEvent[] {
    return visibleCalendarEvents({
      events: calendarStore.eventsInWindow(window.start, window.end),
      visibleCalendarIds: calendarsStore.visibleIds,
      filter: eventFilter,
    });
  }

  const displayResult = $derived.by(() => {
    const projected = projectCalendarDisplay({
      storeEvents: calendarStore.eventsInWindow(viewWindow.start, viewWindow.end),
      frozenEvents: saveDisplayFreeze,
      state: session.state,
      createPreview: session.createPreview,
      changes: session.changes,
      dirty: session.dirty,
      nativePreview: currentNativePreview(),
      window: viewWindow,
      suppressEditPreview,
    });
    const events = visibleCalendarEvents({
      events: projected.events, visibleCalendarIds: calendarsStore.visibleIds, filter: eventFilter,
    });
    const visibleIds = new Set(events.map((event) => event.id));
    return { events, previewedIds: new Set([...projected.previewedIds].filter((id) => visibleIds.has(id))),
      editingId: projected.editingId && visibleIds.has(projected.editingId) ? projected.editingId : undefined };
  });

  function currentVisibleStoreEvents(): CalendarEvent[] {
    return visibleStoreEventsForWindow(viewWindow);
  }

  function currentNativePreview() {
    if (session.state.mode === "closed" || !pomodoro.vaultContext) return null;
    try {
      return nativeEditor.previewFor(commitService.editDraft(session.changes));
    } catch {
      // The preview effect exposes serialization errors while the selected card stays visible.
      return null;
    }
  }

  /** Present only the review for the current selection, scope and vault. */
  function currentDeletePreview() {
    if (session.state.mode !== "edit" || !pomodoro.vaultContext) return null;
    try {
      return deleteEditor.previewFor(currentDeleteDraft());
    } catch {
      // The deletion preview effect publishes any missing-provenance error.
      return null;
    }
  }

  /** Send the original native identity, independently of moved display dates. */
  function currentDeleteDraft(scope?: RecurringScope): CalendarEditDraft {
    const state = session.state;
    const context = pomodoro.vaultContext;
    if (state.mode !== "edit" || !context || !state.instanceEvent.recurrenceDate) {
      throw new Error("Calendar deletion requires a loaded native occurrence");
    }
    const renderZone = getLocalTimezone();
    return { ...context, sessionKey: state.sessionKey,
      edit: { kind: "delete", selection: { templateId: state.templateId,
        recurrenceDate: state.instanceEvent.recurrenceDate, scope: scope ?? session.scope }, stopActive: false },
      window: { windowStartDate: viewWindow.start.toString(), windowEndDate: viewWindow.end.toString(),
        renderZone, includeTotalEventCount: false } };
  }

  function buildSaveDisplayFreeze(): CalendarEvent[] {
    return buildCalendarSaveFreeze(displayResult.events);
  }

  const calendarIdentityById = $derived.by(() => {
    const identities = new Map<string, string>();
    for (const calendar of calendarsStore.list) {
      const identityEmail = calendarIdentityEmail(calendar);
      if (identityEmail) identities.set(calendar.id, identityEmail);
    }
    return identities;
  });

  const visibleEvents = $derived(projectCalendarSurfaceStatuses({
    events: displayResult.events,
    identityByCalendarId: calendarIdentityById,
    pendingStatus: panelLifecycle.surfaceStatus,
    pendingEventId: panelLifecycle.surfaceStatusEventId,
  }));
  const persistedSegments = createPersistedPomodoroSegmentsController({
    getSegmentVersion: () => pomodoro.segmentVersion,
    visibleStoreEventsForWindow,
  });
  const targetController = createCalendarViewTargetController({
    calendarStore,
    getAnchorDate: () => anchorDate,
    getDayHeaderReturnMode: () => dayHeaderReturnMode,
    getViewMode: () => viewMode,
    getViewWindow: () => viewWindow,
    getVisibleEventCount: () => visibleEvents.length,
    persistedSegments,
    setAnchorDate: (date) => {
      anchorDate = date;
    },
    setDayHeaderReturnMode: (mode) => {
      dayHeaderReturnMode = mode;
    },
    setPreferredViewMode: (mode) => {
      if (onViewModeChange) onViewModeChange(mode);
      else if (paneScope === "primary") preferences.setCalendarViewMode(mode);
    },
    setViewMode: (mode) => {
      viewMode = mode;
    },
  });

  $effect(() => {
    if (!calendarStore.loaded) return;
    if (!calendarStore.hasWindow(viewWindow.start, viewWindow.end)) return;
    void pomodoro.segmentVersion;
    void visibleEvents;
    persistedSegments.refreshForTarget({ mode: viewMode, date: new Date(anchorDate) });
  });
  let lastSameAnchorPrefetchKey = "";
  $effect(() => {
    if (!calendarStore.loaded) return;
    if (!calendarStore.hasWindow(viewWindow.start, viewWindow.end)) return;
    const key = `${viewMode}:${formatDatePart(anchorDate)}`;
    if (key === lastSameAnchorPrefetchKey) return;
    lastSameAnchorPrefetchKey = key;
    targetController.prefetchSameAnchor({ mode: viewMode, date: anchorDate });
  });
  let bootUsablePaintMarked = false;

  $effect(() => {
    if (bootUsablePaintMarked || !calendarStore.loaded) return;
    void visibleEvents.length;
    tick().then(() => {
      requestAnimationFrame(() => {
        if (bootUsablePaintMarked || !calendarStore.loaded) return;
        bootUsablePaintMarked = true;
        perfMark("boot.usable-paint", {
          count: visibleEvents.length,
          events: calendarStore.eventCount,
        });
      });
    });
  });

  const calendarViewModelBuilder = createCalendarViewModelBuilder();
  const calendarViewModel = $derived.by(() => calendarViewModelBuilder.build({
    key: {
      storeVersion: calendarStore.indexVersion,
      windowStart: viewWindow.start.toString(),
      windowEnd: viewWindow.end.toString(),
      timezone: getLocalTimezone(),
      mode: viewMode,
    },
    events: visibleEvents,
    visibleDays: calendarViewModelDays(viewMode, anchorDate, multiDayRangeDays),
  }));
  const eventsByDay = $derived(calendarViewModel.eventsByDay);

  let suppressEditingGlow = $state(false);
  const previewedIds = $derived(suppressEditingGlow ? new Set<string>() : displayResult.previewedIds);
  const editingId = $derived(suppressEditingGlow ? undefined : displayResult.editingId);
  const visualEditingId = $derived(
    suppressEditingGlow ? undefined : (editingId ?? panelLifecycle.pendingEditEventId),
  );

  const dragController = new CalendarDragCommitController({
    session,
    isCommitHidden: () => panelCommitHidden || saveController.saving || directEditor.committing,
    editingId: () => editingId,
    visibleEvents: () => visibleEvents,
    isRecurring,
    isActivePomodoroEvent: (event) => !isRecurring(event)
      && pomodoro.isActive
      && pomodoro.activeOccurrenceId === event.id,
    panelAnchor: panelAnchorFromRenderedEvent,
    loadPanel: () => panelLifecycle.load(),
    confirmDiscard: (action) => {
      requestConfirm(t("calendar.view.changesLost"), action, {
        title: t("calendar.view.discardUnsavedTitle"),
        yesLabel: t("calendar.view.discard"),
        noLabel: t("common.cancel"),
      });
    },
    getTemplate: calendarStore.getTemplate,
    updateEvent: persistDirectCalendarEdit,
  });
  const commitService = new CalendarViewCommitService({
    calendarStore,
    nativeEditor,
    directEditor,
    getSessionState: () => session.state,
    getBaseline: () => session.baseline,
    getScope: () => session.scope,
    getViewWindow: () => viewWindow,
    getContext: () => pomodoro.vaultContext,
    getRenderZone: getLocalTimezone,
  });
  $effect(() => {
    const state = session.state;
    const context = pomodoro.vaultContext;
    if (state.mode === "closed" || !context) {
      untrack(() => nativeEditor.update(null));
      return;
    }
    try {
      const draft = commitService.editDraft(session.changes);
      untrack(() => nativeEditor.update(draft));
    } catch (error) {
      untrack(() => {
        nativeEditor.update(null);
        nativeEditor.error = saveErrorMessage(error);
      });
    }
  });

  async function persistDirectCalendarEdit(event: CalendarEvent): Promise<void> {
    const toastId = toasts.showSavePendingToast(t("calendar.view.saving"));
    try {
      const original = calendarStore.eventsInWindow(viewWindow.start, viewWindow.end)
        .find((candidate) => candidate.id === event.id);
      if (!original) throw new Error("Calendar occurrence is no longer in the visible snapshot");
      await commitService.persistDirect(event, original);
      await calendarStore.refreshWindow(viewWindow.start, viewWindow.end);
      toasts.showSaveSuccessToast(toastId, t("calendar.view.saved"));
    } catch (error) {
      logPanelSaveError("panel-save", error, { ...event, description: event.description ?? "" });
      toasts.showSaveErrorToast(toastId, t("calendar.view.saveFailed", saveErrorMessage(error)));
    }
  }

  async function retryCalendarSave(): Promise<void> {
    const editor = nativeEditor.uncertain ? nativeEditor : directEditor;
    const state = session.state;
    const retryAction = editor.pendingAction;
    let closingEdit: string | null = null;
    let closeAfterRetry = false;
    if (state.mode !== "closed" && editor === nativeEditor) {
      try {
        const draft = commitService.editDraft(session.changes, undefined, retryAction);
        closeAfterRetry = editor.matchesPending(draft);
        closingEdit = JSON.stringify(draft.edit);
      } catch (error) {
        console.warn("Calendar retry will preserve the selected draft", error);
      }
    }
    const toastId = toasts.showSavePendingToast(t("calendar.view.saving"));
    try {
      await editor.retryPending();
      calendarStore.acceptNativeEdit();
      await calendarStore.refreshWindow(viewWindow.start, viewWindow.end);
      if (closeAfterRetry && state.mode !== "closed" && session.state.mode !== "closed"
        && session.state.sessionKey === state.sessionKey) {
        try {
          if (JSON.stringify(commitService.editDraft(session.changes, undefined, retryAction).edit) === closingEdit) closeSession();
        } catch (error) {
          console.warn("Calendar retry preserved a changed draft", error);
        }
      }
      toasts.showSaveSuccessToast(toastId, t("calendar.view.saved"));
    } catch (error) {
      console.error("Calendar Save retry failed", error);
      toasts.showSaveErrorToast(toastId, t("calendar.view.saveFailed", saveErrorMessage(error)));
    }
  }

  async function retryCalendarPreview(): Promise<void> {
    if (session.state.mode === "closed") return;
    try {
      await nativeEditor.review(commitService.editDraft(session.changes));
    } catch (error) {
      console.error("Calendar preview retry failed", error);
    }
  }

  const saveController = new CalendarViewSaveController({
    toasts,
    commitService,
    getSessionState: () => session.state,
    canEnablePomodoro: canEnablePomodoroForActiveCalendarEvent,
    isSelectedEndable: isSelectedEndableActiveOccurrence,
    wouldSaveStopSession,
    endWouldStopProductivity: selectedActiveEndWouldStopProductivity,
    confirmSaveStop: (action) => {
      requestConfirm(t("calendar.view.stopCurrentFocusSession"), action, {
        title: t("calendar.view.saveAndStopTitle"),
        yesLabel: t("calendar.view.stopAndSave"),
        noLabel: t("calendar.view.keepEditing"),
      });
    },
    confirmEndStop: (action) => {
      requestConfirm(t("calendar.view.stopCurrentFocusSession"), action, {
        title: t("calendar.view.stopFocusSessionTitle"),
        yesLabel: t("calendar.view.endEventConfirm"),
        noLabel: t("calendar.view.keepSession"),
        extraConfirmShortcut: (event) =>
          (event.key === "d" || event.key === "D") && hasOnlyShortcutModifier(event),
      });
    },
    buildFreeze: buildSaveDisplayFreeze,
    setDisplayState: ({ suppressGlow, suppressPreview, frozenEvents }) => {
      suppressEditingGlow = suppressGlow;
      suppressEditPreview = suppressPreview;
      saveDisplayFreeze = frozenEvents;
    },
    refreshWindow: () => calendarStore.refreshWindow(viewWindow.start, viewWindow.end),
    closeSession,
    afterRender: tick,
    savePendingLabel: () => t("calendar.view.saving"),
    saveSuccessLabel: () => t("calendar.view.saved"),
    saveErrorLabel: (error) => t("calendar.view.saveFailed", saveErrorMessage(error)),
    logError: logPanelSaveError,
  });
  const outsideClose = createCalendarOutsideCloseAction({
    closeGuardMs: CREATE_CLOSE_GUARD_MS,
    getConfirmOpen: () => !!confirm.action,
    getLastDragEndTime: () => dragController.lastDragEndTime,
    isPanelCommitHidden: () => panelCommitHidden,
    isSessionClosed: () => session.state.mode === "closed",
    onClose: handlePanelClose,
  });

  // Merged event for the panel (original + changes, so panel sees drag/resize updates)
  const panelEvent = $derived.by(() => {
    const editState = session.state;
    if (editState.mode === "edit") {
      return { ...editState.originalEvent, ...session.changes } as CalendarEvent;
    }
    return undefined;
  });

  const panelDetailsLoaded = $derived(
    session.state.mode === "edit" ? session.state.detailsLoaded : false,
  );

  // Whether clicking delete for the current event+scope would stop the
  // active pomodoro session (modal will appear). When true, the panel
  // skips its inline two-step confirmation since the modal already acts
  // as a strong confirmation step.
  const deleteWouldStopSession = $derived.by(() => {
    if (session.state.mode !== "edit") return false;
    try {
      return deleteEditor.previewFor(currentDeleteDraft())?.deletion?.requiresActiveStop ?? false;
    } catch {
      return false;
    }
  });

  const selectedEditLock = $derived.by(() => {
    const editState = session.state;
    if (editState.mode !== "edit") return { locked: false, allowArchive: false };
    return getCalendarEventEditLock(editState.instanceEvent, calendarsStore.list, {
      isActivePomodoroEvent: isSelectedActivePomodoroOccurrence(editState),
    });
  });

  function currentPanelEditProjection(): PanelEditProjection | undefined {
    const editState = session.state;
    if (editState.mode !== "edit") return undefined;
    return {
      selectedActive: isSelectedEndableActiveOccurrence(editState),
      recurring: isRecurring(editState.originalEvent),
      detailsLoaded: panelDetailsLoaded,
      locked: selectedEditLock.locked,
      allowArchive: selectedEditLock.allowArchive,
      allowPomodoroWhenReadOnly: canEnablePomodoroForActiveCalendarEvent(editState),
      deleteWouldStopSession,
      endWouldStopProductivity: selectedActiveEndWouldStopProductivity(editState),
    };
  }

  function snapshotCurrentPanel() {
    return snapshotCalendarPanel({
      state: session.state,
      changes: session.changes,
      panelEvent,
      edit: currentPanelEditProjection(),
    });
  }

  const panelRender = $derived.by<PanelRenderState | null>(() => {
    return projectCalendarPanel({
      hidden: panelCommitHidden,
      state: session.state,
      changes: session.changes,
      dirty: session.dirty,
      panelEvent,
      parked: panelLifecycle.parkedSnapshot,
      endingActiveEvent: saveController.endingActiveEvent,
      edit: currentPanelEditProjection(),
    });
  });

  const panelCalendarIdentityEmail = $derived.by(() => {
    if (!panelRender || panelRender.mode !== "edit") return undefined;
    return calendarIdentityEmail(
      calendarsStore.list.find((calendar) => calendar.id === panelRender.event.calendarId),
    );
  });
  let musicInspectionEventId = $state<string | null>(null);

  function isRecurring(event: CalendarEvent): boolean {
    return !!event.recurringParentId || !!event.recurrence;
  }

  function isLocalWritableEvent(event: CalendarEvent): boolean {
    const calendar = calendarsStore.list.find((item) => item.id === event.calendarId);
    return calendar?.source === "local" && calendar.readOnly !== true;
  }

  function isSelectedActivePomodoroOccurrence(editState: Extract<EditSessionState, { mode: "edit" }>): boolean {
    return pomodoro.isActive && editState.instanceEvent.id === pomodoro.activeOccurrenceId;
  }

  function isSelectedActiveCalendarOccurrence(editState: Extract<EditSessionState, { mode: "edit" }>): boolean {
    return isLocalWritableEvent(editState.originalEvent) && isActiveTimedCalendarEvent(editState.instanceEvent);
  }

  function isSelectedEndableActiveOccurrence(editState: Extract<EditSessionState, { mode: "edit" }>): boolean {
    return isSelectedActivePomodoroOccurrence(editState) || isSelectedActiveCalendarOccurrence(editState);
  }

  function canEnablePomodoroForActiveCalendarEvent(
    editState: Extract<EditSessionState, { mode: "edit" }>,
  ): boolean {
    return !pomodoro.isActive
      && !isRecurring(editState.originalEvent)
      && !editState.instanceEvent.pomodoroConfig
      && isSelectedActiveCalendarOccurrence(editState);
  }

  function selectedActiveEndWouldStopProductivity(
    editState: Extract<EditSessionState, { mode: "edit" }>,
  ): boolean {
    if (!isSelectedActivePomodoroOccurrence(editState)) return false;
    return endActiveEventWouldStopProductivity(
      editState.instanceEvent,
      currentVisibleStoreEvents(),
      new Date(),
    );
  }

  function effectiveRecurringScope(
    editState: Extract<EditSessionState, { mode: "edit" }>,
    requested?: RecurringScope,
  ): RecurringScope {
    return isSelectedEndableActiveOccurrence(editState) ? "this" : requested ?? session.scope;
  }

  /** Whether this save removes the active pomodoro or moves it out of the current time window. */
  function wouldSaveStopSession(data: PanelSaveData, _scope?: RecurringScope): boolean {
    if (!pomodoro.isActive || !pomodoro.activeOccurrenceId || session.state.mode !== "edit") return false;

    const editState = session.state;
    // Active recurring occurrences are isolated to this occurrence. Future
    // "following" or "all" edits in the same series do not move the active run.
    if (!isSelectedActivePomodoroOccurrence(editState)) return false;
    return activePomodoroSaveWouldStopSession(data);
  }

  function saveErrorMessage(error: unknown): string {
    if (error instanceof Error && error.message.trim()) return error.message;
    if (typeof error === "string" && error.trim()) return error;
    return t("calendar.view.unknownSaveError");
  }

  function logPanelSaveError(
    context: "enable-active-pomodoro" | "panel-save",
    error: unknown,
    data: PanelSaveData,
    scope?: RecurringScope,
  ): void {
    const editState = session.state;
    console.error("[CalendarView] event save failed", {
      context,
      mode: editState.mode,
      eventId: editState.mode === "edit" ? editState.originalEvent.id : undefined,
      scope,
      keys: Object.keys(data).sort(),
      hasPomodoroConfig: !!data.pomodoroConfig,
      allDay: !!data.allDay,
      error,
    });
  }

  let mobileSwipeSuppressClick = false;
  let mobileSwipeState = $state<{
    pointerId: number;
    startX: number;
    startY: number;
    startedAt: number;
    lastX: number;
    lastY: number;
    lastAt: number;
    axis: CalendarSwipeAxis;
  } | null>(null);

  function removeMobileSwipeListeners(): void {
    window.removeEventListener("pointermove", handleMobileSwipeMove);
    window.removeEventListener("pointerup", handleMobileSwipeEnd);
    window.removeEventListener("pointercancel", handleMobileSwipeCancel);
  }

  function resetMobileSwipe(): void {
    removeMobileSwipeListeners();
    mobileSwipeState = null;
  }

  function completeMobileSwipeNavigation(direction: "back" | "forward"): void {
    removeMobileSwipeListeners();
    mobileSwipeState = null;
    navigate(direction, "touch");
  }

  function handleMobileSwipeStart(event: PointerEvent): void {
    if (!mobileLayout || event.pointerType !== "touch" || !event.isPrimary || event.button !== 0
      || session.state.mode !== "closed" || confirm.action) return;
    mobileSwipeState = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      startedAt: performance.now(),
      lastX: event.clientX,
      lastY: event.clientY,
      lastAt: performance.now(),
      axis: null,
    };
    window.addEventListener("pointermove", handleMobileSwipeMove);
    window.addEventListener("pointerup", handleMobileSwipeEnd);
    window.addEventListener("pointercancel", handleMobileSwipeCancel);
  }

  function handleMobileSwipeMove(event: PointerEvent): void {
    const state = mobileSwipeState;
    if (!state || event.pointerId !== state.pointerId) return;
    state.lastX = event.clientX;
    state.lastY = event.clientY;
    state.lastAt = performance.now();
    const deltaX = event.clientX - state.startX;
    const deltaY = event.clientY - state.startY;
    if (state.axis === null) {
      const axis = calendarSwipeAxis(deltaX, deltaY);
      if (axis === null) return;
      if (axis === "vertical") {
        resetMobileSwipe();
        return;
      }
      state.axis = axis;
    }
    event.preventDefault();
  }

  function finishMobileSwipe(state: NonNullable<typeof mobileSwipeState>): void {
    const direction = state.axis === "horizontal"
      ? calendarSwipeNavigation({
        deltaX: state.lastX - state.startX,
        elapsedMs: state.lastAt - state.startedAt,
        viewportWidth: viewportController.viewWrapper?.clientWidth ?? window.innerWidth,
      })
      : null;
    if (state.axis === "horizontal") mobileSwipeSuppressClick = true;
    if (direction) completeMobileSwipeNavigation(direction);
    else resetMobileSwipe();
  }

  function handleMobileSwipeEnd(event: PointerEvent): void {
    const state = mobileSwipeState;
    if (!state || event.pointerId !== state.pointerId) return;
    finishMobileSwipe(state);
  }

  function handleMobileSwipeCancel(event: PointerEvent): void {
    const state = mobileSwipeState;
    if (!state || state.pointerId !== event.pointerId) return;
    if (state.axis !== "horizontal") {
      resetMobileSwipe();
      return;
    }
    const direction = calendarSwipeDirection(state.lastX - state.startX);
    mobileSwipeSuppressClick = true;
    if (direction) completeMobileSwipeNavigation(direction);
    else resetMobileSwipe();
  }

  function handleMobileSwipeClick(event: MouseEvent): void {
    if (!mobileSwipeSuppressClick) return;
    mobileSwipeSuppressClick = false;
    event.preventDefault();
    event.stopImmediatePropagation();
  }

  function handleMobileContextMenu(event: MouseEvent): void {
    if (mobileLayout) event.preventDefault();
  }

  function handleMobileTouchEditStart(): void {
    resetMobileSwipe();
  }

  function handleMobileTouchEditEnd(): void {
    mobileSwipeSuppressClick = true;
  }

  onDestroy(() => {
    viewAlive = false;
    resetMobileSwipe();
    nativeEditor.reset();
    directEditor.reset();
    deleteEditor.reset();
    void deleteController.dismissUndo();
    toasts.destroy();
  });

  let containerEl: HTMLDivElement | undefined = $state();

  const paneFocused = getWorkspacePaneFocus();
  const navigationController = new CalendarViewNavigationController({
    getViewMode: () => viewMode,
    isFocused: paneFocused,
    getSessionClosed: () => session.state.mode === "closed",
    getConfirmOpen: () => !!confirm.action,
    getViewWrapper: () => viewportController.viewWrapper,
    getVisibleEventCount: () => visibleEvents.length,
    closePanel: handlePanelClose,
    navigate: (direction, source) => navigate(direction, source),
    canRepeat: targetController.canRepeatHeldNavigation,
    waitForSettled: targetController.waitForSettled,
    mark: perfMark,
  });

  $effect(() => {
    if (!mobileLayout || session.state.mode === "closed") return;
    return mobileBackStack.activate({
      handle: handlePanelClose,
    });
  });

  onMount(() => {
    const removeNavigationListeners = navigationController.installWindowListeners();
    void calendarStore.loadInitialWindow(viewWindow.start, viewWindow.end).catch((error: unknown) => {
      console.error("[calendar] initial window load failed", error);
    });
    // A Calendar tab inspects music assignments, one per split; a calendar scoped to a project leaves them to it.
    const musicInspectionTarget = { scope: paneScope, isFocused: paneFocused };
    const unregisterMusicInspection = eventFilter
      ? () => {}
      : registerCalendarMusicInspectionTarget(musicInspectionTarget);
    const inspectMusicAssignment = (event: Event) => {
      if (eventFilter || !handlesCalendarMusicInspection(musicInspectionTarget)) return;
      if (!(event instanceof CustomEvent) || typeof event.detail?.eventId !== "string") return;
      const eventId = event.detail.eventId;
      const selected = currentVisibleStoreEvents().find((candidate) => candidate.id === eventId)
        ?? calendarStore.sourceEvents.find((candidate) => candidate.id === eventId);
      if (!selected) return;
      musicInspectionEventId = selected.id;
      void handleEventClick(selected, new DOMRect(window.innerWidth / 2, window.innerHeight / 3, 0, 0));
    };
    window.addEventListener("ganbaru:inspect-music-assignment", inspectMusicAssignment);

    tick().then(() => {
      requestAnimationFrame(() => {
        perfMark("boot.first-paint", { count: visibleEvents.length });
      });
    });

    // Expose navigate / view-mode setters through the headless handle so the
    // benchmark harness (and any future scripted driver) can drive the
    // calendar without reaching into component internals. Only the primary pane registers it.
    const unregisterNav = paneScope !== "primary" ? () => {} : navigationController.registerBenchmark({
      navigate,
      setViewMode: (mode) => changeView(mode),
      setAnchorDate: (date) => {
        void targetController.request({ mode: targetController.currentState().mode, date }, {
          history: false,
          reason: "programmatic",
        });
      },
      openVisibleEvent: openVisibleEventForBenchmark,
      getVisibleEventCount: getVisibleEventCountForBenchmark,
      openCreatePanel: openCreatePanelForBenchmark,
      closePanel: closePanelForBenchmark,
      canRepeatHeldNavigation: targetController.canRepeatHeldNavigation,
      getViewMode: () => viewMode,
    });

    return () => {
      unregisterNav();
      unregisterMusicInspection();
      removeNavigationListeners();
      window.removeEventListener("ganbaru:inspect-music-assignment", inspectMusicAssignment);
      targetController.clearPending();
    };
  });

  function navigate(
    direction: "today" | "back" | "forward",
    source: "programmatic" | "wheel" | "key" | "hold-repeat" | "touch" = "programmatic",
  ) {
    perfMark("nav.start", { dir: direction, source });
    if (direction === "today") {
      void targetController.request(
        { mode: targetController.currentState().mode, date: new Date() },
        { history: false, reason: "nav" },
      );
      return;
    }

    void targetController.request(
      {
        mode: targetController.currentState().mode,
        date: targetController.targetAnchorForNavigation(direction),
      },
      { history: false, reason: "nav" },
    );
  }

  function handleWheelNavigate(direction: "back" | "forward") {
    navigate(direction, "wheel");
  }

  function changeView(mode: CalendarViewMode) {
    perfMark("view.start", { from: viewMode, to: mode });
    void targetController.request(
      { mode, date: targetController.currentState().date },
      { history: true, reason: "view" },
    );
  }

  async function handleEventCreate(start: string, end: string, allDay?: boolean, createAnchor?: PanelAnchor) {
    if (panelCommitHidden) return;

    const openCreate = async () => {
      const initialData = createDefaults?.({ start, end, allDay }) ?? {};
      const initialStart = initialData.start ?? start;
      const initialEnd = initialData.end ?? end;
      const initialAllDay = initialData.allDay ?? allDay;
      panelLifecycle.pendingEditEventId = undefined;
      // Track that a create operation ended (prevents click-to-close)
      dragController.markInteractionEnd();

      const anchor: PanelAnchor = createAnchor
        ?? { x: window.innerWidth / 2, y: window.innerHeight / 3, width: 0, height: 0 };

      const panelState = session.state.mode === "closed"
        ? panelLifecycle.parkedSnapshot ? "unpark" : "open"
        : "switch";
      const requestId = panelLifecycle.beginOpen("create", panelState);
      try {
        const panelReady = panelLifecycle.ensureReady(requestId);
        session.openCreate(initialStart, initialEnd, anchor, initialAllDay, initialData);
        panelLifecycle.markStateOpen(requestId);
        if (panelReady) await panelReady;
        if (!panelLifecycle.isCurrent(requestId)) return;
        panelLifecycle.markPaintDone(requestId);
      } catch (e) {
        if (panelLifecycle.recoverFailedOpen(requestId)) session.close();
        console.error("[CalendarView] open create panel failed:", e);
      }
    };

    if (session.dirty) {
      requestConfirm(
        t("calendar.view.changesLost"),
        openCreate,
        {
          title: t("calendar.view.discardUnsavedTitle"),
          yesLabel: t("calendar.view.discard"),
          noLabel: t("common.cancel"),
        },
      );
      return;
    }

    await openCreate();
  }

  function formatMinuteOfDay(minute: number): string {
    const hour = Math.floor(minute / 60);
    const minutePart = minute % 60;
    return `${String(hour).padStart(2, "0")}:${String(minutePart).padStart(2, "0")}`;
  }

  function openMobileEventCreate(target: HTMLButtonElement): void {
    const today = new Date();
    const isAnchorToday = formatDatePart(anchorDate) === formatDatePart(today);
    const start = new Date(anchorDate);
    start.setHours(0, 0, 0, 0);
    const startMinute = isAnchorToday
      ? Math.ceil((today.getHours() * 60 + today.getMinutes()) / 30) * 30
      : 9 * 60;
    start.setMinutes(startMinute);
    const end = new Date(start.getTime() + 30 * 60 * 1000);
    const rect = target.getBoundingClientRect();
    void handleEventCreate(
      `${formatDatePart(start)} ${formatMinuteOfDay(start.getHours() * 60 + start.getMinutes())}`,
      `${formatDatePart(end)} ${formatMinuteOfDay(end.getHours() * 60 + end.getMinutes())}`,
      false,
      { x: rect.left, y: rect.top, width: rect.width, height: rect.height },
    );
  }

  function panelAnchorFromRenderedEvent(eventId: string): PanelAnchor {
    return panelAnchorFromRenderedEventElement(containerEl, eventId);
  }

  async function handleEventClick(event: CalendarEvent, rect?: DOMRect): Promise<void> {
    if (panelCommitHidden) return;
    if (event.id === PENDING_CREATE_ID || event.id.startsWith(PENDING_CREATE_ID + "::")) return;

    // Already editing this exact event. A clean panel is just a peek and can
    // be toggled closed. Dirty edits stay open so the event click does not
    // trigger discard confirmation.
    if (session.state.mode === "edit" && (
      session.state.originalEvent.id === event.id || editingId === event.id
    )) {
      if (!session.dirty) handlePanelClose();
      return;
    }

    const persistedEvent = resolvePersistedCalendarPanelEvent(
      event.id,
      calendarStore.eventsInWindow(viewWindow.start, viewWindow.end),
    );
    if (!persistedEvent) return;
    const anchor = panelAnchorFromRect(rect);

    const openEvent = async () => {
      panelLifecycle.pendingEditEventId = event.id;
      const panelState = session.state.mode === "closed"
        ? panelLifecycle.parkedSnapshot ? "unpark" : "open"
        : "switch";
      const requestId = panelLifecycle.beginOpen("edit", panelState);
      try {
        const lookupId = persistedEvent.recurringParentId ?? persistedEvent.id;
        const [fullEvent] = await Promise.all([
          calendarStore.loadPanelEvent(lookupId).then((full) => {
            panelLifecycle.markDetailsReady(requestId, !!full);
            return full;
          }),
          panelLifecycle.ensureReady(requestId) ?? Promise.resolve(),
        ]);
        if (!panelLifecycle.isCurrent(requestId)) return;
        const hydratedEvent = resolvePersistedCalendarPanelEvent(
          event.id,
          calendarStore.eventsInWindow(viewWindow.start, viewWindow.end),
          fullEvent,
        );
        if (!hydratedEvent) {
          panelLifecycle.recoverFailedOpen(requestId);
          return;
        }
        if (isRecurring(hydratedEvent)) {
          session.openEdit(hydratedEvent, anchor, hydratedEvent, !!fullEvent);
        } else {
          session.openEdit(hydratedEvent, anchor, undefined, !!fullEvent);
        }
        panelLifecycle.markStateOpen(requestId);
        panelLifecycle.markPaintDone(requestId);
      } catch (e) {
        panelLifecycle.recoverFailedOpen(requestId);
        console.error("[CalendarView] open event failed:", e);
      } finally {
        if (panelLifecycle.isCurrent(requestId)) panelLifecycle.pendingEditEventId = undefined;
      }
    };

    if (session.dirty) {
      requestConfirm(
        t("calendar.view.changesLost"),
        async () => { await openEvent(); },
        {
          title: t("calendar.view.discardUnsavedTitle"),
          yesLabel: t("calendar.view.discard"),
          noLabel: t("common.cancel"),
        },
      );
      return;
    }

    await openEvent();
  }

  function handleEventPrefetch(event: CalendarEvent) {
    if (event.id === PENDING_CREATE_ID || event.id.startsWith(PENDING_CREATE_ID + "::")) return;
    const persistedEvent = resolvePersistedCalendarPanelEvent(
      event.id,
      calendarStore.eventsInWindow(viewWindow.start, viewWindow.end),
    );
    if (persistedEvent) {
      calendarStore.prefetchPanelEvent(persistedEvent.recurringParentId ?? persistedEvent.id);
    }
  }

  async function openVisibleEventForBenchmark(index: number): Promise<boolean> {
    const event = visibleEvents.filter((item) => !item.id.startsWith(PENDING_CREATE_ID))[index];
    if (!event) return false;
    await handleEventClick(event);
    await tick();
    await waitForFrames(2);
    return true;
  }

  function getVisibleEventCountForBenchmark(): number {
    return visibleEvents.filter((item) => !item.id.startsWith(PENDING_CREATE_ID)).length;
  }

  async function openCreatePanelForBenchmark(
    start: string,
    end: string,
    allDay?: boolean,
  ): Promise<boolean> {
    await handleEventCreate(start, end, allDay);
    await tick();
    await waitForFrames(2);
    return true;
  }

  async function closePanelForBenchmark(): Promise<void> {
    handlePanelClose();
    await tick();
    await waitForFrames(2);
  }

  const handleEventUpdate = (event: CalendarEvent): Promise<void> => dragController.handle(event);

  function handlePanelChange(data: Partial<CalendarEvent>) {
    session.updateChanges(data);
  }

  function handlePanelInitialSync(data: Partial<CalendarEvent>) {
    session.setInitialChanges(data);
  }

  function handleScopeChange(newScope: RecurringScope) {
    session.updateScope(newScope);
  }

  function handlePanelClose() {
    if (panelCommitHidden) return;
    panelLifecycle.invalidate();
    if (session.dirty) {
      requestConfirm(
        t("calendar.view.changesLost"),
        async () => { closeSession(); },
        {
          title: t("calendar.view.discardUnsavedTitle"),
          yesLabel: t("calendar.view.discard"),
          noLabel: t("common.cancel"),
        },
      );
      return;
    }
    closeSession();
  }

  const handlePanelSave = (data: PanelSaveData, scope?: RecurringScope): Promise<boolean> =>
    saveController.save(data, scope);
  const handleEndEvent = (data: PanelSaveData, scope?: RecurringScope): Promise<void> =>
    saveController.end(data, scope);

  function eventDeleteOutcomeLabel(outcome: CalendarDeleteOutcome): string {
    if (outcome === "archive") return t("calendar.view.eventArchived");
    if (outcome === "mixed") return t("calendar.view.eventsDeletedAndArchived");
    return t("calendar.view.eventDeleted");
  }

  function eventDeletePendingLabel(outcome: CalendarDeleteOutcome): string {
    if (outcome === "archive") return t("calendar.view.archiving");
    if (outcome === "mixed") return t("calendar.view.deletingAndArchiving");
    return t("calendar.view.deleting");
  }

  const deleteController = new CalendarViewDeleteController({
    calendarStore,
    nativeEditor: deleteEditor,
    toasts,
    getWindow: () => viewWindow,
    getEvents: currentVisibleStoreEvents,
    canPresent: (review) => viewAlive && (!review || activeVaultIdentity() === review.vaultId),
    isCurrent: (input) => {
      if (!viewAlive || session.state.mode !== "edit" || session.state.sessionKey !== input.sessionKey) return false;
      try {
        const current = currentDeleteDraft();
        return current.vaultId === input.vaultId && current.vaultGeneration === input.vaultGeneration
          && JSON.stringify(current.edit) === JSON.stringify(input.edit);
      } catch { return false; }
    },
    setCommitState: ({ hidden, suppressPreview, frozenEvents }) => {
      panelCommitHidden = hidden;
      suppressEditPreview = suppressPreview;
      saveDisplayFreeze = frozenEvents;
    },
    closeSession,
    pendingLabel: eventDeletePendingLabel,
    outcomeLabel: eventDeleteOutcomeLabel,
    errorLabel: (error) => t("calendar.view.deleteFailed", saveErrorMessage(error)),
  });
  const undoDeletedEvent = (): Promise<void> => deleteController.undoCurrent();


  function reportDeleteError(error: unknown): void {
    const label = t("calendar.view.deleteFailed", saveErrorMessage(error));
    const toastId = toasts.showSavePendingToast(label);
    toasts.showSaveErrorToast(toastId, label);
  }

  async function handleDelete(id: string, scope?: RecurringScope): Promise<void> {
    try {
      if (session.state.mode !== "edit" || session.state.instanceEvent.id !== id) throw new Error("Calendar deletion selection changed");
      const input = currentDeleteDraft(scope);
      const review = await deleteEditor.reviewDeletion(input);
      if (!review.deletion) throw new Error("Native Calendar deletion review is unavailable");
      const outcome = review.deletion.outcome;
      if (review.deletion.requiresActiveStop) {
        requestConfirm(
          outcome === "archive"
            ? t("calendar.view.stopBeforeArchived")
            : t("calendar.view.stopBeforeDeleted"),
          async () => {
            try { await deleteController.execute(input, review, true); }
            catch (error) { reportDeleteError(error); }
          },
          {
            title: outcome === "archive"
              ? t("calendar.view.stopAndArchiveEventTitle")
              : t("calendar.view.stopAndDeleteEventTitle"),
            yesLabel: outcome === "archive"
              ? t("calendar.view.stopAndArchive")
              : t("calendar.view.stopAndDelete"),
            noLabel: t("calendar.view.keepSession"),
            extraConfirmShortcut: (e) =>
              (e.key === "d" || e.key === "D") && hasOnlyShortcutModifier(e),
          },
        );
        return;
      }

      await deleteController.execute(input, review, false);
    } catch (error) { reportDeleteError(error); }
  }

  $effect(() => {
    if (deleteEditor.committing || deleteEditor.uncertain || deleteEditor.resultPending) return;
    if (session.state.mode !== "edit" || !pomodoro.vaultContext) {
      untrack(() => deleteEditor.update(null));
      return;
    }
    try {
      const input = currentDeleteDraft();
      untrack(() => deleteEditor.update(input));
    } catch (error) {
      untrack(() => { deleteEditor.update(null); deleteEditor.error = saveErrorMessage(error); });
    }
  });

  function handleDayClickFromMonth(date: Date) {
    void targetController.request({ mode: "day", date }, { history: true, reason: "view" });
  }

  function handleWeekDayHeaderClick(date: Date) {
    void targetController.request({ mode: "day", date }, { history: true, reason: "view" });
  }

  function handleDayHeaderClick() {
    void targetController.request(
      { mode: dayHeaderReturnMode, date: targetController.currentState().date },
      { history: true, reason: "view" },
    );
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div bind:this={containerEl} use:outsideClose class="relative flex h-full flex-col select-none overflow-hidden">
  {#snippet header(embedded: boolean)}
    <CalendarHeader
      {anchorDate}
      {viewMode}
      {mobileLayout}
      {embedded}
      onNavigate={navigate}
      onViewChange={changeView}
      onDaySelect={(date) => {
        void targetController.request({ mode: targetController.currentState().mode, date }, {
          history: false,
          reason: "programmatic",
        });
      }}
    />
  {/snippet}
  {#if headerTarget}
    <div class="flex shrink-0 select-none" use:portal={headerTarget}>
      {@render header(true)}
    </div>
  {:else}
    {@render header(false)}
  {/if}

  {#if deleteEditor.uncertain || deleteEditor.resultPending}
    <Notice
      class={calendarBannerClass}
      role="status"
      tone="warning"
      copy={false}
      message={t(deleteEditor.uncertain ? "calendar.view.deleteUncertain" : "calendar.view.deleteRefreshPending")}
      onRetry={() => (deleteEditor.committing ? undefined : deleteController.retry())}
    />
  {:else if nativeEditor.uncertain || directEditor.uncertain}
    <Notice
      class={calendarBannerClass}
      role="status"
      tone="warning"
      copy={false}
      message={t("calendar.view.saveUncertain")}
      onRetry={() => (nativeEditor.committing || directEditor.committing ? undefined : retryCalendarSave())}
    />
  {:else if nativeEditor.error && session.state.mode !== "closed" && !nativeEditor.committing}
    <Notice
      class={calendarBannerClass}
      role="status"
      message={t("calendar.view.previewFailed", nativeEditor.error)}
      copy={nativeEditor.error}
      onRetry={retryCalendarPreview}
    />
  {/if}

  {#if persistedSegments.unavailable}
    <Notice
      class={calendarBannerClass}
      role="status"
      icon={CalendarX}
      copy={false}
      message={t("calendar.view.historyUnavailable")}
      onRetry={() => persistedSegments.refreshForTarget({ mode: viewMode, date: new Date(anchorDate) })}
    />
  {/if}

  <div
    bind:this={viewportController.viewWrapper}
    class="min-w-0 flex-1 overflow-hidden"
    style="background-color: var(--cal-bg); touch-action: {mobileLayout ? 'pan-y pinch-zoom' : 'auto'};"
    onpointerdowncapture={handleMobileSwipeStart}
    onclickcapture={handleMobileSwipeClick}
    oncontextmenu={handleMobileContextMenu}
  >
    <div class="h-full min-h-0">
    {#if viewMode === "week" || viewMode === "workweek"}
      <WeekView
        {anchorDate}
        days={multiDayRangeDays}
        events={visibleEvents}
        positionedTimedEventsByDay={calendarViewModel.positionedTimedEventsByDay}
        positionedAllDayEvents={calendarViewModel.positionedAllDayEvents}
        theme={theme.current}
        timezones={viewportController.timezones}
        timezoneAbbreviationMode={viewportController.timezoneAbbreviationMode}
        editingId={visualEditingId}
        {previewedIds}
        persistedSegmentsByEvent={persistedSegments.byEvent}
        initialScrollMinute={viewportController.scrollMinute}
        onScrollChange={(minute) => { viewportController.scrollMinute = minute; }}
        onEventClick={handleEventClick}
        onEventPrefetch={handleEventPrefetch}
        onEventUpdate={handleEventUpdate}
        onEventCreate={handleEventCreate}
        onAddTimezone={(timezone) => viewportController.addTimezone(timezone)}
        onRemoveTimezone={(index) => viewportController.removeTimezone(index)}
        onReorderTimezone={(from, to) => viewportController.reorderTimezone(from, to)}
        onTimezoneAbbreviationModeChange={(mode) => { viewportController.timezoneAbbreviationMode = mode; }}
        onWheelNavigate={handleWheelNavigate}
        onDayHeaderClick={handleWeekDayHeaderClick}
        {mobileLayout}
        onMobileTouchEditStart={handleMobileTouchEditStart}
        onMobileTouchEditEnd={handleMobileTouchEditEnd}
      />
    {:else if viewMode === "day"}
      <DayView
        {anchorDate}
        events={visibleEvents}
        positionedTimedEventsByDay={calendarViewModel.positionedTimedEventsByDay}
        allDayEventsByDay={calendarViewModel.allDayEventsByDay}
        theme={theme.current}
        timezones={viewportController.timezones}
        timezoneAbbreviationMode={viewportController.timezoneAbbreviationMode}
        editingId={visualEditingId}
        {previewedIds}
        persistedSegmentsByEvent={persistedSegments.byEvent}
        initialScrollMinute={viewportController.scrollMinute}
        onScrollChange={(minute) => { viewportController.scrollMinute = minute; }}
        onEventClick={handleEventClick}
        onEventPrefetch={handleEventPrefetch}
        onEventUpdate={handleEventUpdate}
        onEventCreate={handleEventCreate}
        onAddTimezone={(timezone) => viewportController.addTimezone(timezone)}
        onRemoveTimezone={(index) => viewportController.removeTimezone(index)}
        onReorderTimezone={(from, to) => viewportController.reorderTimezone(from, to)}
        onTimezoneAbbreviationModeChange={(mode) => { viewportController.timezoneAbbreviationMode = mode; }}
        onWheelNavigate={handleWheelNavigate}
        onDayHeaderClick={handleDayHeaderClick}
        allowPointerEditing={true}
        {mobileLayout}
        onMobileTouchEditStart={handleMobileTouchEditStart}
        onMobileTouchEditEnd={handleMobileTouchEditEnd}
      />
    {:else}
      <MonthView
        {anchorDate}
        {eventsByDay}
        theme={theme.current}
        onDayClick={handleDayClickFromMonth}
        onEventClick={handleEventClick}
        onEventPrefetch={handleEventPrefetch}
        onRequestPanelClose={handlePanelClose}
        onWheelNavigate={handleWheelNavigate}
      />
    {/if}
    </div>
  </div>

  {#if mobileLayout}
    <button
      type="button"
      aria-label={t("mobile.createEvent")}
      onclick={(event) => openMobileEventCreate(event.currentTarget)}
      class="absolute bottom-4 right-4 z-50 flex min-h-14 min-w-14 items-center justify-center rounded-2xl bg-primary text-primary-foreground shadow-floating active:opacity-85"
    >
      <Plus size={25} strokeWidth={2} aria-hidden="true" />
    </button>
  {/if}

  {#if confirm.action}
    <ConfirmDialog
      title={confirm.title}
      message={confirm.message}
      confirmLabel={confirm.yesLabel}
      cancelLabel={confirm.noLabel}
      extraConfirmShortcut={confirm.extraShortcut}
      onConfirm={confirm.confirmYes}
      onCancel={confirm.confirmNo}
    />
  {/if}

  <!-- Floating event panel -->
  {#if panelLifecycle.component && panelRender}
    {@const Panel = panelLifecycle.component}
    {@const render = panelRender}
    <Panel
      {mobileLayout}
      parked={render.parked}
      mode={render.mode}
      panelSessionKey={render.sessionKey}
      start={render.start}
      end={render.end}
      event={render.mode === "edit" ? render.event : undefined}
      initialCreateData={render.mode === "create" ? render.initialCreateData : undefined}
      recurringScopeEnabled={render.mode === "edit" ? render.recurringScopeEnabled : false}
      anchor={render.anchor}
      initialAllDay={render.initialAllDay}
      detailsLoaded={render.detailsLoaded}
      externalDirty={render.externalDirty}
      initialSyncSeeded
      readOnly={render.readOnly || saveController.saving}
      allowDeleteWhenReadOnly={render.allowDeleteWhenReadOnly && !saveController.saving}
      allowPomodoroWhenReadOnly={render.allowPomodoroWhenReadOnly && !saveController.saving}
      skipInlineDeleteConfirm={render.skipInlineDeleteConfirm}
      inlineEndEventConfirm={render.mode === "edit" ? render.inlineEndEventConfirm : false}
      lockStartControls={render.mode === "edit" ? render.endActiveEventAvailable : false}
      openMusicSection={render.mode === "edit" && render.event.id === musicInspectionEventId}
      calendarIdentityEmail={panelCalendarIdentityEmail}
      loadFullEvent={calendarStore.loadPanelEvent}
      onSave={handlePanelSave}
      deletionOutcome={currentDeletePreview()?.deletion?.outcome}
      onDelete={render.mode === "edit" && !render.parked ? handleDelete : undefined}
      onEndEvent={render.mode === "edit" && !render.parked && render.endActiveEventAvailable
        ? handleEndEvent
        : undefined}
      onChange={handlePanelChange}
      onInitialSync={handlePanelInitialSync}
      onClose={handlePanelClose}
      onScopeChange={handleScopeChange}
      onSurfaceStatusChange={handlePanelSurfaceStatusChange}
    />
  {/if}

  {#if toasts.deleteUndoToast}
    {@const deleteToast = toasts.deleteUndoToast}
    <ActionToast
      message={deleteToast.label}
      actionLabel={!deleteToast.pending && deleteToast.restore ? t("calendar.view.undo") : undefined}
      reserveActionLabel={t("calendar.view.undo")}
      controlsVisible={!deleteToast.pending}
      dismissLabel={t("calendar.view.dismissEventNotification")}
      onAction={undoDeletedEvent}
      onDismiss={() => deleteController.dismissUndo()}
    />
  {/if}


  {#if toasts.saveToast}
    {@const saveToast = toasts.saveToast}
    <ActionToast
      message={saveToast.message}
      variant={saveToast.variant}
      stacked={!!toasts.deleteUndoToast}
      controlsVisible={!saveToast.pending}
      dismissLabel={t("calendar.view.dismissSaveNotification")}
      onDismiss={() => toasts.dismissSaveToastIfCurrent(saveToast.id)}
    />
  {/if}

</div>
