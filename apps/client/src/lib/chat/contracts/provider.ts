import type {
  CredentialReferenceId,
  JsonValue,
  ModelAvailability,
  ModelCatalogSource,
  ModelId,
  ProviderCapability,
  ProviderFamilyId,
  ProviderImplementationStatus,
  ProviderMaturity,
  ProviderInstanceId,
  ProbeState,
  ProviderAuthSource,
  ProviderVersionAdmission,
  UtcTimestamp,
  VersionedJson,
} from "./common";

export interface ProviderInstanceConfig {
  schemaVersion: number;
  instanceId: ProviderInstanceId;
  familyId: ProviderFamilyId;
  label: string;
  enabled: boolean;
  executable: string;
  providerHome: string | null;
  launchArguments: string[];
  environment: Record<string, string>;
  credentialReferences: Record<string, CredentialReferenceId>;
  visibleModelIds: ModelId[];
  favoriteModelIds: ModelId[];
  providerConfig: VersionedJson;
  [unknownField: string]: unknown;
}

export interface ProviderCapabilitySupport {
  capability: ProviderCapability;
  supported: boolean;
  explanation: string | null;
}

export interface ProviderCapabilities {
  entries: ProviderCapabilitySupport[];
}

export interface ProviderAuthoritySupport {
  isolatedConversation: boolean;
  internalHostTools: boolean;
  denyShell: boolean;
  readOnlyRoot: boolean;
  writableRoot: boolean;
  confinedCommands: boolean;
  networkBoundary: boolean;
  classifiedPublish: boolean;
  /** Every shell command the provider can run reaches Ganbaru for the person to approve. */
  personApprovedCommands: boolean;
}

/** A provider's own install instructions for its CLI. */
export interface ProviderInstallGuide {
  /** The installer command the provider documents for this platform's terminal. */
  command: string;
  /** The provider documentation page about installing the CLI, always `https`. */
  docsUrl: string;
}

export interface ProviderFamilyMetadataRead {
  familyId: ProviderFamilyId;
  displayName: string;
  configurationSchemaVersion: number;
  supportedPlatforms: string[];
  minimumTestedCliVersion: string | null;
  maximumTestedCliVersion: string | null;
  /** The command that updates the provider CLI, shown next to version notices. */
  updateCommand: string | null;
  /** How to install the provider CLI on this platform, shown when its program is missing. */
  installGuide: ProviderInstallGuide | null;
  defaultExecutableCandidates: string[];
  implementationStatus: ProviderImplementationStatus;
  maturity: ProviderMaturity;
  protocolName: string;
  potentialCapabilities: ProviderCapability[];
  unavailableReason: string | null;
}

export interface ProviderProbeResult {
  instanceId: ProviderInstanceId;
  state: ProbeState;
  version: string | null;
  negotiatedProtocolVersion: string | null;
  accountLabel: string | null;
  authSource: ProviderAuthSource | null;
  capabilities: ProviderCapabilities;
  authoritySupport: ProviderAuthoritySupport;
  checkedAt: UtcTimestamp;
  detail: string | null;
  versionAdmission: ProviderVersionAdmission | null;
  /** The oldest CLI version teammate runs on this device accept, present only when the probed version is older. */
  teammateMinimumVersion: string | null;
}

export interface ModelChoiceOption {
  value: string;
  label: string;
  description: string | null;
}

interface ModelOptionBase {
  key: string;
  label: string;
  description: string | null;
}

export type ModelOptionDefinition =
  | (ModelOptionBase & { kind: "boolean"; defaultValue: boolean | null })
  | (ModelOptionBase & { kind: "choice"; options: ModelChoiceOption[]; defaultValue: string | null })
  | (ModelOptionBase & { kind: "multiple_choice"; options: ModelChoiceOption[]; defaultValue: string[] })
  | (ModelOptionBase & { kind: "integer_range"; minimum: number; maximum: number; step: number; defaultValue: number | null })
  | (ModelOptionBase & { kind: "text"; defaultValue: string | null; allowEmpty: boolean })
  | { kind: "unknown"; key: string; label: string; rawKind: string; schemaVersion: number; data: JsonValue };

export type ModelOptionValue =
  | { kind: "boolean"; value: boolean }
  | { kind: "choice"; value: string }
  | { kind: "multiple_choice"; value: string[] }
  | { kind: "integer"; value: number }
  | { kind: "text"; value: string }
  | { kind: "unknown"; value: VersionedJson };

export interface ModelOptionSelection {
  key: string;
  value: ModelOptionValue;
}

export interface ProviderModel {
  id: ModelId;
  displayName: string;
  description: string | null;
  contextLimit: number | null;
  availability: ModelAvailability;
  capabilities: ProviderCapability[];
  options: ModelOptionDefinition[];
  custom: boolean;
}

export interface ProviderModelCatalog {
  instanceId: ProviderInstanceId;
  models: ProviderModel[];
  source: ModelCatalogSource;
  discoveredAt: UtcTimestamp;
  stale: boolean;
}
