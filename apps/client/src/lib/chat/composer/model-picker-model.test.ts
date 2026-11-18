import { describe, expect, it } from "vitest";
import type { ProbeState, ProviderAccountPlan, ProviderInstanceRead, ProviderModel } from "$lib/chat/contracts";
import {
  familiesInPickerOrder,
  providersInPickerOrder,
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
    lastUsedAt?: string | null;
    accountPlan?: ProviderAccountPlan | null;
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
      accountPlan: options.accountPlan ?? null,
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
      ? { instanceId, models: options.models, recommendedModelId: null, source: "provider", discoveredAt: "2026-11-17T11:00:00Z", stale: false }
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

  it("ranks a paid plan above a free one, before recent use and the company order", () => {
    const codexFree = provider("codex-main", "codex", {
      accountPlan: "free",
      lastUsedAt: "2026-11-17T11:30:00Z",
      models: [model("gpt-5.6-sol")],
    });
    const claudePaid = provider("claude-main", "claude", { accountPlan: "subscription", models: [model("opus")] });
    expect(ids(providersInPickerOrder([codexFree, claudePaid], NOW_MS))).toEqual(["claude-main", "codex-main"]);
  });

  it("lets the most recent use decide between equal plans", () => {
    const codex = provider("codex-main", "codex", { accountPlan: "subscription" });
    const claudeUsed = provider("claude-main", "claude", { accountPlan: "subscription", lastUsedAt: "2026-11-17T11:30:00Z" });
    expect(ids(providersInPickerOrder([codex, claudeUsed], NOW_MS))).toEqual(["claude-main", "codex-main"]);
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

