import type {
  ModelOptionDefinition,
  ProviderFamilyId,
  ProviderFamilyMetadataRead,
  ProviderInstanceRead,
} from "$lib/chat/contracts";
import { compareProviderReadiness } from "$lib/chat/usage/provider-readiness";
import { modelOptionRole } from "./model";
import { integrationCompany } from "./model-company";

export { modelOptionRole };

export type KnownModelOption = Exclude<ModelOptionDefinition, { kind: "unknown" }>;

/** Formats a model option label, such as an effort level, for Chat. */
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

export function isKnownModelOption(
  definition: ModelOptionDefinition,
): definition is KnownModelOption {
  return definition.kind !== "unknown";
}

/**
 * Orders provider instances for settings and for choosing the provider of a new thread: ready
 * ones first, then by account plan, then the most recently used,
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
 * Orders provider families for settings: families with a configured
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
