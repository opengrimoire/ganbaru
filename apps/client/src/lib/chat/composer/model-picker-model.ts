import type {
  ModelOptionDefinition,
  ProviderFamilyId,
  ProviderFamilyMetadataRead,
  ProviderInstanceRead,
  ProviderModel,
  RememberedComposerSelection,
} from "$lib/chat/contracts";
import { compareProviderReadiness } from "$lib/chat/usage/provider-readiness";
import { isProviderAvailable, modelOptionRole, providerHasCatalogModels, rankedModels, recommendedProviderModel } from "./model";
import {
  compareCompanyModels,
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

/**
 * Shows a selection that names no model as the model `provider` recommends, which is what runs
 * for it whenever the provider has a catalog on this device.
 */
export function resolveRecommendedSelection<Selection extends { modelId: string | null; providerManaged: boolean }>(
  provider: ProviderInstanceRead | null,
  selection: Selection,
): Selection {
  if (!selection.providerManaged || !provider) return selection;
  const recommended = recommendedProviderModel(provider);
  return recommended ? { ...selection, modelId: recommended.id, providerManaged: false } : selection;
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

/** The most ready provider that serves a section, or `null` for a section that only offers setup. */
function sectionLeadProvider(section: ModelCompanySection, nowMs: number): ProviderInstanceRead | null {
  const providers = [...section.models.map((entry) => entry.provider), ...section.managedProviders];
  return providers.reduce<ProviderInstanceRead | null>((best, provider) => (
    best === null || compareProviderReadiness(provider, best, nowMs) < 0 ? provider : best
  ), null);
}

function compareSectionReadiness(left: ModelCompanySection, right: ModelCompanySection, nowMs: number): number {
  const leftLead = sectionLeadProvider(left, nowMs);
  const rightLead = sectionLeadProvider(right, nowMs);
  if (!leftLead || !rightLead) return Number(leftLead === null) - Number(rightLead === null);
  return compareProviderReadiness(leftLead, rightLead, nowMs);
}

/**
 * Groups every visible model and setup offer by model company, with a choice of the provider's own
 * configured model for each provider without catalog models. Sections served by a ready provider
 * come first, then limited ones, then those that need setup; ties rank the lead provider's account
 * plan, then its most recent use, then the company order.
 */
export function buildModelCompanySections(
  providers: readonly ProviderInstanceRead[],
  setupFamilies: readonly ProviderFamilyMetadataRead[],
  selection: ModelCatalogSelection,
  query: string,
  providerManagedLabel: string,
  locale: string,
  nowMs: number,
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
    if (!providerHasCatalogModels(provider) && (!query || providerManagedLabel.toLocaleLowerCase(locale).includes(query.toLocaleLowerCase(locale)))) {
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
    .sort((left, right) => compareSectionReadiness(left, right, nowMs)
      || left.company.order - right.company.order
      || left.company.name.localeCompare(right.company.name));
  if (!query.trim()) {
    for (const section of results) {
      section.models.sort((left, right) => compareProviderReadiness(left.provider, right.provider, nowMs)
        || compareCompanyModels(section.company.id, left.model, right.model));
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

/** The first model the picker offers, or a provider-managed choice when that comes first. */
export interface FirstPickerChoice {
  provider: ProviderInstanceRead;
  model: ProviderModel | null;
  providerManaged: boolean;
}

/**
 * The first choice a person can pick in the model picker as it lists everything with no search:
 * the first usable favorite, otherwise the first usable entry of the first company section.
 *
 * @param providers Configured provider instances.
 * @param rememberedSelections Recent selections, which order favorites as the picker does.
 * @param nowMs Current instant, which decides plan-limit readiness.
 */
export function firstPickerChoice(
  providers: readonly ProviderInstanceRead[],
  rememberedSelections: readonly RememberedComposerSelection[],
  nowMs: number,
): FirstPickerChoice | null {
  const selection: ModelCatalogSelection = { providerInstanceId: null, modelId: null, providerManaged: false };
  const usable = (entry: CompanyModelEntry): boolean => (
    isProviderAvailable(entry.provider) && entry.model.availability !== "unavailable"
  );
  const favorite = buildFavoriteModelEntries(providers, rememberedSelections, selection, "").find(usable);
  if (favorite) return { ...favorite, providerManaged: false };
  // With no query, the provider-managed label and locale only matter for filtering, so they stay empty.
  for (const section of buildModelCompanySections(providers, [], selection, "", "", "en", nowMs)) {
    const entry = section.models.find(usable);
    if (entry) return { ...entry, providerManaged: false };
    const managed = section.managedProviders.find(isProviderAvailable);
    if (managed) return { provider: managed, model: null, providerManaged: true };
  }
  return null;
}

/**
 * Orders provider instances as the model picker orders their sections, which also picks the
 * provider of a new thread: ready ones first, then by account plan, then the most recently used,
 * then by model company, keeping the given order for ties.
 *
 * @param providers Provider instances in backend order.
 * @param nowMs Current instant, which decides plan-limit readiness.
 */
export function providersInPickerOrder(
  providers: readonly ProviderInstanceRead[],
  nowMs: number,
): ProviderInstanceRead[] {
  return providers
    .map((provider, index) => ({ provider, index }))
    .sort((left, right) => compareProviderReadiness(left.provider, right.provider, nowMs)
      || integrationCompany(left.provider.configuration.familyId).order
        - integrationCompany(right.provider.configuration.familyId).order
      || left.index - right.index)
    .map(({ provider }) => provider);
}

/**
 * Orders provider families as the model picker would list them: families with a configured
 * instance first, ranked by their best instance, then the rest by model company.
 *
 * @param families Provider families in backend order.
 * @param providers Configured provider instances.
 * @param nowMs Current instant, which decides plan-limit readiness.
 */
export function familiesInPickerOrder<Family extends Pick<ProviderFamilyMetadataRead, "familyId">>(
  families: readonly Family[],
  providers: readonly ProviderInstanceRead[],
  nowMs: number,
): Family[] {
  const ordered = providersInPickerOrder(providers, nowMs);
  const rank = (family: Family): number => {
    const index = ordered.findIndex((provider) => provider.configuration.familyId === family.familyId);
    return index === -1 ? ordered.length : index;
  };
  return families
    .map((family, index) => ({ family, index, rank: rank(family) }))
    .sort((left, right) => left.rank - right.rank
      || integrationCompany(left.family.familyId).order - integrationCompany(right.family.familyId).order
      || left.index - right.index)
    .map(({ family }) => family);
}
