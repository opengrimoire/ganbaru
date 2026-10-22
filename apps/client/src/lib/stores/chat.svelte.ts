import * as chatApi from "$lib/api/chat";
import * as workingFolderApi from "$lib/api/project-working-folders";
import type {
  ChatBehaviorPreferences,
  ChatChannelId,
  ChatChannelRead,
  ChatChannelPageRead,
  ChatProjectPrimaryWorkingFolderRead,
  ChatReplyThreadId,
  ChatReplyThreadPageRead,
  ChatMessageRead,
  ChatMessageSearchResultRead,
  ChatScheduledMessageDispatchRead,
  ChatScheduledMessageId,
  ChatScheduledMessageRead,
  ChatAiTeammateRead,
  ChatWorkAssignmentId,
  ChatAttachmentRead,
  ChatDraftMention,
  ChatInteractionStateRead,
  ChatSettingsRead,
  ChatThreadId,
  ChatThreadShellRead,
  ChatTimelineItemRead,
  ChatTimelinePageRead,
  ProjectWorkingFolderId,
  ProjectWorkingFolderRead,
  CreateProjectWorkingFolderRequest,
  InteractionMode,
  ModelId,
  ProviderInstanceConfig,
  ProviderInstanceId,
  ProviderInstanceRead,
  ProviderProbeResult,
  ProviderRefreshResult,
  ProviderSetupTestRead,
  RemoveProviderResult,
  SafetyMode,
  UserInputAnswer,
  UtcTimestamp,
  VersionedJson,
} from "$lib/chat/contracts";
import type { ChatComposerSnapshot } from "$lib/chat/composer/controller";
import { chatErrorCode, chatErrorMessage } from "$lib/chat/error-presentation";
import { listenForChatChanges, publishChatChanges, type ChatWindowChanges } from "$lib/chat/window-sync";
import { ChatPaneStores, chatWindowChanges, type ChatChangeIds } from "$lib/stores/chat/pane-stores";
import type { WorkspacePaneScope } from "$lib/workspace-tabs/model";
import { getWorkspacePaneScope } from "$lib/workspace-tabs/pane-scope";
import { registerWindowCloseFlusher } from "$lib/windows/close-flush";
import { AsyncFrameCoalescer } from "$lib/chat/timeline/frame-coalescer";
import {
  toggleChatMessageReactionParticipant,
  type ChatMessageReaction,
} from "$lib/chat/timeline/organizational-message-model";
import { LOCAL_CHAT_PARTICIPANT_ID } from "$lib/chat/teammates/participant-display";
import { getProjectsStore, type ProjectsStore } from "$lib/stores/projects.svelte";
import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
import { preferredProjectWorkingFolder } from "$lib/chat/workspace/working-folder-selection";
import { readLastChatChannelId, saveLastChatChannelId } from "$lib/chat/channel-sections";
import {
  ChatOrganizationalController,
  type ChatOrganizationalDraft,
} from "$lib/stores/chat/organizational-controller.svelte";
import { ChatCommunicationController } from "$lib/stores/chat/communication-controller.svelte";
import { ChatTimelineController } from "$lib/stores/chat/timeline-controller.svelte";
import { ChatConfigurationController } from "$lib/stores/chat/configuration-controller.svelte";
import { ChatChannelNavigationController } from "$lib/stores/chat/channel-navigation-controller.svelte";
import { ChatThreadCollectionController } from "$lib/stores/chat/thread-collection-controller.svelte";
import { ChatComposerDefaultsCoordinator } from "$lib/stores/chat/composer-defaults-coordinator";
import {
  ChatComposerRuntimeController,
  composerSeedForThread,
  type ChatComposerSendOptions,
} from "$lib/stores/chat/composer-runtime-controller.svelte";

export type { ChatOrganizationalDraft } from "$lib/stores/chat/organizational-controller.svelte";

const localExecutionAvailable = platformHasCapability(
  BUILD_PLATFORM_PROFILE,
  "chat.local-execution",
);
export type { ChatComposerSendOptions } from "$lib/stores/chat/composer-runtime-controller.svelte";

const paneStores = new ChatPaneStores<ChatStore>();

/** Vault-wide Chat state that every pane's store shows, loaded by the primary pane's store. */
class ChatSharedState {
  teammates = $state<ChatAiTeammateRead[]>([]);
  archivedTeammates = $state<ChatAiTeammateRead[]>([]);
  providerDiscoveryStarted = false;
  readonly configuration = new ChatConfigurationController({
    onSettingsChanged: () => {
      for (const store of paneStores.all()) store.scheduleComposerDefaults();
    },
  });
}

const shared = new ChatSharedState();

/**
 * The Chat state of one split pane: its channel, thread, reply thread, composer, timeline, and panels. Settings,
 * working folders, and teammates are shared by every pane, and only the primary pane's store loads them and
 * remembers its selection for the next launch.
 */
export class ChatStore {
  private readonly channelNavigationController: ChatChannelNavigationController;
  private readonly configurationController: ChatConfigurationController;
  private readonly communicationController: ChatCommunicationController;
  private readonly composerDefaultsCoordinator: ChatComposerDefaultsCoordinator;
  private readonly composerRuntimeController: ChatComposerRuntimeController;
  private readonly organizationalController: ChatOrganizationalController;
  private readonly threadCollectionController: ChatThreadCollectionController;
  private readonly timelineController: ChatTimelineController;
  selectedChannelId = $state<ChatChannelId | null>(null);
  teammateIdentities = $derived([...this.teammates, ...this.archivedTeammates]);
  openReplyThreadId = $state<ChatReplyThreadId | null>(null);
  selectedExecutionRunId = $state<string | null>(null);
  messageAnchorId = $state<string | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  selectedWorkingFolderId = $state<ProjectWorkingFolderId | null>(null);
  /** The primary working folder of the selected channel's project. */
  primaryWorkingFolder = $state<ChatProjectPrimaryWorkingFolderRead | null>(null);
  selectedThreadId = $state<ChatThreadId | null>(null);
  selectedExecutionEnvironmentId = $state<string | null>(null);
  railOpen = $state(true);
  inspectorOpen = $state(false);
  loaded = $state(false);
  private loadRequest = 0;
  private projectSelectionProjectId: string | null = null;
  private projectSelectionPromise: Promise<void> | null = null;
  private channelSelectionRequest = 0;
  private channelReadPromise: { channelId: ChatChannelId; promise: Promise<void> } | null = null;
  private sessionMessageReactions = $state<Record<string, ChatMessageReaction[]>>({});
  private loadPromise: Promise<void> | null = null;
  private readonly nativeChanges = new AsyncFrameCoalescer<string>(
    (threadId) => this.refreshNativeChange(threadId),
  );
  private vaultGeneration = 0;
  private otherWindowListening = false;
  /** Whether this is the primary pane's store, which loads shared state and persists its selection. */
  private readonly owner: boolean;
  /** The Projects store of this pane's scope, whose selection the pane follows. */
  private readonly projects: ProjectsStore;

  constructor(scope: WorkspacePaneScope) {
    this.owner = scope === "primary";
    this.projects = getProjectsStore(scope);
    this.configurationController = shared.configuration;
    registerWindowCloseFlusher(() => this.flushComposer());
    this.composerDefaultsCoordinator = new ChatComposerDefaultsCoordinator({
      settings: () => this.settings,
      composer: () => this.composer,
      setProvider: (providerInstanceId) => this.setComposerProvider(providerInstanceId),
      setModel: (modelSelection) => this.setComposerModel(modelSelection),
      setModes: (safetyMode, interactionMode) => {
        this.setComposerModes(safetyMode, interactionMode);
      },
      flush: () => this.flushComposer(),
    });
    this.channelNavigationController = new ChatChannelNavigationController({
      currentProjectId: () => this.projects.selectedProjectId,
      selectedChannelId: () => this.selectedChannelId,
      selectChannel: (channelId) => this.selectChannel(channelId),
      onChanged: (channelId) => this.publishChanges({ channelIds: [channelId] }),
    });
    this.communicationController = new ChatCommunicationController({
      selectedChannelId: () => this.selectedChannelId,
      openReplyThreadId: () => this.openReplyThreadId,
    });
    this.timelineController = new ChatTimelineController({
      selectedThreadId: () => this.selectedThreadId,
    });
    this.threadCollectionController = new ChatThreadCollectionController({
      selectedThreadId: () => this.selectedThreadId,
      selectThread: (threadId) => this.selectThread(threadId),
      onChanged: (threadIds) => this.publishChanges({ threadIds }),
    });
    this.organizationalController = new ChatOrganizationalController({
      selectedChannel: () => this.selectedChannel,
      selectedChannelId: () => this.selectedChannelId,
      openReplyThreadId: () => this.openReplyThreadId,
      upsertChannel: (channel) => {
        this.channelNavigationController.upsert(channel);
        this.publishChanges({ channelIds: [channel.id] });
      },
      loadChannelMessages: (channelId, force) => this.loadChannelMessages(channelId, force),
      loadReplyThread: (replyThreadId, force) => this.loadReplyThread(replyThreadId, force),
      openReplyThread: (replyThreadId) => this.openReplyThread(replyThreadId),
    });
    this.composerRuntimeController = new ChatComposerRuntimeController({
      selectedThread: () => this.selectedThread,
      selectedThreadId: () => this.selectedThreadId,
      selectedWorkingFolderId: () => this.selectedWorkingFolderId,
      selectedExecutionEnvironmentId: () => this.selectedExecutionEnvironmentId,
      setSelectedThreadId: (threadId) => { this.selectedThreadId = threadId; },
      upsertThread: (thread) => this.threadCollectionController.upsert(thread),
      loadTimeline: (threadId) => this.loadTimeline(threadId),
      clearTimeline: () => this.timelineController.clear(),
      setSettings: (settings) => { this.settings = settings; },
      onComposerChanged: () => this.composerDefaultsCoordinator.schedule(),
    });
    paneStores.add(this);
  }

  get selectedWorkingFolder(): ProjectWorkingFolderRead | null {
    return this.workingFolders.find((entry) => entry.workingFolder.id === this.selectedWorkingFolderId) ?? null;
  }

  get settings(): ChatSettingsRead | null {
    return this.configurationController.settings;
  }

  set settings(settings: ChatSettingsRead | null) {
    this.configurationController.setSettings(settings);
  }

  get teammates(): ChatAiTeammateRead[] { return shared.teammates; }
  set teammates(teammates: ChatAiTeammateRead[]) { shared.teammates = teammates; }
  get archivedTeammates(): ChatAiTeammateRead[] { return shared.archivedTeammates; }
  set archivedTeammates(teammates: ChatAiTeammateRead[]) { shared.archivedTeammates = teammates; }

  /** Applies changed shared settings to this pane's composer defaults. */
  scheduleComposerDefaults(): void {
    this.composerDefaultsCoordinator.schedule();
  }

  get workingFolders(): ProjectWorkingFolderRead[] { return this.configurationController.workingFolders; }
  set workingFolders(workingFolders: ProjectWorkingFolderRead[]) { this.configurationController.workingFolders = workingFolders; }
  get providerDiscoveryLoading(): boolean { return this.configurationController.providerDiscoveryLoading; }
  set providerDiscoveryLoading(loading: boolean) { this.configurationController.providerDiscoveryLoading = loading; }
  get activeChannels(): ChatChannelRead[] { return this.channelNavigationController.activeChannels; }
  set activeChannels(channels: ChatChannelRead[]) { this.channelNavigationController.activeChannels = channels; }
  get archivedChannels(): ChatChannelRead[] { return this.channelNavigationController.archivedChannels; }
  set archivedChannels(channels: ChatChannelRead[]) { this.channelNavigationController.archivedChannels = channels; }
  get archivedChannelsLoading(): boolean { return this.channelNavigationController.archivedChannelsLoading; }
  set archivedChannelsLoading(loading: boolean) { this.channelNavigationController.archivedChannelsLoading = loading; }
  get archivedChannelsError(): string | null { return this.channelNavigationController.archivedChannelsError; }
  set archivedChannelsError(error: string | null) { this.channelNavigationController.archivedChannelsError = error; }
  get channelsLoading(): boolean { return this.channelNavigationController.channelsLoading; }
  set channelsLoading(loading: boolean) { this.channelNavigationController.channelsLoading = loading; }
  get channelArchiveOpen(): boolean { return this.channelNavigationController.archiveOpen; }
  set channelArchiveOpen(open: boolean) { this.channelNavigationController.archiveOpen = open; }
  get showArchivedChannelsInPickers(): boolean { return this.channelNavigationController.showArchivedInPickers; }
  set showArchivedChannelsInPickers(show: boolean) { this.channelNavigationController.showArchivedInPickers = show; }
  get activeThreads(): ChatThreadShellRead[] { return this.threadCollectionController.activeThreads; }
  set activeThreads(threads: ChatThreadShellRead[]) { this.threadCollectionController.activeThreads = threads; }
  get archivedThreads(): ChatThreadShellRead[] { return this.threadCollectionController.archivedThreads; }
  set archivedThreads(threads: ChatThreadShellRead[]) { this.threadCollectionController.archivedThreads = threads; }
  get archivedThreadsLoading(): boolean { return this.threadCollectionController.archivedThreadsLoading; }
  set archivedThreadsLoading(loading: boolean) { this.threadCollectionController.archivedThreadsLoading = loading; }

  get composer(): ChatComposerSnapshot { return this.composerRuntimeController.composer; }
  set composer(composer: ChatComposerSnapshot) { this.composerRuntimeController.composer = composer; }
  get composerAttachments(): ChatAttachmentRead[] { return this.composerRuntimeController.attachments; }
  set composerAttachments(attachments: ChatAttachmentRead[]) { this.composerRuntimeController.attachments = attachments; }
  get interaction(): ChatInteractionStateRead | null { return this.composerRuntimeController.interaction; }
  set interaction(interaction: ChatInteractionStateRead | null) { this.composerRuntimeController.interaction = interaction; }
  get interactionLoading(): boolean { return this.composerRuntimeController.interactionLoading; }
  set interactionLoading(loading: boolean) { this.composerRuntimeController.interactionLoading = loading; }
  get sendError(): string | null { return this.composerRuntimeController.sendError; }
  set sendError(error: string | null) { this.composerRuntimeController.sendError = error; }
  get draftWorkingFolderId(): ProjectWorkingFolderId | null { return this.composerRuntimeController.draftWorkingFolderId; }
  set draftWorkingFolderId(workingFolderId: ProjectWorkingFolderId | null) { this.composerRuntimeController.draftWorkingFolderId = workingFolderId; }
  get draftThreadId(): ChatThreadId | null { return this.composerRuntimeController.draftThreadId; }
  set draftThreadId(threadId: ChatThreadId | null) { this.composerRuntimeController.draftThreadId = threadId; }
  get pendingUserMessage(): { threadId: ChatThreadId; row: import("$lib/chat/timeline/model").TimelineMessageRow } | null {
    return this.composerRuntimeController.pendingUserMessage;
  }
  set pendingUserMessage(message: { threadId: ChatThreadId; row: import("$lib/chat/timeline/model").TimelineMessageRow } | null) {
    this.composerRuntimeController.pendingUserMessage = message;
  }

  get channelPages(): ChatChannelPageRead[] {
    return this.communicationController.channelPages;
  }

  set channelPages(pages: ChatChannelPageRead[]) {
    this.communicationController.channelPages = pages;
  }

  get channelMessages(): ChatMessageRead[] {
    return this.communicationController.channelMessages;
  }

  set channelMessages(messages: ChatMessageRead[]) {
    this.communicationController.channelMessages = messages;
  }

  get channelMessagesLoading(): boolean {
    return this.communicationController.channelLoading;
  }

  set channelMessagesLoading(loading: boolean) {
    this.communicationController.channelLoading = loading;
  }

  get channelMessagesError(): string | null {
    return this.communicationController.channelError;
  }

  set channelMessagesError(error: string | null) {
    this.communicationController.channelError = error;
  }

  get replyThreadPages(): ChatReplyThreadPageRead[] {
    return this.communicationController.replyThreadPages;
  }

  set replyThreadPages(pages: ChatReplyThreadPageRead[]) {
    this.communicationController.replyThreadPages = pages;
  }

  get replyThread(): ChatReplyThreadPageRead | null {
    return this.communicationController.replyThread;
  }

  set replyThread(replyThread: ChatReplyThreadPageRead | null) {
    this.communicationController.replyThread = replyThread;
  }

  get replyThreadLoading(): boolean {
    return this.communicationController.replyThreadLoading;
  }

  set replyThreadLoading(loading: boolean) {
    this.communicationController.replyThreadLoading = loading;
  }

  get replyThreadError(): string | null {
    return this.communicationController.replyThreadError;
  }

  set replyThreadError(error: string | null) {
    this.communicationController.replyThreadError = error;
  }

  get timelinePages(): ChatTimelinePageRead[] { return this.timelineController.pages; }
  set timelinePages(pages: ChatTimelinePageRead[]) { this.timelineController.pages = pages; }
  get timelineItems(): ChatTimelineItemRead[] { return this.timelineController.items; }
  set timelineItems(items: ChatTimelineItemRead[]) { this.timelineController.items = items; }
  get timelineLoading(): boolean { return this.timelineController.loading; }
  set timelineLoading(loading: boolean) { this.timelineController.loading = loading; }
  get timelineError(): string | null { return this.timelineController.error; }
  set timelineError(error: string | null) { this.timelineController.error = error; }

  get organizationalDrafts(): Record<string, ChatOrganizationalDraft> {
    return this.organizationalController.drafts;
  }

  get organizationalScrollPositions(): Record<string, number> {
    return this.organizationalController.scrollPositions;
  }

  get scheduledMessagesVersion(): number {
    return this.organizationalController.scheduledMessagesVersion;
  }

  get selectedChannel(): ChatChannelRead | null {
    return this.selectedChannelId
      ? this.channelNavigationController.find(this.selectedChannelId)
      : null;
  }

  channelsForProject(projectId: string): ChatChannelRead[] {
    return this.channelNavigationController.channelsForProject(projectId);
  }

  archivedChannelsForProject(projectId: string): ChatChannelRead[] {
    return this.channelNavigationController.archivedChannelsForProject(projectId);
  }

  loadArchivedChannelsForPicker(projectId: string): Promise<void> {
    return this.channelNavigationController.loadArchivedForPicker(projectId);
  }

  get selectedThread(): ChatThreadShellRead | null {
    return this.selectedThreadId
      ? this.threadCollectionController.find(this.selectedThreadId)
      : null;
  }

  /** Opens Chat from cached history and discovers providers without delaying that history. */
  async ensureLoaded(): Promise<void> {
    const vaultGeneration = this.vaultGeneration;
    await this.ensureCachedLoaded();
    if (vaultGeneration === this.vaultGeneration && this.loaded) this.startProviderDiscovery();
  }

  /** Shares workspace hydration without starting provider discovery during prewarming. */
  private async ensureCachedLoaded(): Promise<void> {
    if (this.loaded) return;
    this.loadPromise ??= this.reload(false).finally(() => {
      this.loadPromise = null;
    });
    await this.loadPromise;
  }

  /** Starts the bounded Chat working set before the route is opened. */
  async prewarmForProject(projectId: string | null): Promise<void> {
    await this.ensureCachedLoaded();
    if (projectId && this.selectedChannel?.projectId !== projectId) {
      await this.syncProjectSelection(projectId);
    }
  }

  /** Invalidates this pane's vault-scoped Chat state; {@link resetChatForVault} also resets the shared state. */
  resetPaneForVault(): void {
    this.vaultGeneration += 1;
    this.loadRequest += 1;
    this.channelSelectionRequest += 1;
    this.loaded = false;
    this.communicationController.reset();
    this.channelNavigationController.reset();
    this.organizationalController.reset();
    this.threadCollectionController.reset();
    this.timelineController.reset();
    this.loadPromise = null;
    this.projectSelectionProjectId = null;
    this.projectSelectionPromise = null;
    this.channelReadPromise = null;
    this.sessionMessageReactions = {};
    this.selectedChannelId = null;
    this.openReplyThreadId = null;
    this.selectedExecutionRunId = null;
    this.messageAnchorId = null;
    this.selectedWorkingFolderId = null;
    this.primaryWorkingFolder = null;
    this.selectedThreadId = null;
    this.error = null;
    this.loading = false;
    this.composerRuntimeController.reset();
  }

  /** Refreshes local Chat data; background prewarming leaves provider CLIs dormant. */
  async reload(discoverProviders = true): Promise<void> {
    const request = ++this.loadRequest;
    const vaultGeneration = this.vaultGeneration;
    this.loading = true;
    this.error = null;
    try {
      const loaded = this.owner ? await this.loadSharedState() : await this.followSharedState();
      if (!loaded || request !== this.loadRequest || vaultGeneration !== this.vaultGeneration) return;
      const { settings, navigationChannels } = loaded;
      this.channelNavigationController.hydrateNavigation(navigationChannels);
      this.threadCollectionController.resetWindow();
      if (discoverProviders) this.startProviderDiscovery();
      if (this.owner && localExecutionAvailable) {
        void chatApi.recoverChatAssignmentDispatchJobs().catch((error: unknown) => {
          console.error("Chat assignment recovery failed", error);
        });
      }
      await this.projects.ensureLoaded();
      if (request !== this.loadRequest || vaultGeneration !== this.vaultGeneration) return;
      const projectId = this.projects.selectedProjectId;
      if (projectId) {
        await this.loadProjectChannels(projectId);
        const rememberedChannelId = readLastChatChannelId(projectId);
        const initial = this.activeChannels.find((channel) => channel.id === rememberedChannelId)
          ?? this.activeChannels.find((channel) => channel.isDefault)
          ?? this.activeChannels[0]
          ?? null;
        if (initial) await this.selectChannel(initial.id);
        else await this.restoreSelection(this.settings ?? settings);
      } else {
        this.channelsLoading = false;
        await this.restoreSelection(this.settings ?? settings);
      }
      const workingFolderId = this.selectedWorkingFolderId;
      if (workingFolderId && !this.selectedChannel) await this.composerRuntimeController.bind(
        workingFolderId,
        this.selectedThreadId,
        composerSeedForThread(this.selectedThread),
      );
      if (request !== this.loadRequest || vaultGeneration !== this.vaultGeneration) return;
      this.loaded = true;
      this.listenForOtherWindows();
      if (!this.owner) return;
      void this.prefetchRememberedProjectChannels(projectId, vaultGeneration);
      void this.configurationController.refreshWorkingFolders().catch((error: unknown) => {
        console.error("Chat working folder reconciliation failed", error);
      });
    } catch (error: unknown) {
      if (request !== this.loadRequest) return;
      this.error = chatErrorMessage(error, "Chat could not be loaded");
      throw error;
    } finally {
      if (request === this.loadRequest) this.loading = false;
    }
  }

  /** Recovers interrupted turns and loads the settings, working folders, and teammates every pane shows. */
  private async loadSharedState(): Promise<{ settings: ChatSettingsRead; navigationChannels: ChatChannelRead[] }> {
    const vaultGeneration = this.vaultGeneration;
    if (localExecutionAvailable) await chatApi.recoverInterruptedChatTurns();
    const [settings, workingFolders, navigationChannels, teammates, archivedTeammates] = await Promise.all([
      chatApi.readChatSettings(),
      localExecutionAvailable
        ? workingFolderApi.listCachedProjectWorkingFolders()
        : Promise.resolve([]),
      chatApi.listChatNavigationChannels(),
      chatApi.listChatTeammates(false),
      chatApi.listChatTeammates(true),
    ]);
    if (vaultGeneration === this.vaultGeneration) {
      this.configurationController.hydrate(settings, workingFolders);
      this.teammates = teammates;
      this.archivedTeammates = archivedTeammates;
    }
    return { settings, navigationChannels };
  }

  /** Waits for the primary pane's store to load the shared state, then reads this pane's own channel list. */
  private async followSharedState(): Promise<{ settings: ChatSettingsRead; navigationChannels: ChatChannelRead[] } | null> {
    await storeFor("primary").ensureCachedLoaded();
    const navigationChannels = await chatApi.listChatNavigationChannels();
    const settings = this.settings;
    return settings ? { settings, navigationChannels } : null;
  }

  /** Starts at most one automatic discovery per vault, without delaying local history. */
  private startProviderDiscovery(): void {
    if (shared.providerDiscoveryStarted) return;
    shared.providerDiscoveryStarted = true;
    const vaultGeneration = this.vaultGeneration;
    const isCurrentVault = () => vaultGeneration === this.vaultGeneration;
    void this.configurationController.discoverProviders(isCurrentVault).catch(async (error: unknown) => {
      if (!isCurrentVault()) return;
      console.error("Automatic Chat provider discovery failed", error);
      try {
        await this.configurationController.refreshSettings(isCurrentVault);
      } catch (refreshError: unknown) {
        if (isCurrentVault()) console.error("Chat settings refresh after discovery failed", refreshError);
      }
    });
  }

  private async prefetchRememberedProjectChannels(
    selectedProjectId: string | null,
    vaultGeneration: number,
  ): Promise<void> {
    for (const project of this.projects.projects) {
      if (project.id === selectedProjectId || project.status !== "active") continue;
      const rememberedChannelId = readLastChatChannelId(project.id);
      const projectChannels = this.channelNavigationController.channelsForProject(project.id);
      const channel = projectChannels.find((entry) => entry.id === rememberedChannelId)
        ?? projectChannels.find((entry) => entry.isDefault)
        ?? projectChannels[0]
        ?? null;
      if (!channel || this.communicationController.hasCachedChannel(channel.id)) continue;
      try {
        await this.communicationController.prefetchChannel(
          channel.id,
          () => vaultGeneration === this.vaultGeneration,
        );
        if (vaultGeneration !== this.vaultGeneration) return;
      } catch (error: unknown) {
        console.error(`Chat prefetch failed for project ${project.id}`, error);
      }
    }
  }

  async refreshSettings(): Promise<void> {
    return this.configurationController.refreshSettings();
  }

  async ensureArchivedThreads(): Promise<void> {
    return this.threadCollectionController.ensureArchived();
  }

  syncProjectSelection(projectId: string | null): Promise<void> {
    if (!projectId || !this.loaded) return Promise.resolve();
    if (this.selectedChannel?.projectId === projectId) return Promise.resolve();
    if (this.projectSelectionProjectId === projectId && this.projectSelectionPromise) {
      return this.projectSelectionPromise;
    }
    const selection = this.performProjectSelection(projectId);
    const tracked = selection.finally(() => {
      if (this.projectSelectionPromise !== tracked) return;
      this.projectSelectionProjectId = null;
      this.projectSelectionPromise = null;
    });
    this.projectSelectionProjectId = projectId;
    this.projectSelectionPromise = tracked;
    return tracked;
  }

  private async performProjectSelection(projectId: string): Promise<void> {
    if (!await this.loadProjectChannels(projectId)) return;
    if (this.projects.selectedProjectId !== projectId) return;
    const rememberedChannelId = readLastChatChannelId(projectId);
    const selected = this.activeChannels.find((channel) => channel.id === rememberedChannelId)
      ?? this.activeChannels.find((channel) => channel.isDefault)
      ?? this.activeChannels[0]
      ?? null;
    if (selected) await this.selectChannel(selected.id);
  }

  async loadProjectChannels(projectId: string): Promise<boolean> {
    return this.channelNavigationController.loadProject(projectId);
  }

  async selectChannel(channelId: ChatChannelId): Promise<void> {
    const request = ++this.channelSelectionRequest;
    const channel = this.channelNavigationController.find(channelId)
      ?? await chatApi.readChatChannel(channelId);
    if (request !== this.channelSelectionRequest) return;
    this.channelNavigationController.upsert(channel);
    const changedChannel = this.selectedChannelId !== channelId;
    this.selectedChannelId = channelId;
    if (this.owner) saveLastChatChannelId(channel.projectId, channelId);
    this.channelArchiveOpen = false;
    await this.projects.selectProject(channel.projectId);
    if (request !== this.channelSelectionRequest) return;
    if (changedChannel) this.closeReplyThread();
    try {
      this.primaryWorkingFolder = await this.configurationController.readPrimaryWorkingFolder(channel.projectId);
    } catch {
      this.primaryWorkingFolder = null;
    }
    const rememberedFolderId = localExecutionAvailable
      ? await workingFolderApi.lastProjectWorkingFolder(channel.projectId)
      : null;
    const fallbackFolder = preferredProjectWorkingFolder(this.workingFolders, channel.projectId, rememberedFolderId);
    this.selectedWorkingFolderId = this.primaryWorkingFolder?.workingFolderId
      ?? fallbackFolder?.workingFolder.id
      ?? null;
    this.selectedThreadId = null;
    this.selectedExecutionRunId = null;
    this.selectedExecutionEnvironmentId = null;
    // Workspace tools such as the terminal are scoped to a thread, so a channel without a session keeps a draft one.
    if (this.selectedWorkingFolderId) {
      this.composerRuntimeController.ensureDraftThread(this.selectedWorkingFolderId);
    } else {
      this.composerRuntimeController.discardDraft();
    }
    await this.loadChannelMessages(channel.id);
    if (request !== this.channelSelectionRequest) return;
    this.interaction = null;
    if (channel.unreadCount > 0) void this.markSelectedChannelRead();
  }

  async markSelectedChannelRead(): Promise<void> {
    const channel = this.selectedChannel;
    if (!channel || channel.unreadCount === 0) return;
    if (this.channelReadPromise?.channelId === channel.id) return this.channelReadPromise.promise;
    const vaultGeneration = this.vaultGeneration;
    const promise = chatApi.setChatChannelRead(channel.id, true)
      .then((updated) => {
        if (this.vaultGeneration !== vaultGeneration) return;
        this.channelNavigationController.upsert(updated);
        this.publishChanges({ channelIds: [updated.id] });
      })
      .catch((error: unknown) => {
        console.error(`Could not mark #${channel.name} as read`, error);
      })
      .finally(() => {
        if (this.channelReadPromise?.promise === promise) this.channelReadPromise = null;
      });
    this.channelReadPromise = { channelId: channel.id, promise };
    return promise;
  }

  messageReactionsFor(messageKey: string): readonly ChatMessageReaction[] {
    return this.sessionMessageReactions[messageKey] ?? [];
  }

  toggleSessionMessageReaction(messageKey: string, value: string, displayName: string): void {
    const normalizedKey = messageKey.trim();
    if (!normalizedKey) return;
    const current = this.sessionMessageReactions[normalizedKey] ?? [];
    this.sessionMessageReactions[normalizedKey] = toggleChatMessageReactionParticipant(
      current,
      value,
      {
        participantId: LOCAL_CHAT_PARTICIPANT_ID,
        displayName: displayName.trim(),
      },
    );
  }

  async createChannel(request: import("$lib/chat/contracts").CreateChatChannelRequest): Promise<ChatChannelRead> {
    return this.channelNavigationController.create(request);
  }

  async updateChannelDetails(channel: ChatChannelRead, name: string, topic: string): Promise<ChatChannelRead> {
    return this.channelNavigationController.updateDetails(channel, name, topic);
  }

  async refreshTeammates(): Promise<void> {
    const [teammates, archivedTeammates] = await Promise.all([
      chatApi.listChatTeammates(false),
      chatApi.listChatTeammates(true),
    ]);
    this.teammates = teammates;
    this.archivedTeammates = archivedTeammates;
    if (this.selectedChannelId) {
      this.channelNavigationController.upsert(await chatApi.readChatChannel(this.selectedChannelId));
    }
  }

  organizationalDraft(destination: string): ChatOrganizationalDraft {
    return this.organizationalController.draft(destination);
  }

  setOrganizationalDraft(destination: string, draft: ChatOrganizationalDraft): void {
    this.organizationalController.setDraft(destination, draft);
  }

  setOrganizationalScrollPosition(destination: string, scrollTop: number): void {
    this.organizationalController.setScrollPosition(destination, scrollTop);
  }

  async postOrganizationalMessage(
    destination: string,
    options: { alsoSendToChannel?: boolean } = {},
  ): Promise<import("$lib/chat/contracts").PostChatMessageResult> {
    const result = await this.organizationalController.post(destination, options);
    await this.markSelectedChannelRead();
    return result;
  }

  async scheduleOrganizationalMessage(
    destination: string,
    scheduledFor: UtcTimestamp,
    options: { alsoSendToChannel?: boolean } = {},
  ): Promise<ChatScheduledMessageRead> {
    const channelId = this.selectedChannelId;
    const scheduled = await this.organizationalController.schedule(destination, scheduledFor, options);
    this.publishChannelChange(channelId);
    return scheduled;
  }

  async listScheduledOrganizationalMessages(destination: string): Promise<ChatScheduledMessageRead[]> {
    return this.organizationalController.listScheduled(destination);
  }

  async cancelScheduledOrganizationalMessage(id: ChatScheduledMessageId): Promise<void> {
    const channelId = this.selectedChannelId;
    await this.organizationalController.cancelScheduled(id);
    this.publishChannelChange(channelId);
  }

  async retryScheduledOrganizationalMessage(id: ChatScheduledMessageId): Promise<ChatScheduledMessageRead> {
    const channelId = this.selectedChannelId;
    const scheduled = await this.organizationalController.retryScheduled(id);
    this.publishChannelChange(channelId);
    return scheduled;
  }

  async sendScheduledOrganizationalMessageNow(
    id: ChatScheduledMessageId,
  ): Promise<import("$lib/chat/contracts").PostChatMessageResult> {
    return this.organizationalController.sendScheduledNow(id);
  }

  async dispatchDueScheduledMessages(): Promise<ChatScheduledMessageDispatchRead> {
    return this.organizationalController.dispatchDue();
  }

  async openReplyThread(replyThreadId: ChatReplyThreadId): Promise<void> {
    this.openReplyThreadId = replyThreadId;
    this.selectedExecutionRunId = null;
    await this.loadReplyThread(replyThreadId, true);
  }

  async searchOrganizationalMessages(query: string): Promise<ChatMessageSearchResultRead[]> {
    const normalized = query.trim();
    if (normalized.length < 2) return [];
    return chatApi.searchChatMessages(normalized);
  }

  async openMessageSearchResult(result: ChatMessageSearchResultRead): Promise<void> {
    this.messageAnchorId = null;
    await this.projects.selectProject(result.projectId);
    await this.loadProjectChannels(result.projectId);
    await this.selectChannel(result.channelId);
    const anchorCursor = String(result.ordinal + 1);
    if (result.replyThreadId) {
      this.openReplyThreadId = result.replyThreadId;
      this.selectedExecutionRunId = null;
      await this.communicationController.loadReplyThreadAtCursor(result.replyThreadId, anchorCursor);
    } else {
      await this.communicationController.loadChannelAtCursor(result.channelId, anchorCursor);
    }
    this.messageAnchorId = result.messageItemId;
  }

  clearMessageAnchor(messageItemId: string): void {
    if (this.messageAnchorId === messageItemId) this.messageAnchorId = null;
  }

  closeReplyThread(): void {
    this.openReplyThreadId = null;
    this.communicationController.clearReplyThread();
    this.selectedExecutionRunId = null;
  }

  async selectAssignmentExecution(runId: string): Promise<void> {
    const replyThreadId = this.openReplyThreadId;
    const run = this.replyThread?.agentRuns.find((entry) => entry.id === runId) ?? null;
    if (!run?.providerExecutionThreadId) throw new Error("This execution has not started yet");
    let thread = [...this.activeThreads, ...this.archivedThreads]
      .find((entry) => entry.id === run.providerExecutionThreadId) ?? null;
    thread ??= await chatApi.readChatThreadShell(run.providerExecutionThreadId);
    if (this.openReplyThreadId !== replyThreadId
      || !this.replyThread?.agentRuns.some((entry) => entry.id === runId)) return;
    if (thread.scratchGenerationId !== null) {
      this.selectedExecutionRunId = run.id;
      return;
    }
    if (thread.workingFolderId === null) {
      throw new Error("This execution does not have a valid direct working folder");
    }
    this.threadCollectionController.upsert(thread);
    this.selectedExecutionRunId = run.id;
    this.selectThread(thread.id);
  }

  async cancelAssignment(assignmentId: ChatWorkAssignmentId): Promise<void> {
    const expectedRevision = this.replyThread?.assignment?.id === assignmentId
      ? this.replyThread.assignment.revision
      : null;
    if (expectedRevision === null) throw new Error("Work assignment was not loaded");
    await chatApi.cancelChatAssignment(assignmentId, expectedRevision);
    this.publishChannelChange(this.selectedChannelId);
    if (this.openReplyThreadId) await this.loadReplyThread(this.openReplyThreadId, true);
    if (this.selectedChannelId) await this.loadChannelMessages(this.selectedChannelId, true);
  }

  async retryAssignment(assignmentId: ChatWorkAssignmentId): Promise<void> {
    const expectedRevision = this.replyThread?.assignment?.id === assignmentId
      ? this.replyThread.assignment.revision
      : null;
    if (expectedRevision === null) throw new Error("Work assignment was not loaded");
    await chatApi.retryChatAssignment(assignmentId, expectedRevision);
    this.publishChannelChange(this.selectedChannelId);
    if (this.openReplyThreadId) await this.loadReplyThread(this.openReplyThreadId, true);
  }

  async archiveChannel(channel: ChatChannelRead): Promise<void> {
    return this.channelNavigationController.archive(channel);
  }

  async restoreChannel(channel: ChatChannelRead): Promise<void> {
    return this.channelNavigationController.restore(channel);
  }

  openChannelArchive(): void {
    this.channelNavigationController.openArchive(this.projects.selectedProjectId);
  }

  closeChannelArchive(): void {
    this.channelNavigationController.closeArchive();
  }

  async saveProvider(configuration: ProviderInstanceConfig): Promise<ProviderInstanceRead | null> {
    return this.configurationController.saveProvider(configuration);
  }

  async confirmProviderRuntime(instanceId: ProviderInstanceId): Promise<ProviderInstanceRead | null> {
    return this.configurationController.confirmProviderRuntime(instanceId);
  }

  async testProvider(configuration: ProviderInstanceConfig): Promise<ProviderSetupTestRead | null> {
    return this.configurationController.testProvider(configuration);
  }

  async probeProvider(instanceId: ProviderInstanceId): Promise<ProviderProbeResult> {
    return this.configurationController.probeProvider(instanceId);
  }

  async refreshAllProviders(): Promise<ProviderRefreshResult> {
    return this.configurationController.refreshAllProviders();
  }

  async setProviderEnabled(instanceId: ProviderInstanceId, enabled: boolean): Promise<void> {
    return this.configurationController.setProviderEnabled(instanceId, enabled);
  }

  async removeProvider(instanceId: ProviderInstanceId): Promise<RemoveProviderResult> {
    return this.configurationController.removeProvider(instanceId);
  }

  async refreshModels(instanceId: ProviderInstanceId): Promise<void> {
    return this.configurationController.refreshModels(instanceId);
  }

  async updateModels(instanceId: ProviderInstanceId, visible: ModelId[], favorites: ModelId[]): Promise<void> {
    return this.configurationController.updateModels(instanceId, visible, favorites);
  }

  async updateBehavior(behavior: ChatBehaviorPreferences): Promise<void> {
    return this.configurationController.updateBehavior(behavior);
  }

  async addExternalWorkingFolder(
    request: CreateProjectWorkingFolderRequest,
    pickerTitle: string,
  ): Promise<ProjectWorkingFolderRead | null> {
    const workingFolder = await this.configurationController.addExternalWorkingFolder(
      request,
      pickerTitle,
    );
    if (!workingFolder) return null;
    this.selectWorkingFolder(workingFolder.workingFolder.id);
    return workingFolder;
  }

  async locateWorkingFolder(workingFolderId: ProjectWorkingFolderId, pickerTitle: string): Promise<void> {
    return this.configurationController.locateWorkingFolder(workingFolderId, pickerTitle);
  }

  async rebindWorkingFolder(workingFolderId: ProjectWorkingFolderId, pickerTitle: string): Promise<void> {
    return this.configurationController.rebindWorkingFolder(workingFolderId, pickerTitle);
  }

  async unbindWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    return this.configurationController.unbindWorkingFolder(workingFolderId);
  }

  async openWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    return this.configurationController.openWorkingFolder(workingFolderId);
  }

  async recreateManagedWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    return this.configurationController.recreateManagedWorkingFolder(workingFolderId);
  }

  async renameWorkingFolder(workingFolderId: ProjectWorkingFolderId, displayName: string): Promise<void> {
    return this.configurationController.renameWorkingFolder(workingFolderId, displayName);
  }

  async archiveWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    return this.configurationController.archiveWorkingFolder(workingFolderId);
  }

  async restoreWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    return this.configurationController.restoreWorkingFolder(workingFolderId);
  }

  async removeWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    await this.configurationController.removeWorkingFolder(workingFolderId);
    if (this.selectedWorkingFolderId === workingFolderId) {
      this.selectedWorkingFolderId = null;
      await this.syncProjectSelection(this.projects.selectedProjectId);
    }
  }

  async setWorkingFolderProviderPreference(workingFolderId: ProjectWorkingFolderId, instanceId: ProviderInstanceId | null): Promise<void> {
    return this.configurationController.setWorkingFolderProviderPreference(
      workingFolderId,
      instanceId,
    );
  }

  /** Show a project's newly saved primary working folder when this pane's channel belongs to that project. */
  adoptProjectPrimaryWorkingFolder(primary: ChatProjectPrimaryWorkingFolderRead): void {
    if (this.selectedChannel?.projectId !== primary.projectId) return;
    this.primaryWorkingFolder = primary;
    this.selectedWorkingFolderId = primary.workingFolderId;
  }

  selectWorkingFolder(workingFolderId: ProjectWorkingFolderId): void {
    this.selectedWorkingFolderId = workingFolderId;
    this.selectedExecutionEnvironmentId = null;
    const projectId = this.selectedWorkingFolder?.workingFolder.projectId;
    if (projectId) {
      void this.projects.selectProject(projectId);
      if (this.owner) void workingFolderApi.rememberProjectWorkingFolder(projectId, workingFolderId);
    }
    this.selectThread(null);
    this.composerRuntimeController.ensureDraftThread(workingFolderId);
  }

  selectThread(threadId: ChatThreadId | null): void {
    if (threadId !== this.selectedThreadId) {
      this.timelineController.clear();
    }
    this.selectedThreadId = threadId;
    if (threadId) {
      void chatApi.readChatThreadExecutionEnvironment(threadId)
        .then((environmentId) => {
          if (this.selectedThreadId === threadId) this.selectedExecutionEnvironmentId = environmentId;
        })
        .catch(() => undefined);
    } else {
      this.selectedExecutionEnvironmentId = null;
    }
    const thread = this.selectedThread;
    if (thread) {
      if (thread.workingFolderId === null || thread.scratchGenerationId !== null) {
        throw new Error("Private scratch threads cannot be opened as direct agent threads");
      }
      this.selectedWorkingFolderId = thread.workingFolderId;
      void this.projects.selectProject(thread.projectId);
      if (this.owner) void workingFolderApi.rememberProjectWorkingFolder(thread.projectId, thread.workingFolderId);
    }
    if (this.selectedWorkingFolderId) {
      void this.composerRuntimeController.bind(
        this.selectedWorkingFolderId,
        threadId,
        composerSeedForThread(thread),
      ).catch(() => undefined);
    }
    if (this.owner) {
      void chatApi.setLastSelectedChatThread(threadId).catch((error) => {
        console.error("Failed to persist selected Chat thread", error);
      });
    }
    if (threadId) void this.loadTimeline(threadId).catch(() => undefined);
    if (threadId) void this.refreshInteraction(threadId).catch(() => undefined);
    else this.interaction = null;
  }

  selectThreadShell(thread: ChatThreadShellRead): void {
    if (thread.workingFolderId === null || thread.scratchGenerationId !== null) {
      throw new Error("Private scratch threads cannot be opened as direct agent threads");
    }
    this.threadCollectionController.upsert(thread);
    this.selectThread(thread.id);
  }

  async loadOlderTimeline(selectedSequence: number | null = null): Promise<void> {
    return this.timelineController.loadOlder(selectedSequence);
  }

  async startNewChannelSession(): Promise<void> {
    if (!this.selectedWorkingFolderId) return;
    this.selectedThreadId = null;
    this.selectedExecutionEnvironmentId = null;
    this.composerRuntimeController.ensureDraftThread(this.selectedWorkingFolderId);
    this.interaction = null;
    await this.composerRuntimeController.bind(
      this.selectedWorkingFolderId,
      null,
      null,
    );
  }

  newDraft(workingFolderId: ProjectWorkingFolderId): void {
    this.selectWorkingFolder(workingFolderId);
  }

  discardDraft(): void {
    this.composerRuntimeController.discardDraft();
  }

  setComposerText(text: string): void { this.composerRuntimeController.setText(text); }
  setComposerRichContent(text: string, richContent: VersionedJson): void { this.composerRuntimeController.setRichContent(text, richContent); }
  setComposerAttachments(attachmentIds: string[]): void { this.composerRuntimeController.setAttachments(attachmentIds); }
  setExecutionEnvironment(environmentId: string | null): void { this.selectedExecutionEnvironmentId = environmentId; }
  setComposerMentions(mentions: ChatDraftMention[]): void { this.composerRuntimeController.setMentions(mentions); }
  setComposerProvider(instanceId: ProviderInstanceId | null): void { this.composerRuntimeController.setProvider(instanceId); }
  setComposerModel(selection: VersionedJson | null): void { this.composerRuntimeController.setModel(selection); }
  setComposerModes(safety: SafetyMode | null, interaction: InteractionMode | null): void { this.composerRuntimeController.setModes(safety, interaction); }
  markComposerSent(): void { this.composerRuntimeController.markSent(); }
  dismissComposerChangedElsewhere(): void { this.composerRuntimeController.dismissChangedElsewhere(); }
  flushComposer(): Promise<void> { return this.composerRuntimeController.flush(); }
  refreshInteraction(threadId = this.selectedThreadId): Promise<void> { return this.composerRuntimeController.refreshInteraction(threadId); }
  sendComposer(options: ChatComposerSendOptions = {}): Promise<void> { return this.composerRuntimeController.send(options); }
  retryFailedSend(): Promise<void> { return this.composerRuntimeController.retryFailedSend(); }
  editFailedSend(): Promise<void> { return this.composerRuntimeController.editFailedSend(); }
  changeProviderAfterFailure(): Promise<void> { return this.composerRuntimeController.changeProviderAfterFailure(); }
  forkComposerWithProvider(providerInstanceId: ProviderInstanceId): Promise<void> { return this.composerRuntimeController.forkWithProvider(providerInstanceId); }
  steerComposer(): Promise<void> { return this.composerRuntimeController.steer(); }
  queueComposer(): Promise<void> { return this.composerRuntimeController.queue(); }
  cancelQueuedFollowup(): Promise<void> { return this.composerRuntimeController.cancelQueuedFollowup(); }
  editQueuedFollowup(): Promise<void> { return this.composerRuntimeController.editQueuedFollowup(); }
  stop(force = false): Promise<void> { return this.composerRuntimeController.stop(force); }
  resolveApproval(decision: import("$lib/chat/contracts").ApprovalDecision): Promise<void> { return this.composerRuntimeController.resolveApproval(decision); }
  resolveUserInput(answers: UserInputAnswer[]): Promise<void> { return this.composerRuntimeController.resolveUserInput(answers); }
  importComposerImages(files: File[]): Promise<void> { return this.composerRuntimeController.importImages(files); }
  pickComposerImages(title: string): Promise<void> { return this.composerRuntimeController.pickImages(title); }
  removeComposerAttachment(attachmentId: string): void { this.composerRuntimeController.removeAttachment(attachmentId); }

  async handleNativeChange(threadId: string): Promise<void> {
    await this.nativeChanges.push(threadId);
  }

  private async refreshNativeChange(threadId: string): Promise<void> {
    const thread = await chatApi.readChatThreadShell(threadId);
    if (thread.workingFolderId !== null && thread.scratchGenerationId === null) {
      this.threadCollectionController.upsert(thread);
    }
    const channelId = this.selectedChannelId;
    if (channelId) await this.loadChannelMessages(channelId, true);
    if (this.openReplyThreadId) await this.loadReplyThread(this.openReplyThreadId, true);
    if (threadId !== this.selectedThreadId) return;
    await this.loadTimeline(threadId);
    await this.refreshInteraction(threadId);
  }

  private publishChannelChange(channelId: ChatChannelId | null): void {
    if (channelId) this.publishChanges({ channelIds: [channelId] });
  }

  /** Tells other windows and this window's other panes which Chat records this pane changed. */
  private publishChanges(ids: ChatChangeIds): void {
    const changes = chatWindowChanges(ids);
    publishChatChanges(changes);
    paneStores.notifyOthers(this, changes);
  }

  /** Rereads what another pane of this window changed. Receiving never publishes, so panes do not echo each other. */
  refreshFromOtherPane(changes: ChatWindowChanges): void {
    void this.refreshFromOtherWindow(changes).catch((error: unknown) => {
      console.error("Chat refresh from another pane failed", error);
    });
  }

  /** Starts receiving Chat changes from other windows once per store. */
  private listenForOtherWindows(): void {
    if (this.otherWindowListening) return;
    this.otherWindowListening = true;
    void listenForChatChanges((changes) => {
      void this.refreshFromOtherWindow(changes).catch((error: unknown) => {
        console.error("Chat refresh from another window failed", error);
      });
    }).catch((error: unknown) => {
      this.otherWindowListening = false;
      console.error("Chat could not listen for other windows", error);
    });
  }

  /** Rereads what another window changed. Receiving never publishes, so windows do not echo each other. */
  private async refreshFromOtherWindow(changes: ChatWindowChanges): Promise<void> {
    const vaultGeneration = this.vaultGeneration;
    if (!this.loaded) return;
    const isCurrentVault = () => vaultGeneration === this.vaultGeneration;
    await Promise.all([
      ...[...changes.threadIds].map((threadId) => this.refreshThreadFromOtherWindow(threadId, isCurrentVault)),
      ...[...changes.channelIds].map((channelId) => this.refreshChannelFromOtherWindow(channelId, isCurrentVault)),
    ]);
    const channelId = this.selectedChannelId;
    if (!isCurrentVault() || !channelId || !changes.channelIds.has(channelId)) return;
    this.organizationalController.scheduledMessagesVersion += 1;
    await this.loadChannelMessages(channelId, true);
    if (this.openReplyThreadId && isCurrentVault()) await this.loadReplyThread(this.openReplyThreadId, true);
  }

  private async refreshThreadFromOtherWindow(threadId: ChatThreadId, isCurrentVault: () => boolean): Promise<void> {
    try {
      const thread = await chatApi.readChatThreadShell(threadId);
      if (!isCurrentVault()) return;
      if (thread.workingFolderId !== null && thread.scratchGenerationId === null) {
        this.threadCollectionController.upsert(thread);
      }
    } catch (error: unknown) {
      if (chatErrorCode(error) !== "not_found") throw error;
      if (isCurrentVault()) this.threadCollectionController.remove(threadId);
    }
  }

  private async refreshChannelFromOtherWindow(channelId: ChatChannelId, isCurrentVault: () => boolean): Promise<void> {
    try {
      const channel = await chatApi.readChatChannel(channelId);
      if (isCurrentVault()) this.channelNavigationController.upsert(channel);
    } catch (error: unknown) {
      if (chatErrorCode(error) !== "not_found") throw error;
    }
  }

  async renameThread(thread: ChatThreadShellRead, title: string): Promise<void> {
    return this.threadCollectionController.rename(thread, title);
  }

  async setThreadRead(thread: ChatThreadShellRead, read: boolean): Promise<void> {
    return this.threadCollectionController.setRead(thread, read);
  }

  async forkThread(thread: ChatThreadShellRead, title: string): Promise<void> {
    return this.threadCollectionController.fork(thread, title);
  }

  async archiveThread(thread: ChatThreadShellRead): Promise<void> {
    return this.threadCollectionController.archive(thread);
  }

  async restoreThread(thread: ChatThreadShellRead): Promise<void> {
    return this.threadCollectionController.restore(thread);
  }

  async deleteThread(thread: ChatThreadShellRead): Promise<void> {
    return this.threadCollectionController.delete(thread);
  }

  private async restoreSelection(settings: ChatSettingsRead): Promise<void> {
    const remembered = settings.configuration.behavior.restoreLastSelectedThread
      ? settings.lastSelectedThreadId
      : null;
    if (remembered && ![...this.activeThreads, ...this.archivedThreads].some((thread) => thread.id === remembered)) {
      try {
        const restored = await chatApi.readChatThreadShell(remembered);
        if (restored.workingFolderId !== null && restored.scratchGenerationId === null) {
          this.threadCollectionController.upsert(restored);
        }
      } catch (error: unknown) {
        console.warn("Remembered Chat thread could not be restored", error);
      }
    }
    if (remembered && [...this.activeThreads, ...this.archivedThreads].some((thread) => thread.id === remembered)) {
      this.selectedThreadId = remembered;
      this.selectedWorkingFolderId = this.selectedThread?.workingFolderId ?? null;
      const selectedThread = this.selectedThread;
      if (selectedThread) await this.projects.selectProject(selectedThread.projectId);
      void this.loadTimeline(remembered).catch(() => undefined);
      return;
    }
    if (this.selectedWorkingFolderId && this.workingFolders.some((entry) => (
      entry.workingFolder.id === this.selectedWorkingFolderId
        && entry.workingFolder.projectId === this.projects.selectedProjectId
    ))) return;
    this.selectedThreadId = null;
    const projectId = this.projects.selectedProjectId;
    const rememberedFolderId = projectId && localExecutionAvailable
      ? await workingFolderApi.lastProjectWorkingFolder(projectId)
      : null;
    const selected = projectId
      ? preferredProjectWorkingFolder(this.workingFolders, projectId, rememberedFolderId)
      : null;
    this.selectedWorkingFolderId = selected?.workingFolder.id ?? null;
  }

  async loadChannelMessages(channelId: ChatChannelId, force = false): Promise<void> {
    return this.communicationController.loadChannel(channelId, force);
  }

  async loadOlderChannelMessages(): Promise<void> {
    return this.communicationController.loadOlderChannel();
  }

  async loadReplyThread(replyThreadId: ChatReplyThreadId, force = false): Promise<void> {
    return this.communicationController.loadReplyThread(replyThreadId, force);
  }

  async loadOlderReplyThreadMessages(): Promise<void> {
    return this.communicationController.loadOlderReplyThread();
  }

  private async loadTimeline(
    threadId: ChatThreadId,
    cursor: string | null = null,
    prepend = false,
    selectedSequence: number | null = null,
  ): Promise<void> {
    return this.timelineController.load(threadId, cursor, prepend, selectedSequence);
  }
}

const stores: Partial<Record<WorkspacePaneScope, ChatStore>> = {};

function storeFor(scope: WorkspacePaneScope): ChatStore {
  return stores[scope] ??= new ChatStore(scope);
}

/** The primary pane's Chat store, which app-wide flows such as settings and notifications use. */
export function getChat(): ChatStore {
  return storeFor("primary");
}

/** The Chat store a split pane scope shows through. */
export function getChatStore(scope: WorkspacePaneScope): ChatStore {
  return storeFor(scope);
}

/** The Chat store of the calling view's split pane. Call during component initialization. */
export function useChat(): ChatStore {
  return storeFor(getWorkspacePaneScope());
}

/** Invalidates every pane's Chat state and the shared Chat state before prewarming another vault. */
export function resetChatForVault(): void {
  for (const store of paneStores.all()) store.resetPaneForVault();
  shared.providerDiscoveryStarted = false;
  shared.teammates = [];
  shared.archivedTeammates = [];
  shared.configuration.reset();
}

/** Show a project's newly saved primary working folder in every pane whose channel belongs to that project. */
export function adoptChatProjectPrimaryWorkingFolder(primary: ChatProjectPrimaryWorkingFolderRead): void {
  for (const store of paneStores.all()) store.adoptProjectPrimaryWorkingFolder(primary);
}
