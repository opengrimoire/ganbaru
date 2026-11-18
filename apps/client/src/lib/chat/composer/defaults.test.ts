import { describe, expect, it } from "vitest";
import { claudeModelSettings, composer } from "$lib/components/chat/composer/composer-test-store";
import { composerModelSelection, readComposerModelSelection } from "./model";
import { resolveComposerDefaults } from "./defaults";

function resolvedModel(modelSelection: ReturnType<typeof composerModelSelection> | null, catalog = true) {
  const settings = claudeModelSettings();
  const provider = settings.providerInstances[0];
  if (!provider) throw new Error("Composer defaults require a provider fixture");
  if (!catalog) provider.modelCatalog = null;
  const defaults = resolveComposerDefaults(settings, {
    ...composer(),
    providerInstanceId: "claude",
    modelSelection,
  });
  return defaults?.modelSelection ? readComposerModelSelection(defaults.modelSelection) : null;
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
});
