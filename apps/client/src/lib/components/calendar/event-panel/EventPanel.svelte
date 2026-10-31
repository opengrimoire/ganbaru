<script lang="ts">
  import type {
    CalendarEvent, EventSurfaceStatus, EventTransparency, EventVisibility, RecurringScope,
  } from "$lib/calendar/types";
  import MiniDatePicker from "$lib/components/ui/MiniDatePicker.svelte";
  import TimePicker from "$lib/components/ui/TimePicker.svelte";
  import EventColorPicker from "$lib/components/calendar/EventColorPicker.svelte";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import MeetingSection from "./MeetingSection.svelte";
  import PomodoroSection from "./PomodoroSection.svelte";
  import EventConflictNotice from "$lib/components/calendar/conflicts/EventConflictNotice.svelte";
  import { hasTimingConflict } from "$lib/calendar/conflicts";
  import { calendarConflicts } from "$lib/stores/calendar-conflicts.svelte";
  import NotificationsSection from "./NotificationsSection.svelte";
  import RecurrenceSection from "./RecurrenceSection.svelte";
  import MusicSection from "./MusicSection.svelte";
  import ProjectSelector from "$lib/components/projects/pickers/ProjectSelector.svelte";
  import { onMount, tick } from "svelte";
  import { getWorkspacePaneFocus } from "$lib/workspace-tabs/pane-focus";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import type { CalendarDeleteOutcome } from "$lib/api/calendar-edit";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import {
    mobileCalendarNotificationStatus,
    requestMobileCalendarNotificationPermission,
    resolveMobileCalendarNotificationStatus,
    type MobileCalendarNotificationStatus,
  } from "$lib/scheduling/mobile-calendar-notifications";
  import { cn } from "$lib/utils";
  import { formatShortcut, hasOnlyShortcutModifier, hasShortcutModifier } from "$lib/keyboard-shortcuts";
  import { moveRovingIndex } from "$lib/calendar/event-panel-utils";
  import { buildEventPanelInitKey } from "./init-key";
  import { panelArrowKeyTarget } from "./arrow-nav";
  import { formatCalendarDate } from "$lib/calendar/utils";
  import { EventPanelGeometryController } from "./geometry-controller.svelte";
  import {
    EventPanelSessionController,
    eventPanelHeavyLookupId,
  } from "./session-controller.svelte";
  import {
    EventPanelActionsController,
    canRunEventPanelSave,
    isEventPanelDeleteActionTarget,
  } from "./actions-controller.svelte";
  import type { PanelSaveData } from "./payloads";
  import { getMusicContextAssignments, getMusicPlaylistSummaries } from "$lib/music/platform-library";
  import {
    completeMusicAssignmentDrafts,
    musicAssignmentDraftsEqual,
    persistedMusicAssignmentDrafts,
  } from "$lib/music/assignment-draft";
  import { eventMusicPhases, eventPlaysMusic } from "$lib/music/event-playlists";
  import { hasMeetingState } from "$lib/calendar/meeting-state";
  import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";

  import Trash2 from "@lucide/svelte/icons/trash";
  import Archive from "@lucide/svelte/icons/archive";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Calendar1 from "@lucide/svelte/icons/calendar-1";
  import Clock4 from "@lucide/svelte/icons/clock-4";
  import Ban from "@lucide/svelte/icons/ban";
  import Smile from "@lucide/svelte/icons/face-slightly-smiling";
  import Eye from "@lucide/svelte/icons/eye";
  import Lock from "@lucide/svelte/icons/lock";
  import X from "@lucide/svelte/icons/x";


  const theme = getTheme();
  const musicAssignmentsAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "music.context-assignments",
  );
  const notificationSchedulingAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "notifications.native-scheduling",
  );
  const soundscapesAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.soundscapes");
  const androidNotificationScheduling = BUILD_PLATFORM_PROFILE.platform === "android";
  const nativeIdleDetectionAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "pomodoro.native-idle-detection",
  );
  const projects = useProjects();
  const preferences = getPreferences();
  const mobileBackStack = getMobileBackStack();
  const viewport = getViewport();
  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);

  let {
    mode,
    mobileLayout = false,
    panelSessionKey = 0,
    start,
    end,
    event,
    initialCreateData,
    anchor,
    initialAllDay = false,
    externalDirty = false,
    detailsLoaded = false,
    recurringScopeEnabled = false,
    initialSyncSeeded = false,
    parked = false,
    readOnly = false,
    allowDeleteWhenReadOnly = false,
    allowPomodoroWhenReadOnly = false,
    skipInlineDeleteConfirm = false,
    inlineEndEventConfirm = false,
    deletionOutcome,
    lockStartControls = false,
    openMusicSection = false,
    calendarIdentityEmail,
    loadFullEvent,
    onSave,
    confirmMeetingMusic,
    onDelete,
    onEndEvent,
    onClose,
    onChange,
    onInitialSync,
    onScopeChange,
    onSurfaceStatusChange,
  }: {
    mode: "create" | "edit";
    mobileLayout?: boolean;
    panelSessionKey?: number;
    start?: string;
    end?: string;
    event?: CalendarEvent;
    initialCreateData?: Partial<CalendarEvent>;
    anchor: { x: number; y: number; width: number; height: number };
    initialAllDay?: boolean;
    externalDirty?: boolean;
    detailsLoaded?: boolean;
    recurringScopeEnabled?: boolean;
    initialSyncSeeded?: boolean;
    parked?: boolean;
    readOnly?: boolean;
    allowDeleteWhenReadOnly?: boolean;
    allowPomodoroWhenReadOnly?: boolean;
    skipInlineDeleteConfirm?: boolean;
    inlineEndEventConfirm?: boolean;
    deletionOutcome?: CalendarDeleteOutcome;
    lockStartControls?: boolean;
    openMusicSection?: boolean;
    calendarIdentityEmail?: string;
    /**
     * Fetches the panel detail row (description, attendees, organizer, etc.)
     * for an event id. Used as a fallback when edit mode receives a slim
     * event. Normal edit opens preload details before mounting.
     */
    loadFullEvent?: (id: string) => Promise<CalendarEvent | undefined>;
    onSave: (data: PanelSaveData, scope?: RecurringScope) => boolean | Promise<boolean>;
    /** Asks before a save that gives a meeting automatic music; runs `proceed` on confirmation. */
    confirmMeetingMusic?: (proceed: () => Promise<void>) => void;
    onDelete?:(id: string, scope?: RecurringScope) => void;
    onEndEvent?: (data: PanelSaveData, scope?: RecurringScope) => void;
    onClose: () => void;
    onChange?: (data: Partial<CalendarEvent>) => void;
    onInitialSync?: (data: Partial<CalendarEvent>) => void;
    onScopeChange?: (scope: RecurringScope) => void;
    onSurfaceStatusChange?: (status: EventSurfaceStatus | undefined) => void;
  } = $props();

  const session: EventPanelSessionController = new EventPanelSessionController({
    projects,
    controlsDisabled: () => controlsDisabled,
    lockStartControls: () => lockStartControls,
    timeFormat: () => preferences.calendarTimeFormat,
    mode: () => mode,
    preferences: () => ({
      idlePauseEnabled: preferences.focusIdlePauseOnEventCreate,
      idleThresholdMinutes: preferences.focusIdleThresholdMinutes,
    }),
    onChange: () => onChange,
  });
  const controlsDisabled: boolean = $derived(readOnly || parked || session.savePending);
  const startControlsDisabled = $derived(controlsDisabled || lockStartControls);
  const deleteControlsDisabled = $derived(parked || session.savePending || (readOnly && !allowDeleteWhenReadOnly));
  const scopeControlsDisabled = $derived(parked || session.savePending || (readOnly && !allowDeleteWhenReadOnly));
  const deleteEndsEvent = $derived(mode === "edit" && !!event && !!onEndEvent);
  const generalDisabledAffordance = $derived(parked);
  const startDisabledAffordance = $derived(parked || (lockStartControls && !readOnly));

  const dateTime = session.dateTime;
  const timedSectionsVisible = $derived(session.timedSectionsVisible);
  const pomodoroControlsDisabled = $derived(
    parked || session.savePending || !timedSectionsVisible || (readOnly && !allowPomodoroWhenReadOnly),
  );
  const pomodoroReadOnlyInteractive = $derived(
    readOnly && allowPomodoroWhenReadOnly && !parked && timedSectionsVisible,
  );

  // Two-step delete: first click arms, second click confirms. Any other
  // click inside the panel disarms (see panel-root onclick below).
  const deleteAction = $derived(deletionOutcome ?? "mixed");
  const deleteActionLabel = $derived(
    deleteEndsEvent
      ? t("calendar.eventPanel.deleteEndEvent")
      : deleteAction === "archive"
        ? t("calendar.eventPanel.deleteArchive")
        : deleteAction === "delete"
          ? t("calendar.eventPanel.deleteDelete")
          : t("calendar.eventPanel.deleteRemove"),
  );

  const handleProjectSelect = (projectId: string | undefined): void => {
    const changed = session.projectId !== projectId;
    session.handleProjectSelect(projectId);
    if (changed && musicAssignmentsAvailable) void adoptProjectMusicSnapshot(projectId);
  };

  const handleProjectCreate = (projectId: string): void => {
    const changed = session.projectId !== projectId;
    session.handleProjectCreated(projectId);
    if (changed && musicAssignmentsAvailable) void adoptProjectMusicSnapshot(projectId);
  };

  /** Gives a project created from this panel the color of its first saved event. */
  function adoptCreatedProjectColor(saved: PanelSaveData): void {
    const color = session.createdProjectColorForSave(saved);
    const project = projects.projectById(saved.projectId);
    if (color === null || !project) return;
    void projects.setProjectColor(project, color).catch((error) => {
      console.error("set created project color failed", error);
    });
  }

  let musicSnapshots = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  let savedMusicSnapshots = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  let musicOverrides = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  let savedMusicOverrides = $state<MusicContextAssignmentDraft[]>(completeMusicAssignmentDrafts([]));
  let musicPlaylists = $state<MusicPlaylistSummary[]>([]);
  let musicAssignmentsLoading = $state(false);
  let musicAssignmentsError = $state<string | null>(null);
  let musicAssignmentsReadyKey = $state<string | null>(null);
  let musicLoadGeneration = 0;
  let musicSnapshotGeneration = 0;
  let musicOverrideGeneration = 0;
  /** Id of the event, or of the series an occurrence belongs to, whose sync conflicts the panel shows. */
  const conflictEventId = $derived(mode === "edit" && !parked && event ? event.recurringParentId ?? event.id : null);
  const musicAssignmentsDirty = $derived(
    musicAssignmentsReadyKey === session.lastInitKey
      && (!musicAssignmentDraftsEqual(musicSnapshots, savedMusicSnapshots)
        || !musicAssignmentDraftsEqual(musicOverrides, savedMusicOverrides)),
  );

  function copiedProjectSnapshot(
    assignments: readonly MusicContextAssignmentDraft[],
    projectId: string,
  ): MusicContextAssignmentDraft[] {
    return completeMusicAssignmentDrafts(assignments).map((assignment) => ({
      ...assignment,
      provenanceKind: "copied-project",
      provenanceId: projectId,
    }));
  }

  async function initializeMusicAssignments(key: string): Promise<void> {
    if (!musicAssignmentsAvailable) return;
    const generation = ++musicLoadGeneration;
    const snapshotGeneration = ++musicSnapshotGeneration;
    const overrideGeneration = ++musicOverrideGeneration;
    musicAssignmentsLoading = true;
    musicAssignmentsError = null;
    const eventId = mode === "edit" && event ? event.recurringParentId ?? event.id : null;
    const assignmentRequests = eventId
      ? Promise.all([
          getMusicContextAssignments("event-snapshot", eventId),
          getMusicContextAssignments("event-override", eventId),
        ])
      : session.projectId
        ? getMusicContextAssignments("project-default", session.projectId).then((assignments) => [
            copiedProjectSnapshot(assignments, session.projectId as string),
            [],
          ] as const)
        : Promise.resolve([[], []] as const);
    const [assignmentResult, playlistResult] = await Promise.allSettled([
      assignmentRequests,
      getMusicPlaylistSummaries(Date.now(), 0, 500),
    ]);
    if (generation !== musicLoadGeneration || key !== session.lastInitKey) return;
    if (assignmentResult.status === "fulfilled") {
      const [snapshots, overrides] = assignmentResult.value;
      savedMusicSnapshots = completeMusicAssignmentDrafts(snapshots);
      if (snapshotGeneration === musicSnapshotGeneration) {
        musicSnapshots = completeMusicAssignmentDrafts(snapshots);
      }
      savedMusicOverrides = completeMusicAssignmentDrafts(overrides);
      if (overrideGeneration === musicOverrideGeneration) {
        musicOverrides = completeMusicAssignmentDrafts(overrides);
      }
      musicAssignmentsReadyKey = key;
    }
    if (playlistResult.status === "fulfilled") musicPlaylists = playlistResult.value;
    const failures = [assignmentResult, playlistResult]
      .filter((result): result is PromiseRejectedResult => result.status === "rejected")
      .map((result) => result.reason instanceof Error ? result.reason.message : String(result.reason));
    musicAssignmentsError = failures.length > 0 ? failures.join(" ") : null;
    musicAssignmentsLoading = false;
  }

  async function adoptProjectMusicSnapshot(projectId: string | undefined): Promise<void> {
    if (!musicAssignmentsAvailable) return;
    const generation = ++musicSnapshotGeneration;
    musicAssignmentsLoading = true;
    musicAssignmentsError = null;
    try {
      const [assignments, playlists] = await Promise.all([
        projectId ? getMusicContextAssignments("project-default", projectId) : Promise.resolve([]),
        musicPlaylists.length === 0
          ? getMusicPlaylistSummaries(Date.now(), 0, 500)
          : Promise.resolve(musicPlaylists),
      ]);
      if (generation !== musicSnapshotGeneration) return;
      musicPlaylists = playlists;
      musicSnapshots = projectId
        ? copiedProjectSnapshot(assignments, projectId)
        : completeMusicAssignmentDrafts([]).map((assignment) => ({
            ...assignment,
            provenanceKind: "copied-project" as const,
            provenanceId: null,
          }));
    } catch (cause) {
      if (generation !== musicSnapshotGeneration) return;
      musicAssignmentsError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      if (generation === musicSnapshotGeneration) musicAssignmentsLoading = false;
    }
  }

  type Section = "meeting" | "pomodoro" | "notifications" | "repeat" | "music";
  /** Sections whose header toggle the panel owns; Music toggles its own playlist overrides. */
  type ToggleSection = Exclude<Section, "music">;
  let openSection: Section | null = $state(null);
  let mobileNotificationStatus = $state<MobileCalendarNotificationStatus | null>(null);
  let mobileNotificationStatusBusy = $state(false);
  let lastAutoOpenedMusicSession: number | null = null;

  $effect(() => {
    if (
      !musicAssignmentsAvailable
      || !openMusicSection
      || panelSessionKey === undefined
      || panelSessionKey === lastAutoOpenedMusicSession
    ) return;
    lastAutoOpenedMusicSession = panelSessionKey;
    openSection = "music";
  });

  const geometry = new EventPanelGeometryController({
    anchor: () => anchor,
    parked: () => parked,
    viewport: () => ({ width: viewport.width, height: viewport.height }),
  });
  const panelWidth = $derived(geometry.width);
  const panelLayout = $derived(geometry.layout);
  const activePanelLayout = $derived(mobileLayout ? "fullscreen" : panelLayout);
  const panelCanDrag = $derived(!mobileLayout && geometry.canDrag);
  const isDateTimeStacked = $derived(geometry.isDateTimeStacked);

  function isSectionEnabled(s: ToggleSection): boolean {
    if (s === "meeting") return session.meetingEnabled;
    if (s === "pomodoro") return session.pomodoroEnabled;
    if (s === "notifications") return session.notificationsEnabled;
    return !!session.recurrence;
  }


  function handleToggle(s: ToggleSection) {
    if (s === "pomodoro") {
      if (pomodoroControlsDisabled) return;
    } else if (controlsDisabled) {
      return;
    }
    const enabled = isSectionEnabled(s);
    if (enabled) {
      // Disable: keep the meeting data in memory so a misclick is recoverable.
      // Save is what actually commits the erasure (session.saveData() gates on the flag).
      if (s === "meeting") session.meetingEnabled = false;
      if (s === "pomodoro") session.pomodoroEnabled = false;
      if (s === "notifications") {
        session.notificationsEnabled = false;
        session.selectedNotificationMinutes = new Set();
        session.customNotifications = [];
      }
      if (s === "repeat") session.recurrence = undefined;
      if (openSection === s) openSection = null;
    } else {
      // Enable with defaults
      if (s === "meeting") {
        session.meetingEnabled = true;
        session.emitChange();
        handleExpand("meeting");
        return;
      }
      if (s === "pomodoro") {
        session.pomodoroEnabled = true;
        session.pomodoroPreset = "adaptive";
        session.focusDurationMinutes = 40;
        session.shortBreakMinutes = 5;
        session.longBreakMinutes = 10;
        session.applyDefaultIdleTimeoutPreference();
      }
      if (s === "notifications") {
        session.notificationsEnabled = true;
        session.selectedNotificationMinutes = new Set([0]);
      }
      if (s === "repeat") session.recurrence = { frequency: "daily", interval: 1, end: { type: "never" } };
    }
    session.emitChange();
  }

  async function refreshMobileNotificationStatus(): Promise<MobileCalendarNotificationStatus | null> {
    if (!androidNotificationScheduling) return null;
    try {
      const status = await mobileCalendarNotificationStatus();
      mobileNotificationStatus = status;
      return status;
    } catch (error) {
      console.error("Failed to read Android Calendar notification status", error);
      return null;
    }
  }

  async function handleNotificationToggle(): Promise<void> {
    const enabling = !session.notificationsEnabled;
    handleToggle("notifications");
    if (!enabling || !androidNotificationScheduling) return;
    const status = mobileNotificationStatus ?? await refreshMobileNotificationStatus();
    if (status?.permission !== "prompt") return;
    mobileNotificationStatusBusy = true;
    try {
      mobileNotificationStatus = await requestMobileCalendarNotificationPermission();
    } catch (error) {
      console.error("Failed to request Android Calendar notification permission", error);
    } finally {
      mobileNotificationStatusBusy = false;
    }
  }

  async function resolveMobileNotificationDelivery(): Promise<void> {
    const status = mobileNotificationStatus ?? await refreshMobileNotificationStatus();
    if (!status) return;
    mobileNotificationStatusBusy = true;
    try {
      if (status.permission === "prompt") {
        mobileNotificationStatus = await requestMobileCalendarNotificationPermission();
      } else {
        await resolveMobileCalendarNotificationStatus(status);
      }
    } catch (error) {
      console.error("Failed to resolve Android Calendar notification access", error);
    } finally {
      mobileNotificationStatusBusy = false;
    }
  }

  const mobileNotificationDeliveryNotice = $derived.by(() => {
    if (!androidNotificationScheduling || !mobileNotificationStatus) return null;
    if (mobileNotificationStatus.permission !== "granted") {
      return t("calendar.notifications.permissionRequired");
    }
    if (
      mobileNotificationStatus.channel.exists
      && (
        !mobileNotificationStatus.channel.enabled
        || !mobileNotificationStatus.channel.soundConfigured
      )
    ) {
      return t("calendar.notifications.channelRestricted");
    }
    if (
      mobileNotificationStatus.exactAlarm.required
      && !mobileNotificationStatus.exactAlarm.granted
    ) {
      return t("calendar.notifications.exactAlarmRequired");
    }
    return null;
  });

  const mobileNotificationDeliveryAction = $derived.by(() => {
    if (!mobileNotificationStatus) return null;
    if (mobileNotificationStatus.permission === "prompt") {
      return t("calendar.notifications.allowNotifications");
    }
    if (mobileNotificationStatus.permission === "denied") {
      return t("calendar.notifications.openNotificationSettings");
    }
    if (
      mobileNotificationStatus.channel.exists
      && (
        !mobileNotificationStatus.channel.enabled
        || !mobileNotificationStatus.channel.soundConfigured
      )
    ) {
      return t("calendar.notifications.openSoundSettings");
    }
    return t("calendar.notifications.allowExactAlarms");
  });

  function canExpandSection(s: Section): boolean {
    return !controlsDisabled
      || (s === "meeting" && readOnly && !parked && !!showHeavySections)
      || (s === "pomodoro" && !pomodoroControlsDisabled);
  }

  /** Label click: expand/collapse the details panel. */
  function handleExpand(s: Section) {
    if (!canExpandSection(s)) return;
    const canMutate = s === "pomodoro" ? !pomodoroControlsDisabled : !controlsDisabled;
    // Auto-activate repeat when expanding for the first time.
    if (canMutate && s === "repeat" && !session.recurrence && openSection !== s) {
      session.recurrence = { frequency: "daily", interval: 1, end: { type: "never" } };
      session.emitChange();
    }
    // Auto-activate meeting when expanding from a disabled state.
    if (canMutate && s === "meeting" && !session.meetingEnabled && openSection !== s) {
      session.meetingEnabled = true;
      session.emitChange();
    }
    const opening = openSection !== s;
    openSection = opening ? s : null;
    geometry.updateSectionPin(opening);
  }

  let titleInput: HTMLInputElement | undefined = $state();

  const isRecurring = $derived(
    mode === "edit" && recurringScopeEnabled,
  );

  // Edit mode normally receives a full event row preloaded by CalendarView,
  // so the panel paints meeting details, visibility, and description in its
  // first stable render. The async `loadFullEvent` path remains as a fallback
  // for callers that still hand over a slim event.
  const showHeavySections = $derived(mode === "create" || detailsLoaded || session.fullEvent);

  // Trigger the heavy-field fetch whenever a different event opens. The
  // resolver checks `lastFullKey` again at completion so a stale promise
  // for an event the user already navigated away from can't clobber the
  // current panel.
  $effect(() => {
    if (parked || mode !== "edit" || !event?.id || detailsLoaded || !loadFullEvent) {
      session.cancelHeavyLoad();
      return;
    }
    const id = event.id;
    if (id === session.lastFullKey) return;
    const request = session.beginHeavyLoad(id);
    const lookupId = eventPanelHeavyLookupId(event);
    loadFullEvent(lookupId).then((full) => {
      if (full) session.completeHeavyLoad(request, full);
    }).catch((e) => {
      console.error("[EventPanel] loadFullEvent failed:", e);
    });
  });

  $effect(() => {
    const key = buildEventPanelInitKey({
      parked,
      mode,
      panelSessionKey,
      eventId: event?.id,
    });
    if (!session.beginInitialization(key)) return;
    geometry.resetExitAnimation();
    actions.reset();
    geometry.resetForSession(parked);

    if (mode === "edit" && event) {
      session.initializeEdit(event);
    } else if (mode === "create") {
      const createData = initialCreateData ?? {};
      session.initializeCreate(createData, start ?? "", end ?? "", initialAllDay);
      if (!notificationSchedulingAvailable) {
        session.notificationsEnabled = false;
        session.selectedNotificationMinutes = new Set();
        session.customNotifications = [];
      }
    }

    dateTime.resetInteraction();
    openSection = null;
    session.scope = "this";

    // Sync the panel's initial field values so the session baseline exactly
    // matches what the user sees. This must not mark the session dirty: the
    // user has not edited anything yet. With this baseline in place, any
    // subsequent edit that reverts back to the original value restores a
    // clean session, and the panel can be closed silently (no "Discard
    // unsaved changes?" prompt).
    if (!parked && !initialSyncSeeded) {
      (onInitialSync ?? onChange)?.(session.changesPayload());
    }
    session.initialized = true;
    if (!parked && musicAssignmentsAvailable) void initializeMusicAssignments(key);

    if (!parked && mode === "create") {
      const selectKey = key;
      tick().then(() => {
        if (session.lastInitKey === selectKey && !parked && mode === "create") {
          titleInput?.select();
        }
      });
    }

    if (!parked) {
      const measureKey = key;
      tick().then(() => {
        if (session.lastInitKey === measureKey && !parked) {
          geometry.measureNaturalHeight();
        }
      });
    }
  });

  // Heavy-field init runs once per fullEvent arrival. The initial sync
  // (setInitialChanges on the edit session) merges these keys into both `changes` and `baseline` on the
  // session, so a subsequent emitChange that re-emits the same heavy values
  // does not flip dirty. Since the heavy sections are gated on `fullEvent`,
  // the user cannot have edited any of them before this runs, so overwriting
  // their state is safe.
  $effect(() => {
    if (detailsLoaded || !session.fullEvent) return;
    if (mode !== "edit") return;
    if (session.fullEvent.id === session.lastHeavyAppliedKey) return;
    session.lastHeavyAppliedKey = session.fullEvent.id;
    (onInitialSync ?? onChange)?.(session.heavyPayload());
  });

  $effect(() => {
    if (!lockStartControls) return;
    dateTime.startDatePickerOpen = false;
    if (dateTime.timePickerTarget === "start") {
      dateTime.closeTimePicker();
      dateTime.restoreTimeInput("start");
    }
  });

  $effect(() => {
    if (!dateTime.startDatePickerOpen) return;
    return mobileBackStack.activate({
      handle: () => dateTime.cancelDatePicker("start"),
    });
  });

  $effect(() => {
    if (!dateTime.timePickerTarget) return;
    return mobileBackStack.activate({
      handle: () => dateTime.closeTimePicker(),
    });
  });

  $effect(() => {
    if (!dateTime.endDatePickerOpen) return;
    return mobileBackStack.activate({
      handle: () => dateTime.cancelDatePicker("end"),
    });
  });

  // Sync date and time from the event prop when the event is dragged or resized externally.
  // Only updates time fields, not title/description/etc. which the user may
  // have edited in the panel. The session's diff-based dirty tracking handles
  // revert-to-original automatically.
  $effect(() => {
    if (dateTime.isEditing) return;
    if (mode === "edit" && event) {
      session.syncExternalTimes(event.start, event.end);
    } else if (mode === "create") {
      session.syncExternalTimes(start ?? "", end ?? "");
    }
  });

  // Save is always available in create mode (even with default values).
  // In edit mode it tracks the session's diff-based dirty flag so that
  // reverting all edits back to the original values disables the button
  // again, matching the click-outside cancellation behavior.
  const saveReady = $derived(mode === "create" || externalDirty || musicAssignmentsDirty || dateTime.hasSaveableTimeDraft);
  const saveControlsDisabled = $derived(
    (controlsDisabled && !pomodoroReadOnlyInteractive) || session.savePending || !saveReady,
  );
  const isBodyConstrained = $derived(mobileLayout || geometry.isBodyConstrained);

  // When a section is expanded, the panel's bottom edge is pinned at
  // its pre-expansion position so it only grows upward. Otherwise the
  // top is pinned and the panel grows downward, nudging up only if
  // it would overflow the viewport.
  const panelStyle = $derived(geometry.style);
  const parkedPanelStyle = $derived(geometry.parkedStyle);
  const activePanelStyle = $derived(
    mobileLayout
      ? "position:fixed; left:calc(var(--visual-viewport-offset-left) + var(--safe-area-left)); top:calc(var(--visual-viewport-offset-top) + var(--safe-area-top)); width:calc(var(--visual-viewport-width) - var(--safe-area-left) - var(--safe-area-right)); height:calc(var(--visual-viewport-height) - var(--safe-area-top) - var(--safe-area-bottom)); z-index:50;"
      : panelStyle,
  );


  const shortDate = $derived.by(() => {
    if (!session.startDate) return "";
    const [y, m, d] = session.startDate.split("-").map(Number);
    const dt = new Date(y, m - 1, d);
    return dt.toLocaleDateString(locale, { weekday: "short", month: "short", day: "numeric" });
  });

  const isCrossMidnight = $derived(
    session.endDate !== "" && session.endDate !== session.startDate,
  );

  const shortEndDate = $derived.by(() => {
    if (!session.endDate) return "";
    const [y, m, d] = session.endDate.split("-").map(Number);
    const dt = new Date(y, m - 1, d);
    return dt.toLocaleDateString(locale, { weekday: "short", month: "short", day: "numeric" });
  });

  /** Whether the event starts music in its phases, from the given assignment drafts. */
  function playsMusic(
    overrides: readonly MusicContextAssignmentDraft[],
    snapshots: readonly MusicContextAssignmentDraft[],
  ): boolean {
    return musicAssignmentsAvailable
      && timedSectionsVisible
      && eventPlaysMusic(overrides, snapshots, eventMusicPhases(session.pomodoroEnabled), soundscapesAvailable);
  }

  /** Whether this save turns the event into a meeting that starts music, which others may hear. */
  function saveAddsMeetingMusic(): boolean {
    if (!session.meetingEnabled || !playsMusic(musicOverrides, musicSnapshots)) return false;
    if (mode === "create") return true;
    const savedEvent = session.fullEvent ?? event;
    const savedMeeting = !!savedEvent && hasMeetingState(savedEvent);
    return !savedMeeting || !playsMusic(savedMusicOverrides, savedMusicSnapshots);
  }

  function canSave(): boolean {
    return canRunEventPanelSave({
      parked,
      controlsDisabled,
      pomodoroReadOnlyInteractive,
      savePending: session.savePending,
    });
  }

  async function handleSave() {
    if (!canSave()) return;
    if (confirmMeetingMusic && saveAddsMeetingMusic()) {
      confirmMeetingMusic(performSave);
      return;
    }
    await performSave();
  }

  async function performSave() {
    if (!canSave()) return;
    const hadSaveableTimeDraft = dateTime.hasSaveableTimeDraft;
    const committedTimeDraft = dateTime.commitSaveableTimeDrafts();
    if (!externalDirty && !musicAssignmentsDirty && mode !== "create" && (!hadSaveableTimeDraft || !committedTimeDraft)) return;
    const data: PanelSaveData = {
      ...session.saveData(),
      ...(musicAssignmentsAvailable
        ? {
            musicSnapshotAssignments: persistedMusicAssignmentDrafts(musicSnapshots),
            musicOverrideAssignments: persistedMusicAssignmentDrafts(musicOverrides),
          }
        : {}),
    };
    const s = isRecurring ? session.scope : undefined;
    session.savePending = true;
    try {
      if (await onSave(data, s)) {
        adoptCreatedProjectColor(data);
        savedMusicSnapshots = completeMusicAssignmentDrafts(musicSnapshots);
        savedMusicOverrides = completeMusicAssignmentDrafts(musicOverrides);
      }
    } finally {
      session.savePending = false;
    }
  }
  function handleDeleteClick() {
    if (deleteControlsDisabled) return;
    if (!parked && event && onDelete) onDelete(event.id, isRecurring ? session.scope : undefined);
  }

  function handleEndEventClick() {
    if (deleteControlsDisabled || !onEndEvent) return;
    const data: PanelSaveData = {
      ...session.saveData(),
      end: formatCalendarDate(new Date()),
    };
    actions.reset();
    onEndEvent(data, isRecurring ? session.scope : undefined);
  }

  const actions = new EventPanelActionsController({
    parked: () => parked,
    canDelete: () => !deleteControlsDisabled,
    hasDeleteTarget: () => mode === "edit" && !!event && !!(onDelete || onEndEvent),
    deleteEndsEvent: () => deleteEndsEvent,
    inlineEndEventConfirm: () => inlineEndEventConfirm,
    skipInlineDeleteConfirm: () => skipInlineDeleteConfirm,
    save: () => { void handleSave(); },
    delete: handleDeleteClick,
    endEvent: handleEndEventClick,
  });

  function handlePanelClick(e: MouseEvent) {
    if (parked) return;
    e.stopPropagation();
    actions.disarmOutsideConfirm(isEventPanelDeleteActionTarget(e.target));
  }

  function focusPanelArrowTarget(target: HTMLElement) {
    target.focus();

    if (target === dateTime.startTimeInput) {
      dateTime.leaveTimeInputEditMode("start");
      dateTime.startTimeInput?.select();
    } else if (target === dateTime.endTimeInput) {
      dateTime.leaveTimeInputEditMode("end");
      dateTime.endTimeInput?.select();
    }
  }

  function handlePanelArrowKeydown(e: KeyboardEvent) {
    const next = panelArrowKeyTarget(e, geometry.panelEl, parked);
    if (!next) return;

    e.preventDefault();
    e.stopPropagation();
    focusPanelArrowTarget(next);
    next.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  /**
   * Local keydown handler for input/textarea elements. It stops propagation
   * for normal text editing while explicitly letting the panel's own shortcuts
   * (Mod+Enter save, Mod+D end/delete, Escape close) bubble up to the
   * window-level listeners.
   */
  function handleInputKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && hasShortcutModifier(e)) return;
    if ((e.key === "d" || e.key === "D") && hasOnlyShortcutModifier(e)) return;
    if (e.key === "Escape") return;
    e.stopPropagation();
  }

  function metadataButtonClass(extra?: string): string {
    return cn(
      "flex min-w-0 max-w-full items-center justify-center gap-1.5 rounded-none px-0 py-1.5",
      "text-foreground",
      disabledAffordanceClass(generalDisabledAffordance),
      extra,
    );
  }

  function metadataStartButtonClass(): string {
    return cn(
      "flex min-w-0 max-w-full items-center justify-center gap-1.5 rounded-none px-0 py-1.5",
      "text-foreground",
      disabledAffordanceClass(startDisabledAffordance),
    );
  }

  function disabledAffordanceClass(show: boolean): string {
    return show
      ? "disabled:cursor-not-allowed disabled:opacity-60"
      : "disabled:cursor-default disabled:opacity-100";
  }

  function startTimeShellStateClass(): string {
    if (!startControlsDisabled) {
      return dateTime.timePickerTarget === "start"
        ? "ring-1 ring-primary/60"
        : "hover:bg-black/5 dark:hover:bg-black/15";
    }
    return startDisabledAffordance ? "cursor-not-allowed opacity-60" : "";
  }

  const METADATA_ICON_SIZE = 11;
  const METADATA_ICON_CLASS = "shrink-0 translate-y-[0.5px]";
  const scopeOptions: ReadonlyArray<{ value: RecurringScope; label: string }> = $derived([
    { value: "this", label: t("calendar.eventPanel.onlyThis") },
    { value: "following", label: t("calendar.eventPanel.following") },
    { value: "all", label: t("calendar.eventPanel.all") },
  ]);
  function transparencyDisplayLabel(value: EventTransparency): string {
    return value === "transparent"
      ? t("calendar.eventPanel.free")
      : t("calendar.eventPanel.busy");
  }

  function visibilityDisplayLabel(value: EventVisibility): string {
    return value === "public"
      ? t("calendar.eventPanel.visibilityPublic")
      : t("calendar.eventPanel.visibilityPrivate");
  }

  const transparencyLabel = $derived(transparencyDisplayLabel(session.transparency));
  const visibilityLabel = $derived(visibilityDisplayLabel(session.visibility));
  const deleteActionVerb = $derived(
    deleteEndsEvent
      ? t("calendar.eventPanel.actionEndEvent")
      : deleteAction === "archive"
        ? t("calendar.eventPanel.actionArchive")
        : deleteAction === "delete"
          ? t("calendar.eventPanel.actionDelete")
          : t("calendar.eventPanel.actionRemove"),
  );
  const armedDeleteLabel = $derived.by(() => {
    const shortcut = formatShortcut("Mod + D");
    if (deleteEndsEvent) return t("calendar.eventPanel.pressAgainToEndEvent", shortcut);
    if (deleteAction === "archive") return t("calendar.eventPanel.pressAgainToArchive", shortcut);
    if (deleteAction === "mixed") return t("calendar.eventPanel.pressAgainToRemove", shortcut);
    return t("calendar.eventPanel.pressAgainToDelete", shortcut);
  });
  let scopeFocusIndex = $state(0);
  let metadataFocusIndex = $state(0);
  const metadataItemCount = $derived(showHeavySections ? 3 : 2);

  $effect(() => {
    const firstEnabledIndex = startControlsDisabled ? 1 : 0;
    if (metadataFocusIndex < firstEnabledIndex) metadataFocusIndex = firstEnabledIndex;
    else if (metadataFocusIndex >= metadataItemCount) metadataFocusIndex = metadataItemCount - 1;
  });

  async function focusPanelRovingButton(group: string, index: number) {
    await tick();
    geometry.panelEl
      ?.querySelector<HTMLButtonElement>(`[data-panel-roving="${group}"][data-roving-index="${index}"]`)
      ?.focus();
  }

  function setPanelRovingIndex(group: "scope" | "metadata", index: number) {
    if (group === "scope") scopeFocusIndex = index;
    else metadataFocusIndex = index;
  }

  function handlePanelRovingKeydown(
    e: KeyboardEvent,
    group: "scope" | "metadata",
    index: number,
    itemCount: number,
  ) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const firstEnabledIndex = group === "metadata" && startControlsDisabled ? 1 : 0;
    const nextRelativeIndex = moveRovingIndex({
      currentIndex: index - firstEnabledIndex,
      itemCount: itemCount - firstEnabledIndex,
      key: e.key,
      orientation: "horizontal",
    });
    const nextIndex = nextRelativeIndex + firstEnabledIndex;
    if (nextIndex === index) return;
    e.preventDefault();
    e.stopPropagation();
    setPanelRovingIndex(group, nextIndex);
    void focusPanelRovingButton(group, nextIndex);
  }

  function toggleTransparency() {
    if (controlsDisabled) return;
      dateTime.resetInteraction();
    session.transparency = session.transparency === "transparent" ? "opaque" : "transparent";
    session.emitChange();
  }

  function toggleVisibility() {
    if (controlsDisabled) return;
      dateTime.resetInteraction();
    session.visibility = session.visibility === "public" ? "private" : "public";
    session.emitChange();
  }

  function handleScopeClick(s: RecurringScope) {
    if (scopeControlsDisabled) return;
    session.scope = s;
    onScopeChange?.(s);
  }

  const paneFocused = getWorkspacePaneFocus();

  // Global shortcut handling: active whenever the panel is mounted in the focused pane, so the
  // user does not have to click a field inside the panel first. If a modal
  // (ConfirmDialog) is open, its capture-phase window listener swallows the
  // event before it reaches this handler.
  onMount(() => {
    void projects.ensureLoaded().catch((error) => {
      console.error("load projects failed", error);
    });

    function handleKeydown(e: KeyboardEvent) {
      if (paneFocused()) actions.handleKeydown(e);
    }
    function refreshNotificationAccess(): void {
      if (document.visibilityState === "visible") void refreshMobileNotificationStatus();
    }
    window.addEventListener("keydown", handleKeydown);
    window.addEventListener("focus", refreshNotificationAccess);
    document.addEventListener("visibilitychange", refreshNotificationAccess);
    void refreshMobileNotificationStatus();
    return () => {
      musicLoadGeneration += 1;
      musicSnapshotGeneration += 1;
      musicOverrideGeneration += 1;
      window.removeEventListener("keydown", handleKeydown);
      window.removeEventListener("focus", refreshNotificationAccess);
      document.removeEventListener("visibilitychange", refreshNotificationAccess);
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  bind:this={geometry.panelEl}
  class="panel-root flex flex-col"
  data-layout={activePanelLayout}
  data-mobile={mobileLayout || undefined}
  data-readonly={controlsDisabled || undefined}
  data-parked={parked || undefined}
  aria-hidden={parked || undefined}
  style="box-shadow: 0 0 2px 0px var(--panel-edge), 0 1px 2px var(--panel-shadow); {parked ? parkedPanelStyle : activePanelStyle} background-color: var(--panel-bg); visibility: {session.initialized && geometry.positionReady && !parked ? 'visible' : 'hidden'};"
  onclick={handlePanelClick}
  onkeydown={handlePanelArrowKeydown}
>
  <!-- Drag handle bar -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class={cn(
      "sticky top-0 z-10 flex items-center pl-4 pr-2",
      panelCanDrag ? "cursor-grab active:cursor-grabbing" : "cursor-default",
    )}
    style="background-color: var(--sidebar);"
    onpointerdown={(event) => {
      if (panelCanDrag) geometry.handleDragStart(event);
    }}
    onpointermove={(event) => {
      if (panelCanDrag) geometry.handleDragMove(event);
    }}
    onpointerup={() => {
      if (panelCanDrag) geometry.handleDragEnd();
    }}
    onpointercancel={() => {
      if (panelCanDrag) geometry.handleDragEnd();
    }}
    onlostpointercapture={() => {
      if (panelCanDrag) geometry.handleDragEnd();
    }}
  >
    {#if mobileLayout && !parked}
      <div class="w-12" aria-hidden="true"></div>
    {/if}
    <div class="flex flex-1 items-center justify-center py-2.5">
      <div class="h-[1.5px] w-9 bg-muted-foreground/50"></div>
    </div>
    {#if mobileLayout && !parked}
      <button
        type="button"
        aria-label={t("common.close")}
        onclick={onClose}
        onpointerdown={(event) => event.stopPropagation()}
        class="panel-chrome flex h-12 w-12 shrink-0 items-center justify-center text-muted-foreground active:bg-accent"
      >
        <X size={22} aria-hidden="true" />
      </button>
    {/if}
  </div>

  <div
    class={cn(
      "relative min-h-0",
      isBodyConstrained ? "flex-1 overflow-hidden" : "shrink-0",
    )}
  >
    <div
      bind:this={geometry.scrollEl}
      class={cn(
        "event-panel-scroll hide-scrollbar overscroll-contain",
        isBodyConstrained ? "h-full overflow-y-auto" : "overflow-visible",
      )}
    >
    <div bind:this={geometry.contentEl}>
    <!-- Main editor: title + date -->
    <div class="shrink-0 px-4 pt-2.5">

    <!-- Scope selector (recurring events only) -->
    {#if isRecurring}
      <div class="relative top-0.5 mb-2 grid grid-cols-3 overflow-hidden rounded-sm bg-event-panel-contrast p-0.5 text-[0.733333rem]"
        role="radiogroup"
        aria-label={t("calendar.eventPanel.applyChangesTo")}>
        {#each scopeOptions as option, index}
          <button
            role="radio"
            aria-checked={session.scope === option.value}
            onclick={() => handleScopeClick(option.value)}
            onfocus={() => { scopeFocusIndex = index; }}
            onkeydown={(e) => handlePanelRovingKeydown(e, "scope", index, scopeOptions.length)}
            data-panel-roving="scope"
            data-roving-index={index}
            tabindex={scopeFocusIndex === index ? 0 : -1}
            disabled={scopeControlsDisabled}
            class={cn(
              "readonly-interactive flex min-w-0 items-center justify-center rounded px-2 py-1 text-center font-semibold",
              session.scope === option.value
                ? "bg-action-confirm text-action-confirm-foreground"
                : "text-event-panel-input-text/70",
            )}
          >
            <span class="translate-y-[0.5px]">{option.label}</span>
          </button>
        {/each}
      </div>
    {/if}

    <!-- Title + color circle -->
    <div class="relative top-0.5 flex items-center gap-2.5 px-1">
      <div class="title-wrapper relative min-w-0 flex-1">
        <input
          bind:this={titleInput}
          type="text"
          bind:value={session.title}
          placeholder={t("calendar.eventPanel.titlePlaceholder")}
          disabled={controlsDisabled}
          class="w-full bg-transparent py-0.5 text-[1rem] font-semibold text-foreground outline-none placeholder:text-event-panel-placeholder"
          oninput={() => session.emitChange()}
          onkeydown={handleInputKeydown}
        />
      </div>
      <div class="event-identity-controls flex items-center {mobileLayout ? 'gap-1' : 'gap-2.5'}">
        <ProjectSelector
          selectedProjectId={session.projectId}
          disabled={controlsDisabled}
          {mobileLayout}
          onSelect={handleProjectSelect}
          onCreate={handleProjectCreate}
        />
        {#if !controlsDisabled}
          <EventColorPicker
            color={session.color}
            theme={theme.current}
            {mobileLayout}
            buttonClass="event-identity-trigger"
            onSelect={(color) => {
              session.color = color;
              session.emitChange();
            }}
          />
        {/if}
      </div>
    </div>
    <hr class="border-event-panel-divider mx-1 mt-0.5" />

    <div class="mt-1.5 flex flex-col gap-px px-1">
      <!-- Date + time -->
      <div
        class="date-time-grid relative text-[0.866667rem] leading-none"
        data-stacked={isDateTimeStacked || undefined}
      >
      <!-- Start date -->
      <div class="relative z-1 min-w-0 justify-self-start">
        <button bind:this={dateTime.startDateButton}
          onclick={() => dateTime.toggleDatePicker("start", "pointer")}
          onkeydown={(e) => dateTime.handleDateButtonKeydown(e, "start")}
          disabled={startControlsDisabled}
          class={cn(
            "date-chip max-w-full rounded py-0.5 text-event-panel-input-text",
            disabledAffordanceClass(startDisabledAffordance),
            startControlsDisabled
              ? ""
              : dateTime.startDatePickerOpen
                ? "ring-1 ring-primary/60"
                : "hover:bg-black/5 dark:hover:bg-black/15",
          )}>
          {shortDate}
        </button>

        <!-- Floating start date picker -->
        {#if dateTime.startDatePickerOpen}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="fixed inset-0 z-19" onclick={() => { dateTime.startDatePickerOpen = false; }}></div>
          <div class="absolute left-0 top-full z-20 mt-1 w-60 rounded-lg bg-popover p-2 shadow-lg ring-1 ring-border/60">
            <MiniDatePicker
              selectedDate={session.startDate}
              rangeStartDate={session.startDate}
              rangeEndDate={session.endDate}
              highlightToday={false}
              activeHighlight="primary"
              onSelect={(date, source) => dateTime.selectDate("start", date, source)}
              onCancel={(source) => dateTime.cancelDatePicker("start", source)}
            />
          </div>
        {/if}
      </div>

      <!-- Time group, visually hidden when all-day so the date grid keeps its shape. -->
      <div
        class="time-group relative {dateTime.timePickerTarget ? 'z-20' : 'z-2'} flex items-center justify-center gap-1.5 py-0.5"
        class:invisible={session.allDay}
        class:pointer-events-none={session.allDay}
        aria-hidden={session.allDay}
      >
        <span
          class="time-input-shell relative z-30 rounded text-center text-event-panel-input-text
            {preferences.calendarTimeFormat === '12h' ? 'text-[0.8rem]' : 'text-[0.866667rem]'}
            {startTimeShellStateClass()}"
          data-value={dateTime.timeInputMirrorValue("start")}>
          <input bind:this={dateTime.startTimeInput}
            type="text"
            data-panel-arrow-nav="true"
            inputmode={preferences.calendarTimeFormat === "12h" ? "text" : "numeric"}
            onbeforeinput={(event) => dateTime.handleTimeBeforeInput(event)}
            oninput={(event) => dateTime.handleTimeDraftInput(event, "start")}
            onblur={() => dateTime.commitTimeInput("start")}
            onclick={() => dateTime.handleTimeInputClick("start")}
            disabled={startControlsDisabled || session.allDay}
            maxlength={preferences.calendarTimeFormat === "12h" ? 7 : 5}
            placeholder={preferences.calendarTimeFormat === "12h" ? "h:mmam" : "HH:MM"}
            class={cn(
              "time-input bg-transparent px-0 py-0.5 text-center outline-none",
              disabledAffordanceClass(startDisabledAffordance),
            )}
            value={dateTime.timeInputDisplayValue("start")}
            onkeydown={(event) => dateTime.handleTimeInputKeydown(event, "start")} />
        </span>
        <span class="text-muted-foreground/60">-</span>
        <span
          class="time-input-shell relative z-30 rounded text-center text-event-panel-input-text
            {preferences.calendarTimeFormat === '12h' ? 'text-[0.8rem]' : 'text-[0.866667rem]'}
            {controlsDisabled ? '' : dateTime.timePickerTarget === 'end' ? 'ring-1 ring-primary/60' : 'hover:bg-black/5 dark:hover:bg-black/15'}"
          data-value={dateTime.timeInputMirrorValue("end")}>
          <input bind:this={dateTime.endTimeInput}
            type="text"
            data-panel-arrow-nav="true"
            inputmode={preferences.calendarTimeFormat === "12h" ? "text" : "numeric"}
            onbeforeinput={(event) => dateTime.handleTimeBeforeInput(event)}
            oninput={(event) => dateTime.handleTimeDraftInput(event, "end")}
            onblur={() => dateTime.commitTimeInput("end")}
            onclick={() => dateTime.handleTimeInputClick("end")}
            disabled={controlsDisabled || session.allDay}
            maxlength={preferences.calendarTimeFormat === "12h" ? 7 : 5}
            placeholder={preferences.calendarTimeFormat === "12h" ? "h:mmam" : "HH:MM"}
            class="time-input bg-transparent px-0 py-0.5 text-center outline-none"
            value={dateTime.timeInputDisplayValue("end")}
            onkeydown={(event) => dateTime.handleTimeInputKeydown(event, "end")} />
        </span>

        <!-- Floating time picker -->
        {#if dateTime.timePickerTarget}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="fixed inset-0 z-19" onclick={() => dateTime.closeTimePicker("pointer")}></div>
          {@const isEnd = dateTime.timePickerTarget === 'end'}
          {@const startMinutes = (() => { const [h, m] = (session.startTime || "0:0").split(":").map(Number); return h * 60 + m; })()}
          <div class="absolute top-full z-20 mt-1 rounded-lg bg-popover shadow-lg ring-1 ring-border/60"
            style="left: {isEnd ? '50%' : '0'}; width: {dateTime.timePickerWidth(isEnd)};">
            <TimePicker
              currentTime={isEnd ? session.endTime : session.startTime}
              {isEnd}
              {startMinutes}
              focusOnOpen={dateTime.timePickerKeyboardOpen}
              inputNavigation={dateTime.timePickerInputNavigation}
              onSelect={(time, source) => dateTime.selectTime(time, source)}
              onCancel={(source) => dateTime.closeTimePicker(source)}
              onTypeDigit={(digit) => dateTime.beginTimeTypingFromPicker(digit)} />
          </div>
        {/if}
      </div>

      <!-- End date -->
      <div class="relative z-1 min-w-0 justify-self-end text-right">
        <button bind:this={dateTime.endDateButton}
          onclick={() => dateTime.toggleDatePicker("end", "pointer")}
          onkeydown={(e) => dateTime.handleDateButtonKeydown(e, "end")}
          disabled={controlsDisabled}
          class="date-chip max-w-full rounded py-0.5 text-event-panel-input-text
            {controlsDisabled ? '' : dateTime.endDatePickerOpen ? 'ring-1 ring-primary/60' : 'hover:bg-black/5 dark:hover:bg-black/15'}">
          {shortEndDate}
        </button>

        <!-- Floating end date picker -->
        {#if dateTime.endDatePickerOpen}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="fixed inset-0 z-19" onclick={() => { dateTime.endDatePickerOpen = false; }}></div>
          <div class="absolute right-0 top-full z-20 mt-1 w-60 rounded-lg bg-popover p-2 shadow-lg ring-1 ring-border/60">
            <MiniDatePicker
              selectedDate={session.endDate}
              rangeStartDate={session.startDate}
              rangeEndDate={session.endDate}
              highlightToday={false}
              activeHighlight="primary"
              onSelect={(date, source) => dateTime.selectDate("end", date, source)}
              onCancel={(source) => dateTime.cancelDatePicker("end", source)}
            />
          </div>
        {/if}
      </div>
      </div>

    </div>

  </div>

  <!-- Metadata strip -->
  <div class="flex flex-col gap-3 px-4 pb-0 pt-1.5">

    <!-- All-day / Availability / Visibility -->
    <div
      class="flex w-full items-center justify-evenly overflow-hidden rounded-none bg-event-panel-contrast text-[0.733333rem]"
      role="toolbar"
      aria-label={t("calendar.eventPanel.metadataControls")}
    >
      <!-- All day -->
      <button
        onclick={() => dateTime.toggleAllDay()}
        onfocus={() => { metadataFocusIndex = 0; }}
        onkeydown={(e) => handlePanelRovingKeydown(e, "metadata", 0, metadataItemCount)}
        data-panel-roving="metadata"
        data-roving-index="0"
        tabindex={!startControlsDisabled && metadataFocusIndex === 0 ? 0 : -1}
        disabled={startControlsDisabled}
        class={metadataStartButtonClass()}
      >
        {#if session.allDay}
          <Calendar1 size={METADATA_ICON_SIZE} class={METADATA_ICON_CLASS} />
          <span class="translate-y-[1.13px] truncate">{t("calendar.eventPanel.allDay")}</span>
        {:else}
          <Clock4 size={METADATA_ICON_SIZE} class={METADATA_ICON_CLASS} />
          <span class="translate-y-[1.13px] truncate">{t("calendar.eventPanel.timed")}</span>
        {/if}
      </button>

      <!-- Show as -->
      <button
        onclick={toggleTransparency}
        onfocus={() => { metadataFocusIndex = 1; }}
        onkeydown={(e) => handlePanelRovingKeydown(e, "metadata", 1, metadataItemCount)}
        data-panel-roving="metadata"
        data-roving-index="1"
        data-app-tooltip-focus-disabled="true"
        tabindex={!controlsDisabled && metadataFocusIndex === 1 ? 0 : -1}
        disabled={controlsDisabled}
        class={metadataButtonClass()}
        title={t("calendar.eventPanel.busyTitle")}
      >
        {#if session.transparency === "transparent"}
          <Smile size={METADATA_ICON_SIZE} class={METADATA_ICON_CLASS} />
        {:else}
          <Ban size={METADATA_ICON_SIZE} class={METADATA_ICON_CLASS} />
        {/if}
        <span class="translate-y-[1.13px] truncate">{transparencyLabel}</span>
      </button>

      {#if showHeavySections}
        <button
          onclick={toggleVisibility}
          onfocus={() => { metadataFocusIndex = 2; }}
          onkeydown={(e) => handlePanelRovingKeydown(e, "metadata", 2, metadataItemCount)}
          data-panel-roving="metadata"
          data-roving-index="2"
          data-app-tooltip-focus-disabled="true"
          tabindex={!controlsDisabled && metadataFocusIndex === 2 ? 0 : -1}
          disabled={controlsDisabled}
          class={metadataButtonClass("capitalize")}
          title={t("calendar.eventPanel.privateTitle")}
        >
          {#if session.visibility === "public"}
            <Eye size={METADATA_ICON_SIZE} class={METADATA_ICON_CLASS} />
          {:else}
            <Lock size={METADATA_ICON_SIZE} class={METADATA_ICON_CLASS} />
          {/if}
          <span class="translate-y-[1.13px] truncate">{visibilityLabel}</span>
        </button>
      {/if}
    </div>

  </div>

  <!-- Feature sections -->
  <div class="shrink-0 flex flex-col gap-1.5 px-4 py-1.5">

      {#if conflictEventId}
        <EventConflictNotice eventId={conflictEventId} blocked={externalDirty} onResolved={onClose} />
      {/if}

      <!-- 1) Meeting -->
      {#if showHeavySections}
        <MeetingSection
          enabled={session.meetingEnabled}
          bind:url={session.eventUrl}
          bind:location={session.location}
          geo={session.geo}
          bind:attendees={session.attendees}
          bind:localParticipationStatus={session.localParticipationStatus}
          bind:guestCanModify={session.guestCanModify}
          bind:guestCanInviteOthers={session.guestCanInviteOthers}
          bind:guestCanSeeOtherGuests={session.guestCanSeeOtherGuests}
          organizer={session.organizer}
          selfEmail={calendarIdentityEmail}
          eventTitle={session.title}
          description={session.description}
          readOnly={controlsDisabled}
          allowReadOnlyExpand={readOnly && !parked}
          expanded={openSection === "meeting"}
          onToggle={() => handleToggle("meeting")}
          onExpand={() => handleExpand("meeting")}
          onSurfaceStatusChange={onSurfaceStatusChange}
          onChange={() => session.emitChange()}
          onDescriptionChange={(html) => {
            session.description = html;
            session.emitChange();
          }} />
      {/if}

      <!-- 2) Pomodoro -->
      {#if timedSectionsVisible}
        <PomodoroSection
          enabled={session.pomodoroEnabled}
          bind:preset={session.pomodoroPreset}
          bind:focusDurationMinutes={session.focusDurationMinutes}
          bind:shortBreakMinutes={session.shortBreakMinutes}
          bind:longBreakMinutes={session.longBreakMinutes}
          bind:longBreakAfterFocusCount={session.longBreakAfterFocusCount}
          bind:customRhythmMode={session.customRhythmMode}
          bind:sequenceSteps={session.sequenceSteps}
          bind:idleTimeoutEnabled={session.idleTimeoutEnabled}
          expanded={openSection === "pomodoro"}
          readonlyInteractive={pomodoroReadOnlyInteractive}
          idleDetectionAvailable={nativeIdleDetectionAvailable}
          timingConflict={conflictEventId !== null && hasTimingConflict(calendarConflicts.all, conflictEventId)}
          onToggle={() => handleToggle("pomodoro")}
          onExpand={() => handleExpand("pomodoro")}
          onChange={() => session.emitChange()} />
      {/if}

      <!-- 3) Notifications -->
      {#if notificationSchedulingAvailable}
      <NotificationsSection
        enabled={session.notificationsEnabled}
        bind:selected={session.selectedNotificationMinutes}
        bind:customNotifications={session.customNotifications}
        expanded={openSection === "notifications"}
        onToggle={() => { void handleNotificationToggle(); }}
        onExpand={() => handleExpand("notifications")}
        onChange={() => session.emitChange()}
        deliveryNotice={mobileNotificationDeliveryNotice}
        deliveryActionLabel={mobileNotificationDeliveryAction}
        deliveryActionBusy={mobileNotificationStatusBusy}
        onDeliveryAction={() => { void resolveMobileNotificationDelivery(); }} />
      {/if}

      <!-- 4) Repeat -->
      <RecurrenceSection
        bind:recurrence={session.recurrence}
        startDate={session.startDate}
        rdate={session.rdate}
        expanded={openSection === "repeat"}
        onToggle={() => handleToggle("repeat")}
        onExpand={() => handleExpand("repeat")}
        onChange={() => session.emitChange()} />

      <!-- 5) Music -->
      {#if timedSectionsVisible && musicAssignmentsAvailable}
        <MusicSection
          overrides={musicOverrides}
          snapshots={musicSnapshots}
          playlists={musicPlaylists}
          pomodoroEnabled={session.pomodoroEnabled}
          expanded={openSection === "music"}
          disabled={controlsDisabled || musicAssignmentsReadyKey !== session.lastInitKey}
          loading={musicAssignmentsLoading}
          error={musicAssignmentsError}
          onOverridesChange={(assignments) => {
            musicOverrideGeneration += 1;
            musicOverrides = assignments;
          }}
          onExpand={() => handleExpand("music")}
          onRetry={() => initializeMusicAssignments(session.lastInitKey)} />
      {/if}
  </div>
  </div>
  </div>
    {#if isBodyConstrained}
      <CalendarScrollbar scrollContainer={geometry.scrollEl} wheelPassthrough />
    {/if}
  </div>

  <!-- Save (pinned outside scroll) -->
  <div
    class={cn(
      "shrink-0 px-4",
      activePanelLayout === "fullscreen" ? "pb-2 pt-1" : "pb-3.5 pt-1.5",
    )}
    style="background-color: var(--panel-bg);"
  >
    {#if readOnly
      && !allowPomodoroWhenReadOnly
      && !(allowDeleteWhenReadOnly && mode === "edit" && (onDelete || onEndEvent) && event)}
      <div class="flex w-full items-center justify-center rounded-none py-1.5 text-[0.8rem] text-muted-foreground/60"
        style="background-color: var(--panel-contrast);">
        {t("calendar.eventPanel.readOnly")}
      </div>
    {:else}
      <div class="panel-footer-actions flex">
        {#if actions.deleteArmed && mode === "edit" && event && (onDelete || onEndEvent) && (!deleteEndsEvent || inlineEndEventConfirm)}
          <button
            type="button"
            data-event-panel-delete-action
            onclick={() => actions.confirmArmedDelete()}
            disabled={deleteControlsDisabled}
            class="readonly-interactive flex flex-1 items-center justify-center gap-2 py-1.5 text-[0.866667rem] text-action-danger-armed-foreground bg-action-danger-armed">
            {#if deleteEndsEvent}
              <Scissors size={14} strokeWidth={1.8} />
            {:else if deleteAction === "archive"}
              <Archive size={14} strokeWidth={1.8} />
            {:else}
              <Trash2 size={14} strokeWidth={1.8} />
            {/if}
            <span>{armedDeleteLabel}</span>
          </button>
        {:else}
          {#if mode === "edit" && (onDelete || onEndEvent) && event}
            <button
              type="button"
              data-event-panel-delete-action
              onclick={() => actions.armOrConfirmDelete()}
              disabled={deleteControlsDisabled}
              class={cn(
                "readonly-interactive event-panel-delete-icon-button flex w-10 shrink-0 items-center justify-center text-foreground",
              )}
              title={t("calendar.eventPanel.deleteShortcut", deleteActionLabel, formatShortcut("Mod + D"))}>
              {#if deleteEndsEvent}
                <Scissors size={14} strokeWidth={1.8} />
              {:else if deleteAction === "archive"}
                <Archive size={14} strokeWidth={1.8} />
              {:else}
                <Trash2 size={14} strokeWidth={1.8} />
              {/if}
            </button>
          {/if}
          {#if readOnly && !allowPomodoroWhenReadOnly}
            <div class="flex flex-1 cursor-not-allowed items-center justify-center gap-2 py-1.5 text-[0.866667rem] text-muted-foreground"
              style="background-color: var(--panel-contrast);">
              <span>{t("calendar.eventPanel.readOnlyShortcut", formatShortcut("Mod + D"), deleteActionVerb)}</span>
            </div>
          {:else}
          <button onclick={handleSave}
            disabled={saveControlsDisabled}
            class="flex flex-1 items-center justify-center gap-2 py-1.5 text-[0.866667rem]
              {pomodoroReadOnlyInteractive ? 'readonly-interactive' : ''}
              {saveReady
                ? 'bg-action-confirm text-action-confirm-foreground hover:opacity-90'
                : 'text-muted-foreground cursor-not-allowed'}"
            style="background-color: {saveReady ? '' : 'var(--panel-contrast)'};">
            <span>{t("calendar.eventPanel.saveShortcut", formatShortcut("Mod + Enter"))}</span>
          </button>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .panel-root {
    --panel-bg: var(--event-panel-bg);
    --panel-contrast: var(--event-panel-contrast);
    --panel-edge: var(--event-panel-edge);
    --panel-shadow: var(--event-panel-shadow);
    --foreground: var(--event-panel-text);
    --muted-foreground: var(--event-panel-muted-text);
    font-variant-numeric: tabular-nums;
    min-height: 0;
    overflow: hidden;
  }

  .panel-root[data-mobile="true"] :global(button),
  .panel-root[data-mobile="true"] :global(input),
  .panel-root[data-mobile="true"] :global(select) {
    min-height: 3rem;
  }

  .panel-root[data-mobile="true"] :global([data-section="meeting"] .meeting-detail-row),
  .panel-root[data-mobile="true"] :global([data-section="meeting"] input) {
    min-height: 2rem;
  }

  .panel-root[data-mobile="true"] :global([data-section="meeting"] button) {
    min-height: 0;
  }

  .panel-root[data-mobile="true"] .event-identity-controls :global(.event-identity-trigger) {
    width: 2.5rem;
    min-width: 2.5rem;
    height: 2.5rem;
    min-height: 2.5rem;
  }

  .panel-root[data-mobile="true"] .time-input-shell {
    min-height: 3rem;
  }

  .panel-root[data-mobile="true"] .panel-footer-actions :global(button),
  .panel-root[data-mobile="true"] .panel-footer-actions :global(div) {
    min-height: 3rem;
  }

  .date-time-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    column-gap: 0.375rem;
  }

  .date-time-grid[data-stacked] {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    row-gap: 0.125rem;
  }

  .date-time-grid[data-stacked] .time-group {
    grid-column: 1 / -1;
    grid-row: 2;
    justify-self: center;
  }

  .date-chip {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .time-input::placeholder {
    font-size: calc(0.68rem * var(--type-scale));
  }

  .time-input-shell {
    position: relative;
    display: inline-block;
  }

  .time-input-shell::after {
    content: attr(data-value);
    display: block;
    font: inherit;
    letter-spacing: inherit;
    padding-block: 0.125rem;
    visibility: hidden;
    white-space: pre;
    pointer-events: none;
  }

  .time-input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    font: inherit;
    letter-spacing: inherit;
    color: inherit;
  }

  .title-wrapper::after {
    content: "";
    position: absolute;
    right: 0;
    top: 0;
    bottom: 0;
    width: 24px;
    background: linear-gradient(to right, transparent, var(--panel-bg));
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.15s;
  }

  .title-wrapper:not(:focus-within)::after {
    opacity: 1;
  }

  .event-panel-delete-icon-button {
    background-color: var(--panel-contrast);
    background-image: linear-gradient(rgb(0 0 0 / 3%), rgb(0 0 0 / 3%));
  }

  :global(.dark) .event-panel-delete-icon-button {
    background-image: linear-gradient(rgb(0 0 0 / 30%), rgb(0 0 0 / 30%));
  }

  :global(html[data-focus-intent="keyboard"]) .panel-root :global(.section-header button:focus) {
    position: relative;
    z-index: 1;
    outline: none;
    box-shadow: inset 0 0 0 2px var(--ring);
  }

  :global(html[data-focus-intent="keyboard"]) .panel-root :global(.panel-footer-actions button:focus) {
    position: relative;
    z-index: 1;
    outline: none;
    box-shadow: inset 0 0 0 2px var(--ring);
  }

  :global(html[data-focus-intent="keyboard"]) .panel-root :global([data-panel-roving="metadata"]:focus),
  :global(html[data-focus-intent="keyboard"]) .panel-root :global([data-panel-roving="scope"]:focus) {
    position: relative;
    z-index: 1;
    outline: none;
    box-shadow: inset 0 0 0 2px var(--ring);
  }

  /* Kill all interactivity below the drag-handle bar when readOnly */
  .panel-root[data-readonly] :global(button:not(.panel-chrome):not(.readonly-interactive)),
  .panel-root[data-readonly] :global(input:not(.readonly-interactive-input)),
  .panel-root[data-readonly] :global([contenteditable]) {
    pointer-events: none !important;
    cursor: default !important;
  }

</style>
