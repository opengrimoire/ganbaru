import { describe, expect, it } from "vitest";
import type { ProbeState, ProviderInstanceRead, ProviderModel } from "$lib/chat/contracts";
import {
  buildModelCompanySections,
  familiesInPickerOrder,
  firstPickerChoice,
  providersInPickerOrder,
  resolveRecommendedSelection,
} from "./model-picker-model";

const NOW_MS = Date.parse("2026-11-17T12:00:00Z");

function model(id: string, overrides: Partial<ProviderModel> = {}): ProviderModel {
  return {
    id,
    displayName: id,
    description: null,
    contextLimit: null,
    availability: "available",
    capabilities: [],
    options: [],
    custom: false,
    ...overrides,
  };
}

function provider(
  instanceId: string,
  familyId: string,
  options: {
    state?: ProbeState;
    models?: ProviderModel[];
    recommendedModelId?: string | null;
    favoriteModelIds?: string[];
    lastUsedAt?: string | null;
  } = {},
): ProviderInstanceRead {
  const state = options.state ?? "healthy";
  return {
    configuration: {
      schemaVersion: 1,
      instanceId,
      familyId,
      label: instanceId,
      enabled: true,
      executable: familyId,
      providerHome: null,
      launchArguments: [],
      environment: {},
      credentialReferences: {},
      visibleModelIds: [],
      favoriteModelIds: options.favoriteModelIds ?? [],
      providerConfig: { schemaVersion: 1, value: {} },
    },
    runtimeReview: [],
    lastProbe: {
      instanceId,
      state,
      version: null,
      negotiatedProtocolVersion: null,
      accountLabel: null,
      authSource: null,
      accountPlan: null,
      capabilities: { entries: [] },
      authoritySupport: {
        isolatedConversation: true,
        internalHostTools: true,
        denyShell: true,
        readOnlyRoot: true,
        writableRoot: true,
        confinedCommands: true,
        networkBoundary: true,
        classifiedPublish: true,
        personApprovedCommands: false,
      },
      checkedAt: "2026-11-17T11:00:00Z",
      detail: null,
      versionAdmission: null,
      agentMinimumVersion: null,
    },
    lastSuccessfulProbeAt: state === "healthy" ? "2026-11-17T11:00:00Z" : null,
    modelCatalog: options.models
      ? { instanceId, models: options.models, recommendedModelId: options.recommendedModelId ?? null, source: "provider", discoveredAt: "2026-11-17T11:00:00Z", stale: false }
      : null,
    usageLimits: null,
    lastUsedAt: options.lastUsedAt ?? null,
    duplicateOf: null,
  };
}

function ids(providers: readonly ProviderInstanceRead[]): string[] {
  return providers.map((entry) => entry.configuration.instanceId);
}

describe("picker order", () => {
  it("orders providers by readiness, then recent use, then model company", () => {
    const claude = provider("claude-main", "claude");
    const codex = provider("codex-main", "codex");
    const grokUsed = provider("grok-main", "grok", { lastUsedAt: "2026-11-17T10:00:00Z" });
    const cursorSignedOut = provider("cursor-main", "cursor", { state: "authentication_required" });
    expect(ids(providersInPickerOrder([cursorSignedOut, claude, codex, grokUsed], NOW_MS)))
      .toEqual(["grok-main", "codex-main", "claude-main", "cursor-main"]);
  });

  it("keeps backend order between equally ready instances of one family", () => {
    const first = provider("codex-first", "codex");
    const second = provider("codex-second", "codex");
    expect(ids(providersInPickerOrder([first, second], NOW_MS))).toEqual(["codex-first", "codex-second"]);
  });

  it("lists configured families by their best instance before families without one", () => {
    const families = [{ familyId: "codex" }, { familyId: "claude" }, { familyId: "grok" }, { familyId: "cursor" }];
    const providers = [
      provider("cursor-main", "cursor", { lastUsedAt: "2026-11-17T10:00:00Z" }),
      provider("claude-main", "claude"),
    ];
    expect(familiesInPickerOrder(families, providers, NOW_MS).map((family) => family.familyId))
      .toEqual(["cursor", "claude", "codex", "grok"]);
  });
});

describe("first picker choice", () => {
  it("takes the first usable favorite before any company section", () => {
    const codex = provider("codex-main", "codex", { models: [model("gpt-5.5")] });
    const claude = provider("claude-main", "claude", {
      models: [model("claude-opus-5-5"), model("claude-haiku-5-5")],
      favoriteModelIds: ["claude-haiku-5-5"],
    });
    const choice = firstPickerChoice([codex, claude], [], NOW_MS);
    expect(choice?.provider.configuration.instanceId).toBe("claude-main");
    expect(choice?.model?.id).toBe("claude-haiku-5-5");
    expect(choice?.providerManaged).toBe(false);
  });

  it("skips unusable favorites and takes the first model of the first section", () => {
    const codex = provider("codex-main", "codex", {
      models: [model("gpt-5.5"), model("gpt-old", { availability: "unavailable" })],
      favoriteModelIds: ["gpt-old"],
    });
    const claude = provider("claude-main", "claude", {
      models: [model("claude-opus-5-5")],
      lastUsedAt: "2026-11-17T10:00:00Z",
    });
    const choice = firstPickerChoice([codex, claude], [], NOW_MS);
    expect(choice?.provider.configuration.instanceId).toBe("claude-main");
    expect(choice?.model?.id).toBe("claude-opus-5-5");
  });

  it("offers a provider-managed choice for a provider without a catalog", () => {
    const choice = firstPickerChoice([provider("cursor-main", "cursor")], [], NOW_MS);
    expect(choice).toMatchObject({ model: null, providerManaged: true });
    expect(choice?.provider.configuration.instanceId).toBe("cursor-main");
  });

  it("returns nothing when no provider can take work", () => {
    const signedOut = provider("codex-main", "codex", { state: "authentication_required", models: [model("gpt-5.5")] });
    expect(firstPickerChoice([signedOut], [], NOW_MS)).toBeNull();
  });
});

describe("models without a provider default", () => {
  const unselected = { providerInstanceId: null, modelId: null, providerManaged: false };

  function configuredRows(providers: ProviderInstanceRead[]): string[] {
    return buildModelCompanySections(providers, [], unselected, "", "Model set in the provider", "en", NOW_MS)
      .flatMap((section) => section.managedProviders.map((entry) => entry.configuration.instanceId));
  }

  it("offers the provider's configured model only for providers without discovered models", () => {
    const codex = provider("codex-main", "codex", { models: [model("gpt-5.6-sol")] });
    const cursor = provider("cursor-main", "cursor");
    expect(configuredRows([codex, cursor])).toEqual(["cursor-main"]);
  });

  it("shows a selection without a model as the catalog's recommended model", () => {
    const claude = provider("claude-main", "claude", {
      models: [model("fable"), model("opus"), model("sonnet")],
      recommendedModelId: "opus",
    });
    const managed = { modelId: null, providerManaged: true, options: [] };
    expect(resolveRecommendedSelection(claude, managed)).toEqual({ modelId: "opus", providerManaged: false, options: [] });
    expect(resolveRecommendedSelection(provider("cursor-main", "cursor"), managed)).toBe(managed);
    const named = { modelId: "sonnet", providerManaged: false };
    expect(resolveRecommendedSelection(claude, named)).toBe(named);
  });

  it("falls back to the strongest visible model when the recommendation is hidden", () => {
    const claude = provider("claude-main", "claude", {
      models: [model("sonnet"), model("fable"), model("opus")],
      recommendedModelId: "opus",
    });
    claude.configuration.visibleModelIds = ["sonnet", "fable"];
    const managed = { modelId: null, providerManaged: true, options: [] };
    expect(resolveRecommendedSelection(claude, managed).modelId).toBe("fable");
  });
});
