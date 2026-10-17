import type {
  ChatPromptCatalogEntry,
  ChatSendKey,
  ProjectWorkingFolderPathRead,
  InteractionMode,
  JsonValue,
  ModelOptionDefinition,
  ModelOptionSelection,
  ProviderCapabilities,
  ProviderInstanceRead,
  ProviderModel,
  ProviderSessionState,
  RateLimitStatusEvent,
  SafetyMode,
  UserInputQuestion,
  VersionedJson,
} from "$lib/chat/contracts";
import { compareCompanyModels, integrationCompany, modelCompany } from "./model-company";

export const CHAT_IMAGE_LIMIT = 8;
export const CHAT_IMAGE_BYTE_LIMIT = 20 * 1024 * 1024;
export const CHAT_IMAGE_TOTAL_BYTE_LIMIT = 50 * 1024 * 1024;

export type ComposerPrimaryAction = "send" | "stop" | "stopping" | "resolve_request";
export type ComposerFollowupAction = "steer" | "queue" | "retain" | null;

export interface ComposerActionState {
  primary: ComposerPrimaryAction;
  followup: ComposerFollowupAction;
  sendEnabled: boolean;
}

export interface ComposerSelections {
  workingFolderId: string | null;
  providerInstanceId: string | null;
  modelId: string | null;
  providerManagedModel: boolean;
  safetyMode: SafetyMode | null;
  interactionMode: InteractionMode | null;
  fullAccessTrusted: boolean;
}

export interface ComposerSelectionError {
  field: "workspace" | "provider" | "model" | "safety" | "interaction" | "trust";
  message: string;
}

export interface DefaultProviderModelSelection {
  provider: ProviderInstanceRead;
  model: ProviderModel | null;
  providerManaged: boolean;
  options: ModelOptionSelection[];
}

/** Reports whether a configured provider can currently accept Chat work. */
export function isProviderAvailable(provider: ProviderInstanceRead): boolean {
  return provider.configuration.enabled && provider.lastProbe?.state === "healthy";
}

/** Orders usable providers by Ganbaru's model-company priority, with OpenAI first. */
export function availableProvidersInDefaultOrder(
  providers: readonly ProviderInstanceRead[],
): ProviderInstanceRead[] {
  return providers
    .map((provider, index) => ({ provider, index }))
    .filter(({ provider }) => isProviderAvailable(provider))
    .sort((left, right) => (
      integrationCompany(left.provider.configuration.familyId).order
      - integrationCompany(right.provider.configuration.familyId).order
      || left.index - right.index
    ))
    .map(({ provider }) => provider);
}

/** Selects the provider's `default` model, otherwise its strongest visible built-in model. */
export function recommendedProviderModel(
  provider: ProviderInstanceRead,
  candidates: readonly ProviderModel[] = configuredVisibleModels(provider),
): ProviderModel | null {
  const selectable = candidates.filter((model) => (
    model.availability === "available" || model.availability === "stale"
  ));
  const builtIn = selectable.filter((model) => !model.custom);
  const pool = builtIn.length > 0 ? builtIn : selectable;
  const providerDefault = pool.find((model) => model.id === "default");
  if (providerDefault) return providerDefault;
  const first = pool[0];
  if (!first) return null;
  const company = modelCompany(provider.configuration.familyId, first);
  const sameCompany = pool.filter((model) => (
    modelCompany(provider.configuration.familyId, model).id === company.id
  ));
  return [...sameCompany].sort((left, right) => (
    compareCompanyModels(company.id, left, right)
  ))[0] ?? first;
}

/** Resolves default model options, preferring Medium for reasoning when supported. */
export function defaultModelOptions(
  definitions: readonly ModelOptionDefinition[],
): ModelOptionSelection[] {
  const options: ModelOptionSelection[] = [];
  for (const definition of definitions) {
    switch (definition.kind) {
      case "boolean":
        if (definition.defaultValue !== null) {
          options.push({ key: definition.key, value: { kind: "boolean", value: definition.defaultValue } });
        }
        break;
      case "choice": {
        const medium = modelOptionRole(definition) === "effort"
          ? definition.options.find((option) => option.value.toLowerCase() === "medium")?.value ?? null
          : null;
        const value = medium ?? definition.defaultValue;
        if (value !== null) options.push({ key: definition.key, value: { kind: "choice", value } });
        break;
      }
      case "multiple_choice":
        options.push({ key: definition.key, value: { kind: "multiple_choice", value: [...definition.defaultValue] } });
        break;
      case "integer_range":
        if (definition.defaultValue !== null) {
          options.push({ key: definition.key, value: { kind: "integer", value: definition.defaultValue } });
        }
        break;
      case "text":
        if (definition.defaultValue !== null) {
          options.push({ key: definition.key, value: { kind: "text", value: definition.defaultValue } });
        }
        break;
      case "unknown":
        break;
    }
  }
  return options;
}

/** Copies model options without retaining reactive collection proxies. */
export function copyModelOptionSelections(
  selections: readonly ModelOptionSelection[],
): ModelOptionSelection[] {
  return selections.map((selection) => {
    switch (selection.value.kind) {
      case "multiple_choice":
        return {
          key: selection.key,
          value: { kind: "multiple_choice", value: [...selection.value.value] },
        };
      case "unknown":
        return {
          key: selection.key,
          value: {
            kind: "unknown",
            value: copyVersionedJson(selection.value.value),
          },
        };
      default:
        return { key: selection.key, value: { ...selection.value } };
    }
  });
}

/** Copies versioned JSON without retaining reactive object proxies. */
export function copyVersionedJson(value: VersionedJson): VersionedJson {
  return {
    schemaVersion: value.schemaVersion,
    value: copyJsonValue(value.value),
  };
}

/** Resolves a ready initial provider and model while honoring a healthy folder preference. */
export function resolveDefaultProviderModel(
  providers: readonly ProviderInstanceRead[],
  preferredProviderId: string | null = null,
): DefaultProviderModelSelection | null {
  const available = availableProvidersInDefaultOrder(providers);
  const provider = available.find((entry) => (
    entry.configuration.instanceId === preferredProviderId
  )) ?? available[0];
  if (!provider) return null;
  const model = recommendedProviderModel(provider);
  const providerManaged = model === null && (provider.modelCatalog?.models.length ?? 0) === 0;
  return {
    provider,
    model,
    providerManaged,
    options: model ? defaultModelOptions(model.options) : [],
  };
}

/** Lists non-deprecated models allowed by the provider visibility setting; an empty setting allows all. */
function configuredVisibleModels(provider: ProviderInstanceRead): ProviderModel[] {
  const visibleIds = provider.configuration.visibleModelIds;
  return provider.modelCatalog?.models.filter((model) => (
    model.availability !== "deprecated"
    && (visibleIds.length === 0 || visibleIds.includes(model.id))
  )) ?? [];
}

/** Classifies a known model option as the effort, speed, or other control. */
export function modelOptionRole(
  definition: Exclude<ModelOptionDefinition, { kind: "unknown" }>,
): "effort" | "speed" | "other" {
  const identity = `${definition.key} ${definition.label}`.toLowerCase();
  if (identity.includes("effort") || identity.includes("reasoning")) return "effort";
  if (identity.includes("speed") || identity.includes("fast") || identity.includes("service tier") || identity.includes("service_tier")) return "speed";
  return "other";
}

function copyJsonValue(value: JsonValue): JsonValue {
  if (Array.isArray(value)) return value.map(copyJsonValue);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, copyJsonValue(entry)]),
    );
  }
  return value;
}

export function interactionModeForPrompt(
  text: string,
  current: InteractionMode | null,
): InteractionMode {
  return /^\/plan(?:\s|$)/i.test(text.trimStart()) ? "plan" : current ?? "build";
}

export interface ComposerTokenTrigger {
  kind: "mention" | "skill" | "command";
  query: string;
  start: number;
  end: number;
}

export interface ContextMeterRead {
  usedTokens: number;
  maximumTokens: number | null;
  ratio: number | null;
  warning: boolean;
}

export interface ComposerRateLimitWindow {
  id: string;
  label: string;
  usedPercent: number;
  windowDurationMinutes: number | null;
  resetsAtSeconds: number | null;
}

export function composerRateLimitWindows(
  status: RateLimitStatusEvent | null,
): ComposerRateLimitWindow[] {
  const root = readJsonRecord(status?.providerData?.value);
  if (!root) return [];
  const bucketRecords = readJsonRecord(root.rateLimitsByLimitId);
  const buckets: [string, unknown][] = bucketRecords
    ? Object.entries(bucketRecords)
    : [[readJsonString(root.limitId) ?? "rate-limit", root]];
  const windows: ComposerRateLimitWindow[] = [];
  for (const [bucketId, bucketValue] of buckets) {
    const bucket = readJsonRecord(bucketValue);
    if (!bucket) continue;
    const bucketLabel = readJsonString(bucket.limitName) ?? readJsonString(bucket.limitId) ?? bucketId;
    for (const windowName of ["primary", "secondary"] as const) {
      const window = readJsonRecord(bucket[windowName]);
      const usedPercent = readJsonNumber(window?.usedPercent);
      if (!window || usedPercent === null) continue;
      const duration = readJsonNumber(window.windowDurationMins);
      windows.push({
        id: `${bucketId}:${windowName}`,
        label: duration === null ? bucketLabel : `${formatWindowDuration(duration)} ${bucketLabel}`,
        usedPercent: Math.max(0, Math.min(100, usedPercent)),
        windowDurationMinutes: duration,
        resetsAtSeconds: readJsonNumber(window.resetsAt),
      });
    }
  }
  return windows;
}

function readJsonRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function readJsonString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function readJsonNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function formatWindowDuration(minutes: number): string {
  if (minutes >= 1_440 && minutes % 1_440 === 0) return `${minutes / 1_440}d`;
  if (minutes >= 60 && minutes % 60 === 0) return `${minutes / 60}h`;
  return `${minutes}m`;
}

export interface ApprovalChoiceRead {
  id: string;
  label: string;
  decisionKind: "allow_once" | "allow_session" | "deny" | "cancel";
  description: string | null;
}

const ACTIVE_SESSION_STATES: ProviderSessionState[] = [
  "active",
  "waiting_for_approval",
  "waiting_for_user_input",
];

export function composerActionState(
  sessionState: ProviderSessionState,
  capabilities: ProviderCapabilities,
  hasDraft: boolean,
  hasPendingRequest: boolean,
): ComposerActionState {
  if (hasPendingRequest) return { primary: "resolve_request", followup: null, sendEnabled: false };
  if (sessionState === "stopping") return { primary: "stopping", followup: null, sendEnabled: false };
  if (ACTIVE_SESSION_STATES.includes(sessionState)) {
    const followup = !hasDraft
      ? null
      : supports(capabilities, "steering")
        ? "steer"
        : supports(capabilities, "queued_follow_up")
          ? "queue"
          : "retain";
    return { primary: "stop", followup, sendEnabled: false };
  }
  return { primary: "send", followup: null, sendEnabled: hasDraft && sessionState !== "starting" };
}

export function queuedFollowupDispatchReady(
  sessionState: ProviderSessionState,
  latestTurnState: string | null,
): boolean {
  if (sessionState === "ready") return true;
  return sessionState === "stopped"
    && !["pending", "dispatching", "active", "waiting_for_approval", "waiting_for_user_input"]
      .includes(latestTurnState ?? "");
}

export function validateComposerSelections(
  selections: ComposerSelections,
  capabilities: ProviderCapabilities,
): ComposerSelectionError[] {
  const errors: ComposerSelectionError[] = [];
  if (!selections.workingFolderId) errors.push({ field: "workspace", message: "Choose a workspace" });
  if (!selections.providerInstanceId) errors.push({ field: "provider", message: "Choose a provider" });
  if (!selections.modelId && !selections.providerManagedModel) errors.push({ field: "model", message: "Choose a model or provider-managed model" });
  if (!selections.safetyMode) errors.push({ field: "safety", message: "Choose a safety mode" });
  if (!selections.interactionMode) errors.push({ field: "interaction", message: "Choose Build or Plan" });
  if (selections.interactionMode === "plan" && !supports(capabilities, "native_plan")) {
    errors.push({ field: "interaction", message: "This provider does not support native Plan mode" });
  }
  if ((selections.safetyMode === "full_access" || selections.safetyMode === "custom") && !selections.fullAccessTrusted) {
    errors.push({ field: "trust", message: "Confirm broad permission trust for this provider and workspace" });
  }
  return errors;
}

export function composerTokenTrigger(text: string, cursor: number): ComposerTokenTrigger | null {
  const before = text.slice(0, Math.max(0, Math.min(cursor, text.length)));
  const command = /^([ \t]*)\/([^\s/]*)$/.exec(before);
  if (command) {
    return {
      kind: "command",
      query: command[2],
      start: command[1].length,
      end: before.length,
    };
  }
  const match = /(^|\s)([@$])([^\s@$]*)$/.exec(before);
  if (!match) return null;
  const prefix = match[2];
  return {
    kind: prefix === "@" ? "mention" : "skill",
    query: match[3],
    start: before.length - match[3].length - 1,
    end: before.length,
  };
}

export interface ComposerModeCommand {
  mode: InteractionMode;
  prompt: string;
}

export function composerModeCommand(text: string): ComposerModeCommand | null {
  const match = /^\s*\/(plan|build)(?:\s+([\s\S]*))?$/i.exec(text);
  if (!match) return null;
  return {
    mode: match[1].toLowerCase() === "plan" ? "plan" : "build",
    prompt: match[2]?.trimStart() ?? "",
  };
}

export function filterPromptCatalog(
  entries: ChatPromptCatalogEntry[],
  kind: "skill" | "command",
  query: string,
): ChatPromptCatalogEntry[] {
  const normalized = query.trim().toLowerCase();
  return entries
    .filter((entry) => entry.kind === kind && fuzzyMatch(
      `${entry.label} ${entry.value} ${entry.description ?? ""} ${entry.argumentHint ?? ""}`,
      normalized,
    ))
    .sort((left, right) => fuzzyRank(left.label, normalized) - fuzzyRank(right.label, normalized)
      || left.label.localeCompare(right.label));
}

export function filterWorkspacePaths(entries: ProjectWorkingFolderPathRead[], query: string): ProjectWorkingFolderPathRead[] {
  const normalized = query.trim().toLowerCase();
  return [...entries]
    .filter((entry) => fuzzyMatch(entry.relativePath, normalized))
    .sort((left, right) => fuzzyRank(left.relativePath, normalized) - fuzzyRank(right.relativePath, normalized)
      || left.relativePath.localeCompare(right.relativePath));
}

export function rankedModels(
  models: ProviderModel[],
  favoriteIds: string[],
  recentIds: string[],
  query: string,
): ProviderModel[] {
  const favorite = new Map(favoriteIds.map((id, index) => [id, index]));
  const recent = new Map(recentIds.map((id, index) => [id, index]));
  const normalized = query.trim().toLowerCase();
  return models
    .filter((model) => fuzzyMatch(`${model.displayName} ${model.id}`, normalized))
    .sort((left, right) => {
      const leftGroup = favorite.has(left.id) ? 0 : recent.has(left.id) ? 1 : 2;
      const rightGroup = favorite.has(right.id) ? 0 : recent.has(right.id) ? 1 : 2;
      return leftGroup - rightGroup
        || (favorite.get(left.id) ?? recent.get(left.id) ?? Number.MAX_SAFE_INTEGER)
          - (favorite.get(right.id) ?? recent.get(right.id) ?? Number.MAX_SAFE_INTEGER)
        || fuzzyRank(`${left.displayName} ${left.id}`, normalized) - fuzzyRank(`${right.displayName} ${right.id}`, normalized)
        || left.displayName.localeCompare(right.displayName);
    });
}

export function validateModelOptions(
  definitions: ModelOptionDefinition[],
  selections: ModelOptionSelection[],
): string[] {
  const definitionsByKey = new Map(definitions.map((definition) => [definition.key, definition]));
  const errors: string[] = [];
  for (const selection of selections) {
    const definition = definitionsByKey.get(selection.key);
    if (!definition || definition.kind === "unknown") continue;
    if (!optionMatchesDefinition(definition, selection)) errors.push(selection.key);
  }
  return errors;
}

export function composerModelSelection(
  modelId: string | null,
  providerManaged: boolean,
  options: ModelOptionSelection[],
): VersionedJson {
  return {
    schemaVersion: 1,
    value: { modelId, providerManaged, options: options as unknown as JsonValue },
  };
}

export function readComposerModelSelection(value: VersionedJson | null): {
  modelId: string | null;
  providerManaged: boolean;
  options: ModelOptionSelection[];
} {
  if (!value || typeof value.value !== "object" || value.value === null || Array.isArray(value.value)) {
    return { modelId: null, providerManaged: false, options: [] };
  }
  const modelId = typeof value.value.modelId === "string" ? value.value.modelId : null;
  const providerManaged = value.value.providerManaged === true;
  const options = Array.isArray(value.value.options) ? value.value.options as unknown as ModelOptionSelection[] : [];
  return { modelId, providerManaged, options };
}

export function contextMeter(usedTokens: number | null, maximumTokens: number | null): ContextMeterRead | null {
  if (usedTokens === null || !Number.isSafeInteger(usedTokens) || usedTokens < 0) return null;
  const maximum = maximumTokens !== null && Number.isSafeInteger(maximumTokens) && maximumTokens > 0
    ? maximumTokens
    : null;
  const ratio = maximum === null ? null : Math.min(1, usedTokens / maximum);
  return { usedTokens, maximumTokens: maximum, ratio, warning: ratio !== null && ratio >= 0.85 };
}

export function parseApprovalChoices(value: VersionedJson): ApprovalChoiceRead[] {
  if (!Array.isArray(value.value)) return [];
  return value.value.flatMap((entry) => {
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) return [];
    const id = entry.id;
    const label = entry.label;
    const decisionKind = entry.decisionKind;
    const description = entry.description;
    if (typeof id !== "string" || typeof label !== "string") return [];
    if (!["allow_once", "allow_session", "deny", "cancel"].includes(String(decisionKind))) return [];
    if (description !== null && typeof description !== "string") return [];
    return [{ id, label, decisionKind: decisionKind as ApprovalChoiceRead["decisionKind"], description }];
  });
}

export function parseUserInputQuestions(value: VersionedJson): UserInputQuestion[] {
  if (!Array.isArray(value.value)) return [];
  return value.value.flatMap((entry) => {
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) return [];
    if (typeof entry.id !== "string" || typeof entry.question !== "string" || !Array.isArray(entry.options)) return [];
    const options = entry.options.flatMap((option) => {
      if (typeof option !== "object" || option === null || Array.isArray(option)) return [];
      if (typeof option.id !== "string" || typeof option.label !== "string") return [];
      return [{ id: option.id, label: option.label, description: typeof option.description === "string" ? option.description : null }];
    });
    return [{
      id: entry.id,
      header: typeof entry.header === "string" ? entry.header : null,
      question: entry.question,
      options,
      multiple: entry.multiple === true,
      freeFormAllowed: entry.freeFormAllowed === true,
      required: entry.required === true,
    }];
  });
}

export function shouldSendComposerKey(event: Pick<KeyboardEvent, "key" | "shiftKey" | "ctrlKey" | "metaKey" | "isComposing">, sendKey: ChatSendKey): boolean {
  if (event.isComposing || event.shiftKey || event.key !== "Enter") return false;
  const modified = event.ctrlKey || event.metaKey;
  return sendKey === "enter" ? !modified : modified;
}

export function validateImageFiles(
  files: Pick<File, "name" | "size" | "type">[],
  existingCount: number,
  existingBytes = 0,
): string | null {
  if (existingCount + files.length > CHAT_IMAGE_LIMIT) return `Attach up to ${CHAT_IMAGE_LIMIT} images`;
  const unsupported = files.find((file) => !["image/png", "image/jpeg", "image/gif", "image/webp"].includes(file.type));
  if (unsupported) return `${unsupported.name} is not a supported image`;
  const oversized = files.find((file) => file.size > CHAT_IMAGE_BYTE_LIMIT);
  if (oversized) return `${oversized.name} exceeds the 20 MiB limit`;
  const totalBytes = files.reduce((total, file) => total + file.size, existingBytes);
  if (totalBytes > CHAT_IMAGE_TOTAL_BYTE_LIMIT) return "Chat images must total 50 MiB or less";
  return null;
}

/** Reports whether the selected provider and model can receive an image prompt. */
export function supportsImagePrompt(
  providerFamilyId: string | null,
  model: ProviderModel | null,
  capabilities: ProviderCapabilities,
): boolean {
  if (model?.capabilities.includes("images")) return true;
  if (providerFamilyId === "codex" && model && !model.custom) return false;
  return supports(capabilities, "images");
}

/** Returns deduplicated clipboard images from both file and item representations. */
export function clipboardImageFiles(
  clipboard: Pick<DataTransfer, "files" | "items"> | null,
): File[] {
  if (!clipboard) return [];
  const images: File[] = [];
  const add = (file: File | null, advertisedType?: string): void => {
    if (!file) return;
    const mimeType = file.type || advertisedType || "";
    if (!mimeType.startsWith("image/")) return;
    if (file.name.trim()) {
      images.push(file);
      return;
    }
    const extension = imageExtension(mimeType);
    images.push(new File([file], `pasted-image.${extension}`, {
      type: mimeType,
      lastModified: file.lastModified,
    }));
  };
  for (const file of Array.from(clipboard.files)) add(file);
  if (images.length === 0) {
    for (const item of Array.from(clipboard.items)) {
      if (item.kind === "file") add(item.getAsFile(), item.type);
    }
  }
  return images;
}

function imageExtension(mimeType: string): "png" | "jpg" | "gif" | "webp" {
  if (mimeType === "image/jpeg") return "jpg";
  if (mimeType === "image/gif") return "gif";
  if (mimeType === "image/webp") return "webp";
  return "png";
}

export function validateUserInputAnswers(
  questions: UserInputQuestion[],
  answers: import("$lib/chat/contracts").UserInputAnswer[],
): string[] {
  const byQuestion = new Map(answers.map((answer) => [answer.questionId, answer]));
  const invalid: string[] = [];
  for (const question of questions) {
    const answer = byQuestion.get(question.id);
    const selected = answer?.selectedOptionIds ?? [];
    const freeForm = answer?.freeFormText?.trim() ?? "";
    const optionIds = new Set(question.options.map((option) => option.id));
    const validSelection = selected.every((optionId) => optionIds.has(optionId))
      && (question.multiple || selected.length <= 1);
    const validFreeForm = question.freeFormAllowed || freeForm.length === 0;
    const complete = !question.required || selected.length > 0 || freeForm.length > 0;
    if (!validSelection || !validFreeForm || !complete) invalid.push(question.id);
  }
  return invalid;
}

function supports(capabilities: ProviderCapabilities, capability: string): boolean {
  return capabilities.entries.some((entry) => entry.capability === capability && entry.supported);
}

function fuzzyMatch(value: string, query: string): boolean {
  if (!query) return true;
  const normalized = value.toLowerCase();
  if (normalized.includes(query)) return true;
  let index = 0;
  for (const character of normalized) if (character === query[index]) index += 1;
  return index === query.length;
}

function fuzzyRank(value: string, query: string): number {
  if (!query) return 0;
  const normalized = value.toLowerCase();
  if (normalized === query) return 0;
  if (normalized.startsWith(query)) return 1;
  const index = normalized.indexOf(query);
  return index === -1 ? 1_000 + normalized.length : 10 + index;
}

function optionMatchesDefinition(definition: ModelOptionDefinition, selection: ModelOptionSelection): boolean {
  switch (definition.kind) {
    case "boolean": return selection.value.kind === "boolean";
    case "choice": return selection.value.kind === "choice" && definition.options.some((option) => option.value === selection.value.value);
    case "multiple_choice": return selection.value.kind === "multiple_choice" && selection.value.value.every((value) => definition.options.some((option) => option.value === value));
    case "integer_range": return selection.value.kind === "integer"
      && selection.value.value >= definition.minimum
      && selection.value.value <= definition.maximum
      && (selection.value.value - definition.minimum) % definition.step === 0;
    case "text": return selection.value.kind === "text" && (definition.allowEmpty || selection.value.value.length > 0);
    case "unknown": return true;
  }
}
