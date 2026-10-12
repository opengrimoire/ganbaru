import type {
  ModelOptionDefinition,
  ProviderFamilyId,
  ProviderFamilyMetadataRead,
  ProviderInstanceRead,
  ProviderModel,
  RememberedComposerSelection,
} from "$lib/chat/contracts";
import { modelOptionRole, rankedModels } from "./model";
import {
  compareCompanyModels,
  formatModelDisplayName,
  integrationCompany,
  modelCompany,
  type ModelCompanyIdentity,
} from "./model-company";

export { modelOptionRole };

export type KnownModelOption =Exclude<ModelOptionDefinition, { kind: "unknown" }>;

export interface QuickEffortChoice {
  modelId: string;
  modelName: string;
  effortKey: string;
  effortValue: string;
  effortLabel: string;
}

export interface FavoriteModelEntry {
  provider: ProviderInstanceRead;
  model: ProviderModel;
}

export interface CompanyModelEntry {
  provider: ProviderInstanceRead;
  model: ProviderModel;
}

export interface ModelCompanySection {
  company: ModelCompanyIdentity;
  models: CompanyModelEntry[];
  managedProviders: ProviderInstanceRead[];
  setupFamilies: ProviderFamilyMetadataRead[];
}

export interface ModelCatalogSelection {
  providerInstanceId: string | null;
  modelId: string | null;
  providerManaged: boolean;
}

/** Formats a model name for the compact Chat model control. */
export function compactModelName(
  displayName: string,
  familyId: ProviderFamilyId | null,
): string {
  const compactName = familyId === "codex" ? displayName.replace(/^GPT-/i, "") : displayName;
  return formatModelDisplayName(compactName);
}

/** Formats an option label for the compact Chat model control. */
export function compactModelOptionLabel(
  label: string,
  familyId: ProviderFamilyId | null,
  lightLabel: string,
  extraHighLabel: string,
): string {
  const normalized = label.replaceAll("_", " ").trim();
  if (familyId === "codex" && normalized.toLowerCase() === "low") return lightLabel;
  if (["xhigh", "extra high"].includes(normalized.toLowerCase())) return extraHighLabel;
  if (!normalized || normalized !== normalized.toLowerCase()) return normalized;
  return `${normalized[0]?.toUpperCase() ?? ""}${normalized.slice(1)}`;
}

export function visibleProviderModels(
  provider: ProviderInstanceRead | null,
  selection: ModelCatalogSelection,
): ProviderModel[] {
  if (!provider) return [];
  const selectedId = provider.configuration.instanceId === selection.providerInstanceId
    ? selection.modelId
    : null;
  return provider.modelCatalog?.models.filter((model) => (
    model.availability !== "deprecated"
    && (
      provider.configuration.visibleModelIds.length === 0
      || provider.configuration.visibleModelIds.includes(model.id)
      || model.id === selectedId
    )
  )) ?? [];
}

export function buildFavoriteModelEntries(
  providers: readonly ProviderInstanceRead[],
  rememberedSelections: readonly RememberedComposerSelection[],
  selection: ModelCatalogSelection,
  query: string,
): FavoriteModelEntry[] {
  const favorites: FavoriteModelEntry[] = [];
  for (const provider of providers) {
    const favoriteIds = provider.configuration.favoriteModelIds;
    if (favoriteIds.length === 0) continue;
    const recentIds = rememberedSelections
      .filter((entry) => entry.providerInstanceId === provider.configuration.instanceId && entry.modelId)
      .map((entry) => entry.modelId)
      .filter((modelId): modelId is string => modelId !== null);
    const favoriteModels = visibleProviderModels(provider, selection)
      .filter((model) => favoriteIds.includes(model.id));
    for (const model of rankedModels(favoriteModels, favoriteIds, recentIds, query)) {
      favorites.push({ provider, model });
    }
  }
  return favorites;
}

export function buildModelCompanySections(
  providers: readonly ProviderInstanceRead[],
  setupFamilies: readonly ProviderFamilyMetadataRead[],
  selection: ModelCatalogSelection,
  query: string,
  providerManagedLabel: string,
  locale: string,
): ModelCompanySection[] {
  const sections = new Map<string, ModelCompanySection>();
  const sectionFor = (company: ModelCompanyIdentity): ModelCompanySection => {
    const existing = sections.get(company.id);
    if (existing) return existing;
    const created = { company, models: [], managedProviders: [], setupFamilies: [] };
    sections.set(company.id, created);
    return created;
  };

  for (const provider of providers) {
    const visible = visibleProviderModels(provider, selection);
    for (const model of rankedModels(visible, [], [], query)) {
      sectionFor(modelCompany(provider.configuration.familyId, model)).models.push({ provider, model });
    }
    const providerManaged = (provider.modelCatalog?.models.length ?? 0) === 0
      || (provider.configuration.instanceId === selection.providerInstanceId && selection.providerManaged);
    if (providerManaged && (!query || providerManagedLabel.toLocaleLowerCase(locale).includes(query.toLocaleLowerCase(locale)))) {
      sectionFor(integrationCompany(provider.configuration.familyId)).managedProviders.push(provider);
    }
  }

  for (const family of setupFamilies) {
    const company = integrationCompany(family.familyId);
    if (!query || company.name.toLocaleLowerCase(locale).includes(query.toLocaleLowerCase(locale))) {
      sectionFor(company).setupFamilies.push(family);
    }
  }

  const results = [...sections.values()]
    .filter((section) => section.models.length > 0
      || section.managedProviders.length > 0
      || section.setupFamilies.length > 0)
    .sort((left, right) => left.company.order - right.company.order
      || left.company.name.localeCompare(right.company.name));
  if (!query.trim()) {
    for (const section of results) {
      section.models.sort((left, right) => compareCompanyModels(section.company.id, left.model, right.model));
    }
  }
  return results;
}

export function buildQuickEffortChoices(
  anchor: ProviderModel | null,
  availableModels: readonly ProviderModel[],
  familyId: ProviderFamilyId | null,
  displayModelName: (model: ProviderModel) => string,
  optionLabel: (label: string) => string,
): QuickEffortChoice[] {
  if (!anchor) return [];
  const anchorEffort = anchor.options.find((definition) => (
    isKnownModelOption(definition) && modelOptionRole(definition) === "effort"
  ));
  if (!anchorEffort || anchorEffort.kind !== "choice") return [];
  const choices: QuickEffortChoice[] = [];
  const lowerModel = recommendedLowerModel(anchor, availableModels, familyId);
  if (lowerModel) {
    const lowerEffort = lowerModel.options.find((definition) => (
      isKnownModelOption(definition) && modelOptionRole(definition) === "effort"
    ));
    if (lowerEffort?.kind === "choice") {
      const light = lowerEffort.options.find((choice) => choice.value.toLowerCase() === "low");
      if (light) {
        choices.push({
          modelId: lowerModel.id,
          modelName: displayModelName(lowerModel),
          effortKey: lowerEffort.key,
          effortValue: light.value,
          effortLabel: optionLabel(light.label),
        });
      }
    }
  }
  for (const effort of anchorEffort.options) {
    if (["none", "minimal"].includes(effort.value.toLowerCase())) continue;
    choices.push({
      modelId: anchor.id,
      modelName: displayModelName(anchor),
      effortKey: anchorEffort.key,
      effortValue: effort.value,
      effortLabel: optionLabel(effort.label),
    });
  }
  return choices;
}

export function isKnownModelOption(
  definition: ModelOptionDefinition,
): definition is KnownModelOption {
  return definition.kind !== "unknown";
}

export function modelEffortStopPosition(
  index: number,
  count: number,
  endpointInsetRem: number,
): string {
  const progress = count <= 1 ? 0.5 : index / (count - 1);
  const percent = progress * 100;
  const endpointOffset = (1 - 2 * progress) * endpointInsetRem;
  if (Math.abs(endpointOffset) < 0.0001) return `${percent}%`;
  return endpointOffset > 0
    ? `calc(${percent}% + ${endpointOffset}rem)`
    : `calc(${percent}% - ${Math.abs(endpointOffset)}rem)`;
}

function recommendedLowerModel(
  anchor: ProviderModel,
  availableModels: readonly ProviderModel[],
  familyId: ProviderFamilyId | null,
): ProviderModel | null {
  if (familyId !== "codex") return null;
  const lowerTier = anchor.id.endsWith("-sol")
    ? "terra"
    : anchor.id.endsWith("-terra") ? "luna" : null;
  if (!lowerTier) return null;
  const lowerId = anchor.id.replace(/-(sol|terra)$/, `-${lowerTier}`);
  return availableModels.find((model) => model.id === lowerId) ?? null;
}
