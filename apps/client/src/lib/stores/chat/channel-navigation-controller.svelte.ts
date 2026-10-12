import * as chatApi from "$lib/api/chat";
import type {
  ChatChannelId,
  ChatChannelRead,
  CreateChatChannelRequest,
} from "$lib/chat/contracts";
import { chatErrorMessage } from "$lib/chat/error-presentation";

export interface ChatChannelNavigationControllerOptions {
  currentProjectId: () => string | null;
  selectedChannelId: () => ChatChannelId | null;
  selectChannel: (channelId: ChatChannelId) => Promise<void>;
}

/** Owns project channel collections, archive loading, and channel mutations. */
export class ChatChannelNavigationController {
  activeChannels = $state<ChatChannelRead[]>([]);
  archivedChannels = $state<ChatChannelRead[]>([]);
  archivedChannelsLoading = $state(false);
  archivedChannelsError = $state<string | null>(null);
  channelsLoading = $state(true);
  archiveOpen = $state(false);

  private navigationChannels: ChatChannelRead[] = [];
  private loadRequest = 0;
  private archivedProjectId: string | null = null;

  constructor(private readonly options: ChatChannelNavigationControllerOptions) {}

  reset(): void {
    this.loadRequest += 1;
    this.navigationChannels = [];
    this.activeChannels = [];
    this.archivedChannels = [];
    this.archivedProjectId = null;
    this.archivedChannelsLoading = false;
    this.archivedChannelsError = null;
    this.channelsLoading = true;
    this.archiveOpen = false;
  }

  hydrateNavigation(channels: ChatChannelRead[]): void {
    this.navigationChannels = channels;
    this.activeChannels = [];
    this.archivedChannels = [];
    this.archivedProjectId = null;
    this.archivedChannelsError = null;
  }

  channelsForProject(projectId: string): ChatChannelRead[] {
    const channels = new Map(
      this.navigationChannels
        .filter((channel) => channel.projectId === projectId)
        .map((channel) => [channel.id, channel]),
    );
    for (const channel of this.activeChannels) {
      if (channel.projectId === projectId) channels.set(channel.id, channel);
    }
    return [...channels.values()].sort(compareChannels);
  }

  find(channelId: ChatChannelId): ChatChannelRead | null {
    return [...this.activeChannels, ...this.archivedChannels]
      .find((channel) => channel.id === channelId) ?? null;
  }

  async loadProject(projectId: string): Promise<boolean> {
    const request = ++this.loadRequest;
    this.channelsLoading = true;
    this.activeChannels = [];
    this.archivedChannels = [];
    this.archivedProjectId = null;
    this.archivedChannelsError = null;
    try {
      const cached = this.channelsForProject(projectId);
      const active = cached.length > 0
        ? cached
        : await chatApi.listChatChannels(projectId, false);
      if (request !== this.loadRequest) return false;
      this.activeChannels = active;
      if (cached.length === 0) this.mergeNavigation(active);
      return true;
    } finally {
      if (request === this.loadRequest) this.channelsLoading = false;
    }
  }

  async loadArchived(projectId: string): Promise<void> {
    if (this.archivedProjectId === projectId) return;
    this.archivedChannelsLoading = true;
    this.archivedChannelsError = null;
    try {
      const archived = await chatApi.listChatChannels(projectId, true);
      if (this.options.currentProjectId() !== projectId) return;
      this.archivedChannels = archived;
      this.archivedProjectId = projectId;
    } catch (error: unknown) {
      if (this.options.currentProjectId() === projectId) {
        this.archivedChannelsError = chatErrorMessage(
          error,
          "Archived Chat channels could not be loaded",
        );
      }
      throw error;
    } finally {
      if (this.options.currentProjectId() === projectId) this.archivedChannelsLoading = false;
    }
  }

  async create(request: CreateChatChannelRequest): Promise<ChatChannelRead> {
    const channel = await chatApi.createChatChannel(request);
    this.upsert(channel);
    await this.options.selectChannel(channel.id);
    return channel;
  }

  async updateDetails(
    channel: ChatChannelRead,
    name: string,
    topic: string,
  ): Promise<ChatChannelRead> {
    const updated = await chatApi.updateChatChannelDetails({
      channelId: channel.id,
      name,
      topic,
      expectedRevision: channel.revision,
    });
    this.upsert(updated);
    return updated;
  }

  async archive(channel: ChatChannelRead): Promise<void> {
    const archived = await chatApi.archiveChatChannel(channel.id, channel.revision);
    this.upsert(archived);
    if (this.options.selectedChannelId() !== channel.id) return;
    const fallback = this.activeChannels.find((entry) => entry.isDefault)
      ?? this.activeChannels[0];
    if (fallback) await this.options.selectChannel(fallback.id);
  }

  async restore(channel: ChatChannelRead): Promise<void> {
    this.upsert(await chatApi.restoreChatChannel(channel.id, channel.revision));
  }

  openArchive(projectId: string | null): void {
    this.archiveOpen = true;
    if (projectId) void this.loadArchived(projectId).catch(() => undefined);
  }

  closeArchive(): void {
    this.archiveOpen = false;
  }

  upsert(channel: ChatChannelRead): void {
    this.mergeNavigation([channel]);
    const target = channel.archivedAt ? this.archivedChannels : this.activeChannels;
    const next = target.some((entry) => entry.id === channel.id)
      ? target.map((entry) => entry.id === channel.id ? channel : entry)
      : [...target, channel];
    if (channel.archivedAt) {
      this.activeChannels = this.activeChannels.filter((entry) => entry.id !== channel.id);
      this.archivedChannels = next.sort(compareChannels);
    } else {
      this.archivedChannels = this.archivedChannels.filter((entry) => entry.id !== channel.id);
      this.activeChannels = next.sort(compareChannels);
    }
  }

  private mergeNavigation(channels: readonly ChatChannelRead[]): void {
    const byId = new Map(this.navigationChannels.map((channel) => [channel.id, channel]));
    for (const channel of channels) {
      if (channel.archivedAt) byId.delete(channel.id);
      else byId.set(channel.id, channel);
    }
    this.navigationChannels = [...byId.values()];
  }
}

function compareChannels(left: ChatChannelRead, right: ChatChannelRead): number {
  if (left.isDefault !== right.isDefault) return left.isDefault ? -1 : 1;
  return left.name.localeCompare(right.name) || left.id.localeCompare(right.id);
}
