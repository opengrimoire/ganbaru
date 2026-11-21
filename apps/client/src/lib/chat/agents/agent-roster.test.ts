import { describe, expect, it } from "vitest";
import type { ChatAgentKind, ModelOptionDefinition, ProviderModel } from "$lib/chat/contracts";
import { customAgentModelChoices, familyAgentRoster, isRemovableAgent, listedCatalogModels, selectedEffortLabel } from "./agent-roster";

function model(id: string, displayName: string): ProviderModel {
  return { id, displayName, description: null, contextLimit: null, availability: "available", capabilities: [], options: [] };
}

function agent(displayName: string, agentKind: ChatAgentKind, modelId: string | null = null, providerFamilyId = "claude") {
  return {
    agentKind,
    providerFamilyId,
    latestPolicy: modelId ? { modelId } : null,
    participant: {
      id: `agent:${providerFamilyId}:${displayName}`,
      kind: "ai_agent" as const,
      displayName,
      avatar: { schemaVersion: 1, value: {} },
      revision: 1,
      archivedAt: null,
    },
  };
}

const compare = (left: string, right: string) => left.localeCompare(right, "en");

describe("familyAgentRoster", () => {
  it("orders line agents by line strength and named agents by name, within the family only", () => {
    const roster = familyAgentRoster("claude", [
      agent("Haiku", "line"),
      agent("Zed", "custom", "claude-opus-5-5"),
      agent("Fable", "line"),
      agent("Ada", "custom", "claude-sonnet-5-5"),
      agent("Sol", "line", null, "codex"),
      agent("Opus", "line"),
    ], compare);

    expect(roster.lineAgents.map((entry) => entry.participant.displayName)).toEqual(["Fable", "Opus", "Haiku"]);
    expect(roster.namedAgents.map((entry) => entry.participant.displayName)).toEqual(["Ada", "Zed"]);
  });

  it("maps each catalog model to the first model agent that runs it and skips named agents of the same model", () => {
    const first = agent("Opus 5.5", "model", "claude-opus-5-5");
    const roster = familyAgentRoster("claude", [
      agent("Reviewer", "custom", "claude-opus-5-5"),
      first,
      agent("Opus 5.5 copy", "model", "claude-opus-5-5"),
      agent("Unpinned", "model"),
    ], compare);

    expect([...roster.modelAgents.keys()]).toEqual(["claude-opus-5-5"]);
    expect(roster.modelAgents.get("claude-opus-5-5")).toBe(first);
  });
});

describe("listedCatalogModels", () => {
  it("leaves out the aliases a line agent already runs, ignoring case", () => {
    const models = [{ id: "opus" }, { id: "Sonnet" }, { id: "claude-opus-5-5" }, { id: "sol" }];
    expect(listedCatalogModels("claude", models).map((model) => model.id)).toEqual(["claude-opus-5-5", "sol"]);
    expect(listedCatalogModels("cursor", models)).toEqual(models);
  });
});

describe("customAgentModelChoices", () => {
  it("offers each offered line by its newest model first, strongest line first, then the catalog newest first", () => {
    const choices = customAgentModelChoices("claude", [
      model("claude-sonnet-5-5", "Sonnet 5.5"),
      model("opus", "Opus"),
      model("claude-opus-5-0", "Opus 5"),
      model("claude-opus-5-5", "Opus 5.5"),
    ], compare);

    expect(choices.map((choice) => [choice.modelId, choice.line, choice.model.id])).toEqual([
      ["opus", "opus", "claude-opus-5-5"],
      ["sonnet", "sonnet", "claude-sonnet-5-5"],
      ["claude-opus-5-5", null, "claude-opus-5-5"],
      ["claude-opus-5-0", null, "claude-opus-5-0"],
      ["claude-sonnet-5-5", null, "claude-sonnet-5-5"],
    ]);
  });

  it("offers a gated line such as Mythos only when the catalog has a model of it", () => {
    const lines = (models: ProviderModel[]) => customAgentModelChoices("claude", models, compare).flatMap((choice) => (choice.line ? [choice.line] : []));
    expect(lines([model("claude-opus-5-5", "Opus 5.5")])).toEqual(["opus"]);
    expect(lines([model("claude-opus-5-5", "Opus 5.5"), model("claude-mythos-1", "Mythos 1")])).toEqual(["mythos", "opus"]);
  });
});

describe("selectedEffortLabel", () => {
  const effort: ModelOptionDefinition = {
    kind: "choice",
    key: "reasoning_effort",
    label: "Reasoning effort",
    description: null,
    options: [
      { value: "low", label: "Low", description: null },
      { value: "medium", label: "Medium", description: null },
    ],
    defaultValue: null,
  };
  const tier: ModelOptionDefinition = { ...effort, key: "service_tier", label: "Service tier" };

  it("labels the chosen effort and falls back to its raw value", () => {
    expect(selectedEffortLabel([{ key: "reasoning_effort", value: { kind: "choice", value: "medium" } }], [tier, effort]))
      .toBe("Medium");
    expect(selectedEffortLabel([{ key: "reasoning_effort", value: { kind: "choice", value: "max" } }], [effort]))
      .toBe("max");
  });

  it("returns null without an effort choice or an effort option", () => {
    expect(selectedEffortLabel([], [effort])).toBeNull();
    expect(selectedEffortLabel([{ key: "service_tier", value: { kind: "choice", value: "low" } }], [tier])).toBeNull();
  });
});

describe("isRemovableAgent", () => {
  const withId = (id: string) => ({ participant: { ...agent("Reviewer", "custom").participant, id } });

  it("accepts the ids the custom agent dialog creates", () => {
    expect(isRemovableAgent(withId("agent:0123456789abcdef0123456789abcdef"))).toBe(true);
  });

  it("refuses built-in ids and malformed named ids", () => {
    expect(isRemovableAgent(withId("agent:claude:opus"))).toBe(false);
    expect(isRemovableAgent(withId("agent:claude:claude-opus-4-7"))).toBe(false);
    expect(isRemovableAgent(withId("agent:0123456789ABCDEF0123456789ABCDEF"))).toBe(false);
    expect(isRemovableAgent(withId("agent:0123456789abcdef0123456789abcde"))).toBe(false);
    expect(isRemovableAgent(withId("agent:0123456789abcdef0123456789abcdef0"))).toBe(false);
  });
});
