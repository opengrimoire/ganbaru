import {
  CHAT_SEND_KEYS,
  INTERACTION_MODES,
  SAFETY_MODES,
  type ChatBehaviorPreferences,
  type ChatPanelPreferences,
  type ChatPortableProviderConfig,
  type ChatConfiguration,
  type RememberedComposerSelection,
} from "../contracts";
import { parseModelOptionSelection } from "./provider";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readNullable,
  readNonNegativeSafeInteger,
  readRecord,
  readString,
  readStringRecord,
  readVersionedJson,
} from "./readers";

const MAX_PROVIDERS = 64;
const MAX_ARGUMENTS = 64;
const MAX_ENVIRONMENT_ROWS = 64;
const MAX_MODELS_PER_SET = 512;
const MAX_REMEMBERED_SELECTIONS = 256;
const MAX_LABEL_BYTES = 160;
const MAX_ARGUMENT_BYTES = 4_096;
const MAX_ENVIRONMENT_VALUE_BYTES = 16_384;

const DEFAULT_PANELS: ChatPanelPreferences = { inspectorWidthPx: 520 };
const DEFAULT_BEHAVIOR: ChatBehaviorPreferences = {
  sendKey: "enter",
  restoreLastSelectedThread: true,
  showReasoningSummaries: true,
  automaticallyFoldSettledWork: true,
  terminalScrollbackLines: 10_000,
  idleSessionTimeoutSeconds: 900,
  confirmMultilineTerminalPaste: true,
};

export function defaultChatConfiguration(): ChatConfiguration {
  return {
    providers: [],
    automaticProviderSetupDisabled: [],
    rememberedSelections: [],
    workingFolderProviderPreferences: {},
    panels: { ...DEFAULT_PANELS },
    behavior: { ...DEFAULT_BEHAVIOR },
  };
}

function readBoundedString(value: unknown, maximum: number, label: string, required: boolean): string {
  const text = readString(value, label);
  if (required && text.trim().length === 0) throw new Error(`${label} is required`);
  if (new TextEncoder().encode(text).byteLength > maximum) {
    throw new Error(`${label} exceeds the ${maximum} byte limit`);
  }
  return text;
}

function parsePortableProvider(value: unknown, label: string): ChatPortableProviderConfig {
  const record = readRecord(value, label);
  for (const field of ["executable", "providerHome", "canonicalPath", "lastProbe"]) {
    if (Object.hasOwn(record, field)) {
      throw new Error(`${label} contains machine-specific field ${field}`);
    }
  }
  const launchArguments = readArray(
    record.launchArguments,
    `${label}.launchArguments`,
    (entry, entryLabel) => readBoundedString(entry, MAX_ARGUMENT_BYTES, entryLabel, false),
  );
  if (launchArguments.length > MAX_ARGUMENTS) throw new Error(`${label}.launchArguments exceeds the item limit`);
  const environment = readStringRecord(record.environment, `${label}.environment`);
  if (Object.keys(environment).length > MAX_ENVIRONMENT_ROWS) throw new Error(`${label}.environment exceeds the item limit`);
  for (const [name, environmentValue] of Object.entries(environment)) {
    if (!/^[A-Za-z0-9_]+$/.test(name)) throw new Error(`${label}.environment.${name} has an invalid name`);
    readBoundedString(environmentValue, MAX_ENVIRONMENT_VALUE_BYTES, `${label}.environment.${name}`, false);
  }
  const visibleModelIds = readArray(record.visibleModelIds, `${label}.visibleModelIds`, readIdentifier);
  const favoriteModelIds = readArray(record.favoriteModelIds, `${label}.favoriteModelIds`, readIdentifier);
  if (visibleModelIds.length > MAX_MODELS_PER_SET || favoriteModelIds.length > MAX_MODELS_PER_SET) {
    throw new Error(`${label}.visibleModelIds exceeds the item limit`);
  }
  return {
    ...record,
    schemaVersion: readNonNegativeSafeInteger(record.schemaVersion, `${label}.schemaVersion`),
    instanceId: readIdentifier(record.instanceId, `${label}.instanceId`),
    familyId: readIdentifier(record.familyId, `${label}.familyId`),
    label: readBoundedString(record.label, MAX_LABEL_BYTES, `${label}.label`, true),
    enabled: readBoolean(record.enabled, `${label}.enabled`),
    launchArguments,
    environment,
    credentialReferences: readStringRecord(record.credentialReferences, `${label}.credentialReferences`),
    visibleModelIds,
    favoriteModelIds,
    providerConfig: readVersionedJson(record.providerConfig, `${label}.providerConfig`),
  };
}

function parseRememberedSelection(value: unknown, label: string): RememberedComposerSelection {
  const record = readRecord(value, label);
  const modelId = readNullable(record.modelId, `${label}.modelId`, readIdentifier);
  const providerManagedModel = readBoolean(record.providerManagedModel, `${label}.providerManagedModel`);
  if (modelId === null && !providerManagedModel) {
    throw new Error(`${label}.modelId or provider-managed model state is required`);
  }
  return {
    workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
    providerInstanceId: readIdentifier(record.providerInstanceId, `${label}.providerInstanceId`),
    modelId,
    providerManagedModel,
    modelOptions: readArray(record.modelOptions, `${label}.modelOptions`, parseModelOptionSelection),
    safetyMode: readEnum(record.safetyMode, SAFETY_MODES, `${label}.safetyMode`),
    interactionMode: readEnum(record.interactionMode, INTERACTION_MODES, `${label}.interactionMode`),
  };
}

function parsePanels(value: unknown, label: string): ChatPanelPreferences {
  const record = readRecord(value, label);
  const inspectorWidthPx = readNonNegativeSafeInteger(
    record.inspectorWidthPx,
    `${label}.inspectorWidthPx`,
  );
  if (inspectorWidthPx < 240 || inspectorWidthPx > 960) throw new Error(`${label}.inspectorWidthPx is out of range`);
  return { inspectorWidthPx };
}

function parseBehavior(value: unknown, label: string): ChatBehaviorPreferences {
  const record = readRecord(value, label);
  const terminalScrollbackLines = readNonNegativeSafeInteger(
    record.terminalScrollbackLines,
    `${label}.terminalScrollbackLines`,
  );
  const idleSessionTimeoutSeconds = readNonNegativeSafeInteger(
    record.idleSessionTimeoutSeconds,
    `${label}.idleSessionTimeoutSeconds`,
  );
  if (terminalScrollbackLines < 1_000 || terminalScrollbackLines > 100_000) {
    throw new Error(`${label}.terminalScrollbackLines is out of range`);
  }
  if (idleSessionTimeoutSeconds < 60 || idleSessionTimeoutSeconds > 7_200) {
    throw new Error(`${label}.idleSessionTimeoutSeconds is out of range`);
  }
  return {
    sendKey: readEnum(record.sendKey, CHAT_SEND_KEYS, `${label}.sendKey`),
    restoreLastSelectedThread: readBoolean(
      record.restoreLastSelectedThread,
      `${label}.restoreLastSelectedThread`,
    ),
    showReasoningSummaries: readBoolean(
      record.showReasoningSummaries,
      `${label}.showReasoningSummaries`,
    ),
    automaticallyFoldSettledWork: readBoolean(
      record.automaticallyFoldSettledWork,
      `${label}.automaticallyFoldSettledWork`,
    ),
    terminalScrollbackLines,
    idleSessionTimeoutSeconds,
    confirmMultilineTerminalPaste: readBoolean(
      record.confirmMultilineTerminalPaste,
      `${label}.confirmMultilineTerminalPaste`,
    ),
  };
}

/**
 * Validates a Chat settings read from the backend and drops unknown top-level fields.
 */
export function parseChatConfiguration(value: unknown, label = "chat"): ChatConfiguration {
  const record = readRecord(value, label);
  const providers = readArray(record.providers, `${label}.providers`, parsePortableProvider);
  if (providers.length > MAX_PROVIDERS) throw new Error(`${label}.providers exceeds the item limit`);
  const automaticProviderSetupDisabled = readArray(
    record.automaticProviderSetupDisabled,
    `${label}.automaticProviderSetupDisabled`,
    readIdentifier,
  );
  if (
    automaticProviderSetupDisabled.length > MAX_PROVIDERS
    || new Set(automaticProviderSetupDisabled).size !== automaticProviderSetupDisabled.length
  ) {
    throw new Error(`${label}.automaticProviderSetupDisabled is invalid`);
  }
  const instanceIds = new Set<string>();
  for (const provider of providers) {
    if (instanceIds.has(provider.instanceId)) throw new Error(`${label}.providers contains a duplicate instance ID`);
    instanceIds.add(provider.instanceId);
  }
  const rememberedSelections = readArray(
    record.rememberedSelections,
    `${label}.rememberedSelections`,
    parseRememberedSelection,
  );
  if (rememberedSelections.length > MAX_REMEMBERED_SELECTIONS) {
    throw new Error(`${label}.rememberedSelections exceeds the item limit`);
  }
  const workingFolderProviderPreferences = readStringRecord(
    record.workingFolderProviderPreferences,
    `${label}.workingFolderProviderPreferences`,
  );
  const providerIds = new Set(providers.map((provider) => provider.instanceId));
  for (const [workingFolderId, providerId] of Object.entries(workingFolderProviderPreferences)) {
    readIdentifier(workingFolderId, `${label}.workingFolderProviderPreferences workspace ID`);
    readIdentifier(providerId, `${label}.workingFolderProviderPreferences.${workingFolderId}`);
    if (!providerIds.has(providerId)) {
      throw new Error(`${label}.workingFolderProviderPreferences references an unknown provider`);
    }
  }
  return {
    providers,
    automaticProviderSetupDisabled,
    rememberedSelections,
    workingFolderProviderPreferences,
    panels: parsePanels(record.panels, `${label}.panels`),
    behavior: parseBehavior(record.behavior, `${label}.behavior`),
  };
}
