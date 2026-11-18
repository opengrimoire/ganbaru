// @vitest-environment jsdom
import { en } from "$lib/i18n/messages/en";

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { composerModelSelection, readComposerModelSelection } from "$lib/chat/composer/model";
import { getChat } from "$lib/stores/chat.svelte";
import ChatDirectStartComposer from "$lib/components/chat/ChatDirectStartComposer.svelte";
import {
  claudeModelSettings,
  composer,
  imageAttachment,
  interactionState,
  modelSettings,
  multiProviderSettings,
  openSlashMenu,
  pointerEvent,
  resetChatComposerTestStore,
  setEditorSelection,
  threadShell,
} from "$lib/components/chat/composer/composer-test-store";

const api = vi.hoisted(() => ({
  attachmentUrl: vi.fn(async () => "data:image/png;base64,iVBORw0KGgo="),
  promptCatalog: vi.fn(async () => [] as import("$lib/chat/contracts").ChatPromptCatalogEntry[]),
  compactContext: vi.fn(async () => undefined),
  mcpStatus: vi.fn(async () => ({ servers: [] }) as import("$lib/chat/contracts").McpStatusRead),
  fullAccessTrust: vi.fn(async () => false),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  chatAttachmentDataUrl: api.attachmentUrl,
  hasChatFullAccessTrust: api.fullAccessTrust,
  listChatPromptCatalog: api.promptCatalog,
  compactChatContext: api.compactContext,
  readChatMcpStatus: api.mcpStatus,
  searchChatWorkingFolderPaths: vi.fn(async () => ({ entries: [], nextCursor: null })),
}));

interface MountedComposer {
  target: HTMLDivElement;
  component: ReturnType<typeof mount>;
}

/**
 * Mounts direct start composers into the document and unmounts them in reverse order.
 *
 * @returns The mounted composers, a mount function returning the target and editor, and cleanup.
 */
function createDirectStartHarness() {
  const mounted: MountedComposer[] = [];
  return {
    mounted,
    setup(hero = false): { target: HTMLDivElement; editor: HTMLDivElement } {
      const target = document.createElement("div");
      document.body.append(target);
      const component = mount(ChatDirectStartComposer, { target, props: { hero } });
      mounted.push({ target, component });
      const editor = target.querySelector("div[data-chat-composer]");
      if (!(editor instanceof HTMLDivElement)) throw new Error("Chat composer did not render");
      return { target, editor };
    },
    async cleanup(): Promise<void> {
      while (mounted.length > 0) {
        const entry = mounted.pop();
        if (!entry) continue;
        await unmount(entry.component);
        entry.target.remove();
      }
    },
  };
}

describe("ChatDirectStartComposer", () => {
  const { mounted, setup, cleanup } = createDirectStartHarness();

  beforeEach(() => {
    resetChatComposerTestStore();
    api.attachmentUrl.mockClear();
    api.promptCatalog.mockClear();
    api.compactContext.mockClear();
    api.mcpStatus.mockClear();
    api.fullAccessTrust.mockClear();
  });

  afterEach(async () => {
    await cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("restores focus and selection when the shared composer changes layout", async () => {
    const first = setup(true);
    await tick();
    first.editor.focus();
    setEditorSelection(first.editor, 3, 7);
    expect(document.getSelection()?.toString()).toBe("iew ");
    document.dispatchEvent(new Event("selectionchange"));
    const firstMount = mounted.pop();
    if (!firstMount) throw new Error("First composer mount was not recorded");
    await unmount(firstMount.component);
    firstMount.target.remove();

    const second = setup(false);
    await tick();
    await tick();
    expect(document.activeElement).toBe(second.editor);
    expect(document.getSelection()?.toString()).toBe("iew ");
  });

  it("preserves a trailing soft break as a fixed editor line", async () => {
    const chat = getChat();
    chat.composer = { ...composer(), text: "Example\n" };
    const { editor } = setup(false);
    await tick();

    const lines = editor.querySelectorAll(":scope > [data-chat-composer-line]");
    expect(lines).toHaveLength(2);
    expect(lines[0]?.textContent).toBe("Example");
    expect(lines[1]?.querySelector("[data-chat-composer-sentinel]")).not.toBeNull();
  });

  it("shows one flat icon command list and exposes Plan as a composer mode", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = { ...composer(), text: "", providerInstanceId: "codex-local" };
    api.promptCatalog.mockResolvedValueOnce([{
      value: "/compact",
      label: "Compact context",
      description: "Compact the active context",
      argumentHint: null,
      kind: "command",
      source: "provider",
      stale: false,
    }]);
    const setModes = vi.spyOn(chat, "setComposerModes").mockImplementation((safetyMode, interactionMode) => {
      chat.composer = { ...chat.composer, safetyMode, interactionMode };
    });
    const { target, editor } = setup(false);
    await tick();
    const line = editor.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!line) throw new Error("Composer line did not render");
    line.textContent = "/";
    setEditorSelection(editor, 1, 1);
    editor.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: "/" }));
    await tick();
    await tick();

    expect(api.promptCatalog).toHaveBeenCalledWith("workspace-1", "codex-local", null);
    const menu = target.querySelector(".composer-menu");
    expect(menu?.textContent).not.toContain("Ganbaru");
    expect(menu?.textContent).not.toContain("/plan");
    expect(menu?.textContent).not.toContain("/compact");
    expect(menu?.textContent).toContain("Plan");
    expect(menu?.textContent).toContain("Compact context");
    expect(menu?.querySelector(".command-group")).toBeNull();
    const options = [...(menu?.querySelectorAll<HTMLButtonElement>('button[role="option"]') ?? [])];
    expect(options.every((option) => option.querySelector("svg") !== null)).toBe(true);
    const plan = [...(menu?.querySelectorAll<HTMLButtonElement>('button[role="option"]') ?? [])]
      .find((button) => button.querySelector("strong")?.textContent === "Plan");
    plan?.click();
    await tick();
    expect(setModes).toHaveBeenCalledWith("ask_for_approval", "plan");
    expect(editor.textContent?.replaceAll("\u200b", "")).toBe("");
    const mode = target.querySelector<HTMLButtonElement>(".composer-mode");
    expect(mode?.textContent).toContain("Plan");
    expect(mode?.getAttribute("aria-label")).toBe("Exit Plan mode");
    mode?.click();
    await tick();
    expect(setModes).toHaveBeenLastCalledWith("ask_for_approval", "build");
  });

  it("prefetches commands and opens the slash palette without waiting for provider I/O", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = { ...composer(), text: "", providerInstanceId: "codex-local" };
    let resolveCatalog!: (entries: import("$lib/chat/contracts").ChatPromptCatalogEntry[]) => void;
    api.promptCatalog.mockImplementationOnce(() => new Promise((resolve) => {
      resolveCatalog = resolve;
    }));
    const { target, editor } = setup(false);

    await vi.waitFor(() => expect(api.promptCatalog).toHaveBeenCalledWith("workspace-1", "codex-local", null));
    await openSlashMenu(editor);

    const menu = target.querySelector<HTMLElement>(".composer-menu");
    expect(menu?.textContent).toContain("Plan");
    expect(menu?.querySelector(".animate-spin")).toBeNull();
    expect(api.promptCatalog).toHaveBeenCalledTimes(1);

    resolveCatalog([{
      value: "/compact",
      label: "Compact",
      description: "Compact the active context",
      argumentHint: null,
      kind: "command",
      source: "provider",
      stale: false,
    }]);
    await vi.waitFor(() => expect(menu?.textContent).toContain("Compact"));
  });

  it("runs action commands immediately without placing slash syntax in the editor", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = {
      ...composer(),
      text: "",
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
      threadId: "thread-1",
    };
    chat.activeThreads = [threadShell()];
    chat.selectedThreadId = "thread-1";
    api.promptCatalog.mockResolvedValueOnce([{
      value: "/compact",
      label: "Compact",
      description: "Compact the active context",
      argumentHint: null,
      kind: "command",
      source: "provider",
      stale: false,
    }]);
    vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
      chat.composer = { ...chat.composer, text, richContent };
    });
    const sendComposer = vi.spyOn(chat, "sendComposer").mockResolvedValue();
    vi.spyOn(chat, "refreshInteraction").mockResolvedValue();
    const { target, editor } = setup(false);
    await openSlashMenu(editor);

    const compact = [...target.querySelectorAll<HTMLButtonElement>('.composer-menu button[role="option"]')]
      .find((button) => button.querySelector("strong")?.textContent === "Compact");
    compact?.click();
    await tick();
    await Promise.resolve();

    expect(editor.textContent?.replaceAll("\u200b", "")).toBe("");
    expect(api.compactContext).toHaveBeenCalledWith("thread-1");
    expect(sendComposer).not.toHaveBeenCalled();
    expect(target.querySelector(".composer-mode")).toBeNull();
  });

  it("runs Claude Compact as a direct control action", async () => {
    const chat = getChat();
    chat.settings = claudeModelSettings();
    chat.composer = {
      ...composer(),
      text: "",
      providerInstanceId: "claude",
      modelSelection: composerModelSelection("opus", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
      threadId: "thread-1",
    };
    chat.activeThreads = [threadShell()];
    chat.selectedThreadId = "thread-1";
    api.promptCatalog.mockResolvedValueOnce([{
      value: "/compact",
      label: "Compact",
      description: "Compact conversation history",
      argumentHint: null,
      kind: "command",
      source: "provider",
      stale: false,
    }]);
    vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
      chat.composer = { ...chat.composer, text, richContent };
    });
    const sendComposer = vi.spyOn(chat, "sendComposer").mockResolvedValue();
    vi.spyOn(chat, "refreshInteraction").mockResolvedValue();
    const { target, editor } = setup(false);
    await openSlashMenu(editor);

    const compact = [...target.querySelectorAll<HTMLButtonElement>('.composer-menu button[role="option"]')]
      .find((button) => button.querySelector("strong")?.textContent === "Compact");
    compact?.click();
    await tick();
    await Promise.resolve();

    expect(api.compactContext).toHaveBeenCalledWith("thread-1");
    expect(sendComposer).not.toHaveBeenCalled();
  });

  it("opens Status and MCP as anchored panels without sending synthetic messages", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = {
      ...composer(),
      text: "",
      threadId: "thread-1",
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    chat.activeThreads = [threadShell()];
    chat.selectedThreadId = "thread-1";
    chat.interaction = interactionState();
    api.promptCatalog.mockResolvedValue([{
      value: "/mcp",
      label: "MCP",
      description: "Show configured MCP servers",
      argumentHint: null,
      kind: "command",
      source: "provider",
      stale: false,
    }]);
    api.mcpStatus.mockResolvedValueOnce({
      servers: [{ name: "openaiDeveloperDocs", authStatus: "unsupported", enabled: true, runtimeStatus: "ready" }],
    });
    vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
      chat.composer = { ...chat.composer, text, richContent };
    });
    const sendComposer = vi.spyOn(chat, "sendComposer").mockResolvedValue();
    const { target, editor } = setup(false);

    await openSlashMenu(editor);
    [...target.querySelectorAll<HTMLButtonElement>('.composer-menu button[role="option"]')]
      .find((button) => button.querySelector("strong")?.textContent === "Status")?.click();
    await tick();
    const statusPanel = target.querySelector<HTMLElement>(".composer-info-panel");
    expect(statusPanel?.textContent).toContain("session-codex-1");
    expect(statusPanel?.textContent).toContain("61% left (100,000 used / 258,000)");
    expect(target.querySelector(".command-notice")).toBeNull();
    [...statusPanel?.querySelectorAll<HTMLButtonElement>("button") ?? []]
      .find((button) => button.textContent === "Close")?.click();
    await tick();

    await openSlashMenu(editor);
    [...target.querySelectorAll<HTMLButtonElement>('.composer-menu button[role="option"]')]
      .find((button) => button.querySelector("strong")?.textContent === "MCP")?.click();
    await vi.waitFor(() => expect(target.querySelector(".mcp-status-table")?.textContent).toContain("openaiDeveloperDocs"));
    expect(target.querySelector(".mcp-status-table")?.textContent).toContain("Auth Unsupported");
    expect(target.querySelector(".mcp-status-table")?.textContent).toContain("Enabled");
    expect(api.mcpStatus).toHaveBeenCalledWith("workspace-1", "codex-local", "thread-1");
    expect(sendComposer).not.toHaveBeenCalled();
  });

  it("opens MCP from a new draft without creating a task or sending a message", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = {
      ...composer(),
      text: "",
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    api.promptCatalog.mockResolvedValue([{
      value: "/mcp",
      label: "MCP",
      description: "Show configured MCP servers",
      argumentHint: null,
      kind: "command",
      source: "provider",
      stale: false,
    }]);
    api.mcpStatus.mockResolvedValueOnce({
      servers: [{ name: "openaiDeveloperDocs", authStatus: "unsupported", enabled: true, runtimeStatus: "ready" }],
    });
    vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
      chat.composer = { ...chat.composer, text, richContent };
    });
    const sendComposer = vi.spyOn(chat, "sendComposer").mockResolvedValue();
    const { target, editor } = setup(true);

    await openSlashMenu(editor);
    [...target.querySelectorAll<HTMLButtonElement>('.composer-menu button[role="option"]')]
      .find((button) => button.querySelector("strong")?.textContent === "MCP")?.click();

    await vi.waitFor(() => expect(target.querySelector(".mcp-status-table")?.textContent).toContain("openaiDeveloperDocs"));
    expect(api.mcpStatus).toHaveBeenCalledWith("workspace-1", "codex-local", null);
    expect(target.textContent).not.toContain("This command needs an active task.");
    expect(chat.activeThreads).toHaveLength(0);
    expect(sendComposer).not.toHaveBeenCalled();
  });

  it("keeps the keyboard-selected command visible inside the palette", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = { ...composer(), text: "", providerInstanceId: "codex-local" };
    const { target, editor } = setup(false);
    await openSlashMenu(editor);
    const menu = target.querySelector<HTMLElement>(".composer-menu");
    if (!menu) throw new Error("Command menu did not render");
    Object.defineProperties(menu, {
      clientHeight: { configurable: true, value: 60 },
      scrollTop: { configurable: true, writable: true, value: 0 },
    });
    [...menu.querySelectorAll<HTMLElement>('[data-menu-index]')].forEach((option, index) => {
      Object.defineProperties(option, {
        offsetTop: { configurable: true, value: index * 30 },
        offsetHeight: { configurable: true, value: 30 },
      });
    });
    for (let index = 0; index < 5; index += 1) {
      editor.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "ArrowDown" }));
      await tick();
    }
    await Promise.resolve();
    expect(menu.scrollTop).toBeGreaterThan(0);
    expect(menu.querySelector('[aria-selected="true"]')?.getAttribute("data-menu-index")).toBe("5");
  });

  it("turns commands that need input into a persistent composer mode", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = {
      ...composer(),
      text: "",
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "plan",
    };
    api.promptCatalog.mockResolvedValueOnce([{
      value: "/goal",
      label: "Goal",
      description: "Set a goal to keep pursuing",
      argumentHint: "[objective]",
      kind: "command",
      source: "provider",
      stale: false,
    }]);
    vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
      chat.composer = { ...chat.composer, text, richContent };
    });
    const setModes = vi.spyOn(chat, "setComposerModes").mockImplementation((safetyMode, interactionMode) => {
      chat.composer = { ...chat.composer, safetyMode, interactionMode };
    });
    const sendComposer = vi.spyOn(chat, "sendComposer").mockResolvedValue();
    let { target, editor } = setup(false);
    await openSlashMenu(editor);

    const goal = [...target.querySelectorAll<HTMLButtonElement>('.composer-menu button[role="option"]')]
      .find((button) => button.querySelector("strong")?.textContent === "Goal");
    goal?.click();
    await tick();

    expect(sendComposer).not.toHaveBeenCalled();
    expect(setModes).toHaveBeenLastCalledWith("ask_for_approval", "build");
    expect(target.querySelector(".composer-mode")?.textContent).toContain("Goal");
    expect(editor.dataset.placeholder).toBe("Describe the goal");

    const firstMount = mounted.pop();
    if (!firstMount) throw new Error("Goal composer mount was not recorded");
    await unmount(firstMount.component);
    firstMount.target.remove();
    ({ target, editor } = setup(true));
    await tick();
    expect(target.querySelector(".composer-mode")?.textContent).toContain("Goal");
    expect(editor.dataset.placeholder).toBe("Describe the goal");

    const line = editor.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!line) throw new Error("Composer line did not render");
    line.textContent = "Finish the migration";
    setEditorSelection(editor, 20, 20);
    editor.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: "n" }));
    await tick();
    target.querySelector<HTMLButtonElement>("button.primary-action")?.click();
    await tick();
    await Promise.resolve();

    expect(sendComposer).toHaveBeenCalledWith({
      promptOverride: "/goal Finish the migration",
      omitComposerContext: true,
    });
    await vi.waitFor(() => expect(target.querySelector(".composer-mode")).toBeNull());
  });

  it("accepts ordinary and composed characters after deleting the entire draft", async () => {
    const chat = getChat();
    chat.composer = { ...composer(), text: "Example" };
    vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
      chat.composer = { ...chat.composer, text, richContent };
    });
    const { editor } = setup(false);
    await tick();
    editor.focus();
    editor.replaceChildren();
    setEditorSelection(editor, 0, 0);
    editor.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "deleteContentForward" }));
    await tick();

    const emptyLine = editor.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!emptyLine) throw new Error("Empty editor line did not render");
    emptyLine.textContent = "abc";
    setEditorSelection(editor, 3, 3);
    editor.dispatchEvent(new InputEvent("input", { bubbles: true, data: "c", inputType: "insertText" }));
    editor.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    const composingLine = editor.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!composingLine) throw new Error("Composing editor line did not render");
    composingLine.textContent = "abcñ";
    setEditorSelection(editor, 4, 4);
    editor.dispatchEvent(new InputEvent("input", { bubbles: true, data: "ñ", inputType: "insertCompositionText", isComposing: true }));
    editor.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "ñ" }));
    await tick();
    await Promise.resolve();

    expect(chat.composer.text).toBe("abcñ");
    expect(editor.textContent).toBe("abcñ");
    expect(editor.querySelectorAll(":scope > [data-chat-composer-line]")).toHaveLength(1);
  });

  it("creates a normal editor line for Shift+Enter", async () => {
    const chat = getChat();
    chat.composer = { ...composer(), text: "Example" };
    vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
      chat.composer = { ...chat.composer, text, richContent };
    });
    const { editor } = setup(false);
    await tick();
    editor.focus();
    setEditorSelection(editor, 7, 7);
    expect(document.getSelection()?.anchorOffset).toBe(7);
    const softBreak = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Enter", shiftKey: true });
    editor.dispatchEvent(softBreak);
    await tick();

    expect(softBreak.defaultPrevented).toBe(true);
    expect(chat.composer.text).toBe("Example\n");
    const lines = editor.querySelectorAll(":scope > [data-chat-composer-line]");
    expect(lines).toHaveLength(2);
    expect(lines[0]?.textContent).toBe("Example");
    expect(lines[1]?.querySelector("[data-chat-composer-sentinel]")).not.toBeNull();
  });

  it("keeps the simplified composer actions directly discoverable", () => {
    const { target } = setup(false);
    expect(target.querySelector("[data-chat-model-trigger]")).not.toBeNull();
    const safetyTrigger = target.querySelector<HTMLElement>('[data-chat-field="safety"]');
    expect(safetyTrigger?.tagName).toBe("BUTTON");
    expect(safetyTrigger?.hasAttribute("title")).toBe(false);
    expect(safetyTrigger?.dataset.appTooltipDisabled).toBe("true");
    expect(target.querySelector('[data-chat-field="interaction"]')).toBeNull();
    expect(target.querySelector("button.context-ring")?.getAttribute("aria-label")).toMatch(/context/i);
    expect(target.querySelector("select")).toBeNull();
    expect(target.querySelector(".attachment-menu summary")?.getAttribute("aria-label")).toBe("Attach images");
    expect(target.querySelector("button.primary-action")?.getAttribute("aria-label")).toBe("Send");
    expect(target.querySelector('[aria-label*="microphone" i]')).toBeNull();
  });

  it("starts a routine send without waiting for a full-access trust lookup", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    const sendComposer = vi.spyOn(chat, "sendComposer").mockResolvedValue();
    const { target } = setup(false);
    await tick();
    await Promise.resolve();
    api.fullAccessTrust.mockClear();

    target.querySelector<HTMLButtonElement>("button.primary-action")?.click();

    expect(sendComposer).toHaveBeenCalledOnce();
    expect(api.fullAccessTrust).not.toHaveBeenCalled();
  });

  it("keeps images from being sent to a text-only Codex model", async () => {
    const chat = getChat();
    const settings = modelSettings();
    const provider = settings.providerInstances[0];
    const template = provider?.modelCatalog?.models[0];
    if (!provider?.modelCatalog || !template) throw new Error("Model settings require a discovered model");
    provider.modelCatalog.models.push({
      ...template,
      id: "gpt-5.3-codex-spark",
      displayName: "GPT-5.3-Codex-Spark",
      capabilities: [],
    });
    const attachment = imageAttachment();
    chat.settings = settings;
    chat.composer = {
      ...composer(),
      attachmentIds: [attachment.id],
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.3-codex-spark", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    chat.composerAttachments = [attachment];
    const sendComposer = vi.spyOn(chat, "sendComposer").mockResolvedValue();
    const { target } = setup(false);
    await tick();

    target.querySelector<HTMLButtonElement>("button.primary-action")?.click();
    await tick();

    expect(sendComposer).not.toHaveBeenCalled();
    expect(target.querySelector('[role="alert"]')?.textContent).toContain(
      "GPT-5.3-Codex-Spark does not support image input",
    );
  });

  it("centers broad permission confirmation at the app root", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    chat.workingFolders = [{
      workingFolder: {
        id: "workspace-1",
        projectId: "project-1",
        displayName: "Example",
        kind: "external",
        managedRelativePath: null,
        sortOrder: 10,
        repositoryKind: "git",
        repositoryIdentity: "example-repository",
        createdAt: "2026-07-24T12:00:00.000Z",
        updatedAt: "2026-07-24T12:00:00.000Z",
        archivedAt: null,
        revision: 1,
      },
      bindingStatus: "available",
      canonicalPath: "/workspace/example",
      lastVerifiedAt: "2026-07-24T12:00:00.000Z",
      currentBranch: "feat/chat",
      trust: "trusted",
    }];
    vi.spyOn(chat, "setComposerModes").mockImplementation((safetyMode, interactionMode) => {
      chat.composer = { ...chat.composer, safetyMode, interactionMode };
    });

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>('[data-chat-field="safety"]')?.click();
    await tick();
    [...document.body.querySelectorAll<HTMLButtonElement>('[role="listbox"] button')]
      .find((button) => button.textContent?.includes("Full access"))?.click();
    await tick();

    expect(target.querySelector('[role="dialog"]')).toBeNull();
    let dialog = document.body.querySelector<HTMLElement>('.confirm-dialog[role="dialog"]');
    const dialogViewport = dialog?.parentElement;
    expect(dialogViewport?.classList.contains("fixed")).toBe(true);
    expect(dialogViewport?.style.left).toBe("var(--visual-viewport-offset-left)");
    expect(dialogViewport?.style.top).toBe("var(--visual-viewport-offset-top)");
    expect(dialogViewport?.style.width).toBe("var(--visual-viewport-width)");
    expect(dialogViewport?.style.height).toBe("var(--visual-viewport-height)");
    expect(dialog?.textContent).toContain("Allow Full access?");
    expect(dialog?.textContent).toContain("Unrestricted access to the internet and any file");
    expect(dialog?.textContent).not.toContain("Example");

    [...dialog?.querySelectorAll<HTMLButtonElement>("button") ?? []]
      .find((button) => button.textContent?.trim() === "Cancel (Esc)")?.click();
    await tick();
    target.querySelector<HTMLButtonElement>('[data-chat-field="safety"]')?.click();
    await tick();
    [...document.body.querySelectorAll<HTMLButtonElement>('[role="listbox"] button')]
      .find((button) => button.textContent?.includes("Custom"))?.click();
    await tick();

    dialog = document.body.querySelector<HTMLElement>('.confirm-dialog[role="dialog"]');
    expect(dialog?.textContent).toContain("Use custom permissions?");
    expect(dialog?.textContent).toContain("Uses permissions from config.toml, which may grant Full access");
    expect(dialog?.textContent).not.toContain("Example");
  });

  it("opens a managed image preview and preserves a failed import error", async () => {
    const chat = getChat();
    const attachment = imageAttachment();
    chat.composer = { ...composer(), attachmentIds: [attachment.id] };
    chat.composerAttachments = [attachment];
    const importImages = vi.spyOn(chat, "importComposerImages").mockRejectedValue(new Error("Image signature is invalid"));
    const { target } = setup(false);
    await tick();
    await Promise.resolve();
    await tick();
    const previewTrigger = target.querySelector<HTMLButtonElement>(".attachment-preview");
    previewTrigger?.focus();
    previewTrigger?.click();
    await tick();
    await Promise.resolve();
    await tick();
    const previewDialog = document.body.querySelector<HTMLElement>('.chat-image-dialog[role="dialog"]');
    expect(previewDialog?.textContent).toContain("diagram.png");
    expect(previewDialog?.contains(document.activeElement)).toBe(true);
    const zoomIn = previewDialog?.querySelector<HTMLButtonElement>('button[aria-label="Zoom in"]');
    zoomIn?.click();
    await tick();
    expect(previewDialog?.textContent).toContain("125%");
    previewDialog?.querySelector<HTMLButtonElement>('button[aria-label="Cancel"]')?.click();
    await tick();
    await Promise.resolve();
    expect(document.activeElement).toBe(previewTrigger);

    const fileInput = target.querySelector<HTMLInputElement>('input[type="file"]');
    if (!fileInput) throw new Error("Image input did not render");
    const file = new File([new Uint8Array([1, 2, 3])], "broken.png", { type: "image/png" });
    Object.defineProperty(fileInput, "files", { configurable: true, value: [file] });
    fileInput.dispatchEvent(new Event("change", { bubbles: true }));
    await tick();
    await Promise.resolve();
    await tick();
    expect(importImages).toHaveBeenCalledWith([file]);
    expect(target.querySelector('[role="alert"]')?.textContent).toContain(en.chatErrors.actions.fileAction);
  });


  function textPaste(text: string): ClipboardEvent {
    const paste = new Event("paste", { bubbles: true, cancelable: true }) as ClipboardEvent;
    Object.defineProperty(paste, "clipboardData", {
      configurable: true,
      value: { files: [], items: [], types: ["text/plain"], getData: () => text },
    });
    return paste;
  }

  it("strips hidden characters from pasted text and tells the user how many were removed", async () => {
    const chat = getChat();
    const setRichContent = vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
      chat.composer = { ...chat.composer, text, richContent };
    });
    const { target, editor } = setup(false);
    await tick();
    editor.focus();
    const paste = textPaste("approve \u{202E}nimda\u{202C} now\u{200B}");
    editor.dispatchEvent(paste);
    await tick();

    expect(paste.defaultPrevented).toBe(true);
    expect(setRichContent).toHaveBeenCalled();
    expect(chat.composer.text).toContain("approve nimda now");
    expect(chat.composer.text).not.toMatch(/[\u{200B}\u{202C}\u{202E}]/u);
    expect(target.querySelector('[role="status"]')?.textContent).toContain("3");
  });

  it("attaches a large paste as a text attachment instead of inserting it", async () => {
    const chat = getChat();
    const importSnippet = vi.spyOn(chat, "importComposerTextSnippet").mockResolvedValue();
    const setRichContent = vi.spyOn(chat, "setComposerRichContent");
    const { editor } = setup(false);
    await tick();
    editor.focus();
    const text = `${"x".repeat(40 * 1024)}\nsecond line\n`;
    editor.dispatchEvent(textPaste(text));
    await tick();

    expect(importSnippet).toHaveBeenCalledWith(text, "Pasted text (2 lines)");
    expect(setRichContent).not.toHaveBeenCalled();
  });

  it("refuses a paste above the text attachment bound and names the limit", async () => {
    const chat = getChat();
    const importSnippet = vi.spyOn(chat, "importComposerTextSnippet").mockResolvedValue();
    const { target, editor } = setup(false);
    await tick();
    editor.focus();
    editor.dispatchEvent(textPaste("x".repeat(1024 * 1024 + 1)));
    await tick();

    expect(importSnippet).not.toHaveBeenCalled();
    expect(target.querySelector('[role="alert"]')?.textContent).toMatch(/1\s?MB/u);
  });
  it("leaves unsupported clipboard payloads to the editor without requesting platform permission", async () => {
    const { target, editor } = setup(false);
    await tick();
    const paste = new Event("paste", { bubbles: true, cancelable: true }) as ClipboardEvent;
    Object.defineProperty(paste, "clipboardData", {
      configurable: true,
      value: {
        files: [],
        items: [],
        types: [],
        getData: () => "",
      },
    });

    editor.dispatchEvent(paste);
    await tick();

    expect(paste.defaultPrevented).toBe(false);
    expect(target.querySelector('[role="alert"]')).toBeNull();
  });

  describe("formatting toolbar", () => {
    const rangeRect = Object.getOwnPropertyDescriptor(Range.prototype, "getBoundingClientRect");

    beforeEach(() => {
      Object.defineProperty(Range.prototype, "getBoundingClientRect", {
        configurable: true,
        value: () => new DOMRect(40, 320, 80, 20),
      });
    });

    afterEach(() => {
      if (rangeRect) Object.defineProperty(Range.prototype, "getBoundingClientRect", rangeRect);
      else Reflect.deleteProperty(Range.prototype, "getBoundingClientRect");
    });

    async function selectText(start: number, end: number): Promise<HTMLDivElement> {
      const chat = getChat();
      chat.composer = { ...composer(), text: "Example text" };
      vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
        chat.composer = { ...chat.composer, text, richContent };
      });
      const { editor } = setup(false);
      await tick();
      vi.spyOn(editor, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 300, 600, 100));
      editor.focus();
      setEditorSelection(editor, start, end);
      document.dispatchEvent(new Event("selectionchange"));
      await tick();
      await tick();
      return editor;
    }

    function toolbar(): HTMLElement | null {
      return document.querySelector<HTMLElement>("[data-chat-format-toolbar]");
    }

    it("shows above a selection with every action named with its shortcut", async () => {
      await selectText(0, 7);

      const bar = toolbar();
      expect(bar?.getAttribute("role")).toBe("toolbar");
      expect(bar?.classList.contains("placed")).toBe(true);
      expect(bar?.dataset.side).toBe("above");
      const buttons = [...(bar?.querySelectorAll("button") ?? [])];
      expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual([
        "Bold", "Italic", "Strikethrough", "Numbered list", "Bulleted list", "Quote", "Code", "Code block",
      ]);
      expect(buttons[0]?.dataset.appTooltip).toBe("Bold (Ctrl + B)");
      expect(buttons.filter((button) => button.tabIndex === 0)).toHaveLength(1);
    });

    it("formats without moving the selection and marks the active format as pressed", async () => {
      const editor = await selectText(0, 7);
      const bold = toolbar()?.querySelector<HTMLButtonElement>('button[aria-label="Bold"]');
      const press = new MouseEvent("pointerdown", { bubbles: true, cancelable: true });
      bold?.dispatchEvent(press);
      bold?.click();
      await tick();

      expect(press.defaultPrevented).toBe(true);
      expect(getChat().composer.text).toBe("**Example** text");
      expect(editor.querySelector("strong")?.textContent).toBe("Example");
      expect(document.activeElement).toBe(editor);
      expect(document.getSelection()?.toString()).toBe("Example");
      expect(toolbar()?.querySelector('button[aria-label="Bold"]')?.getAttribute("aria-pressed")).toBe("true");
    });

    it("dismisses on Escape without clearing the draft until the selection changes", async () => {
      const editor = await selectText(0, 7);
      const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
      const outer = vi.fn();
      window.addEventListener("keydown", outer);
      editor.dispatchEvent(escape);
      window.removeEventListener("keydown", outer);
      await tick();

      expect(escape.defaultPrevented).toBe(true);
      expect(outer).not.toHaveBeenCalled();
      expect(toolbar()).toBeNull();
      expect(getChat().composer.text).toBe("Example text");

      setEditorSelection(editor, 0, 4);
      document.dispatchEvent(new Event("selectionchange"));
      await tick();
      await tick();
      expect(toolbar()).not.toBeNull();
    });

    it("hides when the selection collapses or an autocomplete menu opens", async () => {
      const editor = await selectText(0, 7);
      setEditorSelection(editor, 3, 3);
      document.dispatchEvent(new Event("selectionchange"));
      await tick();
      expect(toolbar()).toBeNull();

      setEditorSelection(editor, 0, 12);
      document.dispatchEvent(new Event("selectionchange"));
      await tick();
      expect(toolbar()).not.toBeNull();
      editor.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      await tick();
      expect(toolbar()).toBeNull();
    });

    it("leaves a touch selection to the platform selection menu", async () => {
      getChat().composer = { ...composer(), text: "Example text" };
      const { editor } = setup(false);
      await tick();
      const touch = new MouseEvent("pointerdown", { bubbles: true });
      Object.defineProperty(touch, "pointerType", { value: "touch" });
      editor.dispatchEvent(touch);
      editor.focus();
      setEditorSelection(editor, 0, 7);
      document.dispatchEvent(new Event("selectionchange"));
      await tick();

      expect(toolbar()).toBeNull();
    });

    it("moves focus into the toolbar with Alt+F10 and back to the selection with Escape", async () => {
      const editor = await selectText(0, 7);
      editor.dispatchEvent(new KeyboardEvent("keydown", { key: "F10", altKey: true, bubbles: true, cancelable: true }));
      const buttons = [...(toolbar()?.querySelectorAll("button") ?? [])];
      expect(document.activeElement).toBe(buttons[0]);

      buttons[0]?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true, cancelable: true }));
      await tick();
      expect(document.activeElement).toBe(buttons[7]);
      expect(toolbar()).not.toBeNull();

      buttons[7]?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await tick();
      expect(document.activeElement).toBe(editor);
      expect(document.getSelection()?.toString()).toBe("Example");
    });
  });
});

describe("ChatDirectStartComposer model controls", () => {
  const { setup, cleanup } = createDirectStartHarness();

  beforeEach(() => {
    resetChatComposerTestStore();
  });

  afterEach(async () => {
    await cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("keeps model, effort, and speed inside the compact model control", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, [
        { key: "reasoning_effort", value: { kind: "choice", value: "medium" } },
        { key: "service_tier", value: { kind: "choice", value: "standard" } },
      ]),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });
    const { target } = setup(false);
    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]");
    expect(trigger?.querySelector(".provider-icon")).toBeNull();
    expect(trigger?.textContent).toContain("Sol 5.6");
    expect(trigger?.textContent).toContain("Medium");
    trigger?.click();
    await tick();
    expect(target.querySelector(".model-popover")).not.toBeNull();
    expect(target.querySelector<HTMLElement>(".model-control")?.style.width).toBe("");
    expect(trigger?.querySelector(".model-chevron")?.classList.contains("open")).toBe(true);
    const effortChoices = [...target.querySelectorAll<HTMLButtonElement>(".effort-options button")];
    expect(effortChoices).toHaveLength(7);
    expect(effortChoices[0]?.getAttribute("aria-label")).toBe("Terra 5.6 Light");
    expect(effortChoices[2]?.getAttribute("aria-pressed")).toBe("true");
    expect(effortChoices.every((choice) => !choice.hasAttribute("title"))).toBe(true);
    expect(effortChoices.every((choice) => choice.dataset.appTooltipDisabled === "true")).toBe(true);
    const effortKnob = target.querySelector<HTMLElement>(".effort-knob");
    expect(effortKnob).not.toBeNull();
    expect(effortChoices[0]?.style.left).toContain("0% + 0.875rem");
    expect(effortChoices.at(-1)?.style.left).toContain("100% - 0.875rem");
    effortChoices.forEach((choice, index) => {
      vi.spyOn(choice, "getBoundingClientRect").mockReturnValue(new DOMRect(index * 40, 0, 20, 20));
    });
    const effortLadder = target.querySelector<HTMLElement>(".effort-ladder");
    effortLadder?.dispatchEvent(pointerEvent("pointerdown", 90));
    await tick();
    expect(target.querySelector(".effort-footer.holding")).toBeNull();
    effortLadder?.dispatchEvent(pointerEvent("pointermove", 130));
    await tick();
    expect(target.querySelector(".effort-footer.holding")?.textContent).toContain("Faster");
    expect(target.querySelector(".effort-footer.holding")?.textContent).toContain("Smarter");
    effortLadder?.dispatchEvent(pointerEvent("pointermove", 250));
    await tick();
    expect(trigger?.textContent).toContain("Ultra");
    effortLadder?.dispatchEvent(pointerEvent("pointerup", 250));
    await tick();
    expect(target.querySelector(".effort-footer.holding")).toBeNull();
    expect(effortLadder?.classList.contains("handle-hovered")).toBe(true);
    effortLadder?.dispatchEvent(pointerEvent("pointermove", 10));
    await tick();
    expect(effortLadder?.classList.contains("handle-hovered")).toBe(false);
    effortChoices[2]?.click();
    await tick();
    expect(trigger?.textContent).toContain("Medium");
    const fastButton = target.querySelector<HTMLButtonElement>(".fast-button");
    expect(fastButton?.getAttribute("aria-label")).toBe("Enable Fast mode");
    fastButton?.click();
    await tick();
    expect(target.querySelector(".effort-ladder.fast")).not.toBeNull();
    expect(fastButton?.getAttribute("aria-pressed")).toBe("true");
    effortChoices.at(-1)?.click();
    await tick();
    expect(trigger?.textContent).toContain("Ultra");
    expect(target.querySelector(".effort-name.ultra")).not.toBeNull();
    expect(target.querySelector(".effort-ladder.fast.ultra")).not.toBeNull();
    expect(target.querySelector(".fast-button.active.ultra")).not.toBeNull();
    expect(effortKnob?.style.left).toContain("100% - 0.875rem");
    effortChoices[0]?.click();
    await tick();
    expect(trigger?.textContent).toContain("Terra 5.6");
    expect(trigger?.textContent).toContain("Light");
    expect(target.querySelectorAll<HTMLButtonElement>(".effort-options button")[0]?.getAttribute("aria-pressed")).toBe("true");
    expect(effortKnob?.style.left).toContain("0% + 0.875rem");
    expect(target.querySelector(".option-rows")).toBeNull();
    const modelToggle = target.querySelector<HTMLButtonElement>(".model-toggle");
    expect(modelToggle?.textContent).toContain("Model");
    modelToggle?.click();
    await tick();
    expect(document.querySelector(".model-flyout.positioned")).not.toBeNull();
    expect(document.querySelector(".provider-rail")).toBeNull();
    expect(document.querySelector('[data-model-company="openai"] .model-company-heading svg')).not.toBeNull();
    expect(document.querySelector('[data-model-company="openai"] .model-company-heading')?.textContent).toContain("OpenAI");
    expect(document.querySelector('[data-model-company="anthropic"] .model-company-heading')?.textContent).toContain("Anthropic");
    const openAiHeading = document.querySelector<HTMLButtonElement>('[data-model-company="openai"] .model-company-heading');
    openAiHeading?.click();
    await tick();
    expect(openAiHeading?.getAttribute("aria-expanded")).toBe("false");
    expect(document.querySelector<HTMLElement>('[data-model-company="openai"] .model-company-content')?.inert).toBe(true);
    openAiHeading?.click();
    await tick();
    expect(openAiHeading?.getAttribute("aria-expanded")).toBe("true");
    expect(document.querySelector(".model-picker-provider")).toBeNull();
    const modelList = document.querySelector<HTMLElement>(".model-list");
    if (!modelList) throw new Error("Model list did not render");
    expect(modelList.querySelector(".model-search-row")).not.toBeNull();
    Object.defineProperties(modelList, {
      clientHeight: { configurable: true, value: 200 },
      scrollHeight: { configurable: true, value: 600 },
      scrollTop: { configurable: true, writable: true, value: 0 },
    });
    modelList.dispatchEvent(new Event("scroll"));
    await tick();
    expect(modelList.getAttribute("data-scroll-fade")).toBe("bottom");
    modelList.scrollTop = 200;
    modelList.dispatchEvent(new Event("scroll"));
    await tick();
    expect(modelList.getAttribute("data-scroll-fade")).toBe("both");
    modelList.scrollTop = 400;
    modelList.dispatchEvent(new Event("scroll"));
    await tick();
    expect(modelList.getAttribute("data-scroll-fade")).toBe("top");
    expect(document.querySelector<HTMLInputElement>(".model-search input")?.placeholder).toBe("Search models...");
    expect(document.querySelector(".model-flyout")?.textContent).not.toContain("gpt-5.6-sol");
    expect(document.querySelector(".model-flyout")?.textContent).not.toContain("GPT-5.4 Deprecated");
    expect(document.querySelector(".favorite-company-section .model-company-heading")?.textContent).toContain("Favorites");
    expect(document.querySelector(".favorite-company-section .model-company-content-inner > p")?.textContent).toBe("No favorites yet.");
    const setupAction = document.querySelector<HTMLElement>('[data-model-company="anthropic"] .company-setup');
    expect(setupAction?.textContent).toBe("Configure Anthropic");
    expect(setupAction?.querySelector("small")).toBeNull();
    expect(document.querySelector(".model-flyout")?.textContent).not.toContain("ProviderCodex");
    expect(target.querySelector('[data-chat-field="interaction"]')).toBeNull();
    trigger?.click();
    await tick();
    expect(target.querySelector(".model-popover")).toBeNull();
    trigger?.click();
    await tick();
    expect(target.querySelector(".effort-ladder")).not.toBeNull();
    expect(document.querySelector(".model-flyout")).toBeNull();
  });

  it("hides Fast when the provider catalog does not support speed selection", async () => {
    const chat = getChat();
    const settings = modelSettings();
    const provider = settings.providerInstances[0];
    if (!provider?.modelCatalog) throw new Error("Model settings require a discovered catalog");
    provider.modelCatalog.models = provider.modelCatalog.models.map((model) => ({
      ...model,
      options: model.options.filter((definition) => !["service_tier", "fastMode"].includes(definition.key)),
    }));
    chat.settings = settings;
    chat.composer = {
      ...composer(),
      providerInstanceId: provider.configuration.instanceId,
      modelSelection: composerModelSelection("gpt-5.6-sol", false, [
        { key: "reasoning_effort", value: { kind: "choice", value: "medium" } },
      ]),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();

    expect(target.querySelector(".model-toggle")?.textContent).toContain("Model");
    expect(target.querySelector(".fast-button")).toBeNull();
    expect(target.querySelector(".option-rows")).toBeNull();
  });

  it("keeps an option flyout open after selecting a value", async () => {
    const chat = getChat();
    const settings = modelSettings();
    const sol = settings.providerInstances[0]?.modelCatalog?.models.find((model) => model.id === "gpt-5.6-sol");
    if (!sol) throw new Error("Model settings require Sol 5.6");
    sol.options = [...sol.options, {
      kind: "choice",
      key: "context_window",
      label: "Context window",
      description: null,
      defaultValue: "standard",
      options: [
        { value: "standard", label: "Standard", description: null },
        { value: "extended", label: "Extended", description: null },
      ],
    }];
    chat.settings = settings;
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();
    const optionRow = [...target.querySelectorAll<HTMLButtonElement>(".option-rows button")]
      .find((button) => button.textContent?.startsWith("Context window"));
    expect(optionRow).toBeDefined();
    optionRow?.click();
    await tick();
    const extendedChoice = [...document.querySelectorAll<HTMLButtonElement>(".option-list > button")]
      .find((button) => button.textContent?.startsWith("Extended"));
    extendedChoice?.click();
    await tick();
    expect(document.querySelector(".model-flyout")).not.toBeNull();
    expect(readComposerModelSelection(chat.composer.modelSelection).options).toContainEqual({
      key: "context_window",
      value: { kind: "choice", value: "extended" },
    });
  });

  it("switches provider and model together from the model picker", async () => {
    const chat = getChat();
    chat.settings = multiProviderSettings();
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    vi.spyOn(chat, "setComposerProvider").mockImplementation((providerInstanceId) => {
      chat.composer = { ...chat.composer, providerInstanceId };
    });
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();
    target.querySelector<HTMLButtonElement>(".model-toggle")?.click();
    await tick();
    expect(document.querySelector('[data-model-company="openai"]')).not.toBeNull();
    expect(document.querySelector('[data-model-company="anthropic"]')).not.toBeNull();
    const claudeModel = [...document.querySelectorAll<HTMLButtonElement>('[data-model-company="anthropic"] .model-choice')]
      .find((button) => button.textContent?.includes("Opus 4.8"));
    claudeModel?.click();
    await tick();

    expect(chat.composer.providerInstanceId).toBe("claude");
    expect(readComposerModelSelection(chat.composer.modelSelection).modelId).toBe("opus");
    expect(document.querySelector(".model-flyout")).not.toBeNull();
    expect(target.querySelector(".model-popover")).not.toBeNull();
  });

  it("shows and updates favorites across providers without exposing raw model IDs", async () => {
    const chat = getChat();
    const settings = multiProviderSettings();
    const codex = settings.providerInstances.find((entry) => entry.configuration.instanceId === "codex-local");
    const claude = settings.providerInstances.find((entry) => entry.configuration.instanceId === "claude");
    if (!codex || !claude) throw new Error("Favorite fixtures require Codex and Claude");
    codex.configuration.favoriteModelIds = ["gpt-5.6-sol"];
    claude.configuration.favoriteModelIds = ["opus"];
    chat.settings = settings;
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    const updateModels = vi.spyOn(chat, "updateModels").mockResolvedValue();

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();
    target.querySelector<HTMLButtonElement>(".model-toggle")?.click();
    await tick();

    const providerModelRow = [...document.querySelectorAll<HTMLElement>('[data-model-company="openai"] .model-row')]
      .find((row) => row.textContent?.includes("Sol 5.6"));
    expect(providerModelRow?.querySelector(".model-provider-caption")).toBeNull();
    const favoriteText = document.querySelector(".favorite-company-section")?.textContent ?? "";
    expect(favoriteText).toContain("Sol 5.6");
    expect(favoriteText).toContain("Opus 4.8");
    expect(favoriteText).not.toContain("gpt-5.6-sol");
    expect(document.querySelectorAll(".model-provider-caption")).toHaveLength(0);
    expect(document.querySelectorAll(".favorite-source-section")).toHaveLength(0);
    expect(document.querySelectorAll(".favorite-model-label")).toHaveLength(2);

    document.querySelector<HTMLButtonElement>('.favorite-company-section .model-favorite[aria-label="Favorite: Sol 5.6"]')?.click();
    await tick();
    expect(updateModels).toHaveBeenCalledWith("codex-local", [], []);
  });

  it("selects a provider's recommended model with medium effort for a fresh composer", async () => {
    const chat = getChat();
    chat.settings = claudeModelSettings();
    chat.composer = {
      ...composer(),
      providerInstanceId: "claude",
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });

    const { target } = setup(false);
    await tick();
    await tick();

    const selection = readComposerModelSelection(chat.composer.modelSelection);
    expect(selection.modelId).toBe("opus");
    expect(selection.providerManaged).toBe(false);
    expect(selection.options).toEqual([{
      key: "effort",
      value: { kind: "choice", value: "medium" },
    }, {
      key: "fastMode",
      value: { kind: "boolean", value: false },
    }]);
    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]");
    expect(trigger?.textContent).toContain("Opus 4.8");
    expect(trigger?.textContent).toContain("Medium");

    trigger?.click();
    await tick();
    expect(target.querySelector(".fast-button")).not.toBeNull();
    const choices = [...target.querySelectorAll<HTMLButtonElement>(".effort-options button")];
    expect(choices).toHaveLength(5);
    expect(choices[1]?.getAttribute("aria-pressed")).toBe("true");
    expect(choices[0]?.style.left).toContain("0% + 0.875rem");
    expect(choices.at(-1)?.style.left).toContain("100% - 0.875rem");
    const knob = target.querySelector<HTMLElement>(".effort-knob");
    choices[0]?.click();
    await tick();
    expect(knob?.style.left).toContain("0% + 0.875rem");
    choices.at(-1)?.click();
    await tick();
    expect(knob?.style.left).toContain("100% - 0.875rem");

    const fastButton = target.querySelector<HTMLButtonElement>(".fast-button");
    expect(fastButton?.getAttribute("aria-pressed")).toBe("false");
    fastButton?.click();
    await tick();
    expect(readComposerModelSelection(chat.composer.modelSelection).options).toContainEqual({
      key: "fastMode",
      value: { kind: "boolean", value: true },
    });
    expect(fastButton?.getAttribute("aria-pressed")).toBe("true");
  });

  it("keeps a neutral five-stop track for models without effort levels", async () => {
    const chat = getChat();
    const settings = claudeModelSettings();
    const provider = settings.providerInstances[0];
    if (!provider?.modelCatalog) throw new Error("Claude model settings require a discovered catalog");
    provider.modelCatalog.models = [{
      id: "haiku",
      displayName: "Haiku 4.5",
      description: "Fastest for quick answers",
      contextLimit: null,
      availability: "available",
      capabilities: [],
      custom: false,
      options: [],
    }];
    chat.settings = settings;
    chat.composer = {
      ...composer(),
      providerInstanceId: "claude",
      modelSelection: composerModelSelection("haiku", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();

    const dummyLadder = target.querySelector<HTMLElement>(".effort-ladder.dummy");
    expect(dummyLadder?.getAttribute("aria-disabled")).toBe("true");
    expect(dummyLadder?.getAttribute("aria-label")).toBe("This model does not provide effort levels");
    expect(dummyLadder?.querySelectorAll(".effort-stop")).toHaveLength(5);
    expect(dummyLadder?.querySelector(".effort-fill")).toBeNull();
    expect(dummyLadder?.querySelector(".effort-knob")).toBeNull();
  });

  it("selects the strongest model from the first healthy provider and defaults permissions", async () => {
    const chat = getChat();
    const settings = modelSettings();
    const firstProvider = settings.providerInstances[0];
    if (!firstProvider?.modelCatalog) throw new Error("Model settings require a discovered catalog");
    firstProvider.modelCatalog.models.reverse();
    chat.settings = settings;
    vi.spyOn(chat, "setComposerProvider").mockImplementation((providerInstanceId) => {
      chat.composer = { ...chat.composer, providerInstanceId };
    });
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });
    vi.spyOn(chat, "setComposerModes").mockImplementation((safetyMode, interactionMode) => {
      chat.composer = { ...chat.composer, safetyMode, interactionMode };
    });

    const { target } = setup(false);
    await tick();
    await tick();
    await tick();

    expect(chat.composer.providerInstanceId).toBe("codex-local");
    expect(readComposerModelSelection(chat.composer.modelSelection).modelId).toBe("gpt-5.6-sol");
    expect(chat.composer.safetyMode).toBe("ask_for_approval");
    expect(chat.composer.interactionMode).toBe("build");
    expect(target.querySelector('[data-chat-field="safety"]')?.textContent).toContain("Ask for approval");
    expect(target.querySelector("[data-chat-model-trigger]")?.textContent).toContain("Sol 5.6");
  });

  it("previews disabled model controls when no provider is configured", async () => {
    const chat = getChat();
    const settings = modelSettings();
    settings.providerInstances = [];
    chat.settings = settings;
    vi.spyOn(chat, "setComposerModes").mockImplementation((safetyMode, interactionMode) => {
      chat.composer = { ...chat.composer, safetyMode, interactionMode };
    });

    const { target } = setup(false);
    await tick();
    await tick();

    expect(chat.composer.providerInstanceId).toBeNull();
    expect(chat.composer.safetyMode).toBe("ask_for_approval");
    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]");
    expect(trigger?.textContent).toContain("Choose provider");
    trigger?.click();
    await tick();
    await tick();

    const modelToggle = target.querySelector<HTMLButtonElement>(".model-toggle");
    expect(modelToggle?.disabled).toBe(false);
    expect(modelToggle?.getAttribute("aria-expanded")).toBe("true");
    expect(document.querySelector(".model-flyout")).not.toBeNull();
    const dummyLadder = target.querySelector<HTMLElement>(".effort-ladder.dummy");
    expect(dummyLadder?.getAttribute("aria-disabled")).toBe("true");
    expect(dummyLadder?.querySelectorAll(".effort-stop")).toHaveLength(5);
    expect(dummyLadder?.querySelector(".effort-knob")).toBeNull();
    expect(target.querySelector<HTMLButtonElement>(".fast-button.dummy")?.disabled).toBe(true);
  });
});
