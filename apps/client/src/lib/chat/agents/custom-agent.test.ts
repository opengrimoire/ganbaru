import { describe, expect, it } from "vitest";
import type { ModelOptionDefinition } from "$lib/chat/contracts";
import { customAgentControls, selectedModelOption, withModelOption } from "./custom-agent";

const effort: ModelOptionDefinition = {
  kind: "choice",
  key: "reasoning_effort",
  label: "Reasoning effort",
  description: null,
  options: [
    { value: "low", label: "Low", description: null },
    { value: "high", label: "High", description: null },
  ],
  defaultValue: "low",
};

const fastMode: ModelOptionDefinition = {
  kind: "boolean",
  key: "fast_mode",
  label: "Fast mode",
  description: null,
  defaultValue: false,
};

const verbosity: ModelOptionDefinition = {
  kind: "choice",
  key: "verbosity",
  label: "Verbosity",
  description: null,
  options: [{ value: "low", label: "Low", description: null }],
  defaultValue: null,
};

describe("custom agent controls", () => {
  it("finds the reasoning and speed options and ignores the rest", () => {
    expect(customAgentControls([verbosity, fastMode, effort])).toEqual({ reasoning: effort, speed: fastMode });
  });

  it("offers neither control for a model without them", () => {
    expect(customAgentControls([verbosity])).toEqual({ reasoning: null, speed: null });
  });

  it("replaces an earlier value for the same option and keeps the others", () => {
    const selections = withModelOption(
      [
        { key: "reasoning_effort", value: { kind: "choice", value: "low" } },
        { key: "fast_mode", value: { kind: "boolean", value: false } },
      ],
      "reasoning_effort",
      { kind: "choice", value: "high" },
    );
    expect(selections).toHaveLength(2);
    expect(selectedModelOption(selections, "reasoning_effort")).toEqual({ kind: "choice", value: "high" });
    expect(selectedModelOption(selections, "fast_mode")).toEqual({ kind: "boolean", value: false });
    expect(selectedModelOption(selections, "verbosity")).toBeNull();
  });
});
