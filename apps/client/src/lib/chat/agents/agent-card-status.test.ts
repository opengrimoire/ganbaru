import { describe, expect, it } from "vitest";
import type {
  ChatAiAgentRead,
  ChatLinkedProviderStatusRead,
  ProviderInstanceRead,
  ProviderModel,
  ProviderUsageLimitsObservation,
} from "$lib/chat/contracts";
import { agentCardStatus } from "./agent-card-status";
import { chatAgentModelParticipant } from "./identity";

const OBSERVED_AT = "2026-11-19T10:00:00.000Z";

function model(id: string, displayName: string): ProviderModel {
  return {
    id,
    displayName,
    description: null,
    contextLimit: null,
    availability: "available",
    capabilities: [],
    options: [],
  };
}

function limits(usedPercent: number): ProviderUsageLimitsObservation {
  return {
    observedAt: OBSERVED_AT,
    status: {
      limited: false,
      resetsAt: null,
      detail: null,
      providerData: {
        schemaVersion: 1,
        value: { rateLimitsByLimitId: { codex: { primary: { usedPercent } } } },
      },
    },
  };
}

function linked(available: boolean, models: ProviderModel[] = []): ChatLinkedProviderStatusRead {
  return { deviceId: "device-desktop", available, models, usageLimits: limits(30) };
}

function agent(
  options: { healthy?: boolean; linked?: ChatLinkedProviderStatusRead | null; modelId?: string | null } = {},
): ChatAiAgentRead {
  return {
    participant: {
      id: "participant:atlas",
      kind: "ai_agent",
      displayName: "Atlas",
      avatar: { schemaVersion: 1, value: {} },
      revision: 1,
      archivedAt: null,
    },
    providerFamilyId: "codex",
    agentKind: "custom",
    localProviderInstanceId: options.healthy ? "codex-local" : null,
    configurationState: options.healthy ? "healthy" : "provider_unavailable",
    latestPolicy: options.modelId === undefined || options.modelId === null
      ? null
      : {
        id: "policy:atlas:1",
        agentId: "participant:atlas",
        revision: 1,
        providerManagedModel: false,
        modelId: options.modelId,
        modelOptions: [],
        providerOptions: { schemaVersion: 1, value: {} },
        interactionMode: "build",
        createdAt: OBSERVED_AT,
      },
    channelCount: 1,
    activeAssignmentCount: 0,
    hiddenAt: null,
    linkedProviderStatus: options.linked ?? null,
  };
}

function localProvider(models: ProviderModel[]): ProviderInstanceRead {
  return {
    configuration: {
      schemaVersion: 1,
      instanceId: "codex-local",
      familyId: "codex",
      label: "Codex",
      enabled: true,
      executable: "codex",
      providerHome: null,
      launchArguments: [],
      environment: {},
      credentialReferences: {},
      providerConfig: { schemaVersion: 1, value: {} },
    },
    runtimeReview: [],
    lastProbe: null,
    lastSuccessfulProbeAt: null,
    modelCatalog: {
      instanceId: "codex-local",
      models,
      recommendedModelId: null,
      source: "provider",
      discoveredAt: OBSERVED_AT,
      stale: false,
    },
    usageLimits: limits(10),
    lastUsedAt: null,
    duplicateOf: null,
  };
}

describe("agentCardStatus on a device that runs agents", () => {
  it("is available with its own catalog and limits when its provider is healthy", () => {
    const provider = localProvider([model("gpt-5", "GPT-5")]);
    const status = agentCardStatus(agent({ healthy: true }), provider, false, true);
    expect(status.availability).toBe("available");
    expect(status.models.map((entry) => entry.displayName)).toEqual(["GPT-5"]);
    expect(status.usageLimits).toEqual(limits(10));
  });

  it("needs setup without a healthy provider, even when a linked desktop could run the agent", () => {
    const status = agentCardStatus(agent({ linked: linked(true, [model("gpt-5", "GPT-5")]) }), null, false, true);
    expect(status).toEqual({ availability: "needs_setup", models: [], usageLimits: null });
  });

  it("says archived before anything else", () => {
    expect(agentCardStatus(agent({ healthy: true }), localProvider([]), true, true).availability).toBe("archived");
  });
});

describe("agentCardStatus on a device that leaves the work to a linked desktop", () => {
  it("is available with the desktop's catalog and limits when that desktop can run the agent", () => {
    const status = agentCardStatus(agent({ linked: linked(true, [model("gpt-5", "GPT-5")]) }), null, false, false);
    expect(status.availability).toBe("available");
    expect(status.models.map((entry) => entry.id)).toEqual(["gpt-5"]);
    expect(status.usageLimits).toEqual(limits(30));
  });

  it("names no linked desktop when the desktop cannot run the provider or shares nothing", () => {
    expect(agentCardStatus(agent({ linked: linked(false) }), null, false, false).availability)
      .toBe("no_linked_desktop");
    expect(agentCardStatus(agent(), null, false, false)).toEqual({
      availability: "no_linked_desktop",
      models: [],
      usageLimits: null,
    });
    expect(agentCardStatus(null, null, false, false).availability).toBe("no_linked_desktop");
  });

  it("says archived even when the desktop can run the agent", () => {
    expect(agentCardStatus(agent({ linked: linked(true) }), null, true, false).availability).toBe("archived");
  });
});

describe("chatAgentModelParticipant with a linked desktop catalog", () => {
  const atlas = agent({ linked: linked(true, [model("gpt-5", "GPT-5 Sol")]), modelId: "gpt-5" });

  it("names the agent's model from the catalog the linked desktop shares", () => {
    expect(chatAgentModelParticipant(atlas.participant.id, [atlas], null, false)?.displayName).toBe("GPT-5 Sol");
  });

  it("keeps a device that runs agents on its own catalog", () => {
    expect(chatAgentModelParticipant(atlas.participant.id, [atlas], null, true)).toBeNull();
  });
});
