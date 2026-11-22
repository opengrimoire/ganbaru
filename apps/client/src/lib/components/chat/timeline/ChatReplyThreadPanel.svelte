<script lang="ts">
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import X from "@lucide/svelte/icons/x";
  import { tick, untrack } from "svelte";
  import * as chatApi from "$lib/api/chat";
  import type { ChatAgentRunRead, ChatThreadShellRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import {
    agentRunRestart,
    agentRunLocation,
    agentMayUseChannelFolders,
    exactExecutionReadKey,
    exactExecutionRuns,
    activeWorkWithoutRun,
    exactRunPresentationsReady,
    failureShownByReply,
    latestRenderableAgentRun,
    planExactExecutionReads,
    remoteReplyThreadRenderEntries,
    remoteReplyWork,
    replyThreadRenderEntries,
    retryableFailureEntryKey,
    shouldGroupReplyMessages,
    type CachedExactExecution,
    type ExactExecutionRead,
  } from "$lib/chat/timeline/reply-thread-model";
  import { settledLiveExecutions } from "$lib/chat/timeline/live-execution-cache";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { organizationalScrollFollowsEnd } from "$lib/chat/timeline/organizational-scroll";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    loadChatScratchManager,
    type ChatScratchManagerComponent,
  } from "$lib/chat/workspace/local-execution-ui";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { useChat } from "$lib/stores/chat.svelte";
  import ChatAgentRunStatus from "./ChatAgentRunStatus.svelte";
  import ChatRemoteWorkRow from "./ChatRemoteWorkRow.svelte";
  import ChatLiveExecution from "./ChatLiveExecution.svelte";
  import ChatExecutionTimeline from "./ChatExecutionTimeline.svelte";
  import ChatMessageComposer from "$lib/components/chat/composer/ChatMessageComposer.svelte";
  import { chatComposerDock, type ChatComposerDockMetrics } from "$lib/components/chat/composer/composer-dock";
  import ChatOrganizationalMessage from "./ChatOrganizationalMessage.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ChatRequestPanel from "./ChatRequestPanel.svelte";
  import ChatRemoteApprovalPanel from "./ChatRemoteApprovalPanel.svelte";
  import { answerableHostQuestion, answerableRemoteApproval } from "$lib/chat/timeline/agent-update";
  import { chatComposerDocumentFromText, chatComposerDocumentVersioned, chatComposerMarkdown } from "$lib/chat/composer/rich-text";
  import { chatParticipantDisplayName } from "$lib/chat/agents/participant-display";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import ChatDelegatedWorkList from "./ChatDelegatedWorkList.svelte";
  import ChatDelegationOrigin from "./ChatDelegationOrigin.svelte";
  import ChatSecretRequests from "./ChatSecretRequests.svelte";

  let {
    presentation = "complementary",
    reserveGlobalActions = false,
    onClose,
  }: {
    presentation?: "complementary" | "dialog" | "main";
    reserveGlobalActions?: boolean;
    onClose: () => void;
  } = $props();

  const chat = useChat();
  const localization = getLocalization();
  const { t } = localization;
  /** A phone shows the thread in a sheet that a swipe down closes, so it leaves out the header row and its tab. */
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  /** On a phone the timeline fades only toward its composer; the sheet marks scrolled content above with a divider. */
  function threadScrollFade(node: HTMLElement) {
    return mobileShell ? scrollEdgeFadeAction(node, { edges: "bottom" }) : undefined;
  }
  const localExecutionAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "chat.local-execution",
  );
  /** Most work of one thread whose runs a phone reads; older work keeps its replies. */
  const REMOTE_REPLY_WORK_LIMIT = 8;
  let scroller = $state<HTMLDivElement | null>(null);
  /** The thread entries, observed so content that grows after a restore, such as a loaded execution, keeps the end pinned. */
  let canvas = $state<HTMLDivElement | null>(null);
  let actionError = $state<string | null>(null);
  /** A failed message action; unlike `actionError`, it never decides whether the thread can render. */
  let messageActionError = $state<string | null>(null);
  let executionSelectionRequest = 0;
  let executionThreadScope = $state<string | null>(null);
  let exactExecutionScope = "";
  let exactExecutionThreadId: string | null = null;
  let exactExecutionCache = new Map<string, CachedExactExecution<ExactExecutionRead>>();
  const exactExecutionReadsInFlight = new Set<string>();
  let exactExecutions = $state<Record<string, ExactExecutionRead>>({});
  let exactExecutionError = $state<string | null>(null);
  let scratchManagerOpen = $state(false);
  let scratchManagerLoading = $state(false);
  let ChatScratchManager = $state<ChatScratchManagerComponent | null>(null);
  let scratchManagerLoad: Promise<void> | null = null;
  let followingEnd = true;
  let restoredDestination: string | null = null;
  /** Height of the composer dock floating over the thread end, reserved below the last entry. */
  let dockHeight = $state(0);
  const page = $derived(chat.replyThread);
  /** Whether the composer dock floats over the thread end. */
  const composerDocked = $derived(Boolean(page) && !chat.selectedChannel?.archivedAt);
  const assignment = $derived(page?.assignment ?? null);
  const executionAgent = $derived(assignment?.agent
    ?? page?.thread.participants.find((participant) => participant.kind === "ai_agent")
    ?? null);
  const executionRun = $derived(latestRenderableAgentRun(page?.agentRuns ?? []));
  /** Active work that no run in this thread stands for, such as work another linked device runs. */
  const workWithoutRun = $derived(activeWorkWithoutRun(assignment, page?.agentRuns ?? []));
  const hasPrivateScratch = $derived((page?.agentRuns ?? []).some((run) => (
    run.scratchGenerationId !== null
  )));
  const runLocation = $derived(agentRunLocation(page?.agentRuns ?? [], chat.workingFolders));
  /** Whether the run's agent may use folders in this channel, read only when its newest run is conversation only. */
  let conversationOnlyMayUseFolders = $state(false);
  let conversationOnlyAccessRequest = 0;
  const conversationOnlyAgentId = $derived(runLocation?.kind === "conversation" ? executionAgent?.id ?? null : null);
  const runLocationLabel = $derived(runLocation?.kind === "folder"
    ? t("chat.organization.workingIn", runLocation.name)
    : runLocation?.kind === "conversation" && conversationOnlyMayUseFolders
      ? t("chat.organization.conversationOnlyNoDefaultFolder")
      : null);
  const loadedExecutionTurnIds = $derived(new Set([
    ...chat.timelineItems.flatMap((item) => item.turnId ? [item.turnId] : []),
    ...chat.timelinePages.flatMap((timelinePage) => timelinePage.turns.map((turn) => turn.turnId)),
  ]));
  const loadedExecutionRuns = $derived((page?.agentRuns ?? []).filter((run) => (
    exactExecutions[run.id] !== undefined
    || run.providerExecutionThreadId === chat.selectedThreadId
      && loadedExecutionTurnIds.has(run.providerExecutionTurnId)
  )));
  const loadedExecutionRunIds = $derived(new Set(loadedExecutionRuns.map((run) => run.id)));
  const executionPresentationReady = $derived(
    exactRunPresentationsReady(page?.agentRuns ?? [], loadedExecutionRunIds)
    || exactExecutionError !== null
    || actionError !== null,
  );
  /** Work of the thread this device reads from the linked desktop that executes it, on a device that executes nothing. */
  const remoteWork = $derived(localExecutionAvailable
    ? []
    : remoteReplyWork(page?.replies ?? [], assignment, REMOTE_REPLY_WORK_LIMIT));
  /** The run each remote work shows whole, by assignment id, as its live view reports it. Assignment ids are unique
   * across threads, so a view of another thread cannot stand for this one. */
  let remoteRunsShown = $state<Record<string, string | null>>({});
  const remoteShown = $derived(new Map(remoteWork.flatMap((work) => {
    // Until its live view reports, a work shows the settled run held this session, so the thread opens with its turn.
    const runId = work.assignmentId in remoteRunsShown
      ? remoteRunsShown[work.assignmentId]
      : settledLiveExecutions.shownRun(work.assignmentId);
    if (runId === null || runId === undefined) return [];
    const active = workWithoutRun?.id === work.assignmentId;
    return [[work.assignmentId, { runId, active }] as const];
  })));
  const renderEntries = $derived(localExecutionAvailable
    ? replyThreadRenderEntries(page?.replies ?? [], page?.agentRuns ?? [], loadedExecutionRunIds)
    : remoteReplyThreadRenderEntries(page?.replies ?? [], remoteWork, remoteShown));
  /** The one failure in the thread that offers Retry, while the thread's work is still failed. */
  const retryableFailureKey = $derived(retryableFailureEntryKey(renderEntries, assignment));
  const destination = $derived(chat.openReplyThreadId
    ? `reply-thread:${chat.openReplyThreadId}`
    : "reply-thread:none");
  const preferences = getPreferences();
  /** Changes to remount the composer after its draft was replaced from outside, such as by an answer choice. */
  let composerRevision = $state(0);
  const answerableQuestion = $derived(answerableHostQuestion(page?.replies ?? [], assignment));
  /** Approvals this device answered on the linked desktop, hidden until the synced work row moves on. */
  let answeredRemoteApprovals = $state<readonly string[]>([]);
  /** The approval a phone can answer on the linked desktop that runs the thread's work. */
  const remoteApproval = $derived.by(() => {
    if (localExecutionAvailable) return null;
    const approval = answerableRemoteApproval(page?.replies ?? [], assignment);
    return approval && !answeredRemoteApprovals.includes(approval.requestId) ? approval : null;
  });
  const executionAgentName = $derived(executionAgent
    ? chatParticipantDisplayName(executionAgent, preferences.profileDisplayName, t("chat.timeline.you"))
    : null);

  /** Replaces the composer text with a suggested answer, keeping its attachments and target. */
  function chooseAnswer(choice: string): void {
    const document = chatComposerDocumentFromText(choice);
    const current = chat.organizationalDraft(destination);
    chat.setOrganizationalDraft(destination, {
      ...current,
      normalizedMarkdown: chatComposerMarkdown(document),
      richContent: chatComposerDocumentVersioned(document),
      references: [],
      selectionStart: choice.length,
      selectionEnd: choice.length,
    });
    composerRevision += 1;
  }

  function openThread(replyThreadId: string): void {
    void chat.openReplyThread(replyThreadId).catch(reportMessageActionError);
  }

  function isSameDay(left: string, right: string): boolean {
    return new Date(left).toDateString() === new Date(right).toDateString();
  }

  /** Records the run a remote work shows whole, or that it shows none. */
  function noteRemoteRunShown(assignmentId: string, runId: string | null): void {
    // Called from a live view's effect, which must not come to depend on every other view's run.
    const current = untrack(() => remoteRunsShown);
    if (assignmentId in current && current[assignmentId] === runId) return;
    remoteRunsShown = { ...current, [assignmentId]: runId };
  }

  function entryCreatedAt(entry: (typeof renderEntries)[number]): string {
    if (entry.kind === "message") return entry.message.createdAt;
    return entry.kind === "live" ? entry.createdAt : entry.run.createdAt;
  }

  /** Names how a run restarted its provider session, or `null` for an ordinary start. */
  function runRestartLabel(run: ChatAgentRunRead): string | null {
    switch (agentRunRestart(run)) {
      case "handoff": return t("chat.runs.attemptReason.handoff");
      case "retry": return t("chat.runs.attemptReason.retry");
      case null: return null;
    }
  }

  function preventMiddleButtonScroll(event: MouseEvent): void {
    if (event.button === 1) event.preventDefault();
  }

  function closeFromMiddleClick(event: MouseEvent): void {
    if (event.button !== 1) return;
    event.preventDefault();
    event.stopPropagation();
    onClose();
  }

  /** Reloads the reply thread history that failed and rejects when the load fails again. */
  function retryReplyThread(): Promise<void> {
    if (page) return chat.loadOlderReplyThreadMessages();
    const replyThreadId = chat.openReplyThreadId;
    if (!replyThreadId) return Promise.reject(new Error("No reply thread is open to reload"));
    return chat.loadReplyThread(replyThreadId, true);
  }

  function rememberScrollPosition(): void {
    if (!scroller) return;
    followingEnd = organizationalScrollFollowsEnd(scroller);
    chat.setOrganizationalScrollPosition(destination, scroller.scrollTop);
  }

  function scrollToBottomForUserAction(): void {
    const element = scroller;
    const key = destination;
    if (!element) return;
    followingEnd = true;
    void tick().then(() => {
      if (element !== scroller || key !== destination) return;
      element.scrollTop = element.scrollHeight;
      chat.setOrganizationalScrollPosition(key, element.scrollTop);
    });
  }

  function loadScratchManager(): Promise<void> {
    if (ChatScratchManager) return Promise.resolve();
    scratchManagerLoad ??= loadChatScratchManager()
      .then((component) => { ChatScratchManager = component; })
      .finally(() => { scratchManagerLoad = null; });
    return scratchManagerLoad;
  }

  function reportMessageActionError(error: unknown): void {
    messageActionError = chatErrorMessage(error);
  }

  /** Retries the thread's failed work; a rejection reports itself and marks the Retry button as failed. */
  async function retryFailedWork(): Promise<boolean> {
    const failed = assignment;
    if (failed?.state !== "failed") return false;
    const replyThreadId = chat.openReplyThreadId;
    messageActionError = null;
    try {
      await chat.retryAssignment(failed.id);
      // A retry that settles after another thread opened leaves that thread's scroll position alone.
      if (chat.openReplyThreadId === replyThreadId) scrollToBottomForUserAction();
      return true;
    } catch (cause: unknown) {
      // The failure belongs to the thread the retry started in, not to one opened since.
      if (chat.openReplyThreadId === replyThreadId) {
        messageActionError = chatErrorMessage(cause, t("chatErrors.actions.workAction"));
      }
      return false;
    }
  }

  async function openScratchManager(): Promise<void> {
    if (scratchManagerLoading || scratchManagerOpen) return;
    scratchManagerLoading = true;
    try {
      await loadScratchManager();
      scratchManagerOpen = true;
    } catch (cause: unknown) {
      actionError = chatErrorMessage(cause, t("chatErrors.actions.scratchReclaim"));
    } finally {
      scratchManagerLoading = false;
    }
  }

  function followNewContentIfAtBottom(): void {
    const element = scroller;
    const key = destination;
    if (!element || restoredDestination !== key || !followingEnd) return;
    void tick().then(() => {
      if (element !== scroller || key !== destination || !followingEnd) return;
      element.scrollTop = element.scrollHeight;
      chat.setOrganizationalScrollPosition(key, element.scrollTop);
    });
  }

  /** Reserves the dock height below the last entry and keeps a thread that follows its end pinned when the dock resizes. */
  function reserveDockSpace(metrics: ChatComposerDockMetrics): void {
    if (metrics.height === dockHeight) return;
    dockHeight = metrics.height;
    followNewContentIfAtBottom();
  }

  /** Reads the exact turns of runs that are new or changed and merges them into the read cache kept by the Chat store. */
  async function loadExactExecutions(
    runs: readonly ChatAgentRunRead[],
    threadId: string,
  ): Promise<void> {
    const keys = runs.map(exactExecutionReadKey);
    for (const key of keys) exactExecutionReadsInFlight.add(key);
    const threadReads = new Map<string, Promise<ChatThreadShellRead>>();
    const results = await Promise.allSettled(runs.map(async (run) => {
      const executionThreadId = run.providerExecutionThreadId;
      if (!executionThreadId) throw new Error("This execution has not started yet");
      let threadRead = threadReads.get(executionThreadId);
      if (!threadRead) {
        threadRead = chatApi.readChatThreadShell(executionThreadId);
        threadReads.set(executionThreadId, threadRead);
      }
      const [timelinePage, thread] = await Promise.all([
        chatApi.readChatTimelineTurn(executionThreadId, run.providerExecutionTurnId),
        threadRead,
      ]);
      return { run, read: { timelinePage, thread } };
    }));
    for (const key of keys) exactExecutionReadsInFlight.delete(key);
    if (threadId !== exactExecutionThreadId) return;
    const failures: string[] = [];
    for (const result of results) {
      if (result.status === "fulfilled") {
        const { run, read } = result.value;
        const cached = exactExecutionCache.get(run.id);
        if (!cached || cached.updatedAt <= run.updatedAt) {
          exactExecutionCache.set(run.id, { updatedAt: run.updatedAt, read });
        }
      } else {
        failures.push(chatErrorMessage(result.reason, t("chatErrors.actions.workAction")));
      }
    }
    chat.exactExecutionReads.set(threadId, exactExecutionCache);
    if (page?.thread.id !== threadId) return;
    const plan = planExactExecutionReads(exactExecutionRuns(page?.agentRuns ?? []), exactExecutionCache, exactExecutionReadsInFlight);
    exactExecutionCache = plan.cache;
    exactExecutions = plan.visible;
    if (failures.length > 0) exactExecutionError = failures[0] ?? null;
  }

  $effect(() => {
    const replyThreadId = chat.openReplyThreadId;
    if (executionThreadScope === replyThreadId) return;
    executionThreadScope = replyThreadId;
    executionSelectionRequest += 1;
    actionError = null;
    messageActionError = null;
  });

  $effect(() => {
    const currentPage = page;
    const exactRuns = exactExecutionRuns(currentPage?.agentRuns ?? []);
    const threadId = currentPage?.thread.id ?? null;
    const scope = threadId === null
      ? ""
      : `${threadId}:${exactRuns.map(exactExecutionReadKey).join("|")}`;
    if (scope === exactExecutionScope) return;
    exactExecutionScope = scope;
    if (threadId !== exactExecutionThreadId) {
      exactExecutionThreadId = threadId;
      exactExecutionCache = threadId === null ? new Map() : chat.exactExecutionReads.get(threadId);
    }
    exactExecutionError = null;
    const plan = planExactExecutionReads(exactRuns, exactExecutionCache, exactExecutionReadsInFlight);
    exactExecutionCache = plan.cache;
    if (threadId !== null) chat.exactExecutionReads.set(threadId, plan.cache);
    exactExecutions = plan.visible;
    if (threadId !== null && plan.toRead.length > 0) {
      void loadExactExecutions(plan.toRead, threadId);
    }
  });

  $effect(() => {
    const agentId = conversationOnlyAgentId;
    const channelId = chat.selectedChannel?.id ?? null;
    const request = ++conversationOnlyAccessRequest;
    conversationOnlyMayUseFolders = false;
    if (agentId === null || channelId === null) return;
    void chatApi.readChatChannelAgentSettings(channelId)
      .then((settings) => {
        if (request === conversationOnlyAccessRequest) {
          conversationOnlyMayUseFolders = agentMayUseChannelFolders(settings, agentId);
        }
      })
      .catch((cause: unknown) => {
        // The line only explains conversation-only work, so a failed read leaves it out.
        console.warn("Could not read channel agent settings for the reply location", cause);
      });
  });

  $effect(() => {
    const agentRun = executionRun;
    if (!agentRun || chat.selectedExecutionRunId === agentRun.id) return;
    const request = ++executionSelectionRequest;
    actionError = null;
    void chat.selectAssignmentExecution(agentRun.id)
      .catch((cause: unknown) => {
        if (request === executionSelectionRequest) {
          actionError = chatErrorMessage(cause, t("chatErrors.actions.workAction"));
        }
      });
  });

  $effect(() => {
    const element = scroller;
    const key = destination;
    const presentationReady = executionPresentationReady;
    if (!element || !presentationReady) return;
    restoredDestination = null;
    void tick().then(() => {
      if (element !== scroller || key !== destination) return;
      element.scrollTop = chat.organizationalScrollPositions[key] ?? element.scrollHeight;
      followingEnd = organizationalScrollFollowsEnd(element);
      restoredDestination = key;
    });
  });

  $effect(() => {
    const element = scroller;
    const content = canvas;
    const key = destination;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (restoredDestination !== key || !followingEnd) return;
      element.scrollTop = element.scrollHeight;
      chat.setOrganizationalScrollPosition(key, element.scrollTop);
    });
    observer.observe(element);
    if (content) observer.observe(content);
    return () => observer.disconnect();
  });

  $effect(() => {
    page;
    renderEntries;
    chat.timelineItems;
    chat.timelinePages;
    followNewContentIfAtBottom();
  });

  $effect(() => {
    const element = scroller;
    const anchorId = chat.messageAnchorId;
    const presentationReady = executionPresentationReady;
    if (!element || !anchorId || !chat.openReplyThreadId || !presentationReady) return;
    void tick().then(() => {
      const target = element.querySelector<HTMLElement>(`[data-message-item-id="${CSS.escape(anchorId)}"]`);
      if (!target) return;
      target.scrollIntoView({ block: "center" });
      target.focus({ preventScroll: true });
      chat.clearMessageAnchor(anchorId);
    });
  });
</script>

<section
  class="reply-thread-panel"
  role={presentation === "dialog" ? "dialog" : presentation === "complementary" ? "complementary" : undefined}
  aria-modal={presentation === "dialog" ? "true" : undefined}
  aria-label={t("chat.organization.thread")}
  data-presentation={presentation}
  data-chat-reply-thread
  tabindex="-1"
  style:--chat-composer-dock-height={`${composerDocked ? dockHeight : 0}px`}
>
  {#if !mobileShell}
  <header class="thread-header" class:reserve-global-actions={reserveGlobalActions} data-reply-thread-header>
    <div class="thread-tab-shell" role="tablist">
      <button type="button" class="thread-tab" data-thread-tab role="tab" aria-selected="true" tabindex="0" onmousedown={preventMiddleButtonScroll} onauxclick={closeFromMiddleClick}>
        <MessagesSquare size={13} /><strong>{t("chat.organization.thread")}</strong>
      </button>
      <button type="button" class="tab-close" data-thread-close aria-label={t("chat.organization.closeThread")} onmousedown={preventMiddleButtonScroll} onauxclick={closeFromMiddleClick} onclick={onClose}><X size={11} /></button>
    </div>
    <span></span>
    {#if localExecutionAvailable && hasPrivateScratch}
      <button
        type="button"
        class="scratch-inspector"
        aria-label={scratchManagerLoading ? t("common.loading") : t("chat.organization.openScratchInspector")}
        aria-busy={scratchManagerLoading || undefined}
        title={t("chat.organization.openScratchInspector")}
        disabled={scratchManagerLoading}
        onclick={() => void openScratchManager()}
      ><HardDrive size={14} /></button>
    {/if}
  </header>
  {/if}

  {#if actionError || exactExecutionError}<Notice class="thread-error" message={actionError ?? exactExecutionError ?? ""} />{/if}
  {#if messageActionError}<Notice class="thread-error" message={messageActionError} onDismiss={() => { messageActionError = null; }} />{/if}

  <div
    bind:this={scroller}
    use:threadScrollFade class="thread-scroll"
    data-reply-thread-scroll
    aria-busy={!executionPresentationReady || chat.replyThreadLoading || undefined}
    onscroll={rememberScrollPosition}
  >
    {#if chat.replyThreadError}
      {#if page}<Notice class="thread-error" message={chat.replyThreadError} onRetry={retryReplyThread} />
      {:else}<div class="thread-failure"><Notice layout="fill" message={chat.replyThreadError} onRetry={retryReplyThread} /></div>{/if}
    {/if}
    {#if page && executionPresentationReady}
      <div bind:this={canvas} class="thread-canvas">
        {#if page.previousCursor}<button type="button" class="load-older" onclick={() => void chat.loadOlderReplyThreadMessages()}>{t("chat.organization.loadOlder")}</button>{/if}
        <div class="date-divider"><span>{formatDateTime(localization.locale, Date.parse(page.rootMessage.createdAt), { dateStyle: "full" })}</span></div>
        <ChatOrganizationalMessage message={page.rootMessage} showReplyStrip={false} currentResponseSettings onActionError={reportMessageActionError} />
        {#if assignment?.delegation}<ChatDelegationOrigin delegation={assignment.delegation} onOpenThread={openThread} />{/if}
        {#if runLocationLabel}<p class="run-location" data-run-location>{runLocationLabel}</p>{/if}
        {#each renderEntries as entry, index (entry.key)}
          {@const previousEntry = renderEntries[index - 1]}
          {@const previousCreatedAt = previousEntry ? entryCreatedAt(previousEntry) : page.rootMessage.createdAt}
          {#if !isSameDay(previousCreatedAt, entryCreatedAt(entry))}
            <div class="date-divider"><span>{formatDateTime(localization.locale, Date.parse(entryCreatedAt(entry)), { dateStyle: "full" })}</span></div>
          {/if}
          {#if entry.kind === "message"}
            <ChatOrganizationalMessage
              message={entry.message}
              showReplyStrip={false}
              currentResponseSettings
              grouped={previousEntry?.kind === "message" && shouldGroupReplyMessages(previousEntry.message, entry.message)}
              onActionError={reportMessageActionError}
              answerChoices={answerableQuestion?.messageItemId === entry.message.itemId ? answerableQuestion.choices : []}
              onChooseAnswer={answerableQuestion?.messageItemId === entry.message.itemId ? chooseAnswer : undefined}
              onRetryFailure={retryableFailureKey === entry.key ? retryFailedWork : undefined}
            />
          {:else if entry.kind === "live"}
            <!-- A phone reads each run from the linked desktop that executes it and keeps it once settled. -->
            <ChatLiveExecution
              assignmentId={entry.assignmentId}
              work={workWithoutRun?.id === entry.assignmentId ? workWithoutRun : null}
              agent={executionAgent}
              agentName={executionAgentName ?? t("chat.runs.outcome.agentFallback")}
              onActionError={reportMessageActionError}
              onShown={(runId) => noteRemoteRunShown(entry.assignmentId, runId)}
            />
          {:else}
            {@const exactExecution = exactExecutions[entry.run.id]}
            {@const restartLabel = runRestartLabel(entry.run)}
            {#if restartLabel}<p class="run-restart" data-run-restart>{restartLabel}</p>{/if}
            {#if entry.loaded}
              <ChatExecutionTimeline
                embedded
                hideUserMessages
                agent={executionAgent}
                turnId={entry.run.providerExecutionTurnId}
                timelinePage={exactExecution?.timelinePage ?? null}
                executionThread={exactExecution?.thread ?? null}
              />
            {/if}
            <ChatAgentRunStatus
              run={entry.run}
              agentName={executionAgentName}
              timelinePage={exactExecution?.timelinePage ?? null}
              replies={page.replies}
              fallbackDetail={entry.run.id === page.agentRuns.at(-1)?.id ? assignment?.stateReason ?? null : null}
              failureShownElsewhere={failureShownByReply(renderEntries, entry.run)}
              onRetryFailure={retryableFailureKey === entry.key ? retryFailedWork : undefined}
              onActionError={reportMessageActionError}
            />
          {/if}
        {/each}
        {#if workWithoutRun && localExecutionAvailable}
          <!-- Work another linked device runs shows its row here; a phone shows it with its live run above. -->
          <ChatRemoteWorkRow
            work={workWithoutRun}
            agent={executionAgent}
            agentName={executionAgentName ?? t("chat.runs.outcome.agentFallback")}
            onActionError={reportMessageActionError}
          />
        {/if}
        {#if page.delegatedChildren.length > 0}<ChatDelegatedWorkList delegatedChildren={page.delegatedChildren} onOpenThread={openThread} />{/if}
      </div>
    {:else if chat.replyThreadLoading || page}
      <Loading layout="fill" />
    {/if}
  </div>

  {#if composerDocked}
    <div class="thread-dock" use:chatComposerDock={{ onMeasure: reserveDockSpace, scroller }}>
      {#each chat.interaction?.pendingRequests ?? [] as pending (pending.id)}<div class="thread-request"><ChatRequestPanel {pending} /></div>{/each}
      {#if remoteApproval}
        <div class="thread-request">
          {#key remoteApproval.requestId}
            <ChatRemoteApprovalPanel
              approval={remoteApproval}
              agentName={executionAgentName ?? t("chat.runs.outcome.agentFallback")}
              onAnswered={(requestId) => { answeredRemoteApprovals = [...answeredRemoteApprovals, requestId]; }}
            />
          {/key}
        </div>
      {/if}
      {#if localExecutionAvailable && chat.openReplyThreadId}
        <div class="thread-secrets"><ChatSecretRequests replyThreadId={chat.openReplyThreadId} refreshKey={page} agentName={executionAgentName} /></div>
      {/if}
      <div class="thread-composer">
        {#key `${destination}:${composerRevision}`}
          <ChatMessageComposer {destination} threadComposer placeholder={t("chat.organization.messageChannel")} onRequestScrollToBottom={scrollToBottomForUserAction} />
        {/key}
      </div>
    </div>
  {/if}
</section>

{#if scratchManagerOpen && chat.openReplyThreadId && ChatScratchManager}
  {@const Manager = ChatScratchManager}
  <Manager initialReplyThreadId={chat.openReplyThreadId} onClose={() => { scratchManagerOpen = false; }} />
{/if}

<style>
  .reply-thread-panel { position:relative; display:flex; width:100%; height:100%; min-height:0; flex-direction:column; background:var(--cal-bg); outline:none; }
  .thread-header { display:flex; height:var(--toolbar-h); min-height:var(--toolbar-h); flex:0 0 auto; align-items:center; gap:0.2rem; background:var(--cal-bg); padding-inline:0.45rem; }
  .thread-header.reserve-global-actions { padding-right:var(--chat-global-actions-width); }
  .thread-tab-shell { display:flex; width:9.5rem; min-width:3.75rem; height:2rem; flex:0 1 9.5rem; align-items:stretch; overflow:hidden; border-radius:0.55rem; background:var(--accent); color:var(--foreground); user-select:none; }
  .thread-tab { display:flex; min-width:0; flex:1 1 auto; align-items:center; gap:0.4rem; overflow:hidden; padding:0.3rem 0.2rem 0.3rem 0.65rem; text-align:left; user-select:none; }
  :global(html[data-focus-intent="keyboard"]) .thread-tab:focus { outline:none; }
  :global(html[data-focus-intent="keyboard"]) .thread-tab-shell:has(.thread-tab:focus) { box-shadow:inset 0 0 0 2px var(--ring); }
  .thread-tab :global(svg) { flex:0 0 auto; }.thread-tab strong { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size: calc(0.733333rem * var(--type-scale)); font-weight:500; user-select:none; }
  .tab-close { display:grid; width:1.5rem; flex:0 0 auto; place-items:center; border-radius:0.3rem; opacity:0; }
  .thread-tab-shell:hover .tab-close, .tab-close:focus-visible { opacity:1; }
  .tab-close:hover { background:var(--accent); }
  .thread-header > span { flex:1; }
  .scratch-inspector { display:grid; min-width:2rem; min-height:2rem; place-items:center; border-radius:0.42rem; color:var(--muted-foreground); }
  .scratch-inspector:hover { background:var(--accent); color:var(--foreground); }
  .thread-scroll { display:flex; min-height:0; flex:1; flex-direction:column; overflow-y:scroll; overscroll-behavior:contain; padding-block:0.4rem calc(var(--chat-composer-dock-height,0px) + 0.4rem); }
  .date-divider { display:flex; align-items:center; gap:0.5rem; margin:0.75rem 0; color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  .date-divider::before,.date-divider::after { height:1px; flex:1; background:var(--border); content:""; }
  .run-restart,.run-location { margin:0.25rem 0; color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  /* The dock floats over the thread end: entries show around the surfaces above the composer and inside its rounded top corners, and the backdrop hides them below the corner curve. Only the surfaces take pointer input. */
  .thread-dock { position:absolute; inset:auto 0 0; z-index:1; display:flex; flex-direction:column; background:linear-gradient(to bottom,transparent var(--chat-composer-dock-backdrop-top,0px),var(--cal-bg) var(--chat-composer-dock-backdrop-top,0px)); pointer-events:none; }
  .thread-request { flex:0 0 auto; border-top:1px solid var(--border); background:var(--cal-bg); padding-block:0.5rem; pointer-events:auto; }
  .thread-secrets { flex:0 0 auto; background:var(--cal-bg); pointer-events:auto; }
  .thread-secrets:empty { display:none; }
  .thread-composer { display:flex; flex:0 0 auto; justify-content:center; padding:0.5rem var(--chat-conversation-gutter,1rem) 0.75rem; }
  .reply-thread-panel :global(.thread-error) { padding:0.6rem; font-size: calc(0.7rem * var(--type-scale)); }
  .thread-failure { height:100%; font-size: calc(0.7rem * var(--type-scale)); }
  .load-older { display:block; margin:0.3rem auto 0.6rem; border-radius:0.4rem; padding:0.3rem 0.5rem; color:var(--muted-foreground); font-size: calc(0.68rem * var(--type-scale)); }.load-older:hover { background:var(--accent); }
  /* Every presentation lays entries and the composer on the channel feed's gutters and width, so their edges line up the same way. */
  .thread-canvas { --chat-message-row-padding-inline:0rem; flex:0 0 auto; width:min(calc(100% - var(--chat-conversation-gutter,1rem) - var(--chat-conversation-gutter,1rem)),var(--chat-conversation-max-width,60rem)); margin-inline:auto; }
  .thread-request,.thread-secrets { padding-inline:max(var(--chat-conversation-gutter,1rem),calc((100% - var(--chat-conversation-max-width,60rem)) / 2)); }
  @container chat-shell (max-width:440px) { .reply-thread-panel { --chat-conversation-gutter:0.5rem; } }
  @media (pointer:coarse) { .scratch-inspector { min-width:2.75rem; min-height:2.75rem; } }
  @media (hover:none) { .tab-close { opacity:1; } }
</style>
