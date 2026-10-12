<script module lang="ts">
  import { ChatInspectorSessionState } from "$lib/chat/workspace/inspector-model";
  import { TerminalPanelRegistry } from "$lib/chat/workspace/terminal-model";

  const workspacePanelSessions = {
    inspector: new ChatInspectorSessionState("files"),
    bottom: new ChatInspectorSessionState("terminal"),
  };
  const terminalPanels = new TerminalPanelRegistry();
  const terminalLoadLocks = new Map<string, Promise<void>>();

  /** Serializes terminal discovery and creation within one thread and workspace. */
  async function withTerminalLoadLock<T>(scopeKey: string, operation: () => Promise<T>): Promise<T> {
    const previous = terminalLoadLocks.get(scopeKey) ?? Promise.resolve();
    let release: () => void = () => undefined;
    const current = new Promise<void>((resolve) => { release = resolve; });
    const queued = previous.then(() => current);
    terminalLoadLocks.set(scopeKey, queued);
    await previous;
    try {
      return await operation();
    } finally {
      release();
      if (terminalLoadLocks.get(scopeKey) === queued) terminalLoadLocks.delete(scopeKey);
    }
  }
</script>

<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import FileDiff from "@lucide/svelte/icons/file-diff";
  import Files from "@lucide/svelte/icons/files";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import Globe from "@lucide/svelte/icons/globe";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import Plus from "@lucide/svelte/icons/plus";
  import SquareTerminal from "@lucide/svelte/icons/square-terminal";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import type { ChatInspectorTab, ChatTerminalRead, ReviewDiffSource } from "$lib/chat/contracts";
  import {
    CHAT_WORKSPACE_PANEL_TAB_NAME_MAX_LENGTH,
    closeInspectorTab,
    inspectorSessionKey,
    normalizeWorkspacePanelTabName,
    openInspectorTab,
    reconcileWorkspacePanelTabOrder,
    terminalWorkspacePanelDefaultLabel,
    terminalWorkspacePanelTabKey,
    workspacePanelKinds,
    workspacePanelTerminalId,
    type ChatInspectorThreadState,
    type ChatWorkspacePanelTabKey,
  } from "$lib/chat/workspace/inspector-model";
  import { WorkspacePanelTabController } from "$lib/chat/workspace/panel-tabs.svelte";
  import { terminalErrorMessage } from "$lib/chat/workspace/terminal-model";
  import {
    CHAT_OPEN_WORKSPACE_PANEL_EVENT,
    isChatWorkspaceRequest,
    type ChatOpenFileDetail,
    type ChatOpenReviewDetail,
  } from "$lib/chat/workspace/events";
  import {
    pickSelectPopoverGeometry,
    type SelectPopoverGeometry,
    type SelectPopoverRect,
  } from "$lib/utils/select-popover-position";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { portal } from "$lib/utils/portal";
  import ChatBrowserPanel from "./ChatBrowserPanel.svelte";
  import ChatFileIcon from "./ChatFileIcon.svelte";
  import ChatFilesPanel from "./ChatFilesPanel.svelte";
  import ChatPlanPanel from "./ChatPlanPanel.svelte";
  import ChatReviewPanel from "$lib/components/chat/review/ChatReviewPanel.svelte";
  import ChatSourceControlPanel from "./ChatSourceControlPanel.svelte";
  import ChatTerminalView from "./ChatTerminalView.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    placement,
    visible = true,
    onClose,
  }: {
    placement: "inspector" | "bottom";
    visible?: boolean;
    onClose: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  const DEFAULT_PICKER_GEOMETRY: SelectPopoverGeometry = {
    top: 0,
    left: 0,
    width: null,
    minWidth: 0,
    maxWidth: 0,
    maxHeight: 0,
    placement: "below",
  };
  let panelState = $state<ChatInspectorThreadState>(placementSession().read(null));
  let terminals: ChatTerminalRead[] = $state([]);
  let selectedTerminalId: string | null = $state(null);
  let terminalsLoading = $state(false);
  let panelPickerOpen = $state(false);
  let panelPickerReady = $state(false);
  let panelPickerTrigger: HTMLButtonElement | undefined = $state();
  let panelPickerList: HTMLDivElement | undefined = $state();
  let panelPicker: HTMLDivElement | undefined = $state();
  let panelPickerGeometry = $state<SelectPopoverGeometry>(DEFAULT_PICKER_GEOMETRY);
  let fadedTerminalIds: string[] = $state([]);
  let error: string | null = $state(null);
  let loadedKey: string | null = null;
  let terminalScopeKey = "";
  let terminalLayoutSaveTimer: number | null = null;
  let reviewOpenRequest = 0;
  let destroyed = false;
  let isTerminalSplit = $state(false);
  const threadId = $derived(chat.selectedThreadId ?? chat.draftThreadId);
  const workingFolderId = $derived(chat.selectedWorkingFolderId);
  const sessionKey = $derived(inspectorSessionKey(threadId, workingFolderId));
  const selectedTerminal = $derived(terminals.find((terminal) => terminal.id === selectedTerminalId) ?? null);
  /** True when the terminal tab has no terminal to show after loading, so its failure fills the tab. */
  const terminalLoadFailed = $derived(panelState.tab === "terminal" && !terminalsLoading && !(isTerminalSplit && terminals.length > 1) && !selectedTerminal);
  const panelTabs: {
    id: Exclude<ChatInspectorTab, "terminal">;
    label: "plan" | "files" | "sourceControl" | "browser" | "review";
    icon: typeof FileDiff;
  }[] = [
    { id: "plan", label: "plan", icon: ListTodo },
    { id: "files", label: "files", icon: Files },
    { id: "sourceControl", label: "sourceControl", icon: GitBranch },
    { id: "browser", label: "browser", icon: Globe },
    { id: "review", label: "review", icon: FileDiff },
  ];
  type WorkspacePanelRenderTab =
    | { key: ChatWorkspacePanelTabKey; type: "loading-terminal" }
    | { key: ChatWorkspacePanelTabKey; type: "terminal"; terminal: ChatTerminalRead }
    | { key: ChatWorkspacePanelTabKey; type: "panel"; panel: (typeof panelTabs)[number] };
  const orderedTabKeys = $derived(reconcileWorkspacePanelTabOrder(
    panelState.tabOrder,
    panelState.openTabs,
    terminals.map((terminal) => terminal.id),
  ));
  const orderedTabs = $derived.by<WorkspacePanelRenderTab[]>(() => {
    const tabs: WorkspacePanelRenderTab[] = [];
    for (const key of orderedTabKeys) {
      if (key === "terminal") {
        tabs.push({ key, type: "loading-terminal" });
        continue;
      }
      const terminalId = workspacePanelTerminalId(key);
      const terminal = terminalId
        ? terminals.find((candidate) => candidate.id === terminalId)
        : undefined;
      if (terminal) {
        tabs.push({ key, type: "terminal", terminal });
        continue;
      }
      const panel = panelForKey(key);
      if (panel) tabs.push({ key, type: "panel", panel });
    }
    return tabs;
  });
  const tabController = new WorkspacePanelTabController({
    orderedKeys: () => orderedTabKeys,
    tabNames: () => panelState.tabNames,
    label: workspacePanelTabLabel,
    defaultLabel: defaultWorkspacePanelTabLabel,
    activate: activateWorkspacePanelTab,
    update,
    closePicker: () => { panelPickerOpen = false; },
  });

  onMount(() => {
    if (placement !== "inspector") return;
    const openWorkspacePanel = (event: Event) => {
      if (!(event instanceof CustomEvent) || !isChatWorkspaceRequest(event.detail)) return;
      switch (event.detail.source) {
        case "review": openReview(event.detail.detail); break;
        case "file": openFile(event.detail.detail); break;
      }
    };
    window.addEventListener(CHAT_OPEN_WORKSPACE_PANEL_EVENT, openWorkspacePanel);
    return () => {
      window.removeEventListener(CHAT_OPEN_WORKSPACE_PANEL_EVENT, openWorkspacePanel);
    };
  });

  function openReview(detail: ChatOpenReviewDetail | null): void {
    if (!detail) {
      reviewOpenRequest += 1;
      openPanel("review");
      update({
        reviewSource: chat.selectedThreadId
          ? { kind: "checkpoint", range: "turn", turnId: null }
          : { kind: "working_tree", mode: "all" },
        selectedFile: null,
        reviewThreadId: null,
        reviewWorkingFolderId: null,
        reviewExecutionEnvironmentId: null,
      });
      return;
    }
    if (detail.sourceThreadId && detail.sourceWorkingFolderId) {
      void openSessionReview({
        ...detail,
        sourceThreadId: detail.sourceThreadId,
        sourceWorkingFolderId: detail.sourceWorkingFolderId,
      });
      return;
    }
    reviewOpenRequest += 1;
    openPanel("review");
    update({
      reviewSource: detail.source,
      selectedFile: detail.relativePath,
      reviewThreadId: null,
      reviewWorkingFolderId: null,
      reviewExecutionEnvironmentId: null,
    });
  }

  function openFile(detail: ChatOpenFileDetail): void {
    openPanel("files");
    update({ filePreviewPath: detail.relativePath });
  }

  async function openSessionReview(detail: {
    source: ReviewDiffSource;
    relativePath: string | null;
    sourceThreadId: string;
    sourceWorkingFolderId: string;
  }): Promise<void> {
    const request = ++reviewOpenRequest;
    const selectedThreadId = chat.selectedThreadId;
    const selectedEnvironmentId = chat.selectedExecutionEnvironmentId;
    let executionEnvironmentId = selectedThreadId === detail.sourceThreadId
      ? selectedEnvironmentId
      : null;
    if (executionEnvironmentId === null) {
      try {
        executionEnvironmentId = await chatApi.readChatThreadExecutionEnvironment(detail.sourceThreadId);
      } catch (reason: unknown) {
        if (request === reviewOpenRequest && !destroyed) error = message(reason);
        return;
      }
    }
    if (request !== reviewOpenRequest || destroyed) return;
    openPanel("review");
    update({
      reviewSource: detail.source,
      selectedFile: detail.relativePath,
      reviewThreadId: detail.sourceThreadId,
      reviewWorkingFolderId: detail.sourceWorkingFolderId,
      reviewExecutionEnvironmentId: executionEnvironmentId,
    });
  }


  function initialPanelTab(): "files" | "terminal" {
    return placement === "bottom" ? "terminal" : "files";
  }

  function placementSession(): ChatInspectorSessionState {
    return workspacePanelSessions[placement];
  }

  $effect(() => {
    const key = sessionKey;
    if (key === loadedKey) return;
    tabController.cancelDrag();
    tabController.closeRename();
    loadedKey = key;
    panelState = placementSession().read(key);
  });

  $effect(() => {
    const terminalOpen = panelState.openTabs.includes("terminal");
    const thread = threadId;
    const workspace = workingFolderId;
    const nextKey = `${thread ?? ""}:${workspace ?? ""}`;
    if (!terminalOpen || !thread || !workspace) {
      terminalScopeKey = "";
      terminals = [];
      selectedTerminalId = null;
      return;
    }
    if (!visible) {
      if (terminals.length === 0) terminalScopeKey = "";
      return;
    }
    if (terminalScopeKey === nextKey && (terminalsLoading || terminals.length > 0 || error)) return;
    terminalScopeKey = nextKey;
    void loadTerminals(thread, workspace, nextKey);
  });

  function update(updateValue: Partial<ChatInspectorThreadState>): void {
    const key = sessionKey;
    if (!key) return;
    panelState = placementSession().update(key, updateValue);
    if (updateValue.tabNames !== undefined || updateValue.tabOrder !== undefined) scheduleTerminalLayoutSave();
  }

  function scheduleTerminalLayoutSave(): void {
    if (terminalLayoutSaveTimer !== null) window.clearTimeout(terminalLayoutSaveTimer);
    terminalLayoutSaveTimer = window.setTimeout(() => {
      terminalLayoutSaveTimer = null;
      const thread = threadId;
      if (!thread || chat.selectedThreadId !== thread || terminals.length === 0) return;
      const terminalNames = terminals.map((terminal) => workspacePanelTabLabel(terminalWorkspacePanelTabKey(terminal.id)));
      const selectedIndex = selectedTerminalId === null
        ? -1
        : terminals.findIndex((terminal) => terminal.id === selectedTerminalId);
      void chatApi.saveChatTerminalPanelLayout(thread, {
        placement,
        terminalNames,
        selectedIndex: selectedIndex >= 0 ? selectedIndex : null,
        splitDirection: placement === "bottom" ? "horizontal" : "vertical",
        splitSizes: isTerminalSplit ? terminalNames.map(() => 1) : [],
      }).catch((reason: unknown) => { error = message(reason); });
    }, 250);
  }

  function openPanel(tab: ChatInspectorTab): void {
    const openTabs = openInspectorTab(panelState.openTabs, tab);
    const currentOrder = reconcileWorkspacePanelTabOrder(
      panelState.tabOrder,
      openTabs,
      terminals.map((terminal) => terminal.id),
    );
    update({
      tab,
      openTabs,
      tabOrder: currentOrder,
      ...(tab === "files" ? { fileTreeVisible: true } : {}),
    });
    panelPickerOpen = false;
    void tabController.reveal(tab);
  }

  function closePanel(tab: ChatInspectorTab): void {
    const currentOrder = orderedTabKeys;
    const remainingOrder = currentOrder.filter((key) => panelKind(key) !== tab);
    const tabNames = Object.fromEntries(
      Object.entries(panelState.tabNames).filter(([key]) => (
        panelKind(key as ChatWorkspacePanelTabKey) !== tab
      )),
    ) as Partial<Record<ChatWorkspacePanelTabKey, string>>;
    const next = closeInspectorTab(panelState.openTabs, tab, panelState.tab);
    if (next.tabs.length === 0) {
      onClose();
      return;
    }
    const selectedTab = panelState.tab === tab
      ? panelKind(remainingOrder[Math.min(
        Math.max(0, currentOrder.findIndex((key) => panelKind(key) === tab)),
        remainingOrder.length - 1,
      )] ?? remainingOrder[0] ?? initialPanelTab())
      : next.selectedTab ?? panelState.tab;
    update({
      openTabs: workspacePanelKinds(remainingOrder),
      tabOrder: remainingOrder,
      tabNames,
      tab: selectedTab,
    });
  }

  function selectPanel(tab: ChatInspectorTab): void {
    update({ tab, ...(tab === "files" ? { fileTreeVisible: true } : {}) });
  }

  function panelLabel(tab: (typeof panelTabs)[number]): string {
    if (tab.id !== "files" || !panelState.filePreviewPath) return t(`chat.inspector.${tab.label}`);
    return panelState.filePreviewPath.split("/").filter(Boolean).at(-1) ?? t("chat.inspector.files");
  }

  function defaultWorkspacePanelTabLabel(key: ChatWorkspacePanelTabKey): string {
    const terminalId = workspacePanelTerminalId(key);
    if (terminalId) {
      const terminal = terminals.find((candidate) => candidate.id === terminalId);
      return terminal ? terminalWorkspacePanelDefaultLabel(terminal.name) : t("chat.inspector.terminal");
    }
    if (key === "terminal") return t("chat.inspector.terminal");
    const panel = panelForKey(key);
    return panel ? panelLabel(panel) : t("chat.inspector.terminal");
  }

  function workspacePanelTabLabel(key: ChatWorkspacePanelTabKey): string {
    return panelState.tabNames[key] ?? defaultWorkspacePanelTabLabel(key);
  }

  function panelKind(key: ChatWorkspacePanelTabKey): ChatInspectorTab {
    return key.startsWith("terminal:") || key === "terminal"
      ? "terminal"
      : key as Exclude<ChatWorkspacePanelTabKey, `terminal:${string}`>;
  }

  function panelForKey(key: ChatWorkspacePanelTabKey): (typeof panelTabs)[number] | undefined {
    return panelTabs.find((candidate) => candidate.id === key);
  }

  async function loadTerminals(
    thread: string,
    workspace: string,
    scopeKey: string,
  ): Promise<void> {
    terminalsLoading = true;
    terminals = [];
    selectedTerminalId = null;
    error = null;
    try {
      const loaded = await withTerminalLoadLock(scopeKey, async () => {
        const [available, layout] = await Promise.all([
          chatApi.listChatTerminals(thread, workspace),
          chatApi.readChatTerminalLayout(thread),
        ]);
        const owned = terminalPanels.claimAvailable(available, placement);
        if (owned.length > 0) return { terminals: owned, layout };
        const snapshot = await createTerminal(thread, workspace);
        return { terminals: [snapshot.terminal], layout };
      });
      if (destroyed || terminalScopeKey !== scopeKey) return;
      terminals = loaded.terminals;
      const savedPanel = loaded.layout.groups.find((group) => group.placement === placement);
      if (savedPanel) {
        isTerminalSplit = savedPanel.splitSizes.length > 1;
        const tabNames = { ...panelState.tabNames };
        loaded.terminals.forEach((terminal, index) => {
          const name = savedPanel.terminalNames[index];
          if (name) tabNames[terminalWorkspacePanelTabKey(terminal.id)] = name;
        });
        update({ tabNames });
      }
      const placeholderName = panelState.tabNames.terminal;
      const firstTerminal = loaded.terminals[0];
      if (placeholderName && firstTerminal) {
        const tabNames = { ...panelState.tabNames };
        delete tabNames.terminal;
        tabNames[terminalWorkspacePanelTabKey(firstTerminal.id)] = placeholderName;
        update({ tabNames });
      }
      const remembered = terminalPanels.selected(thread, placement);
      const savedSelection = savedPanel?.selectedIndex === null || savedPanel?.selectedIndex === undefined
        ? null
        : loaded.terminals[savedPanel.selectedIndex]?.id ?? null;
      selectTerminal(loaded.terminals.some((terminal) => terminal.id === remembered)
        ? remembered ?? null
        : savedSelection ?? loaded.terminals[0]?.id ?? null);
    } catch (reason: unknown) {
      if (!destroyed && terminalScopeKey === scopeKey) error = message(reason);
    } finally {
      if (!destroyed && terminalScopeKey === scopeKey) terminalsLoading = false;
    }
  }

  async function createTerminal(
    thread: string,
    workspace: string,
  ): ReturnType<typeof chatApi.createChatTerminal> {
    const snapshot = await chatApi.createChatTerminal({
      terminalId: crypto.randomUUID(),
      threadId: thread,
      workingFolderId: workspace,
      columns: 80,
      rows: 24,
    });
    terminalPanels.assign(snapshot.terminal.id, placement);
    return snapshot;
  }

  async function addTerminal(): Promise<void> {
    const thread = threadId;
    const workspace = workingFolderId;
    if (!thread || !workspace) return;
    panelPickerOpen = false;
    const snapshot = await createTerminal(thread, workspace);
    if (destroyed || terminalScopeKey !== `${thread}:${workspace}`) return;
    const previousOrder = orderedTabKeys;
    terminals = [...terminals, snapshot.terminal];
    update({
      tabOrder: [
        ...previousOrder.filter((key) => key !== "terminal"),
        terminalWorkspacePanelTabKey(snapshot.terminal.id),
      ],
    });
    selectTerminal(snapshot.terminal.id);
    openPanel("terminal");
    await tabController.reveal(terminalWorkspacePanelTabKey(snapshot.terminal.id));
  }

  async function retryTerminalLoad(): Promise<void> {
    const thread = threadId;
    const workspace = workingFolderId;
    if (!thread || !workspace) throw new Error("Terminal retry needs a thread and a working folder");
    const scopeKey = `${thread}:${workspace}`;
    terminalScopeKey = scopeKey;
    await loadTerminals(thread, workspace, scopeKey);
    if (error) throw new Error(error);
  }

  function openOrAddTerminal(): void {
    if (panelState.openTabs.includes("terminal")) {
      void run(addTerminal);
      return;
    }
    openPanel("terminal");
  }

  function selectTerminal(terminalId: string | null): void {
    selectedTerminalId = terminalId;
    if (threadId) terminalPanels.select(threadId, placement, terminalId);
    scheduleTerminalLayoutSave();
  }

  function toggleTerminalSplit(): void {
    isTerminalSplit = !isTerminalSplit;
    scheduleTerminalLayoutSave();
  }

  function updateTerminal(terminal: ChatTerminalRead): void {
    terminals = terminals.map((entry) => entry.id === terminal.id ? terminal : entry);
  }

  async function closeTerminal(terminal: ChatTerminalRead): Promise<void> {
    const thread = threadId;
    const workspace = workingFolderId;
    if (!thread || !workspace) return;
    const scopeKey = `${thread}:${workspace}`;
    let result = await chatApi.closeChatTerminal(terminal.id, thread, workspace, false);
    if (destroyed || terminalScopeKey !== scopeKey) return;
    if (result.confirmationRequired) {
      if (!window.confirm(t("chat.inspector.confirmCloseTerminal"))) return;
      result = await chatApi.closeChatTerminal(terminal.id, thread, workspace, true);
      if (destroyed || terminalScopeKey !== scopeKey) return;
    }
    if (!result.closed) return;
    terminalPanels.release(terminal.id);
    const closedKey = terminalWorkspacePanelTabKey(terminal.id);
    const currentOrder = orderedTabKeys;
    const closedIndex = currentOrder.indexOf(closedKey);
    const remainingOrder = currentOrder.filter((key) => key !== closedKey);
    const tabNames = { ...panelState.tabNames };
    delete tabNames[closedKey];
    const next = terminals.filter((entry) => entry.id !== terminal.id);
    if (next.length === 0) {
      closePanel("terminal");
      terminals = next;
      return;
    }
    terminals = next;
    update({ tabOrder: remainingOrder, tabNames });
    if (selectedTerminalId === terminal.id) {
      const nearestKey = remainingOrder[Math.min(
        Math.max(0, closedIndex),
        remainingOrder.length - 1,
      )] ?? remainingOrder[0];
      const nearestTerminalId = nearestKey ? workspacePanelTerminalId(nearestKey) : null;
      selectTerminal(nearestTerminalId ?? next[0]?.id ?? null);
      if (nearestKey) selectPanel(panelKind(nearestKey));
    }
  }

  function trackTerminalOverflow(
    node: HTMLElement,
    details: { terminalId: string; label: string },
  ): { update: (next: { terminalId: string; label: string }) => void; destroy: () => void } {
    let terminalId = details.terminalId;
    const updateOverflow = () => {
      const overflows = node.scrollWidth > node.clientWidth;
      const included = fadedTerminalIds.includes(terminalId);
      if (overflows !== included) {
        fadedTerminalIds = overflows
          ? [...fadedTerminalIds, terminalId]
          : fadedTerminalIds.filter((id) => id !== terminalId);
      }
    };
    const observer = new ResizeObserver(updateOverflow);
    observer.observe(node);
    queueMicrotask(updateOverflow);
    return {
      update: (next) => {
        if (next.terminalId !== terminalId) {
          fadedTerminalIds = fadedTerminalIds.filter((id) => id !== terminalId);
          terminalId = next.terminalId;
        }
        queueMicrotask(updateOverflow);
      },
      destroy: () => {
        observer.disconnect();
        fadedTerminalIds = fadedTerminalIds.filter((id) => id !== terminalId);
      },
    };
  }

  async function run(operation: () => Promise<void>): Promise<void> {
    error = null;
    try {
      await operation();
    } catch (reason: unknown) {
      if (!destroyed) error = message(reason);
    }
  }

  onDestroy(() => {
    destroyed = true;
    reviewOpenRequest += 1;
    tabController.destroy();
    if (terminalLayoutSaveTimer !== null) window.clearTimeout(terminalLayoutSaveTimer);
  });

  function activateWorkspacePanelTab(key: ChatWorkspacePanelTabKey): void {
    const terminalId = workspacePanelTerminalId(key);
    if (terminalId) selectTerminal(terminalId);
    selectPanel(panelKind(key));
  }

  function preventMiddleButtonScroll(event: MouseEvent): void {
    if (event.button === 1) event.preventDefault();
  }

  function closeTabFromAuxClick(
    event: MouseEvent,
    key: ChatWorkspacePanelTabKey,
    terminal?: ChatTerminalRead,
  ): void {
    if (event.button !== 1) return;
    event.preventDefault();
    event.stopPropagation();
    if (terminal) void run(() => closeTerminal(terminal));
    else closePanel(panelKind(key));
  }

  function message(reason: unknown): string {
    return terminalErrorMessage(
      reason,
      t("common.viewLoadFailed", t("chat.inspector.terminal")),
    );
  }

  function rect(value: DOMRect): SelectPopoverRect {
    return {
      top: value.top,
      right: value.right,
      bottom: value.bottom,
      left: value.left,
      width: value.width,
      height: value.height,
    };
  }

  /** Full picker height including rows hidden by the inner scroll list, so placement stays correct after a max height applies. */
  function panelPickerContentHeight(): number | null {
    if (!panelPicker || !panelPickerList) return null;
    return panelPickerList.scrollHeight + panelPicker.offsetHeight - panelPicker.clientHeight;
  }

  function positionPanelPicker(): void {
    if (!panelPickerTrigger) return;
    const triggerRect = panelPickerTrigger.getBoundingClientRect();
    panelPickerGeometry = pickSelectPopoverGeometry({
      triggerRect: rect(triggerRect),
      boundaryRect: {
        top: 0,
        right: window.innerWidth,
        bottom: window.innerHeight,
        left: 0,
        width: window.innerWidth,
        height: window.innerHeight,
      },
      contentHeight: panelPickerContentHeight() ?? 300,
      contentWidth: panelPicker?.scrollWidth ?? FLOATING_WIDTH.md,
      horizontalAlign: triggerRect.left + FLOATING_WIDTH.md <= window.innerWidth - 8 ? "start" : "end",
    });
    panelPickerReady = true;
  }

  function panelPickerStyle(): string {
    if (!panelPickerReady) return "visibility:hidden;top:0;left:0;";
    const width = Math.min(FLOATING_WIDTH.md, panelPickerGeometry.maxWidth);
    return `visibility:visible;top:${panelPickerGeometry.top}px;left:${panelPickerGeometry.left}px;width:${width}px;max-height:${panelPickerGeometry.maxHeight}px;`;
  }

  async function togglePanelPicker(focusFirst = false): Promise<void> {
    if (panelPickerOpen) {
      panelPickerOpen = false;
      return;
    }
    panelPickerReady = false;
    panelPickerOpen = true;
    await tick();
    positionPanelPicker();
    if (focusFirst) panelPicker?.querySelector<HTMLButtonElement>("button")?.focus();
  }

  function handlePanelPickerTriggerKeydown(event: KeyboardEvent): void {
    if (!["ArrowDown", "ArrowUp"].includes(event.key)) return;
    event.preventDefault();
    void togglePanelPicker(true);
  }

  function handlePanelPickerKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      panelPickerOpen = false;
      queueMicrotask(() => panelPickerTrigger?.focus());
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const buttons = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>("button")];
    if (buttons.length === 0) return;
    event.preventDefault();
    const currentIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const nextIndex = event.key === "Home"
      ? 0
      : event.key === "End"
        ? buttons.length - 1
        : (Math.max(0, currentIndex) + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
    buttons[nextIndex]?.focus();
  }

  $effect(() => {
    if (!panelPickerOpen) return;
    const handleOutsidePointer = (event: MouseEvent) => {
      if (!(event.target instanceof Node)) return;
      if (panelPickerTrigger?.contains(event.target) || panelPicker?.contains(event.target)) return;
      panelPickerOpen = false;
    };
    const handleViewportChange = () => positionPanelPicker();
    window.addEventListener("mousedown", handleOutsidePointer, true);
    window.addEventListener("resize", handleViewportChange);
    window.addEventListener("scroll", handleViewportChange, true);
    return () => {
      window.removeEventListener("mousedown", handleOutsidePointer, true);
      window.removeEventListener("resize", handleViewportChange);
      window.removeEventListener("scroll", handleViewportChange, true);
    };
  });

  $effect(() => {
    if (!tabController.renameState) return;
    const handleOutsidePointer = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || tabController.renamePanel?.contains(event.target)) return;
      tabController.closeRename();
    };
    const handleViewportChange = () => tabController.positionRename();
    window.addEventListener("pointerdown", handleOutsidePointer, true);
    window.addEventListener("resize", handleViewportChange);
    return () => {
      window.removeEventListener("pointerdown", handleOutsidePointer, true);
      window.removeEventListener("resize", handleViewportChange);
    };
  });
</script>

<section class="workspace-panel" data-placement={placement} aria-label={placement === "bottom" ? t("chat.bottomPanel") : t("chat.inspector.title")}>
  <div class="panel-tabbar" class:reordering={tabController.draggedKey !== null}>
      <div bind:this={tabController.tabbar} class="panel-tab-strip" role="tablist" aria-label={placement === "bottom" ? t("chat.bottomPanel") : t("chat.inspector.title")} onwheel={(event) => tabController.handleWheel(event)}>
        {#each orderedTabs as item (item.key)}
        <div role="presentation" class="panel-tab-slot" class:dragging={tabController.draggedKey === item.key} data-panel-tab-key={item.key} style:--tab-shift-x={`${tabController.dragShift(item.key)}px`} onpointerdown={(event) => tabController.beginDrag(event, item.key)} onmousedown={preventMiddleButtonScroll} onauxclick={(event) => closeTabFromAuxClick(event, item.key, item.type === "terminal" ? item.terminal : undefined)} oncontextmenu={(event) => tabController.handleContextMenu(event, item.key)}>
          {#if item.type === "loading-terminal"}
            <button type="button" role="tab" aria-selected={panelState.tab === "terminal"} tabindex={panelState.tab === "terminal" ? 0 : -1} class="terminal-tab loading" class:active={panelState.tab === "terminal"} title={workspacePanelTabLabel(item.key)} onclick={() => selectPanel("terminal")} onkeydown={(event) => tabController.handleKeydown(event, item.key)}>
              <SquareTerminal size={13} /><span class="tab-label">{workspacePanelTabLabel(item.key)}</span>
            </button>
          {:else if item.type === "terminal"}
            <div class="terminal-tab-shell" class:active={panelState.tab === "terminal" && selectedTerminalId === item.terminal.id}>
              <button type="button" role="tab" aria-selected={panelState.tab === "terminal" && selectedTerminalId === item.terminal.id} tabindex={panelState.tab === "terminal" && selectedTerminalId === item.terminal.id ? 0 : -1} class="terminal-tab" onclick={() => { selectTerminal(item.terminal.id); selectPanel("terminal"); }} onkeydown={(event) => tabController.handleKeydown(event, item.key)} title={workspacePanelTabLabel(item.key)}>
                <SquareTerminal size={13} />
                <span class="tab-label terminal-label" class:faded={fadedTerminalIds.includes(item.terminal.id)} use:trackTerminalOverflow={{ terminalId: item.terminal.id, label: workspacePanelTabLabel(item.key) }}>{workspacePanelTabLabel(item.key)}</span>
              </button>
              <button type="button" class="tab-close terminal-close" aria-label={t("chat.inspector.closeTerminal")} onclick={() => void run(() => closeTerminal(item.terminal))}><X size={11} /></button>
            </div>
          {:else}
            {@const Icon = item.panel.icon}
            <div class="panel-tab-shell" class:active={panelState.tab === item.panel.id}>
              <button type="button" role="tab" aria-selected={panelState.tab === item.panel.id} tabindex={panelState.tab === item.panel.id ? 0 : -1} class="panel-tab" title={workspacePanelTabLabel(item.key)} onclick={() => selectPanel(item.panel.id)} onkeydown={(event) => tabController.handleKeydown(event, item.key)}>
                {#if item.panel.id === "files" && panelState.filePreviewPath}<ChatFileIcon path={panelState.filePreviewPath} size={13} />{:else}<Icon size={13} />{/if}<span class="tab-label">{workspacePanelTabLabel(item.key)}</span>
              </button>
              <button type="button" class="tab-close" aria-label={t("chat.inspector.closePanel", workspacePanelTabLabel(item.key))} onclick={() => closePanel(item.panel.id)}><X size={11} /></button>
            </div>
          {/if}
        </div>
        {/each}
      </div>

    <button
      bind:this={panelPickerTrigger}
      type="button"
      class="panel-add-button"
      aria-label={t("chat.inspector.addPanel")}
      aria-haspopup="menu"
      aria-expanded={panelPickerOpen}
      data-app-tooltip-disabled="true"
      data-app-tooltip-focus-disabled="true"
      onclick={() => void togglePanelPicker()}
      onkeydown={handlePanelPickerTriggerKeydown}
    ><Plus size={14} /></button>
    {#if panelState.tab === "terminal" && terminals.length > 1}
      <button type="button" class="panel-add-button" class:active={isTerminalSplit} aria-pressed={isTerminalSplit} title={t("chat.inspector.toggleTerminalSplit")} aria-label={t("chat.inspector.toggleTerminalSplit")} onclick={toggleTerminalSplit}><Columns2 size={14} /></button>
    {/if}
    {#if panelPickerOpen}
      <div
        bind:this={panelPicker}
        use:portal
        class="panel-picker surface-floating flex flex-col overflow-hidden"
        role="menu"
        tabindex="-1"
        aria-label={t("chat.inspector.addPanel")}
        data-app-floating-surface
        data-placement={panelPickerGeometry.placement}
        style={panelPickerStyle()}
        onkeydown={handlePanelPickerKeydown}
      >
        <div bind:this={panelPickerList} class="surface-floating-body min-h-0 flex-1 overflow-y-auto" use:scrollEdgeFadeAction>
          <p class="menu-label">{t("chat.inspector.addPanel")}</p>
          <button type="button" class="menu-item" role="menuitem" onclick={openOrAddTerminal}>
            <SquareTerminal size={15} />
            <span><strong>{t("chat.inspector.terminal")}</strong><small>{t("chat.inspector.terminalDescription")}</small></span>
          </button>
          {#each panelTabs as tab (tab.id)}
            {@const Icon = tab.icon}
            <button type="button" class="menu-item" role="menuitem" onclick={() => openPanel(tab.id)}>
              <Icon size={15} />
              <span><strong>{t(`chat.inspector.${tab.label}`)}</strong><small>{t(`chat.inspector.${tab.label}Description`)}</small></span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    {#if tabController.renameState}
      <div
        bind:this={tabController.renamePanel}
        use:portal
        class="tab-rename-panel surface-floating"
        role="dialog"
        tabindex="-1"
        aria-labelledby={`${placement}-tab-rename-title`}
        data-app-floating-surface
        style={tabController.renameStyle()}
        onkeydown={(event) => tabController.handleRenameKeydown(event)}
      >
        <form onsubmit={(event) => { event.preventDefault(); tabController.saveName(); }}>
          <label id={`${placement}-tab-rename-title`} for={`${placement}-tab-rename-input`}>{t("chat.inspector.renameTab")}</label>
          <input
            bind:this={tabController.renameInput}
            class="field w-full"
            id={`${placement}-tab-rename-input`}
            data-app-shortcuts="ignore"
            maxlength={CHAT_WORKSPACE_PANEL_TAB_NAME_MAX_LENGTH}
            autocomplete="off"
            spellcheck="false"
            bind:value={tabController.renameDraft}
            aria-label={t("chat.inspector.tabName")}
          />
          <div class="tab-rename-actions">
            <button type="button" onclick={() => tabController.resetName()}>{t("common.reset")}</button>
            <span></span>
            <button type="button" onclick={() => tabController.closeRename(true)}>{t("common.cancel")}</button>
            <button type="submit" class="primary" disabled={normalizeWorkspacePanelTabName(tabController.renameDraft) === null}>{t("common.save")}</button>
          </div>
        </form>
      </div>
    {/if}

    <span class="flex-1"></span>
    {#if placement === "bottom"}
      <button type="button" class="chat-icon-button" aria-label={t("chat.closeBottomPanel")} onclick={onClose}><X size={14} /></button>
    {/if}
  </div>

  {#if error && !terminalLoadFailed}<Notice class="border-b border-border p-2 text-xs" message={error} />{/if}
  <div class="min-h-0 flex-1" role="tabpanel" aria-label={t(`chat.inspector.${panelState.tab}`)}>
    {#if panelState.openTabs.includes("review")}
      <div class="retained-workspace-panel" class:hidden={panelState.tab !== "review"}>
        <ChatReviewPanel
          active={visible && panelState.tab === "review"}
          source={panelState.reviewSource}
          sourceThreadId={panelState.reviewThreadId}
          sourceWorkingFolderId={panelState.reviewWorkingFolderId}
          sourceExecutionEnvironmentId={panelState.reviewExecutionEnvironmentId}
          selectedFile={panelState.selectedFile}
          layoutPreference={panelState.reviewLayoutPreference}
          whitespaceIgnored={panelState.whitespaceIgnored}
          diffView={panelState.diffView}
          onStateChange={(change) => update({
            ...(change.source === undefined ? {} : { reviewSource: change.source }),
            ...(change.selectedFile === undefined ? {} : { selectedFile: change.selectedFile }),
            ...(change.layoutPreference === undefined ? {} : { reviewLayoutPreference: change.layoutPreference }),
            ...(change.whitespaceIgnored === undefined ? {} : { whitespaceIgnored: change.whitespaceIgnored }),
            ...(change.diffView === undefined ? {} : { diffView: change.diffView }),
          })}
        />
      </div>
    {/if}
    {#if panelState.openTabs.includes("files")}
      <div class="retained-workspace-panel" class:hidden={panelState.tab !== "files"}>
        <ChatFilesPanel
          active={visible && panelState.tab === "files"}
          directoryPath={panelState.fileBrowserPath}
          selectedPath={panelState.filePreviewPath}
          treeVisible={panelState.fileTreeVisible}
          treeWidthPx={panelState.fileTreeWidthPx}
          onStateChange={(change) => update({
            ...(change.directoryPath === undefined ? {} : { fileBrowserPath: change.directoryPath }),
            ...(change.selectedPath === undefined ? {} : { filePreviewPath: change.selectedPath }),
            ...(change.treeVisible === undefined ? {} : { fileTreeVisible: change.treeVisible }),
            ...(change.treeWidthPx === undefined ? {} : { fileTreeWidthPx: change.treeWidthPx }),
          })}
          onReviewCreated={() => openPanel("review")}
        />
      </div>
    {/if}
    {#if panelState.tab === "terminal"}
      {#if terminalsLoading}
        <p class="grid h-full place-items-center text-xs text-muted-foreground">{t("common.loading")}</p>
      {:else if isTerminalSplit && terminals.length > 1}
        <div class="terminal-split" data-direction={placement === "bottom" ? "horizontal" : "vertical"}>
          {#each terminals as terminal (terminal.id)}
            <section aria-label={workspacePanelTabLabel(terminalWorkspacePanelTabKey(terminal.id))}>
              <header>{workspacePanelTabLabel(terminalWorkspacePanelTabKey(terminal.id))}</header>
              <ChatTerminalView terminalRead={terminal} onState={updateTerminal} />
            </section>
          {/each}
        </div>
      {:else if selectedTerminal}
        <div class="flex h-full min-h-0 flex-col">
          {#if !selectedTerminal.running}
            <div class="terminal-stopped-bar">
              <span>{selectedTerminal.exitCode === null ? t("chat.inspector.terminalStopped") : t("chat.inspector.terminalExited", formatNumber(localization.locale, selectedTerminal.exitCode))}</span>
            </div>
          {/if}
          {#key `${selectedTerminal.id}:${selectedTerminal.generation}`}
            <ChatTerminalView terminalRead={selectedTerminal} onState={updateTerminal} />
          {/key}
        </div>
      {:else}
        <div class="h-full text-xs">
          <Notice layout="fill" icon={SquareTerminal} message={error ?? t("chat.inspector.terminalUnavailable")} onRetry={retryTerminalLoad} copy={error ?? false} />
        </div>
      {/if}
    {:else if panelState.tab === "plan"}
      <ChatPlanPanel />
    {:else if panelState.tab === "sourceControl"}
      <ChatSourceControlPanel />
    {:else if panelState.tab === "browser"}
      <ChatBrowserPanel />
    {/if}
  </div>
</section>

<style>
  .panel-picker :global(svg.lucide), .tab-rename-panel :global(svg.lucide) { stroke-width: var(--icon-stroke-width-compact) !important; }
  .workspace-panel { display: flex; height: 100%; min-height: 0; flex-direction: column; background: var(--cal-bg); }
  .retained-workspace-panel { height: 100%; min-height: 0; }
  .panel-tabbar { --workspace-panel-tab-width: 9.5rem; position: relative; display: flex; min-height: 2.65rem; flex: 0 0 auto; align-items: center; gap: 0.2rem; padding-inline: 0.45rem; }
  .workspace-panel[data-placement="inspector"] .panel-tabbar { height: var(--cal-header-row-h); min-height: var(--cal-header-row-h); border-bottom: 1px solid var(--sidebar); background: var(--cal-header-bg); padding-right: var(--chat-global-actions-width); }
  .panel-tabbar.reordering { user-select: none; }
  .panel-tabbar > :global(.chat-icon-button) { flex: 0 0 auto; align-self: center; }
  .panel-add-button { display: grid; width: 1.75rem; height: 1.75rem; flex: 0 0 1.75rem; place-items: center; align-self: center; border-radius: 0.375rem; color: var(--foreground); transition: color 120ms ease, background-color 120ms ease; }
  .panel-add-button:hover, .panel-add-button:focus-visible, .panel-add-button[aria-expanded="true"] { background: var(--accent); color: var(--accent-foreground); }
  .panel-add-button.active { background: var(--accent); color: var(--foreground); }
  .panel-tab-strip { display: flex; min-width: 0; flex: 0 1 auto; align-items: stretch; gap: 0.2rem; overflow-x: auto; overflow-y: hidden; scrollbar-width: none; }
  .panel-tab-strip::-webkit-scrollbar { display: none; }
  .panel-tab-slot { display: flex; width: var(--workspace-panel-tab-width); min-width: 3.75rem; flex: 0 1 var(--workspace-panel-tab-width); transform: translate3d(var(--tab-shift-x), 0, 0); align-items: stretch; transition: transform 140ms cubic-bezier(0.2, 0, 0, 1); }
  .panel-tabbar.reordering .panel-tab-slot { will-change: transform; }
  .panel-tab-slot.dragging { z-index: 1; transition: none; }
  .terminal-tab, .panel-tab { display: flex; min-width: 0; min-height: 2rem; flex: 1 1 auto; align-items: center; gap: 0.4rem; overflow: hidden; padding: 0.3rem 0.65rem; color: inherit; font-size: calc(0.733333rem * var(--type-scale)); }
  .terminal-tab > :global(svg), .panel-tab > :global(svg) { flex: 0 0 auto; }
  .terminal-tab.loading { width: 100%; border-radius: 0.55rem; color: var(--foreground); }
  .terminal-tab-shell, .panel-tab-shell { display: flex; width: 100%; min-width: 0; align-items: stretch; border-radius: 0.55rem; color: var(--foreground); }
  .terminal-tab-shell:hover, .panel-tab-shell:hover, .terminal-tab.loading:hover { background: color-mix(in srgb, var(--accent) 70%, transparent); color: var(--foreground); }
  .terminal-tab-shell.active, .panel-tab-shell.active, .terminal-tab.loading.active { background: var(--accent); color: var(--foreground); }
  .tab-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .terminal-label.faded { text-overflow: clip; -webkit-mask-image: linear-gradient(to right, black calc(100% - 1.4rem), transparent); mask-image: linear-gradient(to right, black calc(100% - 1.4rem), transparent); }
  .panel-tab { padding-right: 0.2rem; }
  .tab-close { display: grid; width: 1.5rem; flex: 0 0 auto; place-items: center; border-radius: 0.3rem; opacity: 0; }
  .terminal-tab-shell:hover .tab-close, .panel-tab-shell:hover .tab-close, .tab-close:focus-visible { opacity: 1; }
  .tab-close:hover { background: var(--accent); }
  .terminal-close { margin-right: 0.2rem; }
  .terminal-stopped-bar { min-height: 1.8rem; flex: 0 0 auto; border-bottom: 1px solid var(--border); padding: 0.45rem 0.55rem; color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  .terminal-split { display: grid; height: 100%; min-height: 0; grid-template-columns: repeat(auto-fit, minmax(min(20rem, 100%), 1fr)); overflow: hidden; }
  .terminal-split[data-direction="vertical"] { grid-template-columns: 1fr; grid-template-rows: repeat(auto-fit, minmax(min(12rem, 100%), 1fr)); }
  .terminal-split > section { display: flex; min-width: 0; min-height: 0; flex-direction: column; overflow: hidden; border-right: 1px solid var(--border); border-bottom: 1px solid var(--border); }
  .terminal-split > section > header { min-height: 1.65rem; flex: 0 0 auto; overflow: hidden; border-bottom: 1px solid var(--border); padding: 0.3rem 0.45rem; text-overflow: ellipsis; white-space: nowrap; color: var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  .panel-picker { position: fixed; z-index: 80; }
  .panel-picker button { align-items: flex-start; gap: 0.65rem; padding-block: 0.45rem; }
  .panel-picker button > :global(svg) { margin-top: 0.1rem; flex: 0 0 auto; }
  .panel-picker span { display: grid; min-width: 0; gap: 0.1rem; }
  .panel-picker strong { font-weight: 500; }
  .panel-picker small { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); line-height: 1.3; }
  .tab-rename-panel { position: fixed; z-index: 90; overflow-y: auto; padding: 0.6rem; }
  .tab-rename-panel label { display: block; margin-bottom: 0.4rem; font-weight: 500; }
  .tab-rename-actions { display: flex; align-items: center; gap: 0.25rem; margin-top: 0.55rem; }
  .tab-rename-actions > span { flex: 1; }
  .tab-rename-actions > button { border-radius: var(--floating-item-radius); padding: 0.35rem 0.5rem; font-size: var(--panel-detail-font-size); }
  .tab-rename-actions > button:hover { background: var(--accent); }
  .tab-rename-actions > button.primary { background: var(--primary); color: var(--primary-foreground); }
  .tab-rename-actions > button.primary:hover { background: color-mix(in srgb, var(--primary) 88%, transparent); }
  .tab-rename-actions > button:disabled { opacity: 0.45; }
  @container chat-shell (max-width: 520px) { .panel-tabbar { --workspace-panel-tab-width: 3.75rem; } .tab-label { display: none; } .tab-close { width: 1.3rem; opacity: 1; } }
  @media (hover: none) { .tab-close { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .panel-tab-slot { transition: none; } }
</style>
