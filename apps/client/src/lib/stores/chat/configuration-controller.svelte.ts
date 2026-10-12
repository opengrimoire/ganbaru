import * as chatApi from "$lib/api/chat";
import * as workingFolderApi from "$lib/api/project-working-folders";
import type {
  ChatBehaviorPreferences,
  ChatProjectPrimaryWorkingFolderRead,
  ChatSettingsRead,
  CreateProjectWorkingFolderRequest,
  ModelId,
  ProjectWorkingFolderId,
  ProjectWorkingFolderRead,
  ProviderInstanceConfig,
  ProviderInstanceId,
  ProviderInstanceRead,
  ProviderProbeResult,
  ProviderRefreshResult,
  ProviderSetupTestRead,
  RemoveProviderResult,
} from "$lib/chat/contracts";
import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";

const localExecutionAvailable = platformHasCapability(
  BUILD_PLATFORM_PROFILE,
  "chat.local-execution",
);

export interface ChatConfigurationControllerOptions {
  onSettingsChanged: () => void;
}

/** Owns Chat provider configuration and project working-folder persistence. */
export class ChatConfigurationController {
  settings = $state<ChatSettingsRead | null>(null);
  workingFolders = $state<ProjectWorkingFolderRead[]>([]);
  primaryWorkingFolder = $state<ChatProjectPrimaryWorkingFolderRead | null>(null);
  providerDiscoveryLoading = $state(false);

  private generation = 0;
  private settingsRequest = 0;
  private providerDiscoveryPromise: Promise<ChatSettingsRead> | null = null;
  private workingFolderRefreshPromise: Promise<void> | null = null;

  constructor(private readonly options: ChatConfigurationControllerOptions) {}

  reset(): void {
    this.generation += 1;
    this.settings = null;
    this.workingFolders = [];
    this.primaryWorkingFolder = null;
    this.providerDiscoveryPromise = null;
    this.workingFolderRefreshPromise = null;
    this.providerDiscoveryLoading = false;
    this.options.onSettingsChanged();
  }

  hydrate(settings: ChatSettingsRead, workingFolders: ProjectWorkingFolderRead[]): void {
    this.setSettings(settings);
    this.workingFolders = workingFolders;
  }

  setSettings(settings: ChatSettingsRead | null): void {
    this.settingsRequest += 1;
    this.settings = settings;
    this.options.onSettingsChanged();
  }

  /** Refreshes settings without applying a response after a vault or load change. */
  async refreshSettings(isCurrent: () => boolean = () => true): Promise<void> {
    const generation = this.generation;
    const request = ++this.settingsRequest;
    const settings = await chatApi.readChatSettings();
    if (generation === this.generation && request === this.settingsRequest && isCurrent()) {
      this.setSettings(settings);
    }
  }

  /** Shares discovery work while only the current load may apply its settings. */
  async discoverProviders(isCurrent: () => boolean = () => true): Promise<void> {
    const generation = this.generation;
    const request = ++this.settingsRequest;
    if (!this.providerDiscoveryPromise) {
      this.providerDiscoveryLoading = true;
      const discovery = chatApi.discoverDefaultChatProviders().finally(() => {
        if (this.providerDiscoveryPromise !== discovery) return;
        this.providerDiscoveryLoading = false;
        this.providerDiscoveryPromise = null;
      });
      this.providerDiscoveryPromise = discovery;
    }
    const settings = await this.providerDiscoveryPromise;
    if (generation === this.generation && request === this.settingsRequest && isCurrent()) {
      this.setSettings(settings);
    }
  }

  async refreshWorkingFolders(): Promise<void> {
    if (!localExecutionAvailable) return;
    if (this.workingFolderRefreshPromise) return this.workingFolderRefreshPromise;
    const generation = this.generation;
    const refresh = workingFolderApi.listProjectWorkingFolders()
      .then((workingFolders) => {
        if (generation !== this.generation) return;
        for (const workingFolder of workingFolders) this.upsertWorkingFolder(workingFolder);
      })
      .finally(() => {
        if (this.workingFolderRefreshPromise === refresh) {
          this.workingFolderRefreshPromise = null;
        }
      });
    this.workingFolderRefreshPromise = refresh;
    return refresh;
  }

  async readPrimaryWorkingFolder(projectId: string): Promise<void> {
    if (!localExecutionAvailable) {
      this.primaryWorkingFolder = null;
      return;
    }
    this.primaryWorkingFolder = await chatApi.readChatProjectPrimaryWorkingFolder(projectId);
  }

  async saveProvider(configuration: ProviderInstanceConfig): Promise<ProviderInstanceRead> {
    const provider = await chatApi.saveChatProvider(configuration);
    await this.refreshSettings();
    return provider;
  }

  testProvider(configuration: ProviderInstanceConfig): Promise<ProviderSetupTestRead> {
    return chatApi.testChatProvider(configuration);
  }

  async probeProvider(instanceId: ProviderInstanceId): Promise<ProviderProbeResult> {
    const result = await chatApi.probeChatProvider(instanceId);
    await this.refreshSettings();
    return result;
  }

  async refreshAllProviders(): Promise<ProviderRefreshResult> {
    const result = await chatApi.refreshAllChatProviders();
    await this.refreshSettings();
    return result;
  }

  async setProviderEnabled(instanceId: ProviderInstanceId, enabled: boolean): Promise<void> {
    await chatApi.setChatProviderEnabled(instanceId, enabled);
    await this.refreshSettings();
  }

  async removeProvider(instanceId: ProviderInstanceId): Promise<RemoveProviderResult> {
    const result = await chatApi.removeChatProvider(instanceId);
    await this.refreshSettings();
    return result;
  }

  async refreshModels(instanceId: ProviderInstanceId): Promise<void> {
    await chatApi.refreshChatProviderModels(instanceId);
    await this.refreshSettings();
  }

  async updateModels(
    instanceId: ProviderInstanceId,
    visible: ModelId[],
    favorites: ModelId[],
  ): Promise<void> {
    await chatApi.updateChatProviderModels(instanceId, visible, favorites);
    await this.refreshSettings();
  }

  async updateBehavior(behavior: ChatBehaviorPreferences): Promise<void> {
    await chatApi.updateChatBehavior(behavior);
    await this.refreshSettings();
  }

  async addExternalWorkingFolder(
    request: CreateProjectWorkingFolderRequest,
    pickerTitle: string,
  ): Promise<ProjectWorkingFolderRead | null> {
    const workingFolder = await workingFolderApi.addExternalProjectWorkingFolder(
      request,
      pickerTitle,
    );
    if (workingFolder) this.upsertWorkingFolder(workingFolder);
    return workingFolder;
  }

  async locateWorkingFolder(
    workingFolderId: ProjectWorkingFolderId,
    pickerTitle: string,
  ): Promise<void> {
    const workingFolder = await workingFolderApi.locateProjectWorkingFolder(
      workingFolderId,
      pickerTitle,
    );
    if (workingFolder) this.upsertWorkingFolder(workingFolder);
  }

  async rebindWorkingFolder(
    workingFolderId: ProjectWorkingFolderId,
    pickerTitle: string,
  ): Promise<void> {
    const workingFolder = await workingFolderApi.rebindProjectWorkingFolder(
      workingFolderId,
      pickerTitle,
    );
    if (workingFolder) this.upsertWorkingFolder(workingFolder);
  }

  async unbindWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    this.upsertWorkingFolder(await workingFolderApi.unbindProjectWorkingFolder(workingFolderId));
  }

  openWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    return workingFolderApi.openProjectWorkingFolder(workingFolderId);
  }

  async recreateManagedWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    this.upsertWorkingFolder(
      await workingFolderApi.recreateManagedProjectWorkingFolder(workingFolderId),
    );
  }

  async renameWorkingFolder(
    workingFolderId: ProjectWorkingFolderId,
    displayName: string,
  ): Promise<void> {
    const current = this.findWorkingFolder(workingFolderId);
    this.upsertWorkingFolder(await workingFolderApi.renameProjectWorkingFolder(
      workingFolderId,
      displayName,
      current.workingFolder.revision,
    ));
  }

  async archiveWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    const current = this.findWorkingFolder(workingFolderId);
    this.upsertWorkingFolder(await workingFolderApi.archiveProjectWorkingFolder(
      workingFolderId,
      current.workingFolder.revision,
    ));
  }

  async restoreWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    const current = this.findWorkingFolder(workingFolderId);
    this.upsertWorkingFolder(await workingFolderApi.restoreProjectWorkingFolder(
      workingFolderId,
      current.workingFolder.revision,
    ));
  }

  async removeWorkingFolder(workingFolderId: ProjectWorkingFolderId): Promise<void> {
    await workingFolderApi.removeProjectWorkingFolder(workingFolderId);
    this.workingFolders = this.workingFolders.filter(
      (entry) => entry.workingFolder.id !== workingFolderId,
    );
  }

  async setWorkingFolderProviderPreference(
    workingFolderId: ProjectWorkingFolderId,
    instanceId: ProviderInstanceId | null,
  ): Promise<void> {
    await chatApi.setChatWorkingFolderProviderPreference(workingFolderId, instanceId);
    await this.refreshSettings();
  }

  async setProjectPrimaryWorkingFolder(
    projectId: string,
    workingFolderId: ProjectWorkingFolderId,
  ): Promise<void> {
    if (!this.primaryWorkingFolder) {
      throw new Error("Project primary working folder was not loaded");
    }
    this.primaryWorkingFolder = await chatApi.setChatProjectPrimaryWorkingFolder(
      projectId,
      workingFolderId,
      this.primaryWorkingFolder.revision,
    );
  }

  findWorkingFolder(workingFolderId: ProjectWorkingFolderId): ProjectWorkingFolderRead {
    const workingFolder = this.workingFolders.find(
      (entry) => entry.workingFolder.id === workingFolderId,
    );
    if (!workingFolder) throw new Error("Project working folder was not found");
    return workingFolder;
  }

  upsertWorkingFolder(workingFolder: ProjectWorkingFolderRead): void {
    const exists = this.workingFolders.some(
      (entry) => entry.workingFolder.id === workingFolder.workingFolder.id,
    );
    this.workingFolders = exists
      ? this.workingFolders.map((entry) => (
          entry.workingFolder.id === workingFolder.workingFolder.id ? workingFolder : entry
        ))
      : [...this.workingFolders, workingFolder];
  }
}
