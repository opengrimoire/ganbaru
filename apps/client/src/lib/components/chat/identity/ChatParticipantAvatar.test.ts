// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatAiTeammateRead, ChatParticipantRead } from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import { getPreferences } from "$lib/stores/preferences.svelte";
import { modelSettings } from "$lib/components/chat/ChatComposer.test-support";
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
  kind: "ai_teammate",
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
  const originalImagePath = preferences.profileImagePath;
  const originalSettings = chat.settings;
  const originalTeammates = chat.teammates;
  const originalArchivedTeammates = chat.archivedTeammates;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    preferences.setProfileDisplayName(originalDisplayName);
    preferences.setProfileImagePath(originalImagePath);
    chat.settings = originalSettings;
    chat.teammates = originalTeammates;
    chat.archivedTeammates = originalArchivedTeammates;
    profileImage.load.mockClear();
  });

  it("reacts to current local profile names and images without changing the participant", async () => {
    preferences.setProfileDisplayName("Alice Smith");
    preferences.setProfileImagePath(null);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatParticipantAvatar, {
      target,
      props: { participant: localParticipant, size: 32 },
    });

    expect(target.textContent?.trim()).toBe("AS");

    preferences.setProfileDisplayName("Renamed Person");
    const imagePath = `profile/${"a".repeat(64)}.png`;
    preferences.setProfileImagePath(imagePath);
    await tick();
    await vi.waitFor(() => {
      expect(target?.querySelector("img")?.src).toBe(`https://profile.invalid/${imagePath}`);
    });
    expect(profileImage.load).toHaveBeenCalledWith(imagePath);
    expect(localParticipant.displayName).toBe("You");
  });

  it("uses the teammate provider identity instead of a generic agent icon", () => {
    chat.settings = modelSettings();
    chat.teammates = [{
      participant: agentParticipant,
      role: "Coding",
      instructions: "",
      configurationState: "healthy",
      latestPolicy: {
        id: "policy:atlas:1",
        teammateId: agentParticipant.id,
        revision: 1,
        providerInstanceId: "codex-local",
        safetyMode: "ask_for_approval",
        providerManagedModel: false,
        modelId: "gpt-5.6-sol",
        modelOptions: [],
        effort: "medium",
        speed: null,
        providerOptions: { schemaVersion: 1, value: {} },
        createdAt: "2026-08-04T12:00:00.000Z",
      },
      channelCount: 1,
      activeAssignmentCount: 0,
      hasDurableHistory: false,
    } satisfies ChatAiTeammateRead];
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

  it("uses an explicitly provided archived teammate identity", () => {
    chat.settings = modelSettings();
    chat.teammates = [];
    const archivedTeammate = {
      participant: { ...agentParticipant, archivedAt: "2026-08-14T12:00:00.000Z" },
      role: "Coding",
      instructions: "",
      configurationState: "healthy",
      latestPolicy: {
        id: "policy:atlas:1",
        teammateId: agentParticipant.id,
        revision: 1,
        providerInstanceId: "codex-local",
        safetyMode: "ask_for_approval",
        providerManagedModel: false,
        modelId: "gpt-5.6-sol",
        modelOptions: [],
        effort: "medium",
        speed: null,
        providerOptions: { schemaVersion: 1, value: {} },
        createdAt: "2026-08-04T12:00:00.000Z",
      },
      channelCount: 1,
      activeAssignmentCount: 0,
      hasDurableHistory: true,
    } satisfies ChatAiTeammateRead;
    chat.archivedTeammates = [archivedTeammate];
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatParticipantAvatar, {
      target,
      props: {
        participant: archivedTeammate.participant,
        size: 32,
      },
    });

    expect(target.querySelector('.model-avatar[aria-label="OpenAI"]')).not.toBeNull();
    expect(target.querySelector(".agent-fallback")).toBeNull();
  });
});
