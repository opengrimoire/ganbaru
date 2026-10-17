import { describe, expect, it } from "vitest";
import type { ProviderCapabilities, ProviderInstanceRead, ProviderModel } from "$lib/chat/contracts";
import {
  clipboardImageFiles,
  composerActionState,
  composerRateLimitWindows,
  composerModeCommand,
  composerTokenTrigger,
  contextMeter,
  copyModelOptionSelections,
  copyVersionedJson,
  defaultModelOptions,
  filterPromptCatalog,
  filterWorkspacePaths,
  interactionModeForPrompt,
  parseApprovalChoices,
  parseUserInputQuestions,
  queuedFollowupDispatchReady,
  rankedModels,
  resolveDefaultProviderModel,
  shouldSendComposerKey,
  supportsImagePrompt,
  validateComposerSelections,
  validateImageFiles,
  validateModelOptions,
  validateUserInputAnswers,
} from "./model";

function capabilities(...values: ProviderCapabilities["entries"][number]["capability"][]): ProviderCapabilities {
  return { entries: values.map((capability) => ({ capability, supported: true, explanation: null })) };
}

function provider(
  instanceId: string,
  familyId: string,
  models: ProviderModel[],
  state: "healthy" | "transport_unavailable" = "healthy",
): ProviderInstanceRead {
  return {
    configuration: {
      schemaVersion: 1,
      instanceId,
      familyId,
      label: instanceId,
      enabled: true,
      executable: instanceId,
      providerHome: null,
      launchArguments: [],
      environment: {},
      credentialReferences: {},
      visibleModelIds: [],
      favoriteModelIds: [],
      providerConfig: { schemaVersion: 1, value: {} },
    },
    lastProbe: {
      instanceId,
      state,
      version: null,
      negotiatedProtocolVersion: null,
      accountLabel: null,
      capabilities: capabilities(),
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
      checkedAt: "2026-08-01T00:00:00Z",
      detail: null,
    },
    lastSuccessfulProbeAt: state === "healthy" ? "2026-08-01T00:00:00Z" : null,
    modelCatalog: {
      instanceId,
      models,
      source: "provider",
      discoveredAt: "2026-08-01T00:00:00Z",
      stale: false,
    },
  };
}

describe("Chat composer model", () => {
  it("prioritizes requests and Stop while selecting capability-driven follow-up behavior", () => {
    expect(composerActionState("waiting_for_approval", capabilities("steering"), true, true).primary).toBe("resolve_request");
    expect(composerActionState("active", capabilities("steering"), true, false)).toMatchObject({ primary: "stop", followup: "steer" });
    expect(composerActionState("active", capabilities("queued_follow_up"), true, false).followup).toBe("queue");
    expect(composerActionState("active", capabilities(), true, false).followup).toBe("retain");
    expect(composerActionState("ready", capabilities(), true, false)).toMatchObject({ primary: "send", sendEnabled: true });
    expect(queuedFollowupDispatchReady("ready", "completed")).toBe(true);
    expect(queuedFollowupDispatchReady("stopped", "failed")).toBe(true);
    expect(queuedFollowupDispatchReady("stopped", "active")).toBe(false);
  });

  it("requires every explicit first-use choice and workspace-specific broad permission trust", () => {
    expect(validateComposerSelections({
      workingFolderId: null,
      providerInstanceId: null,
      modelId: null,
      providerManagedModel: false,
      safetyMode: null,
      interactionMode: null,
      fullAccessTrusted: false,
    }, capabilities())).toHaveLength(5);
    expect(validateComposerSelections({
      workingFolderId: "workspace-1",
      providerInstanceId: "codex",
      modelId: "gpt-5",
      providerManagedModel: false,
      safetyMode: "full_access",
      interactionMode: "plan",
      fullAccessTrusted: false,
    }, capabilities())).toEqual([
      { field: "interaction", message: "This provider does not support native Plan mode" },
      { field: "trust", message: "Confirm broad permission trust for this provider and workspace" },
    ]);
  });

  it("detects mention, skill, and command tokens at the caret", () => {
    expect(composerTokenTrigger("Inspect @src/cal", 16)).toMatchObject({ kind: "mention", query: "src/cal" });
    expect(composerTokenTrigger("Use $doc", 8)).toMatchObject({ kind: "skill", query: "doc" });
    expect(composerTokenTrigger("/review", 7)).toMatchObject({ kind: "command", query: "review" });
    expect(composerTokenTrigger("  /review", 9)).toMatchObject({ kind: "command", query: "review", start: 2 });
    expect(composerTokenTrigger("Run /review", 11)).toBeNull();
  });

  it("extracts universal mode commands without sending the command prefix to providers", () => {
    expect(composerModeCommand("/plan inspect the architecture")).toEqual({
      mode: "plan",
      prompt: "inspect the architecture",
    });
    expect(composerModeCommand(" /BUILD implement it")).toEqual({
      mode: "build",
      prompt: "implement it",
    });
    expect(composerModeCommand("Explain /plan behavior")).toBeNull();
  });

  it("maps the universal plan command to Plan mode and otherwise preserves the current mode", () => {
    expect(interactionModeForPrompt("  /plan inspect the architecture", "build")).toBe("plan");
    expect(interactionModeForPrompt("/PLAN", null)).toBe("plan");
    expect(interactionModeForPrompt("Implement the plan", "plan")).toBe("plan");
    expect(interactionModeForPrompt("Implement the change", null)).toBe("build");
  });

  it("fuzzy-ranks provider menus and workspace paths while preserving stale entries", () => {
    const catalog = [
      { value: "/review", label: "Review changes", description: null, argumentHint: null, kind: "command" as const, source: "provider" as const, stale: true },
      { value: "$docs", label: "Documentation", description: null, argumentHint: null, kind: "skill" as const, source: "user" as const, stale: false },
    ];
    expect(filterPromptCatalog(catalog, "command", "rvw")).toMatchObject([{ value: "/review", stale: true }]);
    expect(filterPromptCatalog(catalog, "skill", "doc")).toMatchObject([{ value: "$docs" }]);
    expect(filterWorkspacePaths([
      { relativePath: "src/calendar/view.ts", displayName: "view.ts", kind: "file", ignored: false },
      { relativePath: "docs/calendar.md", displayName: "calendar.md", kind: "file", ignored: true },
    ], "cal").map((entry) => entry.relativePath)).toEqual(["src/calendar/view.ts", "docs/calendar.md"]);
  });

  it("ranks favorite and recent models before the remaining catalog without replacing raw IDs", () => {
    const models = ["z-model", "favorite", "recent"].map((id) => ({
      id, displayName: id, description: null, contextLimit: null, availability: "available" as const,
      capabilities: [], options: [], custom: false,
    }));
    expect(rankedModels(models, ["favorite"], ["recent"], "").map((model) => model.id)).toEqual(["favorite", "recent", "z-model"]);
  });

  it("defaults to the strongest OpenAI model at Medium effort when OpenAI is connected", () => {
    const effort = {
      kind: "choice" as const,
      key: "reasoningEffort",
      label: "Reasoning effort",
      description: null,
      options: [
        { value: "low", label: "Low", description: null },
        { value: "medium", label: "Medium", description: null },
        { value: "high", label: "High", description: null },
      ],
      defaultValue: "high",
    };
    const model = (id: string): ProviderModel => ({
      id,
      displayName: id,
      description: null,
      contextLimit: null,
      availability: "available",
      capabilities: [],
      options: [effort],
      custom: false,
    });
    const claude = provider("claude-local", "claude", [model("claude-opus-4-1")]);
    const codex = provider("codex-local", "codex", [model("gpt-5.5-sol"), model("gpt-5.6-sol")]);

    const resolved = resolveDefaultProviderModel([claude, codex]);

    expect(resolved?.provider.configuration.instanceId).toBe("codex-local");
    expect(resolved?.model?.id).toBe("gpt-5.6-sol");
    expect(resolved?.options).toContainEqual({
      key: "reasoningEffort",
      value: { kind: "choice", value: "medium" },
    });
    expect(defaultModelOptions([effort])).toEqual(resolved?.options);
    expect(resolveDefaultProviderModel([claude, codex], "claude-local")?.provider.configuration.instanceId)
      .toBe("claude-local");
    expect(resolveDefaultProviderModel([
      provider("codex-offline", "codex", [model("gpt-5.6-sol")], "transport_unavailable"),
      claude,
    ])?.provider.configuration.instanceId).toBe("claude-local");
  });

  it("copies reactive model option proxies before sending setup commands", () => {
    const selections = new Proxy([new Proxy({
      key: "tools",
      value: new Proxy({ kind: "multiple_choice" as const, value: new Proxy(["shell"], {}) }, {}),
    }, {})], {});

    const copied = copyModelOptionSelections(selections);

    expect(copied).toEqual([{ key: "tools", value: { kind: "multiple_choice", value: ["shell"] } }]);
    expect(() => structuredClone(copied)).not.toThrow();
    expect(() => structuredClone(copyVersionedJson(new Proxy({
      schemaVersion: 1,
      value: new Proxy({ kind: "initials" }, {}),
    }, {})))).not.toThrow();
  });

  it("parses safe approval and structured-question payloads", () => {
    expect(parseApprovalChoices({ schemaVersion: 1, value: [{ id: "allow", label: "Allow once", decisionKind: "allow_once", description: null }] })).toHaveLength(1);
    expect(parseUserInputQuestions({ schemaVersion: 1, value: [{ id: "q1", question: "Choose", options: [{ id: "a", label: "A" }], multiple: false, freeFormAllowed: true, required: true }] })).toMatchObject([{ id: "q1", options: [{ id: "a" }] }]);
  });

  it("validates typed traits and structured answers without accepting unknown options", () => {
    expect(validateModelOptions([
      { kind: "choice", key: "effort", label: "Effort", description: null, options: [{ value: "high", label: "High", description: null }], defaultValue: "high" },
    ], [{ key: "effort", value: { kind: "choice", value: "invalid" } }])).toEqual(["effort"]);
    const questions = parseUserInputQuestions({
      schemaVersion: 1,
      value: [{ id: "q1", question: "Choose", options: [{ id: "a", label: "A" }], multiple: false, freeFormAllowed: false, required: true }],
    });
    expect(validateUserInputAnswers(questions, [{ questionId: "q1", selectedOptionIds: [], freeFormText: null }])).toEqual(["q1"]);
    expect(validateUserInputAnswers(questions, [{ questionId: "q1", selectedOptionIds: ["a"], freeFormText: null }])).toEqual([]);
    expect(validateUserInputAnswers(questions, [{ questionId: "q1", selectedOptionIds: ["missing"], freeFormText: null }])).toEqual(["q1"]);
  });

  it("shows only provider-reported context facts and caps visual ratios", () => {
    expect(contextMeter(null, 100)).toBeNull();
    expect(contextMeter(90, 100)).toMatchObject({ ratio: 0.9, warning: true });
    expect(contextMeter(120, 100)?.ratio).toBe(1);
    expect(contextMeter(40, null)).toMatchObject({ maximumTokens: null, ratio: null });
  });

  it("extracts bounded primary and secondary provider rate limit windows", () => {
    expect(composerRateLimitWindows({
      limited: false,
      resetsAt: null,
      detail: null,
      providerData: {
        schemaVersion: 1,
        value: {
          limitId: "codex",
          primary: { usedPercent: 31, windowDurationMins: 300, resetsAt: 1_730_948_100 },
          secondary: { usedPercent: 108, windowDurationMins: 10_080, resetsAt: 1_731_000_000 },
        },
      },
    })).toEqual([
      {
        id: "codex:primary",
        label: "5h codex",
        usedPercent: 31,
        windowDurationMinutes: 300,
        resetsAtSeconds: 1_730_948_100,
      },
      {
        id: "codex:secondary",
        label: "7d codex",
        usedPercent: 100,
        windowDurationMinutes: 10_080,
        resetsAtSeconds: 1_731_000_000,
      },
    ]);
  });

  it("implements send keys and image limits", () => {
    expect(shouldSendComposerKey({ key: "Enter", shiftKey: false, ctrlKey: false, metaKey: false, isComposing: false }, "enter")).toBe(true);
    expect(shouldSendComposerKey({ key: "Enter", shiftKey: true, ctrlKey: false, metaKey: false, isComposing: false }, "enter")).toBe(false);
    expect(validateImageFiles(Array.from({ length: 2 }, (_, index) => ({ name: `${index}.png`, size: 10, type: "image/png" })) as File[], 7)).toContain("8 images");
    expect(validateImageFiles([{ name: "large.png", size: 2 * 1024 * 1024, type: "image/png" }] as File[], 1, 49 * 1024 * 1024)).toContain("50 MiB");
  });

  it("respects model-specific Codex image input support", () => {
    const spark = {
      id: "gpt-5.3-codex-spark",
      displayName: "GPT-5.3-Codex-Spark",
      description: null,
      contextLimit: null,
      availability: "available" as const,
      capabilities: [],
      options: [],
      custom: false,
    };
    const sol = { ...spark, id: "gpt-5.6-sol", capabilities: ["images" as const] };

    expect(supportsImagePrompt("codex", spark, capabilities("images"))).toBe(false);
    expect(supportsImagePrompt("codex", sol, capabilities())).toBe(true);
    expect(supportsImagePrompt("cursor", spark, capabilities("images"))).toBe(true);
    expect(supportsImagePrompt("codex", { ...spark, custom: true }, capabilities("images"))).toBe(true);
  });

  it("reads screenshot images exposed only as clipboard items", () => {
    const screenshot = new File([new Uint8Array([1, 2, 3])], "", { type: "image/png" });
    const clipboard = {
      files: [] as unknown as FileList,
      items: [{
        kind: "file",
        type: "image/png",
        getAsFile: () => screenshot,
      }] as unknown as DataTransferItemList,
    };

    const images = clipboardImageFiles(clipboard);
    expect(images).toHaveLength(1);
    expect(images[0]?.name).toBe("pasted-image.png");
    expect(images[0]?.type).toBe("image/png");
  });
});
