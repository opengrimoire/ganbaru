import type { ChatVaultConfig } from "./config";
import type { ChatThreadId } from "./common";
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
}

export interface ChatSettingsRead {
  configuration: ChatVaultConfig;
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
