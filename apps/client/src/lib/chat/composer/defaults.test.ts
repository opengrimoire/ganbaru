import { describe, expect, it } from "vitest";
import type { ProviderAccountPlan, ProviderInstanceRead } from "$lib/chat/contracts";
import {
  claudeModelSettings,
  composer,
  multiProviderSettings,
} from "$lib/components/chat/composer/composer-test-store";
import { composerModelSelection, readComposerModelSelection } from "./model";
import { resolveComposerDefaults } from "./defaults";

const NOW_MS = Date.parse("2026-07-22T12:30:00.000Z");

function resolvedModel(modelSelection: ReturnType<typeof composerModelSelection> | null, catalog = true) {
  const settings = claudeModelSettings();
  const provider = settings.providerInstances[0];
  if (!provider) throw new Error("Composer defaults require a provider fixture");
  if (!catalog) provider.modelCatalog = null;
  const defaults = resolveComposerDefaults(settings, {
    ...composer(),
    providerInstanceId: "claude",
    modelSelection,
  }, NOW_MS);
  return defaults?.modelSelection ? readComposerModelSelection(defaults.modelSelection) : null;
}

function withPlan(provider: ProviderInstanceRead, accountPlan: ProviderAccountPlan | null): void {
  if (!provider.lastProbe) throw new Error("Composer defaults require a probed provider fixture");
  provider.lastProbe = { ...provider.lastProbe, accountPlan };
}

/** The provider a new thread starts on, given the Codex and Claude plans the probes reported. */
function newThreadProvider(
  codexPlan: ProviderAccountPlan | null,
  claudePlan: ProviderAccountPlan | null,
  codexLastUsedAt: string | null = null,
): string | null {
  const settings = multiProviderSettings();
  const [codex, claude] = settings.providerInstances;
  if (!codex || !claude) throw new Error("Composer defaults require Codex and Claude fixtures");
  withPlan(codex, codexPlan);
  withPlan(claude, claudePlan);
  codex.lastUsedAt = codexLastUsedAt;
  return resolveComposerDefaults(settings, composer(), NOW_MS)?.providerInstanceId ?? null;
}

describe("resolveComposerDefaults", () => {
  it("replaces a selection without a model by the recommended model when the provider has a catalog", () => {
    expect(resolvedModel(composerModelSelection(null, true, []))).toMatchObject({ modelId: "opus", providerManaged: false });
  });

  it("replaces a model the catalog no longer offers by the recommended model", () => {
    expect(resolvedModel(composerModelSelection("default", false, []))).toMatchObject({ modelId: "opus", providerManaged: false });
  });

  it("keeps the provider's configured model only when the provider has no catalog", () => {
    expect(resolvedModel(composerModelSelection(null, true, []), false)).toMatchObject({ modelId: null, providerManaged: true });
  });

  it("starts new threads on the provider the account pays for over a free plan", () => {
    expect(newThreadProvider("free", "subscription")).toBe("claude");
    expect(newThreadProvider("free", "subscription", "2026-07-22T12:29:00.000Z")).toBe("claude");
    expect(newThreadProvider("free", null)).toBe("claude");
    expect(newThreadProvider(null, "subscription")).toBe("claude");
    expect(newThreadProvider("api_key", null)).toBe("codex-local");
  });

  it("falls back to recent use and then the company order when the plans rank alike", () => {
    expect(newThreadProvider(null, null)).toBe("codex-local");
    expect(newThreadProvider("subscription", "subscription")).toBe("codex-local");
    expect(newThreadProvider("free", "free")).toBe("codex-local");
  });
});
