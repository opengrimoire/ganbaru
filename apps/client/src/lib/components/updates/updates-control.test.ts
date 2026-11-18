import { describe, expect, it } from "vitest";
import type { ProviderInstanceRead, ProviderUpdateRead } from "$lib/chat/contracts";
import { appUpdatePending, cliUpdateEntries } from "./updates-control";
import type { UpdateStatus } from "$lib/stores/updates.svelte";

const NOW_MS = Date.parse("2026-11-17T12:00:00Z");

function provider(instanceId: string, familyId: string, lastUsedAt: string | null = null): ProviderInstanceRead {
  return {
    configuration: {
      schemaVersion: 1,
      instanceId,
      familyId,
      label: instanceId,
      enabled: true,
      executable: familyId,
      providerHome: null,
      launchArguments: [],
      environment: {},
      credentialReferences: {},
      visibleModelIds: [],
      favoriteModelIds: [],
      providerConfig: { schemaVersion: 1, value: {} },
    },
    runtimeReview: [],
    lastProbe: {
      instanceId,
      state: "healthy",
      version: null,
      negotiatedProtocolVersion: null,
      accountLabel: null,
      authSource: null,
      accountPlan: null,
      capabilities: { entries: [] },
      authoritySupport: {
        isolatedConversation: true,
        internalHostTools: true,
        denyShell: true,
        readOnlyRoot: true,
        writableRoot: true,
        confinedCommands: true,
        networkBoundary: true,
        classifiedPublish: true,
        personApprovedCommands: false,
      },
      checkedAt: "2026-11-17T11:00:00Z",
      detail: null,
      versionAdmission: null,
      agentMinimumVersion: null,
    },
    lastSuccessfulProbeAt: "2026-11-17T11:00:00Z",
    modelCatalog: null,
    usageLimits: null,
    lastUsedAt,
    duplicateOf: null,
  };
}

function update(instanceId: string): ProviderUpdateRead {
  return { instanceId, currentVersion: "1.0.0", latestVersion: "1.1.0", updateCommand: "cli update", runsInApp: true };
}

describe("updates control", () => {
  it("keeps the app update listed from discovery until the restart", () => {
    const pending: UpdateStatus[] = ["available", "downloading", "installed"];
    const settled: UpdateStatus[] = ["idle", "checking", "current", "error"];
    expect(pending.every(appUpdatePending)).toBe(true);
    expect(settled.some(appUpdatePending)).toBe(false);
  });

  it("lists CLI updates in model picker order and drops removed configurations", () => {
    const providers = [
      provider("claude-main", "claude"),
      provider("codex-main", "codex", "2026-11-17T10:00:00Z"),
      provider("cursor-main", "cursor"),
    ];
    const entries = cliUpdateEntries([update("claude-main"), update("removed"), update("codex-main")], providers, NOW_MS);
    expect(entries.map((entry) => entry.provider.configuration.instanceId)).toEqual(["codex-main", "claude-main"]);
  });

  it("lists nothing when no configured provider has an update", () => {
    expect(cliUpdateEntries([], [provider("claude-main", "claude")], NOW_MS)).toEqual([]);
  });
});
