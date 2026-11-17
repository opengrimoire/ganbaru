import { describe, expect, it } from "vitest";
import type { ProbeState, ProviderInstanceRead, ProviderUsageLimitsObservation } from "$lib/chat/contracts";
import { buildModelCompanySections } from "$lib/chat/composer/model-picker-model";
import {
  compareProviderReadiness,
  planLimitWindowName,
  providerPlanLimits,
  providerReadiness,
} from "./provider-readiness";

const NOW_MS = Date.parse("2026-11-16T12:00:00Z");
const HOUR_SECONDS = 3_600;
const nowSeconds = NOW_MS / 1_000;

function provider(
  familyId: string,
  options: {
    state?: ProbeState;
    enabled?: boolean;
    usageLimits?: ProviderUsageLimitsObservation | null;
    lastUsedAt?: string | null;
  } = {},
): ProviderInstanceRead {
  const state = options.state ?? "healthy";
  return {
    configuration: {
      schemaVersion: 1,
      instanceId: familyId,
      familyId,
      label: familyId,
      enabled: options.enabled ?? true,
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
      instanceId: familyId,
      state,
      version: null,
      negotiatedProtocolVersion: null,
      accountLabel: null,
      authSource: null,
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
      checkedAt: "2026-11-16T11:00:00Z",
      detail: null,
      versionAdmission: null,
      teammateMinimumVersion: null,
    },
    lastSuccessfulProbeAt: state === "healthy" ? "2026-11-16T11:00:00Z" : null,
    modelCatalog: null,
    usageLimits: options.usageLimits ?? null,
    lastUsedAt: options.lastUsedAt ?? null,
  };
}

function codexLimits(
  observedAt: string,
  primaryUsed: number,
  primaryResetsAt: number,
  limited = false,
  resetsAt: string | null = null,
): ProviderUsageLimitsObservation {
  return {
    observedAt,
    status: {
      limited,
      resetsAt,
      detail: null,
      providerData: {
        schemaVersion: 1,
        value: {
          rateLimitsByLimitId: {
            codex: {
              limitId: "codex",
              primary: { usedPercent: primaryUsed, windowDurationMins: 300, resetsAt: primaryResetsAt },
              secondary: { usedPercent: 1, windowDurationMins: 10_080, resetsAt: nowSeconds + 72 * HOUR_SECONDS },
            },
            codex_mini: {
              limitId: "codex_mini",
              primary: { usedPercent: 40, windowDurationMins: 300, resetsAt: primaryResetsAt },
            },
          },
        },
      },
    },
  };
}

describe("provider plan limits", () => {
  it("shows the account-wide windows with what is left and when each resets", () => {
    const read = providerPlanLimits(
      provider("codex", { usageLimits: codexLimits("2026-11-16T11:50:00Z", 7, nowSeconds + 2 * HOUR_SECONDS) }),
      NOW_MS,
    );
    expect(read).toMatchObject({ stale: false, limited: false });
    expect(read?.windows.map((window) => [planLimitWindowName(window, "Week"), window.remainingPercent, window.resetsAtMs]))
      .toEqual([
        ["5h", 93, NOW_MS + 2 * HOUR_SECONDS * 1_000],
        ["Week", 99, NOW_MS + 72 * HOUR_SECONDS * 1_000],
      ]);
  });

  it("reads a window whose reset passed as fully available and marks an old observation stale", () => {
    const read = providerPlanLimits(
      provider("codex", { usageLimits: codexLimits("2026-11-16T06:00:00Z", 100, nowSeconds - HOUR_SECONDS) }),
      NOW_MS,
    );
    expect(read?.stale).toBe(true);
    expect(read?.windows[0]).toMatchObject({ remainingPercent: 100, resetsAtMs: null });
  });

  it("keeps a provider limited only until its reset, or briefly when the provider gave no reset", () => {
    const limitedUntil = (resetsAt: string | null, observedAt: string) => providerPlanLimits(
      provider("codex", { usageLimits: codexLimits(observedAt, 100, nowSeconds + HOUR_SECONDS, true, resetsAt) }),
      NOW_MS,
    )?.limited;
    expect(limitedUntil("2026-11-16T13:00:00Z", "2026-11-16T11:00:00Z")).toBe(true);
    expect(limitedUntil("2026-11-16T11:30:00Z", "2026-11-16T11:00:00Z")).toBe(false);
    expect(limitedUntil(null, "2026-11-16T11:00:00Z")).toBe(true);
    expect(limitedUntil(null, "2026-11-16T05:00:00Z")).toBe(false);
  });

  it("returns nothing without an observation", () => {
    expect(providerPlanLimits(provider("claude"), NOW_MS)).toBeNull();
  });
});

describe("provider readiness", () => {
  const limited = codexLimits("2026-11-16T11:00:00Z", 100, nowSeconds + HOUR_SECONDS, true, "2026-11-16T13:00:00Z");

  it("ranks usable providers first, limited ones next, and those needing setup last", () => {
    expect(providerReadiness(provider("claude"), NOW_MS)).toBe("ready");
    expect(providerReadiness(provider("codex", { usageLimits: limited }), NOW_MS)).toBe("limited");
    expect(providerReadiness(provider("codex", { state: "authentication_required" }), NOW_MS)).toBe("unavailable");
    expect(providerReadiness(provider("claude", { enabled: false }), NOW_MS)).toBe("unavailable");
  });

  it("breaks readiness ties by the most recent use and leaves unused peers equal", () => {
    const recent = provider("claude", { lastUsedAt: "2026-11-16T11:00:00Z" });
    const older = provider("cursor", { lastUsedAt: "2026-11-15T11:00:00Z" });
    expect(compareProviderReadiness(recent, older, NOW_MS)).toBeLessThan(0);
    expect(compareProviderReadiness(provider("claude"), provider("cursor"), NOW_MS)).toBe(0);
    expect(compareProviderReadiness(provider("codex", { usageLimits: limited }), older, NOW_MS)).toBeGreaterThan(0);
  });

  it("puts a company served by a signed-in provider ahead of one whose provider needs sign-in", () => {
    const sections = buildModelCompanySections(
      [provider("codex", { state: "authentication_required" }), provider("claude")],
      [],
      { providerInstanceId: null, modelId: null, providerManaged: false },
      "",
      "Provider managed",
      "en",
      NOW_MS,
    );
    expect(sections.map((section) => section.managedProviders[0]?.configuration.familyId)).toEqual(["claude", "codex"]);
  });
});
