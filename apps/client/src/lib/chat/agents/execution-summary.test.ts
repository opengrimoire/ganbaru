import { describe, expect, it } from "vitest";
import type { ModelOptionSelection } from "$lib/chat/contracts";
import { agentExecutionSummary } from "./execution-summary";

const OPTIONS: ModelOptionSelection[] = [
  { key: "speed", value: { kind: "choice", value: "standard" } },
  { key: "effort", value: { kind: "choice", value: "medium" } },
];

describe("agent execution summary", () => {
  it("derives effort and choice speed summaries", () => {
    expect(agentExecutionSummary(OPTIONS, null)).toEqual({
      effort: "medium",
      speed: "standard",
    });
    expect(agentExecutionSummary([
      { key: "reasoning_effort", value: { kind: "choice", value: "ultra" } },
      { key: "service_tier", value: { kind: "choice", value: "priority" } },
    ], null)).toEqual({ effort: "ultra", speed: "fast" });
  });

  it("derives boolean Fast mode summaries", () => {
    expect(agentExecutionSummary([
      { key: "fastMode", value: { kind: "boolean", value: true } },
    ], null)).toEqual({ effort: null, speed: "fast" });
    expect(agentExecutionSummary([
      { key: "fastMode", value: { kind: "boolean", value: false } },
    ], null)).toEqual({ effort: null, speed: "standard" });
  });

});
