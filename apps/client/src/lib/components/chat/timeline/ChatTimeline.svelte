<script module lang="ts">
  const THREAD_SCROLL_OFFSETS = new Map<string, number>();
</script>

<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import FileText from "@lucide/svelte/icons/file-text";
  import FolderArchive from "@lucide/svelte/icons/folder-archive";
  import FolderX from "@lucide/svelte/icons/folder-x";
  import Globe from "@lucide/svelte/icons/globe";
  import ImageIcon from "@lucide/svelte/icons/image";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import Search from "@lucide/svelte/icons/search";
  import Terminal from "@lucide/svelte/icons/terminal";
  import Unplug from "@lucide/svelte/icons/unplug";
  import Wrench from "@lucide/svelte/icons/wrench";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import { executionMessageActionTarget } from "$lib/chat/timeline/message-action-target";
  import { chatScrollBehavior } from "$lib/chat/responsive-layout";
  import type { ChatParticipantRead, ChatThreadShellRead, ChatTimelinePageRead, ChatTurnId, ProviderInstanceId, ProviderProbeResult } from "$lib/chat/contracts";
  import { activityFilePath, fileChangePresentation, fileReadActivityPresentation, fileSearchActivityPresentation, isFileReadActivity, isFileSearchActivity, isImageViewActivity, summarizeActivityKinds, transientActivitySummary, type ActivitySummaryCount } from "$lib/chat/timeline/activity-presentation";
  import { LOCAL_CHAT_PARTICIPANT_ID } from "$lib/chat/teammates/participant-display";
  import { chatModelParticipant, type ChatModelParticipant } from "$lib/chat/teammates/participant-identity";
  import { buildTimelineDisplayRows, includeOptimisticTimelineMessage, projectTimelineReadModel, timelineActivityShowsLiveStatus, timelineActivitySupportsDisclosure, timelineModelGroupStartIds, timelineRowsForTurn, type TimelineActivityGroupRow, type TimelineActivityRow, type TimelineDisplayRow, type TimelineMessageRow, type TimelinePlanRow, type TimelineTurnFoldRow } from "$lib/chat/timeline/model";
  import { computeTimelineVirtualWindow, nextTimelineUnreadCount, scrollTopForPreservedAnchor, timelineMinimapRows, timelineScrollbarThumbGeometry, timelineScrollIntent, type TimelineScrollbarThumbGeometry, type TimelineScrollIntent } from "$lib/chat/timeline/virtualization";
  import { formatDateTime, formatList, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ChatMarkdown from "./ChatMarkdown.svelte";
  import ChatActivityDetail from "./ChatActivityDetail.svelte";
  import ChatChangedFilesSummary from "$lib/components/chat/review/ChatChangedFilesSummary.svelte";
  import ChatImageGallery from "./ChatImageGallery.svelte";
  import ChatMessageActionToolbar from "./ChatMessageActionToolbar.svelte";
  import ChatMessageReactionList from "./ChatMessageReactionList.svelte";
  import ChatIdentityButton from "$lib/components/chat/identity/ChatIdentityButton.svelte";

  interface TimelineModelIdentity {
    model: ChatModelParticipant;
  }

  const {
    bottomInsetPx = 0,
    embedded = false,
    hideUserMessages = false,
    teammate = null,
    turnId = null,
    timelinePage = null,
    executionThread = null,
  } = $props<{
    bottomInsetPx?: number;
    embedded?: boolean;
    hideUserMessages?: boolean;
    teammate?: ChatParticipantRead | null;
    turnId?: ChatTurnId | null;
    timelinePage?: ChatTimelinePageRead | null;
    executionThread?: ChatThreadShellRead | null;
  }>();
  const COMPOSER_READING_GAP_PX = 8;
  const TIMELINE_EDGE_PADDING_PX = 16;
  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  const preferences = getPreferences();
  const settings = getSettingsLauncher();
  let scroller: HTMLDivElement | undefined = $state();
  let timelineContent: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(600);
  let viewportWidth = $state(800);
  let expandedTurns = $state<string[]>([]);
  let expandedGroups = $state<string[]>([]);
  let expandedActivities = $state<string[]>([]);
  let expandedMessages = $state<string[]>([]);
  let dismissedPlans = $state<string[]>([]);
  let measuredHeights = $state<Map<string, number>>(new Map());
  let intent = $state<TimelineScrollIntent>("following");
  let unreadEvents = $state(0);
  let operationError = $state<string | null>(null);
  let loadingOlder = $state(false);
  let initialTimelineLoadingVisible = $state(false);
  let previousItemCount = $state(0);
  let restoredThreadId = $state<string | null>(null);
  let reducedMotion = $state(false);
  let hoveredActionMessageId = $state<string | null>(null);
  let hoveredActionTurnId = $state<ChatTurnId | null>(null);
  let disclosureMeasureFrame: number | null = null;
  let destroyed = false;
  let pendingDisclosureAnchor: { rowId: string; viewportOffset: number } | null = null;
  let scrollbarGeometry = $state<TimelineScrollbarThumbGeometry | null>(null);
  let scrollbarTrackTop = $state(0);
  let scrollbarTrackHeight = $state(0);
  let scrollbarDrag = $state<{
    pointerId: number;
    startClientY: number;
    startScrollTop: number;
    scrollPerPixel: number;
  } | null>(null);
  const timelinePages = $derived(timelinePage ? [timelinePage] : chat.timelinePages);
  const timelineItems = $derived(timelinePage ? timelinePage.items : chat.timelineItems);
  const timelineLoading = $derived(timelinePage ? false : chat.timelineLoading);
  const pageTurns = $derived(timelinePages.flatMap((page) => page.turns));
  const projection = $derived(projectTimelineReadModel(timelineItems, pageTurns));
  const selectedThread = $derived(executionThread ?? chat.selectedThread);
  const timelineIdentity = $derived(chat.selectedChannelId ?? selectedThread?.id ?? null);
  const optimisticMessage = $derived.by(() => {
    if (timelinePage) return null;
    const pendingMessage = chat.pendingUserMessage;
    if (!pendingMessage) return null;
    return pendingMessage.threadId === (selectedThread?.id ?? chat.draftThreadId)
      ? pendingMessage.row
      : null;
  });
  const timelineRows = $derived(timelineRowsForTurn(
    includeOptimisticTimelineMessage(projection.rows, optimisticMessage),
    turnId,
  )
    .filter((row) => !hideUserMessages || row.kind !== "message" || row.role !== "user"));
  const displayRows = $derived(buildTimelineDisplayRows(timelineRows.filter((row) => row.kind !== "plan" || !dismissedPlans.includes(row.id)), projection.turns, new Set(expandedTurns), new Set(expandedGroups)));
  const turnsById = $derived(new Map(projection.turns.map((turn) => [turn.id, turn])));
  const modelGroupStartIds = $derived(timelineModelGroupStartIds(displayRows));
  const assistantActionMessages = $derived.by(() => {
    const messages = new Map<ChatTurnId, TimelineMessageRow>();
    for (const row of displayRows) {
      if (row.kind !== "message" || row.role !== "assistant" || row.state !== "complete" || !row.turnId) continue;
      const current = messages.get(row.turnId);
      const replacesCurrent = !current
        || row.phase === "final_answer" && current.phase !== "final_answer"
        || row.phase === current.phase && row.sequence > current.sequence;
      if (replacesCurrent) {
        messages.set(row.turnId, row);
      }
    }
    return messages;
  });
  const virtualWindow = $derived(computeTimelineVirtualWindow(displayRows, measuredHeights, scrollTop, viewportHeight));
  const renderedRows = $derived(embedded
    ? displayRows.map((row, index) => ({ row, index }))
    : virtualWindow.items);
  const selectedWorkingFolder = $derived(executionThread
    ? chat.workingFolders.find((entry) => entry.workingFolder.id === executionThread.workingFolderId) ?? null
    : chat.selectedWorkingFolder);
  const selectedProvider = $derived(chat.settings?.providerInstances.find((provider) => provider.configuration.instanceId === selectedThread?.providerInstanceId) ?? null);
  const localParticipant = $derived<ChatParticipantRead>({
    id: LOCAL_CHAT_PARTICIPANT_ID,
    kind: "local_user",
    displayName: preferences.profileDisplayName || t("chat.timeline.you"),
    avatar: { schemaVersion: 1, value: {} },
    revision: 0,
    archivedAt: null,
  });
  const minimapRows = $derived(timelineMinimapRows(displayRows));
  const showMinimap = $derived(!embedded && displayRows.length >= 80 && viewportWidth >= 900 && minimapRows.length > 0);
  const latestTimelinePage = $derived(timelinePages.at(-1));
  const timelineRevision = $derived(`${latestTimelinePage
    ? "revision" in latestTimelinePage
      ? latestTimelinePage.revision
      : latestTimelinePage.threadRevision
    : 0}:${timelinePage ? "" : chat.pendingUserMessage?.row.id ?? ""}`);
  const bottomPadding = $derived(virtualWindow.paddingBottom
    + (bottomInsetPx > 0 ? bottomInsetPx + COMPOSER_READING_GAP_PX : TIMELINE_EDGE_PADDING_PX));

  $effect(() => {
    if (!timelineLoading || displayRows.length > 0) {
      initialTimelineLoadingVisible = false;
      return;
    }
    const timer = window.setTimeout(() => {
      if (timelineLoading && displayRows.length === 0) {
        initialTimelineLoadingVisible = true;
      }
    }, 180);
    return () => window.clearTimeout(timer);
  });

  onMount(() => {
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      viewportHeight = entry.contentRect.height;
      viewportWidth = entry.contentRect.width;
      updateTimelineScrollbar();
    });
    if (!embedded && scroller) observer.observe(scroller);
    const contentObserver = new ResizeObserver(updateTimelineScrollbar);
    if (!embedded && timelineContent) contentObserver.observe(timelineContent);
    if (!embedded) updateTimelineScrollbar();
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const updateMotion = () => { reducedMotion = motion.matches; };
    updateMotion();
    motion.addEventListener("change", updateMotion);
    scroller?.addEventListener("click", handleTimelineClick);
    scroller?.addEventListener("transitionend", handleTimelineTransitionEnd);
    return () => {
      observer.disconnect();
      contentObserver.disconnect();
      motion.removeEventListener("change", updateMotion);
      scroller?.removeEventListener("click", handleTimelineClick);
      scroller?.removeEventListener("transitionend", handleTimelineTransitionEnd);
    };
  });

  onDestroy(() => {
    destroyed = true;
    if (disclosureMeasureFrame !== null) cancelAnimationFrame(disclosureMeasureFrame);
  });

  $effect(() => {
    renderedRows;
    if (embedded) return;
    void tick().then(measureRows);
  });

  $effect(() => {
    const count = timelineItems.length;
    if (count > previousItemCount) {
      unreadEvents = nextTimelineUnreadCount(unreadEvents, count - previousItemCount, intent, false);
    }
    previousItemCount = count;
  });

  $effect(() => {
    timelineRevision;
    if (embedded || intent !== "following") return;
    void tick().then(pinToLatest);
  });

  $effect(() => {
    bottomInsetPx;
    if (embedded || intent !== "following") return;
    void tick().then(pinToLatest);
  });

  $effect(() => {
    const identity = timelineIdentity;
    if (embedded || !identity || timelineLoading || restoredThreadId === identity) return;
    restoredThreadId = identity;
    void tick().then(() => {
      if (destroyed || !scroller || timelineIdentity !== identity) return;
      scroller.scrollTop = THREAD_SCROLL_OFFSETS.get(identity) ?? scroller.scrollHeight;
      scrollTop = scroller.scrollTop;
      intent = timelineScrollIntent(scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop, "anchored");
    });
  });

  function handleScroll(): void {
    if (embedded || !scroller) return;
    scrollTop = scroller.scrollTop;
    updateTimelineScrollbar();
    if (timelineIdentity) THREAD_SCROLL_OFFSETS.set(timelineIdentity, scrollTop);
    intent = timelineScrollIntent(scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop, intent);
    if (intent === "following") unreadEvents = 0;
    if (scroller.scrollTop < 240) void loadOlder();
  }

  async function loadOlder(): Promise<void> {
    if (!scroller || loadingOlder || !timelinePages.some((page) => page.previousCursor !== null)) return;
    loadingOlder = true;
    const anchor = scroller.querySelector<HTMLElement>("[data-timeline-row-id]");
    const anchorId = anchor?.dataset.timelineRowId;
    const beforeTop = anchor?.offsetTop ?? 0;
    const beforeScroll = scroller.scrollTop;
    try {
      await chat.loadOlderTimeline(virtualWindow.items[0]?.row.sequence ?? null);
      await tick();
      const after = anchorId ? scroller.querySelector<HTMLElement>(`[data-timeline-row-id="${CSS.escape(anchorId)}"]`) : null;
      if (after) scroller.scrollTop = scrollTopForPreservedAnchor(beforeScroll, beforeTop, after.offsetTop);
    } catch (error: unknown) {
      reportError(error);
    } finally {
      loadingOlder = false;
    }
  }

  function measureRows(): void {
    if (embedded || destroyed || !scroller) return;
    const scrollerTop = scroller.getBoundingClientRect().top;
    const disclosureAnchor = pendingDisclosureAnchor;
    pendingDisclosureAnchor = null;
    const anchor = disclosureAnchor
      ? scroller.querySelector<HTMLElement>(`[data-timeline-row-id="${CSS.escape(disclosureAnchor.rowId)}"]`)
      : [...scroller.querySelectorAll<HTMLElement>("[data-timeline-row-id]")]
        .find((element) => element.getBoundingClientRect().bottom >= scrollerTop);
    const anchorId = anchor?.dataset.timelineRowId;
    const anchorOffset = disclosureAnchor?.viewportOffset
      ?? (anchor ? anchor.getBoundingClientRect().top - scrollerTop : 0);
    const next = new Map(measuredHeights);
    let changed = false;
    for (const element of scroller.querySelectorAll<HTMLElement>("[data-timeline-row-id]")) {
      const id = element.dataset.timelineRowId;
      if (!id) continue;
      const height = element.getBoundingClientRect().height;
      if (height > 0 && next.get(id) !== height) { next.set(id, height); changed = true; }
    }
    if (changed) {
      measuredHeights = next;
      if (intent === "following") {
        void tick().then(pinToLatest);
      } else if (anchorId) {
        void tick().then(() => {
          if (destroyed || !scroller) return;
          const restored = scroller.querySelector<HTMLElement>(`[data-timeline-row-id="${CSS.escape(anchorId)}"]`);
          if (!restored) return;
          const restoredOffset = restored.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
          scroller.scrollTop = scrollTopForPreservedAnchor(scroller.scrollTop, anchorOffset, restoredOffset);
          scrollTop = scroller.scrollTop;
          if (timelineIdentity) THREAD_SCROLL_OFFSETS.set(timelineIdentity, scrollTop);
        });
      }
    }
  }

  function handleTimelineClick(event: MouseEvent): void {
    if (embedded || !scroller || !(event.target instanceof Element)) return;
    const disclosure = event.target.closest<HTMLElement>("[data-timeline-disclosure-expanded]");
    if (!disclosure || !scroller.contains(disclosure)) return;
    const expanding = disclosure.dataset.timelineDisclosureExpanded === "false";
    if (!expanding) return;
    const row = disclosure.closest<HTMLElement>("[data-timeline-row-id]");
    const rowId = row?.dataset.timelineRowId;
    if (!row || !rowId) return;
    pendingDisclosureAnchor = {
      rowId,
      viewportOffset: row.getBoundingClientRect().top - scroller.getBoundingClientRect().top,
    };
    intent = "anchored";
    if (disclosureMeasureFrame !== null) cancelAnimationFrame(disclosureMeasureFrame);
    disclosureMeasureFrame = requestAnimationFrame(() => {
      disclosureMeasureFrame = null;
      measureRows();
    });
  }

  function handleTimelineTransitionEnd(event: TransitionEvent): void {
    if (embedded) return;
    if (!(event.target instanceof HTMLElement)) return;
    if (event.propertyName !== "grid-template-rows" && event.propertyName !== "max-height") return;
    if (!event.target.matches(".chat-disclosure-region, .chat-message-expandable, .file-change-region")) return;
    measureRows();
  }

  function updateTimelineScrollbar(): void {
    if (embedded || !scroller) return;
    scrollbarTrackTop = scroller.offsetTop;
    scrollbarTrackHeight = scroller.clientHeight;
    scrollbarGeometry = timelineScrollbarThumbGeometry(scroller.scrollHeight, scroller.clientHeight, scroller.scrollTop);
  }

  function handleScrollbarTrackPointerDown(event: PointerEvent): void {
    if (!scroller || !scrollbarGeometry || event.button !== 0 || event.target !== event.currentTarget) return;
    event.preventDefault();
    const track = event.currentTarget;
    if (!(track instanceof HTMLElement)) return;
    const pointerOffset = event.clientY - track.getBoundingClientRect().top;
    if (pointerOffset < scrollbarGeometry.offset) scroller.scrollTop -= scroller.clientHeight;
    else if (pointerOffset > scrollbarGeometry.offset + scrollbarGeometry.size) scroller.scrollTop += scroller.clientHeight;
    updateTimelineScrollbar();
  }

  function handleScrollbarThumbPointerDown(event: PointerEvent): void {
    if (!scroller || !scrollbarGeometry || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const thumb = event.currentTarget;
    if (!(thumb instanceof HTMLElement)) return;
    const thumbTravel = scroller.clientHeight - scrollbarGeometry.size;
    const scrollRange = scroller.scrollHeight - scroller.clientHeight;
    scrollbarDrag = {
      pointerId: event.pointerId,
      startClientY: event.clientY,
      startScrollTop: scroller.scrollTop,
      scrollPerPixel: thumbTravel > 0 ? scrollRange / thumbTravel : 0,
    };
    thumb.setPointerCapture(event.pointerId);
  }

  function handleScrollbarThumbPointerMove(event: PointerEvent): void {
    if (!scroller || !scrollbarDrag || event.pointerId !== scrollbarDrag.pointerId) return;
    event.preventDefault();
    scroller.scrollTop = scrollbarDrag.startScrollTop
      + (event.clientY - scrollbarDrag.startClientY) * scrollbarDrag.scrollPerPixel;
    updateTimelineScrollbar();
  }

  function finishScrollbarDrag(event: PointerEvent): void {
    if (!scrollbarDrag || event.pointerId !== scrollbarDrag.pointerId) return;
    const thumb = event.currentTarget;
    if (thumb instanceof HTMLElement && thumb.hasPointerCapture(event.pointerId)) thumb.releasePointerCapture(event.pointerId);
    scrollbarDrag = null;
  }

  function handleScrollbarWheel(event: WheelEvent): void {
    if (!scroller) return;
    event.preventDefault();
    scroller.scrollTop += event.deltaY;
    updateTimelineScrollbar();
  }

  function measureExpandableHeight(node: HTMLElement): { destroy(): void } {
    const content = node.firstElementChild;
    let measuredHeight = -1;
    const update = () => {
      const height = content instanceof HTMLElement ? content.scrollHeight : node.scrollHeight;
      if (height === measuredHeight) return;
      measuredHeight = height;
      node.style.setProperty("--chat-expanded-height", `${Math.ceil(height)}px`);
    };
    update();
    const observer = new ResizeObserver(update);
    if (content) observer.observe(content);
    else observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  function pinToLatest(): void {
    if (destroyed || !scroller || intent !== "following") return;
    scroller.scrollTop = scroller.scrollHeight;
    scrollTop = scroller.scrollTop;
  }

  function jumpToLatest(): void {
    scroller?.scrollTo({ top: scroller.scrollHeight, behavior: chatScrollBehavior(reducedMotion) });
    intent = "following";
    unreadEvents = 0;
  }

  function toggle(list: string[], id: string): string[] {
    return list.includes(id) ? list.filter((entry) => entry !== id) : [...list, id];
  }

  function copy(value: string): void {
    void writeTextToClipboard(value).catch(reportError);
  }

  function showModelActionToolbar(turnId: ChatTurnId): void {
    hoveredActionMessageId = null;
    hoveredActionTurnId = turnId;
  }

  function showUserActionToolbar(messageId: string): void {
    hoveredActionMessageId = messageId;
    hoveredActionTurnId = null;
  }

  function clearActionToolbar(): void {
    hoveredActionMessageId = null;
    hoveredActionTurnId = null;
  }

  function reportError(error: unknown): void {
    operationError = error instanceof Error ? error.message : String(error);
  }

  function durationLabel(milliseconds: number | null): string {
    if (milliseconds === null) return t("chat.timeline.durationUnknown");
    const seconds = Math.max(1, Math.round(milliseconds / 1000));
    return t("chat.timeline.seconds", formatNumber(localization.locale, seconds));
  }

  function foldLabel(row: TimelineTurnFoldRow): string {
    if (row.state === "interrupted") return t("chat.timeline.stoppedAfter", durationLabel(row.durationMs));
    if (row.state === "failed") return t("chat.timeline.failedAfter", durationLabel(row.durationMs));
    return t("chat.timeline.workedFor", durationLabel(row.durationMs));
  }

  function foldActivities(row: TimelineTurnFoldRow): TimelineActivityRow[] {
    return row.hiddenRows.flatMap((hiddenRow) => {
      if (hiddenRow.kind === "activity") return [hiddenRow];
      if (hiddenRow.kind === "activity_group") return [...hiddenRow.earlierRows, hiddenRow.latest];
      return [];
    });
  }

  function timestampLabel(value: string): string {
    return formatDateTime(
      localization.locale,
      new Date(value),
      embedded ? { timeStyle: "short" } : { dateStyle: "medium", timeStyle: "short" },
    );
  }

  function statusLabel(status: TimelineActivityRow["status"]): string {
    switch (status) {
      case "pending": return t("chat.timeline.activityStatus.pending");
      case "active": return t("chat.timeline.activityStatus.active");
      case "waiting": return t("chat.timeline.activityStatus.waiting");
      case "completed": return t("chat.timeline.activityStatus.completed");
      case "interrupted": return t("chat.timeline.activityStatus.interrupted");
      case "failed": return t("chat.timeline.activityStatus.failed");
      case "unknown": return t("chat.timeline.activityStatus.unknown");
    }
  }

  function scrollToMinimapRow(rowId: string): void {
    if (!scroller) return;
    const index = displayRows.findIndex((row) => row.id === rowId);
    if (index < 0) return;
    const denominator = Math.max(1, displayRows.length - 1);
    scroller.scrollTo({ top: (index / denominator) * Math.max(0, scroller.scrollHeight - scroller.clientHeight), behavior: chatScrollBehavior(reducedMotion) });
    intent = index === displayRows.length - 1 ? "following" : "anchored";
  }

  function minimapLabel(row: TimelineMessageRow | TimelineActivityRow): string {
    if (row.kind === "activity") return t("chat.timeline.errorActivity");
    return row.role === "user" ? t("chat.timeline.userMessage") : t("chat.timeline.assistantMessage");
  }

  function activityTitle(activity: TimelineActivityRow): string {
    if (activity.activityKind === "channel_session_boundary") return t("chat.timeline.sessionBoundary", activity.title);
    if (activityIsThinking(activity)) return transientActivitySummary(activity) ?? t("chat.timeline.thinking");
    if (activity.title === "thread_reverted") return t("chat.timeline.threadRestored");
    const active = activity.status === "pending" || activity.status === "active" || activity.status === "waiting";
    if (activity.activityKind === "file_change" || activity.activityKind === "file_change_output") {
      const path = activityFilePath(activity);
      if (path) return active ? t("chat.timeline.editingPath", path) : t("chat.timeline.editedPath", path);
      return active ? t("chat.timeline.editingFile") : t("chat.timeline.editedFile");
    }
    if (isImageViewActivity(activity)) {
      return active ? t("chat.timeline.viewingImage") : t("chat.timeline.viewedImage");
    }
    const search = fileSearchActivityPresentation(activity);
    if (search) {
      if (search.query && search.scope) {
        return active
          ? t("chat.timeline.searchingForIn", search.query, search.scope)
          : t("chat.timeline.searchedForIn", search.query, search.scope);
      }
      if (search.query) {
        return active
          ? t("chat.timeline.searchingFor", search.query)
          : t("chat.timeline.searchedFor", search.query);
      }
      return active ? t("chat.timeline.searchingFiles") : t("chat.timeline.searchedFiles");
    }
    if (activity.activityKind === "web_search") {
      const query = activity.title.trim();
      const generic = ["web search", "web search result", "web_search"].includes(query.toLowerCase());
      return active
        ? t("chat.timeline.searchingWeb")
        : generic || !query
          ? t("chat.timeline.searchedWeb")
          : t("chat.timeline.searchedFor", query);
    }
    if (isFileReadActivity(activity)) {
      const path = fileReadActivityPresentation(activity)?.path;
      if (path) return active ? t("chat.timeline.readingPath", path) : t("chat.timeline.readPath", path);
      return active ? t("chat.timeline.readingFile") : t("chat.timeline.readFile");
    }
    if (activity.activityKind === "command_execution" || activity.activityKind === "command_output") {
      const command = activity.title.trim().replace(/^(?:run|running|ran)\s+/i, "");
      if (!command || command === "command execution" || command === "command output") {
        return active ? t("chat.timeline.runningCommands") : t("chat.timeline.ranCommands");
      }
      return active
        ? t("chat.timeline.runningCommand", command)
        : t("chat.timeline.ranCommand", command);
    }
    if (activity.activityKind === "mcp_tool_call" || activity.activityKind === "dynamic_tool_call") {
      return active
        ? t("chat.timeline.usingTool", activity.title)
        : t("chat.timeline.usedTool", activity.title);
    }
    if (activity.activityKind === "collaboration_task") {
      return active ? t("chat.timeline.delegatingWork") : t("chat.timeline.delegatedWork");
    }
    if (activity.activityKind === "context_compaction") {
      return active ? t("chat.timeline.compactingContext") : t("chat.timeline.compactedContext");
    }
    return activity.title;
  }

  function activitySummaryTitle(activities: readonly TimelineActivityRow[]): string {
    if (activities.length === 0) return "";
    if (activities.length === 1 && activities[0] && activityIsThinking(activities[0])) {
      return activityTitle(activities[0]);
    }
    const active = activities.some((activity) => (
      activity.status === "pending" || activity.status === "active" || activity.status === "waiting"
    ));
    const labels = summarizeActivityKinds(activities).map((summary, index) => {
      const label = activitySummaryLabel(summary, active);
      return index === 0 ? label : lowercaseInitial(label);
    });
    return formatList(localization.locale, labels);
  }

  function lowercaseInitial(value: string): string {
    const [first = "", ...rest] = [...value];
    return `${first.toLocaleLowerCase(localization.locale)}${rest.join("")}`;
  }

  function activitySummaryLabel(summary: ActivitySummaryCount, active: boolean): string {
    const multiple = summary.count > 1;
    switch (summary.kind) {
      case "thinking": return t("chat.timeline.thinking");
      case "commands":
        return active
          ? multiple ? t("chat.timeline.runningCommands") : t("chat.timeline.runningCommandSummary")
          : multiple ? t("chat.timeline.ranCommands") : t("chat.timeline.ranCommandSummary");
      case "file_changes":
        return active
          ? multiple ? t("chat.timeline.editingFiles") : t("chat.timeline.editingFile")
          : multiple ? t("chat.timeline.editedFiles") : t("chat.timeline.editedFile");
      case "file_reads":
        return active
          ? multiple ? t("chat.timeline.readingFiles") : t("chat.timeline.readingFile")
          : multiple ? t("chat.timeline.readFiles") : t("chat.timeline.readFile");
      case "file_searches": return active ? t("chat.timeline.searchingFiles") : t("chat.timeline.searchedFiles");
      case "web_searches": return active ? t("chat.timeline.searchingWeb") : t("chat.timeline.searchedWeb");
      case "image_views":
        return active
          ? multiple ? t("chat.timeline.viewingImages") : t("chat.timeline.viewingImage")
          : multiple ? t("chat.timeline.viewedImages") : t("chat.timeline.viewedImage");
      case "tools":
        return active
          ? multiple ? t("chat.timeline.usingTools") : t("chat.timeline.usingToolSummary")
          : multiple ? t("chat.timeline.usedTools") : t("chat.timeline.usedToolSummary");
      case "collaboration": return active ? t("chat.timeline.delegatingWork") : t("chat.timeline.delegatedWork");
      case "review": return active ? t("chat.timeline.reviewingChanges") : t("chat.timeline.reviewedChanges");
      case "compaction": return active ? t("chat.timeline.compactingContext") : t("chat.timeline.compactedContext");
      case "errors": return multiple ? t("chat.timeline.encounteredErrors") : t("chat.timeline.encounteredError");
    }
  }

  function activityIsThinking(activity: TimelineActivityRow): boolean {
    return activity.id.startsWith("turn-pending:")
      || activity.activityKind === "reasoning"
      || activity.activityKind === "reasoning_text"
      || activity.activityKind === "reasoning_summary";
  }

  function activityIsInProgress(activity: TimelineActivityRow): boolean {
    const turnState = activity.turnId ? turnsById.get(activity.turnId)?.state : null;
    return timelineActivityShowsLiveStatus(activity, turnState);
  }

  function activityGroupCurrent(activities: readonly TimelineActivityRow[]): TimelineActivityRow | null {
    return [...activities].reverse().find(activityIsInProgress) ?? null;
  }

  function activityGroupTitle(activities: readonly TimelineActivityRow[]): string {
    const current = activityGroupCurrent(activities);
    return current ? activityTitle(current) : activitySummaryTitle(activities);
  }

  function fileChangeLineCounts(activity: TimelineActivityRow): { additions: number; deletions: number } | null {
    const changes = fileChangePresentation(activity);
    if (changes.length === 0) return null;
    return changes.reduce(
      (total, change) => ({
        additions: total.additions + change.additions,
        deletions: total.deletions + change.deletions,
      }),
      { additions: 0, deletions: 0 },
    );
  }

  function activityDetail(activity: TimelineActivityRow): string | null {
    if (activity.title !== "thread_reverted") return activity.detail;
    const value = activity.metadata?.value;
    if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
    const count = "revertedTurnCount" in value && typeof value.revertedTurnCount === "number"
      ? value.revertedTurnCount
      : 0;
    const action = "providerHistoryAction" in value && value.providerHistoryAction === "rolled_back"
      ? t("chat.timeline.providerHistoryRolledBack")
      : t("chat.timeline.providerHistoryForkRequired");
    return t("chat.timeline.threadRestoredDetail", formatNumber(localization.locale, count), action);
  }

  function rowAriaLabel(row: TimelineDisplayRow): string {
    if (row.kind === "message") {
      const role = row.role === "user" ? t("chat.timeline.userMessage") : t("chat.timeline.assistantMessage");
      return `${role}, ${timestampLabel(row.createdAt)}`;
    }
    if (row.kind === "activity") return `${activitySummaryTitle([row])}, ${statusLabel(row.status)}`;
    if (row.kind === "activity_group") return `${activityGroupTitle([...row.earlierRows, row.latest])}, ${statusLabel(row.latest.status)}`;
    if (row.kind === "turn_fold") return foldLabel(row);
    return t("chat.timeline.plan");
  }

  function messageImages(message: TimelineMessageRow): { id: string; displayName: string; byteSize: number | null }[] {
    const images: { id: string; displayName: string; byteSize: number | null }[] = [];
    for (const attachment of message.userContext?.attachments ?? []) {
      if (attachment.kind !== "image") continue;
      images.push({ id: attachment.attachmentId, displayName: attachment.displayName, byteSize: attachment.byteSize });
    }
    return images;
  }

  function hasMessageContextChips(message: TimelineMessageRow): boolean {
    const context = message.userContext;
    return Boolean(context && (
      context.attachments.some((attachment) => attachment.kind !== "image")
      || context.mentions.length > 0
      || context.terminalContext.length > 0
    ));
  }

  function isModelOwnedRow(row: TimelineDisplayRow): boolean {
    return row.turnId !== null && !(row.kind === "message" && row.role === "user");
  }

  function modelIdentityForRow(row: TimelineDisplayRow): TimelineModelIdentity {
    const sourceThreadId = row.kind === "activity_group"
      ? row.latest.sourceThreadId
      : row.kind === "turn_fold"
        ? row.hiddenRows[0]?.sourceThreadId
        : row.sourceThreadId;
    const sourceThread = [executionThread, ...chat.activeThreads, ...chat.archivedThreads]
      .filter((thread): thread is ChatThreadShellRead => thread !== null)
      .find((thread) => thread.id === sourceThreadId)
      ?? selectedThread;
    const sourceProvider = chat.settings?.providerInstances.find((provider) => (
      provider.configuration.instanceId === sourceThread?.providerInstanceId
    )) ?? selectedProvider;
    const turn = row.turnId ? turnsById.get(row.turnId) : null;
    const modelId = row.kind === "message" && row.metadata?.modelId
      ? row.metadata.modelId
      : turn?.effectiveModelId ?? turn?.modelId ?? sourceThread?.modelId ?? null;
    const familyId = sourceThread?.providerFamilyId
      ?? sourceProvider?.configuration.familyId
      ?? "opencode";
    return {
      model: chatModelParticipant(familyId, modelId, sourceProvider?.modelCatalog ?? null),
    };
  }

  async function retryTimeline(): Promise<void> {
    try {
      if (chat.selectedChannelId) await chat.selectChannel(chat.selectedChannelId);
      else if (selectedThread) chat.selectThread(selectedThread.id);
      operationError = null;
    } catch (error: unknown) {
      reportError(error);
      throw error;
    }
  }

  /** Probes the provider again and rejects while it still reports an unhealthy state. */
  async function retryProvider(instanceId: ProviderInstanceId): Promise<void> {
    let result: ProviderProbeResult;
    try {
      result = await chat.probeProvider(instanceId);
    } catch (error: unknown) {
      reportError(error);
      throw error;
    }
    if (result.state !== "healthy") throw new Error(`Chat provider probe reported ${result.state}`);
  }
</script>

{#snippet activityIcon(activity: TimelineActivityRow)}
  {#if isFileSearchActivity(activity)}
    <Search size={15} />
  {:else if isFileReadActivity(activity)}
    <FileText size={15} />
  {:else if activity.activityKind === "command_execution" || activity.activityKind === "command_output"}
    <Terminal size={15} />
  {:else if activity.activityKind === "file_change" || activity.activityKind === "file_change_output"}
    <FileText size={15} />
  {:else if activity.activityKind === "web_search"}
    <Globe size={15} />
  {:else if isImageViewActivity(activity)}
    <ImageIcon size={15} />
  {:else}
    <Wrench size={15} />
  {/if}
{/snippet}

{#snippet activityHistoryRow(activity: TimelineActivityRow, simplified: boolean)}
  {@const changeCounts = simplified ? null : fileChangeLineCounts(activity)}
  {#if timelineActivitySupportsDisclosure(activity)}
    {@const activityExpanded = expandedActivities.includes(activity.id)}
    <div class="chat-process-step disclosure" class:active={activityIsInProgress(activity)} class:failed={activity.status === "failed"}>
      <button type="button" class="chat-process-step-trigger" data-timeline-disclosure-expanded={activityExpanded} aria-expanded={activityExpanded} onclick={() => { expandedActivities = toggle(expandedActivities, activity.id); }}>
        {@render activityIcon(activity)}
        <span class="chat-process-step-label">
          <span>{simplified ? activitySummaryTitle([activity]) : activityTitle(activity)}</span>
          {#if changeCounts && (changeCounts.additions > 0 || changeCounts.deletions > 0)}<small class="chat-file-change-counts">{#if changeCounts.additions > 0}<span class="additions">+{formatNumber(localization.locale, changeCounts.additions)}</span>{/if}{#if changeCounts.deletions > 0}<span class="deletions">−{formatNumber(localization.locale, changeCounts.deletions)}</span>{/if}</small>{/if}
          <ChevronRight class={activityExpanded ? "chat-step-chevron expanded" : "chat-step-chevron"} size={14} />
        </span>
      </button>
      <div class="chat-disclosure-region" class:expanded={activityExpanded} aria-hidden={!activityExpanded} inert={!activityExpanded}>
        <div class="chat-disclosure-inner">
          <div class="chat-step-detail">
            <ChatActivityDetail {activity} detail={activityDetail(activity)} />
          </div>
        </div>
      </div>
    </div>
  {:else}
    <div class="chat-process-step" class:active={activityIsInProgress(activity)} class:failed={activity.status === "failed"} class:thinking={activityIsThinking(activity)}>
      {#if !activityIsThinking(activity)}{@render activityIcon(activity)}{/if}
      <span>{simplified ? activitySummaryTitle([activity]) : activityTitle(activity)}</span>
      {#if changeCounts && (changeCounts.additions > 0 || changeCounts.deletions > 0)}<small class="chat-file-change-counts">{#if changeCounts.additions > 0}<span class="additions">+{formatNumber(localization.locale, changeCounts.additions)}</span>{/if}{#if changeCounts.deletions > 0}<span class="deletions">−{formatNumber(localization.locale, changeCounts.deletions)}</span>{/if}</small>{/if}
    </div>
  {/if}
{/snippet}

{#snippet modelRowContent(row: TimelineDisplayRow)}
  {#if row.kind === "message"}
    {@const message = row as TimelineMessageRow}
    {@const sourceThreadId = message.sourceThreadId ?? selectedThread?.id ?? chat.selectedThreadId}
    {@const sourceWorkingFolderId = sourceThreadId
      ? [executionThread, ...chat.activeThreads, ...chat.archivedThreads]
        .filter((thread): thread is ChatThreadShellRead => thread !== null)
        .find((thread) => thread.id === sourceThreadId)?.workingFolderId
        ?? (sourceThreadId === selectedThread?.id ? selectedThread.workingFolderId : null)
      : null}
    <article class="chat-assistant-message">
      <ChatMarkdown markdown={message.markdown} onError={reportError} />
      {#if message.turnId && message.metadata?.changedFiles.length}
        <ChatChangedFilesSummary
          turnId={message.turnId}
          files={message.metadata.changedFiles}
          {sourceThreadId}
          {sourceWorkingFolderId}
        />
      {/if}
      {#if message.state === "complete"}
        <ChatMessageReactionList target={executionMessageActionTarget(message)} />
      {/if}
    </article>
  {:else if row.kind === "activity"}
    {@const activity = row as TimelineActivityRow}
    {@render activityHistoryRow(activity, true)}
  {:else if row.kind === "activity_group"}
    {@const group = row as TimelineActivityGroupRow}
    {@const activities = [...group.earlierRows, group.latest]}
    {@const currentActivity = activityGroupCurrent(activities)}
    {@const representativeActivity = currentActivity ?? activities[0] ?? group.latest}
    <button type="button" class="chat-process-toggle" class:active={currentActivity !== null} data-timeline-disclosure-expanded={group.expanded} aria-expanded={group.expanded} onclick={() => { expandedGroups = toggle(expandedGroups, group.id); }}>
      {@render activityIcon(representativeActivity)}
      <span class="chat-process-toggle-label">
        <span>{activityGroupTitle(activities)}</span>
        <ChevronRight class={group.expanded ? "chat-step-chevron expanded" : "chat-step-chevron"} size={14} />
      </span>
    </button>
    <div class="chat-disclosure-region" class:expanded={group.expanded} aria-hidden={!group.expanded} inert={!group.expanded}>
      <div class="chat-disclosure-inner">
        <div class="chat-process-history">{#each activities as activity}{@render activityHistoryRow(activity, false)}{/each}</div>
      </div>
    </div>
  {:else if row.kind === "turn_fold"}
    {@const fold = row as TimelineTurnFoldRow}
    {@const activities = foldActivities(fold)}
    {#if fold.hiddenRows.length > 0}
      <button type="button" class="chat-process-toggle" class:failed={fold.state === "failed"} data-timeline-disclosure-expanded={fold.expanded} aria-expanded={fold.expanded} onclick={() => { expandedTurns = toggle(expandedTurns, fold.turnId); }}>
        {#if activities[0]}{@render activityIcon(activities[0])}{/if}
        <span class="chat-process-toggle-label">
          <span>{foldLabel(fold)}</span>
          <ChevronRight class={fold.expanded ? "chat-step-chevron expanded" : "chat-step-chevron"} size={14} />
        </span>
      </button>
    {:else}
      <div class="chat-process-toggle chat-process-summary" class:failed={fold.state === "failed"}>
        <span>{foldLabel(fold)}</span>
      </div>
    {/if}
    <div class="chat-disclosure-region" class:expanded={fold.expanded} aria-hidden={!fold.expanded} inert={!fold.expanded}>
      <div class="chat-disclosure-inner">
        <div class="chat-process-history chat-turn-history">{#each fold.hiddenRows as hiddenRow}{#if hiddenRow.kind === "activity"}{@render activityHistoryRow(hiddenRow, false)}{:else}{@render modelRowContent(hiddenRow)}{/if}{/each}</div>
      </div>
    </div>
  {:else if row.kind === "plan"}
    {@const plan = row as TimelinePlanRow}
    <article class="chat-plan">
      <h3><ListChecks size={16} />{t("chat.timeline.plan")}</h3>
      <ChatMarkdown markdown={plan.markdown} onError={reportError} />
      {#if plan.steps.length > 0}<ol>{#each plan.steps as step}<li><span>{step.text}</span><small>{statusLabel(step.status)}</small></li>{/each}</ol>{/if}
      <div class="chat-plan-actions"><button type="button" onclick={() => window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-continue-plan", { detail: { planId: plan.id } }))}>{t("chat.timeline.continuePlanning")}</button><button type="button" onclick={() => window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-implement-plan", { detail: { planId: plan.id } }))}>{t("chat.timeline.implementPlan")}</button><button type="button" onclick={() => { dismissedPlans = [...dismissedPlans, plan.id]; }}>{t("chat.timeline.dismiss")}</button></div>
    </article>
  {/if}
{/snippet}

<div class="chat-execution-timeline relative min-h-0 flex-1" class:embedded>
  {#if !embedded}
    {#if selectedThread?.archivedAt}
      <div class="chat-timeline-banner">
        <Notice class="chat-timeline-banner-notice" role="status" icon={Archive} message={t("chat.firstUse.archivedDescription")} copy={false}>
          {#snippet actions()}<button type="button" class="notice-action-text" onclick={() => void chat.restoreThread(selectedThread).catch(reportError)}>{t("chat.restore")}</button>{/snippet}
        </Notice>
      </div>
    {/if}
    {#if selectedWorkingFolder?.workingFolder.archivedAt}
      <div class="chat-timeline-banner">
        <Notice class="chat-timeline-banner-notice" role="status" tone="warning" icon={FolderArchive} message={t("chat.timeline.workingFolderArchived")} copy={false}>
          {#snippet actions()}<button type="button" class="notice-action-text" onclick={() => void chat.restoreWorkingFolder(selectedWorkingFolder.workingFolder.id).catch(reportError)}>{t("chat.restore")}</button>{/snippet}
        </Notice>
      </div>
    {:else if selectedWorkingFolder && selectedWorkingFolder.bindingStatus !== "available"}
      <div class="chat-timeline-banner">
        <Notice class="chat-timeline-banner-notice" role="status" tone="warning" icon={FolderX} message={t("chat.timeline.workspaceMissing")} copy={false}>
          {#snippet actions()}
            {#if selectedWorkingFolder.workingFolder.kind === "managed"}<button type="button" class="notice-action-text" onclick={() => void chat.recreateManagedWorkingFolder(selectedWorkingFolder.workingFolder.id).catch(reportError)}>{t("chat.firstUse.recreateFolder")}</button>{:else}<button type="button" class="notice-action-text" onclick={() => void chat.rebindWorkingFolder(selectedWorkingFolder.workingFolder.id, t("chat.firstUse.chooseWorkingFolder")).catch(reportError)}>{t("chat.timeline.rebind")}</button>{/if}
          {/snippet}
        </Notice>
      </div>
    {/if}
    {#if selectedProvider && (!selectedProvider.configuration.enabled || selectedProvider.lastProbe?.state !== "healthy")}
      <div class="chat-timeline-banner">
        <Notice
          class="chat-timeline-banner-notice"
          role="status"
          tone="warning"
          icon={Unplug}
          message={selectedProvider.lastProbe?.detail ?? t("chat.status.providerUnavailable")}
          retryLabel={t("chat.timeline.retry")}
          onRetry={() => retryProvider(selectedProvider.configuration.instanceId)}
          copy={selectedProvider.lastProbe?.detail ?? false}
        >
          {#snippet actions()}<button type="button" class="notice-action-text" onclick={() => settings.open("chat", { chatSubsection: "providers" })}>{t("chat.timeline.openSettings")}</button>{/snippet}
        </Notice>
      </div>
    {/if}
    {#if selectedThread?.state === "error"}
      <div class="chat-timeline-banner">
        <Notice class="chat-timeline-banner-notice" message={t("chat.timeline.threadError")} copy={false}>
          {#snippet actions()}<button type="button" class="notice-action-text" onclick={() => void chat.startNewChannelSession().catch(reportError)}>{t("chat.timeline.startNewThread")}</button>{/snippet}
        </Notice>
      </div>
    {/if}
  {/if}
  {#if operationError || !timelinePage && chat.timelineError}
    <div class="chat-timeline-banner">
      <Notice class="chat-timeline-banner-notice" message={operationError ?? chat.timelineError ?? ""} retryLabel={t("chat.timeline.retry")} onRetry={selectedThread || chat.selectedChannelId ? retryTimeline : undefined} />
    </div>
  {/if}
  <div bind:this={scroller} class="chat-timeline-scroller h-full overflow-y-auto" role="feed" aria-busy={timelineLoading || undefined} aria-label={t("chat.title")} onscroll={handleScroll} onpointerleave={clearActionToolbar}>
    <div
      bind:this={timelineContent}
      class="chat-timeline-content mx-auto flex min-h-full flex-col justify-end"
      style={`padding-top:${embedded ? "var(--chat-conversation-entry-space, 0.45rem)" : `${virtualWindow.paddingTop + TIMELINE_EDGE_PADDING_PX}px`};padding-bottom:${embedded ? "var(--chat-conversation-entry-space, 0.45rem)" : `${bottomPadding}px`}`}
    >
      {#if loadingOlder}<div class="mb-3 flex justify-center text-xs text-muted-foreground"><LoaderCircle size={14} class="animate-spin" />{t("chat.timeline.loadingOlder")}</div>{/if}
      {#if initialTimelineLoadingVisible}<div class="py-12 text-center text-sm text-muted-foreground">{t("common.loading")}</div>{/if}
      {#each renderedRows as virtual (virtual.row.id)}
        {@const row = virtual.row}
        <div
          data-timeline-row-id={row.id}
          class="chat-timeline-row"
          class:optimistic={row.id === optimisticMessage?.id}
          class:participant-start={row.kind === "message" && row.role === "user" || modelGroupStartIds.has(row.id)}
          class:model-owned={isModelOwnedRow(row)}
          role="article"
          aria-label={rowAriaLabel(row)}
          aria-posinset={virtual.index + 1}
          aria-setsize={displayRows.length}
          tabindex="-1"
          onpointerenter={() => {
            if (isModelOwnedRow(row) && row.turnId) showModelActionToolbar(row.turnId);
            else if (row.kind === "message" && row.role === "user") showUserActionToolbar(row.id);
            else clearActionToolbar();
          }}
          onpointerleave={() => {
            if (row.kind === "message" && row.role === "user" && hoveredActionMessageId === row.id) {
              hoveredActionMessageId = null;
            }
          }}
        >
          {#if row.kind === "message"}
            {@const message = row as TimelineMessageRow}
            {@const messageExpanded = expandedMessages.includes(message.id)}
            {#if message.role === "user"}
              {@const userDisplayName = preferences.profileDisplayName || t("chat.timeline.you")}
              <div class="chat-participant-row">
                <ChatIdentityButton participant={localParticipant} presentation="avatar" size={36} />
                <div class="chat-participant-content">
                  <div class="chat-participant-header"><strong><ChatIdentityButton participant={localParticipant} presentation="name" triggerLabel={userDisplayName} /></strong><span title={t("chat.timeline.timestamp")}>{timestampLabel(message.createdAt)}</span></div>
                  <article class="chat-user-message">
                    <div use:measureExpandableHeight class="chat-message-expandable" data-selectable-content class:collapsed={message.markdown.length > 1200 && !messageExpanded} class:expanded={messageExpanded}><div><p class="wrap-break-word whitespace-pre-wrap">{message.markdown}</p></div></div>
                    {#if messageImages(message).length > 0}<ChatImageGallery images={messageImages(message)} />{/if}
                    {#if message.userContext && hasMessageContextChips(message)}<div class="chat-user-context">{#each message.userContext.attachments.filter((attachment) => attachment.kind !== "image") as attachment}<button type="button" title={attachment.status} onclick={() => copy(attachment.displayName)}><FileText size={12} /><span>{attachment.displayName}</span><small>{formatNumber(localization.locale, attachment.byteSize)} B</small></button>{/each}{#each message.userContext.mentions as mention}<button type="button" title={t("chat.timeline.mention")} onclick={() => copy(mention.relativePath)}><span>@</span><span>{mention.relativePath}</span></button>{/each}{#each message.userContext.terminalContext as context}<button type="button" title={t("chat.timeline.terminalContext")} onclick={() => copy(context)}><Terminal size={12} /><span>{context}</span></button>{/each}</div>{/if}
                    <ChatMessageReactionList target={executionMessageActionTarget(message)} />
                    {#if message.markdown.length > 1200 || message.userContext?.preCheckpointId && (!message.sourceThreadId || message.sourceThreadId === chat.selectedThreadId)}
                      <div class="chat-message-secondary-actions text-muted-foreground">
                        {#if message.markdown.length > 1200}<button type="button" data-timeline-disclosure-expanded={messageExpanded} aria-expanded={messageExpanded} onclick={() => { expandedMessages = toggle(expandedMessages, message.id); }}>{messageExpanded ? t("chat.timeline.showLess") : t("chat.timeline.showMore")}</button>{/if}
                        {#if message.userContext?.preCheckpointId && (!message.sourceThreadId || message.sourceThreadId === chat.selectedThreadId)}<button type="button" class="inline-flex items-center gap-1" onclick={() => window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-revert-message", { detail: { threadId: message.sourceThreadId ?? chat.selectedThreadId, checkpointId: message.userContext?.preCheckpointId, turnId: message.turnId } }))}><RotateCcw size={11} />{t("chat.timeline.revert")}</button>{/if}
                      </div>
                    {/if}
                    <ChatMessageActionToolbar
                      target={executionMessageActionTarget(message)}
                      visible={hoveredActionMessageId === message.id}
                      onError={reportError}
                    />
                  </article>
                </div>
              </div>
            {:else if modelGroupStartIds.has(row.id)}
              {@const identity = modelIdentityForRow(row)}
              {@const actionMessage = row.turnId ? assistantActionMessages.get(row.turnId) : undefined}
              <div class="chat-participant-row">
                <ChatIdentityButton participant={teammate} model={identity.model} presentation="avatar" size={embedded ? 34 : 36} currentResponseSettings={embedded} />
                <div class="chat-participant-content"><div class="chat-participant-header"><strong><ChatIdentityButton participant={teammate} model={identity.model} presentation="name" triggerLabel={teammate?.displayName ?? identity.model.displayName} currentResponseSettings={embedded} /></strong><span>{timestampLabel(row.createdAt)}</span></div>{#if actionMessage}<ChatMessageActionToolbar target={executionMessageActionTarget(actionMessage)} visible={hoveredActionTurnId === row.turnId} onError={reportError} />{/if}{@render modelRowContent(row)}</div>
              </div>
            {:else}
              <div class="chat-participant-followup">{@render modelRowContent(row)}</div>
            {/if}
          {:else if isModelOwnedRow(row)}
            {#if modelGroupStartIds.has(row.id)}
              {@const identity = modelIdentityForRow(row)}
              {@const actionMessage = row.turnId ? assistantActionMessages.get(row.turnId) : undefined}
              <div class="chat-participant-row">
                <ChatIdentityButton participant={teammate} model={identity.model} presentation="avatar" size={embedded ? 34 : 36} currentResponseSettings={embedded} />
                <div class="chat-participant-content"><div class="chat-participant-header"><strong><ChatIdentityButton participant={teammate} model={identity.model} presentation="name" triggerLabel={teammate?.displayName ?? identity.model.displayName} currentResponseSettings={embedded} /></strong><span>{timestampLabel(row.createdAt)}</span></div>{#if actionMessage}<ChatMessageActionToolbar target={executionMessageActionTarget(actionMessage)} visible={hoveredActionTurnId === row.turnId} onError={reportError} />{/if}{@render modelRowContent(row)}</div>
              </div>
            {:else}
              <div class="chat-participant-followup">{@render modelRowContent(row)}</div>
            {/if}
          {:else}
            {@render modelRowContent(row)}
          {/if}
        </div>
      {/each}
    </div>
  </div>
  <div
    class="chat-timeline-scrollbar"
    class:visible={scrollbarGeometry !== null}
    style={`top:${scrollbarTrackTop}px;height:${scrollbarTrackHeight}px`}
    aria-hidden="true"
    onpointerdown={handleScrollbarTrackPointerDown}
    onwheel={handleScrollbarWheel}
  >
    {#if scrollbarGeometry}
      <div
        class="chat-timeline-scrollbar-thumb"
        class:dragging={scrollbarDrag !== null}
        role="presentation"
        style={`height:${scrollbarGeometry.size}px;transform:translateY(${scrollbarGeometry.offset}px)`}
        onpointerdown={handleScrollbarThumbPointerDown}
        onpointermove={handleScrollbarThumbPointerMove}
        onpointerup={finishScrollbarDrag}
        onpointercancel={finishScrollbarDrag}
        onlostpointercapture={finishScrollbarDrag}
      ></div>
    {/if}
  </div>
  {#if showMinimap}<nav class="chat-timeline-minimap" aria-label={t("chat.timeline.minimap")}>{#each minimapRows as row}<button type="button" class:user={row.kind === "message" && row.role === "user"} class:assistant={row.kind === "message" && row.role === "assistant"} class:error={row.kind === "activity"} class:current={row.sequence >= (virtualWindow.items[0]?.row.sequence ?? Number.MAX_SAFE_INTEGER) && row.sequence <= (virtualWindow.items.at(-1)?.row.sequence ?? Number.MIN_SAFE_INTEGER)} title={minimapLabel(row)} aria-label={t("chat.timeline.minimapRow", minimapLabel(row))} onclick={() => scrollToMinimapRow(row.id)}></button>{/each}</nav>{/if}
  {#if !embedded && intent !== "following"}<button type="button" class="chat-jump-latest" onclick={jumpToLatest}><ArrowDown size={13} />{t("chat.timeline.jumpLatest")}{#if unreadEvents > 0}<span>{unreadEvents}</span>{/if}</button>{/if}
</div>

<style>
  .chat-execution-timeline.embedded { --chat-conversation-font-size:var(--chat-organizational-font-size,calc(0.875rem * var(--type-scale))); --chat-conversation-line-height:var(--chat-organizational-line-height,calc(1.3125rem * var(--type-scale))); min-height:0; flex:none; overflow:visible; }
  .chat-execution-timeline.embedded .chat-timeline-scroller { height:auto; overflow:visible; }
  .chat-execution-timeline.embedded .chat-timeline-content { width:100%; min-height:0; justify-content:flex-start; }
  .chat-execution-timeline.embedded .chat-timeline-row { --chat-participant-gap:0.65rem; margin:0; padding:0 1rem; }
  .chat-execution-timeline.embedded .chat-timeline-row.participant-start { margin-top:0; }
  .chat-execution-timeline.embedded .chat-participant-row { grid-template-columns:34px minmax(0,1fr); }
  .chat-execution-timeline.embedded .chat-participant-followup { margin-left:calc(34px + var(--chat-participant-gap)); }
  .chat-execution-timeline.embedded .chat-participant-header span { font-size:var(--chat-organizational-time-font-size,calc(0.6875rem * var(--type-scale))); }
  .chat-timeline-content { width:min(calc(100% - var(--chat-conversation-gutter,1rem) - var(--chat-conversation-gutter,1rem)),var(--chat-conversation-max-width,60rem)); }
  .chat-timeline-scroller { overflow-anchor: none; scrollbar-width: none; }
  .chat-timeline-scroller::-webkit-scrollbar { display: none; width: 0; height: 0; }
  .chat-timeline-scrollbar { pointer-events: none; position: absolute; right: 0; z-index: 30; width: 8px; contain: strict; opacity: 0; touch-action: none; }
  .chat-timeline-scrollbar.visible { pointer-events: auto; opacity: 1; }
  .chat-timeline-scrollbar-thumb { position: absolute; left: 2px; width: 4px; contain: strict; border-radius: 9999px; background-color: var(--app-scrollbar-thumb); cursor: default; will-change: height, transform; }
  .chat-timeline-scrollbar-thumb:hover, .chat-timeline-scrollbar-thumb.dragging { background-color: var(--app-scrollbar-thumb-hover); }
  .chat-timeline-row { --chat-participant-gap: 0.75rem; border-radius: 0.4rem; }
  .chat-timeline-row.optimistic { animation: chat-message-in 140ms ease-out; }
  .chat-timeline-row.participant-start { margin-top: 1.1rem; }
  .chat-timeline-row:first-child { margin-top: 0; }
  .chat-participant-row { display: grid; min-width: 0; grid-template-columns: 36px minmax(0, 1fr); align-items: start; gap: var(--chat-participant-gap); }
  .chat-participant-content { --chat-message-action-anchor-bottom:1.25rem; position:relative; min-width:0; transform:translateY(-4px); }
  .chat-participant-followup { min-width: 0; margin-left: calc(36px + var(--chat-participant-gap)); }
  .chat-participant-header { display: flex; min-height: 1.25rem; min-width: 0; align-items: baseline; gap: 0.45rem; margin-bottom: 0.12rem; line-height: calc(1.25rem * var(--type-scale)); }
  .chat-participant-header strong { min-width: 0; overflow: hidden; color: var(--foreground); font-size: var(--chat-conversation-font-size, calc(0.875rem * var(--type-scale))); font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .chat-participant-header span { flex: 0 0 auto; color: var(--muted-foreground); font-size: calc(0.6875rem * var(--type-scale)); font-weight: 400; }
  .chat-timeline-banner { display: flex; min-height: 2.25rem; align-items: center; border-bottom: 1px solid var(--border); background: var(--background); padding-block: 0.4rem; font-size: calc(0.733333rem * var(--type-scale)); }
  .chat-timeline-banner > :global(.chat-timeline-banner-notice) { width: min(calc(100% - var(--chat-conversation-gutter,1rem) - var(--chat-conversation-gutter,1rem)),var(--chat-conversation-max-width,60rem)); margin-inline: auto; }
  .chat-user-message, .chat-assistant-message { width:100%; min-width:0; color:var(--foreground); font-size:var(--chat-conversation-font-size, calc(0.875rem * var(--type-scale))); line-height:var(--chat-conversation-line-height, calc(1.3125rem * var(--type-scale))); }
  .chat-message-secondary-actions { display:flex; flex-wrap:wrap; align-items:center; gap:0.75rem; margin-top:0.4rem; font-size: calc(0.7rem * var(--type-scale)); }
  .chat-user-context { display: flex; flex-wrap: wrap; gap: 0.3rem; margin-top: 0.55rem; }
  .chat-user-context button { display: inline-flex; max-width: 100%; align-items: center; gap: 0.3rem; border: 1px solid var(--border); border-radius: 999px; padding: 0.18rem 0.45rem; color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  .chat-user-context button span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chat-user-context small { font-size: inherit; opacity: 0.8; }
  .chat-message-expandable { --chat-expanded-height: none; position: relative; max-height: var(--chat-expanded-height); overflow: hidden; transition: max-height 420ms cubic-bezier(0.22, 1, 0.36, 1); }
  .chat-message-expandable.collapsed { max-height: 14rem; }
  .chat-message-expandable::after { position: absolute; inset: auto 0 0; height: 3rem; background: linear-gradient(transparent, var(--cal-bg)); content: ""; opacity: 0; pointer-events: none; transition: opacity 220ms ease; }
  .chat-message-expandable.collapsed::after { opacity: 1; }
  .chat-process-toggle { display: flex; width: 100%; min-height: var(--chat-process-line-height, calc(1.1875rem * var(--type-scale))); align-items: center; gap: 0.4rem; color: var(--muted-foreground); font-size: var(--chat-process-font-size, calc(0.8125rem * var(--type-scale))); line-height: var(--chat-process-line-height, calc(1.1875rem * var(--type-scale))); text-align: left; }
  .chat-process-toggle-label { display: inline-flex; width: fit-content; min-width: 0; max-width: 100%; align-items: center; gap: 0.25rem; }
  .chat-process-toggle-label > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chat-process-toggle :global(svg) { flex: 0 0 auto; transition: color 120ms ease; }
  .chat-process-toggle:hover { color: var(--foreground); }
  .chat-process-toggle:focus-visible { border-radius: 0.25rem; outline: 2px solid var(--ring); outline-offset: 2px; }
  .chat-process-toggle.failed, .chat-process-step.failed { color: var(--destructive); }
  .chat-process-summary { padding-left: 0; }
  .chat-process-history { display:grid; min-width:0; gap:var(--chat-conversation-tight-space,0.125rem); margin-block:var(--chat-conversation-tight-space,0.125rem) 0; color:var(--muted-foreground); }
  .chat-process-step { display:grid; width:100%; min-width:0; grid-template-columns:1rem minmax(0,1fr) 1rem; align-items:start; gap:0.45rem; color:var(--muted-foreground); font-size:var(--chat-process-font-size,calc(0.8125rem * var(--type-scale))); line-height:var(--chat-process-line-height,calc(1.1875rem * var(--type-scale))); }
  .chat-process-step.disclosure { display: block; }
  .chat-process-step.thinking { grid-template-columns: minmax(0, 1fr); }
  .chat-process-step > span { width: fit-content; min-width: 0; max-width: 100%; justify-self: start; overflow-wrap: anywhere; }
  .chat-process-step-trigger { display: grid; width: 100%; min-width: 0; cursor: pointer; grid-template-columns: 1rem minmax(0, 1fr); align-items: start; gap: 0.45rem; text-align: left; }
  .chat-process-step-label { display: inline-flex; width: fit-content; min-width: 0; max-width: 100%; align-items: flex-start; gap: 0.25rem; justify-self: start; }
  .chat-process-step-label > span { min-width: 0; overflow-wrap: anywhere; }
  .chat-file-change-counts { display: inline-flex; flex: 0 0 auto; gap: 0.25rem; font-size: calc(0.7rem * var(--type-scale)); }
  .chat-file-change-counts .additions { color: var(--action-confirm); }
  .chat-file-change-counts .deletions { color: var(--destructive); }
  .chat-process-step-label :global(svg) { flex: 0 0 auto; margin-top: calc((var(--chat-process-line-height, calc(1.1875rem * var(--type-scale))) - 0.875rem) / 2); }
  .chat-process-step.active, .chat-process-toggle.active { color: color-mix(in srgb, var(--muted-foreground) 78%, var(--foreground)); }
  .chat-process-step.active > span, .chat-process-step.active .chat-process-step-label > span, .chat-process-toggle.active .chat-process-toggle-label > span {
    animation: chat-process-shimmer 6s ease-in-out infinite;
    background: linear-gradient(100deg, var(--muted-foreground) 0%, var(--muted-foreground) 42%, var(--foreground) 50%, var(--muted-foreground) 58%, var(--muted-foreground) 100%);
    background-repeat: no-repeat;
    background-size: 320% 100%;
    background-clip: text;
    color: transparent;
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
  }
  .chat-process-step-trigger:focus-visible { border-radius: 0.25rem; outline: 2px solid var(--ring); outline-offset: 2px; }
  :global(.chat-step-chevron.expanded) { transform: rotate(90deg); }
  :global(.chat-step-chevron) { transition: opacity 120ms ease, transform 120ms ease; }
  .chat-turn-history [aria-expanded="false"] :global(.chat-step-chevron) { opacity: 0; }
  .chat-turn-history [aria-expanded="false"]:hover :global(.chat-step-chevron),
  .chat-turn-history [aria-expanded="false"]:focus-visible :global(.chat-step-chevron) { opacity: 1; }
  .chat-disclosure-region { display: grid; grid-template-rows: 0fr; opacity: 0; transition: grid-template-rows 420ms cubic-bezier(0.22, 1, 0.36, 1), opacity 180ms ease; }
  .chat-disclosure-region.expanded { grid-template-rows: 1fr; opacity: 1; transition: grid-template-rows 420ms cubic-bezier(0.22, 1, 0.36, 1), opacity 240ms ease 55ms; }
  .chat-disclosure-inner { min-height: 0; overflow: hidden; transform: translateY(-0.3rem); transition: transform 360ms cubic-bezier(0.22, 1, 0.36, 1); }
  .chat-disclosure-region.expanded > .chat-disclosure-inner { transform: translateY(0); }
  .chat-step-detail { box-sizing: border-box; width: calc(100% - 1.45rem); min-width: 0; max-width: calc(100% - 1.45rem); margin-top: 0.35rem; margin-left: 1.45rem; }
  .chat-plan { color: var(--foreground); font-size: var(--chat-conversation-font-size, calc(0.875rem * var(--type-scale))); line-height: var(--chat-conversation-line-height, calc(1.3125rem * var(--type-scale))); }
  .chat-plan h3 { display: flex; align-items: center; gap: 0.4rem; margin-bottom: 0.35rem; font-weight: 650; }
  .chat-plan ol { display: grid; gap: 0.2rem; margin-top: 0.55rem; }
  .chat-plan li { display: flex; align-items: baseline; gap: 0.5rem; }
  .chat-plan li small { margin-left: auto; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
  .chat-plan-actions { display: flex; flex-wrap: wrap; gap: 0.65rem; margin-top: 0.55rem; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); opacity: 0; transition: opacity 150ms ease; }
  .chat-plan:hover .chat-plan-actions, .chat-plan:focus-within .chat-plan-actions { opacity: 1; }
  .chat-timeline-minimap { position: absolute; right: 0.55rem; top: 3rem; bottom: 4rem; display: flex; width: 0.7rem; flex-direction: column; justify-content: space-evenly; gap: 1px; border-radius: 999px; background: color-mix(in srgb, var(--popover) 88%, transparent); padding: 0.2rem; box-shadow: var(--floating-shadow); }
  .chat-timeline-minimap button { min-height: 2px; flex: 1 1 2px; border-radius: 999px; background: var(--muted-foreground); opacity: 0.45; }
  .chat-timeline-minimap button.user { background: var(--primary); opacity: 0.8; }
  .chat-timeline-minimap button.assistant { background: var(--foreground); opacity: 0.65; }
  .chat-timeline-minimap button.error { background: var(--destructive); opacity: 0.9; }
  .chat-timeline-minimap button.current { outline: 1px solid var(--ring); opacity: 1; }
  .chat-timeline-minimap button:focus-visible { width: 0.8rem; outline: 2px solid var(--ring); }
  .chat-jump-latest { position: absolute; bottom: 1rem; left: 50%; display: inline-flex; min-height: 2.25rem; transform: translateX(-50%); align-items: center; gap: 0.4rem; border: 1px solid var(--border); border-radius: 999px; background: var(--popover); padding: 0.35rem 0.75rem; box-shadow: var(--floating-shadow); font-size: var(--panel-detail-font-size); }
  @keyframes chat-message-in { from { opacity: 0; transform: translateY(0.2rem); } to { opacity: 1; transform: translateY(0); } }
  @keyframes chat-process-shimmer { 0%, 8% { background-position: 100% 0; } 65%, 100% { background-position: 0% 0; } }
  @media (hover: none) { .chat-plan-actions { opacity:1; } }
  @media (prefers-reduced-motion: reduce) { .chat-timeline-row.optimistic, .chat-process-step.active > span, .chat-process-step.active .chat-process-step-label > span, .chat-process-toggle.active .chat-process-toggle-label > span { animation: none; background: none; color: inherit; -webkit-text-fill-color: currentColor; } .chat-process-toggle :global(svg), :global(.chat-step-chevron), .chat-disclosure-region, .chat-disclosure-inner, .chat-message-expandable, .chat-message-expandable::after { transition: none; } }
  @media (forced-colors: active) { .chat-process-step.active > span, .chat-process-step.active .chat-process-step-label > span, .chat-process-toggle.active .chat-process-toggle-label > span { animation: none; background: none; color: inherit; -webkit-text-fill-color: currentColor; } }
  @container chat-shell (max-width: 420px) {
    .chat-timeline-row { --chat-participant-gap: 0.5rem; }
    .chat-participant-header { flex-wrap: wrap; column-gap: 0.35rem; }
  }
</style>
