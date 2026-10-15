<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import AtSign from "@lucide/svelte/icons/at-sign";
  import Bold from "@lucide/svelte/icons/bold";
  import Bot from "@lucide/svelte/icons/bot";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Clock3 from "@lucide/svelte/icons/clock-3";
  import File from "@lucide/svelte/icons/file";
  import Folder from "@lucide/svelte/icons/folder";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import Hash from "@lucide/svelte/icons/hash";
  import Image from "@lucide/svelte/icons/image";
  import Italic from "@lucide/svelte/icons/italic";
  import MessageSquareShare from "@lucide/svelte/icons/message-square-share";
  import Plus from "@lucide/svelte/icons/plus";
  import Square from "@lucide/svelte/icons/square";
  import * as chatApi from "$lib/api/chat";
  import type {
    ChatAiTeammateRead,
    ChatAssignmentTargetRead,
    ChatChannelRead,
    ChatChannelRosterRead,
    ChatExecutionTarget,
    ChatMessageReference,
    ChatParticipantRead,
    ChatScheduledMessageRead,
    ChatTeammateAccessRead,
    ChatWorkAssignmentState,
    ProjectWorkingFolderPathRead,
  } from "$lib/chat/contracts";
  import { ChatComposerEditor, type ChatComposerEditorChange } from "$lib/chat/composer/editor";
  import { CHAT_IMAGE_LIMIT } from "$lib/chat/composer/model";
  import {
    chatComposerDocumentFromText,
    chatComposerDocumentVersioned,
    chatComposerMarkdown,
    chatComposerPlainText,
    parseChatComposerDocument,
    type ChatComposerDocument,
    type ChatComposerMark,
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
  import { parseChatMessageReference } from "$lib/chat/validation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { getChat, type ChatOrganizationalDraft } from "$lib/stores/chat.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { requireActiveVaultIdentity } from "$lib/vault/active-vault";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { isAppFloatingSurfaceTarget } from "$lib/utils";
  import { cloneData } from "$lib/utils/clone";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  type ScheduleMenuComponent = typeof import("./ChatMessageScheduleMenu.svelte").default;

  type ReferenceCandidate =
    | {
        key: string;
        kind: "participant";
        group: string;
        label: string;
        description: string;
        participant: ChatParticipantRead;
        teammate: ChatAiTeammateRead | null;
        needsMembership: boolean;
      }
    | {
        key: string;
        kind: "channel";
        group: string;
        label: string;
        description: string;
        channel: ChatChannelRead;
      }
    | {
        key: string;
        kind: "workingFolder";
        group: string;
        label: string;
        description: string;
        workingFolderId: string;
      }
    | {
        key: string;
        kind: "workspacePath";
        group: string;
        label: string;
        description: string;
        workingFolderId: string;
        path: ProjectWorkingFolderPathRead;
      }
    | {
        key: string;
        kind: "executionEnvironment";
        group: string;
        label: string;
        description: string;
        target: ChatAssignmentTargetRead;
      };

  interface DisclosureBlocker {
    message: string;
    sourceChannelId: string;
    sourceReferenceId: string | null;
  }

  interface ReferenceCandidateGroup {
    label: string;
    entries: Array<{ candidate: ReferenceCandidate; index: number }>;
  }

  const REFERENCE_CLIPBOARD_TYPE = "application/x-ganbaru-chat-references+json";
  const MAX_MESSAGE_BYTES = 131_072;
  const STOPPABLE_ASSIGNMENT_STATES = new Set<ChatWorkAssignmentState>([
    "queued",
    "working",
    "waiting_for_answer",
    "waiting_for_approval",
  ]);

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

  const chat = getChat();
  const projects = getProjects();
  const localization = getLocalization();
  const { t } = localization;
  const localExecutionAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "chat.local-execution",
  );
  const focusEditorOnMount = BUILD_PLATFORM_PROFILE.shell === "desktop";
  const initialDraft = untrack(() => chat.organizationalDraft(destination));
  const initialDocument = parseChatComposerDocument(
    initialDraft.richContent,
    initialDraft.normalizedMarkdown,
  );

  let editorRoot = $state<HTMLDivElement | null>(null);
  let editorController: ChatComposerEditor | null = null;
  let editorDocument = $state<ChatComposerDocument>(initialDocument);
  let normalizedMarkdown = $state(chatComposerMarkdown(initialDocument));
  let editorReferences = $state<ChatMessageReference[]>(chatReferencesForEditor(
    initialDocument,
    initialDraft.references,
  ));
  let attachmentIds = $state([...initialDraft.attachmentIds]);
  let executionTarget = $state<ChatExecutionTarget | null>(initialDraft.executionTarget);
  let selectionStart = $state(initialDraft.selectionStart);
  let selectionEnd = $state(initialDraft.selectionEnd);
  let isBoldActive = $state(false);
  let isItalicActive = $state(false);
  let composing = $state(false);
  let pickerOpen = $state(false);
  let pickerTrigger = $state<ChatReferenceTrigger | null>(null);
  let pickerEntries = $state<ReferenceCandidate[]>([]);
  let pickerIndex = $state(0);
  let pickerLoading = $state(false);
  let pickerRequest = 0;
  let pickerRoot = $state<HTMLDivElement | null>(null);
  let addMenuOpen = $state(false);
  let addMenuAnchor = $state<HTMLDivElement | null>(null);
  let scheduleMenuOpen = $state(false);
  let scheduleMenuAnchor = $state<HTMLDivElement | null>(null);
  let scheduledMessagesOpen = $state(false);
  let scheduledMessagesAnchor = $state<HTMLDivElement | null>(null);
  let ScheduleMenu = $state<ScheduleMenuComponent | null>(null);
  let scheduleMenuLoad = $state<Promise<void> | null>(null);
  let scheduledFor = $state<string | null>(initialDraft.scheduledFor);
  let scheduledMessages = $state<ChatScheduledMessageRead[]>([]);
  let scheduledMessagesRequest = 0;
  let alsoSendToChannel = $state(false);
  let sending = $state(false);
  let stopping = $state(false);
  let retryingAssignment = $state(false);
  let error = $state<string | null>(null);
  let disclosureBlocker = $state<DisclosureBlocker | null>(null);
  let blockerElement = $state<HTMLDivElement | null>(null);
  let announcement = $state("");
  let teammateAccess = $state<ChatTeammateAccessRead | null>(null);
  let assignmentTargets = $state<ChatAssignmentTargetRead[]>([]);
  let assignmentPreviewLoading = $state(false);
  let assignmentPreviewRequest = 0;
  let navigationChannels = $state<ChatChannelRead[]>([]);
  let navigationChannelsLoaded = false;

  const participantReferences = $derived(editorReferences.filter((reference) => (
    reference.kind === "participant"
  )));
  const aiParticipantReferences = $derived(participantReferences.filter((reference) => (
    reference.kind === "participant" && reference.participantKind === "ai_teammate"
  )));
  const invokedAiCount = $derived(new Set(aiParticipantReferences.map((reference) => (
    reference.kind === "participant" ? reference.participantId : ""
  ))).size);
  const mentionedTeammateId = $derived(aiParticipantReferences.find((reference) => (
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
  const failedAssignment = $derived.by(() => {
    const assignment = threadComposer ? chat.replyThread?.assignment : null;
    return assignment?.state === "failed" ? assignment : null;
  });
  const inferredTarget = $derived.by(() => inferAssignmentTarget(
    executionTarget,
    editorReferences,
    assignmentTargets,
  ));
  const secondaryFolderNames = $derived.by(() => {
    const targetFolderId = inferredTarget?.executionTarget?.kind === "workingFolder"
      ? inferredTarget.executionTarget.workingFolderId
      : null;
    const ids = new Set(editorReferences.flatMap((reference) => {
      if (reference.kind !== "workingFolder" && reference.kind !== "workspacePath") return [];
      return reference.workingFolderId === targetFolderId ? [] : [reference.workingFolderId];
    }));
    return [...ids].map((id) => (
      chat.workingFolders.find((folder) => folder.workingFolder.id === id)?.workingFolder.displayName ?? id
    ));
  });
  const providerBlockers = $derived.by(() => assignmentProviderBlockers(
    mentionedTeammateId,
    inferredTarget,
    editorReferences,
    teammateAccess,
    assignmentTargets,
  ));
  const executionTargetOptions = $derived([
    { value: "", label: t("chat.organization.automaticTarget") },
    ...assignmentTargets.flatMap((target) => target.executionTarget ? [{
      value: target.executionTarget.executionEnvironmentId,
      label: `${target.displayName}${target.isDefault ? ` · ${t("chat.organization.defaultTarget")}` : ""}`,
      disabled: !target.eligible,
    }] : []),
  ]);
  const pickerGroups = $derived.by(() => groupReferenceCandidates(pickerEntries));
  const nextScheduledMessage = $derived(
    scheduledMessages.find((message) => message.state !== "failed") ?? scheduledMessages[0] ?? null,
  );
  const stopShortcut = formatShortcut("Mod + .");

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
    const selectionChanged = () => {
      if (document.activeElement === editorRoot) editorController?.handleSelectionChange();
    };
    const stop = () => { void stopAssignment(); };
    document.addEventListener("selectionchange", selectionChanged);
    window.addEventListener("ganbaru-ai:chat-stop-requested", stop);
    if (focusEditorOnMount) {
      editorController.focus({ start: selectionStart, end: selectionEnd });
    }
    return () => {
      document.removeEventListener("selectionchange", selectionChanged);
      window.removeEventListener("ganbaru-ai:chat-stop-requested", stop);
      editorController = null;
    };
  });

  $effect(() => {
    const _version = chat.scheduledMessagesVersion;
    const currentDestination = destination;
    void loadScheduledMessages(currentDestination);
  });

  $effect(() => {
    const teammateId = mentionedTeammateId;
    const channelId = chat.selectedChannel?.id ?? null;
    const replyThreadId = destinationReplyThreadId(destination);
    if (!teammateId || !channelId) {
      assignmentPreviewRequest += 1;
      teammateAccess = null;
      assignmentTargets = [];
      assignmentPreviewLoading = false;
      executionTarget = null;
      return;
    }
    teammateAccess = null;
    assignmentTargets = [];
    void loadAssignmentPreview(teammateId, channelId, replyThreadId);
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
    if (!addMenuOpen && !scheduleMenuOpen && !scheduledMessagesOpen && !pickerOpen) return;
    const closeMenusFromOutside = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node) || isAppFloatingSurfaceTarget(target)) return;
      if (addMenuOpen && !addMenuAnchor?.contains(target)) addMenuOpen = false;
      if (scheduleMenuOpen && !scheduleMenuAnchor?.contains(target)) scheduleMenuOpen = false;
      if (scheduledMessagesOpen && !scheduledMessagesAnchor?.contains(target)) scheduledMessagesOpen = false;
      if (pickerOpen && !editorRoot?.contains(target) && !pickerRoot?.contains(target)) closePicker();
    };
    window.addEventListener("pointerdown", closeMenusFromOutside, true);
    return () => window.removeEventListener("pointerdown", closeMenusFromOutside, true);
  });

  function persist(): void {
    const references = chatReferencesForMarkdown(editorDocument, editorReferences);
    const draft: ChatOrganizationalDraft = {
      normalizedMarkdown,
      richContent: chatComposerDocumentVersioned(editorDocument),
      attachmentIds: [...attachmentIds],
      references,
      executionTarget: executionTarget ? cloneData(executionTarget) : null,
      selectionStart,
      selectionEnd,
      scheduledFor,
    };
    chat.setOrganizationalDraft(destination, draft);
  }

  function handleEditorChange(change: ChatComposerEditorChange): void {
    editorDocument = change.document;
    normalizedMarkdown = change.markdown;
    editorReferences = change.references;
    disclosureBlocker = null;
    persist();
    void updateReferencePicker(change.plainText, editorController?.selection().start ?? change.plainText.length);
  }

  function handleEditorSelectionChange(selection: ChatComposerSelection, marks: ChatComposerMark[]): void {
    selectionStart = selection.start;
    selectionEnd = selection.end;
    isBoldActive = marks.includes("bold");
    isItalicActive = marks.includes("italic");
    persist();
    void updateReferencePicker(editorController?.plainText() ?? "", selection.start);
  }

  function handleBeforeInput(event: InputEvent): void {
    editorController?.handleBeforeInput(event);
  }

  function handleInput(): void {
    editorController?.handleInput();
  }

  function handleCompositionStart(): void {
    composing = true;
    editorController?.handleCompositionStart();
  }

  function handleCompositionEnd(): void {
    composing = false;
    editorController?.handleCompositionEnd();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (composing || event.isComposing || event.keyCode === 229) return;
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
      void post();
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

  function handlePaste(event: ClipboardEvent): void {
    if (!event.clipboardData || !editorController) return;
    event.preventDefault();
    const plainText = event.clipboardData.getData("text/plain").replace(/\r\n?/gu, "\n");
    const semantic = parseReferenceClipboardSlice(
      event.clipboardData.getData(REFERENCE_CLIPBOARD_TYPE),
      plainText,
    );
    if (semantic) {
      editorController.pasteReferenceSlice(semantic);
      announcement = t("chat.organization.referencesPasted", semantic.references.length);
    } else {
      editorController.insertPlainText(plainText);
    }
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

  async function updateReferencePicker(text: string, caret: number): Promise<void> {
    const trigger = chatReferenceTriggerAtCaret(text, caret);
    if (!trigger) {
      closePicker();
      return;
    }
    const request = ++pickerRequest;
    pickerTrigger = trigger;
    pickerOpen = true;
    pickerIndex = 0;
    pickerLoading = true;
    pickerEntries = [];
    try {
      const entries = trigger.kind === "channel"
        ? await channelCandidates(trigger.query)
        : await mentionCandidates(trigger.query);
      if (request !== pickerRequest) return;
      pickerEntries = entries;
      pickerIndex = firstPickerIndex(entries);
      announcement = t("chat.organization.referenceResults", entries.length);
    } catch (cause: unknown) {
      if (request === pickerRequest) error = cause instanceof Error ? cause.message : String(cause);
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
      membership.removedAt === null
        && membership.participant.archivedAt === null
        && (membership.participant.kind !== "ai_teammate"
          || membership.aiAccess?.capabilities.participate === true)
    ));
    const memberIds = new Set(memberships.map((membership) => membership.participant.id));
    const entries: ReferenceCandidate[] = memberships.flatMap((membership) => {
      const teammate = chat.teammates.find((candidate) => (
        candidate.participant.id === membership.participant.id
      )) ?? null;
      if (!matchesQuery(normalized, membership.participant.displayName, teammate?.role ?? "")) return [];
      return [{
        key: `participant:${membership.participant.id}`,
        kind: "participant" as const,
        group: t("chat.organization.channelParticipants"),
        label: membership.participant.displayName,
        description: teammate?.role || t("chat.organization.humanMember"),
        participant: membership.participant,
        teammate,
        needsMembership: false,
      }];
    });
    for (const teammate of chat.teammates) {
      if (memberIds.has(teammate.participant.id)
        || !matchesQuery(normalized, teammate.participant.displayName, teammate.role)) continue;
      entries.push({
        key: `outside-participant:${teammate.participant.id}`,
        kind: "participant",
        group: t("chat.organization.otherTeammates"),
        label: teammate.participant.displayName,
        description: t("chat.organization.addBeforeMentioning"),
        participant: teammate.participant,
        teammate,
        needsMembership: true,
      });
    }
    const projectId = chat.selectedChannel?.projectId;
    const selectedAccess = mentionedTeammateId
      ? teammateAccess?.channels.find((channel) => (
        channel.channelId === chat.selectedChannel?.id && channel.removedAt === null
      )) ?? null
      : null;
    const grantedFolderIds = mentionedTeammateId
      ? new Set(selectedAccess?.folderGrants.flatMap((grant) => (
        grant.revokedAt === null && grant.capability !== "none" ? [grant.workingFolderId] : []
      )) ?? [])
      : null;
    const folders = chat.workingFolders.filter((folder) => (
      folder.workingFolder.projectId === projectId
        && folder.workingFolder.archivedAt === null
        && (grantedFolderIds === null || grantedFolderIds.has(folder.workingFolder.id))
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
    if (candidate.kind === "participant" && candidate.needsMembership) {
      persist();
      closePicker();
      window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-configure-teammate", {
        detail: {
          participantId: candidate.participant.id,
          channelId: chat.selectedChannel?.id,
        },
      }));
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

  function candidateReference(candidate: ReferenceCandidate): ChatMessageReference | null {
    const metadata = {
      referenceId: crypto.randomUUID(),
      labelSnapshot: candidate.label.replace(/^[@#]/u, ""),
      startOffset: 0,
      endOffset: 0,
      plainTextProjection: candidate.kind === "channel"
        ? candidate.label
        : `@${candidate.label.replace(/^@/u, "")}`,
    };
    switch (candidate.kind) {
      case "participant":
        return {
          kind: "participant",
          metadata,
          participantId: candidate.participant.id,
          participantKind: candidate.participant.kind,
        };
      case "channel":
        return { kind: "channel", metadata, channelId: candidate.channel.id };
      case "workingFolder":
        return { kind: "workingFolder", metadata, workingFolderId: candidate.workingFolderId };
      case "workspacePath":
        return {
          kind: "workspacePath",
          metadata,
          workingFolderId: candidate.workingFolderId,
          pathKind: candidate.path.kind === "directory" ? "folder" : "file",
          relativePath: candidate.path.relativePath,
        };
      case "executionEnvironment":
        return candidate.target.executionTarget ? {
          kind: "executionEnvironment",
          metadata,
          executionEnvironmentId: candidate.target.executionTarget.executionEnvironmentId,
        } : null;
    }
  }

  function insertReferenceTrigger(value: "@" | "#"): void {
    closePicker();
    editorController?.insertPlainText(value);
    queueMicrotask(() => editorController?.focus());
  }

  function closePicker(): void {
    pickerRequest += 1;
    pickerOpen = false;
    pickerTrigger = null;
    pickerEntries = [];
    pickerLoading = false;
    pickerIndex = 0;
  }

  async function loadAssignmentPreview(
    teammateId: string,
    channelId: string,
    replyThreadId: string | null,
  ): Promise<void> {
    const requestId = ++assignmentPreviewRequest;
    assignmentPreviewLoading = true;
    try {
      const [access, targets] = await Promise.all([
        chatApi.readChatTeammateAccess(teammateId),
        chatApi.listChatAssignmentTargets({ teammateId, channelId, replyThreadId }),
      ]);
      if (
        requestId !== assignmentPreviewRequest
        || mentionedTeammateId !== teammateId
        || chat.selectedChannel?.id !== channelId
        || destinationReplyThreadId(destination) !== replyThreadId
      ) return;
      teammateAccess = access;
      assignmentTargets = targets;
      if (executionTarget && !targets.some((target) => sameExecutionTarget(target.executionTarget, executionTarget))) {
        executionTarget = null;
        persist();
      }
    } catch (cause: unknown) {
      if (
        requestId === assignmentPreviewRequest
        && mentionedTeammateId === teammateId
        && chat.selectedChannel?.id === channelId
        && destinationReplyThreadId(destination) === replyThreadId
      ) {
        assignmentTargets = [];
        error = cause instanceof Error ? cause.message : String(cause);
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
    const teammateCanRead = aiParticipantReferences.every((reference) => (
      reference.kind !== "participant" || sourceReaders.has(reference.participantId)
    ));
    const missing = rosterReaders(destinationRoster).filter((reader) => !sourceReaders.has(reader.id));
    if (requester && teammateCanRead && missing.length === 0) return null;
    const labels = missing.map((reader) => reader.displayName);
    if (!requester) labels.unshift(t("chat.organization.you"));
    if (!teammateCanRead) {
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
    return roster.memberships.flatMap((membership) => {
      if (membership.removedAt !== null) return [];
      if (membership.participant.kind === "ai_teammate"
        && membership.aiAccess?.capabilities.readHistory !== true) return [];
      return [membership.participant];
    });
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
    window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-manage-members", {
      detail: { channelId },
    }));
  }

  async function pickImages(): Promise<void> {
    const workingFolderId = chat.selectedWorkingFolderId
      ?? chat.workingFolders.find((folder) => folder.workingFolder.projectId === chat.selectedChannel?.projectId)
        ?.workingFolder.id;
    if (!workingFolderId) return;
    addMenuOpen = false;
    const available = Math.max(0, CHAT_IMAGE_LIMIT - attachmentIds.length);
    if (available === 0) return;
    const imported = await chatApi.pickChatImages(
      workingFolderId,
      Array.from({ length: available }, () => crypto.randomUUID()),
      t("chat.composer.attachImages"),
    );
    attachmentIds = [...attachmentIds, ...imported.map((attachment) => attachment.id)];
    persist();
  }

  function clearComposerAfterDelivery(): void {
    editorDocument = chatComposerDocumentFromText("");
    normalizedMarkdown = "";
    editorReferences = [];
    attachmentIds = [];
    executionTarget = null;
    selectionStart = 0;
    selectionEnd = 0;
    alsoSendToChannel = false;
    scheduledFor = null;
    teammateAccess = null;
    assignmentTargets = [];
    disclosureBlocker = null;
    editorController?.setDocument(editorDocument, []);
    persist();
  }

  async function post(): Promise<void> {
    if (activeAssignment || sending || !hasMessageContent) return;
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
        scheduledMessages = [...scheduledMessages.filter((message) => message.id !== scheduled.id), scheduled]
          .sort((left, right) => left.scheduledFor.localeCompare(right.scheduledFor));
      } else {
        await chat.postOrganizationalMessage(destination, {
          alsoSendToChannel: threadComposer && alsoSendToChannel,
        });
        onRequestScrollToBottom();
      }
      clearComposerAfterDelivery();
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      sending = false;
    }
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
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      stopping = false;
    }
  }

  async function retryFailedAssignment(): Promise<void> {
    const assignment = failedAssignment;
    if (!assignment || retryingAssignment) return;
    retryingAssignment = true;
    error = null;
    try {
      await chat.retryAssignment(assignment.id);
      onRequestScrollToBottom();
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
      throw cause;
    } finally {
      retryingAssignment = false;
    }
  }

  function performPrimaryAction(): void {
    if (activeAssignment) void stopAssignment();
    else void post();
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
      .catch((cause: unknown) => { error = cause instanceof Error ? cause.message : String(cause); })
      .finally(() => { scheduleMenuLoad = null; });
    return scheduleMenuLoad;
  }

  function toggleScheduleMenu(): void {
    scheduleMenuOpen = !scheduleMenuOpen;
    scheduledMessagesOpen = false;
    addMenuOpen = false;
    if (scheduleMenuOpen) void loadScheduleMenu();
  }

  function selectSchedule(nextScheduledFor: string): void {
    scheduledFor = nextScheduledFor;
    scheduleMenuOpen = false;
    persist();
    void tick().then(() => editorController?.focus());
  }

  function clearSchedule(): void {
    scheduledFor = null;
    scheduleMenuOpen = false;
    persist();
  }

  function toggleScheduledMessages(): void {
    scheduledMessagesOpen = !scheduledMessagesOpen;
    scheduleMenuOpen = false;
    addMenuOpen = false;
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
    if (messages.length === 0) scheduledMessagesOpen = false;
  }

  function channelAncestry(channel: ChatChannelRead): string {
    const project = projects.projects.find((entry) => entry.id === channel.projectId);
    const group = projects.groups.find((entry) => entry.id === project?.groupId);
    return [group?.name, project?.name].filter(Boolean).join(" / ") || channel.projectId;
  }

  function matchesQuery(query: string, ...values: string[]): boolean {
    return !query || values.some((value) => value.toLocaleLowerCase().includes(query));
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
    if (candidate.kind === "participant") return candidate.needsMembership ? 1 : 0;
    if (candidate.kind === "workingFolder") return 2;
    if (candidate.kind === "workspacePath") return 3;
    return 4;
  }

  function groupReferenceCandidates(candidates: readonly ReferenceCandidate[]): ReferenceCandidateGroup[] {
    const groups: ReferenceCandidateGroup[] = [];
    candidates.forEach((candidate, index) => {
      const group = groups.at(-1);
      if (group?.label === candidate.group) group.entries.push({ candidate, index });
      else groups.push({ label: candidate.group, entries: [{ candidate, index }] });
    });
    return groups;
  }

  function candidateIsDisabled(candidate: ReferenceCandidate): boolean {
    return candidate.kind === "executionEnvironment" && !candidate.target.eligible;
  }

  function firstPickerIndex(candidates: readonly ReferenceCandidate[]): number {
    const index = candidates.findIndex((candidate) => !candidateIsDisabled(candidate));
    return index < 0 ? 0 : index;
  }

  function lastPickerIndex(candidates: readonly ReferenceCandidate[]): number {
    for (let index = candidates.length - 1; index >= 0; index -= 1) {
      if (!candidateIsDisabled(candidates[index]!)) return index;
    }
    return 0;
  }

  function nextPickerIndex(
    candidates: readonly ReferenceCandidate[],
    current: number,
    direction: 1 | -1,
  ): number {
    if (candidates.length === 0) return 0;
    let index = current;
    for (let attempts = 0; attempts < candidates.length; attempts += 1) {
      index = (index + direction + candidates.length) % candidates.length;
      if (!candidateIsDisabled(candidates[index]!)) return index;
    }
    return current;
  }

  function inferAssignmentTarget(
    explicit: ChatExecutionTarget | null,
    references: readonly ChatMessageReference[],
    targets: readonly ChatAssignmentTargetRead[],
  ): ChatAssignmentTargetRead | null {
    if (explicit) {
      const selected = targets.find((target) => sameExecutionTarget(target.executionTarget, explicit));
      if (selected) return selected;
    }
    const referencedEnvironmentIds = new Set(references.flatMap((reference) => (
      reference.kind === "executionEnvironment" ? [reference.executionEnvironmentId] : []
    )));
    if (referencedEnvironmentIds.size === 1) {
      const environmentId = [...referencedEnvironmentIds][0];
      const target = targets.find((candidate) => (
        candidate.executionTarget?.executionEnvironmentId === environmentId && candidate.eligible
      ));
      if (target) return target;
    }
    return targets.find((target) => target.isDefault && target.eligible)
      ?? targets.find((target) => target.kind === "scratch" && target.eligible)
      ?? null;
  }

  function sameExecutionTarget(
    left: ChatExecutionTarget | null,
    right: ChatExecutionTarget | null,
  ): boolean {
    if (!left || !right || left.kind !== right.kind) return false;
    return left.executionEnvironmentId === right.executionEnvironmentId;
  }

  function assignmentProviderBlockers(
    teammateId: string | null,
    target: ChatAssignmentTargetRead | null,
    references: readonly ChatMessageReference[],
    access: ChatTeammateAccessRead | null,
    targets: readonly ChatAssignmentTargetRead[],
  ): string[] {
    if (!teammateId) return [];
    const teammate = chat.teammates.find((candidate) => candidate.participant.id === teammateId);
    const blockers: string[] = [];
    if (assignmentPreviewLoading) blockers.push(t("chat.organization.assignmentStillResolving"));
    if (!teammate?.latestPolicy) blockers.push(t("chat.organization.providerNotConfigured"));
    if (teammate && teammate.configurationState !== "healthy") {
      blockers.push(t("chat.organization.teammateNeedsSetup", teammate.participant.displayName));
    }
    if (!assignmentPreviewLoading && !target) blockers.push(t("chat.organization.noEligibleTarget"));
    if (target && !target.eligible) {
      blockers.push(target.unavailableReason ?? t("chat.organization.targetUnavailable"));
    }
    const channelAccess = access?.channels.find((channel) => (
      channel.channelId === chat.selectedChannel?.id && channel.removedAt === null
    )) ?? null;
    if (!assignmentPreviewLoading && !channelAccess) {
      blockers.push(t("chat.organization.teammateChannelAccessUnavailable"));
    }
    if (channelAccess) {
      const grantedFolderIds = new Set(channelAccess.folderGrants.flatMap((grant) => (
        grant.revokedAt === null && grant.capability !== "none" ? [grant.workingFolderId] : []
      )));
      const referencedFolderIds = new Set(references.flatMap((reference) => (
        reference.kind === "workingFolder" || reference.kind === "workspacePath"
          ? [reference.workingFolderId]
          : []
      )));
      const ungrantedFolders = [...referencedFolderIds]
        .filter((folderId) => !grantedFolderIds.has(folderId))
        .map((folderId) => chat.workingFolders.find((folder) => (
          folder.workingFolder.id === folderId
        ))?.workingFolder.displayName ?? folderId);
      if (ungrantedFolders.length > 0) {
        blockers.push(t(
          "chat.organization.ungrantedFolderReferences",
          new Intl.ListFormat(localization.locale, { style: "long", type: "conjunction" })
            .format(ungrantedFolders),
        ));
      }
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
    const provider = chat.settings?.providerInstances.find((entry) => (
      entry.configuration.instanceId === teammate?.latestPolicy?.providerInstanceId
    ));
    const authority = provider?.lastProbe?.authoritySupport;
    if (teammate?.latestPolicy && !authority && !assignmentPreviewLoading) {
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
    if (target?.kind === "scratch" && authority) {
      if (!authority.writableRoot) blockers.push(t("chat.organization.providerCannotWriteRoot"));
      if (!authority.confinedCommands) blockers.push(t("chat.organization.providerCannotConfineCommands"));
      if (!authority.networkBoundary) blockers.push(t("chat.organization.providerCannotEnforceNetwork"));
    }
    if (target?.folderCapability && authority) {
      const capability = target.folderCapability;
      if (capability === "read" && !authority.readOnlyRoot) blockers.push(t("chat.organization.providerCannotReadRoot"));
      if (["edit", "execute", "publish"].includes(capability) && !authority.writableRoot) {
        blockers.push(t("chat.organization.providerCannotWriteRoot"));
      }
      if (["read", "edit"].includes(capability) && !authority.denyShell) {
        blockers.push(t("chat.organization.providerCannotDenyShell"));
      }
      if (["execute", "publish"].includes(capability) && !authority.confinedCommands) {
        blockers.push(t("chat.organization.providerCannotConfineCommands"));
      }
      if (["execute", "publish"].includes(capability) && !authority.networkBoundary) {
        blockers.push(t("chat.organization.providerCannotEnforceNetwork"));
      }
      if (capability === "publish" && !authority.classifiedPublish) {
        blockers.push(t("chat.organization.providerCannotPublish"));
      }
    }
    return [...new Set(blockers)];
  }

  function destinationReplyThreadId(value: string): string | null {
    return value.startsWith("reply-thread:") ? value.slice("reply-thread:".length) : null;
  }

  function runtimeApprovalLabel(policy: ChatAssignmentTargetRead["effectiveRuntimeApproval"]): string {
    switch (policy) {
      case "ask": return t("settings.chat.teammates.runtime.ask");
      case "autoApprove": return t("settings.chat.teammates.runtime.autoApprove");
      case "unattended": return t("settings.chat.teammates.runtime.unattended");
      case "providerCustom": return t("settings.chat.teammates.runtime.providerCustom");
    }
  }
</script>

<div class="composer-stack">
  <span class="sr-only" aria-live="polite">{announcement}</span>
  {#if failedAssignment}
    <Notice class="assignment-notice" role="status" message={t("chat.organization.workStopped")} retryLabel={t("chat.organization.retry")} onRetry={retryFailedAssignment} copy={false} />
  {/if}
  {#if nextScheduledMessage}
    <div class="scheduled-summary">
      <Clock3 size={13} />
      <span>{t("chat.organization.scheduledMessageSummary", scheduledMessages.length, formatScheduledInstant(nextScheduledMessage.scheduledFor))}</span>
      <div bind:this={scheduledMessagesAnchor} class="scheduled-summary-anchor">
        <button type="button" aria-haspopup="dialog" aria-expanded={scheduledMessagesOpen} onpointerenter={() => { void loadScheduleMenu(); }} onfocus={() => { void loadScheduleMenu(); }} onclick={toggleScheduledMessages}>{t("chat.organization.viewScheduledMessages", scheduledMessages.length)}</button>
        {#if scheduledMessagesOpen}
          {#if ScheduleMenu}
            {@const LoadedScheduledMessagesMenu = ScheduleMenu}
            <LoadedScheduledMessagesMenu mode="manage" align="right" {scheduledMessages} onMessagesChange={updateScheduledMessages} onClose={() => { scheduledMessagesOpen = false; }} />
          {:else}<div class="composer-menu schedule-loading align-right surface-floating surface-floating-body"><Loading layout="fill" /></div>{/if}
        {/if}
      </div>
    </div>
  {/if}

  {#if mentionedTeammateId}
    <section class="assignment-preview" aria-label={t("chat.organization.assignmentPreview")}>
      <Bot size={13} />
      <div>
        <strong>{#if assignmentPreviewLoading}<Loading layout="inline" label={t("chat.organization.resolvingAssignment")} />{:else}{inferredTarget?.displayName ?? t("chat.organization.privateScratch")}{/if}</strong>
        <span>{secondaryFolderNames.length ? t("chat.organization.secondaryFolders", secondaryFolderNames.join(", ")) : t("chat.organization.noSecondaryFolders")}</span>
      </div>
      {#if inferredTarget}<span class="approval-pill">{runtimeApprovalLabel(inferredTarget.effectiveRuntimeApproval)}</span>{/if}
      {#if assignmentTargets.length > 0}
        <Select
          inline
          class="assignment-target-select"
          ariaLabel={t("chat.organization.executionTarget")}
          value={executionTarget?.executionEnvironmentId ?? ""}
          options={executionTargetOptions}
          onChange={(value) => {
            const selected = assignmentTargets.find((target) => target.executionTarget?.executionEnvironmentId === value);
            executionTarget = selected?.executionTarget ? cloneData(selected.executionTarget) : null;
            persist();
          }}
        />
      {/if}
    </section>
  {/if}

  <div class="organizational-composer" data-organizational-composer>
    <div class="editor-frame">
      <div
        bind:this={editorRoot}
        class="organizational-editor"
        class:empty={!chatComposerPlainText(editorDocument)}
        contenteditable="true"
        role="combobox"
        tabindex="0"
        aria-label={placeholder}
        aria-autocomplete="list"
        aria-haspopup="listbox"
        aria-expanded={pickerOpen}
        aria-controls={pickerOpen ? "chat-reference-picker" : undefined}
        aria-activedescendant={pickerOpen && pickerEntries[pickerIndex] ? `chat-reference-${pickerIndex}` : undefined}
        data-placeholder={placeholder}
        data-chat-composer
        onbeforeinput={handleBeforeInput}
        oninput={handleInput}
        onkeydown={handleKeydown}
        oncopy={handleCopy}
        onpaste={handlePaste}
        oncompositionstart={handleCompositionStart}
        oncompositionend={handleCompositionEnd}
      ></div>
    </div>

    {#if pickerOpen}
      <div bind:this={pickerRoot} id="chat-reference-picker" class="reference-picker surface-floating" role="listbox" aria-label={pickerTrigger?.kind === "channel" ? t("chat.organization.referenceChannel") : t("chat.organization.referencePeopleAndResources")}>
        <div class="reference-picker-scroll surface-floating-body" use:scrollEdgeFadeAction>
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
                      {#if candidate.kind === "participant"}<ChatParticipantAvatar participant={candidate.participant} teammate={candidate.teammate ?? undefined} size={28} />
                      {:else if candidate.kind === "channel"}<Hash size={15} />
                      {:else if candidate.kind === "workingFolder"}<Folder size={15} />
                      {:else if candidate.kind === "workspacePath"}{#if candidate.path.kind === "directory"}<Folder size={15} />{:else}<File size={15} />{/if}
                      {:else}<GitBranch size={15} />{/if}
                    </span>
                    <span class="candidate-copy"><strong>{candidate.label}</strong><small>{candidate.description}</small></span>
                    {#if candidate.kind === "participant" && candidate.needsMembership}<Plus size={13} />{/if}
                  </button>
                {/each}
              </div>
            {/each}
          {/if}
        </div>
      </div>
    {/if}

    {#if attachmentIds.length > 0}
      <div class="context-chips">
        {#each attachmentIds as id, index (id)}<button type="button" onclick={() => { attachmentIds = attachmentIds.filter((entry) => entry !== id); persist(); }}><Image size={12} />{t("chat.organization.imageNumber", index + 1)}<span>×</span></button>{/each}
      </div>
    {/if}

    <div class="composer-footer">
      <div class="composer-tools">
        <div bind:this={addMenuAnchor} class="menu-anchor">
          <button type="button" class="tool-button" aria-label={t("chat.organization.addContext")} aria-expanded={addMenuOpen} onclick={() => { addMenuOpen = !addMenuOpen; scheduleMenuOpen = false; scheduledMessagesOpen = false; }}><Plus size={16} /></button>
          {#if addMenuOpen}<div class="composer-menu add-menu surface-floating surface-floating-body w-floating-sm">{#if localExecutionAvailable}<button type="button" class="menu-item" onclick={() => void pickImages()}><Image size={14} />{t("chat.composer.attachImages")}</button>{/if}<button type="button" class="menu-item" onclick={() => { addMenuOpen = false; insertReferenceTrigger("@"); }}><AtSign size={14} />{t("chat.organization.peopleAndResources")}</button><button type="button" class="menu-item" onclick={() => { addMenuOpen = false; insertReferenceTrigger("#"); }}><Hash size={14} />{t("chat.organization.channels")}</button></div>{/if}
        </div>
        <button type="button" class="tool-button" class:active={isBoldActive} aria-label={t("chat.organization.bold")} aria-pressed={isBoldActive} onclick={() => editorController?.toggleMark("bold")}><Bold size={15} /></button>
        <button type="button" class="tool-button" class:active={isItalicActive} aria-label={t("chat.organization.italic")} aria-pressed={isItalicActive} onclick={() => editorController?.toggleMark("italic")}><Italic size={15} /></button>
        <button type="button" class="tool-button" aria-label={t("chat.organization.peopleAndResources")} onpointerdown={(event) => event.preventDefault()} onclick={() => insertReferenceTrigger("@")}><AtSign size={15} /></button>
        <button type="button" class="tool-button" aria-label={t("chat.organization.channels")} onpointerdown={(event) => event.preventDefault()} onclick={() => insertReferenceTrigger("#")}><Hash size={15} /></button>
        <div bind:this={scheduleMenuAnchor} class="menu-anchor">
          <button type="button" class="tool-button" class:active={scheduleMenuOpen || Boolean(scheduledFor)} aria-label={scheduledFor ? t("chat.organization.scheduleSelected", formatScheduledInstant(scheduledFor)) : t("chat.organization.scheduleMessage")} aria-haspopup="dialog" aria-expanded={scheduleMenuOpen} aria-pressed={Boolean(scheduledFor)} title={scheduledFor ? t("chat.organization.scheduleSelected", formatScheduledInstant(scheduledFor)) : t("chat.organization.scheduleMessage")} onpointerenter={() => { void loadScheduleMenu(); }} onfocus={() => { void loadScheduleMenu(); }} onclick={toggleScheduleMenu}><Clock3 size={15} /></button>
          {#if scheduleMenuOpen}
            {#if ScheduleMenu}
              {@const LoadedScheduleMenu = ScheduleMenu}
              <LoadedScheduleMenu mode="choose" selectedScheduledFor={scheduledFor} disabled={sending} onSelect={selectSchedule} onClear={clearSchedule} onClose={() => { scheduleMenuOpen = false; }} />
            {:else}<div class="composer-menu schedule-loading surface-floating surface-floating-body"><Loading layout="fill" /></div>{/if}
          {/if}
        </div>
        {#if threadComposer}<button type="button" class="tool-button" class:active={alsoSendToChannel} aria-label={t("chat.organization.shareReplyToChannel", channelName)} aria-pressed={alsoSendToChannel} title={t("chat.organization.shareReplyToChannel", channelName)} onclick={() => { alsoSendToChannel = !alsoSendToChannel; }}><MessageSquareShare size={15} /></button>{/if}
      </div>
      <button type="button" class="send-button" data-action={activeAssignment ? "stop" : "send"} disabled={sending || (!activeAssignment && !hasMessageContent)} aria-disabled={!activeAssignment && Boolean(validation || disclosureBlocker || providerBlockers.length || messageTooLarge)} aria-label={activeAssignment ? t("chat.organization.stopWorkShortcut", stopShortcut) : scheduledFor ? t("chat.organization.scheduleMessage") : t("chat.composer.send")} aria-busy={stopping || undefined} title={activeAssignment ? t("chat.organization.stopWorkShortcut", stopShortcut) : undefined} onclick={performPrimaryAction}>{#if activeAssignment}<Square size={13} />{:else}<ArrowUp size={16} />{/if}</button>
    </div>

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
  </div>
</div>

<style>
  .composer-stack { width:min(100%,var(--chat-conversation-max-width,60rem)); }
  .scheduled-summary { display:flex; min-height:2rem; align-items:center; gap:0.45rem; margin:0 0.35rem 0.35rem; border-radius:0.55rem; padding:0.25rem 0.35rem 0.25rem 0.55rem; }
  .composer-stack :global(.assignment-notice) { margin:0 0.35rem 0.35rem; padding:0.25rem 0.35rem 0.25rem 0.55rem; font-size:calc(0.7rem * var(--type-scale)); }
  .scheduled-summary > span { min-width:0; flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:calc(0.7rem * var(--type-scale)); }
  .scheduled-summary button { min-height:1.75rem; border-radius:0.4rem; padding:0.2rem 0.45rem; font-size:calc(0.68rem * var(--type-scale)); font-weight:500; }
  .scheduled-summary { background:color-mix(in srgb,var(--accent) 58%,transparent); color:var(--muted-foreground); }
  .scheduled-summary-anchor,.menu-anchor { position:relative; }
  .assignment-preview { display:grid; min-height:2.45rem; grid-template-columns:auto minmax(0,1fr) auto auto; align-items:center; gap:0.5rem; margin:0 0.4rem 0.4rem; border:1px solid color-mix(in srgb,var(--border) 68%,transparent); border-radius:0.65rem; background:color-mix(in srgb,var(--card) 80%,transparent); padding:0.35rem 0.55rem; color:var(--muted-foreground); }
  .assignment-preview > div { display:grid; min-width:0; }
  .assignment-preview strong,.assignment-preview span { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .assignment-preview strong { color:var(--foreground); font-size:calc(0.68rem * var(--type-scale)); }.assignment-preview span { font-size:calc(0.6rem * var(--type-scale)); }
  .assignment-preview :global(.assignment-target-select) { width:auto; max-width:12rem; }
  .approval-pill { border-radius:999px; background:var(--accent); padding:0.18rem 0.4rem; color:var(--foreground); }
  .organizational-composer { position:relative; width:100%; border:1px solid color-mix(in srgb,var(--border) 88%,transparent); border-radius:1.3rem; background:var(--card); box-shadow:0 8px 22px -18px rgb(0 0 0 / 0.24),0 1px 4px -3px rgb(0 0 0 / 0.16); }
  .editor-frame { max-height:8.2rem; overflow-y:auto; padding:1rem 1.25rem 0.4rem; }
  .organizational-editor { position:relative; min-height:2.625rem; outline:none; white-space:pre-wrap; overflow-wrap:anywhere; font-size:var(--chat-conversation-font-size,calc(0.875rem * var(--type-scale))); line-height:var(--chat-conversation-line-height,calc(1.3125rem * var(--type-scale))); }
  .organizational-editor.empty::before { position:absolute; pointer-events:none; content:attr(data-placeholder); color:color-mix(in srgb,var(--muted-foreground) 52%,transparent); }
  .organizational-editor :global(.chat-reference-atom) { display:inline-block; border-radius:0.3rem; background:color-mix(in srgb,var(--primary) 13%,transparent); padding-inline:0.12rem; color:color-mix(in srgb,var(--primary) 78%,var(--foreground)); box-decoration-break:clone; cursor:default; user-select:all; }
  .reference-picker { position:absolute; z-index:100; right:0.6rem; bottom:3rem; left:0.6rem; display:flex; max-height:min(22rem,55vh); flex-direction:column; overflow:hidden; }
  .reference-picker-scroll { min-height:0; flex:1 1 auto; overflow-y:auto; }
  .reference-picker-scroll > p { padding:0.5rem; color:var(--muted-foreground); font-size:var(--panel-detail-font-size); }
  .picker-group { position:sticky; top:calc(-1 * var(--floating-padding)); z-index:1; background:var(--popover); }
  .candidate-icon { display:grid; width:2rem; flex:0 0 auto; place-items:center; color:var(--muted-foreground); }
  .candidate-copy { display:grid; min-width:0; flex:1 1 0%; }
  .reference-picker strong,.reference-picker small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }.reference-picker strong { font-weight:500; }.reference-picker small { color:var(--muted-foreground); font-size:var(--panel-detail-font-size); }
  .context-chips { display:flex; flex-wrap:wrap; gap:0.3rem; padding:0 0.75rem 0.35rem; }.context-chips button { display:flex; max-width:14rem; align-items:center; gap:0.25rem; border-radius:999px; background:var(--accent); padding:0.22rem 0.45rem; font-size:calc(0.68rem * var(--type-scale)); }.context-chips button span { color:var(--muted-foreground); }
  .composer-footer { display:flex; min-height:2.6rem; align-items:center; justify-content:space-between; gap:0.5rem; padding:0 0.75rem 0.5rem; }.composer-tools { display:flex; align-items:center; gap:0.3rem; }
  .tool-button { display:grid; width:1.9rem; height:1.9rem; place-items:center; border-radius:0.5rem; color:var(--muted-foreground); }.tool-button:hover,.tool-button.active { background:var(--accent); color:var(--foreground); }
  .send-button { display:grid; width:2rem; height:2rem; flex:0 0 auto; place-items:center; border-radius:999px; background:color-mix(in srgb,var(--primary) 92%,transparent); color:var(--primary-foreground); box-shadow:0 2px 7px color-mix(in srgb,var(--primary) 22%,transparent); transition:transform 120ms ease,filter 120ms ease; }.send-button:hover:not(:disabled) { filter:brightness(1.04); transform:scale(1.04); }.send-button:disabled,.send-button[aria-disabled="true"] { opacity:0.45; }
  .composer-menu { position:absolute; z-index:110; max-width:calc(100vw - 1rem); }.add-menu,.schedule-loading { bottom:calc(100% + 0.35rem); left:0; }.schedule-loading.align-right { right:0; left:auto; }
  .composer-blocker { display:flex; gap:0.45rem; margin:0 0.65rem 0.5rem; border-radius:0.5rem; background:color-mix(in srgb,var(--destructive) 8%,transparent); padding:0.45rem 0.55rem; color:var(--destructive); outline:none; }.composer-blocker:focus-visible { outline:2px solid var(--ring); }.composer-blocker > div { display:grid; gap:0.2rem; }.composer-blocker p { font-size:calc(0.68rem * var(--type-scale)); }.composer-blocker span { display:flex; flex-wrap:wrap; gap:0.25rem; }.composer-blocker button { min-height:1.75rem; border-radius:0.35rem; padding:0.2rem 0.4rem; font-size:calc(0.62rem * var(--type-scale)); font-weight:600; }.composer-blocker button:hover { background:color-mix(in srgb,var(--destructive) 10%,transparent); }
  .composer-stack :global(.composer-error) { padding:0 0.75rem 0.5rem; font-size:calc(0.7rem * var(--type-scale)); }
  .sr-only { position:absolute; width:1px; height:1px; overflow:hidden; clip:rect(0,0,0,0); white-space:nowrap; }
  @media (max-width:42rem) { .assignment-preview { grid-template-columns:auto minmax(0,1fr); }.assignment-preview :global(.assignment-target-select),.approval-pill { grid-column:2; }.reference-picker { right:0.3rem; left:0.3rem; }.composer-tools { gap:0.05rem; } }
  @media (pointer:coarse) { .tool-button,.send-button { min-width:2.75rem; min-height:2.75rem; } }
  @media (prefers-reduced-motion:reduce) { .send-button { transition:none; } }
  @media (forced-colors:active) { .organizational-composer { border:1px solid CanvasText; } }
</style>
