// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as chatApi from "$lib/api/chat";
import * as workingFolderApi from "$lib/api/project-working-folders";
import type { ChatChannelRead, ChatMessageRead, ChatSettingsRead } from "$lib/chat/contracts";
import { readLastChatChannelId } from "$lib/chat/channel-sections";
import { setActiveVaultIdentity } from "$lib/vault/active-vault";
import { getChat, getChatStore, resetChatForVault } from "./chat.svelte";
import { ChatConfigurationController } from "$lib/stores/chat/configuration-controller.svelte";

const projects = vi.hoisted(() => ({
  selectedProjectId: "project",
  projects: [],
  ensureLoaded: vi.fn(async () => undefined),
  selectProject: vi.fn(async () => undefined),
}));
vi.mock("$lib/stores/projects.svelte", () => ({ getProjects: () => projects }));
vi.mock("$lib/chat/window-sync", () => ({
  listenForChatChanges: vi.fn(async () => () => undefined),
  publishChatChanges: vi.fn(),
}));

/** Creates a controllable external request without timeouts or real provider processes. */
function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  let reject: (error: unknown) => void = () => undefined;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

/** Builds cached settings with no installed providers, as on a new vault. */
function settings(sendKey: "enter" | "mod_enter" = "enter"): ChatSettingsRead {
  return {
    configuration: {
      schemaVersion: 1, providers: [], automaticProviderSetupDisabled: [], rememberedSelections: [],
      workingFolderProviderPreferences: {}, panels: { inspectorWidthPx: 320 },
      behavior: {
        sendKey, restoreLastSelectedThread: true, showReasoningSummaries: true,
        automaticallyFoldSettledWork: true, terminalScrollbackLines: 1_000,
        idleSessionTimeoutSeconds: 600, confirmMultilineTerminalPaste: true,
      },
    },
    providerFamilies: [], providerInstances: [], credentialStoreAvailability: "available",
    lastSelectedThreadId: null,
  };
}

/** Builds navigation for local history that is independent of provider availability. */
function channel(id = "general"): ChatChannelRead {
  return {
    id, conversationId: `conversation-${id}`, projectId: "project", name: id, topic: "",
    isDefault: id === "general", memberships: [], messageCount: 0, unreadCount: 0,
    latestPreview: null, lastActivityAt: "2026-09-22T12:00:00Z", attentionState: null,
    revision: 1, archivedAt: null, createdAt: "2026-09-22T12:00:00Z", updatedAt: "2026-09-22T12:00:00Z",
  };
}

/** Represents a saved message that is readable without a provider. */
function message(channelId: string): ChatMessageRead {
  return {
    itemId: `message-${channelId}`, conversationId: `conversation-${channelId}`,
    replyThreadId: null, revisionId: "revision", revision: 1,
    author: {
      id: "local-user", kind: "local_user", displayName: "User",
      avatar: { schemaVersion: 1, value: {} }, revision: 1, archivedAt: null,
    },
    authorLabelSnapshot: "User", normalizedMarkdown: "Saved local history",
    richContent: { schemaVersion: 1, value: {} }, attachmentIds: [], references: [],
    replyThread: null, ordinal: 1, editedAt: null, createdAt: "2026-09-22T12:00:00Z",
  };
}

const chat = getChat();
let discovery: ReturnType<typeof deferred<ChatSettingsRead>>;

beforeEach(() => {
  discovery = deferred<ChatSettingsRead>();
  resetChatForVault();
  vi.spyOn(chatApi, "readChatSettings").mockResolvedValue(settings());
  vi.spyOn(chatApi, "discoverDefaultChatProviders").mockReturnValue(discovery.promise);
  vi.spyOn(chatApi, "recoverInterruptedChatTurns").mockResolvedValue();
  vi.spyOn(chatApi, "recoverChatAssignmentDispatchJobs").mockResolvedValue(0);
  vi.spyOn(chatApi, "listChatNavigationChannels").mockResolvedValue([channel(), channel("other")]);
  vi.spyOn(chatApi, "listChatTeammates").mockResolvedValue([]);
  vi.spyOn(chatApi, "readChatProjectPrimaryWorkingFolder").mockRejectedValue(new Error("no working folder"));
  vi.spyOn(chatApi, "readChatChannelPage").mockImplementation(async (channelId) => ({
    channelId, messages: [message(channelId)], previousCursor: null, revision: 1,
  }));
  vi.spyOn(workingFolderApi, "listCachedProjectWorkingFolders").mockResolvedValue([]);
  vi.spyOn(workingFolderApi, "listProjectWorkingFolders").mockResolvedValue([]);
  vi.spyOn(workingFolderApi, "lastProjectWorkingFolder").mockResolvedValue(null);
});

afterEach(() => {
  resetChatForVault();
  vi.restoreAllMocks();
});

describe("Chat startup", () => {
  it("prewarms cached history without launching providers, then discovers once when opened", async () => {
    await chat.prewarmForProject("project");
    expect(chat.loaded).toBe(true);
    expect(chat.channelMessages).toEqual([message("general")]);
    expect(chatApi.discoverDefaultChatProviders).not.toHaveBeenCalled();
    await Promise.all([chat.ensureLoaded(), chat.ensureLoaded()]);
    expect(chatApi.discoverDefaultChatProviders).toHaveBeenCalledOnce();
    expect(chat.providerDiscoveryLoading).toBe(true);
  });

  it("loads the selected channel while provider discovery remains unresolved", async () => {
    await chat.ensureLoaded();
    expect(chat.loaded).toBe(true);
    expect(chat.loading).toBe(false);
    expect(chat.providerDiscoveryLoading).toBe(true);
    expect(chat.selectedChannelId).toBe("general");
    expect(chat.channelMessages).toEqual([message("general")]);
    expect(chatApi.recoverInterruptedChatTurns).toHaveBeenCalledOnce();
    expect(chatApi.recoverChatAssignmentDispatchJobs).toHaveBeenCalledOnce();
  });

  it("keeps one draft thread per working folder so a channel's terminal has a scope", async () => {
    await chat.ensureLoaded();
    expect(chat.selectedWorkingFolderId).toBe(null);
    expect(chat.draftThreadId).toBe(null);
    vi.mocked(chatApi.readChatProjectPrimaryWorkingFolder).mockResolvedValue({
      projectId: "project", workingFolderId: "folder", revision: 1,
    });
    await chat.selectChannel("other");
    expect(chat.selectedWorkingFolderId).toBe("folder");
    const draftThreadId = chat.draftThreadId;
    expect(draftThreadId).toEqual(expect.any(String));
    await chat.selectChannel("general");
    expect(chat.draftThreadId).toBe(draftThreadId);
  });

  it("applies later provider settings without resetting a user's channel or draft", async () => {
    await chat.ensureLoaded();
    await chat.selectChannel("other");
    const draft = { ...chat.organizationalDraft("other"), normalizedMarkdown: "Keep this draft" };
    chat.setOrganizationalDraft("other", draft);
    const discovered = settings("mod_enter");
    discovered.providerInstances.push({
      configuration: {
        schemaVersion: 1, instanceId: "provider", familyId: "codex", label: "Codex",
        enabled: true, executable: "codex", providerHome: null, launchArguments: [],
        environment: {}, credentialReferences: {}, visibleModelIds: [], favoriteModelIds: [],
        providerConfig: { schemaVersion: 1, value: {} },
      },
      lastProbe: null, lastSuccessfulProbeAt: null, modelCatalog: null,
    });
    discovery.resolve(discovered);
    await vi.waitFor(() => expect(chat.settings?.configuration.behavior.sendKey).toBe("mod_enter"));
    expect(chat.selectedChannelId).toBe("other");
    expect(chat.settings?.providerInstances).toEqual(discovered.providerInstances);
    expect(chat.organizationalDraft("other")).toEqual(draft);
    expect(chat.providerDiscoveryLoading).toBe(false);
  });

  it("reports discovery and fallback failures while keeping local history usable", async () => {
    const report = vi.spyOn(console, "error").mockImplementation(() => undefined);
    await chat.ensureLoaded();
    const failure = new Error("provider unavailable");
    const refreshFailure = new Error("settings unavailable");
    vi.mocked(chatApi.readChatSettings).mockRejectedValueOnce(refreshFailure);
    discovery.reject(failure);
    await vi.waitFor(() => expect(report).toHaveBeenCalledWith(
      "Chat settings refresh after discovery failed", refreshFailure,
    ));
    expect(report).toHaveBeenCalledWith("Automatic Chat provider discovery failed", failure);
    expect(chat.loaded).toBe(true);
    expect(chat.error).toBeNull();
    expect(chat.selectedChannelId).toBe("general");
    expect(chat.channelPages).toHaveLength(1);
  });

  it("does not start fallback reads for a discovery failure from a previous vault", async () => {
    const report = vi.spyOn(console, "error").mockImplementation(() => undefined);
    await chat.ensureLoaded();
    resetChatForVault();
    discovery.reject(new Error("old discovery failed"));
    await discovery.promise.catch(() => undefined);
    await Promise.resolve();
    expect(chatApi.readChatSettings).toHaveBeenCalledOnce();
    expect(chat.settings).toBeNull();
    expect(report).not.toHaveBeenCalled();
  });

  it("does not apply a fallback settings read after switching vaults", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    await chat.ensureLoaded();
    const fallback = deferred<ChatSettingsRead>();
    vi.mocked(chatApi.readChatSettings).mockReturnValueOnce(fallback.promise);
    discovery.reject(new Error("provider failed"));
    await vi.waitFor(() => expect(chatApi.readChatSettings).toHaveBeenCalledTimes(2));
    resetChatForVault();
    chat.settings = settings();
    fallback.resolve(settings("mod_enter"));
    await fallback.promise;
    await Promise.resolve();
    expect(chat.settings?.configuration.behavior.sendKey).toBe("enter");
  });
});

describe("Chat configuration startup requests", () => {
  it("shares discovery and applies its result only for the latest load", async () => {
    const changed = vi.fn();
    const controller = new ChatConfigurationController({ onSettingsChanged: changed });
    let current = 1;
    const first = controller.discoverProviders(() => current === 1);
    current = 2;
    controller.hydrate(settings(), []);
    changed.mockClear();
    const second = controller.discoverProviders(() => current === 2);
    expect(chatApi.discoverDefaultChatProviders).toHaveBeenCalledOnce();
    discovery.resolve(settings("mod_enter"));
    await Promise.all([first, second]);
    expect(changed).toHaveBeenCalledOnce();
    expect(controller.settings?.configuration.behavior.sendKey).toBe("mod_enter");
    expect(controller.providerDiscoveryLoading).toBe(false);
  });

  it("ignores superseded discovery before the next load has hydrated", async () => {
    const controller = new ChatConfigurationController({ onSettingsChanged: vi.fn() });
    controller.hydrate(settings(), []);
    let current = true;
    const first = controller.discoverProviders(() => current);
    current = false;
    discovery.resolve(settings("mod_enter"));
    await first;
    expect(controller.settings?.configuration.behavior.sendKey).toBe("enter");
  });

  it("does not replace newer settings with an earlier discovery result", async () => {
    const controller = new ChatConfigurationController({ onSettingsChanged: vi.fn() });
    const pending = controller.discoverProviders();
    controller.setSettings(settings("mod_enter"));
    discovery.resolve(settings());
    await pending;
    expect(controller.settings?.configuration.behavior.sendKey).toBe("mod_enter");
  });

  it("keeps a new vault's discovery active when old discovery finishes", async () => {
    const controller = new ChatConfigurationController({ onSettingsChanged: vi.fn() });
    const first = controller.discoverProviders();
    controller.reset();
    const next = deferred<ChatSettingsRead>();
    vi.mocked(chatApi.discoverDefaultChatProviders).mockReturnValueOnce(next.promise);
    const second = controller.discoverProviders();
    discovery.resolve(settings("mod_enter"));
    await first;
    expect(controller.settings).toBeNull();
    expect(controller.providerDiscoveryLoading).toBe(true);
    next.resolve(settings());
    await second;
    expect(controller.settings?.configuration.behavior.sendKey).toBe("enter");
    expect(controller.providerDiscoveryLoading).toBe(false);
  });

  it("allows an explicit retry after discovery failure", async () => {
    const controller = new ChatConfigurationController({ onSettingsChanged: vi.fn() });
    const first = controller.discoverProviders();
    discovery.reject(new Error("probe failed"));
    await expect(first).rejects.toThrow("probe failed");
    expect(controller.providerDiscoveryLoading).toBe(false);
    vi.mocked(chatApi.discoverDefaultChatProviders).mockResolvedValueOnce(settings());
    await controller.discoverProviders();
    expect(chatApi.discoverDefaultChatProviders).toHaveBeenCalledTimes(2);
    expect(controller.settings).toEqual(settings());
  });

  it("ignores fallback settings when its load is superseded", async () => {
    const controller = new ChatConfigurationController({ onSettingsChanged: vi.fn() });
    controller.hydrate(settings(), []);
    const fallback = deferred<ChatSettingsRead>();
    vi.mocked(chatApi.readChatSettings).mockReturnValueOnce(fallback.promise);
    let current = true;
    const pending = controller.refreshSettings(() => current);
    current = false;
    fallback.resolve(settings("mod_enter"));
    await pending;
    expect(controller.settings?.configuration.behavior.sendKey).toBe("enter");
  });
});

describe("Chat stores of split panes", () => {
  const secondary = getChatStore("secondary");

  beforeEach(() => {
    localStorage.clear();
    setActiveVaultIdentity("vault-a");
  });

  afterEach(() => {
    setActiveVaultIdentity(null);
  });

  it("loads the secondary pane after the primary without loading shared state twice", async () => {
    await secondary.ensureLoaded();
    expect(chat.loaded).toBe(true);
    expect(secondary.loaded).toBe(true);
    expect(secondary.selectedChannelId).toBe("general");
    expect(secondary.settings).toBe(chat.settings);
    expect(chatApi.readChatSettings).toHaveBeenCalledOnce();
    expect(chatApi.recoverInterruptedChatTurns).toHaveBeenCalledOnce();
    expect(chatApi.recoverChatAssignmentDispatchJobs).toHaveBeenCalledOnce();
    await chat.ensureLoaded();
    expect(chatApi.discoverDefaultChatProviders).toHaveBeenCalledOnce();
  });

  it("keeps each pane's channel and remembers only the primary pane's for the next launch", async () => {
    await secondary.ensureLoaded();
    await secondary.selectChannel("other");
    expect(secondary.selectedChannelId).toBe("other");
    expect(chat.selectedChannelId).toBe("general");
    expect(readLastChatChannelId("project")).not.toBe("other");
    await chat.selectChannel("other");
    expect(readLastChatChannelId("project")).toBe("other");
  });

  it("rereads a channel in the other pane after a change through one pane", async () => {
    await secondary.ensureLoaded();
    vi.spyOn(chatApi, "setChatChannelRead").mockResolvedValue(channel());
    vi.spyOn(chatApi, "readChatChannel").mockResolvedValue(channel());
    const pageReads = vi.mocked(chatApi.readChatChannelPage).mock.calls.length;
    const scheduledVersion = chat.scheduledMessagesVersion;
    secondary.activeChannels = [{ ...channel(), unreadCount: 1 }, channel("other")];
    await secondary.markSelectedChannelRead();
    expect(chatApi.readChatChannel).toHaveBeenCalledWith("general");
    await vi.waitFor(() => expect(chat.scheduledMessagesVersion).toBe(scheduledVersion + 1));
    await vi.waitFor(() => expect(chatApi.readChatChannelPage).toHaveBeenCalledTimes(pageReads + 1));
    expect(secondary.scheduledMessagesVersion).toBe(scheduledVersion);
  });

  it("applies changed shared settings to every pane's composer defaults", async () => {
    await secondary.ensureLoaded();
    const primaryDefaults = vi.spyOn(chat, "scheduleComposerDefaults");
    const secondaryDefaults = vi.spyOn(secondary, "scheduleComposerDefaults");
    chat.settings = settings("mod_enter");
    expect(primaryDefaults).toHaveBeenCalledOnce();
    expect(secondaryDefaults).toHaveBeenCalledOnce();
    expect(secondary.settings?.configuration.behavior.sendKey).toBe("mod_enter");
  });

  it("resets every pane and the shared state for another vault", async () => {
    await secondary.ensureLoaded();
    resetChatForVault();
    expect(chat.loaded).toBe(false);
    expect(secondary.loaded).toBe(false);
    expect(secondary.selectedChannelId).toBeNull();
    expect(secondary.settings).toBeNull();
  });
});
