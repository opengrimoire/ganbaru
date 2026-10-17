import type { ProviderModel } from "$lib/chat/contracts";

export type ModelCompanyId =
  | "openai"
  | "anthropic"
  | "google"
  | "xai"
  | "meta"
  | "mistral"
  | "deepseek"
  | "alibaba"
  | "moonshot"
  | "minimax"
  | "cursor"
  | "opencode";

export interface ModelCompanyIdentity {
  id: ModelCompanyId;
  name: string;
  iconFamilyId: string;
  order: number;
}

const COMPANIES: Record<ModelCompanyId, ModelCompanyIdentity> = {
  openai: { id: "openai", name: "OpenAI", iconFamilyId: "codex", order: 0 },
  anthropic: { id: "anthropic", name: "Anthropic", iconFamilyId: "claude", order: 1 },
  google: { id: "google", name: "Google", iconFamilyId: "google", order: 2 },
  xai: { id: "xai", name: "xAI", iconFamilyId: "grok", order: 3 },
  meta: { id: "meta", name: "Meta", iconFamilyId: "meta", order: 4 },
  mistral: { id: "mistral", name: "Mistral AI", iconFamilyId: "mistral", order: 5 },
  deepseek: { id: "deepseek", name: "DeepSeek", iconFamilyId: "deepseek", order: 6 },
  alibaba: { id: "alibaba", name: "Alibaba", iconFamilyId: "alibaba", order: 7 },
  moonshot: { id: "moonshot", name: "Moonshot AI", iconFamilyId: "moonshot", order: 8 },
  minimax: { id: "minimax", name: "MiniMax", iconFamilyId: "minimax", order: 9 },
  cursor: { id: "cursor", name: "Cursor", iconFamilyId: "cursor", order: 10 },
  opencode: { id: "opencode", name: "OpenCode", iconFamilyId: "opencode", order: 11 },
};

const FAMILY_DEFAULTS: Record<string, ModelCompanyId> = {
  codex: "openai",
  claude: "anthropic",
  cursor: "cursor",
  grok: "xai",
  opencode: "opencode",
};

const MODEL_MATCHERS: readonly [ModelCompanyId, RegExp][] = [
  ["anthropic", /(?:^|[\s/_.-])(anthropic|claude|sonnet|opus|haiku)(?:$|[\s/_.-])/i],
  ["openai", /(?:^|[\s/_.-])(openai|chatgpt|gpt|codex|o[134])(?:$|[\s/_.-]|\d)/i],
  ["google", /(?:^|[\s/_.-])(google|gemini|gemma)(?:$|[\s/_.-]|\d)/i],
  ["xai", /(?:^|[\s/_.-])(xai|grok)(?:$|[\s/_.-]|\d)/i],
  ["meta", /(?:^|[\s/_.-])(meta|llama)(?:$|[\s/_.-]|\d)/i],
  ["mistral", /(?:^|[\s/_.-])(mistral|codestral|ministral|mixtral)(?:$|[\s/_.-]|\d)/i],
  ["deepseek", /(?:^|[\s/_.-])deepseek(?:$|[\s/_.-])/i],
  ["alibaba", /(?:^|[\s/_.-])(alibaba|qwen)(?:$|[\s/_.-]|\d)/i],
  ["moonshot", /(?:^|[\s/_.-])(moonshot|kimi)(?:$|[\s/_.-]|\d)/i],
  ["minimax", /(?:^|[\s/_.-])minimax(?:$|[\s/_.-])/i],
];

/**
 * Resolves the company presented for a model while preserving its execution integration.
 *
 * @param familyId - Family of the installed execution integration.
 * @param model - Provider model when one is available.
 * @returns Stable company identity used to group the model picker.
 */
export function modelCompany(familyId: string, model: ProviderModel | null): ModelCompanyIdentity {
  if (model) {
    return modelCompanyForIdentity(familyId, `${model.id} ${model.displayName}`);
  }
  return COMPANIES[FAMILY_DEFAULTS[familyId] ?? "opencode"];
}

/** Resolves a model company from a stored model identity when no catalog row remains. */
export function modelCompanyForIdentity(
  familyId: string,
  identity: string | null,
): ModelCompanyIdentity {
  if (identity) {
    for (const [companyId, matcher] of MODEL_MATCHERS) {
      if (matcher.test(identity)) return COMPANIES[companyId];
    }
  }
  return COMPANIES[FAMILY_DEFAULTS[familyId] ?? "opencode"];
}

/**
 * Returns the default company represented by an execution integration.
 *
 * @param familyId - Execution integration family identifier.
 * @returns Default company identity for setup and provider-managed model states.
 */
export function integrationCompany(familyId: string): ModelCompanyIdentity {
  return COMPANIES[FAMILY_DEFAULTS[familyId] ?? "opencode"];
}

/**
 * Formats a provider model name for human-facing controls.
 *
 * Provider catalogs commonly use identifier-style hyphens in display names.
 * The UI presents those separators as spaces while preserving the catalog value
 * used for provider requests.
 *
 * @param displayName - Display name supplied by the provider catalog.
 * @returns A compact human-facing model name.
 */
export function formatModelDisplayName(displayName: string): string {
  return displayName.trim().replace(/-+/g, " ").replace(/\s+/g, " ");
}

/**
 * Reports whether a custom model's exact ID should accompany its display name.
 *
 * Provider-managed aliases and versioned IDs are implementation details. Custom
 * model IDs remain visible when their user-defined label does not identify the
 * exact provider value.
 *
 * @param model - Catalog model whose visible identity is being composed.
 * @returns Whether the exact custom model ID should appear below the name.
 */
export function shouldShowModelId(
  model: Pick<ProviderModel, "id" | "displayName" | "custom">,
): boolean {
  return model.custom
    && normalizeVisibleModelIdentity(model.id) !== normalizeVisibleModelIdentity(model.displayName);
}

/**
 * Compares models in the capability order people expect within a company catalog.
 *
 * Provider catalogs do not consistently arrive newest-first, and alphabetical order
 * puts lightweight variants ahead of flagship models. Known product families use
 * their public capability hierarchy, then semantic model versions. Unknown families
 * retain the provider's original order through the stable caller sort.
 *
 * @param companyId - Company subsection containing both models.
 * @param left - First model to compare.
 * @param right - Second model to compare.
 * @returns A standard array-sort comparison value.
 */
export function compareCompanyModels(
  companyId: ModelCompanyId,
  left: ProviderModel,
  right: ProviderModel,
): number {
  const leftIdentity = modelIdentity(left);
  const rightIdentity = modelIdentity(right);
  if (companyId === "anthropic") {
    return anthropicFamilyRank(leftIdentity) - anthropicFamilyRank(rightIdentity)
      || compareVersionsDescending(leftIdentity, rightIdentity);
  }
  if (companyId === "xai") {
    return xaiFamilyRank(leftIdentity) - xaiFamilyRank(rightIdentity)
      || compareVersionsDescending(leftIdentity, rightIdentity)
      || genericVariantRank(leftIdentity) - genericVariantRank(rightIdentity);
  }
  if (companyId === "openai") {
    return compareVersionsDescending(leftIdentity, rightIdentity)
      || openAiVariantRank(leftIdentity) - openAiVariantRank(rightIdentity);
  }
  if (["google", "meta", "mistral", "deepseek", "alibaba", "moonshot", "minimax"].includes(companyId)) {
    return compareVersionsDescending(leftIdentity, rightIdentity)
      || genericVariantRank(leftIdentity) - genericVariantRank(rightIdentity);
  }
  return 0;
}

function modelIdentity(model: ProviderModel): string {
  return `${model.displayName} ${model.id}`.toLowerCase();
}

function normalizeVisibleModelIdentity(value: string): string {
  return value.trim().toLowerCase().replace(/[\s._-]+/g, " ");
}

function anthropicFamilyRank(identity: string): number {
  if (identity.includes("opus")) return 0;
  if (identity.includes("sonnet")) return 1;
  if (identity.includes("haiku")) return 2;
  return 3;
}

function xaiFamilyRank(identity: string): number {
  if (/grok[\s/_.-]*build/.test(identity)) return 0;
  if (/grok[\s/_.-]*code[\s/_.-]*fast/.test(identity)) return 2;
  return 1;
}

function openAiVariantRank(identity: string): number {
  if (identity.includes("sol")) return 0;
  if (identity.includes("terra")) return 1;
  if (identity.includes("luna")) return 2;
  if (identity.includes("pro")) return 3;
  if (identity.includes("spark")) return 8;
  if (identity.includes("nano")) return 7;
  if (identity.includes("mini")) return 6;
  if (identity.includes("codex")) return 5;
  return 4;
}

function genericVariantRank(identity: string): number {
  if (identity.includes("ultra") || identity.includes("max") || identity.includes("pro")) return 0;
  if (identity.includes("flash-lite") || identity.includes("flash lite")) return 4;
  if (identity.includes("flash") || identity.includes("fast")) return 3;
  if (identity.includes("mini") || identity.includes("nano") || identity.includes("lite")) return 2;
  return 1;
}

function compareVersionsDescending(leftIdentity: string, rightIdentity: string): number {
  const leftVersion = modelVersion(leftIdentity);
  const rightVersion = modelVersion(rightIdentity);
  const length = Math.max(leftVersion.length, rightVersion.length);
  for (let index = 0; index < length; index += 1) {
    const comparison = (rightVersion[index] ?? 0) - (leftVersion[index] ?? 0);
    if (comparison !== 0) return comparison;
  }
  return 0;
}

function modelVersion(identity: string): number[] {
  const match = /(?:^|[^\d])(\d+(?:[.-]\d+){0,2})(?=$|[^\d])/.exec(identity);
  return match?.[1]?.split(/[.-]/).map(Number) ?? [];
}
