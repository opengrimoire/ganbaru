import { describe, expect, it } from "vitest";
import type { ProviderModelCatalog } from "$lib/chat/contracts";
import { chatModelParticipant } from "./participant-identity";

const catalog: ProviderModelCatalog = {
  instanceId: "opencode-local",
  source: "provider",
  discoveredAt: "2026-07-27T00:00:00.000Z",
  stale: false,
  models: [{
    id: "anthropic/claude-sonnet-5",
    displayName: "Claude Sonnet 5",
    description: "Ultra-fast coding model.",
    contextLimit: null,
    availability: "available",
    capabilities: [],
    options: [{
      kind: "choice",
      key: "reasoning_effort",
      label: "Reasoning effort",
      description: null,
      defaultValue: "low",
      options: [
        { value: "low", label: "Low", description: null },
        { value: "medium", label: "Medium", description: null },
      ],
    }],
    custom: false,
  }],
};

describe("Chat participant identity", () => {
  it("uses the catalog display name and model company", () => {
    const participant = chatModelParticipant(
      "opencode",
      "anthropic/claude-sonnet-5",
      catalog,
    );
    expect(participant.displayName).toBe("Claude Sonnet 5");
    expect(participant.company.id).toBe("anthropic");
    expect(participant.company.iconFamilyId).toBe("claude");
    expect(participant.defaultReasoning).toBe("Medium");
    expect(participant).not.toHaveProperty("description");
  });

  it("infers a company from a historical model ID after catalog removal", () => {
    const participant = chatModelParticipant("opencode", "google/gemini-3-pro", null);
    expect(participant.displayName).toBe("google/gemini-3-pro");
    expect(participant.company.id).toBe("google");
  });

  it("falls back to the integration company without a model ID", () => {
    expect(chatModelParticipant("codex", null, null)).toMatchObject({
      displayName: "OpenAI",
      company: { id: "openai" },
    });
  });
});
