import type {
  ProviderFamilyId,
  ProviderInstanceConfig,
  ProviderInstanceId,
  VersionedJson,
} from "./contracts";

export type ProviderEnvironmentValueType = "text" | "secret" | "inherit";

export interface ProviderEnvironmentDraft {
  key: string;
  name: string;
  valueType: ProviderEnvironmentValueType;
  value: string;
  credentialReference: string;
}

export interface ProviderSetupDraft {
  familyId: string;
  label: string;
  instanceId: string;
  executable: string;
  providerHome: string;
  launchArguments: string[];
  environment: ProviderEnvironmentDraft[];
  providerConfig: VersionedJson;
}

export interface ProviderSetupValidation {
  fields: Readonly<Record<string, string>>;
  valid: boolean;
}

const INSTANCE_ID_PATTERN = /^[a-z0-9]+(?:[._-][a-z0-9]+)*$/;
const ENVIRONMENT_NAME_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*$/;
const MAX_ARGUMENTS = 64;
const DEDICATED_HOME_VARIABLES: Readonly<Record<string, string>> = {
  codex: "CODEX_HOME",
  claude: "CLAUDE_CONFIG_DIR",
  grok: "GROK_HOME",
  opencode: "OPENCODE_CONFIG_DIR",
};

export function createProviderSetupDraft(): ProviderSetupDraft {
  return {
    familyId: "",
    label: "",
    instanceId: "",
    executable: "",
    providerHome: "",
    launchArguments: [],
    environment: [],
    providerConfig: { schemaVersion: 1, value: {} },
  };
}

export function providerInstanceIdFromLabel(label: string): string {
  return label
    .trim()
    .toLowerCase()
    .normalize("NFKD")
    .replace(/\p{Diacritic}/gu, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 80);
}

export function validateProviderSetup(
  draft: ProviderSetupDraft,
  existingInstanceIds: ReadonlySet<string>,
  editingInstanceId: string | null = null,
): ProviderSetupValidation {
  const fields: Record<string, string> = {};
  if (!draft.familyId) fields.familyId = "Choose a provider family.";
  if (!draft.label.trim()) fields.label = "Enter an instance label.";
  if (!INSTANCE_ID_PATTERN.test(draft.instanceId)) {
    fields.instanceId = "Use lowercase letters, numbers, dots, underscores, or hyphens.";
  } else if (existingInstanceIds.has(draft.instanceId) && draft.instanceId !== editingInstanceId) {
    fields.instanceId = "This provider instance ID is already in use.";
  }
  if (!draft.executable.trim()) fields.executable = "Enter or select the provider executable.";
  if (draft.launchArguments.length > MAX_ARGUMENTS) {
    fields.launchArguments = `Use no more than ${MAX_ARGUMENTS} launch arguments.`;
  }
  draft.launchArguments.forEach((argument, index) => {
    if (argument.includes("\0")) fields[`launchArguments.${index}`] = "Arguments cannot contain null characters.";
  });

  const names = new Set<string>();
  const dedicatedVariable = DEDICATED_HOME_VARIABLES[draft.familyId];
  for (const row of draft.environment) {
    const field = `environment.${row.key}`;
    if (!ENVIRONMENT_NAME_PATTERN.test(row.name)) {
      fields[`${field}.name`] = "Enter a valid environment variable name.";
    } else if (names.has(row.name)) {
      fields[`${field}.name`] = "Environment variable names must be unique.";
    } else {
      names.add(row.name);
    }
    if (
      dedicatedVariable === row.name
      && (draft.providerHome.trim() || draft.familyId === "codex" || draft.familyId === "claude")
    ) {
      fields[`${field}.name`] = `${dedicatedVariable} must be configured with the dedicated home field.`;
    }
    if (row.valueType === "text" && row.value.length === 0) {
      fields[`${field}.value`] = "Enter a value or remove this row.";
    }
    if (row.valueType === "secret" && !row.credentialReference) {
      fields[`${field}.value`] = "Store a secret value before saving.";
    }
  }

  validateProviderSpecificFields(draft, fields);
  return { fields, valid: Object.keys(fields).length === 0 };
}

export function providerConfigurationFromDraft(
  draft: ProviderSetupDraft,
  current?: ProviderInstanceConfig,
): ProviderInstanceConfig {
  const environment: Record<string, string> = {};
  const credentialReferences: Record<string, string> = {};
  for (const row of draft.environment) {
    if (row.valueType === "text") environment[row.name] = row.value;
    if (row.valueType === "inherit") environment[row.name] = `inherit:${row.name}`;
    if (row.valueType === "secret") credentialReferences[row.name] = row.credentialReference;
  }
  return {
    schemaVersion: 1,
    instanceId: draft.instanceId as ProviderInstanceId,
    familyId: draft.familyId as ProviderFamilyId,
    label: draft.label.trim(),
    enabled: current?.enabled ?? true,
    executable: draft.executable.trim(),
    providerHome: draft.providerHome.trim() || null,
    launchArguments: [...draft.launchArguments],
    environment,
    credentialReferences,
    visibleModelIds: [...(current?.visibleModelIds ?? [])],
    favoriteModelIds: [...(current?.favoriteModelIds ?? [])],
    providerConfig: draft.providerConfig,
  };
}

function validateProviderSpecificFields(
  draft: ProviderSetupDraft,
  fields: Record<string, string>,
): void {
  const config = draft.providerConfig.value;
  if (typeof config !== "object" || config === null || Array.isArray(config)) {
    fields.providerConfig = "Provider connection settings must be an object.";
    return;
  }
  if (draft.familyId === "cursor") {
    const endpoint = config.endpoint;
    if (endpoint !== undefined && endpoint !== null && (typeof endpoint !== "string" || !isSafeHttpEndpoint(endpoint))) {
      fields["providerConfig.endpoint"] = "Enter an HTTPS endpoint or a loopback HTTP endpoint.";
    }
  }
  if (draft.familyId === "opencode") {
    const mode = config.mode;
    if (mode !== "local" && mode !== "external") {
      fields["providerConfig.mode"] = "Choose local or external mode.";
    }
    if (mode === "external") {
      const serverUrl = config.serverUrl;
      const origin = typeof serverUrl === "string" ? parseHttpOrigin(serverUrl) : null;
      if (!origin) {
        fields["providerConfig.serverUrl"] = "Enter an HTTP or HTTPS server origin.";
      } else if (!origin.secure && !origin.loopback && config.allowInsecureExternalHttp !== true) {
        fields["providerConfig.allowInsecureExternalHttp"] = "Confirm the insecure external HTTP connection.";
      }
      if (config.confirmExternalWorkspaceAccess !== true) {
        fields["providerConfig.confirmExternalWorkspaceAccess"] = "Confirm external workspace access.";
      }
    }
  }
}

function isSafeHttpEndpoint(value: string): boolean {
  const origin = parseHttpOrigin(value);
  return origin !== null && (origin.secure || origin.loopback);
}

function parseHttpOrigin(value: string): { secure: boolean; loopback: boolean } | null {
  try {
    const url = new URL(value);
    if (
      !["http:", "https:"].includes(url.protocol)
      || url.username
      || url.password
      || url.search
      || url.hash
      || !["", "/"].includes(url.pathname)
    ) {
      return null;
    }
    const hostname = url.hostname.toLowerCase();
    const loopback = hostname === "localhost" || hostname === "[::1]" || /^127(?:\.\d{1,3}){3}$/.test(hostname);
    return { secure: url.protocol === "https:", loopback };
  } catch {
    return null;
  }
}
