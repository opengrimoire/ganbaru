import type { ModelId, ProviderFamilyId, ProviderModel, ProviderModelCatalog } from "$lib/chat/contracts";
import {
  modelCompany,
  modelCompanyForIdentity,
  type ModelCompanyIdentity,
} from "$lib/chat/composer/model-company";

export interface ChatModelParticipant {
  displayName: string;
  company: ModelCompanyIdentity;
  providerFamilyId: ProviderFamilyId;
  modelId: ModelId | null;
  defaultReasoning: string | null;
}

/** Returns the human-readable reasoning level Ganbaru selects for new work. */
export function modelDefaultReasoning(model: ProviderModel | null): string | null {
  const definition = model?.options.find((option) => (
    option.kind === "choice" && /effort|reasoning/iu.test(`${option.key} ${option.label}`)
  ));
  if (definition?.kind !== "choice") return null;
  const medium = definition.options.find((option) => option.value.toLowerCase() === "medium") ?? null;
  if (medium) return medium.label;
  if (definition.defaultValue === null) return null;
  return definition.options.find((option) => option.value === definition.defaultValue)?.label ?? definition.defaultValue;
}

/** Resolves the visible model identity for a provider family and model, falling back when the catalog lacks the model. */
export function chatModelParticipant(
  providerFamilyId: ProviderFamilyId,
  modelId: ModelId | null,
  catalog: ProviderModelCatalog | null,
): ChatModelParticipant {
  const model = modelId
    ? catalog?.models.find((candidate) => candidate.id === modelId) ?? null
    : null;
  const company = model
    ? modelCompany(providerFamilyId, model)
    : modelCompanyForIdentity(providerFamilyId, modelId);
  return {
    displayName: model?.displayName ?? modelId ?? company.name,
    company,
    providerFamilyId,
    modelId: model?.id ?? modelId,
    defaultReasoning: modelDefaultReasoning(model),
  };
}
