import { describe, expect, it } from "vitest";
import type { ProviderModel } from "$lib/chat/contracts";
import {
  compareCompanyModels,
  formatModelDisplayName,
  integrationCompany,
  modelCompany,
  modelCompanyForIdentity,
  shouldShowModelId,
  type ModelCompanyId,
} from "./model-company";

function model(id: string, displayName = id): ProviderModel {
  return {
    id,
    displayName,
    description: null,
    contextLimit: null,
    availability: "available",
    capabilities: [],
    options: [],
    custom: false,
  };
}

describe("Chat model companies", () => {
  it("maps single-company execution integrations to their companies", () => {
    expect(integrationCompany("codex").name).toBe("OpenAI");
    expect(integrationCompany("claude").name).toBe("Anthropic");
    expect(integrationCompany("grok").name).toBe("xAI");
    expect(integrationCompany("cursor").name).toBe("Cursor");
    expect(integrationCompany("opencode").name).toBe("OpenCode");
  });

  it("infers companies from stored model identities without a catalog row", () => {
    expect(modelCompanyForIdentity("opencode", "google/gemini-3-pro").id).toBe("google");
    expect(modelCompanyForIdentity("opencode", "anthropic/claude-opus-5").id).toBe("anthropic");
  });

  it("classifies models exposed through multi-vendor integrations", () => {
    expect(modelCompany("cursor", model("anthropic/claude-opus-4-1")).name).toBe("Anthropic");
    expect(modelCompany("cursor", model("gpt-5.4")).name).toBe("OpenAI");
    expect(modelCompany("opencode", model("google/gemini-2.5-pro")).name).toBe("Google");
    expect(modelCompany("opencode", model("qwen3-coder")).name).toBe("Alibaba");
    expect(modelCompany("cursor", model("composer-2")).name).toBe("Cursor");
  });

  it("hides exact IDs that only repeat the formatted display name", () => {
    expect(shouldShowModelId(model("gpt-5.6-sol", "GPT-5.6-Sol"))).toBe(false);
    expect(shouldShowModelId(model("claude_4_5_sonnet", "Claude 4.5 Sonnet"))).toBe(false);
  });

  it("hides provider-managed aliases and keeps distinct custom model IDs", () => {
    expect(shouldShowModelId(model("anthropic/claude-sonnet-4-5", "Claude Sonnet 4.5"))).toBe(false);
    expect(shouldShowModelId(model("claude-sonnet-4-5-20250929", "Claude Sonnet 4.5"))).toBe(false);
    expect(shouldShowModelId(model("default", "Claude Opus 4.8"))).toBe(false);
    expect(shouldShowModelId({ ...model("team-model-id", "Team model"), custom: true })).toBe(true);
  });

  it("formats provider display names without identifier-style hyphens", () => {
    expect(formatModelDisplayName("GPT-5.6-Sol")).toBe("GPT 5.6 Sol");
    expect(formatModelDisplayName("Claude-Sonnet-4.5")).toBe("Claude Sonnet 4.5");
  });

  it("orders OpenAI models newest-first and strongest-first within a generation", () => {
    expect(sorted("openai", [
      model("gpt-5.3-codex-spark", "GPT-5.3-Codex-Spark"),
      model("gpt-5.6-terra", "GPT-5.6-Terra"),
      model("gpt-5.5", "GPT-5.5"),
      model("gpt-5.6-luna", "GPT-5.6-Luna"),
      model("gpt-5.6-sol", "GPT-5.6-Sol"),
    ])).toEqual([
      "gpt-5.6-sol",
      "gpt-5.6-terra",
      "gpt-5.6-luna",
      "gpt-5.5",
      "gpt-5.3-codex-spark",
    ]);
  });

  it("orders Anthropic by capability family and newest version within each family", () => {
    expect(sorted("anthropic", [
      model("haiku", "Haiku 4.5"),
      model("sonnet", "Sonnet 5"),
      model("opus-old", "Opus 4.7"),
      model("default", "Opus 4.8"),
    ])).toEqual(["default", "opus-old", "sonnet", "haiku"]);
  });

  it("keeps Grok Build ahead of general and retired coding families", () => {
    expect(sorted("xai", [
      model("grok-code-fast-1"),
      model("grok-4.3"),
      model("grok-build-0.1"),
    ])).toEqual(["grok-build-0.1", "grok-4.3", "grok-code-fast-1"]);
  });
});

function sorted(companyId: ModelCompanyId, models: ProviderModel[]): string[] {
  return models.sort((left, right) => compareCompanyModels(companyId, left, right)).map((entry) => entry.id);
}
