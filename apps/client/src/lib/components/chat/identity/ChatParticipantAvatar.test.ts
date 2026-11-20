// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatAiAgentRead, ChatParticipantRead } from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import { getPreferences } from "$lib/stores/preferences.svelte";
import { modelSettings } from "$lib/components/chat/composer/composer-test-store";
import ChatParticipantAvatar from "./ChatParticipantAvatar.svelte";

const profileImage = vi.hoisted(() => ({
  load: vi.fn(async (relativePath: string) => `https://profile.invalid/${relativePath}`),
}));

vi.mock("$lib/api/profile-image", () => ({
  profileImageAssetUrl: profileImage.load,
}));

const localParticipant: ChatParticipantRead = {
  id: "participant:local-owner",
  kind: "local_user",
  displayName: "You",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

const agentParticipant: ChatParticipantRead = {
  id: "participant:atlas",
  kind: "ai_agent",
  displayName: "Atlas",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

describe("ChatParticipantAvatar", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;
  const preferences = getPreferences();
  const chat = getChat();
  const originalDisplayName = preferences.profileDisplayName;
  const originalImage = preferences.profileImage;
  const originalSettings = chat.settings;
  const originalAgents = chat.agents;
  const originalArchivedAgents = chat.archivedAgents;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    preferences.setProfileDisplayName(originalDisplayName);
    preferences.setProfileImage(originalImage);
    chat.settings = originalSettings;
    chat.agents = originalAgents;
    chat.archivedAgents = originalArchivedAgents;
    profileImage.load.mockClear();
  });

  it("reacts to current local profile names and images without changing the participant", async () => {
    preferences.setProfileDisplayName("Alice Smith");
    preferences.setProfileImage(null);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatParticipantAvatar, {
      target,
      props: { participant: localParticipant, size: 32 },
    });

    expect(target.textContent?.trim()).toBe("AS");

    preferences.setProfileDisplayName("Renamed Person");
    const imagePath = `profile/${"a".repeat(64)}.png`;
    preferences.setProfileImage({ path: imagePath, byteSize: 2048 });
    await tick();
    await vi.waitFor(() => {
      expect(target?.querySelector("img")?.src).toBe(`https://profile.invalid/${imagePath}`);
    });
    expect(profileImage.load).toHaveBeenCalledWith(imagePath);
    expect(localParticipant.displayName).toBe("You");
  });

  it("uses the agent provider identity instead of a generic agent icon", () => {
    chat.settings = modelSettings();
    chat.agents = [{
      participant: agentParticipant,
      providerFamilyId: "codex",
      agentKind: "model",
      localProviderInstanceId: "codex-local",
      configurationState: "healthy",
      latestPolicy: {
        id: "policy:atlas:1",
        agentId: agentParticipant.id,
        revision: 1,
        providerManagedModel: false,
        modelId: "gpt-5.6-sol",
        modelOptions: [],
        interactionMode: "build",
        providerOptions: { schemaVersion: 1, value: {} },
        createdAt: "2026-08-04T12:00:00.000Z",
      },
      channelCount: 1,
      activeAssignmentCount: 0,
      hiddenAt: null,
    } satisfies ChatAiAgentRead];
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatParticipantAvatar, {
      target,
      props: { participant: agentParticipant, size: 32 },
    });

    expect(target.querySelector('.model-avatar[aria-label="OpenAI"]')).not.toBeNull();
    expect(target.querySelector(".provider-icon svg")).not.toBeNull();
    expect(target.querySelector(".agent-fallback")).toBeNull();
  });

  it("shows the provider mark on a device without the agent's provider, such as a phone", () => {
    chat.settings = null;
    chat.agents = [{
      participant: agentParticipant,
      providerFamilyId: "claude",
      agentKind: "line",
      localProviderInstanceId: null,
      configurationState: "provider_unavailable",
      latestPolicy: null,
      channelCount: 1,
      activeAssignmentCount: 0,
      hiddenAt: null,
    } satisfies ChatAiAgentRead];
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatParticipantAvatar, {
      target,
      props: { participant: agentParticipant, size: 32 },
    });

    expect(target.querySelector('.model-avatar[aria-label="Anthropic"]')).not.toBeNull();
    expect(target.querySelector(".agent-fallback")).toBeNull();
  });

  it("uses an explicitly provided archived agent identity", () => {
    chat.settings = modelSettings();
    chat.agents = [];
    const archivedAgent = {
      participant: { ...agentParticipant, archivedAt: "2026-08-14T12:00:00.000Z" },
      providerFamilyId: "codex",
      agentKind: "model",
      localProviderInstanceId: "codex-local",
      configurationState: "healthy",
      latestPolicy: {
        id: "policy:atlas:1",
        agentId: agentParticipant.id,
        revision: 1,
        providerManagedModel: false,
        modelId: "gpt-5.6-sol",
        modelOptions: [],
        interactionMode: "build",
        providerOptions: { schemaVersion: 1, value: {} },
        createdAt: "2026-08-04T12:00:00.000Z",
      },
      channelCount: 1,
      activeAssignmentCount: 0,
      hiddenAt: null,
    } satisfies ChatAiAgentRead;
    chat.archivedAgents = [archivedAgent];
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatParticipantAvatar, {
      target,
      props: {
        participant: archivedAgent.participant,
        size: 32,
      },
    });

    expect(target.querySelector('.model-avatar[aria-label="OpenAI"]')).not.toBeNull();
    expect(target.querySelector(".agent-fallback")).toBeNull();
  });
});
