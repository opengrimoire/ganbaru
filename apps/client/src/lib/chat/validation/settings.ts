import {
  CREDENTIAL_STORE_AVAILABILITIES,
  PROVIDER_FILE_FORMATS,
  PROVIDER_FILE_KINDS,
  PROVIDER_FILE_NOTE_KINDS,
  PROVIDER_PREFERENCE_TYPES,
  type ChatSettingsRead,
  type ProviderFileNote,
  type ProviderFileRead,
  type ProviderFilesRead,
  type ProviderPreferenceRead,
  type ProviderPreferencesRead,
  type ProviderInstanceRead,
  type ProviderRefreshResult,
  type ProviderRuntimeReviewItem,
  type ProviderSetupTestRead,
  type ProviderUsageLimitsObservation,
  type RemoveProviderResult,
} from "../contracts";
import { parseChatConfiguration } from "./config";
import { parseRateLimitStatus } from "./events";
import {
  parseProviderFamilyMetadata,
  parseProviderInstanceConfig,
  parseProviderModelCatalog,
  parseProviderProbeResult,
} from "./provider";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readUtcTimestamp,
  readString,
  readStringArray,
} from "./readers";

export function parseProviderInstanceRead(value: unknown, label = "provider instance"): ProviderInstanceRead {
  const record = readRecord(value, label);
  const configuration = parseProviderInstanceConfig(record.configuration, `${label}.configuration`);
  const modelCatalog = readNullable(record.modelCatalog, `${label}.modelCatalog`, parseProviderModelCatalog);
  return {
    configuration,
    runtimeReview: record.runtimeReview === undefined
      ? []
      : readArray(record.runtimeReview, `${label}.runtimeReview`, parseProviderRuntimeReviewItem),
    lastProbe: readNullable(record.lastProbe, `${label}.lastProbe`, parseProviderProbeResult),
    lastSuccessfulProbeAt: readNullable(record.lastSuccessfulProbeAt, `${label}.lastSuccessfulProbeAt`, readUtcTimestamp),
    modelCatalog,
    usageLimits: record.usageLimits === undefined
      ? null
      : readNullable(record.usageLimits, `${label}.usageLimits`, parseProviderUsageLimitsObservation),
    lastUsedAt: record.lastUsedAt === undefined
      ? null
      : readNullable(record.lastUsedAt, `${label}.lastUsedAt`, readUtcTimestamp),
    duplicateOf: record.duplicateOf === undefined
      ? null
      : readNullable(record.duplicateOf, `${label}.duplicateOf`, readIdentifier),
  };
}

/** Reads plan limits a provider reported, with when they were observed. */
export function parseProviderUsageLimitsObservation(value: unknown, label: string): ProviderUsageLimitsObservation {
  const record = readRecord(value, label);
  return {
    observedAt: readUtcTimestamp(record.observedAt, `${label}.observedAt`),
    usageReadAt: record.usageReadAt === undefined
      ? null
      : readNullable(record.usageReadAt, `${label}.usageReadAt`, readUtcTimestamp),
    status: parseRateLimitStatus(record.status, `${label}.status`),
  };
}

function parseProviderRuntimeReviewItem(value: unknown, label: string): ProviderRuntimeReviewItem {
  const record = readRecord(value, label);
  return {
    label: readString(record.label, `${label}.label`),
    widensPermissions: readBoolean(record.widensPermissions, `${label}.widensPermissions`),
  };
}

/** Reads the CLI programs found for each family, keyed by family id. */
function parseInstalledExecutables(value: unknown, label: string): Record<string, string[]> {
  const record = readRecord(value, label);
  const executables: Record<string, string[]> = {};
  for (const [familyId, paths] of Object.entries(record)) {
    executables[readIdentifier(familyId, `${label} key`)] = readStringArray(paths, `${label}.${familyId}`);
  }
  return executables;
}

export function parseChatSettingsRead(value: unknown): ChatSettingsRead {
  const record = readRecord(value, "Chat settings");
  return {
    configuration: parseChatConfiguration(record.configuration, "Chat settings.configuration"),
    providerFamilies: readArray(record.providerFamilies, "Chat settings.providerFamilies", parseProviderFamilyMetadata),
    providerInstances: readArray(record.providerInstances, "Chat settings.providerInstances", parseProviderInstanceRead),
    installedExecutables: parseInstalledExecutables(record.installedExecutables, "Chat settings.installedExecutables"),
    credentialStoreAvailability: readEnum(
      record.credentialStoreAvailability,
      CREDENTIAL_STORE_AVAILABILITIES,
      "Chat settings.credentialStoreAvailability",
    ),
    lastSelectedThreadId: readNullable(
      record.lastSelectedThreadId,
      "Chat settings.lastSelectedThreadId",
      readIdentifier,
    ),
  };
}

export function parseRemoveProviderResult(value: unknown): RemoveProviderResult {
  const record = readRecord(value, "remove provider result");
  return {
    removed: readBoolean(record.removed, "remove provider result.removed"),
    credentialCleanupFailed: readBoolean(
      record.credentialCleanupFailed,
      "remove provider result.credentialCleanupFailed",
    ),
  };
}

export function parseProviderSetupTestRead(value: unknown): ProviderSetupTestRead {
  const record = readRecord(value, "provider setup test");
  return {
    probe: parseProviderProbeResult(record.probe, "provider setup test.probe"),
    modelCatalog: readNullable(
      record.modelCatalog,
      "provider setup test.modelCatalog",
      parseProviderModelCatalog,
    ),
  };
}

export function parseProviderRefreshResult(value: unknown): ProviderRefreshResult {
  const record = readRecord(value, "provider refresh result");
  return {
    familiesScanned: readNonNegativeSafeInteger(
      record.familiesScanned,
      "provider refresh result.familiesScanned",
    ),
    providersChecked: readNonNegativeSafeInteger(
      record.providersChecked,
      "provider refresh result.providersChecked",
    ),
    providersDiscovered: readNonNegativeSafeInteger(
      record.providersDiscovered,
      "provider refresh result.providersDiscovered",
    ),
    issues: readNonNegativeSafeInteger(record.issues, "provider refresh result.issues"),
  };
}

function parseProviderPreferenceRead(value: unknown, label: string): ProviderPreferenceRead {
  const record = readRecord(value, label);
  const valueType = readEnum(record.valueType, PROVIDER_PREFERENCE_TYPES, `${label}.valueType`);
  const options = readArray(record.options, `${label}.options`, readString);
  if (valueType === "choice" && options.length === 0) {
    throw new Error(`${label}.options must list the choices`);
  }
  const readValue = (raw: unknown, valueLabel: string): boolean | string => {
    if (valueType === "switch" || valueType === "boolean") return readBoolean(raw, valueLabel);
    const text = readString(raw, valueLabel);
    if (valueType === "choice" && !options.includes(text)) {
      throw new Error(`${valueLabel} must be one of the options`);
    }
    return text;
  };
  return {
    id: readString(record.id, `${label}.id`),
    valueType,
    variable: readBoolean(record.variable, `${label}.variable`),
    value: readNullable(record.value, `${label}.value`, readValue),
    ignored: readBoolean(record.ignored, `${label}.ignored`),
    maxBytes: readNullable(record.maxBytes, `${label}.maxBytes`, readNonNegativeSafeInteger),
    multiline: readBoolean(record.multiline, `${label}.multiline`),
    emptyAllowed: readBoolean(record.emptyAllowed, `${label}.emptyAllowed`),
    options,
  };
}

function parseProviderPreferencesRead(value: unknown, label: string): ProviderPreferencesRead {
  const record = readRecord(value, label);
  return {
    editable: readBoolean(record.editable, `${label}.editable`),
    entries: readArray(record.entries, `${label}.entries`, parseProviderPreferenceRead),
  };
}
export function parseProviderFileRead(value: unknown, label = "provider file"): ProviderFileRead {
  const record = readRecord(value, label);
  return {
    fileId: readString(record.fileId, `${label}.fileId`),
    name: readString(record.name, `${label}.name`),
    kind: readEnum(record.kind, PROVIDER_FILE_KINDS, `${label}.kind`),
    format: readEnum(record.format, PROVIDER_FILE_FORMATS, `${label}.format`),
    path: readString(record.path, `${label}.path`),
    exists: readBoolean(record.exists, `${label}.exists`),
    contents: readString(record.contents, `${label}.contents`),
    revision: readString(record.revision, `${label}.revision`),
    preferences: readNullable(record.preferences, `${label}.preferences`, parseProviderPreferencesRead),
  };
}

function parseProviderFileNote(value: unknown, label: string): ProviderFileNote {
  const record = readRecord(value, label);
  return {
    kind: readEnum(record.kind, PROVIDER_FILE_NOTE_KINDS, `${label}.kind`),
    paths: readArray(record.paths, `${label}.paths`, readString),
  };
}

export function parseProviderFiles(value: unknown): ProviderFilesRead {
  const record = readRecord(value, "provider files");
  return {
    files: readArray(record.files, "provider files.files", parseProviderFileRead),
    notes: readArray(record.notes, "provider files.notes", parseProviderFileNote),
  };
}
