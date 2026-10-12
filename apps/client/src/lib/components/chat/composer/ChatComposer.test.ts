// @vitest-environment jsdom

import { tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { composerModelSelection } from "$lib/chat/composer/model";
import { getChat } from "$lib/stores/chat.svelte";
import {
  claudeModelSettings,
  composer,
  createChatComposerHarness,
  imageAttachment,
  interactionState,
  modelSettings,
  openSlashMenu,
  resetChatComposerTestStore,
  setEditorSelection,
  threadShell,
} from "$lib/components/chat/ChatComposer.test-support";

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

describe("ChatComposer", () => {
  const { mounted, setup, cleanup } = createChatComposerHarness();

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
    expect(menu?.textContent).not.toContain("Ganbaru AI");
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
      modelSelection: composerModelSelection("default", false, []),
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

  it("formats a visible selection and serializes it as Markdown", async () => {
    const chat = getChat();
    vi.spyOn(chat, "setComposerRichContent").mockImplementation((text, richContent) => {
      chat.composer = { ...chat.composer, text, richContent };
    });
    const { target, editor } = setup(false);
    await tick();
    editor.focus();
    setEditorSelection(editor, 11, 19);
    expect(document.getSelection()?.toString()).toBe("calendar");
    document.dispatchEvent(new Event("selectionchange"));
    const bold = target.querySelector<HTMLButtonElement>('button[aria-label="Bold"]');
    expect(bold).not.toBeNull();
    bold?.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true }));
    bold?.click();
    await tick();

    expect(chat.composer.text).toBe("Review the **calendar** implementation");
    expect(editor.querySelector("strong")?.textContent).toBe("calendar");
    expect(bold?.getAttribute("aria-pressed")).toBe("true");
    expect(target.querySelector('button[aria-label="Italic"]')).not.toBeNull();
  });

  it("keeps the simplified composer actions directly discoverable", () => {
    const { target } = setup(false);
    expect(target.querySelector("[data-chat-model-trigger]")).not.toBeNull();
    const safetyTrigger = target.querySelector<HTMLElement>('[data-chat-field="safety"]');
    expect(safetyTrigger?.tagName).toBe("BUTTON");
    expect(safetyTrigger?.hasAttribute("title")).toBe(false);
    expect(safetyTrigger?.dataset.appTooltipDisabled).toBe("true");
    expect(target.querySelector('[data-chat-field="interaction"]')).toBeNull();
    expect(target.querySelector(".context-ring svg")?.getAttribute("role")).toBe("img");
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
    expect(target.querySelector('[role="alert"]')?.textContent).toContain("Image signature is invalid");
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

});
