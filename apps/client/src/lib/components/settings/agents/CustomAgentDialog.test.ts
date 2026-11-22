// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderInstanceRead } from "$lib/chat/contracts";
import CustomAgentDialog from "./CustomAgentDialog.svelte";

const chatApi = vi.hoisted(() => ({
  createChatAgent: vi.fn(async (): Promise<void> => undefined),
}));

const chat = vi.hoisted(() => ({
  agentIdentities: [],
  settings: null,
  refreshAgents: vi.fn(async (): Promise<void> => undefined),
}));

vi.mock("$lib/api/chat", () => chatApi);
vi.mock("$lib/stores/chat.svelte", () => ({ getChat: () => chat }));

const provider = {
  configuration: {
    schemaVersion: 1,
    instanceId: "opencode",
    familyId: "opencode",
    label: "OpenCode",
    enabled: true,
    executable: "opencode",
    providerHome: null,
    launchArguments: [],
    environment: {},
    credentialReferences: {},
    providerConfig: { schemaVersion: 1, value: {} },
  },
  runtimeReview: [],
  lastProbe: null,
  lastSuccessfulProbeAt: null,
  usageLimits: null,
  lastUsedAt: null,
  duplicateOf: null,
  modelCatalog: {
    instanceId: "opencode",
    models: [{
      id: "vendor/model",
      displayName: "Model",
      description: null,
      contextLimit: null,
      availability: "available",
      capabilities: [],
      options: [],
    }],
    recommendedModelId: null,
    source: "provider",
    discoveredAt: "2026-08-01T00:00:00Z",
    stale: false,
  },
} satisfies ProviderInstanceRead;

describe("CustomAgentDialog", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    target = undefined;
    component = undefined;
    chatApi.createChatAgent.mockReset();
    chatApi.createChatAgent.mockResolvedValue(undefined);
    chat.refreshAgents.mockReset();
    chat.refreshAgents.mockResolvedValue(undefined);
  });

  function mountDialog(onClose: () => void): HTMLDivElement {
    const host = document.createElement("div");
    document.body.append(host);
    target = host;
    component = mount(CustomAgentDialog, { target: host, props: { provider, onClose } });
    flushSync();
    return host;
  }

  function submit(host: HTMLElement, name: string): void {
    const input = host.querySelector<HTMLInputElement>("input");
    if (!input) throw new Error("Missing agent name input");
    input.value = name;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    host.querySelector("form")?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    flushSync();
  }

  it("closes once the agent exists even when rereading the agents fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    chat.refreshAgents.mockRejectedValueOnce(new Error("channel read failed"));
    const onClose = vi.fn();
    const host = mountDialog(onClose);

    submit(host, "Reviewer");

    await vi.waitFor(() => expect(onClose).toHaveBeenCalledOnce());
    expect(chatApi.createChatAgent).toHaveBeenCalledOnce();
    expect(host.textContent).not.toContain("Could not add the agent.");
    warn.mockRestore();
  });

  it("does not close again after the dialog was dismissed while creating", async () => {
    let finishCreate: () => void = () => {};
    chatApi.createChatAgent.mockImplementationOnce(() => new Promise<void>((resolve) => { finishCreate = resolve; }));
    const onClose = vi.fn();
    const host = mountDialog(onClose);

    submit(host, "Reviewer");
    expect(chatApi.createChatAgent).toHaveBeenCalledOnce();
    host.querySelector<HTMLButtonElement>("header button")?.click();
    expect(onClose).toHaveBeenCalledOnce();
    if (component) await unmount(component);
    component = undefined;

    finishCreate();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(onClose).toHaveBeenCalledOnce();
  });
});
