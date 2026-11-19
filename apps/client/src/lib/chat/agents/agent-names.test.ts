import { describe, expect, it } from "vitest";
import type { ChatAiAgentRead, ProviderModel } from "$lib/chat/contracts";
import { agentModelLine, agentNameProblem, isModelLineName, newestModelOfLine } from "./agent-names";

function model(id: string, displayName: string, overrides: Partial<ProviderModel> = {}): ProviderModel {
  return {
    id,
    displayName,
    description: null,
    contextLimit: null,
    availability: "available",
    capabilities: [],
    options: [],
    ...overrides,
  };
}

function agent(displayName: string, agentKind: ChatAiAgentRead["agentKind"], providerFamilyId = "claude") {
  return {
    agentKind,
    providerFamilyId,
    participant: {
      id: `agent:${displayName}`,
      kind: "ai_agent" as const,
      displayName,
      avatar: { schemaVersion: 1, value: {} },
      revision: 1,
      archivedAt: null,
    },
  };
}

describe("agentNameProblem", () => {
  it("accepts one trimmed word no other agent or model line holds", () => {
    expect(agentNameProblem("  Reviewer ", ["Claude", "Opus"])).toBeNull();
  });

  it("rejects empty, spaced, @, and overlong names", () => {
    expect(agentNameProblem("   ", [])).toBe("empty");
    expect(agentNameProblem("two words", [])).toBe("characters");
    expect(agentNameProblem("@me", [])).toBe("characters");
    expect(agentNameProblem("a".repeat(33), [])).toBe("tooLong");
    expect(agentNameProblem("a".repeat(32), [])).toBeNull();
  });

  it("rejects a taken name or a model line in any case", () => {
    expect(agentNameProblem("claude", ["Claude"])).toBe("taken");
    expect(agentNameProblem("SOL", [])).toBe("taken");
    expect(agentNameProblem("Fable", [])).toBe("taken");
  });
});

describe("isModelLineName", () => {
  it("knows the built-in lines of every family and nothing else", () => {
    expect(isModelLineName("Haiku")).toBe(true);
    expect(isModelLineName("luna")).toBe(true);
    expect(isModelLineName("Grok")).toBe(false);
  });
});

describe("newestModelOfLine", () => {
  it("picks the newest model of the line and skips unusable ones", () => {
    const models = [
      model("claude-opus-4-7", "Claude Opus 4.7"),
      model("claude-opus-4-9", "Claude Opus 4.9", { availability: "unavailable" }),
      model("claude-opus-4-8", "Claude Opus 4.8"),
      model("claude-sonnet-5", "Claude Sonnet 5"),
    ];
    expect(newestModelOfLine("claude", "opus", models)?.id).toBe("claude-opus-4-8");
  });

  it("orders OpenAI lines by version and ignores other lines", () => {
    const models = [
      model("gpt-5.5-sol", "GPT-5.5-Sol"),
      model("gpt-5.6-sol", "GPT-5.6-Sol"),
      model("gpt-5.6-terra", "GPT-5.6-Terra"),
    ];
    expect(newestModelOfLine("codex", "sol", models)?.id).toBe("gpt-5.6-sol");
    expect(newestModelOfLine("codex", "luna", models)).toBeNull();
  });
});

describe("agentModelLine", () => {
  it("reads the line only from a built-in line agent of its family", () => {
    expect(agentModelLine(agent("Opus", "line"))).toBe("opus");
    expect(agentModelLine(agent("Opus", "line", "codex"))).toBeNull();
    expect(agentModelLine(agent("Reviewer", "custom"))).toBeNull();
    expect(agentModelLine(agent("Opus 5.5", "model"))).toBeNull();
  });
});
