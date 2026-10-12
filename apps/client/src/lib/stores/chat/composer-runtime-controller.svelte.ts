import * as chatApi from "$lib/api/chat";
import {
  ChatComposerController,
  parseDraftMentions,
  type ChatComposerSeed,
  type ChatComposerSnapshot,
} from "$lib/chat/composer/controller";
import {
  CHAT_IMAGE_LIMIT,
  composerModelSelection,
  queuedFollowupDispatchReady,
  readComposerModelSelection,
} from "$lib/chat/composer/model";
import type {
  ApprovalDecision,
  ChatAttachmentRead,
  ChatDraftMention,
  ChatInteractionStateRead,
  ChatQueuedFollowupRead,
  ChatSettingsRead,
  ChatThreadId,
  ChatThreadShellRead,
  InteractionMode,
  ModelId,
  ProjectWorkingFolderId,
  ProviderInstanceId,
  SafetyMode,
  UserInputAnswer,
  VersionedJson,
} from "$lib/chat/contracts";
import { chatErrorMessage } from "$lib/chat/error-presentation";
import type { TimelineMessageRow } from "$lib/chat/timeline/model";

export interface ChatComposerSendOptions {
  promptOverride?: string;
  omitComposerContext?: boolean;
}

export interface ChatComposerRuntimeControllerOptions {
  selectedThread: () => ChatThreadShellRead | null;
  selectedThreadId: () => ChatThreadId | null;
  selectedWorkingFolderId: () => ProjectWorkingFolderId | null;
  selectedExecutionEnvironmentId: () => string | null;
  setSelectedThreadId: (threadId: ChatThreadId | null) => void;
  upsertThread: (thread: ChatThreadShellRead) => void;
  loadTimeline: (threadId: ChatThreadId) => Promise<void>;
  clearTimeline: () => void;
  setSettings: (settings: ChatSettingsRead) => void;
  onComposerChanged: () => void;
}

/** Owns provider-thread composer persistence, sending, interactions, and attachments. */
export class ChatComposerRuntimeController {
  private readonly composerController = new ChatComposerController();

  composer = $state<ChatComposerSnapshot>(this.composerController.snapshot());
  attachments = $state<ChatAttachmentRead[]>([]);
  interaction = $state<ChatInteractionStateRead | null>(null);
  interactionLoading = $state(false);
  sendError = $state<string | null>(null);
  pendingUserMessage = $state<{ threadId: ChatThreadId; row: TimelineMessageRow } | null>(null);
  draftWorkingFolderId = $state<ProjectWorkingFolderId | null>(null);
  draftThreadId = $state<ChatThreadId | null>(null);

  private attachmentRequest = 0;
  private interactionRequest = 0;
  private attachmentKey = "";
  private failedSendOptions: ChatComposerSendOptions | null = null;
  private readonly queuedDispatches = new Set<string>();

  constructor(private readonly options: ChatComposerRuntimeControllerOptions) {
    this.composerController.subscribe((snapshot) => {
      this.composer = snapshot;
      const key = `${snapshot.workingFolderId ?? ""}:${snapshot.attachmentIds.join("\0")}`;
      if (key !== this.attachmentKey) {
        this.attachmentKey = key;
        void this.loadAttachments(snapshot);
      }
      this.options.onComposerChanged();
    });
  }

  reset(): void {
    this.attachmentRequest += 1;
    this.interactionRequest += 1;
    this.queuedDispatches.clear();
    this.attachments = [];
    this.interaction = null;
    this.interactionLoading = false;
    this.sendError = null;
    this.pendingUserMessage = null;
    this.draftWorkingFolderId = null;
    this.draftThreadId = null;
    this.failedSendOptions = null;
    this.attachmentKey = "";
    this.composerController.reset();
  }

  bind(
    workingFolderId: ProjectWorkingFolderId,
    threadId: ChatThreadId | null,
    seed: ChatComposerSeed | null = null,
  ): Promise<void> {
    return this.composerController.bind(workingFolderId, threadId, seed);
  }

  ensureDraftThread(workingFolderId: ProjectWorkingFolderId): ChatThreadId {
    if (this.draftWorkingFolderId !== workingFolderId || !this.draftThreadId) {
      this.draftWorkingFolderId = workingFolderId;
      this.draftThreadId = crypto.randomUUID();
    }
    return this.draftThreadId;
  }

  discardDraft(): void {
    this.draftWorkingFolderId = null;
    this.draftThreadId = null;
  }

  setText(text: string): void {
    this.composerController.setText(text);
  }

  setRichContent(text: string, richContent: VersionedJson): void {
    this.composerController.setRichContent(text, richContent);
  }

  setAttachments(attachmentIds: string[]): void {
    this.composerController.setAttachments(attachmentIds);
  }

  setMentions(mentions: ChatDraftMention[]): void {
    this.composerController.setMentions(mentions);
  }

  setProvider(instanceId: ProviderInstanceId | null): void {
    this.composerController.setProvider(instanceId);
  }

  setModel(selection: VersionedJson | null): void {
    this.composerController.setModelSelection(selection);
  }

  setModes(safety: SafetyMode | null, interaction: InteractionMode | null): void {
    this.composerController.setModes(safety, interaction);
  }

  markSent(): void {
    this.composerController.markSent();
  }

  flush(): Promise<void> {
    return this.composerController.flush();
  }

  async refreshInteraction(threadId = this.options.selectedThreadId()): Promise<void> {
    const request = ++this.interactionRequest;
    if (!threadId) {
      this.interaction = null;
      return;
    }
    this.interactionLoading = true;
    try {
      const interaction = await chatApi.readChatInteractionState(threadId);
      if (request !== this.interactionRequest || threadId !== this.options.selectedThreadId()) return;
      this.interaction = interaction;
      const latestTurnState = this.options.selectedThread()?.latestTurnState ?? null;
      if (queuedFollowupDispatchReady(interaction.sessionState, latestTurnState)
        && interaction.queuedFollowup) {
        void this.dispatchQueuedFollowup(interaction.queuedFollowup).catch((error: unknown) => {
          this.sendError = chatErrorMessage(error);
        });
      }
    } finally {
      if (request === this.interactionRequest) this.interactionLoading = false;
    }
  }

  async send(options: ChatComposerSendOptions = {}): Promise<void> {
    const workingFolderId = this.composer.workingFolderId;
    const providerInstanceId = this.composer.providerInstanceId;
    const safetyMode = this.composer.safetyMode;
    const interactionMode = this.composer.interactionMode;
    const model = readComposerModelSelection(this.composer.modelSelection);
    if (!workingFolderId || !providerInstanceId || !safetyMode || !interactionMode) {
      throw new Error("Complete every Chat composer selection before sending");
    }
    const prompt = options.promptOverride ?? this.composer.text;
    const attachmentIds = options.omitComposerContext ? [] : [...this.composer.attachmentIds];
    const mentions = options.omitComposerContext
      ? []
      : this.composer.mentions.map((mention) => ({
          relativePath: mention.relativePath,
          kind: mention.kind,
        }));
    this.sendError = null;
    this.failedSendOptions = null;
    const selectedCurrent = this.options.selectedThread();
    const current = selectedCurrent && !threadMatchesExecutionConfiguration(
      selectedCurrent,
      workingFolderId,
      providerInstanceId,
      model.modelId,
      model.providerManaged,
      model.options,
    )
      ? null
      : selectedCurrent;
    const newThreadId = current ? null : this.ensureDraftThread(workingFolderId);
    const threadId = current?.id ?? newThreadId;
    if (!threadId) throw new Error("A Chat thread ID is required before sending");
    const turnId = crypto.randomUUID();
    const messageId = crypto.randomUUID();
    this.pendingUserMessage = {
      threadId,
      row: {
        id: messageId,
        kind: "message",
        role: "user",
        turnId,
        sequence: Number.MAX_SAFE_INTEGER,
        createdAt: new Date().toISOString(),
        markdown: prompt,
        state: "pending",
        phase: null,
        userContext: {
          attachments: this.attachments
            .filter((attachment) => attachmentIds.includes(attachment.id))
            .map((attachment) => ({
              attachmentId: attachment.id,
              displayName: attachment.originalDisplayName,
              kind: attachment.kind,
              byteSize: attachment.byteSize,
              status: "pending",
            })),
          mentions: mentions.map((mention) => ({ ...mention })),
          terminalContext: [],
          preCheckpointId: null,
        },
        metadata: null,
        sourceThreadId: threadId,
      },
    };
    this.composerController.markSent();
    let result: Awaited<ReturnType<typeof chatApi.sendChatTurn>>;
    try {
      if (mentions.length > 0) {
        await chatApi.validateChatWorkingFolderMentions(
          workingFolderId,
          mentions.map((mention) => mention.relativePath),
          this.options.selectedExecutionEnvironmentId(),
        );
      }
      await this.composerController.flush();
      result = await chatApi.sendChatTurn({
        command: {
          clientCommandId: crypto.randomUUID(),
          expectedThreadRevision: current?.revision ?? null,
        },
        workingFolderId,
        threadId: current?.id ?? null,
        newThreadId,
        executionEnvironmentId: current ? null : this.options.selectedExecutionEnvironmentId(),
        turnId,
        messageId,
        providerInstanceId,
        providerManagedModel: model.providerManaged,
        modelId: model.modelId,
        modelOptions: model.options,
        modes: { safetyMode, interactionMode },
        prompt,
        attachmentIds,
        mentions,
      });
    } catch (error: unknown) {
      this.pendingUserMessage = null;
      this.composerController.restoreSentSnapshot();
      await this.composerController.flush();
      throw error;
    }
    this.options.upsertThread(result.thread);
    if (!current) this.discardDraft();
    this.options.setSelectedThreadId(result.thread.id);
    this.sendError = result.launchError?.message ?? null;
    this.failedSendOptions = result.launchError ? { ...options } : null;
    try {
      await this.options.loadTimeline(result.thread.id);
    } finally {
      this.pendingUserMessage = null;
    }
    // The turn is already sent, so preference bookkeeping must not report a send failure.
    try {
      await chatApi.rememberChatComposerSelection({
        workingFolderId,
        providerInstanceId,
        modelId: model.modelId,
        providerManagedModel: model.providerManaged,
        modelOptions: model.options,
        safetyMode,
        interactionMode,
      });
      this.options.setSettings(await chatApi.readChatSettings());
    } catch (error: unknown) {
      console.error("Chat composer selection could not be remembered after sending", error);
    }
    await this.refreshInteraction(result.thread.id);
  }

  async retryFailedSend(): Promise<void> {
    const options = this.failedSendOptions ?? {};
    if (!this.composerController.restoreSentSnapshot()) return;
    await this.send(options);
  }

  async editFailedSend(): Promise<void> {
    if (!this.composerController.restoreSentSnapshot()) return;
    await this.forkCurrent(null);
    this.sendError = null;
    this.failedSendOptions = null;
  }

  async changeProviderAfterFailure(): Promise<void> {
    this.composerController.restoreSentSnapshot();
    await this.forkCurrent(null);
    this.composerController.setProvider(null);
    this.composerController.setModelSelection(null);
    this.composerController.setModes(null, null);
    this.sendError = null;
    this.failedSendOptions = null;
  }

  forkWithProvider(providerInstanceId: ProviderInstanceId): Promise<void> {
    return this.forkCurrent(providerInstanceId);
  }

  async steer(): Promise<void> {
    const thread = this.options.selectedThread();
    const prompt = this.composer.text.trim();
    if (!thread || !prompt) return;
    await chatApi.steerChatTurn({
      command: { clientCommandId: crypto.randomUUID(), expectedThreadRevision: thread.revision },
      threadId: thread.id,
      messageId: crypto.randomUUID(),
      prompt,
    });
    this.composerController.markSent();
    await this.composerController.flush();
    await this.options.loadTimeline(thread.id);
  }

  async queue(): Promise<void> {
    const thread = this.options.selectedThread();
    const prompt = this.composer.text;
    if (!thread || !prompt.trim() || !this.composer.providerInstanceId
      || !this.composer.modelSelection || !this.composer.safetyMode
      || !this.composer.interactionMode) return;
    await chatApi.saveChatQueuedFollowup({
      id: this.interaction?.queuedFollowup?.id ?? crypto.randomUUID(),
      threadId: thread.id,
      text: prompt,
      providerInstanceId: this.composer.providerInstanceId,
      modelSelection: this.composer.modelSelection,
      safetyMode: this.composer.safetyMode,
      interactionMode: this.composer.interactionMode,
      attachmentIds: [...this.composer.attachmentIds],
      mentions: {
        schemaVersion: 1,
        value: this.composer.mentions.map((mention) => ({
          relativePath: mention.relativePath,
          kind: mention.kind,
          ignored: mention.ignored,
        })),
      },
    });
    this.composerController.markSent();
    await this.composerController.flush();
    await this.refreshInteraction(thread.id);
  }

  async cancelQueuedFollowup(): Promise<void> {
    const threadId = this.options.selectedThreadId();
    if (!threadId) return;
    await chatApi.cancelChatQueuedFollowup(threadId);
    await this.refreshInteraction(threadId);
  }

  async editQueuedFollowup(): Promise<void> {
    const queued = this.interaction?.queuedFollowup;
    if (!queued) return;
    this.composerController.setText(queued.text);
    this.composerController.setAttachments(queued.attachmentIds);
    this.composerController.setMentions(parseDraftMentions(queued.mentions));
    this.composerController.setProvider(queued.providerInstanceId);
    this.composerController.setModelSelection(queued.modelSelection);
    this.composerController.setModes(queued.safetyMode, queued.interactionMode);
    await this.composerController.flush();
    await this.cancelQueuedFollowup();
  }

  async stop(force = false): Promise<void> {
    const threadId = this.options.selectedThreadId();
    if (!threadId) return;
    await chatApi.stopChatSession(threadId, force);
    await this.refreshInteraction(threadId);
  }

  async resolveApproval(decision: ApprovalDecision): Promise<void> {
    const pending = this.interaction?.pendingRequest;
    const thread = this.options.selectedThread();
    if (!pending || !thread) return;
    await chatApi.resolveChatApproval({
      command: { clientCommandId: crypto.randomUUID(), expectedThreadRevision: null },
      threadId: thread.id,
      requestId: pending.id,
      providerRequestId: pending.providerRequestId,
      decision,
    });
    await this.refreshInteraction(thread.id);
  }

  async resolveUserInput(answers: UserInputAnswer[]): Promise<void> {
    const pending = this.interaction?.pendingRequest;
    const thread = this.options.selectedThread();
    if (!pending || !thread) return;
    await chatApi.resolveChatUserInput({
      command: { clientCommandId: crypto.randomUUID(), expectedThreadRevision: null },
      threadId: thread.id,
      requestId: pending.id,
      providerRequestId: pending.providerRequestId,
      answers,
    });
    await this.refreshInteraction(thread.id);
  }

  async importImages(files: File[]): Promise<void> {
    const workingFolderId = this.composer.workingFolderId;
    if (!workingFolderId) throw new Error("Choose a project working folder before attaching images");
    if (this.attachments.length + files.length > CHAT_IMAGE_LIMIT) {
      throw new Error(`Attach up to ${CHAT_IMAGE_LIMIT} Chat images`);
    }
    for (const file of files) {
      const attachment = await chatApi.importChatImage(
        workingFolderId,
        crypto.randomUUID(),
        file.name,
        [...new Uint8Array(await file.arrayBuffer())],
      );
      this.attachments = [...this.attachments, attachment];
      this.composerController.setAttachments(this.attachments.map((entry) => entry.id));
    }
  }

  async pickImages(title: string): Promise<void> {
    const workingFolderId = this.composer.workingFolderId;
    if (!workingFolderId) throw new Error("Choose a project working folder before attaching images");
    const available = Math.max(0, CHAT_IMAGE_LIMIT - this.attachments.length);
    if (available === 0) throw new Error(`Attach up to ${CHAT_IMAGE_LIMIT} Chat images`);
    const imported = await chatApi.pickChatImages(
      workingFolderId,
      Array.from({ length: available }, () => crypto.randomUUID()),
      title,
    );
    this.attachments = [...this.attachments, ...imported];
    this.composerController.setAttachments(this.attachments.map((attachment) => attachment.id));
  }

  removeAttachment(attachmentId: string): void {
    this.attachments = this.attachments.filter((attachment) => attachment.id !== attachmentId);
    this.composerController.setAttachments(this.attachments.map((attachment) => attachment.id));
  }

  private async loadAttachments(snapshot: ChatComposerSnapshot): Promise<void> {
    const request = ++this.attachmentRequest;
    if (!snapshot.workingFolderId || snapshot.attachmentIds.length === 0) {
      this.attachments = [];
      return;
    }
    try {
      const attachments = await chatApi.readChatAttachments(
        snapshot.workingFolderId,
        snapshot.attachmentIds,
      );
      if (request === this.attachmentRequest) this.attachments = attachments;
    } catch (error: unknown) {
      if (request === this.attachmentRequest) {
        this.attachments = [];
        this.sendError = chatErrorMessage(error);
      }
    }
  }

  private async dispatchQueuedFollowup(queued: ChatQueuedFollowupRead): Promise<void> {
    if (this.queuedDispatches.has(queued.id)) return;
    const thread = this.options.selectedThread();
    const workingFolderId = this.options.selectedWorkingFolderId();
    const { providerInstanceId, safetyMode, interactionMode } = queued;
    const model = readComposerModelSelection(queued.modelSelection);
    if (!thread || !workingFolderId || !providerInstanceId || !safetyMode || !interactionMode) return;
    this.queuedDispatches.add(queued.id);
    try {
      const mentions = parseDraftMentions(queued.mentions);
      const result = await chatApi.sendChatTurn({
        command: { clientCommandId: `queue-dispatch:${queued.id}`, expectedThreadRevision: null },
        workingFolderId,
        threadId: thread.id,
        newThreadId: null,
        executionEnvironmentId: null,
        turnId: `queue-turn:${queued.id}`,
        messageId: `queue-message:${queued.id}`,
        providerInstanceId,
        providerManagedModel: model.providerManaged,
        modelId: model.modelId,
        modelOptions: model.options,
        modes: { safetyMode, interactionMode },
        prompt: queued.text,
        attachmentIds: [...queued.attachmentIds],
        mentions: mentions.map((mention) => ({
          relativePath: mention.relativePath,
          kind: mention.kind,
        })),
      });
      await chatApi.markChatQueuedFollowupDispatched(thread.id, queued.id);
      this.options.upsertThread(result.thread);
      this.sendError = result.launchError?.message ?? null;
      await this.options.loadTimeline(thread.id);
      await this.refreshInteraction(thread.id);
    } finally {
      this.queuedDispatches.delete(queued.id);
    }
  }

  private async forkCurrent(providerInstanceId: ProviderInstanceId | null): Promise<void> {
    const snapshot = this.composerController.snapshot();
    if (!snapshot.workingFolderId) return;
    await this.composerController.flush();
    this.options.setSelectedThreadId(null);
    this.draftWorkingFolderId = snapshot.workingFolderId;
    this.draftThreadId = crypto.randomUUID();
    this.options.clearTimeline();
    this.interaction = null;
    await this.composerController.bind(snapshot.workingFolderId, null);
    this.composerController.setText(snapshot.text);
    this.composerController.setAttachments(snapshot.attachmentIds);
    this.composerController.setMentions(snapshot.mentions);
    this.composerController.setProvider(providerInstanceId ?? snapshot.providerInstanceId);
    this.composerController.setModelSelection(
      providerInstanceId === null ? snapshot.modelSelection : null,
    );
    this.composerController.setModes(
      providerInstanceId === null ? snapshot.safetyMode : null,
      providerInstanceId === null ? snapshot.interactionMode : null,
    );
    await this.composerController.flush();
    await chatApi.setLastSelectedChatThread(null);
  }
}

export function composerSeedForThread(thread: ChatThreadShellRead | null): ChatComposerSeed | null {
  if (!thread) return null;
  return {
    providerInstanceId: thread.providerInstanceId,
    modelSelection: composerModelSelection(
      thread.modelId,
      thread.modelId === null,
      thread.modelOptions,
    ),
    safetyMode: thread.modes.safetyMode,
    interactionMode: thread.modes.interactionMode,
  };
}

function threadMatchesExecutionConfiguration(
  thread: ChatThreadShellRead,
  workingFolderId: ProjectWorkingFolderId,
  providerInstanceId: ProviderInstanceId,
  modelId: ModelId | null,
  providerManagedModel: boolean,
  modelOptions: readonly import("$lib/chat/contracts").ModelOptionSelection[],
): boolean {
  return thread.workingFolderId === workingFolderId
    && thread.providerInstanceId === providerInstanceId
    && thread.modelId === modelId
    && (thread.modelId === null) === providerManagedModel
    && JSON.stringify(thread.modelOptions) === JSON.stringify(modelOptions);
}
