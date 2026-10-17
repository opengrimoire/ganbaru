import { describe, expect, it } from "vitest";
import {
  teammateExecutionSummary,
  teammateProfileDraftSnapshot,
  type TeammateProfileDraftSnapshotInput,
} from "./draft";

const PROFILE: TeammateProfileDraftSnapshotInput = {
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

describe("teammate draft snapshots", () => {
  it("ignores whitespace that is removed when saving a profile", () => {
    expect(teammateProfileDraftSnapshot({
      ...PROFILE,
      displayName: " Atlas ",
      role: " Plan work ",
    })).toBe(teammateProfileDraftSnapshot(PROFILE));
  });

  it("ignores model option order", () => {
    expect(teammateProfileDraftSnapshot({
      ...PROFILE,
      modelOptions: [...PROFILE.modelOptions].reverse(),
    })).toBe(teammateProfileDraftSnapshot(PROFILE));
  });

  it("detects a persisted profile change", () => {
    expect(teammateProfileDraftSnapshot({ ...PROFILE, effort: "high" }))
      .not.toBe(teammateProfileDraftSnapshot(PROFILE));
  });

  it("detects execution mode and speed changes", () => {
    expect(teammateProfileDraftSnapshot({
      ...PROFILE,
      providerManagedModel: true,
      modelId: "",
      speed: "fast",
    })).not.toBe(teammateProfileDraftSnapshot(PROFILE));
  });

  it("derives effort and choice speed summaries", () => {
    expect(teammateExecutionSummary(PROFILE.modelOptions, null)).toEqual({
      effort: "medium",
      speed: "standard",
    });
    expect(teammateExecutionSummary([
      { key: "reasoning_effort", value: { kind: "choice", value: "ultra" } },
      { key: "service_tier", value: { kind: "choice", value: "priority" } },
    ], null)).toEqual({ effort: "ultra", speed: "fast" });
  });

  it("derives boolean Fast mode summaries", () => {
    expect(teammateExecutionSummary([
      { key: "fastMode", value: { kind: "boolean", value: true } },
    ], null)).toEqual({ effort: null, speed: "fast" });
    expect(teammateExecutionSummary([
      { key: "fastMode", value: { kind: "boolean", value: false } },
    ], null)).toEqual({ effort: null, speed: "standard" });
  });

});
