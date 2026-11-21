<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Clock3 from "@lucide/svelte/icons/clock-3";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import Hash from "@lucide/svelte/icons/hash";
  import Image from "@lucide/svelte/icons/image";
  import MessageSquareShare from "@lucide/svelte/icons/message-square-share";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import Smile from "@lucide/svelte/icons/smile";
  import AtSign from "@lucide/svelte/icons/at-sign";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import { agentNeedsSetupHere } from "$lib/chat/agents/agent-setup";
  import { chatAgentDefaultModelId } from "$lib/chat/agents/channel-access";
  import { isActiveChannelMember } from "$lib/chat/agents/channel-members";
  import { chatParticipantDisplayName } from "$lib/chat/agents/participant-display";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import type {
    ChatAttachmentRead,
    ChatAssignmentInputRead,
    ChatAssignmentReviewRequest,
    ChatAssignmentTargetRead,
    ChatChannelRead,
    ChatChannelRosterRead,
    ChatFollowupRouting,
    ChatInteractionStateRead,
    ChatMessageRead,
    ChatMessageReference,
    ChatParticipantId,
    ChatParticipantRead,
    ChatPromptCatalogEntry,
    ChatRecurrenceSpec,
    ChatScheduledMessageRead,
    ChatWorkAssignmentId,
    ProviderModel,
  } from "$lib/chat/contracts";
  import { ChatComposerEditor, type ChatComposerEditorChange } from "$lib/chat/composer/editor";
  import { STOPPABLE_ASSIGNMENT_STATES } from "$lib/chat/timeline/reply-thread-model";
  import {
    channelModelChoices,
    genericModelCommandEntry,
    isModelCommandEntry,
    mergePromptCatalog,
    modelCommandEntries,
  } from "$lib/chat/composer/command-model";
  import { composerCollapsed } from "$lib/chat/composer/collapse";
  import { composerMeterRead, type ComposerStatusRead } from "$lib/chat/composer/composer-meter";
  import { CHAT_IMAGE_CONTENT_TYPES, CHAT_IMAGE_LIMIT, composerTokenTrigger, validateImageFiles } from "$lib/chat/composer/model";
  import { pendingInputRows, pendingInputStartsWork, type PendingInputRow } from "$lib/chat/composer/pending-inputs";
  import { formatShortcut, hasOnlyShortcutModifier } from "$lib/keyboard-shortcuts";
  import { latestStructuredPlanSteps, summarizePlanProgress } from "$lib/chat/composer/plan-progress";
  import { suggestAgentMention, type MentionSuggestion } from "$lib/chat/composer/mention-suggestion";
  import { pastedTextLineCount } from "$lib/chat/composer/paste";
  import { providerTargetAuthorityBlockers } from "$lib/chat/composer/provider-authority";
  import {
    chatComposerDocumentFromText,
    chatComposerDocumentVersioned,
    chatComposerMarkdown,
    chatComposerPlainText,
    replaceChatComposerText,
    parseChatComposerDocument,
    type ChatComposerDocument,
    type ChatComposerSelection,
  } from "$lib/chat/composer/rich-text";
  import {
    chatReferenceTriggerAtCaret,
    chatReferencesForEditor,
    chatReferencesForMarkdown,
    normalizeChatMessageReferences,
    type ChatReferenceClipboardSlice,
    type ChatReferenceTrigger,
  } from "$lib/chat/composer/message-references";
  import {
    candidateIsDisabled,
    candidateReference,
    firstPickerIndex,
    groupReferenceCandidates,
    inferAssignmentTarget,
    lastPickerIndex,
    matchesQuery,
    nextPickerIndex,
    type ReferenceCandidate,
  } from "$lib/chat/composer/reference-candidates";
  import { chatErrorCode, chatErrorMessage, chatErrorReason } from "$lib/chat/error-presentation";
  import { parseChatMessageReference } from "$lib/chat/validation";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { providerLimitNotice } from "$lib/chat/usage/provider-limit";
  import { parseProjectIcon } from "$lib/projects/icons/values";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChatPaneEvents } from "$lib/chat/pane-events";
  import { useChat, type ChatOrganizationalDraft } from "$lib/stores/chat.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { requireActiveVaultIdentity } from "$lib/vault/active-vault";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ChatAssignmentReviewCard from "./ChatAssignmentReviewCard.svelte";
  import ChatComposerFrame from "./ChatComposerFrame.svelte";
  import ChatComposerInfoPanel from "./ChatComposerInfoPanel.svelte";
  import ChatComposerSuggestionMenu from "./ChatComposerSuggestionMenu.svelte";
  import ChatQueuedReplies from "./ChatQueuedReplies.svelte";
  import ChatTaskProgressCard from "./ChatTaskProgressCard.svelte";
  import { isAppFloatingSurfaceTarget } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { chatReferencePickerPosition } from "./reference-picker-position";

  type ScheduleMenuComponent = typeof import("./ChatMessageScheduleMenu.svelte").default;
  type EmojiPickerComponent = typeof import("$lib/components/icon-picker/IconPicker.svelte").default;

  interface DisclosureBlocker {
    message: string;
    sourceChannelId: string;
    sourceReferenceId: string | null;
  }

  const REFERENCE_CLIPBOARD_TYPE = "application/x-ganbaru-chat-references+json";
  const MAX_MESSAGE_BYTES = 131_072;

  let {
    destination,
    placeholder,
    threadComposer = false,
    onRequestScrollToBottom = () => undefined,
  }: {
    destination: string;
    placeholder: string;
    threadComposer?: boolean;
    onRequestScrollToBottom?: () => void;
  } = $props();

  const chat = useChat();
  const preferences = getPreferences();
  const paneEvents = useChatPaneEvents();
  const projects = useProjects();
  const localization = getLocalization();
  const { t } = localization;
  const localExecutionAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "chat.local-execution",
  );
  /**
   * Without local execution, attachments go to the channel project's primary working folder,
   * the folder this device may import into and an organizational run reads from.
   */
  let primaryAttachmentFolderId = $state<string | null>(null);
  let primaryAttachmentFolderRequest = 0;
  /** The system image picker used when there is no native file dialog. */
  let imageInput = $state<HTMLInputElement | null>(null);
  const focusEditorOnMount = BUILD_PLATFORM_PROFILE.shell === "desktop";
  /** The phone composer rests as one row until it is used, which keeps the conversation in view. */
  const collapsible = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const initialDraft = untrack(() => chat.organizationalDraft(destination));
  const initialDocument = parseChatComposerDocument(
    initialDraft.richContent,
    initialDraft.normalizedMarkdown,
  );

  let editorRoot = $state<HTMLDivElement | undefined>();
  let editorController: ChatComposerEditor | null = null;
  let editorDocument = $state<ChatComposerDocument>(initialDocument);
  let normalizedMarkdown = $state(chatComposerMarkdown(initialDocument));
  let editorReferences = $state<ChatMessageReference[]>(chatReferencesForEditor(
    initialDocument,
    initialDraft.references,
  ));
  let attachmentIds = $state([...initialDraft.attachmentIds]);
  let selectionStart = $state(initialDraft.selectionStart);
  let selectionEnd = $state(initialDraft.selectionEnd);
  let composing = $state(false);
  let pickerOpen = $state(false);
  let pickerTrigger = $state<ChatReferenceTrigger | null>(null);
  let pickerEntries = $state<ReferenceCandidate[]>([]);
  let pickerIndex = $state(0);
  let pickerLoading = $state(false);
  let pickerRequest = 0;
  let pickerRoot = $state<HTMLDivElement | null>(null);
  let pickerScroll = $state<HTMLDivElement | null>(null);
  let composerRoot = $state<HTMLDivElement | null>(null);
  let focusWithin = $state(false);
  let emojiPickerOpen = $state(false);
  let scheduleMenuOpen = $state(false);
  let scheduleMenuAnchor = $state<HTMLDivElement | null>(null);
  let scheduledMessagesOpen = $state(false);
  let scheduledMessagesAnchor = $state<HTMLDivElement | null>(null);
  let ScheduleMenu = $state<ScheduleMenuComponent | null>(null);
  let scheduleMenuLoad = $state<Promise<void> | null>(null);
  let EmojiPicker = $state<EmojiPickerComponent | null>(null);
  let scheduledFor = $state<string | null>(initialDraft.scheduledFor);
  let scheduledRecurrence = $state<ChatRecurrenceSpec | null>(initialDraft.recurrence);
  let scheduledMessages = $state<ChatScheduledMessageRead[]>(untrack(() => chat.knownScheduledMessages(destination) ?? []));
  let scheduledMessagesRequest = 0;
  let alsoSendToChannel = $state(false);
  let sending = $state(false);
  let stopping = $state(false);
  let reviewNote = $state("");
  let reviewNoteOpen = $state(false);
  let reviewing = $state(false);
  /** The reviewed work the open review card belongs to, so a note typed for it never reaches other work. */
  let reviewNoteAssignmentId: ChatWorkAssignmentId | null = null;
  let error = $state<string | null>(null);
  let disclosureBlocker = $state<DisclosureBlocker | null>(null);
  let blockerElement = $state<HTMLDivElement | null>(null);
  let announcement = $state("");
  /** Attachment reads the chips label from, by attachment id. */
  let attachmentReads = $state<Record<string, ChatAttachmentRead>>({});
  const draftPlainText = $derived(chatComposerPlainText(editorDocument));
  /** Agents whose mention suggestion was dismissed for this draft. */
  let dismissedMentionSuggestions = $state<ChatParticipantId[]>([]);
  let assignmentTargets = $state<ChatAssignmentTargetRead[]>([]);
  let assignmentPreviewLoading = $state(false);
  let assignmentPreviewRequest = 0;
  /** The assignment preview read in flight, which a send waits for before checking its blockers. */
  let assignmentPreviewPending: Promise<void> | null = null;
  let navigationChannels = $state<ChatChannelRead[]>([]);
  let navigationChannelsLoaded = false;
  let promptMenuKind = $state<"skill" | "command" | null>(null);
  let promptMenuEntries = $state<ChatPromptCatalogEntry[]>([]);
  let promptMenuIndex = $state(0);
  let promptMenuLoading = $state(false);
  let promptMenuRequest = 0;
  let promptMenuRoot = $state<HTMLDivElement | undefined>();
  let promptMenuIncludeIgnored = $state(false);
  let promptCatalogCache: { key: string; entries: ChatPromptCatalogEntry[] } | null = null;
  let statusPanelOpen = $state(false);
  let interactionState = $state<ChatInteractionStateRead | null>(null);
  let interactionRequest = 0;
  let routingNotice = $state<string | null>(null);
  let assignmentInputs = $state<ChatAssignmentInputRead[]>([]);
  let assignmentInputsRequest = 0;
  /** The waiting reply the editor holds; the destination draft stays in the store until the edit ends. */
  let editingInput = $state<ChatAssignmentInputRead | null>(null);
  let inputBusyId = $state<string | null>(null);
  /**
   * The scheduled message the editor holds, with the share choice of the draft it set aside; the
   * destination draft, its time, and its repeat rule stay in the store until the edit ends.
   */
  let editingScheduled = $state<{ message: ChatScheduledMessageRead; draftAlsoSendToChannel: boolean } | null>(null);
  /** Why every tool beyond text is off: an edit replaces only the text of a waiting reply. */
  const editLockReason = $derived(editingInput ? t("chat.composer.pendingInputs.textOnly") : null);

  const participantReferences = $derived(editorReferences.filter((reference) => (
    reference.kind === "participant"
  )));
  const aiParticipantReferences = $derived(participantReferences.filter((reference) => (
    reference.kind === "participant" && reference.participantKind === "ai_agent"
  )));
  const invokedAiCount = $derived(new Set(aiParticipantReferences.map((reference) => (
    reference.kind === "participant" ? reference.participantId : ""
  ))).size);
  const mentionedAgentId = $derived(aiParticipantReferences.find((reference) => (
    reference.kind === "participant"
  ))?.participantId ?? null);
  const validation = $derived(invokedAiCount > 1 ? t("chat.organization.oneAgentOnly") : null);
  const hasMessageContent = $derived(Boolean(
    normalizedMarkdown.trim() || attachmentIds.length || editorReferences.length,
  ));
  const messageTooLarge = $derived(new TextEncoder().encode(normalizedMarkdown).byteLength > MAX_MESSAGE_BYTES);
  const channelName = $derived(chat.selectedChannel?.name ?? "");
  const activeAssignment = $derived.by(() => {
    const assignment = threadComposer ? chat.replyThread?.assignment : null;
    return assignment && STOPPABLE_ASSIGNMENT_STATES.has(assignment.state) ? assignment : null;
  });
  const assignmentAwaitingReview = $derived.by(() => {
    const assignment = threadComposer && localExecutionAvailable ? chat.replyThread?.assignment : null;
    return assignment?.state === "ready_for_review" ? assignment : null;
  });
  const inferredTarget = $derived.by(() => inferAssignmentTarget(
    editorReferences,
    assignmentTargets,
  ));
  const providerBlockers = $derived.by(() => assignmentProviderBlockers(
    mentionedAgentId,
    inferredTarget,
    editorReferences,
    chat.selectedChannel,
    assignmentTargets,
  ));
  const pickerGroups = $derived.by(() => groupReferenceCandidates(pickerEntries));
  const nextScheduledMessage = $derived(
    scheduledMessages.find((message) => message.state !== "failed") ?? scheduledMessages[0] ?? null,
  );
  // A repeating message next posts at its pending occurrence, later when that occurrence waits for a quiet period.
  const nextScheduledDelivery = $derived(nextScheduledMessage
    ? nextScheduledMessage.recurrence?.deferredUntil ?? nextScheduledMessage.recurrence?.nextOccurrenceAt ?? nextScheduledMessage.scheduledFor
    : null);
  const threadAssignment = $derived(threadComposer ? chat.replyThread?.assignment ?? null : null);
  /** The agent a post starts or continues work for: the mentioned one, else the thread's. */
  const targetAgentId = $derived(mentionedAgentId ?? threadAssignment?.agent.id ?? null);
  const targetAgent = $derived(chat.agents.find((candidate) => (
    candidate.participant.id === targetAgentId
  )) ?? null);
  const targetProvider = $derived(chat.settings?.providerInstances.find((entry) => (
    entry.configuration.instanceId === targetAgent?.localProviderInstanceId
  )) ?? null);
  const catalogWorkingFolderId = $derived.by(() => {
    const available = (id: string) => chat.workingFolders.some((folder) => (
      folder.workingFolder.id === id
        && folder.workingFolder.archivedAt === null
        && folder.bindingStatus === "available"
    ));
    const targetFolderId = inferredTarget?.executionTarget?.kind === "workingFolder"
      ? inferredTarget.executionTarget.workingFolderId
      : null;
    if (targetFolderId && available(targetFolderId)) return targetFolderId;
    return chat.workingFolders.find((folder) => (
      folder.workingFolder.projectId === chat.selectedChannel?.projectId
        && available(folder.workingFolder.id)
    ))?.workingFolder.id ?? null;
  });
  const promptCatalogKey = $derived([
    catalogWorkingFolderId ?? "",
    targetAgent?.localProviderInstanceId ?? "",
    threadAssignment?.latestRunProviderThreadId ?? "",
  ].join("\u0000"));
  /** Whether the thread has work whose provider session /status can describe. */
  const statusAvailable = $derived(threadAssignment !== null);
  const meterRead = $derived(composerMeterRead(
    interactionState?.usage ?? null,
    interactionState?.rateLimitStatus ?? null,
  ));
  /** Whether a post becomes a reply to work an agent is on, which Enter queues and Mod+Enter steers. */
  const routesFollowups = $derived(activeAssignment !== null && !scheduledFor && !editingInput);
  const sendTooltip = $derived(routesFollowups
    ? t("chat.composer.queueOrSteerTooltip", "Enter", formatShortcut("Mod + Enter"))
    : editingInput ? t("chat.composer.pendingInputs.save")
    : editingScheduled ? t("chat.organization.saveScheduledMessage")
    : scheduledFor ? t("chat.organization.scheduleMessage")
    : t("chat.composer.sendTooltip", "Enter"));
  /** The provider limit that stopped or holds the work of this thread, with its reset time when known. */
  const providerLimit = $derived(threadComposer
    ? providerLimitNotice(threadAssignment, chat.replyThread?.replies ?? [], interactionState?.rateLimitStatus ?? null)
    : null);
  const pendingRows = $derived(pendingInputRows(assignmentInputs));
  /** The latest structured plan of the running work, while some of its steps are still to do. */
  const taskProgress = $derived.by(() => {
    if (!activeAssignment) return null;
    const summary = summarizePlanProgress(latestStructuredPlanSteps(chat.replyThread?.replies ?? [], activeAssignment.id) ?? []);
    return summary && summary.completed < summary.total ? summary : null;
  });
  const sendBlocked = $derived(Boolean(validation || disclosureBlocker || providerBlockers.length || messageTooLarge));
  /** Channel agents that can take work, for the mention suggestion. */
  const mentionRoster = $derived((chat.selectedChannel?.memberships ?? []).flatMap((membership) => (
    isActiveChannelMember(membership)
      && membership.participant.archivedAt === null
      && membership.participant.kind === "ai_agent"
      ? [{
        id: membership.participant.id,
        displayName: membership.participant.displayName,
      }]
      : []
  )));
  /** An agent the draft names without mentioning, offered as a local chip; never while editing a waiting reply. */
  const mentionSuggestion = $derived.by((): MentionSuggestion | null => {
    if (editingInput || pickerOpen) return null;
    const roster = mentionRoster.filter((candidate) => !dismissedMentionSuggestions.includes(candidate.id));
    return suggestAgentMention(draftPlainText, editorReferences, roster);
  });

  onMount(() => {
    if (!editorRoot) return;
    editorController = new ChatComposerEditor(
      editorRoot,
      editorDocument,
      {
        onChange: handleEditorChange,
        onSelectionChange: handleEditorSelectionChange,
      },
      editorReferences,
    );
    void loadAttachmentReads(attachmentIds);
    const stop = () => { void stopAssignment(); };
    paneEvents.addEventListener("ganbaru:chat-stop-requested", stop);
    if (focusEditorOnMount) {
      editorController.focus({ start: selectionStart, end: selectionEnd });
    }
    return () => {
      paneEvents.removeEventListener("ganbaru:chat-stop-requested", stop);
      // The destination draft stayed in the store while a waiting reply was edited, so the next mount restores it.
      editingInput = null;
      editingScheduled = null;
      releaseReviewNote();
      editorController = null;
    };
  });

  $effect(() => {
    const threadId = threadAssignment?.latestRunProviderThreadId ?? null;
    const _revision = threadAssignment?.revision;
    const _state = threadAssignment?.state;
    const request = ++interactionRequest;
    if (!threadId) {
      interactionState = null;
      return;
    }
    chatApi.readChatInteractionState(threadId)
      .then((read) => { if (request === interactionRequest) interactionState = read; })
      .catch(() => {
        // The meter keeps its last reading; usage is advisory and a failed read must not block replies.
      });
  });

  $effect(() => {
    const assignmentId = threadAssignment?.id ?? null;
    const _revision = threadAssignment?.revision;
    const _state = threadAssignment?.state;
    const _threadRevision = chat.replyThread?.revision;
    const _replies = chat.replyThread?.replies.length;
    void loadAssignmentInputs(assignmentId);
  });

  $effect(() => {
    if (localExecutionAvailable) return;
    const projectId = chat.selectedChannel?.projectId ?? null;
    const request = ++primaryAttachmentFolderRequest;
    primaryAttachmentFolderId = null;
    if (!projectId) return;
    chatApi.readChatProjectPrimaryWorkingFolder(projectId)
      .then((read) => {
        if (request !== primaryAttachmentFolderRequest) return;
        primaryAttachmentFolderId = read.workingFolderId;
        void loadAttachmentReads(attachmentIds);
      })
      .catch((cause: unknown) => {
        if (request === primaryAttachmentFolderRequest) error = chatErrorMessage(cause, t("chatErrors.actions.fileAction"));
      });
  });

  $effect(() => {
    const key = promptCatalogKey;
    untrack(() => {
      if (promptCatalogCache && promptCatalogCache.key !== key) promptCatalogCache = null;
    });
  });

  $effect(() => {
    if (!promptMenuKind || !promptMenuRoot) return;
    const selectedIndex = promptMenuIndex;
    void tick().then(() => {
      const menu = promptMenuRoot;
      const selected = menu?.querySelector<HTMLElement>(`[data-menu-index="${selectedIndex}"]`);
      if (!menu || !selected) return;
      const top = selected.offsetTop;
      const bottom = top + selected.offsetHeight;
      if (top < menu.scrollTop) menu.scrollTop = top;
      else if (bottom > menu.scrollTop + menu.clientHeight) menu.scrollTop = Math.max(0, bottom - menu.clientHeight);
    });
  });

  $effect(() => {
    const reviewedId = assignmentAwaitingReview?.id ?? null;
    if (reviewing) return;
    untrack(() => {
      if (reviewedId === reviewNoteAssignmentId) return;
      // Without earlier reviewed work no card held a note, and one opened before this first run belongs to it.
      if (reviewNoteAssignmentId !== null) releaseReviewNote();
      reviewNoteAssignmentId = reviewedId;
    });
  });

  $effect(() => {
    const _version = chat.scheduledMessagesVersion;
    const currentDestination = destination;
    void loadScheduledMessages(currentDestination);
  });

  $effect(() => {
    const agentId = mentionedAgentId;
    const channelId = chat.selectedChannel?.id ?? null;
    const replyThreadId = destinationReplyThreadId(destination);
    // Targets are device-local folders, so only a device that runs the work lists them.
    if (!agentId || !channelId || !localExecutionAvailable) {
      assignmentPreviewRequest += 1;
      assignmentTargets = [];
      assignmentPreviewLoading = false;
      assignmentPreviewPending = null;
      return;
    }
    assignmentTargets = [];
    assignmentPreviewPending = loadAssignmentPreview(agentId, channelId, replyThreadId);
  });

  $effect(() => {
    if (!pickerOpen || !pickerRoot) return;
    const selectedIndex = pickerIndex;
    void tick().then(() => {
      const selected = pickerRoot?.querySelector<HTMLElement>(`[data-reference-index="${selectedIndex}"]`);
      selected?.scrollIntoView({ block: "nearest" });
    });
  });

  $effect(() => {
    if (!scheduleMenuOpen && !scheduledMessagesOpen && !pickerOpen && !promptMenuKind) return;
    const closeMenusFromOutside = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node) || isAppFloatingSurfaceTarget(target)) return;
      if (scheduleMenuOpen && !scheduleMenuAnchor?.contains(target)) scheduleMenuOpen = false;
      if (scheduledMessagesOpen && !scheduledMessagesAnchor?.contains(target)) scheduledMessagesOpen = false;
      if (pickerOpen && !editorRoot?.contains(target) && !pickerRoot?.contains(target)) closePicker();
      if (promptMenuKind && !editorRoot?.contains(target) && !promptMenuRoot?.contains(target)) closePromptMenu();
    };
    window.addEventListener("pointerdown", closeMenusFromOutside, true);
    return () => window.removeEventListener("pointerdown", closeMenusFromOutside, true);
  });

  /** Ends focus within the composer only when focus moves outside it, so moving between its controls keeps it open. */
  function trackFocusLeaving(event: FocusEvent): void {
    const next = event.relatedTarget;
    if (next instanceof Node && composerRoot?.contains(next)) return;
    focusWithin = false;
  }

  function persist(): void {
    if (editingInput || editingScheduled) return;
    const references = chatReferencesForMarkdown(editorDocument, editorReferences);
    const draft: ChatOrganizationalDraft = {
      normalizedMarkdown,
      richContent: chatComposerDocumentVersioned(editorDocument),
      attachmentIds: [...attachmentIds],
      references,
      selectionStart,
      selectionEnd,
      scheduledFor,
      recurrence: scheduledRecurrence,
    };
    chat.setOrganizationalDraft(destination, draft);
  }

  function handleEditorChange(change: ChatComposerEditorChange): void {
    editorDocument = change.document;
    normalizedMarkdown = change.markdown;
    editorReferences = change.references;
    disclosureBlocker = null;
    routingNotice = null;
    persist();
    const end = change.plainText.length;
    updateTokenMenus(change.plainText, editorController?.selection() ?? { start: end, end }, change.references);
  }

  function handleEditorSelectionChange(selection: ChatComposerSelection): void {
    selectionStart = selection.start;
    selectionEnd = selection.end;
    persist();
    updateTokenMenus(editorController?.plainText() ?? "", selection, editorController?.messageReferences() ?? editorReferences);
  }

  /**
   * Opens the one token menu the caret calls for: the reference picker for `@` and `#`, which owns
   * those triggers, otherwise the skill or command menu for `$` and a line-start `/`.
   */
  function updateTokenMenus(
    text: string,
    selection: ChatComposerSelection,
    references: readonly ChatMessageReference[],
  ): void {
    if (editingInput) {
      closePicker();
      closePromptMenu();
      return;
    }
    if (chatReferenceTriggerAtCaret(text, selection, references)) {
      closePromptMenu();
      void updateReferencePicker(text, selection, references);
      return;
    }
    closePicker();
    const trigger = selection.start === selection.end ? composerTokenTrigger(text, selection.start) : null;
    if (trigger?.kind === "skill" || trigger?.kind === "command") void updatePromptMenu(trigger.kind, trigger.query);
    else closePromptMenu();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (composing || event.isComposing || event.keyCode === 229) return;
    if (promptMenuKind && promptMenuEntries.length > 0) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const direction = event.key === "ArrowDown" ? 1 : -1;
        promptMenuIndex = (promptMenuIndex + direction + promptMenuEntries.length) % promptMenuEntries.length;
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        void choosePromptEntry(promptMenuIndex);
        return;
      }
    }
    if (promptMenuKind && event.key === "Escape") {
      event.preventDefault();
      closePromptMenu();
      return;
    }
    if (pickerOpen) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const direction = event.key === "ArrowDown" ? 1 : -1;
        pickerIndex = nextPickerIndex(pickerEntries, pickerIndex, direction);
        return;
      }
      if (event.key === "Home") {
        event.preventDefault();
        pickerIndex = firstPickerIndex(pickerEntries);
        return;
      }
      if (event.key === "End") {
        event.preventDefault();
        pickerIndex = lastPickerIndex(pickerEntries);
        return;
      }
      if ((event.key === "Enter" || event.key === "Tab") && pickerEntries[pickerIndex]) {
        event.preventDefault();
        void chooseReferenceCandidate(pickerEntries[pickerIndex]!);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        closePicker();
        return;
      }
    }
    if (editorController?.handleKeydown(event)) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void post(hasOnlyShortcutModifier(event) ? "steer" : "queue");
    }
  }

  function handleCopy(event: ClipboardEvent): void {
    if (!event.clipboardData || !editorController) return;
    const selection = editorController.selection();
    if (selection.start === selection.end) return;
    const plainText = editorController.plainText().slice(selection.start, selection.end);
    event.preventDefault();
    event.clipboardData.setData("text/plain", plainText);
    try {
      const slice = editorController.copyReferenceSlice(requireActiveVaultIdentity());
      if (slice) event.clipboardData.setData(REFERENCE_CLIPBOARD_TYPE, JSON.stringify(slice));
    } catch {
      // A plain-text copy remains valid before vault bootstrap completes.
    }
  }

  /** Pastes a copied slice that carries references; the frame pastes images and plain text. */
  function handlePaste(event: ClipboardEvent): boolean {
    if (!event.clipboardData || !editorController || editingInput) return false;
    const plainText = event.clipboardData.getData("text/plain").replace(/\r\n?/gu, "\n");
    const semantic = parseReferenceClipboardSlice(
      event.clipboardData.getData(REFERENCE_CLIPBOARD_TYPE),
      plainText,
    );
    if (!semantic) return false;
    event.preventDefault();
    editorController.pasteReferenceSlice(semantic);
    announcement = t("chat.organization.referencesPasted", semantic.references.length);
    return true;
  }

  function parseReferenceClipboardSlice(
    serialized: string,
    plainText: string,
  ): ChatReferenceClipboardSlice | null {
    if (!serialized) return null;
    let value: unknown;
    try {
      value = JSON.parse(serialized);
    } catch {
      return null;
    }
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const record = value as Record<string, unknown>;
    if (record.text !== plainText || typeof record.vaultId !== "string" || !Array.isArray(record.references)) {
      return null;
    }
    try {
      if (record.vaultId !== requireActiveVaultIdentity()) return null;
      return {
        vaultId: record.vaultId,
        text: plainText,
        references: record.references.map((reference, index) => (
          parseChatMessageReference(reference, `Clipboard reference ${index + 1}`)
        )),
      };
    } catch {
      return null;
    }
  }

  async function updateReferencePicker(
    text: string,
    selection: ChatComposerSelection,
    references: readonly ChatMessageReference[],
  ): Promise<void> {
    const trigger = chatReferenceTriggerAtCaret(text, selection, references);
    if (!trigger) {
      closePicker();
      return;
    }
    const request = ++pickerRequest;
    // While the query is being refined, the current results stay until the new ones replace them, so typing never blanks the list.
    const refining = pickerOpen && pickerTrigger?.kind === trigger.kind;
    pickerTrigger = trigger;
    pickerOpen = true;
    if (!refining) {
      pickerIndex = 0;
      pickerLoading = true;
      pickerEntries = [];
    }
    try {
      const entries = trigger.kind === "channel"
        ? await channelCandidates(trigger.query)
        : await mentionCandidates(trigger.query);
      if (request !== pickerRequest) return;
      pickerEntries = entries;
      pickerIndex = firstPickerIndex(entries);
      announcement = t("chat.organization.referenceResults", entries.length);
    } catch (cause: unknown) {
      if (request === pickerRequest) error = chatErrorMessage(cause);
    } finally {
      if (request === pickerRequest) pickerLoading = false;
    }
  }

  async function channelCandidates(query: string): Promise<ReferenceCandidate[]> {
    if (!navigationChannelsLoaded) {
      navigationChannels = await chatApi.listChatNavigationChannels();
      navigationChannelsLoaded = true;
    }
    const normalized = query.trim().toLocaleLowerCase();
    const selectedProjectId = chat.selectedChannel?.projectId ?? null;
    return navigationChannels
      .filter((channel) => channel.archivedAt === null)
      .filter((channel) => !normalized || channel.name.toLocaleLowerCase().includes(normalized)
        || channelAncestry(channel).toLocaleLowerCase().includes(normalized))
      .map((channel) => ({
        key: `channel:${channel.id}`,
        kind: "channel" as const,
        group: channel.projectId === selectedProjectId
          ? t("chat.organization.currentProjectChannels")
          : channelAncestry(channel),
        label: `#${channel.name}`,
        description: channelAncestry(channel),
        channel,
      }))
      .sort(candidateSort);
  }

  async function mentionCandidates(query: string): Promise<ReferenceCandidate[]> {
    const normalized = query.trim().toLocaleLowerCase();
    const memberships = (chat.selectedChannel?.memberships ?? []).filter((membership) => (
      isActiveChannelMember(membership)
        && membership.participant.archivedAt === null
    ));
    const entries: ReferenceCandidate[] = memberships.flatMap((membership) => {
      const agent = chat.agents.find((candidate) => (
        candidate.participant.id === membership.participant.id
      )) ?? null;
      const label = chatParticipantDisplayName(membership.participant, preferences.profileDisplayName, t("chat.timeline.you"));
      if (!matchesQuery(normalized, label, membership.participant.displayName)) return [];
      return [{
        key: `participant:${membership.participant.id}`,
        kind: "participant" as const,
        group: t("chat.organization.channelParticipants"),
        label,
        description: agent ? t("chat.organization.aiAgent") : t("chat.organization.humanMember"),
        participant: membership.participant,
        agent,
      }];
    });
    const projectId = chat.selectedChannel?.projectId;
    const folders = chat.workingFolders.filter((folder) => (
      folder.workingFolder.projectId === projectId
        && folder.workingFolder.archivedAt === null
    ));
    for (const folder of folders) {
      if (!matchesQuery(normalized, folder.workingFolder.displayName)) continue;
      entries.push({
        key: `working-folder:${folder.workingFolder.id}`,
        kind: "workingFolder",
        group: t("chat.organization.projectFolders"),
        label: folder.workingFolder.displayName,
        description: folder.bindingStatus === "available"
          ? t("chat.organization.folderReference")
          : t("chat.organization.folderUnavailable"),
        workingFolderId: folder.workingFolder.id,
      });
    }
    if (normalized) {
      const pathPages = await Promise.allSettled(folders.map(async (folder) => ({
        folder,
        page: await chatApi.searchChatWorkingFolderPaths(
          folder.workingFolder.id,
          query.trim(),
          false,
          null,
          20,
          null,
        ),
      })));
      for (const result of pathPages) {
        if (result.status !== "fulfilled") continue;
        for (const path of result.value.page.entries) {
          entries.push({
            key: `workspace-path:${result.value.folder.workingFolder.id}:${path.kind}:${path.relativePath}`,
            kind: "workspacePath",
            group: result.value.folder.workingFolder.displayName,
            label: path.displayName,
            description: path.relativePath,
            workingFolderId: result.value.folder.workingFolder.id,
            path,
          });
        }
      }
    }
    for (const target of assignmentTargets) {
      if (target.kind !== "worktree" || !matchesQuery(normalized, target.displayName)) continue;
      entries.push({
        key: `execution-environment:${target.executionTarget?.executionEnvironmentId ?? target.displayName}`,
        kind: "executionEnvironment",
        group: t("chat.organization.worktrees"),
        label: target.displayName,
        description: target.eligible
          ? t("chat.organization.worktreeReference")
          : target.unavailableReason ?? t("chat.organization.targetUnavailable"),
        target,
      });
    }
    return entries.sort(candidateSort);
  }

  async function chooseReferenceCandidate(candidate: ReferenceCandidate): Promise<void> {
    const trigger = pickerTrigger;
    if (!trigger || !editorController) return;
    if (candidate.kind === "executionEnvironment" && !candidate.target.eligible) {
      announcement = candidate.target.unavailableReason ?? t("chat.organization.targetUnavailable");
      return;
    }
    if (candidate.kind === "channel") {
      const blocker = await channelDisclosureBlocker(candidate.channel, null);
      if (blocker) {
        disclosureBlocker = blocker;
        closePicker();
        await tick();
        blockerElement?.focus();
        return;
      }
    }
    const reference = candidateReference(candidate);
    if (!reference) return;
    const inserted = editorController.insertReference(trigger.start, trigger.end, reference);
    closePicker();
    if (inserted) {
      announcement = t("chat.organization.referenceInserted", reference.metadata.labelSnapshot);
    } else {
      announcement = t("chat.organization.duplicateReference", reference.metadata.labelSnapshot);
    }
  }

  function closePicker(): void {
    pickerRequest += 1;
    pickerOpen = false;
    pickerTrigger = null;
    pickerEntries = [];
    pickerLoading = false;
    pickerIndex = 0;
  }

  /**
   * Opens the skill or command menu for the typed query. App commands show at once; the provider
   * catalog of the agent's working folder follows when the agent's provider can run here.
   */
  async function updatePromptMenu(kind: "skill" | "command", query: string): Promise<void> {
    const request = ++promptMenuRequest;
    const appEntries = kind === "command" ? agentCommandCatalog() : [];
    const providerInstanceId = targetAgent?.localProviderInstanceId ?? null;
    const workingFolderId = catalogWorkingFolderId;
    const canReadProvider = localExecutionAvailable && providerInstanceId !== null && workingFolderId !== null;
    if (!canReadProvider && kind === "skill") {
      closePromptMenu();
      return;
    }
    promptMenuKind = kind;
    const cached = promptCatalogCache?.key === promptCatalogKey ? promptCatalogCache.entries : null;
    promptMenuEntries = mergePromptCatalog(appEntries, cached ?? [], kind, query);
    promptMenuIndex = Math.min(promptMenuIndex, Math.max(0, promptMenuEntries.length - 1));
    if (!canReadProvider || cached) {
      promptMenuLoading = false;
      return;
    }
    promptMenuLoading = true;
    const key = promptCatalogKey;
    try {
      const entries = await chatApi.listChatPromptCatalog(
        workingFolderId,
        providerInstanceId,
        threadAssignment?.latestRunProviderThreadId ?? null,
      );
      if (key === promptCatalogKey) promptCatalogCache = { key, entries };
      if (request !== promptMenuRequest) return;
      promptMenuEntries = mergePromptCatalog(agentCommandCatalog(), entries, kind, query);
      promptMenuIndex = Math.min(promptMenuIndex, Math.max(0, promptMenuEntries.length - 1));
    } catch {
      // App commands stay available when the provider catalog cannot be read on this device.
      if (request === promptMenuRequest) {
        promptMenuEntries = mergePromptCatalog(agentCommandCatalog(), [], kind, query);
      }
    } finally {
      if (request === promptMenuRequest) promptMenuLoading = false;
    }
  }

  function closePromptMenu(): void {
    promptMenuRequest += 1;
    promptMenuKind = null;
    promptMenuEntries = [];
    promptMenuIndex = 0;
    promptMenuLoading = false;
  }

  /**
   * Applies the chosen menu entry at the typed token: an app command runs and removes the token,
   * while a skill or provider command replaces the token with its text.
   */
  async function choosePromptEntry(index: number): Promise<void> {
    const entry = promptMenuEntries[index];
    if (!entry || !editorController) return;
    const selection = editorController.selection();
    const trigger = composerTokenTrigger(editorController.plainText(), selection.start);
    closePromptMenu();
    if (!trigger || trigger.kind === "mention") return;
    if (entry.source === "app" && !isModelCommandEntry(entry)) {
      editorController.replaceRange(trigger.start, trigger.end, "");
      await runAgentCommand(entry.value);
      return;
    }
    editorController.replaceRange(trigger.start, trigger.end, `${entry.value} `);
  }

  async function runAgentCommand(value: string): Promise<void> {
    switch (value) {
      case "/schedule": toggleScheduleMenu(); return;
      case "/attach": await pickImages(); return;
      case "/stop": await stopAssignment(); return;
      case "/status": statusPanelOpen = true; return;
    }
  }

  /** The app commands an agent conversation offers; provider direct actions never show here. */
  function agentCommandCatalog(): ChatPromptCatalogEntry[] {
    return [
      appCommand("/schedule", t("chat.composer.agentCommands.schedule"), t("chat.composer.agentCommands.scheduleDescription")),
      ...(localExecutionAvailable
        ? [appCommand("/attach", t("chat.composer.agentCommands.attach"), t("chat.composer.agentCommands.attachDescription"))]
        : []),
      ...(activeAssignment
        ? [appCommand("/stop", t("chat.composer.agentCommands.stop"), t("chat.composer.agentCommands.stopDescription"))]
        : []),
      ...(statusAvailable
        ? [appCommand("/status", t("chat.composer.agentCommands.status"), t("chat.composer.agentCommands.statusDescription"))]
        : []),
      ...modelCommandCatalog(),
    ];
  }

  /**
   * The `/model` suggestions: the target agent's models, else every model the channel's agents offer here, else the
   * bare command so it stays discoverable.
   */
  function modelCommandCatalog(): ChatPromptCatalogEntry[] {
    const models = targetAgent
      ? targetProvider?.modelCatalog?.models ?? []
      : channelModelChoices(channelAgentModelCatalogs());
    if (models.length === 0) {
      return [genericModelCommandEntry("/model", t("chat.composer.agentCommands.modelGenericDescription"), t("chat.composer.agentCommands.modelArgument"))];
    }
    return modelCommandEntries(models, (name) => t("chat.composer.agentCommands.modelDescription", name));
  }

  function channelAgentModelCatalogs(): ProviderModel[][] {
    return (chat.selectedChannel?.memberships ?? []).flatMap((membership) => {
      if (membership.participant.kind !== "ai_agent" || !isActiveChannelMember(membership)) return [];
      const agent = chat.agents.find((candidate) => candidate.participant.id === membership.participant.id);
      const provider = chat.settings?.providerInstances.find((entry) => (
        entry.configuration.instanceId === agent?.localProviderInstanceId
      ));
      return provider?.modelCatalog ? [provider.modelCatalog.models] : [];
    });
  }

  function appCommand(value: string, label: string, description: string): ChatPromptCatalogEntry {
    return { value, label, description, argumentHint: null, kind: "command", source: "app", stale: false };
  }

  /** What the status panel shows about the provider session of the thread's latest run. */
  function agentComposerStatus(): ComposerStatusRead {
    const defaultModelId = chatAgentDefaultModelId(targetAgent?.latestPolicy ?? null, targetProvider?.modelCatalog ?? null);
    return {
      providerLabel: targetProvider?.configuration.label ?? null,
      sessionId: interactionState?.sessionId ?? null,
      sessionState: interactionState?.sessionState ?? "stopped",
      modelLabel: targetProvider?.modelCatalog?.models.find((entry) => entry.id === defaultModelId)?.displayName ?? null,
      accountLabel: interactionState?.accountStatus?.accountLabel ?? null,
      planLabel: interactionState?.accountStatus?.planLabel ?? null,
      usage: meterRead,
    };
  }

  async function loadAssignmentInputs(assignmentId: ChatWorkAssignmentId | null): Promise<void> {
    const request = ++assignmentInputsRequest;
    if (!assignmentId) {
      assignmentInputs = [];
      return;
    }
    try {
      const inputs = await chatApi.listChatAssignmentInputs(assignmentId);
      if (request === assignmentInputsRequest) assignmentInputs = inputs;
    } catch {
      // The list keeps its last reading; the thread timeline still shows every reply.
    }
  }

  function replyForInput(input: ChatAssignmentInputRead): ChatMessageRead | null {
    return chat.replyThread?.replies.find((reply) => reply.itemId === input.messageItemId) ?? null;
  }

  function canEditInput(row: PendingInputRow): boolean {
    return !pendingInputStartsWork(row.input) && replyForInput(row.input) !== null;
  }

  /** Replaces the editor content without the store draft, as loading a waiting reply or restoring the draft does. */
  function loadEditorContent(
    document: ChatComposerDocument,
    references: readonly ChatMessageReference[],
    attachments: readonly string[],
    selection?: ChatComposerSelection,
  ): void {
    editorDocument = document;
    normalizedMarkdown = chatComposerMarkdown(document);
    editorReferences = chatReferencesForEditor(document, references);
    attachmentIds = [...attachments];
    const end = chatComposerPlainText(document).length;
    selectionStart = Math.min(selection?.start ?? end, end);
    selectionEnd = Math.min(selection?.end ?? end, end);
    editorController?.setDocument(document, editorReferences);
    editorController?.focus({ start: selectionStart, end: selectionEnd });
  }

  function startInputEdit(row: PendingInputRow): void {
    const reply = replyForInput(row.input);
    if (!reply || editingInput || editingScheduled) return;
    persist();
    closePicker();
    closePromptMenu();
    error = null;
    editingInput = row.input;
    // The saved revision carries no references, so the editor shows their text as plain text.
    loadEditorContent(parseChatComposerDocument(reply.richContent, reply.normalizedMarkdown), [], []);
  }

  function restoreDraftAfterEdit(): void {
    editingInput = null;
    const draft = chat.organizationalDraft(destination);
    loadEditorContent(parseChatComposerDocument(draft.richContent, draft.normalizedMarkdown), draft.references, draft.attachmentIds, {
      start: draft.selectionStart,
      end: draft.selectionEnd,
    });
  }

  function discardInputEdit(): void {
    if (!editingInput || inputBusyId) return;
    restoreDraftAfterEdit();
  }

  /**
   * Loads a scheduled message into the editor with its references, attachments, time, and repeat
   * rule. A repeating message shows its pending occurrence as the time, so saving without touching
   * the time keeps its series where it is.
   */
  function startScheduledEdit(message: ChatScheduledMessageRead): void {
    if (editingInput || editingScheduled || sending || message.state === "dispatching") return;
    persist();
    closePicker();
    closePromptMenu();
    error = null;
    editingScheduled = { message, draftAlsoSendToChannel: alsoSendToChannel };
    const recurrence = message.recurrence;
    scheduledFor = recurrence ? recurrence.nextOccurrenceAt : message.scheduledFor;
    scheduledRecurrence = recurrence
      ? { rule: { ...recurrence.rule }, timeZone: recurrence.timeZone, quietPolicy: recurrence.quietPolicy }
      : null;
    alsoSendToChannel = message.alsoSendToChannel;
    loadEditorContent(
      parseChatComposerDocument(message.richContent, message.normalizedMarkdown),
      message.references,
      message.attachmentIds,
    );
    void loadAttachmentReads(message.attachmentIds);
  }

  /** Ends a scheduled message edit and puts the set aside draft, with its time and repeat rule, back in the editor. */
  function restoreDraftAfterScheduledEdit(): void {
    const editing = editingScheduled;
    if (!editing) return;
    editingScheduled = null;
    const draft = chat.organizationalDraft(destination);
    scheduledFor = draft.scheduledFor;
    scheduledRecurrence = draft.recurrence;
    alsoSendToChannel = editing.draftAlsoSendToChannel;
    loadEditorContent(parseChatComposerDocument(draft.richContent, draft.normalizedMarkdown), draft.references, draft.attachmentIds, {
      start: draft.selectionStart,
      end: draft.selectionEnd,
    });
  }

  function discardScheduledEdit(): void {
    if (!editingScheduled || sending) return;
    restoreDraftAfterScheduledEdit();
  }

  /**
   * Saves the editor over the scheduled message it holds. A message that was sent, cancelled, or
   * started sending meanwhile ends the edit with an explanation and a fresh list.
   */
  async function saveScheduledEdit(): Promise<void> {
    const editing = editingScheduled;
    if (!editing || sending || !hasMessageContent) return;
    const original = editing.message;
    const originalScheduledFor = original.recurrence?.nextOccurrenceAt ?? original.scheduledFor;
    if (!scheduledFor) {
      error = t("chat.organization.invalidScheduleTime");
      return;
    }
    if (validation || providerBlockers.length > 0 || messageTooLarge) {
      await focusBlocker();
      return;
    }
    sending = true;
    error = null;
    try {
      const blocker = await validateChannelDisclosures();
      if (blocker) {
        disclosureBlocker = blocker;
        await focusBlocker();
        return;
      }
      if (scheduledFor !== originalScheduledFor) {
        const { chatScheduleIsFuture } = await import("$lib/chat/composer/message-scheduling");
        if (!chatScheduleIsFuture(scheduledFor)) {
          error = t("chat.organization.invalidScheduleTime");
          return;
        }
      }
      const updated = await chat.updateScheduledOrganizationalMessage({
        scheduledMessageId: original.id,
        scheduledFor,
        normalizedMarkdown,
        richContent: chatComposerDocumentVersioned(editorDocument),
        attachmentIds: [...attachmentIds],
        references: chatReferencesForMarkdown(editorDocument, editorReferences),
        alsoSendToChannel: threadComposer && alsoSendToChannel,
        recurrence: scheduledRecurrence,
      });
      updateScheduledMessages(scheduledMessages.map((message) => message.id === updated.id ? updated : message)
        .sort((left, right) => left.scheduledFor.localeCompare(right.scheduledFor)));
      restoreDraftAfterScheduledEdit();
    } catch (cause: unknown) {
      const code = chatErrorCode(cause);
      error = chatErrorMessage(cause, t("chatErrors.actions.scheduleAction"));
      if (code === "not_found" || code === "busy") {
        restoreDraftAfterScheduledEdit();
        await loadScheduledMessages(destination);
      }
    } finally {
      sending = false;
    }
  }

  /**
   * The explanation of an edit or cancel refused because the reply already left the list's view of
   * it, as when it reached the agent or started its own work after the list loaded, or null for
   * any other failure.
   *
   * @param cause The failure of the edit or cancel.
   */
  function settledInputMessage(cause: unknown): string | null {
    switch (chatErrorReason(cause)) {
      case "input_already_delivered": return t("chat.composer.pendingInputs.alreadyDelivered");
      case "input_starts_work": return t("chat.composer.pendingInputs.startsWork");
      default: return null;
    }
  }

  async function refreshAfterInputChange(): Promise<void> {
    const replyThreadId = destinationReplyThreadId(destination);
    await Promise.all([
      replyThreadId ? chat.loadReplyThread(replyThreadId, true) : Promise.resolve(),
      loadAssignmentInputs(threadAssignment?.id ?? null),
    ]);
  }

  async function saveInputEdit(): Promise<void> {
    const input = editingInput;
    if (!input || inputBusyId || !normalizedMarkdown.trim()) return;
    inputBusyId = input.inputId;
    error = null;
    try {
      await chatApi.editChatAssignmentInput(input.inputId, {
        normalizedMarkdown,
        richContent: chatComposerDocumentVersioned(editorDocument),
      });
      restoreDraftAfterEdit();
      await refreshAfterInputChange();
    } catch (cause: unknown) {
      const settled = settledInputMessage(cause);
      if (settled) {
        error = settled;
        restoreDraftAfterEdit();
        await refreshAfterInputChange().catch(() => undefined);
      } else {
        error = chatErrorMessage(cause, t("chatErrors.actions.updateMessage"));
      }
    } finally {
      inputBusyId = null;
    }
  }

  async function cancelInput(row: PendingInputRow): Promise<void> {
    if (inputBusyId || editingInput) return;
    inputBusyId = row.input.inputId;
    error = null;
    try {
      await chatApi.cancelChatAssignmentInput(row.input.inputId);
      await refreshAfterInputChange();
    } catch (cause: unknown) {
      const settled = settledInputMessage(cause);
      error = settled ?? chatErrorMessage(cause, t("chatErrors.actions.updateMessage"));
      if (settled) await refreshAfterInputChange().catch(() => undefined);
    } finally {
      inputBusyId = null;
    }
  }

  /**
   * Runs one change to a waiting reply and reloads the queue, explaining a refusal because the reply
   * already left the queue.
   *
   * @param row The waiting reply the change applies to.
   * @param change The change to request.
   */
  async function changeWaitingInput(row: PendingInputRow, change: () => Promise<unknown>): Promise<void> {
    if (inputBusyId || editingInput) return;
    inputBusyId = row.input.inputId;
    error = null;
    try {
      await change();
      await refreshAfterInputChange();
    } catch (cause: unknown) {
      const settled = settledInputMessage(cause);
      error = settled ?? chatErrorMessage(cause, t("chatErrors.actions.updateMessage"));
      if (settled) await refreshAfterInputChange().catch(() => undefined);
    } finally {
      inputBusyId = null;
    }
  }

  function setInputHeld(row: PendingInputRow, held: boolean): Promise<void> {
    return changeWaitingInput(row, () => chatApi.setChatAssignmentInputHeld(row.input.inputId, held));
  }

  function steerInput(row: PendingInputRow): Promise<void> {
    return changeWaitingInput(row, () => chatApi.steerChatAssignmentInput(row.input.inputId));
  }

  /** Saves the waiting replies in a new order, reloading the queue when it changed meanwhile. */
  async function reorderInputs(orderedInputIds: string[]): Promise<void> {
    const assignmentId = threadAssignment?.id;
    if (!assignmentId || editingInput) return;
    const request = ++assignmentInputsRequest;
    error = null;
    try {
      const inputs = await chatApi.reorderChatAssignmentInputs(assignmentId, orderedInputIds);
      if (request === assignmentInputsRequest) assignmentInputs = inputs;
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.updateMessage"));
      await loadAssignmentInputs(assignmentId);
    }
  }

  function composerWorkingFolderId(): string | null {
    if (!localExecutionAvailable) return primaryAttachmentFolderId;
    return chat.selectedWorkingFolderId
      ?? chat.workingFolders.find((folder) => folder.workingFolder.projectId === chat.selectedChannel?.projectId)
        ?.workingFolder.id
      ?? null;
  }

  /** Imports images pasted or dropped into the composer into the folder the image picker uses. */
  async function importFiles(files: File[]): Promise<void> {
    if (editLockReason) {
      error = editLockReason;
      return;
    }
    const workingFolderId = composerWorkingFolderId();
    if (!workingFolderId) return;
    const invalid = validateImageFiles(files, attachmentIds.length);
    if (invalid) {
      error = invalid;
      return;
    }
    error = null;
    try {
      const imported = await Promise.all(files.map(async (file) => chatApi.importChatImage(
        workingFolderId,
        crypto.randomUUID(),
        file.name,
        new Uint8Array(await file.arrayBuffer()),
      )));
      rememberAttachments(imported);
      attachmentIds = [...attachmentIds, ...imported.map((attachment) => attachment.id)];
      persist();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.fileAction"));
    }
  }

  function handleImageInput(event: Event & { currentTarget: HTMLInputElement }): void {
    const files = [...(event.currentTarget.files ?? [])];
    event.currentTarget.value = "";
    if (files.length > 0) void importFiles(files);
  }

  /** Attaches pasted or dropped text too large to read inline as a text attachment. */
  async function importLargeText(text: string): Promise<void> {
    if (editLockReason) {
      error = editLockReason;
      return;
    }
    const workingFolderId = composerWorkingFolderId();
    if (!workingFolderId) {
      error = t("chat.composer.paste.attachmentUnavailable");
      return;
    }
    if (attachmentIds.length >= CHAT_IMAGE_LIMIT) {
      error = t("chat.composer.paste.attachmentLimit", CHAT_IMAGE_LIMIT);
      return;
    }
    error = null;
    try {
      const attachment = await chatApi.importChatTextSnippet(
        workingFolderId,
        crypto.randomUUID(),
        t("chat.composer.paste.snippetName", pastedTextLineCount(text)),
        text,
      );
      rememberAttachments([attachment]);
      attachmentIds = [...attachmentIds, attachment.id];
      persist();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.fileAction"));
    }
  }

  function rememberAttachments(attachments: readonly ChatAttachmentRead[]): void {
    attachmentReads = { ...attachmentReads, ...Object.fromEntries(attachments.map((attachment) => [attachment.id, attachment])) };
  }

  /** Reads the draft's attachments so their chips show names; on failure the chips keep generic labels and the error shows. */
  async function loadAttachmentReads(ids: readonly string[]): Promise<void> {
    const workingFolderId = composerWorkingFolderId();
    if (ids.length === 0 || !workingFolderId) return;
    try {
      rememberAttachments(await chatApi.readChatAttachments(workingFolderId, [...ids]));
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.fileAction"));
    }
  }

  function attachmentChipLabel(id: string): string {
    const read = attachmentReads[id];
    if (read?.kind === "text_snippet") return read.originalDisplayName;
    const imageIndex = attachmentIds.filter((entry) => attachmentReads[entry]?.kind !== "text_snippet").indexOf(id);
    return t("chat.organization.imageNumber", imageIndex + 1);
  }

  /** Turns the suggested agent's name into a mention reference; nothing is posted. */
  function applyMentionSuggestion(suggestion: MentionSuggestion): void {
    if (!editorController) return;
    const participant = mentionRoster.find((candidate) => candidate.id === suggestion.participantId);
    if (!participant) return;
    const label = participant.displayName;
    const inserted = editorController.insertReference(suggestion.start, suggestion.end, {
      kind: "participant",
      participantId: participant.id,
      participantKind: "ai_agent",
      metadata: { referenceId: crypto.randomUUID(), labelSnapshot: label, startOffset: 0, endOffset: 0, plainTextProjection: `@${label}` },
    });
    announcement = inserted
      ? t("chat.organization.referenceInserted", label)
      : t("chat.organization.duplicateReference", label);
  }

  async function loadAssignmentPreview(
    agentId: string,
    channelId: string,
    replyThreadId: string | null,
  ): Promise<void> {
    const requestId = ++assignmentPreviewRequest;
    assignmentPreviewLoading = true;
    try {
      const targets = await chatApi.listChatAssignmentTargets({ agentId, channelId, replyThreadId });
      if (
        requestId !== assignmentPreviewRequest
        || mentionedAgentId !== agentId
        || chat.selectedChannel?.id !== channelId
        || destinationReplyThreadId(destination) !== replyThreadId
      ) return;
      assignmentTargets = targets;
    } catch (cause: unknown) {
      if (
        requestId === assignmentPreviewRequest
        && mentionedAgentId === agentId
        && chat.selectedChannel?.id === channelId
        && destinationReplyThreadId(destination) === replyThreadId
      ) {
        assignmentTargets = [];
        error = chatErrorMessage(cause);
      }
    } finally {
      if (requestId === assignmentPreviewRequest) assignmentPreviewLoading = false;
    }
  }

  async function validateChannelDisclosures(): Promise<DisclosureBlocker | null> {
    const channelReferences = editorReferences.filter((reference) => reference.kind === "channel");
    for (const reference of channelReferences) {
      if (reference.kind !== "channel") continue;
      const source = navigationChannels.find((channel) => channel.id === reference.channelId)
        ?? await chatApi.readChatChannel(reference.channelId);
      const blocker = await channelDisclosureBlocker(source, reference.metadata.referenceId);
      if (blocker) return blocker;
    }
    return null;
  }

  async function channelDisclosureBlocker(
    source: ChatChannelRead,
    sourceReferenceId: string | null,
  ): Promise<DisclosureBlocker | null> {
    const destinationChannel = chat.selectedChannel;
    if (!destinationChannel || source.id === destinationChannel.id) return null;
    const [destinationRoster, sourceRoster] = await Promise.all([
      chatApi.readChatChannelRoster(destinationChannel.id),
      chatApi.readChatChannelRoster(source.id),
    ]);
    const sourceReaders = rosterReaderIds(sourceRoster);
    const requester = sourceRoster.memberships.find((membership) => (
      membership.participant.kind === "local_user" && membership.removedAt === null
    ));
    const agentCanRead = aiParticipantReferences.every((reference) => (
      reference.kind !== "participant" || sourceReaders.has(reference.participantId)
    ));
    const missing = rosterReaders(destinationRoster).filter((reader) => !sourceReaders.has(reader.id));
    if (requester && agentCanRead && missing.length === 0) return null;
    const labels = missing.map((reader) => reader.displayName);
    if (!requester) labels.unshift(t("chat.organization.you"));
    if (!agentCanRead) {
      for (const reference of aiParticipantReferences) {
        if (reference.kind === "participant" && !sourceReaders.has(reference.participantId)) {
          labels.push(reference.metadata.labelSnapshot);
        }
      }
    }
    const allAffectedLabels = [...new Set(labels)];
    const visible = allAffectedLabels.slice(0, 3);
    const formatted = new Intl.ListFormat(localization.locale, {
      style: "long",
      type: "conjunction",
    }).format(visible);
    const remaining = Math.max(0, allAffectedLabels.length - visible.length);
    return {
      message: t("chat.organization.channelDisclosureBlocked", `#${source.name}`, formatted, remaining),
      sourceChannelId: source.id,
      sourceReferenceId,
    };
  }

  function rosterReaders(roster: ChatChannelRosterRead): ChatParticipantRead[] {
    return roster.memberships.flatMap((membership) => (isActiveChannelMember(membership) ? [membership.participant] : []));
  }

  function rosterReaderIds(roster: ChatChannelRosterRead): Set<string> {
    return new Set(rosterReaders(roster).map((participant) => participant.id));
  }

  function removeBlockedReference(): void {
    const referenceId = disclosureBlocker?.sourceReferenceId;
    disclosureBlocker = null;
    if (referenceId) editorController?.removeReference(referenceId);
    editorController?.focus();
  }

  function manageBlockedSourceAccess(): void {
    const channelId = disclosureBlocker?.sourceChannelId;
    if (!channelId) return;
    persist();
    paneEvents.dispatchEvent(new CustomEvent("ganbaru:chat-manage-members", {
      detail: { channelId },
    }));
  }

  async function pickImages(): Promise<void> {
    const workingFolderId = composerWorkingFolderId();
    if (!workingFolderId || editLockReason) return;
    const available = Math.max(0, CHAT_IMAGE_LIMIT - attachmentIds.length);
    if (available === 0) return;
    if (!localExecutionAvailable) {
      imageInput?.click();
      return;
    }
    const imported = await chatApi.pickChatImages(
      workingFolderId,
      Array.from({ length: available }, () => crypto.randomUUID()),
      t("chat.composer.attachImages"),
    );
    rememberAttachments(imported);
    attachmentIds = [...attachmentIds, ...imported.map((attachment) => attachment.id)];
    persist();
  }

  function clearComposerAfterDelivery(): void {
    editorDocument = chatComposerDocumentFromText("");
    normalizedMarkdown = "";
    editorReferences = [];
    attachmentIds = [];
    selectionStart = 0;
    selectionEnd = 0;
    alsoSendToChannel = false;
    scheduledFor = null;
    scheduledRecurrence = null;
    assignmentTargets = [];
    disclosureBlocker = null;
    dismissedMentionSuggestions = [];
    editorController?.setDocument(editorDocument, []);
    persist();
  }

  /**
   * Sends the draft, or saves the reply being edited. A reply to work an agent is on carries `followupRouting`;
   * other posts ignore it.
   */
  async function post(followupRouting: ChatFollowupRouting = "queue"): Promise<void> {
    if (editingInput) {
      await saveInputEdit();
      return;
    }
    if (editingScheduled) {
      await saveScheduledEdit();
      return;
    }
    if (sending || !hasMessageContent) return;
    await waitForAssignmentPreview();
    if (!hasMessageContent) return;
    if (validation || providerBlockers.length > 0 || messageTooLarge) {
      await focusBlocker();
      return;
    }
    sending = true;
    error = null;
    let delivered = false;
    try {
      const blocker = await validateChannelDisclosures();
      if (blocker) {
        disclosureBlocker = blocker;
        await focusBlocker();
        return;
      }
      persist();
      if (scheduledFor) {
        const { chatScheduleIsFuture } = await import("$lib/chat/composer/message-scheduling");
        if (!chatScheduleIsFuture(scheduledFor)) {
          error = t("chat.organization.invalidScheduleTime");
          return;
        }
        const scheduled = await chat.scheduleOrganizationalMessage(destination, scheduledFor, {
          alsoSendToChannel: threadComposer && alsoSendToChannel,
        });
        updateScheduledMessages([...scheduledMessages.filter((message) => message.id !== scheduled.id), scheduled]
          .sort((left, right) => left.scheduledFor.localeCompare(right.scheduledFor)));
      } else {
        const routing = routesFollowups ? followupRouting : null;
        const result = await chat.postOrganizationalMessage(destination, {
          alsoSendToChannel: threadComposer && alsoSendToChannel,
          ...(routing ? { followupRouting: routing } : {}),
          // The post is in the vault once written, so the composer clears and takes the next message without waiting for
          // the reads after it.
          onWritten: () => {
            delivered = true;
            sending = false;
            clearComposerAfterDelivery();
          },
        });
        routingNotice = routing === "steer" && result.routedAs === "queue"
          ? t("chat.composer.routing.downgraded")
          : null;
        onRequestScrollToBottom();
      }
      if (!delivered) clearComposerAfterDelivery();
    } catch (cause: unknown) {
      error = delivered
        ? t("chatErrors.actions.refreshAfterSend")
        : chatErrorMessage(cause, t("chatErrors.actions.sendMessage"));
    } finally {
      if (!delivered) sending = false;
    }
  }

  /**
   * Waits for the mentioned agent's access and targets so a send checks its blockers against them instead of
   * flashing a resolving state; the send stays busy meanwhile so a second send cannot start.
   */
  async function waitForAssignmentPreview(): Promise<void> {
    if (!assignmentPreviewLoading || !assignmentPreviewPending) return;
    sending = true;
    try {
      while (assignmentPreviewLoading && assignmentPreviewPending) await assignmentPreviewPending;
    } finally {
      sending = false;
    }
  }

  function providerLimitMessage(resetsAt: string | null): string {
    return resetsAt === null
      ? t("chat.composer.providerLimitUntimed")
      : t("chat.composer.providerLimit", formatDateTime(localization.locale, Date.parse(resetsAt), { hour: "numeric", minute: "2-digit" }));
  }

  async function focusBlocker(): Promise<void> {
    await tick();
    blockerElement?.focus();
  }

  async function stopAssignment(): Promise<void> {
    const assignment = activeAssignment;
    if (!assignment || stopping) return;
    stopping = true;
    error = null;
    try {
      await chat.cancelAssignment(assignment.id);
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.workAction"));
    } finally {
      stopping = false;
    }
  }

  async function reviewWork(review: ChatAssignmentReviewRequest): Promise<boolean> {
    const assignment = assignmentAwaitingReview;
    if (!assignment || reviewing) return false;
    error = null;
    reviewing = true;
    try {
      const reviewed = await chat.reviewAssignment(assignment.id, review);
      if (reviewed) {
        reviewNote = "";
        reviewNoteOpen = false;
      } else {
        error = t("chat.organization.reviewChanged");
      }
      return reviewed;
    } catch (cause: unknown) {
      error = reviewRefusalMessage(cause);
      return false;
    } finally {
      reviewing = false;
    }
  }

  /** The localized reason a review was refused, or the failure message when the refusal names no known reason. */
  function reviewRefusalMessage(cause: unknown): string {
    switch (chatErrorReason(cause)) {
      case "pending_replies": return t("chat.organization.reviewPendingReplies");
      case "no_executing_device": return t("chat.organization.reviewNoDevice");
      default: return chatErrorMessage(cause, t("chatErrors.actions.workAction"));
    }
  }

  /**
   * Moves a Request changes note the card no longer shows into the reply draft, where it posts as a plain reply,
   * which also asks for more work. The note follows any text already in the draft.
   */
  function releaseReviewNote(): void {
    const note = reviewNote.trim();
    reviewNote = "";
    reviewNoteOpen = false;
    if (!note) return;
    const existing = chatComposerPlainText(editorDocument);
    const end = existing.length;
    const separator = existing.trim() ? "\n\n" : "";
    const next = replaceChatComposerText(editorDocument, { start: end, end }, `${separator}${note}`);
    editorDocument = next.document;
    normalizedMarkdown = chatComposerMarkdown(next.document);
    selectionStart = next.selection.start;
    selectionEnd = next.selection.end;
    editorController?.setDocument(editorDocument, editorReferences);
    persist();
    error = t("chat.organization.reviewNoteMoved");
  }

  async function acceptReviewedWork(): Promise<boolean> {
    const reviewed = await reviewWork({ kind: "accept" });
    if (reviewed) announcement = t("chat.organization.reviewAccepted");
    return reviewed;
  }

  async function requestChangesOnReviewedWork(note: string): Promise<boolean> {
    const reviewed = await reviewWork({ kind: "requestChanges", note });
    if (reviewed) onRequestScrollToBottom();
    return reviewed;
  }

  function formatScheduledInstant(value: string): string {
    const instant = new Date(value);
    if (!Number.isFinite(instant.getTime())) return value;
    return new Intl.DateTimeFormat(localization.locale, {
      weekday: "short",
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    }).format(instant);
  }

  function loadScheduleMenu(): Promise<void> {
    if (ScheduleMenu) return Promise.resolve();
    scheduleMenuLoad ??= import("./ChatMessageScheduleMenu.svelte")
      .then((module) => { ScheduleMenu = module.default; })
      .catch((cause: unknown) => { error = chatErrorMessage(cause); })
      .finally(() => { scheduleMenuLoad = null; });
    return scheduleMenuLoad;
  }

  /** Loads the emoji picker on first use; it then mounts already open. */
  async function openEmojiPicker(): Promise<void> {
    if (EmojiPicker) return;
    try {
      EmojiPicker = (await import("$lib/components/icon-picker/IconPicker.svelte")).default;
    } catch (cause: unknown) {
      error = chatErrorMessage(cause);
    }
  }

  /** Puts a picked native emoji in place of the draft selection and keeps the caret after it. */
  function insertEmoji(value: string): void {
    const icon = parseProjectIcon(value);
    if (icon.kind !== "emoji") return;
    const next = replaceChatComposerText(editorDocument, { start: selectionStart, end: selectionEnd }, icon.emoji);
    editorDocument = next.document;
    normalizedMarkdown = chatComposerMarkdown(next.document);
    selectionStart = next.selection.end;
    selectionEnd = next.selection.end;
    editorController?.setDocument(editorDocument, editorReferences);
    editorController?.focus({ start: selectionStart, end: selectionEnd });
    persist();
  }

  function toggleScheduleMenu(): void {
    scheduleMenuOpen = !scheduleMenuOpen;
    scheduledMessagesOpen = false;
    if (scheduleMenuOpen) void loadScheduleMenu();
  }

  function selectSchedule(nextScheduledFor: string, nextRecurrence: ChatRecurrenceSpec | null): void {
    scheduledFor = nextScheduledFor;
    scheduledRecurrence = nextRecurrence;
    persist();
  }

  function clearSchedule(): void {
    scheduledFor = null;
    scheduledRecurrence = null;
    persist();
  }

  function toggleScheduledMessages(): void {
    scheduledMessagesOpen = !scheduledMessagesOpen;
    scheduleMenuOpen = false;
    if (scheduledMessagesOpen) void loadScheduleMenu();
  }

  async function loadScheduledMessages(currentDestination: string): Promise<void> {
    const request = ++scheduledMessagesRequest;
    try {
      const messages = await chat.listScheduledOrganizationalMessages(currentDestination);
      if (request !== scheduledMessagesRequest || currentDestination !== destination) return;
      scheduledMessages = messages;
      if (messages.length === 0) scheduledMessagesOpen = false;
    } catch {
      // Preserve the last successful summary during a transient read failure.
    }
  }

  function updateScheduledMessages(messages: ChatScheduledMessageRead[]): void {
    scheduledMessages = messages;
    chat.rememberScheduledMessages(destination, messages);
    if (messages.length === 0) scheduledMessagesOpen = false;
  }

  function channelAncestry(channel: ChatChannelRead): string {
    const project = projects.projects.find((entry) => entry.id === channel.projectId);
    const group = projects.groups.find((entry) => entry.id === project?.groupId);
    return [group?.name, project?.name].filter(Boolean).join(" / ") || channel.projectId;
  }

  function candidateSort(left: ReferenceCandidate, right: ReferenceCandidate): number {
    return candidateRank(left) - candidateRank(right)
      || left.group.localeCompare(right.group)
      || left.label.localeCompare(right.label);
  }

  function candidateRank(candidate: ReferenceCandidate): number {
    if (candidate.kind === "channel") {
      return candidate.channel.projectId === chat.selectedChannel?.projectId ? 0 : 1;
    }
    if (candidate.kind === "participant") return 0;
    if (candidate.kind === "workingFolder") return 2;
    if (candidate.kind === "workspacePath") return 3;
    return 4;
  }

  /**
   * Why the mentioned agent cannot take this post's work here. An agent without a provider on this device is
   * no blocker: its work waits in the thread for Start here on a device that has one.
   */
  function assignmentProviderBlockers(
    agentId: string | null,
    target: ChatAssignmentTargetRead | null,
    references: readonly ChatMessageReference[],
    channel: ChatChannelRead | null,
    targets: readonly ChatAssignmentTargetRead[],
  ): string[] {
    if (!agentId) return [];
    const agent = chat.agents.find((candidate) => candidate.participant.id === agentId);
    const blockers: string[] = [];
    if (agent && agentNeedsSetupHere(agent.configurationState, localExecutionAvailable)) {
      blockers.push(t("chat.organization.agentNeedsSetup", agent.participant.displayName));
    }
    if (target && !target.eligible) {
      blockers.push(target.unavailableReason ?? t("chat.organization.targetUnavailable"));
    }
    const member = channel?.memberships.some((membership) => (
      membership.participant.id === agentId && isActiveChannelMember(membership)
    )) ?? false;
    if (!assignmentPreviewLoading && !member) {
      blockers.push(t("chat.organization.agentChannelAccessUnavailable"));
    }
    const unavailableEnvironments = references
      .filter((reference): reference is Extract<ChatMessageReference, { kind: "executionEnvironment" }> => (
        reference.kind === "executionEnvironment"
      ))
      .filter((reference) => !targets.some((candidate) => (
        candidate.executionTarget?.executionEnvironmentId === reference.executionEnvironmentId
          && candidate.eligible
      )))
      .map((reference) => reference.metadata.labelSnapshot);
    if (!assignmentPreviewLoading && unavailableEnvironments.length > 0) {
      blockers.push(t(
        "chat.organization.unavailableEnvironmentReferences",
        new Intl.ListFormat(localization.locale, { style: "long", type: "conjunction" })
          .format(unavailableEnvironments),
      ));
    }
    const provider = agent?.localProviderInstanceId
      ? chat.settings?.providerInstances.find((entry) => entry.configuration.instanceId === agent.localProviderInstanceId)
      : undefined;
    const authority = provider?.lastProbe?.authoritySupport;
    if (provider && !authority && !assignmentPreviewLoading) {
      blockers.push(t("chat.organization.providerAuthorityNotVerified"));
    }
    const requiresHostTools = references.some((reference) => (
      reference.kind === "channel"
        || reference.kind === "workingFolder"
        || reference.kind === "workspacePath"
    ));
    if (requiresHostTools && authority && !authority.internalHostTools) {
      blockers.push(t("chat.organization.providerCannotUseHostTools"));
    }
    if (target && authority) {
      for (const blocker of providerTargetAuthorityBlockers(authority, target)) {
        blockers.push(t(blocker));
      }
    }
    return [...new Set(blockers)];
  }

  function destinationReplyThreadId(value: string): string | null {
    return value.startsWith("reply-thread:") ? value.slice("reply-thread:".length) : null;
  }
</script>

<div class="composer-stack">
  <span class="sr-only" aria-live="polite">{announcement}</span>
  {#if providerLimit}
    <Notice class="assignment-notice" role="status" tone="warning" message={providerLimitMessage(providerLimit.resetsAt)} copy={false} />
  {/if}
  {#if assignmentAwaitingReview}
    <ChatAssignmentReviewCard bind:note={reviewNote} bind:composingNote={reviewNoteOpen} onAccept={acceptReviewedWork} onRequestChanges={requestChangesOnReviewedWork} />
  {/if}
  {#if editingScheduled}
    <div class="scheduled-summary" data-scheduled-edit>
      <Pencil size={13} />
      <span>{t("chat.organization.editingScheduledMessage")}</span>
      <button type="button" data-scheduled-edit-action="discard" disabled={sending} onclick={discardScheduledEdit}>{t("chat.organization.discardScheduledMessageEdit")}</button>
      <button type="button" data-scheduled-edit-action="save" disabled={sending || !hasMessageContent} onclick={() => { void saveScheduledEdit(); }}>{t("chat.composer.pendingInputs.save")}</button>
    </div>
  {:else if nextScheduledMessage}
    <div class="scheduled-summary">
      <Clock3 size={13} />
      <span>{t("chat.organization.scheduledMessageSummary", scheduledMessages.length, formatScheduledInstant(nextScheduledDelivery ?? nextScheduledMessage.scheduledFor))}</span>
      <div bind:this={scheduledMessagesAnchor} class="scheduled-summary-anchor">
        <button type="button" aria-haspopup="dialog" aria-expanded={scheduledMessagesOpen} onpointerenter={() => { void loadScheduleMenu(); }} onfocus={() => { void loadScheduleMenu(); }} onclick={toggleScheduledMessages}>{t("chat.organization.viewScheduledMessages", scheduledMessages.length)}</button>
        {#if scheduledMessagesOpen}
          {#if ScheduleMenu}
            {@const LoadedScheduledMessagesMenu = ScheduleMenu}
            <LoadedScheduledMessagesMenu mode="manage" align="right" {scheduledMessages} onMessagesChange={updateScheduledMessages} onEdit={editingInput ? undefined : startScheduledEdit} onClose={() => { scheduledMessagesOpen = false; }} />
          {:else}<div class="composer-menu schedule-loading align-right surface-floating surface-floating-body"><Loading layout="fill" /></div>{/if}
        {/if}
      </div>
    </div>
  {/if}

  {#if taskProgress}<ChatTaskProgressCard summary={taskProgress} />{/if}

  {#if pendingRows.length > 0}
    <ChatQueuedReplies rows={pendingRows} editingInputId={editingInput?.inputId ?? null} busyInputId={inputBusyId} canEdit={canEditInput} onEdit={startInputEdit} onCancel={(row) => { void cancelInput(row); }} onSave={() => { void saveInputEdit(); }} onDiscard={discardInputEdit} onSetHeld={(row, held) => { void setInputHeld(row, held); }} onSteer={(row) => { void steerInput(row); }} onReorder={reorderInputs} />
  {/if}

  <div bind:this={composerRoot} class="organizational-composer" data-organizational-composer onfocusin={() => { focusWithin = true; }} onfocusout={trackFocusLeaving}>
    <ChatComposerFrame
      bind:editorRoot
      {collapsible}
      collapsed={composerCollapsed({
        collapsible,
        focusWithin,
        hasContent: hasMessageContent,
        optionSet: Boolean(scheduledFor) || alsoSendToChannel,
        menuOpen: pickerOpen || promptMenuKind !== null || scheduleMenuOpen || statusPanelOpen || emojiPickerOpen,
        showsStatus: Boolean(error || routingNotice || validation || disclosureBlocker || providerBlockers.length || messageTooLarge || editingInput || editingScheduled),
      })}
      editor={() => editorController ?? undefined}
      disabled={false}
      label={placeholder}
      {placeholder}
      role="combobox"
      menuOpen={pickerOpen || promptMenuKind !== null}
      menuId={pickerOpen ? "chat-reference-picker" : "chat-composer-menu"}
      activeOptionId={pickerOpen
        ? pickerEntries[pickerIndex] ? `chat-reference-${pickerIndex}` : undefined
        : promptMenuEntries[promptMenuIndex] ? `chat-composer-option-${promptMenuIndex}` : undefined}
      showMeter={false}
      onKeydown={handleKeydown}
      onFiles={(files) => { void importFiles(files); }}
      onLargeText={(text) => { void importLargeText(text); }}
      onPaste={handlePaste}
      onCopy={handleCopy}
      onCompositionChange={(value) => { composing = value; }}
    >
      {#snippet above()}
        {#if statusPanelOpen}
          <ChatComposerInfoPanel panel="status" status={agentComposerStatus()} mcpStatus={null} loading={false} error={null} onClose={() => { statusPanelOpen = false; }} />
        {/if}
        {#if attachmentIds.length > 0}
          <div class="context-chips">
            {#each attachmentIds as id (id)}
              {@const chipLabel = attachmentChipLabel(id)}
              <button type="button" aria-label={t("chat.composer.removeAttachment", chipLabel)} onclick={() => { attachmentIds = attachmentIds.filter((entry) => entry !== id); persist(); }}>{#if attachmentReads[id]?.kind === "text_snippet"}<FileText size={12} />{:else}<Image size={12} />{/if}<span class="chip-label">{chipLabel}</span><span aria-hidden="true">×</span></button>
            {/each}
          </div>
        {/if}
        {#if mentionSuggestion}
          {@const suggestion = mentionSuggestion}
          <div class="mention-suggestion" data-chat-mention-suggestion>
            <button type="button" class="mention-suggestion-action" onpointerdown={(event) => event.preventDefault()} onclick={() => applyMentionSuggestion(suggestion)}><AtSign size={12} />{t("chat.composer.mentionSuggestion.action", suggestion.displayName)}</button>
            <button type="button" class="mention-suggestion-dismiss" aria-label={t("chat.composer.mentionSuggestion.dismiss")} onclick={() => { dismissedMentionSuggestions = [...dismissedMentionSuggestions, suggestion.participantId]; }}><X size={11} /></button>
          </div>
        {/if}
      {/snippet}
      {#snippet menu()}
        {#if pickerOpen}
          <div bind:this={pickerRoot} use:chatReferencePickerPosition={{ composer: composerRoot, scrollArea: () => pickerScroll, contentKey: [pickerLoading, pickerEntries] }} id="chat-reference-picker" class="reference-picker surface-floating" data-app-floating-surface role="listbox" aria-label={pickerTrigger?.kind === "channel" ? t("chat.organization.referenceChannel") : t("chat.organization.referencePeopleAndResources")}>
            <div bind:this={pickerScroll} class="reference-picker-scroll surface-floating-body" use:scrollEdgeFadeAction>
              {#if pickerLoading}<Loading layout="rows" rows={3} />
              {:else if pickerEntries.length === 0}<p>{t("chat.organization.noReferenceResults")}</p>
              {:else}
                {#each pickerGroups as group, groupIndex (group.label)}
                  <div role="group" aria-labelledby={`chat-reference-group-${groupIndex}`}>
                    <div id={`chat-reference-group-${groupIndex}`} class="picker-group menu-label">{group.label}</div>
                    {#each group.entries as entry (entry.candidate.key)}
                      {@const candidate = entry.candidate}
                      {@const index = entry.index}
                      <button id={`chat-reference-${index}`} data-reference-index={index} type="button" role="option" tabindex="-1" aria-selected={pickerIndex === index} disabled={candidateIsDisabled(candidate)} class="menu-item" data-highlighted={pickerIndex === index ? "" : undefined} onpointerenter={() => { if (!candidateIsDisabled(candidate)) pickerIndex = index; }} onpointerdown={(event) => event.preventDefault()} onclick={() => void chooseReferenceCandidate(candidate)}>
                        <span class="candidate-icon">
                          {#if candidate.kind === "participant"}<ChatParticipantAvatar participant={candidate.participant} agent={candidate.agent ?? undefined} size={28} />
                          {:else if candidate.kind === "channel"}<Hash size={15} />
                          {:else if candidate.kind === "workingFolder"}<Folder size={15} />
                          {:else if candidate.kind === "workspacePath"}{#if candidate.path.kind === "directory"}<Folder size={15} />{:else}<FileIcon size={15} />{/if}
                          {:else}<GitBranch size={15} />{/if}
                        </span>
                        <span class="candidate-copy"><strong>{candidate.label}</strong><small>{candidate.description}</small></span>                      </button>
                    {/each}
                  </div>
                {/each}
              {/if}
            </div>
          </div>
        {:else if promptMenuKind}
          <ChatComposerSuggestionMenu bind:element={promptMenuRoot} bind:includeIgnored={promptMenuIncludeIgnored} kind={promptMenuKind} entries={promptMenuEntries} selectedIndex={promptMenuIndex} loading={promptMenuLoading} hasMore={false} onRefreshMentions={() => undefined} onChoose={(index) => { void choosePromptEntry(index); }} onLoadMore={() => undefined} />
        {/if}
      {/snippet}
      {#snippet leading()}
        <button type="button" class="tool-button" disabled={editLockReason !== null || (!localExecutionAvailable && primaryAttachmentFolderId === null)} aria-label={t("chat.composer.attachImages")} title={editLockReason ?? t("chat.composer.attachImages")} onclick={() => void pickImages()}><Plus size={16} /></button>
        {#if !localExecutionAvailable}<input bind:this={imageInput} type="file" accept={CHAT_IMAGE_CONTENT_TYPES.join(",")} multiple class="hidden" tabindex="-1" aria-hidden="true" onchange={handleImageInput} />{/if}
        {#if EmojiPicker}
          <EmojiPicker value="none" onChange={insertEmoji} ariaLabel={t("chat.composer.insertEmoji")} showIcons={false} showUpload={false} showRemove={false} showCustomEmoji={false} panelAlign="start" initiallyOpen onOpenChange={(open) => { emojiPickerOpen = open; }}>
            {#snippet trigger({ open, toggle, panelId })}
              <button type="button" class="tool-button" class:active={open} aria-label={t("chat.composer.insertEmoji")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} disabled={editLockReason !== null} title={editLockReason ?? t("chat.composer.insertEmoji")} onclick={toggle}><Smile size={15} /></button>
            {/snippet}
          </EmojiPicker>
        {:else}
          <button type="button" class="tool-button" aria-label={t("chat.composer.insertEmoji")} aria-haspopup="dialog" aria-expanded="false" disabled={editLockReason !== null} title={editLockReason ?? t("chat.composer.insertEmoji")} onclick={() => void openEmojiPicker()}><Smile size={15} /></button>
        {/if}
        <div bind:this={scheduleMenuAnchor} class="menu-anchor">
          <button type="button" class="tool-button" class:active={scheduleMenuOpen || Boolean(scheduledFor)} aria-label={scheduledFor ? t("chat.organization.scheduleSelected", formatScheduledInstant(scheduledFor)) : t("chat.organization.scheduleMessage")} aria-haspopup="dialog" aria-expanded={scheduleMenuOpen} disabled={editLockReason !== null} aria-pressed={Boolean(scheduledFor)} title={editLockReason ?? (scheduledFor ? t("chat.organization.scheduleSelected", formatScheduledInstant(scheduledFor)) : t("chat.organization.scheduleMessage"))} onpointerenter={() => { void loadScheduleMenu(); }} onfocus={() => { void loadScheduleMenu(); }} onclick={toggleScheduleMenu}><Clock3 size={15} /></button>
          {#if scheduleMenuOpen}
            {#if ScheduleMenu}
              {@const LoadedScheduleMenu = ScheduleMenu}
              <LoadedScheduleMenu mode="choose" persistent selectedScheduledFor={scheduledFor} selectedRecurrence={scheduledRecurrence} allowClear={!editingScheduled} disabled={sending} onSelect={selectSchedule} onClear={clearSchedule} onClose={() => { scheduleMenuOpen = false; }} />
            {:else}<div class="composer-menu schedule-loading surface-floating surface-floating-body"><Loading layout="fill" /></div>{/if}
          {/if}
        </div>
        {#if threadComposer}<button type="button" class="tool-button" class:active={alsoSendToChannel} aria-label={t("chat.organization.shareReplyToChannel", channelName)} disabled={editLockReason !== null} aria-pressed={alsoSendToChannel} title={editLockReason ?? t("chat.organization.shareReplyToChannel", channelName)} onclick={() => { alsoSendToChannel = !alsoSendToChannel; }}><MessageSquareShare size={15} /></button>{/if}
      {/snippet}
      {#snippet actions()}
        <button type="button" class="send-button" data-action={editingInput || editingScheduled ? "save" : "send"} disabled={sending || inputBusyId !== null || !hasMessageContent} aria-disabled={sendBlocked} aria-label={editingInput ? t("chat.composer.pendingInputs.save") : editingScheduled ? t("chat.organization.saveScheduledMessage") : scheduledFor ? t("chat.organization.scheduleMessage") : t("chat.composer.send")} data-app-tooltip={sendTooltip} onclick={() => { void post(); }}><ArrowUp size={16} /></button>
      {/snippet}
      {#snippet below()}
        {#if validation || disclosureBlocker || providerBlockers.length || messageTooLarge}
          <div bind:this={blockerElement} class="composer-blocker" tabindex="-1" role="alert">
            <CircleAlert size={13} />
            <div>
              {#if validation}<p>{validation}</p>{/if}
              {#if disclosureBlocker}<p>{disclosureBlocker.message}</p><span><button type="button" onclick={removeBlockedReference}>{t("chat.organization.removeReference")}</button><button type="button" onclick={manageBlockedSourceAccess}>{t("chat.organization.manageSourceAccess")}</button></span>{/if}
              {#each providerBlockers as blocker}<p>{blocker}</p>{/each}
              {#if messageTooLarge}<p>{t("chat.organization.messageTooLarge")}</p>{/if}
            </div>
          </div>
        {/if}
        {#if error}<Notice class="composer-error" message={error} />{/if}
        {#if routingNotice && !error}<Notice class="composer-error" role="status" tone="neutral" message={routingNotice} copy={false} />{/if}
      {/snippet}
    </ChatComposerFrame>
  </div>
</div>

<style>
  /* The stack floats over the timeline end, so its gaps pass pointer input through and each surface above the composer paints an opaque background. */
  .composer-stack { width:min(100%,var(--chat-conversation-max-width,60rem)); pointer-events:none; }
  .composer-stack > :global(*) { pointer-events:auto; }
  .scheduled-summary { display:flex; min-height:2rem; align-items:center; gap:0.45rem; margin:0 0.35rem 0.35rem; border-radius:0.55rem; padding:0.25rem 0.35rem 0.25rem 0.55rem; }
  .composer-stack :global(.assignment-notice) { margin:0 0.35rem 0.35rem; border-radius:0.55rem; background:var(--cal-bg); padding:0.25rem 0.35rem 0.25rem 0.55rem; font-size:calc(0.7rem * var(--type-scale)); }
  .scheduled-summary > span { min-width:0; flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:calc(0.7rem * var(--type-scale)); }
  .scheduled-summary button { min-height:1.75rem; border-radius:0.4rem; padding:0.2rem 0.45rem; font-size:calc(0.68rem * var(--type-scale)); font-weight:500; }
  .scheduled-summary { background:color-mix(in srgb,var(--accent) 58%,var(--cal-bg)); color:var(--muted-foreground); }
  .scheduled-summary-anchor,.menu-anchor { position:relative; }
  .organizational-composer { --composer-button-size:1.9rem; --composer-send-visual-size:2.35rem; --composer-control-size:var(--composer-button-size); position:relative; width:100%; }
  .reference-picker { z-index:100; display:flex; flex-direction:column; overflow:hidden; }
  .reference-picker-scroll { min-height:0; flex:1 1 auto; overflow-y:auto; }
  .reference-picker-scroll > p { padding:0.5rem; color:var(--muted-foreground); font-size:var(--panel-detail-font-size); }
  .picker-group { position:sticky; top:calc(-1 * var(--floating-padding)); z-index:1; background:var(--popover); }
  .candidate-icon { display:grid; width:2rem; flex:0 0 auto; place-items:center; color:var(--muted-foreground); }
  .candidate-copy { display:grid; min-width:0; flex:1 1 0%; }
  .reference-picker strong,.reference-picker small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }.reference-picker strong { font-weight:500; }.reference-picker small { color:var(--muted-foreground); font-size:var(--panel-detail-font-size); }
  .context-chips { display:flex; flex-wrap:wrap; gap:0.3rem; padding:0.65rem 0.5rem 0; }.context-chips button { display:flex; max-width:14rem; align-items:center; gap:0.25rem; border-radius:999px; background:var(--accent); padding:0.22rem 0.45rem; font-size:calc(0.68rem * var(--type-scale)); }.context-chips button span { color:var(--muted-foreground); }.context-chips .chip-label { overflow:hidden; color:inherit; text-overflow:ellipsis; white-space:nowrap; }
  .mention-suggestion { display:flex; width:fit-content; align-items:center; gap:0.1rem; margin-top:0.45rem; border-radius:999px; background:color-mix(in srgb,var(--primary) 10%,transparent); padding:0.1rem 0.15rem 0.1rem 0.45rem; color:color-mix(in srgb,var(--primary) 78%,var(--foreground)); font-size:calc(0.68rem * var(--type-scale)); }.mention-suggestion-action { display:flex; align-items:center; gap:0.25rem; }.mention-suggestion-dismiss { display:grid; width:1.25rem; height:1.25rem; place-items:center; border-radius:999px; color:var(--muted-foreground); }.mention-suggestion-dismiss:hover { background:var(--accent); color:var(--foreground); }
  .tool-button { display:grid; width:var(--composer-button-size); height:var(--composer-button-size); place-items:center; border-radius:0.5rem; color:var(--muted-foreground); }.tool-button:hover:not(:disabled),.tool-button.active { background:var(--accent); color:var(--foreground); }.tool-button:disabled { opacity:0.45; }
  .send-button { display:grid; width:var(--composer-button-size); height:var(--composer-button-size); flex:0 0 auto; place-items:center; border-radius:999px; background:color-mix(in srgb,var(--primary) 92%,transparent); color:var(--primary-foreground); box-shadow:0 2px 7px color-mix(in srgb,var(--primary) 22%,transparent); transition:transform 120ms ease,filter 120ms ease; }.send-button:hover:not(:disabled) { filter:brightness(1.04); transform:scale(1.04); }.send-button:disabled,.send-button[aria-disabled="true"] { opacity:0.45; }
  .composer-menu { position:absolute; z-index:110; max-width:calc(100vw - 1rem); }.schedule-loading { bottom:calc(100% + 0.35rem); left:0; }.schedule-loading.align-right { right:0; left:auto; }
  .composer-blocker { display:flex; gap:0.45rem; margin:0 0.65rem 0.5rem; border-radius:0.5rem; background:color-mix(in srgb,var(--destructive) 8%,transparent); padding:0.45rem 0.55rem; color:var(--destructive); outline:none; }.composer-blocker:focus-visible { outline:2px solid var(--ring); }.composer-blocker > div { display:grid; gap:0.2rem; }.composer-blocker p { font-size:calc(0.68rem * var(--type-scale)); }.composer-blocker span { display:flex; flex-wrap:wrap; gap:0.25rem; }.composer-blocker button { min-height:1.75rem; border-radius:0.35rem; padding:0.2rem 0.4rem; font-size:calc(0.62rem * var(--type-scale)); font-weight:600; }.composer-blocker button:hover { background:color-mix(in srgb,var(--destructive) 10%,transparent); }
  .composer-stack :global(.composer-error) { padding:0 0.5rem 0.5rem; font-size:calc(0.7rem * var(--type-scale)); }
  .sr-only { position:absolute; width:1px; height:1px; overflow:hidden; clip:rect(0,0,0,0); white-space:nowrap; }
  @media (pointer:coarse) { .tool-button,.send-button { min-width:2.75rem; min-height:2.75rem; } .organizational-composer { --composer-control-size:2.75rem; } .send-button { padding:calc((2.75rem - var(--composer-send-visual-size)) / 2); background-clip:content-box; box-shadow:none; filter:drop-shadow(0 2px 4px color-mix(in srgb,var(--primary) 22%,transparent)); } }
  @media (prefers-reduced-motion:reduce) { .send-button { transition:none; } }
</style>
