import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatBehaviorPreferences, ChatSettingsRead } from "$lib/chat/contracts";
import { ChatConfigurationController } from "./configuration-controller.svelte";

const mocks = vi.hoisted(() => ({
  readChatSettings: vi.fn<() => Promise<ChatSettingsRead>>(),
  updateChatBehavior: vi.fn<(behavior: ChatBehaviorPreferences) => Promise<unknown>>(),
  announceConfigSaved: vi.fn<(keys: readonly string[]) => Promise<void>>(),
  listeners: new Set<(keys: ReadonlySet<string>) => void>(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  readChatSettings: mocks.readChatSettings,
  updateChatBehavior: mocks.updateChatBehavior,
}));

vi.mock("$lib/vault/config", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/vault/config")>(),
  announceConfigSaved: mocks.announceConfigSaved,
  onConfigChangedElsewhere: (listener: (keys: ReadonlySet<string>) => void) => {
    mocks.listeners.add(listener);
    return () => mocks.listeners.delete(listener);
  },
}));

function changeElsewhere(...keys: string[]): void {
  for (const listener of mocks.listeners) listener(new Set(keys));
}

/** Settings told apart by their `label`, which only identity checks read. */
function settings(label: string): ChatSettingsRead {
  return { lastSelectedThreadId: label } as unknown as ChatSettingsRead;
}

const behavior: ChatBehaviorPreferences = {
  sendKey: "enter",
  restoreLastSelectedThread: true,
  showReasoningSummaries: true,
  automaticallyFoldSettledWork: true,
  terminalScrollbackLines: 1000,
  idleSessionTimeoutSeconds: 600,
  confirmMultilineTerminalPaste: true,
};

async function settled(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("ChatConfigurationController behavior changes", () => {
  beforeEach(() => {
    mocks.readChatSettings.mockReset();
    mocks.updateChatBehavior.mockReset();
    mocks.announceConfigSaved.mockReset();
    mocks.announceConfigSaved.mockResolvedValue();
    mocks.listeners.clear();
  });

  it("rereads settings when another window or sync changes the Chat behavior branch", async () => {
    const controller = new ChatConfigurationController({ onSettingsChanged: () => undefined });
    changeElsewhere("chat.behavior.sendKey");
    await settled();
    expect(mocks.readChatSettings).not.toHaveBeenCalled();

    controller.setSettings(settings("loaded"));
    changeElsewhere("music.volume");
    await settled();
    expect(mocks.readChatSettings).not.toHaveBeenCalled();

    mocks.readChatSettings.mockResolvedValueOnce(settings("refreshed"));
    changeElsewhere("chat.behavior.sendKey");
    await settled();
    expect(mocks.readChatSettings).toHaveBeenCalledTimes(1);
    expect(controller.settings).toEqual(settings("refreshed"));

    mocks.readChatSettings.mockResolvedValueOnce(settings("whole branch"));
    changeElsewhere("chat");
    await settled();
    expect(controller.settings).toEqual(settings("whole branch"));
  });

  it("stops listening once disposed", async () => {
    const controller = new ChatConfigurationController({ onSettingsChanged: () => undefined });
    controller.setSettings(settings("loaded"));
    controller.dispose();
    expect(mocks.listeners.size).toBe(0);

    changeElsewhere("chat.behavior.sendKey");
    await settled();
    expect(mocks.readChatSettings).not.toHaveBeenCalled();
  });

  it("announces a saved behavior to the other windows before rereading settings", async () => {
    const controller = new ChatConfigurationController({ onSettingsChanged: () => undefined });
    const order: string[] = [];
    mocks.updateChatBehavior.mockImplementation(async () => {
      order.push("save");
    });
    mocks.announceConfigSaved.mockImplementation(async (keys) => {
      order.push(`announce ${keys.join(",")}`);
    });
    mocks.readChatSettings.mockImplementation(async () => {
      order.push("read");
      return settings("saved");
    });

    await controller.updateBehavior(behavior);

    expect(order).toEqual(["save", "announce chat.behavior", "read"]);
    expect(controller.settings).toEqual(settings("saved"));
  });
});
