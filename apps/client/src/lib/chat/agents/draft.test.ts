import { describe, expect, it } from "vitest";
import {
  AGENT_INSTRUCTIONS_MAX_BYTES,
  agentExecutionSummary,
  agentInstructionsTooLong,
  agentProfileDraftSnapshot,
  type AgentProfileDraftSnapshotInput,
} from "./draft";

const PROFILE: AgentProfileDraftSnapshotInput = {
  displayName: "Atlas",
  role: "Plan work",
  instructions: "Stay focused",
  providerId: "codex-default",
  safetyMode: "ask_for_approval",
  modelId: "gpt-5.5",
  providerManagedModel: false,
  modelOptions: [
    { key: "speed", value: { kind: "choice", value: "standard" } },
    { key: "effort", value: { kind: "choice", value: "medium" } },
  ],
  effort: "medium",
  speed: "standard",
};

describe("agent draft snapshots", () => {
  it("ignores whitespace that is removed when saving a profile", () => {
    expect(agentProfileDraftSnapshot({
      ...PROFILE,
      displayName: " Atlas ",
      role: " Plan work ",
    })).toBe(agentProfileDraftSnapshot(PROFILE));
  });

  it("ignores model option order", () => {
    expect(agentProfileDraftSnapshot({
      ...PROFILE,
      modelOptions: [...PROFILE.modelOptions].reverse(),
    })).toBe(agentProfileDraftSnapshot(PROFILE));
  });

  it("detects a persisted profile change", () => {
    expect(agentProfileDraftSnapshot({ ...PROFILE, effort: "high" }))
      .not.toBe(agentProfileDraftSnapshot(PROFILE));
  });

  it("detects execution mode and speed changes", () => {
    expect(agentProfileDraftSnapshot({
      ...PROFILE,
      providerManagedModel: true,
      modelId: "",
      speed: "fast",
    })).not.toBe(agentProfileDraftSnapshot(PROFILE));
  });

  it("derives effort and choice speed summaries", () => {
    expect(agentExecutionSummary(PROFILE.modelOptions, null)).toEqual({
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

describe("agent instruction bound", () => {
  it("counts UTF-8 bytes of the trimmed text, not characters", () => {
    const ascii = "a".repeat(AGENT_INSTRUCTIONS_MAX_BYTES);
    expect(agentInstructionsTooLong(ascii)).toBe(false);
    expect(agentInstructionsTooLong(`  ${ascii}\n`)).toBe(false);
    expect(agentInstructionsTooLong(`${ascii}a`)).toBe(true);
    const multibyte = "é".repeat(AGENT_INSTRUCTIONS_MAX_BYTES / 2);
    expect(agentInstructionsTooLong(multibyte)).toBe(false);
    expect(agentInstructionsTooLong(`${multibyte}é`)).toBe(true);
    expect(agentInstructionsTooLong("😀".repeat(AGENT_INSTRUCTIONS_MAX_BYTES / 4 + 1))).toBe(true);
  });
});
