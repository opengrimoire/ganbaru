// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  ChatAiTeammateRead,
  ChatChannelRead,
  ChatParticipantRead,
  ChatRuntimeApprovalPolicy,
} from "$lib/chat/contracts";
import type { ChatModelParticipant } from "$lib/chat/teammates/participant-identity";
import { getChat } from "$lib/stores/chat.svelte";
import { getPreferences } from "$lib/stores/preferences.svelte";
import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
import { modelSettings } from "$lib/components/chat/ChatComposer.test-support";
import ChatIdentityButton from "./ChatIdentityButton.svelte";

const localParticipant: ChatParticipantRead = {
  id: "participant:local-owner",
  kind: "local_user",
  displayName: "You",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

const model: ChatModelParticipant = {
  displayName: "GPT 5.6 Sol",
  company: { id: "openai", name: "OpenAI", iconFamilyId: "codex", order: 0 },
  providerFamilyId: "codex",
  modelId: "gpt-5.6-sol",
  defaultReasoning: "Medium",
};

const teammateParticipant: ChatParticipantRead = {
  id: "participant:atlas",
  kind: "ai_teammate",
  displayName: "Atlas",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

const teammate: ChatAiTeammateRead = {
  participant: teammateParticipant,
  role: "Coding teammate",
  instructions: "",
  configurationState: "healthy",
  latestPolicy: {
    id: "policy:atlas:1",
    teammateId: teammateParticipant.id,
    revision: 1,
    providerInstanceId: "codex-local",
    safetyMode: "ask_for_approval",
    providerManagedModel: false,
    modelId: "gpt-5.6-terra",
    modelOptions: [{ key: "service_tier", value: { kind: "choice", value: "fast" } }],
    effort: "low",
    speed: "fast",
    providerOptions: { schemaVersion: 1, value: {} },
    createdAt: "2026-08-16T12:00:00.000Z",
  },
  channelCount: 1,
  activeAssignmentCount: 0,
  hasDurableHistory: false,
};

function channel(runtimeApprovalOverride: ChatRuntimeApprovalPolicy): ChatChannelRead {
  return {
    id: "channel:general",
    conversationId: "conversation:general",
    projectId: "project:atlas",
    name: "general",
    topic: "",
    isDefault: true,
    memberships: [{
      conversationId: "conversation:general",
      participant: teammateParticipant,
      aiAccess: {
        accessProfileId: "access-profile:conversation-only",
        accessProfileRevision: 1,
        accessProfileBuiltinKey: "conversationOnly",
        accessProfileName: "Conversation only",
        capabilities: { readHistory: false, participate: true },
        historyBoundary: { kind: "entire" },
        runtimeApprovalOverride,
        scratchRuntimeApprovalOverride: null,
        folderGrants: [],
      },
      revision: 1,
      removedAt: null,
    }],
    messageCount: 0,
    unreadCount: 0,
    latestPreview: null,
    lastActivityAt: "2026-08-16T12:00:00.000Z",
    attentionState: null,
    revision: 1,
    archivedAt: null,
    createdAt: "2026-08-16T12:00:00.000Z",
    updatedAt: "2026-08-16T12:00:00.000Z",
  };
}

describe("ChatIdentityButton", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;
  const preferences = getPreferences();
  const chat = getChat();
  const settings = getSettingsLauncher();
  const originalDisplayName = preferences.profileDisplayName;
  const originalSettings = chat.settings;
  const originalTeammates = chat.teammates;
  const originalArchivedTeammates = chat.archivedTeammates;
  const originalActiveChannels = chat.activeChannels;
  const originalSelectedChannelId = chat.selectedChannelId;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    preferences.setProfileDisplayName(originalDisplayName);
    chat.settings = originalSettings;
    chat.teammates = originalTeammates;
    chat.archivedTeammates = originalArchivedTeammates;
    chat.activeChannels = originalActiveChannels;
    chat.selectedChannelId = originalSelectedChannelId;
    settings.close();
    vi.useRealTimers();
  });

  it("opens local identity details and links to profile settings", async () => {
    preferences.setProfileDisplayName("Alice Smith");
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatIdentityButton, {
      target,
      props: { participant: localParticipant, presentation: "name" },
    });

    target.querySelector<HTMLButtonElement>(".identity-trigger")?.click();
    await tick();
    const card = document.body.querySelector<HTMLElement>(".identity-card");
    expect(card?.textContent).toContain("Alice Smith");
    expect(card?.textContent).toContain("Private local identity");

    card?.querySelector<HTMLButtonElement>("footer button")?.click();
    expect(settings.isOpen).toBe(true);
    expect(settings.targetSection).toBe("profile");
  });

  it.each(["name", "mention"] as const)("uses the same teammate identity card from a %s trigger", async (presentation) => {
    chat.settings = modelSettings();
    chat.teammates = [teammate];
    chat.activeChannels = [channel("unattended")];
    chat.selectedChannelId = "channel:general";
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatIdentityButton, {
      target,
      props: {
        participant: teammateParticipant,
        model,
        presentation,
      },
    });

    target.querySelector<HTMLButtonElement>(".identity-trigger")?.click();
    await tick();
    const card = document.body.querySelector<HTMLElement>(".identity-card");
    expect(card?.querySelector(".identity-heading strong")?.textContent).toBe("Atlas");
    expect(card?.querySelector(".identity-heading small")?.textContent).toBe("Coding teammate");
    expect(card?.querySelector(".identity-status-dot")?.getAttribute("aria-label")).toBe("Available");
    expect(card?.querySelector(".identity-settings-label")?.textContent).toBe("Default settings");
    expect(card?.querySelector(".identity-model-name")?.textContent).toBe("5.6 Terra");
    expect(card?.querySelector(".identity-effort-name")?.textContent).toBe("Light");
    expect(card?.querySelector(".identity-model-row .provider-icon")).toBeNull();
    expect(card?.querySelector(".identity-fast-indicator svg")).not.toBeNull();
    expect(card?.querySelector(".identity-model-divider")?.textContent).toBe("|");
    expect(card?.querySelector(".identity-approval")?.textContent).toBe("Unattended within access");
    expect(card?.querySelector(".identity-approval svg")).toBeNull();
    expect(card?.querySelector("dl")).toBeNull();
    expect(card?.textContent).not.toContain("gpt-5.6-sol");
    expect(card?.textContent).not.toContain("Available");
  });

  it("omits the fast icon for standard speed", async () => {
    chat.settings = modelSettings();
    chat.teammates = [{
      ...teammate,
      latestPolicy: teammate.latestPolicy
        ? { ...teammate.latestPolicy, speed: "standard" }
        : null,
    }];
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatIdentityButton, {
      target,
      props: { participant: teammateParticipant, presentation: "name", currentResponseSettings: true },
    });

    target.querySelector<HTMLButtonElement>(".identity-trigger")?.click();
    await tick();

    expect(document.body.querySelector(".identity-fast-indicator")).toBeNull();
    expect(document.body.querySelector(".identity-settings-label")?.textContent).toBe("Current response settings");
  });

  it.each(["name", "mention"] as const)("left-aligns the %s card with its trigger", async (presentation) => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatIdentityButton, {
      target,
      props: { participant: localParticipant, presentation },
    });
    const trigger = target.querySelector<HTMLButtonElement>(".identity-trigger");
    vi.spyOn(trigger as HTMLButtonElement, "getBoundingClientRect").mockReturnValue(new DOMRect(120, 80, 96, 24));

    trigger?.click();
    await tick();
    await tick();

    expect(document.body.querySelector<HTMLElement>(".identity-card")?.style.left).toBe("120px");
  });

  it("previews identity on deliberate hover and closes after leaving", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatIdentityButton, {
      target,
      props: { participant: localParticipant, presentation: "mention", triggerLabel: "@you" },
    });
    await tick();
    const trigger = target.querySelector<HTMLButtonElement>(".identity-trigger");

    trigger?.dispatchEvent(new MouseEvent("pointerenter", { bubbles: true }));
    await new Promise((resolve) => setTimeout(resolve, 200));
    await tick();
    expect(document.body.querySelector(".identity-card")).not.toBeNull();

    trigger?.dispatchEvent(new MouseEvent("pointerleave", { bubbles: true }));
    await new Promise((resolve) => setTimeout(resolve, 160));
    await tick();
    expect(document.body.querySelector(".identity-card")).toBeNull();
  });
});
