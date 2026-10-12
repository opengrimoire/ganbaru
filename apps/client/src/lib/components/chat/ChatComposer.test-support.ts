import { mount, tick, unmount } from "svelte";
import { vi } from "vitest";
import type { ChatComposerSnapshot } from "$lib/chat/composer/controller";
import type {
  ChatAttachmentRead,
  ChatInteractionStateRead,
  ChatSettingsRead,
  ChatThreadShellRead,
} from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import ChatComposer from "$lib/components/chat/composer/ChatComposer.svelte";

class ResizeObserverMock implements ResizeObserver {
  constructor(_callback: ResizeObserverCallback) {}

  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

export interface ChatComposerHarness {
  mounted: { target: HTMLDivElement; component: ReturnType<typeof mount> }[];
  setup: (hero?: boolean) => { target: HTMLDivElement; editor: HTMLDivElement };
  cleanup: () => Promise<void>;
}

export function createChatComposerHarness(): ChatComposerHarness {
  const mounted: ChatComposerHarness["mounted"] = [];
  return {
    mounted,
    setup(hero = false) {
      const target = document.createElement("div");
      document.body.append(target);
      const component = mount(ChatComposer, { target, props: { hero } });
      mounted.push({ target, component });
      const editor = target.querySelector("div[data-chat-composer]");
      if (!(editor instanceof HTMLDivElement)) throw new Error("Chat composer did not render");
      return { target, editor };
    },
    async cleanup() {
      while (mounted.length > 0) {
        const entry = mounted.pop();
        if (!entry) continue;
        await unmount(entry.component);
        entry.target.remove();
      }
    },
  };
}

export function resetChatComposerTestStore(): void {
  vi.stubGlobal("ResizeObserver", ResizeObserverMock);
  const chat = getChat();
  chat.composer = composer();
  chat.composerAttachments = [];
  chat.interaction = null;
  chat.sendError = null;
  chat.settings = null;
  chat.activeThreads = [];
  chat.archivedThreads = [];
  chat.workingFolders = [availableWorkingFolder()];
  chat.selectedThreadId = null;
  chat.selectedWorkingFolderId = "workspace-1";
  chat.timelinePages = [];
}

export async function openSlashMenu(editor: HTMLDivElement): Promise<void> {
  await tick();
  const line = editor.querySelector<HTMLElement>("[data-chat-composer-line]");
  if (!line) throw new Error("Composer line did not render");
  line.textContent = "/";
  setEditorSelection(editor, 1, 1);
  editor.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: "/" }));
  await tick();
  await tick();
}

export function availableWorkingFolder() {
  return {
    workingFolder: {
      id: "workspace-1",
      projectId: "project-1",
      displayName: "Example",
      kind: "external" as const,
      managedRelativePath: null,
      sortOrder: 10,
      repositoryKind: "git" as const,
      repositoryIdentity: "example-repository",
      createdAt: "2026-07-24T12:00:00.000Z",
      updatedAt: "2026-07-24T12:00:00.000Z",
      archivedAt: null,
      revision: 1,
    },
    bindingStatus: "available" as const,
    canonicalPath: "/workspace/example",
    lastVerifiedAt: "2026-07-24T12:00:00.000Z",
    currentBranch: "feat/chat",
  };
}

export function threadShell(): ChatThreadShellRead {
  return {
    id: "thread-1",
    workingFolderId: "workspace-1",
    executionEnvironmentId: "current-folder:workspace-1",
    scratchGenerationId: null,
    projectId: "project-1",
    title: "Example task",
    providerFamilyId: "codex",
    providerInstanceId: "codex-local",
    providerThreadId: "provider-thread-1",
    modelId: "gpt-5.6-sol",
    modelOptions: [],
    modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
    state: "idle",
    latestTurnState: "completed",
    latestPreview: null,
    messageCount: 2,
    revision: 1,
    lastEventSequence: 4,
    lastActivityAt: "2026-07-28T12:00:00.000Z",
    unreadAt: null,
    archivedAt: null,
  };
}

export function interactionState(): ChatInteractionStateRead {
  return {
    sessionId: "session-codex-1",
    sessionState: "ready",
    activeTurnId: null,
    capabilities: { entries: [] },
    pendingRequest: null,
    queuedFollowup: null,
    usage: {
      inputTokens: 80_000,
      outputTokens: 20_000,
      cachedInputTokens: 10_000,
      contextTokens: 100_000,
      contextLimit: 258_000,
      cost: null,
    },
    accountStatus: { accountLabel: "alice@example.com", planLabel: "Plus", usage: null },
    rateLimitStatus: {
      limited: false,
      resetsAt: null,
      detail: null,
      providerData: {
        schemaVersion: 1,
        value: {
          limitId: "codex",
          primary: { usedPercent: 18, windowDurationMins: 10_080, resetsAt: 1_785_200_400 },
        },
      },
    },
    automaticCompactionReported: false,
  };
}

export function composer(): ChatComposerSnapshot {
  return {
    draftId: "workspace:workspace-1:thread:new",
    workingFolderId: "workspace-1",
    threadId: null,
    text: "Review the calendar implementation",
    richContent: null,
    attachmentIds: [],
    mentions: [],
    providerInstanceId: null,
    modelSelection: null,
    safetyMode: null,
    interactionMode: null,
    sentSnapshot: null,
    loading: false,
    saving: false,
    dirty: false,
    error: null,
  };
}

export function setEditorSelection(editor: HTMLDivElement, start: number, end: number): void {
  const selection = document.getSelection();
  if (!selection) throw new Error("Document selection is unavailable");
  const range = document.createRange();
  const startPoint = editorTextPoint(editor, start);
  const endPoint = editorTextPoint(editor, end);
  range.setStart(startPoint.node, startPoint.offset);
  range.setEnd(endPoint.node, endPoint.offset);
  selection.removeAllRanges();
  selection.addRange(range);
}

function editorTextPoint(editor: HTMLDivElement, offset: number): { node: Node; offset: number } {
  const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT);
  let remaining = offset;
  let last: Text | null = null;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!(node instanceof Text)) continue;
    last = node;
    if (remaining <= node.data.length) return { node, offset: remaining };
    remaining -= node.data.length;
  }
  return last ? { node: last, offset: last.data.length } : { node: editor, offset: 0 };
}

export function pointerEvent(type: string, clientX: number, clientY = 0): PointerEvent {
  const event = new MouseEvent(type, { bubbles: true, button: 0, clientX, clientY }) as PointerEvent;
  Object.defineProperties(event, {
    isPrimary: { value: true },
    pointerId: { value: 1 },
  });
  return event;
}

export function imageAttachment(): ChatAttachmentRead {
  return {
    id: "attachment-1",
    workingFolderId: "workspace-1",
    kind: "image",
    originalDisplayName: "diagram.png",
    mimeType: "image/png",
    byteSize: 123,
    sha256: "a".repeat(64),
    managedRelativePath: "assets/chat/attachments/diagram.png",
    signatureKind: "png",
    createdAt: "2026-07-21T12:00:00.000Z",
  };
}

export function modelSettings(): ChatSettingsRead {
  return {
    configuration: {
      schemaVersion: 1,
      providers: [],
      automaticProviderSetupDisabled: [],
      rememberedSelections: [],
      workingFolderProviderPreferences: {},
      panels: { inspectorWidthPx: 520 },
      behavior: {
        sendKey: "enter",
        restoreLastSelectedThread: true,
        showReasoningSummaries: true,
        automaticallyFoldSettledWork: true,
        terminalScrollbackLines: 10_000,
        idleSessionTimeoutSeconds: 1_800,
        confirmMultilineTerminalPaste: true,
      },
    },
    providerFamilies: [{
      familyId: "codex",
      displayName: "OpenAI",
      configurationSchemaVersion: 1,
      supportedPlatforms: ["linux", "windows", "macos"],
      minimumTestedCliVersion: null,
      defaultExecutableCandidates: ["codex"],
      implementationStatus: "available",
      maturity: "stable",
      protocolName: "codex-app-server",
      potentialCapabilities: [],
      unavailableReason: null,
    }, {
      familyId: "claude",
      displayName: "Anthropic",
      configurationSchemaVersion: 1,
      supportedPlatforms: ["linux", "windows", "macos"],
      minimumTestedCliVersion: null,
      defaultExecutableCandidates: ["claude"],
      implementationStatus: "available",
      maturity: "stable",
      protocolName: "claude-stream-json",
      potentialCapabilities: [],
      unavailableReason: null,
    }],
    providerInstances: [{
      configuration: {
        schemaVersion: 1,
        instanceId: "codex-local",
        familyId: "codex",
        label: "OpenAI",
        enabled: true,
        executable: "codex",
        providerHome: null,
        launchArguments: [],
        environment: {},
        credentialReferences: {},
        visibleModelIds: [],
        favoriteModelIds: [],
        providerConfig: { schemaVersion: 1, value: {} },
      },
      lastProbe: {
        instanceId: "codex-local",
        state: "healthy",
        version: "1.0.0",
        negotiatedProtocolVersion: "2",
        accountLabel: null,
        capabilities: { entries: [{ capability: "native_plan", supported: true, explanation: null }] },
        authoritySupport: {
          isolatedConversation: true,
          internalHostTools: true,
          denyShell: true,
          readOnlyRoot: true,
          writableRoot: true,
          confinedCommands: true,
          networkBoundary: true,
          classifiedPublish: true,
        },
        checkedAt: "2026-07-22T12:00:00.000Z",
        detail: null,
      },
      lastSuccessfulProbeAt: "2026-07-22T12:00:00.000Z",
      modelCatalog: {
        instanceId: "codex-local",
        source: "provider",
        discoveredAt: "2026-07-22T12:00:00.000Z",
        stale: false,
        models: [{
          id: "gpt-5.6-sol",
          displayName: "5.6 Sol",
          description: null,
          contextLimit: 258_000,
          availability: "available",
          capabilities: ["images"],
          custom: false,
          options: [{
            kind: "choice",
            key: "reasoning_effort",
            label: "Reasoning effort",
            description: null,
            defaultValue: "medium",
            options: [
              { value: "low", label: "low", description: "Uses less reasoning" },
              { value: "medium", label: "Medium", description: null },
              { value: "high", label: "High", description: null },
              { value: "xhigh", label: "xhigh", description: null },
              { value: "max", label: "max", description: null },
              { value: "ultra", label: "ultra", description: null },
            ],
          }],
        }, {
          id: "gpt-5.6-terra",
          displayName: "GPT-5.6-Terra",
          description: null,
          contextLimit: 258_000,
          availability: "available",
          capabilities: [],
          custom: false,
          options: [{
            kind: "choice",
            key: "reasoning_effort",
            label: "Reasoning effort",
            description: null,
            defaultValue: "medium",
            options: [
              { value: "low", label: "low", description: null },
              { value: "medium", label: "Medium", description: null },
              { value: "high", label: "High", description: null },
              { value: "xhigh", label: "xhigh", description: null },
              { value: "max", label: "max", description: null },
              { value: "ultra", label: "ultra", description: null },
            ],
          }, {
            kind: "choice",
            key: "service_tier",
            label: "Service tier",
            description: null,
            defaultValue: "standard",
            options: [
              { value: "standard", label: "Standard", description: "Default speed" },
              { value: "fast", label: "Fast", description: "Faster responses" },
            ],
          }],
        }, {
          id: "gpt-5.4",
          displayName: "GPT-5.4 Deprecated",
          description: null,
          contextLimit: 200_000,
          availability: "deprecated",
          capabilities: [],
          custom: false,
          options: [],
        }],
      },
    }],
    credentialStoreAvailability: "available",
    lastSelectedThreadId: null,
  };
}

export function claudeModelSettings(): ChatSettingsRead {
  const settings = modelSettings();
  const provider = settings.providerInstances[0];
  if (!provider) throw new Error("Model settings require a provider fixture");
  provider.configuration.instanceId = "claude";
  provider.configuration.familyId = "claude";
  provider.configuration.label = "Anthropic";
  provider.configuration.executable = "claude";
  provider.lastProbe = provider.lastProbe ? { ...provider.lastProbe, instanceId: "claude" } : null;
  provider.modelCatalog = {
    instanceId: "claude",
    source: "provider",
    discoveredAt: "2026-07-23T12:00:00.000Z",
    stale: false,
    models: [{
      id: "default",
      displayName: "Opus 4.8",
      description: "Use the default model (currently Opus 4.8)",
      contextLimit: null,
      availability: "available",
      capabilities: [],
      custom: false,
      options: [{
        kind: "choice",
        key: "effort",
        label: "Effort",
        description: "Provider-supported reasoning effort",
        defaultValue: "high",
        options: ["low", "medium", "high", "xhigh", "max"].map((value) => ({
          value,
          label: value,
          description: null,
        })),
      }, {
        kind: "boolean",
        key: "fastMode",
        label: "Fast mode",
        description: "Lower latency with higher usage cost",
        defaultValue: false,
      }],
    }],
  };
  return settings;
}

export function multiProviderSettings(): ChatSettingsRead {
  const settings = modelSettings();
  const claude = claudeModelSettings().providerInstances[0];
  if (!claude) throw new Error("Claude provider fixture is unavailable");
  settings.providerInstances.push(claude);
  return settings;
}
