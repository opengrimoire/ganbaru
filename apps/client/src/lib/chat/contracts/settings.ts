import type { ChatConfiguration } from "./config";
import type { ChatThreadId, UtcTimestamp } from "./common";
import type { RateLimitStatusEvent } from "./events";
import type {
  ProviderFamilyMetadataRead,
  ProviderInstanceConfig,
  ProviderModelCatalog,
  ProviderProbeResult,
} from "./provider";

export const CREDENTIAL_STORE_AVAILABILITIES = ["available", "unavailable"] as const;
export type CredentialStoreAvailability = (typeof CREDENTIAL_STORE_AVAILABILITIES)[number];

/** A portable runtime value this device has not confirmed yet. */
export interface ProviderRuntimeReviewItem {
  label: string;
  widensPermissions: boolean;
}

export interface ProviderInstanceRead {
  configuration: ProviderInstanceConfig;
  /** Values that keep the provider from running until this device confirms them. */
  runtimeReview: ProviderRuntimeReviewItem[];
  lastProbe: ProviderProbeResult | null;
  lastSuccessfulProbeAt: string | null;
  modelCatalog: ProviderModelCatalog | null;
  /** The newest usage limits this device saw the provider report, from a probe or a turn. */
  usageLimits: ProviderUsageLimitsObservation | null;
  /** When this device last started a turn with the provider. */
  lastUsedAt: UtcTimestamp | null;
}

/** Usage limits a provider reported, with when this device observed them. */
export interface ProviderUsageLimitsObservation {
  observedAt: UtcTimestamp;
  status: RateLimitStatusEvent;
}

export interface ChatSettingsRead {
  configuration: ChatConfiguration;
  providerFamilies: ProviderFamilyMetadataRead[];
  providerInstances: ProviderInstanceRead[];
  credentialStoreAvailability: CredentialStoreAvailability;
  lastSelectedThreadId: ChatThreadId | null;
}

export interface RemoveProviderResult {
  removed: boolean;
  credentialCleanupFailed: boolean;
}

export interface ProviderSetupTestRead {
  probe: ProviderProbeResult;
  modelCatalog: ProviderModelCatalog | null;
}

export interface ProviderRefreshResult {
  familiesScanned: number;
  providersChecked: number;
  providersDiscovered: number;
  issues: number;
}

export const PROVIDER_FILE_KINDS = ["configuration", "instructions"] as const;
export type ProviderFileKind = (typeof PROVIDER_FILE_KINDS)[number];

export const PROVIDER_FILE_FORMATS = ["toml", "json", "jsonc", "markdown"] as const;
export type ProviderFileFormat = (typeof PROVIDER_FILE_FORMATS)[number];

export interface ProviderFileRead {
  fileId: string;
  name: string;
  kind: ProviderFileKind;
  format: ProviderFileFormat;
  path: string;
  exists: boolean;
  contents: string;
  revision: string;
}
