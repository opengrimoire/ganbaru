import {
  MODEL_AVAILABILITIES,
  MODEL_CATALOG_SOURCES,
  PROVIDER_CAPABILITIES,
  PROVIDER_IMPLEMENTATION_STATUSES,
  PROVIDER_MATURITIES,
  PROBE_STATES,
  PROVIDER_AUTH_SOURCES,
  PROVIDER_VERSION_ADMISSIONS,
  type ModelChoiceOption,
  type ModelOptionDefinition,
  type ModelOptionSelection,
  type ModelOptionValue,
  type ProviderCapabilities,
  type ProviderAuthoritySupport,
  type ProviderCapabilitySupport,
  type ProviderFamilyMetadataRead,
  type ProviderInstallGuide,
  type ProviderInstanceConfig,
  type ProviderModel,
  type ProviderModelCatalog,
  type ProviderProbeResult,
  type ProviderUpdateRead,
  type ProviderUpdateRunRead,
} from "../contracts";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readJsonValue,
  readNullable,
  readNonNegativeSafeInteger,
  readRecord,
  readSafeInteger,
  readString,
  readStringArray,
  readStringRecord,
  readUtcTimestamp,
  readVersionedJson,
} from "./readers";

export function parseProviderCapabilitySupport(value: unknown, label = "provider capability"): ProviderCapabilitySupport {
  const record = readRecord(value, label);
  return {
    capability: readEnum(record.capability, PROVIDER_CAPABILITIES, `${label}.capability`),
    supported: readBoolean(record.supported, `${label}.supported`),
    explanation: readNullable(record.explanation, `${label}.explanation`, readString),
  };
}

export function parseProviderCapabilities(value: unknown, label = "provider capabilities"): ProviderCapabilities {
  const record = readRecord(value, label);
  return { entries: readArray(record.entries, `${label}.entries`, parseProviderCapabilitySupport) };
}

export function parseProviderAuthoritySupport(
  value: unknown,
  label = "provider authority support",
): ProviderAuthoritySupport {
  const record = readRecord(value, label);
  return {
    isolatedConversation: readBoolean(record.isolatedConversation, `${label}.isolatedConversation`),
    internalHostTools: readBoolean(record.internalHostTools, `${label}.internalHostTools`),
    denyShell: readBoolean(record.denyShell, `${label}.denyShell`),
    readOnlyRoot: readBoolean(record.readOnlyRoot, `${label}.readOnlyRoot`),
    writableRoot: readBoolean(record.writableRoot, `${label}.writableRoot`),
    confinedCommands: readBoolean(record.confinedCommands, `${label}.confinedCommands`),
    networkBoundary: readBoolean(record.networkBoundary, `${label}.networkBoundary`),
    classifiedPublish: readBoolean(record.classifiedPublish, `${label}.classifiedPublish`),
    personApprovedCommands: readBoolean(record.personApprovedCommands, `${label}.personApprovedCommands`),
  };
}

/** Reads an install guide, refusing a documentation link that is not `https`. */
function parseProviderInstallGuide(value: unknown, label: string): ProviderInstallGuide {
  const record = readRecord(value, label);
  const docsUrl = readString(record.docsUrl, `${label}.docsUrl`);
  if (!docsUrl.startsWith("https://")) throw new Error(`${label}.docsUrl must use https`);
  return { command: readString(record.command, `${label}.command`), docsUrl };
}

export function parseProviderFamilyMetadata(value: unknown, label = "provider metadata"): ProviderFamilyMetadataRead {
  const record = readRecord(value, label);
  return {
    familyId: readIdentifier(record.familyId, `${label}.familyId`),
    displayName: readString(record.displayName, `${label}.displayName`),
    configurationSchemaVersion: readNonNegativeSafeInteger(record.configurationSchemaVersion, `${label}.configurationSchemaVersion`),
    supportedPlatforms: readStringArray(record.supportedPlatforms, `${label}.supportedPlatforms`),
    minimumTestedCliVersion: readNullable(record.minimumTestedCliVersion, `${label}.minimumTestedCliVersion`, readString),
    maximumTestedCliVersion: readNullable(record.maximumTestedCliVersion, `${label}.maximumTestedCliVersion`, readString),
    updateCommand: readNullable(record.updateCommand, `${label}.updateCommand`, readString),
    installGuide: readNullable(record.installGuide, `${label}.installGuide`, parseProviderInstallGuide),
    defaultExecutableCandidates: readStringArray(record.defaultExecutableCandidates, `${label}.defaultExecutableCandidates`),
    implementationStatus: readEnum(record.implementationStatus, PROVIDER_IMPLEMENTATION_STATUSES, `${label}.implementationStatus`),
    maturity: readEnum(record.maturity, PROVIDER_MATURITIES, `${label}.maturity`),
    protocolName: readString(record.protocolName, `${label}.protocolName`),
    potentialCapabilities: readArray(
      record.potentialCapabilities,
      `${label}.potentialCapabilities`,
      (entry, entryLabel) => readEnum(entry, PROVIDER_CAPABILITIES, entryLabel),
    ),
    unavailableReason: readNullable(record.unavailableReason, `${label}.unavailableReason`, readString),
  };
}

export function parseProviderInstanceConfig(value: unknown, label = "provider instance"): ProviderInstanceConfig {
  const record = readRecord(value, label);
  return {
    ...record,
    schemaVersion: readNonNegativeSafeInteger(record.schemaVersion, `${label}.schemaVersion`),
    instanceId: readIdentifier(record.instanceId, `${label}.instanceId`),
    familyId: readIdentifier(record.familyId, `${label}.familyId`),
    label: readString(record.label, `${label}.label`),
    enabled: readBoolean(record.enabled, `${label}.enabled`),
    executable: readString(record.executable, `${label}.executable`),
    providerHome: readNullable(record.providerHome, `${label}.providerHome`, readString),
    launchArguments: readStringArray(record.launchArguments, `${label}.launchArguments`),
    environment: readStringRecord(record.environment, `${label}.environment`),
    credentialReferences: readStringRecord(record.credentialReferences, `${label}.credentialReferences`),
    visibleModelIds: readArray(record.visibleModelIds, `${label}.visibleModelIds`, readIdentifier),
    favoriteModelIds: readArray(record.favoriteModelIds, `${label}.favoriteModelIds`, readIdentifier),
    providerConfig: readVersionedJson(record.providerConfig, `${label}.providerConfig`),
  };
}

export function parseProviderProbeResult(value: unknown, label = "provider probe"): ProviderProbeResult {
  const record = readRecord(value, label);
  return {
    instanceId: readIdentifier(record.instanceId, `${label}.instanceId`),
    state: readEnum(record.state, PROBE_STATES, `${label}.state`),
    version: readNullable(record.version, `${label}.version`, readString),
    negotiatedProtocolVersion: readNullable(record.negotiatedProtocolVersion, `${label}.negotiatedProtocolVersion`, readString),
    accountLabel: readNullable(record.accountLabel, `${label}.accountLabel`, readString),
    authSource: readNullable(record.authSource, `${label}.authSource`, (entry, entryLabel) =>
      readEnum(entry, PROVIDER_AUTH_SOURCES, entryLabel),
    ),
    capabilities: parseProviderCapabilities(record.capabilities, `${label}.capabilities`),
    authoritySupport: parseProviderAuthoritySupport(
      record.authoritySupport,
      `${label}.authoritySupport`,
    ),
    checkedAt: readUtcTimestamp(record.checkedAt, `${label}.checkedAt`),
    detail: readNullable(record.detail, `${label}.detail`, readString),
    versionAdmission: readNullable(record.versionAdmission, `${label}.versionAdmission`, (entry, entryLabel) =>
      readEnum(entry, PROVIDER_VERSION_ADMISSIONS, entryLabel),
    ),
    teammateMinimumVersion: readNullable(record.teammateMinimumVersion, `${label}.teammateMinimumVersion`, readString),
  };
}

export function parseProviderUpdateRead(value: unknown, label = "provider update"): ProviderUpdateRead {
  const record = readRecord(value, label);
  return {
    instanceId: readIdentifier(record.instanceId, `${label}.instanceId`),
    currentVersion: readString(record.currentVersion, `${label}.currentVersion`),
    latestVersion: readString(record.latestVersion, `${label}.latestVersion`),
    updateCommand: readString(record.updateCommand, `${label}.updateCommand`),
    runsInApp: readBoolean(record.runsInApp, `${label}.runsInApp`),
  };
}

export function parseProviderUpdates(value: unknown, label = "provider updates"): ProviderUpdateRead[] {
  return readArray(value, label, parseProviderUpdateRead);
}

export function parseProviderUpdateRunRead(value: unknown, label = "provider update run"): ProviderUpdateRunRead {
  const record = readRecord(value, label);
  return {
    successful: readBoolean(record.successful, `${label}.successful`),
    output: readNullable(record.output, `${label}.output`, readString),
  };
}

function parseModelChoiceOption(value: unknown, label: string): ModelChoiceOption {
  const record = readRecord(value, label);
  return {
    value: readString(record.value, `${label}.value`),
    label: readString(record.label, `${label}.label`),
    description: readNullable(record.description, `${label}.description`, readString),
  };
}

export function parseModelOptionDefinition(value: unknown, label = "model option"): ModelOptionDefinition {
  const record = readRecord(value, label);
  const kind = readString(record.kind, `${label}.kind`);
  const key = readString(record.key, `${label}.key`);
  const optionLabel = readString(record.label, `${label}.label`);
  if (kind === "unknown") {
    return {
      kind,
      key,
      label: optionLabel,
      rawKind: readString(record.rawKind, `${label}.rawKind`),
      schemaVersion: readNonNegativeSafeInteger(record.schemaVersion, `${label}.schemaVersion`),
      data: readJsonValue(record.data, `${label}.data`),
    };
  }
  const description = readNullable(record.description, `${label}.description`, readString);
  if (kind === "boolean") {
    return { kind, key, label: optionLabel, description, defaultValue: readNullable(record.defaultValue, `${label}.defaultValue`, readBoolean) };
  }
  if (kind === "choice") {
    return {
      kind,
      key,
      label: optionLabel,
      description,
      options: readArray(record.options, `${label}.options`, parseModelChoiceOption),
      defaultValue: readNullable(record.defaultValue, `${label}.defaultValue`, readString),
    };
  }
  if (kind === "multiple_choice") {
    return {
      kind,
      key,
      label: optionLabel,
      description,
      options: readArray(record.options, `${label}.options`, parseModelChoiceOption),
      defaultValue: readStringArray(record.defaultValue, `${label}.defaultValue`),
    };
  }
  if (kind === "integer_range") {
    return {
      kind,
      key,
      label: optionLabel,
      description,
      minimum: readSafeInteger(record.minimum, `${label}.minimum`),
      maximum: readSafeInteger(record.maximum, `${label}.maximum`),
      step: readSafeInteger(record.step, `${label}.step`),
      defaultValue: readNullable(record.defaultValue, `${label}.defaultValue`, readSafeInteger),
    };
  }
  if (kind === "text") {
    return {
      kind,
      key,
      label: optionLabel,
      description,
      defaultValue: readNullable(record.defaultValue, `${label}.defaultValue`, readString),
      allowEmpty: readBoolean(record.allowEmpty, `${label}.allowEmpty`),
    };
  }
  throw new Error(`${label}.kind has an unsupported value`);
}

export function parseModelOptionValue(value: unknown, label = "model option value"): ModelOptionValue {
  const record = readRecord(value, label);
  const kind = readString(record.kind, `${label}.kind`);
  if (kind === "boolean") return { kind, value: readBoolean(record.value, `${label}.value`) };
  if (kind === "choice" || kind === "text") return { kind, value: readString(record.value, `${label}.value`) };
  if (kind === "multiple_choice") return { kind, value: readStringArray(record.value, `${label}.value`) };
  if (kind === "integer") return { kind, value: readSafeInteger(record.value, `${label}.value`) };
  if (kind === "unknown") return { kind, value: readVersionedJson(record.value, `${label}.value`) };
  throw new Error(`${label}.kind has an unsupported value`);
}

export function parseModelOptionSelection(value: unknown, label = "model option selection"): ModelOptionSelection {
  const record = readRecord(value, label);
  return {
    key: readString(record.key, `${label}.key`),
    value: parseModelOptionValue(record.value, `${label}.value`),
  };
}

export function parseProviderModel(value: unknown, label = "provider model"): ProviderModel {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    displayName: readString(record.displayName, `${label}.displayName`),
    description: readNullable(record.description, `${label}.description`, readString),
    contextLimit: readNullable(record.contextLimit, `${label}.contextLimit`, readNonNegativeSafeInteger),
    availability: readEnum(record.availability, MODEL_AVAILABILITIES, `${label}.availability`),
    capabilities: readArray(
      record.capabilities,
      `${label}.capabilities`,
      (entry, entryLabel) => readEnum(entry, PROVIDER_CAPABILITIES, entryLabel),
    ),
    options: readArray(record.options, `${label}.options`, parseModelOptionDefinition),
    custom: readBoolean(record.custom, `${label}.custom`),
  };
}

export function parseProviderModelCatalog(value: unknown, label = "provider model catalog"): ProviderModelCatalog {
  const record = readRecord(value, label);
  const models = readArray(record.models, `${label}.models`, parseProviderModel)
    .filter((model) => model.availability !== "deprecated");
  return {
    instanceId: readIdentifier(record.instanceId, `${label}.instanceId`),
    models,
    source: readEnum(record.source, MODEL_CATALOG_SOURCES, `${label}.source`),
    discoveredAt: readUtcTimestamp(record.discoveredAt, `${label}.discoveredAt`),
    stale: readBoolean(record.stale, `${label}.stale`),
  };
}
