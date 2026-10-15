<script lang="ts">
  import { onMount, tick, untrack, type Snippet } from "svelte";
  import { quintOut } from "svelte/easing";
  import { slide } from "svelte/transition";
  import { chatHeaderActionInset, nextThreadIndex } from "$lib/chat/shell-model";
  import {
    CHAT_LOCAL_EXECUTION_COMPONENTS,
    listenForChatChanges,
    preloadChatLocalExecutionUi,
  } from "$lib/chat/workspace/local-execution-ui";
  import { inspectorFocusAction } from "$lib/chat/workspace/inspector-model";
  import {
    CHAT_OPEN_WORKSPACE_PANEL_EVENT,
    chatWorkspaceRequest,
  } from "$lib/chat/workspace/events";
  import {
    alignPanelSizeToDevicePixel,
    CHAT_AUXILIARY_CONVERSATION_MIN_PX,
    CHAT_REPLY_THREAD_WIDTH_PX,
    chatBottomPanelResizeMaximum,
    chatLayoutDecision,
    chatLayoutsEqual,
    chatInspectorResizeMaximum,
    fittedChatBottomPanelHeight,
    fittedChatInspectorWidth,
    panelSizeWithCollapseSnap,
    panelWidthFromKey,
    type ChatLayoutDecision,
  } from "$lib/chat/responsive-layout";
  import type { ChatPanelPreferences } from "$lib/chat/contracts";
  import { parseChatChangeNotification } from "$lib/chat/validation";
  import { hasOnlyShortcutModifier } from "$lib/keyboard-shortcuts";
  import { firstFocusable, trapTabFocus } from "$lib/chat/focus-navigation";
  import {
    isChatCheckpointRestoreRequest,
    restoreChatCheckpoint,
  } from "$lib/chat/review/checkpoint-restoration";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { suppressHoverOnToggle } from "$lib/utils/hover-suppression";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ChatWorkspaceHeader from "./ChatWorkspaceHeader.svelte";
  import ChatFirstUse from "./ChatFirstUse.svelte";
  import ChatHeaderActions from "./ChatHeaderActions.svelte";
  import ChatProjectSettingsPanel from "./ChatProjectSettingsPanel.svelte";
  import ChatChannelRail from "$lib/components/chat/channels/ChatChannelRail.svelte";
  import ChatChannelArchive from "$lib/components/chat/channels/ChatChannelArchive.svelte";
  import ChatChannelFeed from "$lib/components/chat/channels/ChatChannelFeed.svelte";
  import ChatReplyThreadPanel from "$lib/components/chat/timeline/ChatReplyThreadPanel.svelte";
  import { isTextEntryElement, OPENING_MAY_FOCUS_TEXT } from "$lib/mobile/keyboard";

  let {
    projectSidebar = undefined,
    projectViewSwitcher = undefined,
  }: {
    /**
     * Projects sidebar from a host that opens Chat as one place of a project. It replaces the channel rail, so the
     * header breadcrumb carries channel navigation. The snippet receives the handler for a project switch.
     */
    projectSidebar?: Snippet<[() => void]>;
    /**
     * Phone switcher between the views of the project that hosts Chat. The phone keeps the channel sheet, so hosting
     * there only adds the switcher to the action row.
     */
    projectViewSwitcher?: Snippet;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  const preferences = getPreferences();
  const projects = getProjects();
  const settings = getSettingsLauncher();
  const localExecutionAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "chat.local-execution",
  );
  const DEFAULT_INSPECTOR_WIDTH = 520;
  const MIN_INSPECTOR_WIDTH = 240;
  const MAX_INSPECTOR_WIDTH = 960;
  const DEFAULT_BOTTOM_PANEL_HEIGHT = 190;
  const MIN_BOTTOM_PANEL_HEIGHT = 96;
  const MAX_BOTTOM_PANEL_HEIGHT = 520;
  const PANEL_TRANSITION_MS = 440;
  const HEADER_ACTION_EDGE_GAP_PX = 12;
  const HEADER_PANEL_EDGE_GAP_PX = 8;
  const INITIAL_SHELL_WIDTH = 1_200;
  const INITIAL_SHELL_HEIGHT = 700;
  const INITIAL_FONT_SCALE = 1;
  const initialInspectorWidth = chat.settings?.configuration.panels.inspectorWidthPx
    ?? DEFAULT_INSPECTOR_WIDTH;
  let rootElement: HTMLDivElement | undefined = $state();
  let primaryHeaderElement: HTMLDivElement | undefined = $state();
  let globalActionsElement: HTMLDivElement | undefined = $state();
  let canvasEdgeElement: HTMLDivElement | undefined = $state();
  let railShell: HTMLDivElement | undefined = $state();
  let composerDockElement: HTMLDivElement | undefined = $state();
  let composerDockHeight = $state(0);
  let commandMenuOpen = $state(false);
  const LocalCommandMenu = CHAT_LOCAL_EXECUTION_COMPONENTS?.CommandMenu ?? null;
  const WorkspaceObserver = CHAT_LOCAL_EXECUTION_COMPONENTS?.WorkspaceObserver ?? null;
  const WorkspacePanel = CHAT_LOCAL_EXECUTION_COMPONENTS?.WorkspacePanel ?? null;
  let resizingInspector = $state(false);
  let resizingBottomPanel = $state(false);
  let inspectorWidth = $state(initialInspectorWidth);
  let bottomPanelHeight = $state(DEFAULT_BOTTOM_PANEL_HEIGHT);
  let bottomPanelOpen = $state(false);
  let bottomPanelMounted = $state(false);
  let bottomPanelSkipCloseTransition = $state(false);
  let reducedMotion = $state(false);
  let headerEditingTitle = $state(false);
  let projectSettingsOpen = $state(false);
  let projectSettingsTrigger = $state<HTMLButtonElement | null>(null);
  let projectSettingsProjectId = $state<string | null>(null);
  let inspectorWasOpen = false;
  let inspectorReturnFocus: HTMLElement | null = null;
  let replyThreadWasOpen = false;
  let replyThreadReturnFocus: HTMLElement | null = null;
  let transientRailOpen = $state(false);
  let mobileRailOpen = $state(false);
  let auxiliaryPairWasOpen = false;
  let railModalWasOpen = false;
  let railReturnFocus: HTMLElement | null = null;
  let shellWidth = $state(INITIAL_SHELL_WIDTH);
  let shellHeight = $state(INITIAL_SHELL_HEIGHT);
  let shellRight = $state(INITIAL_SHELL_WIDTH);
  let shellBottom = $state(INITIAL_SHELL_HEIGHT);
  let displayPixelRatio = $state(1);
  let canvasEdgeWidth = $state<number | null>(null);
  const fontScale = $derived(Math.max(INITIAL_FONT_SCALE, preferences.fontScale));
  let layout = $state<ChatLayoutDecision>(chatLayoutDecision({
    containerWidth: INITIAL_SHELL_WIDTH,
    containerHeight: INITIAL_SHELL_HEIGHT,
    fontScale: INITIAL_FONT_SCALE,
    railOpen: BUILD_PLATFORM_PROFILE.shell === "mobile" ? false : chat.railOpen,
    inspectorOpen: chat.inspectorOpen,
    inspectorWidth: initialInspectorWidth,
    replyThreadOpen: chat.openReplyThreadId !== null,
    replyThreadWidth: CHAT_REPLY_THREAD_WIDTH_PX,
  }));
  const bottomPanelVisible = $derived(
    bottomPanelOpen && layout.variant !== "minimum_recovery",
  );
  const auxiliaryPairOpen = $derived(chat.openReplyThreadId !== null && chat.inspectorOpen);
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const projectHosted = $derived(projectSidebar !== undefined && !mobileShell);
  const visibleRailOpen = $derived(
    projectHosted ? false : mobileShell ? mobileRailOpen : auxiliaryPairOpen ? transientRailOpen : chat.railOpen,
  );
  const observeRailOpenForHover = suppressHoverOnToggle();
  $effect(() => observeRailOpenForHover(visibleRailOpen));
  /** The phone shell opens the channel rail as a floating panel over the channel instead of a sheet. */
  const railUsesMobilePanel = $derived(
    mobileShell && layout.railPresentation === "sheet",
  );
  const railDocked = $derived(projectHosted || (layout.railPresentation === "column" && !mobileShell));
  const railIsModal = $derived(
    layout.railPresentation === "sheet" && !railUsesMobilePanel && visibleRailOpen,
  );
  const replyThreadWidth = $derived(CHAT_REPLY_THREAD_WIDTH_PX * fontScale);
  const displayedInspectorWidth = $derived.by(() => {
    if (!auxiliaryPairOpen) return inspectorWidth;
    const reservedWidth = CHAT_AUXILIARY_CONVERSATION_MIN_PX * fontScale + replyThreadWidth;
    return Math.min(inspectorWidth, Math.max(MIN_INSPECTOR_WIDTH, shellWidth - reservedWidth));
  });
  const threadFullSurface = $derived(layout.replyThreadPresentation === "main");
  const threadColumnOpen = $derived(layout.replyThreadPresentation === "column");
  let loadError = $state<string | null>(null);
  let initialLoadingVisible = $state(false);
  let layoutError = $state<string | null>(null);
  let politeAnnouncement = $state("");
  let assertiveAnnouncement = $state("");
  let announcedThreadId: string | null = null;
  let announcedTurnState: string | null = null;
  let announcedRequestId: string | null = null;
  let inspectorResizeFrame: number | null = null;
  let bottomResizeFrame: number | null = null;
  let inspectorResizeEndFrame: number | null = null;
  let bottomResizeEndFrame: number | null = null;
  let panelTransitionFrame: number | null = null;
  let initialLoadingTimer: number | null = null;
  let panelTransitionsEnabled = $state(false);
  type ResizableOuterPanel = "inspector" | "bottom";
  let snapTransitioning = $state<Record<ResizableOuterPanel, boolean>>({
    inspector: false,
    bottom: false,
  });
  const snapTransitionTimers: Partial<Record<ResizableOuterPanel, number>> = {};
  let pendingPanelWidths = $state<ChatPanelPreferences | null>(null);
  let panelWidthSave: Promise<void> | null = null;

  onMount(() => {
    const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    const updateMotionPreference = () => { reducedMotion = motionQuery.matches; };
    updateMotionPreference();
    motionQuery.addEventListener("change", updateMotionPreference);
    void Promise.all([chat.ensureLoaded(), projects.ensureLoaded()])
      .then(() => {
        requestAnimationFrame(preloadChatLocalExecutionUi);
      })
      .catch((error) => {
        loadError = error instanceof Error ? error.message : String(error);
      });
    refreshWorkspacePixelGeometry();
    inspectorWidth = alignInspectorWidthToDisplay(
      chat.settings?.configuration.panels.inspectorWidthPx ?? DEFAULT_INSPECTOR_WIDTH,
    );
    if (chat.settings) enablePanelTransitionsAfterLayout();
    const unlisten = listenForChatChanges((payload) => {
      try {
        const change = parseChatChangeNotification(payload);
        void chat.handleNativeChange(change.threadId).catch(() => undefined);
      } catch (error: unknown) {
        console.error("Invalid Chat change notification", error);
      }
    });
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      shellWidth = entry.contentRect.width;
      shellHeight = entry.contentRect.height;
      refreshWorkspacePixelGeometry();
      refreshHeaderActionInset();
    });
    if (rootElement) observer.observe(rootElement);
    const headerGeometryObserver = new ResizeObserver(refreshHeaderActionInset);
    if (primaryHeaderElement) headerGeometryObserver.observe(primaryHeaderElement);
    if (globalActionsElement) headerGeometryObserver.observe(globalActionsElement);
    refreshHeaderActionInset();
    const revertMessage = (event: Event) => {
      if (!(event instanceof CustomEvent) || !isChatCheckpointRestoreRequest(event.detail)) return;
      void restoreMessageCheckpoint(event.detail.threadId, event.detail.checkpointId);
    };
    const openWorkspaceTool = (event: Event) => { openInspectorWorkspaceForEvent(event); };
    const openTeammates = (event: Event) => {
      const participantId = event instanceof CustomEvent
        && typeof event.detail === "object"
        && event.detail !== null
        && "participantId" in event.detail
        && typeof event.detail.participantId === "string"
        ? event.detail.participantId
        : undefined;
      const channelId = event instanceof CustomEvent
        && typeof event.detail === "object"
        && event.detail !== null
        && "channelId" in event.detail
        && typeof event.detail.channelId === "string"
        ? event.detail.channelId
        : undefined;
      settings.open("chat", {
        chatSubsection: "teammates",
        chatTeammateId: participantId,
        chatChannelId: channelId,
      });
    };
    window.addEventListener("ganbaru-ai:chat-revert-message", revertMessage);
    window.addEventListener("ganbaru-ai:chat-open-review", openWorkspaceTool);
    window.addEventListener("ganbaru-ai:chat-open-file", openWorkspaceTool);
    const manageMembers = (event: Event) => {
      const channelId = event instanceof CustomEvent
        && typeof event.detail === "object"
        && event.detail !== null
        && "channelId" in event.detail
        && typeof event.detail.channelId === "string"
        ? event.detail.channelId
        : chat.selectedChannelId;
      if (!channelId) return;
      window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-edit-channel", { detail: { channelId } }));
    };
    window.addEventListener("ganbaru-ai:chat-configure-teammate", openTeammates);
    window.addEventListener("ganbaru-ai:chat-manage-members", manageMembers);
    return () => {
      observer.disconnect();
      headerGeometryObserver.disconnect();
      if (inspectorResizeFrame !== null) window.cancelAnimationFrame(inspectorResizeFrame);
      if (bottomResizeFrame !== null) window.cancelAnimationFrame(bottomResizeFrame);
      if (inspectorResizeEndFrame !== null) window.cancelAnimationFrame(inspectorResizeEndFrame);
      if (bottomResizeEndFrame !== null) window.cancelAnimationFrame(bottomResizeEndFrame);
      if (panelTransitionFrame !== null) window.cancelAnimationFrame(panelTransitionFrame);
      if (initialLoadingTimer !== null) window.clearTimeout(initialLoadingTimer);
      for (const timer of Object.values(snapTransitionTimers)) {
        if (timer !== undefined) window.clearTimeout(timer);
      }
      motionQuery.removeEventListener("change", updateMotionPreference);
      window.removeEventListener("ganbaru-ai:chat-revert-message", revertMessage);
      window.removeEventListener("ganbaru-ai:chat-open-review", openWorkspaceTool);
      window.removeEventListener("ganbaru-ai:chat-open-file", openWorkspaceTool);
      window.removeEventListener("ganbaru-ai:chat-configure-teammate", openTeammates);
      window.removeEventListener("ganbaru-ai:chat-manage-members", manageMembers);
      void unlisten.then((dispose) => dispose());
    };
  });

  $effect(() => {
    const projectId = projects.selectedProjectId;
    if (chat.loading) return;
    untrack(() => {
      void chat.syncProjectSelection(projectId).catch((error: unknown) => {
        if (projects.selectedProjectId === projectId) {
          loadError = error instanceof Error ? error.message : String(error);
        }
      });
    });
  });

  $effect(() => {
    const open = auxiliaryPairOpen;
    if (open && !auxiliaryPairWasOpen) transientRailOpen = false;
    auxiliaryPairWasOpen = open;
  });

  function threadOpened(trigger: HTMLElement): void {
    replyThreadReturnFocus = trigger;
  }

  function closeReplyThread(): void {
    chat.closeReplyThread();
    const target = replyThreadReturnFocus;
    replyThreadReturnFocus = null;
    queueMicrotask(() => target?.isConnected && target.focus());
  }

  function openRail(): void {
    if (projectHosted) return;
    if (mobileShell) mobileRailOpen = true;
    else if (auxiliaryPairOpen) transientRailOpen = true;
    else chat.railOpen = true;
  }

  function closeRail(): void {
    if (projectHosted) return;
    if (mobileShell) mobileRailOpen = false;
    else if (auxiliaryPairOpen) transientRailOpen = false;
    else chat.railOpen = false;
  }

  function toggleRail(): void {
    if (projectHosted) projects.sidebarCollapsed = !projects.sidebarCollapsed;
    else if (visibleRailOpen) closeRail();
    else openRail();
  }

  function toggleProjectSettings(trigger: HTMLButtonElement): void {
    if (projectSettingsOpen) {
      closeProjectSettings();
      return;
    }
    projectSettingsTrigger = trigger;
    projectSettingsProjectId = projects.selectedProjectId;
    projectSettingsOpen = true;
  }

  function closeProjectSettings(): void {
    projectSettingsOpen = false;
    projectSettingsTrigger = null;
    projectSettingsProjectId = null;
  }

  function openArchiveFromProjectSettings(): void {
    closeProjectSettings();
    chat.openChannelArchive();
    if (mobileShell) closeRail();
  }

  $effect(() => {
    if (projectSettingsOpen && projectSettingsProjectId !== projects.selectedProjectId) {
      closeProjectSettings();
    }
  });

  $effect(() => {
    if (!chat.loading) {
      initialLoadingVisible = false;
      if (initialLoadingTimer !== null) window.clearTimeout(initialLoadingTimer);
      initialLoadingTimer = null;
      return;
    }
    if (initialLoadingTimer !== null) return;
    initialLoadingTimer = window.setTimeout(() => {
      initialLoadingTimer = null;
      if (chat.loading) initialLoadingVisible = true;
    }, 140);
  });

  function refreshHeaderActionInset(): void {
    if (!primaryHeaderElement || !globalActionsElement) return;
    const bottomPanelAction = globalActionsElement.querySelector<HTMLElement>(
      "[data-chat-bottom-panel-action]",
    );
    const inspectorAction = globalActionsElement.querySelector<HTMLElement>(
      "[data-chat-inspector-action]",
    );
    const headerBounds = primaryHeaderElement.getBoundingClientRect();
    const actionBounds = bottomPanelAction?.getBoundingClientRect()
      ?? globalActionsElement.getBoundingClientRect();
    const inspectorActionBounds = inspectorAction?.getBoundingClientRect();
    const actionGap = actionBounds.left >= headerBounds.right
      ? HEADER_PANEL_EDGE_GAP_PX
      : inspectorActionBounds
        ? Math.max(0, inspectorActionBounds.left - actionBounds.right)
        : HEADER_ACTION_EDGE_GAP_PX;
    const inset = chatHeaderActionInset(
      headerBounds.right,
      actionBounds.left,
      actionGap,
    );
    primaryHeaderElement.style.setProperty("--chat-header-action-inset", `${inset}px`);
  }

  $effect(() => {
    if (!chat.settings) return;
    const configuredInspectorWidth = chat.settings.configuration.panels.inspectorWidthPx;
    if (!resizingInspector && !pendingPanelWidths) {
      inspectorWidth = alignInspectorWidthToDisplay(configuredInspectorWidth);
    }
    enablePanelTransitionsAfterLayout();
  });

  $effect(() => {
    const thread = chat.selectedThread;
    if (thread?.id !== announcedThreadId) {
      announcedThreadId = thread?.id ?? null;
      announcedTurnState = thread?.latestTurnState ?? null;
      politeAnnouncement = "";
      return;
    }
    const state = thread?.latestTurnState ?? null;
    if (state === announcedTurnState) return;
    announcedTurnState = state;
    if (state === "active" || state === "dispatching") politeAnnouncement = t("chat.accessibility.providerWorking");
    else if (state === "completed") politeAnnouncement = t("chat.accessibility.responseCompleted");
    else if (state === "interrupted") politeAnnouncement = t("chat.accessibility.turnInterrupted");
    else if (state === "failed") politeAnnouncement = t("chat.accessibility.turnFailed");
  });

  $effect(() => {
    const pending = chat.interaction?.pendingRequest ?? null;
    if (!pending) {
      announcedRequestId = null;
      assertiveAnnouncement = "";
      return;
    }
    if (pending.id === announcedRequestId) return;
    announcedRequestId = pending.id;
    assertiveAnnouncement = pending.requestKind === "approval"
      ? t("chat.accessibility.approvalRequired")
      : t("chat.accessibility.answerRequired");
  });

  $effect(() => {
    const next = chatLayoutDecision({
      containerWidth: shellWidth,
      containerHeight: shellHeight,
      fontScale,
      railOpen: projectHosted ? !projects.sidebarCollapsed : visibleRailOpen,
      inspectorOpen: chat.inspectorOpen,
      inspectorWidth: displayedInspectorWidth,
      replyThreadOpen: chat.openReplyThreadId !== null,
      replyThreadWidth,
      previousVariant: layout.variant,
    });
    if (!chatLayoutsEqual(layout, next)) layout = next;
  });

  $effect(() => {
    const maximum = bottomPanelResizeMaximum();
    if (!resizingBottomPanel) {
      bottomPanelHeight = alignBottomPanelHeightToDisplay(
        bottomPanelHeight,
        maximum,
      );
    }
  });

  $effect(() => {
    const dock = composerDockElement;
    if (!dock) {
      composerDockHeight = 0;
      return;
    }
    const updateHeight = () => {
      const next = Math.ceil(dock.getBoundingClientRect().height);
      if (next !== composerDockHeight) composerDockHeight = next;
    };
    updateHeight();
    const observer = new ResizeObserver(updateHeight);
    observer.observe(dock);
    return () => observer.disconnect();
  });

  $effect(() => {
    const open = chat.inspectorOpen;
    const action = inspectorFocusAction(inspectorWasOpen, open);
    if (action === "enter") {
      inspectorReturnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      queueMicrotask(() => document.querySelector<HTMLElement>("[data-chat-inspector] [role='tab'][aria-selected='true']")?.focus());
    } else if (action === "restore") {
      const target = inspectorReturnFocus;
      // A phone does not reopen the keyboard by returning focus to a text field.
      if (OPENING_MAY_FOCUS_TEXT || !isTextEntryElement(target)) queueMicrotask(() => target?.isConnected && target.focus());
    }
    inspectorWasOpen = open;
  });

  $effect(() => {
    const open = chat.openReplyThreadId !== null;
    if (open && !replyThreadWasOpen) {
      queueMicrotask(() => document.querySelector<HTMLElement>("[data-chat-reply-thread] [data-thread-tab]")?.focus());
    }
    replyThreadWasOpen = open;
  });

  $effect(() => {
    const open = layout.railPresentation === "sheet" && !railUsesMobilePanel && visibleRailOpen;
    if (open && !railModalWasOpen) {
      railReturnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      queueMicrotask(() => firstFocusable(railShell)?.focus());
    } else if (!open && railModalWasOpen) {
      const target = railReturnFocus;
      queueMicrotask(() => {
        if (target?.isConnected) target.focus();
      });
    }
    railModalWasOpen = open;
  });

  function isEditingTarget(target: EventTarget | null): boolean {
    return target instanceof HTMLElement && (
      target.matches("input, textarea, select, [contenteditable='true']")
      || Boolean(target.closest("[role='dialog'], [data-terminal-capture]"))
    );
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (commandMenuOpen && event.key === "Escape") {
      event.preventDefault();
      commandMenuOpen = false;
      return;
    }
    if (hasOnlyShortcutModifier(event) && event.key === ".") {
      event.preventDefault();
      window.dispatchEvent(new Event("ganbaru-ai:chat-stop-requested"));
      return;
    }
    if (isEditingTarget(event.target)) return;
    if (hasOnlyShortcutModifier(event) && event.key.toLowerCase() === "n") {
      event.preventDefault();
      openRail();
      window.dispatchEvent(new Event("ganbaru-ai:chat-new-channel"));
      return;
    }
    if (hasOnlyShortcutModifier(event) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      openRail();
      window.dispatchEvent(new Event("ganbaru-ai:chat-focus-search"));
      return;
    }
    if (hasOnlyShortcutModifier(event) && event.key.toLowerCase() === "l") {
      event.preventDefault();
      document.querySelector<HTMLElement>("[data-chat-composer]")?.focus();
      return;
    }
    if (event.altKey && !event.ctrlKey && !event.metaKey && ["ArrowDown", "ArrowUp"].includes(event.key)) {
      event.preventDefault();
      const index = chat.activeChannels.findIndex((channel) => channel.id === chat.selectedChannelId);
      const next = nextThreadIndex(index, chat.activeChannels.length, event.key === "ArrowDown" ? "next" : "previous");
      const channel = chat.activeChannels[next];
      if (channel) void chat.selectChannel(channel.id);
      return;
    }
    if (hasOnlyShortcutModifier(event) && event.key.toLowerCase() === "b") {
      event.preventDefault();
      toggleRail();
      return;
    }
    if (hasOnlyShortcutModifier(event, { shift: true }) && event.key.toLowerCase() === "j") {
      event.preventDefault();
      chat.inspectorOpen = !chat.inspectorOpen;
      return;
    }
    if (hasOnlyShortcutModifier(event, { shift: true }) && event.key.toLowerCase() === "p") {
      event.preventDefault();
      commandMenuOpen = !commandMenuOpen;
      return;
    }
  }

  function handleSheetKeydown(event: KeyboardEvent, close: () => void): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    trapTabFocus(event);
  }

  function handleRailKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeRail();
      return;
    }
    if (railIsModal) trapTabFocus(event);
  }

  function refreshWorkspacePixelGeometry(): void {
    if (!rootElement) return;
    const bounds = rootElement.getBoundingClientRect();
    shellWidth = bounds.width;
    shellHeight = bounds.height;
    shellRight = bounds.right;
    shellBottom = bounds.bottom;
    displayPixelRatio = Number.isFinite(window.devicePixelRatio) && window.devicePixelRatio > 0
      ? window.devicePixelRatio
      : 1;
    canvasEdgeWidth = canvasEdgeElement ? renderedBorderWidth(canvasEdgeElement) : null;
  }

  /**
   * Reads the width an element's top border renders at. Engines snap border widths to whole device pixels, so at a
   * fractional display scale a 1px border renders narrower than 1px, and an inset that must line up with it uses this.
   */
  function renderedBorderWidth(element: HTMLElement): number | null {
    const width = Number.parseFloat(getComputedStyle(element).borderTopWidth);
    return Number.isFinite(width) && width > 0 ? width : null;
  }

  function enablePanelTransitionsAfterLayout(): void {
    if (panelTransitionsEnabled || panelTransitionFrame !== null) return;
    panelTransitionFrame = window.requestAnimationFrame(() => {
      panelTransitionFrame = window.requestAnimationFrame(() => {
        panelTransitionsEnabled = true;
        panelTransitionFrame = null;
      });
    });
  }

  function alignInspectorWidthToDisplay(
    value: number,
    maximum = inspectorResizeMaximum(),
  ): number {
    return alignPanelSizeToDevicePixel({
      value,
      minimum: MIN_INSPECTOR_WIDTH,
      maximum,
      anchor: shellRight,
      direction: "from-end",
      devicePixelRatio: displayPixelRatio,
    });
  }

  function alignBottomPanelHeightToDisplay(
    value: number,
    maximum = bottomPanelResizeMaximum(),
  ): number {
    return alignPanelSizeToDevicePixel({
      value,
      minimum: MIN_BOTTOM_PANEL_HEIGHT,
      maximum,
      anchor: shellBottom,
      direction: "from-end",
      devicePixelRatio: displayPixelRatio,
    });
  }

  function inspectorWidthFromPointer(requested: number): number {
    const maximum = inspectorResizeMaximum();
    const snapped = panelSizeWithCollapseSnap(
      requested,
      MIN_INSPECTOR_WIDTH,
      maximum,
    );
    return snapped === 0 ? 0 : alignInspectorWidthToDisplay(snapped, maximum);
  }

  function bottomPanelHeightFromPointer(requested: number): number {
    const maximum = bottomPanelResizeMaximum();
    const snapped = panelSizeWithCollapseSnap(
      requested,
      MIN_BOTTOM_PANEL_HEIGHT,
      maximum,
    );
    return snapped === 0 ? 0 : alignBottomPanelHeightToDisplay(snapped, maximum);
  }

  function animateCollapseBoundary(
    panel: ResizableOuterPanel,
    currentSize: number,
    nextSize: number,
  ): void {
    if ((currentSize === 0) === (nextSize === 0)) return;
    const activeTimer = snapTransitionTimers[panel];
    if (activeTimer !== undefined) window.clearTimeout(activeTimer);
    snapTransitioning[panel] = true;
    snapTransitionTimers[panel] = window.setTimeout(() => {
      snapTransitioning[panel] = false;
      delete snapTransitionTimers[panel];
    }, PANEL_TRANSITION_MS);
  }

  function beginInspectorResize(event: PointerEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    refreshWorkspacePixelGeometry();
    if (inspectorResizeEndFrame !== null) window.cancelAnimationFrame(inspectorResizeEndFrame);
    inspectorResizeEndFrame = null;
    resizingInspector = true;
    const startX = event.clientX;
    const startWidth = displayedInspectorWidth;
    const target = event.currentTarget as HTMLElement;
    target.focus();
    target.setPointerCapture(event.pointerId);
    let pendingWidth = startWidth;
    const move = (moveEvent: PointerEvent) => {
      const nextWidth = inspectorWidthFromPointer(startWidth + startX - moveEvent.clientX);
      animateCollapseBoundary("inspector", pendingWidth, nextWidth);
      pendingWidth = nextWidth;
      if (inspectorResizeFrame !== null) return;
      inspectorResizeFrame = window.requestAnimationFrame(() => {
        inspectorWidth = pendingWidth;
        inspectorResizeFrame = null;
      });
    };
    const end = (endEvent: PointerEvent) => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", end);
      target.removeEventListener("pointercancel", end);
      if (inspectorResizeFrame !== null) window.cancelAnimationFrame(inspectorResizeFrame);
      inspectorResizeFrame = null;
      if (endEvent.type === "pointercancel") {
        inspectorWidth = startWidth;
      } else if (pendingWidth === 0) {
        inspectorWidth = startWidth;
        chat.inspectorOpen = false;
      } else {
        inspectorWidth = pendingWidth;
        if (pendingWidth !== startWidth) persistPanelWidths();
      }
      inspectorResizeEndFrame = window.requestAnimationFrame(() => {
        inspectorResizeEndFrame = window.requestAnimationFrame(() => {
          resizingInspector = false;
          inspectorResizeEndFrame = null;
        });
      });
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", end);
    target.addEventListener("pointercancel", end);
  }

  function beginBottomPanelResize(event: PointerEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    refreshWorkspacePixelGeometry();
    if (bottomResizeEndFrame !== null) window.cancelAnimationFrame(bottomResizeEndFrame);
    bottomResizeEndFrame = null;
    resizingBottomPanel = true;
    const startY = event.clientY;
    const startHeight = bottomPanelHeight;
    const target = event.currentTarget as HTMLElement;
    let pendingHeight = startHeight;
    target.focus();
    target.setPointerCapture(event.pointerId);
    const move = (moveEvent: PointerEvent) => {
      const nextHeight = bottomPanelHeightFromPointer(startHeight + startY - moveEvent.clientY);
      animateCollapseBoundary("bottom", pendingHeight, nextHeight);
      pendingHeight = nextHeight;
      if (bottomResizeFrame !== null) return;
      bottomResizeFrame = window.requestAnimationFrame(() => {
        bottomPanelHeight = pendingHeight;
        bottomResizeFrame = null;
      });
    };
    const end = (endEvent: PointerEvent) => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", end);
      target.removeEventListener("pointercancel", end);
      if (bottomResizeFrame !== null) window.cancelAnimationFrame(bottomResizeFrame);
      bottomResizeFrame = null;
      if (endEvent.type === "pointercancel") {
        bottomPanelHeight = startHeight;
      } else if (pendingHeight === 0) {
        bottomPanelHeight = startHeight;
        closeBottomPanel(true);
      } else {
        bottomPanelHeight = pendingHeight;
      }
      bottomResizeEndFrame = window.requestAnimationFrame(() => {
        bottomResizeEndFrame = window.requestAnimationFrame(() => {
          resizingBottomPanel = false;
          bottomResizeEndFrame = null;
        });
      });
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", end);
    target.addEventListener("pointercancel", end);
  }

  function resizeInspectorFromKey(event: KeyboardEvent): void {
    if (event.key === "Enter") {
      event.preventDefault();
      fitInspectorToAvailableSpace();
      return;
    }
    const next = panelWidthFromKey({
      current: displayedInspectorWidth,
      minimum: MIN_INSPECTOR_WIDTH,
      maximum: inspectorResizeMaximum(),
      defaultValue: DEFAULT_INSPECTOR_WIDTH,
      step: 16,
      direction: "reversed",
      key: event.key,
    });
    if (next === null) return;
    event.preventDefault();
    refreshWorkspacePixelGeometry();
    inspectorWidth = alignInspectorWidthToDisplay(next);
    void persistPanelWidths();
  }

  function inspectorResizeMaximum(): number {
    if (layout.inspectorPresentation !== "column") return MAX_INSPECTOR_WIDTH;
    return chatInspectorResizeMaximum({
      containerWidth: shellWidth,
      railVisible: layout.railPresentation === "column" && visibleRailOpen,
      fontScale,
      minimum: MIN_INSPECTOR_WIDTH,
      maximum: MAX_INSPECTOR_WIDTH,
      ...(auxiliaryPairOpen ? {
        railReservedWidth: 0,
        conversationMinimum: CHAT_AUXILIARY_CONVERSATION_MIN_PX * fontScale,
        additionalReservedWidth: replyThreadWidth,
      } : {}),
    });
  }

  function fitInspectorToAvailableSpace(): void {
    refreshWorkspacePixelGeometry();
    const fitted = fittedChatInspectorWidth({
      containerWidth: shellWidth,
      railVisible: layout.railPresentation === "column" && visibleRailOpen,
      fontScale,
      minimum: MIN_INSPECTOR_WIDTH,
      maximum: MAX_INSPECTOR_WIDTH,
      ...(auxiliaryPairOpen ? {
        railReservedWidth: 0,
        conversationMinimum: CHAT_AUXILIARY_CONVERSATION_MIN_PX * fontScale,
        additionalReservedWidth: replyThreadWidth,
      } : {}),
    });
    inspectorWidth = alignInspectorWidthToDisplay(fitted);
    persistPanelWidths();
  }

  function fitBottomPanelToAvailableSpace(): void {
    refreshWorkspacePixelGeometry();
    const fitted = fittedChatBottomPanelHeight({
      containerHeight: shellHeight,
      fontScale,
      minimum: MIN_BOTTOM_PANEL_HEIGHT,
      maximum: MAX_BOTTOM_PANEL_HEIGHT,
    });
    bottomPanelHeight = alignBottomPanelHeightToDisplay(fitted);
  }

  function toggleBottomPanel(): void {
    bottomPanelSkipCloseTransition = false;
    if (bottomPanelOpen) {
      bottomPanelOpen = false;
      return;
    }
    bottomPanelMounted = true;
    bottomPanelOpen = true;
  }

  function openInspectorWorkspaceForEvent(event: Event): void {
    const request = chatWorkspaceRequest(
      event.type,
      event instanceof CustomEvent ? event.detail : null,
    );
    if (!request) return;
    chat.inspectorOpen = true;
    void tick().then(() => {
      window.dispatchEvent(new CustomEvent(CHAT_OPEN_WORKSPACE_PANEL_EVENT, {
        detail: request,
      }));
    });
  }

  function closeBottomPanel(skipTransition = false): void {
    bottomPanelSkipCloseTransition = skipTransition;
    bottomPanelOpen = false;
  }

  function resizeBottomPanelFromKey(event: KeyboardEvent): void {
    if (event.key === "Enter") {
      event.preventDefault();
      fitBottomPanelToAvailableSpace();
      return;
    }
    const maximum = bottomPanelResizeMaximum();
    let next: number;
    switch (event.key) {
      case "ArrowUp": next = bottomPanelHeight + 16; break;
      case "ArrowDown": next = bottomPanelHeight - 16; break;
      case "Home": next = MIN_BOTTOM_PANEL_HEIGHT; break;
      case "End": next = maximum; break;
      default: return;
    }
    event.preventDefault();
    refreshWorkspacePixelGeometry();
    bottomPanelHeight = alignBottomPanelHeightToDisplay(next, maximum);
  }

  function bottomPanelResizeMaximum(): number {
    return chatBottomPanelResizeMaximum({
      containerHeight: shellHeight,
      fontScale,
      minimum: MIN_BOTTOM_PANEL_HEIGHT,
      maximum: MAX_BOTTOM_PANEL_HEIGHT,
    });
  }

  function persistPanelWidths(): void {
    if (!chat.settings) return;
    pendingPanelWidths = {
      inspectorWidthPx: Math.round(inspectorWidth),
    };
    panelWidthSave ??= savePendingPanelWidths();
  }

  async function savePendingPanelWidths(): Promise<void> {
    layoutError = null;
    try {
      const { updateChatPanels } = await import("$lib/api/chat");
      while (pendingPanelWidths) {
        const savedWidths = pendingPanelWidths;
        await updateChatPanels(savedWidths);
        await chat.refreshSettings();
        if (pendingPanelWidths !== savedWidths) continue;
        inspectorWidth = alignInspectorWidthToDisplay(savedWidths.inspectorWidthPx);
        pendingPanelWidths = null;
      }
    } catch (error: unknown) {
      pendingPanelWidths = null;
      layoutError = error instanceof Error ? error.message : String(error);
    } finally {
      panelWidthSave = null;
    }
  }

  async function retryWorkspaceLoad(): Promise<void> {
    try {
      await chat.reload();
      loadError = null;
    } catch (error: unknown) {
      loadError = error instanceof Error ? error.message : String(error);
      throw error;
    }
  }

  async function restoreMessageCheckpoint(threadId: string, checkpointId: string): Promise<void> {
    const thread = chat.selectedThread;
    if (!thread || thread.id !== threadId) return;
    layoutError = null;
    try {
      await restoreChatCheckpoint({
        thread,
        checkpointId,
        confirm: (preview) => window.confirm([
          t("chat.timeline.revert"),
          preview.files.map((file) => file.relativePath).join("\n"),
          ...preview.warnings,
        ].filter(Boolean).join("\n\n")),
        onRestored: (restoredThreadId) => chat.handleNativeChange(restoredThreadId),
      });
    } catch (error: unknown) {
      layoutError = error instanceof Error ? error.message : String(error);
    }
  }
</script>

{#snippet headerActions()}
  <ChatHeaderActions
    {bottomPanelOpen}
    projectName={projects.selectedProject?.name ?? null}
    {projectSettingsOpen}
    onToggleBottomPanel={toggleBottomPanel}
    onToggleProjectSettings={toggleProjectSettings}
  />
{/snippet}

<svelte:window onkeydown={handleKeydown} />

<div bind:this={rootElement} class="chat-workspace @container/chat-shell relative grid h-full min-h-0 overflow-hidden" class:resizing-panels={resizingInspector || resizingBottomPanel} class:panel-transitions-enabled={panelTransitionsEnabled} data-chat-workspace data-layout={layout.variant} data-rail-presentation={layout.railPresentation} data-inspector-presentation={layout.inspectorPresentation} data-reply-thread-presentation={layout.replyThreadPresentation} data-active-surface={layout.activeSurface} data-rail-open={visibleRailOpen} data-rail-docked={railDocked} data-canvas-edge={!mobileShell} data-project-sidebar={projectHosted} style={`container-type:inline-size;container-name:chat-shell;--chat-panel-transition-duration:${PANEL_TRANSITION_MS}ms;--chat-bottom-min-height:${MIN_BOTTOM_PANEL_HEIGHT}px;--chat-inspector-width:${displayedInspectorWidth}px;--chat-reply-thread-width:${replyThreadWidth}px;${canvasEdgeWidth === null ? "" : `--chat-canvas-edge-width:${canvasEdgeWidth}px;`}`}>
  <div class="chat-canvas" aria-hidden="true"></div>
{#if WorkspaceObserver}<WorkspaceObserver />{/if}
  <div class="sr-only" aria-live="polite" aria-atomic="true">{politeAnnouncement}</div>
  <div class="sr-only" aria-live="assertive" aria-atomic="true">{assertiveAnnouncement}</div>
  {#if layoutError}<Notice class="surface-floating absolute inset-x-0 top-2 z-50 mx-auto w-fit max-w-md px-2.5 py-2 text-xs" message={layoutError} />{/if}
  {#if layout.inspectorPresentation === "sheet" && chat.inspectorOpen}
    <button type="button" class="chat-sheet-backdrop chat-inspector-backdrop" aria-label={t("chat.closeInspector")} onclick={() => { chat.inspectorOpen = false; }}></button>
  {:else if railIsModal}
    <button type="button" class="chat-sheet-backdrop chat-rail-backdrop" aria-label={t("chat.collapseRail")} onclick={closeRail}></button>
  {/if}
  <div bind:this={primaryHeaderElement} class="chat-primary-header">
    <ChatWorkspaceHeader
      bind:editingTitle={headerEditingTitle}
      explorerExpanded={layout.railPresentation === "column" && visibleRailOpen}
      showRailButton={!projectHosted && layout.railPresentation === "sheet" && (railUsesMobilePanel || !visibleRailOpen)}
      reserveGlobalActions={!chat.inspectorOpen && !threadColumnOpen}
      mobilePresentation={mobileShell}
      onOpenRail={openRail}
      trailingActions={mobileShell ? headerActions : undefined}
      projectViewSwitcher={mobileShell ? projectViewSwitcher : undefined}
    />
  </div>
  {#if railUsesMobilePanel}
    <ChatChannelRail presentation="panel" expanded={visibleRailOpen} showCollapsedStrip={false} onExpand={openRail} onCollapse={closeRail} />
  {:else}
    <div bind:this={railShell} class="chat-rail-shell" class:closed={!visibleRailOpen} class:explorer-chrome={railDocked} role={railIsModal ? "dialog" : undefined} aria-modal={railIsModal ? "true" : undefined} aria-label={railIsModal ? t("chat.title") : undefined} onkeydown={(event) => { if (!projectHosted && layout.railPresentation === "sheet") handleRailKeydown(event); }}>
      {#if projectSidebar && projectHosted}{@render projectSidebar(() => {})}{/if}
      <ChatChannelRail presentation={layout.railPresentation} expanded={visibleRailOpen} showCollapsedStrip={!projectHosted && layout.railPresentation === "column"} onExpand={openRail} onCollapse={closeRail} />
    </div>
  {/if}
  <main class="main-shell relative flex min-w-0 flex-col">
        {#if loadError}
          <div class="min-h-0 flex-1 text-sm"><Notice layout="fill" message={loadError} onRetry={retryWorkspaceLoad} /></div>
        {:else if chat.channelArchiveOpen}
          <ChatChannelArchive />
        {:else if chat.selectedChannel}
          {#if threadFullSurface && chat.openReplyThreadId}
            <ChatReplyThreadPanel presentation="main" onClose={closeReplyThread} />
          {:else}
            <ChatChannelFeed onThreadOpened={threadOpened} />
          {/if}
        {:else if !chat.loading && !chat.channelsLoading}
          <ChatFirstUse />
        {:else if initialLoadingVisible || chat.channelsLoading}
          <Loading layout="fill" class="min-h-0 flex-1" />
        {/if}
  </main>

  <div class="chat-panel-separator chat-thread-separator" class:hidden={!threadColumnOpen}><span class="chat-panel-separator-line" aria-hidden="true"></span></div>
  <aside class="chat-thread-shell" class:open={threadColumnOpen} inert={!threadColumnOpen} aria-label={t("chat.organization.thread")}>
    {#if threadColumnOpen}
      <ChatReplyThreadPanel presentation="complementary" reserveGlobalActions={!chat.inspectorOpen} onClose={closeReplyThread} />
    {/if}
  </aside>

  <div class="chat-panel-separator chat-inspector-separator" class:hidden={!chat.inspectorOpen || layout.inspectorPresentation !== "column"} class:active={resizingInspector}><input type="range" min={MIN_INSPECTOR_WIDTH} max={inspectorResizeMaximum()} step="any" value={displayedInspectorWidth} aria-label={t("chat.resizeInspector")} onpointerdown={beginInspectorResize} onkeydown={resizeInspectorFromKey} ondblclick={(event) => { event.preventDefault(); fitInspectorToAvailableSpace(); }} /><span class="chat-panel-separator-line" aria-hidden="true"></span></div>
  <aside class="chat-inspector-shell" class:open={chat.inspectorOpen} class:resizing={resizingInspector} class:snap-transition={snapTransitioning.inspector} data-presentation={layout.inspectorPresentation} inert={!chat.inspectorOpen} role={layout.inspectorPresentation === "sheet" && chat.inspectorOpen ? "dialog" : undefined} aria-modal={layout.inspectorPresentation === "sheet" && chat.inspectorOpen ? "true" : undefined} aria-label={t("chat.openInspector")} onkeydown={(event) => { if (layout.inspectorPresentation === "sheet") handleSheetKeydown(event, () => { chat.inspectorOpen = false; }); }}>
    <div class="chat-inspector-content-shell">
      {#if WorkspacePanel}<WorkspacePanel placement="inspector" visible={chat.inspectorOpen} onClose={() => { chat.inspectorOpen = false; }} />{/if}
    </div>
  </aside>

  {#if !mobileShell}
    <!-- Desktop overlays the actions on whichever panel owns the top edge; the phone row carries them inline. -->
    <div bind:this={globalActionsElement} class="chat-global-actions">{@render headerActions()}</div>
  {/if}
  {#if projectSettingsOpen && projectSettingsTrigger && projects.selectedProject}
    <ChatProjectSettingsPanel
      projectId={projects.selectedProject.id}
      projectName={projects.selectedProject.name}
      triggerElement={projectSettingsTrigger}
      onClose={closeProjectSettings}
      onOpenArchive={openArchiveFromProjectSettings}
    />
  {/if}

  {#if bottomPanelMounted}
    <div class="chat-panel-separator chat-bottom-separator" class:hidden={!bottomPanelVisible} class:active={resizingBottomPanel}><input type="range" min={MIN_BOTTOM_PANEL_HEIGHT} max={bottomPanelResizeMaximum()} step="any" value={bottomPanelHeight} aria-label={t("chat.resizeBottomPanel")} onpointerdown={beginBottomPanelResize} onkeydown={resizeBottomPanelFromKey} ondblclick={(event) => { event.preventDefault(); fitBottomPanelToAvailableSpace(); }} /><span class="chat-panel-separator-line" aria-hidden="true"></span></div>
    <div class="chat-bottom-transition-shell" class:open={bottomPanelVisible} class:skip-transition={bottomPanelSkipCloseTransition} class:snap-transition={snapTransitioning.bottom} inert={!bottomPanelVisible} style={`--chat-bottom-height:${bottomPanelHeight}px`} in:slide={{ duration: reducedMotion ? 0 : PANEL_TRANSITION_MS, easing: quintOut }}>
      <div class="chat-bottom-shell">{#if WorkspacePanel}<WorkspacePanel placement="bottom" visible={bottomPanelVisible} onClose={() => { closeBottomPanel(); }} />{/if}</div>
    </div>
  {/if}

  {#if commandMenuOpen && localExecutionAvailable && LocalCommandMenu}
    <LocalCommandMenu
      {bottomPanelOpen}
      onClose={() => { commandMenuOpen = false; }}
      onNewChannel={() => { openRail(); window.dispatchEvent(new Event("ganbaru-ai:chat-new-channel")); }}
      onSearch={() => { openRail(); window.dispatchEvent(new Event("ganbaru-ai:chat-focus-search")); }}
      onToggleBottomPanel={toggleBottomPanel}
      onToggleInspector={() => { chat.inspectorOpen = !chat.inspectorOpen; }}
      onOpenSettings={() => settings.open("chat")}
    />
  {/if}
  {#if !mobileShell}
    <div bind:this={canvasEdgeElement} class="chat-canvas-edge surface-content-edge" aria-hidden="true"></div>
  {/if}
</div>

<style>
  .chat-workspace {
    --chat-icon-stroke-width: var(--icon-stroke-width);
    --chat-compact-icon-stroke-width: var(--icon-stroke-width-compact);
    --chat-small-simple-icon-stroke-width: var(--icon-stroke-width-small-simple);
    --chat-conversation-font-size: calc(0.875rem * var(--type-scale));
    --chat-conversation-line-height: calc(1.3125rem * var(--type-scale));
    --chat-process-font-size: calc(0.8125rem * var(--type-scale));
    --chat-process-line-height: calc(1.1875rem * var(--type-scale));
    --chat-conversation-tight-space: 0.125rem;
    --chat-conversation-flow-space: 0.5rem;
    --chat-conversation-block-space: 0.65rem;
    --chat-organizational-font-size: var(--chat-conversation-font-size);
    --chat-organizational-line-height: var(--chat-conversation-line-height);
    --chat-organizational-time-font-size: calc(0.6875rem * var(--type-scale));
    --chat-conversation-max-width: 60rem;
    --chat-conversation-gutter: 1rem;
    --chat-conversation-entry-space: 0.45rem;
    --chat-reply-thread-column-width: 0px;
    --chat-inspector-column-width: 0px;
    --chat-global-actions-width: 6.5rem;
    grid-template-columns: var(--chat-rail-column-width) minmax(0, 1fr) 0 var(--chat-reply-thread-column-width) 0 var(--chat-inspector-column-width);
    grid-template-rows: var(--toolbar-h) minmax(0, 1fr) auto auto;
  }
  .chat-workspace :global(svg.lucide:not(.explorer-sidebar *)) { stroke-width: var(--chat-icon-stroke-width); }
  .chat-workspace :global(svg.lucide[stroke-width="1"]:not(.explorer-sidebar *)) { stroke-width: 1; }
  .chat-workspace :global(svg.lucide[stroke-width="1.5"]:not(.explorer-sidebar *)) { stroke-width: var(--chat-compact-icon-stroke-width); }
  /* A column rail sizes its own track, so it shares the docked explorer collapse motion with Projects and Notes. */
  .chat-workspace[data-rail-presentation="column"] { --chat-rail-column-width: auto; }
  .chat-workspace[data-rail-presentation="sheet"] { --chat-rail-column-width: 0px; }
  .chat-workspace[data-reply-thread-presentation="column"] { --chat-reply-thread-column-width: var(--chat-reply-thread-width); }
  .chat-workspace[data-inspector-presentation="column"] { --chat-inspector-column-width: var(--chat-inspector-width); }
  /* Content sits inside the canvas edge, as it does in tabs whose content surface carries the edge as its own border; the canvas and the docked rail reach past that inset. The inset is the width the edge renders at, which is narrower than its specified width at fractional display scales. */
  .chat-workspace { --chat-canvas-inset: 0px; padding: var(--chat-canvas-inset); }
  .chat-workspace[data-canvas-edge="true"] { --chat-canvas-inset: var(--chat-canvas-edge-width, var(--content-edge-width)); }
  .chat-canvas, .chat-canvas-edge { grid-column: 1 / 7; grid-row: 1 / 5; margin: calc(-1 * var(--chat-canvas-inset)); pointer-events: none; }
  .chat-canvas { background: var(--cal-bg); }
  /* Above the toolbar actions and composer dock that paint the canvas color at its edges, below sheet backdrops. */
  .chat-canvas-edge { z-index: 20; }
  .chat-workspace[data-rail-docked="true"] :is(.chat-canvas, .chat-canvas-edge) { grid-column: 2 / 7; border-radius: var(--content-radius); }
  .chat-primary-header { grid-column: 1 / 3; grid-row: 1; min-width: 0; }
  .chat-workspace[data-rail-docked="true"] .chat-primary-header { grid-column: 2; }
  .chat-global-actions { position: absolute; top: var(--chat-canvas-inset); right: var(--chat-canvas-inset); z-index: 5; display: flex; width: max-content; min-width: var(--chat-global-actions-width); height: var(--toolbar-h); align-items: center; justify-content: flex-end; background: var(--cal-bg); padding-right: 0.75rem; }
  .chat-rail-shell { grid-column: 1; grid-row: 2; min-width: 0; overflow: hidden; }
  .chat-workspace[data-rail-presentation="column"][data-rail-open="true"] .chat-rail-shell { grid-row: 2 / 5; }
  .chat-workspace[data-rail-presentation="column"][data-rail-docked="true"] .chat-rail-shell { grid-row: 1 / 5; }
  .chat-workspace[data-rail-docked="true"] .chat-rail-shell { margin: calc(-1 * var(--chat-canvas-inset)) var(--chat-canvas-inset) calc(-1 * var(--chat-canvas-inset)) calc(-1 * var(--chat-canvas-inset)); }
  .main-shell { grid-column: 2; grid-row: 2; min-width: min(440px, 100cqw); min-height: 0; }
  .chat-workspace[data-reply-thread-presentation="column"] .main-shell { min-width: min(320px, 100cqw); }
  /* Beside a docked rail the main column owns the canvas's bottom-left corner, so its full-bleed fills (composer docks, panels) follow the rounded edge. Only that corner is clipped, so popovers still escape upward and rightward. */
  .chat-workspace[data-rail-docked="true"]:not(:has(.chat-bottom-transition-shell.open)) .main-shell { clip-path: inset(-100vmax -100vmax 0 0 round 0 0 0 max(0px, var(--content-radius) - var(--chat-canvas-inset))); }
  .chat-thread-separator { grid-column: 3; grid-row: 1 / 3; }
  .chat-thread-shell { --chat-icon-stroke-width: var(--chat-compact-icon-stroke-width); grid-column: 4; grid-row: 1 / 3; min-width: 0; overflow: hidden; background: var(--cal-bg); }
  .chat-inspector-separator { grid-column: 5; grid-row: 1 / 3; }
  .chat-inspector-shell { --chat-icon-stroke-width: var(--chat-compact-icon-stroke-width); grid-column: 6; grid-row: 1 / 3; min-width: 0; overflow: hidden; background: var(--cal-bg); }
  .chat-bottom-separator { grid-column: 1 / 7; grid-row: 3; }
  .chat-bottom-transition-shell { --chat-icon-stroke-width: var(--chat-compact-icon-stroke-width); grid-column: 1 / 7; grid-row: 4; }
  .chat-workspace[data-rail-presentation="column"][data-rail-open="true"] .chat-bottom-separator,
  .chat-workspace[data-rail-presentation="column"][data-rail-open="true"] .chat-bottom-transition-shell,
  .chat-workspace[data-rail-docked="true"] .chat-bottom-separator,
  .chat-workspace[data-rail-docked="true"] .chat-bottom-transition-shell { grid-column-start: 2; }
  .chat-workspace[data-rail-docked="true"] .chat-bottom-transition-shell { border-bottom-left-radius: max(0px, var(--content-radius) - var(--chat-canvas-inset)); }
  .chat-workspace.panel-transitions-enabled { transition: grid-template-columns var(--chat-panel-transition-duration) cubic-bezier(0.22, 1, 0.36, 1); }
  .chat-workspace.panel-transitions-enabled .chat-rail-shell { transition: transform var(--chat-panel-transition-duration) cubic-bezier(0.22, 1, 0.36, 1); }
  .chat-panel-separator { position: relative; z-index: 1; background: transparent; }
  .chat-panel-separator-line { --chat-divider-highlight: color-mix(in srgb, var(--ring) 55%, var(--border)); position: absolute; pointer-events: none; background: var(--border); }
  .chat-panel-separator input { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; appearance: none; touch-action: none; cursor: inherit; opacity: 0; }
  .chat-thread-separator, .chat-inspector-separator { width: 8px; min-width: 8px; margin-inline: -4px; }
  .chat-inspector-separator { cursor: col-resize; }
  .chat-thread-separator::before, .chat-inspector-separator::before { position: absolute; inset: 0 0 auto; height: var(--toolbar-h); background: var(--cal-bg); content: ""; pointer-events: none; }
  .chat-thread-separator .chat-panel-separator-line, .chat-inspector-separator .chat-panel-separator-line { inset-block: 0; left: 50%; width: 1px; }
  .chat-inspector-separator:is(:hover, .active) .chat-panel-separator-line, .chat-inspector-separator input:focus-visible + .chat-panel-separator-line { background: linear-gradient(to bottom, var(--border), var(--chat-divider-highlight) 50%, var(--border)); }
  .chat-inspector-shell.open.resizing { min-width: 0; }
  .chat-inspector-content-shell { width: var(--chat-inspector-width); height: 100%; }
  .chat-bottom-separator { width: 100%; height: 8px; min-height: 8px; margin-block: -4px; cursor: row-resize; }
  .chat-bottom-separator .chat-panel-separator-line { inset-inline: 0; top: 50%; height: 1px; }
  .chat-bottom-separator:is(:hover, .active) .chat-panel-separator-line, .chat-bottom-separator input:focus-visible + .chat-panel-separator-line { background: linear-gradient(to right, var(--border), var(--chat-divider-highlight) 50%, var(--border)); }
  .chat-bottom-transition-shell { height: 0; min-height: 0; overflow: hidden; }
  .chat-workspace.panel-transitions-enabled .chat-bottom-transition-shell { transition: height var(--chat-panel-transition-duration) cubic-bezier(0.22, 1, 0.36, 1); }
  .chat-bottom-transition-shell.open { height: var(--chat-bottom-height); }
  .chat-bottom-transition-shell.skip-transition { transition: none; }
  .chat-bottom-shell { height: max(var(--chat-bottom-height), var(--chat-bottom-min-height)); min-height: 0; overflow: hidden; }
  .chat-workspace.resizing-panels, .chat-workspace.resizing-panels * { user-select: none; }
  .chat-workspace.resizing-panels, .chat-workspace.resizing-panels .chat-bottom-transition-shell { transition: none; }
  .chat-bottom-transition-shell.snap-transition { transition: height var(--chat-panel-transition-duration) cubic-bezier(0.22, 1, 0.36, 1); }
  .chat-sheet-backdrop { position: absolute; inset: 0; z-index: 30; background: rgb(0 0 0 / 0.28); }
  .chat-rail-backdrop { top: var(--toolbar-h); }
  .chat-workspace[data-rail-presentation="sheet"] .chat-rail-shell { position: absolute; top: var(--toolbar-h); bottom: 0; left: 0; z-index: 40; width: min(var(--explorer-width), 88cqw); min-width: min(var(--explorer-width), 88cqw); box-shadow: var(--floating-shadow); }
  .chat-workspace[data-rail-presentation="sheet"] .chat-rail-shell.closed { transform: translateX(-105%); }
  .chat-workspace[data-inspector-presentation="sheet"] .chat-inspector-shell { position: absolute; inset-block: 0; right: 0; z-index: 45; width: 0; box-shadow: var(--floating-shadow); }
  .chat-workspace[data-inspector-presentation="sheet"] .chat-inspector-shell.open { width: min(620px, 94cqw); min-width: min(320px, 94cqw); }
  .chat-workspace[data-inspector-presentation="sheet"] .chat-inspector-content-shell { width: min(620px, 94cqw); }
  .chat-workspace[data-layout="minimum_recovery"] .chat-rail-shell, .chat-workspace[data-layout="minimum_recovery"] .chat-rail-shell.closed, .chat-workspace[data-layout="minimum_recovery"] .chat-inspector-shell.open { width: 100cqw; min-width: 0; box-shadow: none; }
  .chat-workspace[data-layout="minimum_recovery"] .chat-inspector-content-shell { width: 100cqw; }
  .chat-workspace[data-layout="minimum_recovery"][data-active-surface="rail"] .main-shell, .chat-workspace[data-layout="minimum_recovery"][data-active-surface="inspector"] .main-shell { display: none; }
  /* A hosting Projects sidebar sizes its own column and stays docked at every width. */
  .chat-workspace[data-project-sidebar="true"] { --chat-rail-column-width: auto; }
  .chat-workspace[data-project-sidebar="true"] .chat-rail-shell, .chat-workspace[data-project-sidebar="true"] .chat-rail-shell.closed { position: relative; inset: auto; z-index: auto; grid-row: 1 / 5; width: auto; min-width: 0; box-shadow: none; transform: none; }
  .chat-workspace[data-project-sidebar="true"][data-active-surface="rail"] .main-shell { display: flex; }
  @media (prefers-reduced-motion: reduce) {
    :global(.chat-workspace *), :global(.chat-workspace *::before), :global(.chat-workspace *::after) {
      scroll-behavior: auto !important;
      transition: none !important;
      animation-duration: 0.01ms !important;
      animation-iteration-count: 1 !important;
    }
  }
</style>
