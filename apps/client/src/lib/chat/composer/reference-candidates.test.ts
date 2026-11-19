import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatAssignmentTargetRead, ChatMessageReference, ChatParticipantRead } from "$lib/chat/contracts";
import {
  candidateIsDisabled,
  candidateReference,
  firstPickerIndex,
  groupReferenceCandidates,
  inferAssignmentTarget,
  lastPickerIndex,
  matchesQuery,
  nextPickerIndex,
  type ReferenceCandidate,
} from "./reference-candidates";

function target(
  environmentId: string | null,
  overrides: Partial<ChatAssignmentTargetRead> = {},
): ChatAssignmentTargetRead {
  return {
    executionTarget: environmentId
      ? { kind: "workingFolder", workingFolderId: "folder-1", executionEnvironmentId: environmentId }
      : null,
    kind: "currentFolder",
    displayName: environmentId ?? "none",
    folderCapability: null,
    effectiveRuntimeApproval: "ask",
    isDefault: false,
    bindingState: "ready",
    lifecycleState: "available",
    isBusy: false,
    isDirty: null,
    eligible: true,
    unavailableReason: null,
    ...overrides,
  };
}

function folder(key: string, group: string): ReferenceCandidate {
  return { key, kind: "workingFolder", group, label: key, description: "", workingFolderId: `id-${key}` };
}

function environment(key: string, eligible: boolean, group = "Environments"): ReferenceCandidate {
  return {
    key,
    kind: "executionEnvironment",
    group,
    label: key,
    description: "",
    target: target(`env-${key}`, { eligible }),
  };
}

function environmentReference(environmentId: string): ChatMessageReference {
  return {
    kind: "executionEnvironment",
    metadata: {
      referenceId: `reference-${environmentId}`,
      labelSnapshot: environmentId,
      startOffset: 0,
      endOffset: 0,
      plainTextProjection: `@${environmentId}`,
    },
    executionEnvironmentId: environmentId,
  };
}

const participant: ChatParticipantRead = {
  id: "participant-1",
  kind: "ai_agent",
  displayName: "Atlas",
  avatar: { schemaVersion: 1, value: null },
  revision: 1,
  archivedAt: null,
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe("matchesQuery", () => {
  it("matches everything for an empty query", () => {
    expect(matchesQuery("", "Atlas")).toBe(true);
  });

  it("matches a lowercase query against any value case insensitively", () => {
    expect(matchesQuery("las", "Nova", "ATLAS")).toBe(true);
    expect(matchesQuery("zed", "Nova", "Atlas")).toBe(false);
  });
});

describe("groupReferenceCandidates", () => {
  it("groups consecutive candidates and keeps their flat indexes", () => {
    const groups = groupReferenceCandidates([folder("a", "One"), folder("b", "One"), folder("c", "Two")]);
    expect(groups.map((group) => group.label)).toEqual(["One", "Two"]);
    expect(groups[0]!.entries.map((entry) => entry.index)).toEqual([0, 1]);
    expect(groups[1]!.entries.map((entry) => [entry.candidate.key, entry.index])).toEqual([["c", 2]]);
  });

  it("starts a new group when a label returns after another group", () => {
    const groups = groupReferenceCandidates([folder("a", "One"), folder("b", "Two"), folder("c", "One")]);
    expect(groups.map((group) => group.label)).toEqual(["One", "Two", "One"]);
  });

  it("returns no groups for no candidates", () => {
    expect(groupReferenceCandidates([])).toEqual([]);
  });
});

describe("picker navigation", () => {
  it("disables only ineligible execution environments", () => {
    expect(candidateIsDisabled(environment("off", false))).toBe(true);
    expect(candidateIsDisabled(environment("on", true))).toBe(false);
    expect(candidateIsDisabled(folder("a", "One"))).toBe(false);
  });

  it("skips disabled candidates at both ends", () => {
    const candidates = [environment("x", false), folder("a", "One"), folder("b", "One"), environment("y", false)];
    expect(firstPickerIndex(candidates)).toBe(1);
    expect(lastPickerIndex(candidates)).toBe(2);
  });

  it("falls back to the first index when every candidate is disabled", () => {
    const candidates = [environment("x", false), environment("y", false)];
    expect(firstPickerIndex(candidates)).toBe(0);
    expect(lastPickerIndex(candidates)).toBe(0);
    expect(nextPickerIndex(candidates, 1, 1)).toBe(1);
  });

  it("wraps over disabled candidates in both directions", () => {
    const candidates = [folder("a", "One"), environment("x", false), folder("b", "One")];
    expect(nextPickerIndex(candidates, 0, 1)).toBe(2);
    expect(nextPickerIndex(candidates, 2, 1)).toBe(0);
    expect(nextPickerIndex(candidates, 0, -1)).toBe(2);
    expect(nextPickerIndex(candidates, 2, -1)).toBe(0);
  });

  it("returns 0 for an empty list", () => {
    expect(nextPickerIndex([], 3, 1)).toBe(0);
  });
});

describe("candidateReference", () => {
  it("builds a participant reference with a fresh ID and an at-sign projection", () => {
    vi.spyOn(crypto, "randomUUID").mockReturnValue("00000000-0000-4000-8000-000000000001");
    const reference = candidateReference({
      key: "p",
      kind: "participant",
      group: "People",
      label: "@Atlas",
      description: "",
      participant,
      agent: null,
      needsMembership: false,
    });
    expect(reference).toEqual({
      kind: "participant",
      metadata: {
        referenceId: "00000000-0000-4000-8000-000000000001",
        labelSnapshot: "Atlas",
        startOffset: 0,
        endOffset: 0,
        plainTextProjection: "@Atlas",
      },
      participantId: "participant-1",
      participantKind: participant.kind,
    });
  });

  it("keeps a channel label as its projection", () => {
    const reference = candidateReference({
      key: "c",
      kind: "channel",
      group: "Channels",
      label: "#general",
      description: "",
      channel: { id: "channel-1" } as Extract<ReferenceCandidate, { kind: "channel" }>["channel"],
    });
    expect(reference?.kind).toBe("channel");
    expect(reference?.metadata.labelSnapshot).toBe("general");
    expect(reference?.metadata.plainTextProjection).toBe("#general");
  });

  it("maps a directory path to a folder reference", () => {
    const reference = candidateReference({
      key: "w",
      kind: "workspacePath",
      group: "Files",
      label: "src",
      description: "",
      workingFolderId: "folder-1",
      path: { kind: "directory", relativePath: "src", displayName: "src", ignored: false },
    });
    expect(reference).toMatchObject({
      kind: "workspacePath",
      workingFolderId: "folder-1",
      pathKind: "folder",
      relativePath: "src",
      metadata: { plainTextProjection: "@src" },
    });
  });

  it("returns null for an environment without an execution target", () => {
    const candidate: ReferenceCandidate = {
      key: "e",
      kind: "executionEnvironment",
      group: "Environments",
      label: "main",
      description: "",
      target: target(null),
    };
    expect(candidateReference(candidate)).toBeNull();
  });

  it("references the environment of an execution target", () => {
    expect(candidateReference(environment("main", true))).toMatchObject({
      kind: "executionEnvironment",
      executionEnvironmentId: "env-main",
    });
  });
});

describe("inferAssignmentTarget", () => {
  const defaultTarget = target("env-default", { isDefault: true });
  const other = target("env-other");

  it("picks the eligible target of the one referenced environment", () => {
    expect(inferAssignmentTarget([environmentReference("env-other")], [defaultTarget, other])).toBe(other);
  });

  it("falls back to the eligible default when the referenced target is ineligible", () => {
    const ineligible = target("env-other", { eligible: false });
    expect(inferAssignmentTarget([environmentReference("env-other")], [defaultTarget, ineligible])).toBe(defaultTarget);
  });

  it("ignores environment references when they name more than one environment", () => {
    const references = [environmentReference("env-other"), environmentReference("env-third")];
    expect(inferAssignmentTarget(references, [other, defaultTarget])).toBe(defaultTarget);
  });

  it("returns null when the default is ineligible and nothing is referenced", () => {
    expect(inferAssignmentTarget([], [target("env-default", { isDefault: true, eligible: false })])).toBeNull();
  });
});
