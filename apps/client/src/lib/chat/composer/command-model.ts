import type {
  ChatPromptCatalogEntry,
  ChatThreadId,
  ProjectWorkingFolderId,
  ProviderFamilyId,
  ProviderInstanceId,
  ProviderModel,
  ProviderSessionState,
} from "$lib/chat/contracts";
import { filterPromptCatalog } from "./model";

export type ProviderDirectAction = "compact" | "mcp";

const APP_COMMAND_VALUES = new Set([
  "/plan",
  "/build",
  "/model",
  "/permissions",
  "/status",
  "/changes",
]);

export function isAppComposerCommand(value: string): boolean {
  return APP_COMMAND_VALUES.has(value.trim().toLowerCase());
}

export function providerDirectAction(
  entry: ChatPromptCatalogEntry,
  familyId: ProviderFamilyId | null,
): ProviderDirectAction | null {
  if (entry.source !== "provider") return null;
  return typedProviderDirectAction(entry.value, familyId);
}

export function typedProviderDirectAction(
  value: string,
  familyId: ProviderFamilyId | null,
): ProviderDirectAction | null {
  const command = value.trim().toLowerCase();
  if (command === "/compact" && (familyId === "codex" || familyId === "claude")) {
    return "compact";
  }
  return command === "/mcp" && familyId === "codex" ? "mcp" : null;
}

export function providerCommandNeedsInput(
  entry: ChatPromptCatalogEntry,
  familyId: ProviderFamilyId | null,
): boolean {
  const command = entry.value.toLowerCase();
  if (entry.source === "provider"
    && familyId === "codex"
    && ["/compact", "/review", "/mcp"].includes(command)) {
    return false;
  }
  if (entry.source === "provider"
    && familyId === "claude"
    && ["/compact", "/clear", "/context", "/usage"].includes(command)) {
    return false;
  }
  return (entry.source === "provider" && familyId === "codex" && command === "/goal")
    || entry.argumentHint !== null;
}

export function mergePromptCatalog(
  appCatalog: readonly ChatPromptCatalogEntry[],
  providerCatalog: readonly ChatPromptCatalogEntry[],
  kind: "skill" | "command",
  query: string,
): ChatPromptCatalogEntry[] {
  const appValues = new Set(appCatalog.map((entry) => entry.value.toLowerCase()));
  return filterPromptCatalog([
    ...appCatalog,
    ...providerCatalog.filter((entry) => !appValues.has(entry.value.toLowerCase())),
  ], kind, query);
}

export function promptEntryDisplayLabel(entry: ChatPromptCatalogEntry): string {
  if (entry.kind !== "command") return entry.label;
  const commandName = entry.value.replace(/^\//, "");
  if (entry.label.toLowerCase() !== commandName.toLowerCase()) return entry.label;
  return `${entry.label.charAt(0).toUpperCase()}${entry.label.slice(1)}`;
}

export function promptCatalogKey(input: {
  workingFolderId: ProjectWorkingFolderId | null;
  providerInstanceId: ProviderInstanceId | null;
  threadId: ChatThreadId | null;
  sessionState: ProviderSessionState;
}): string {
  return [
    input.workingFolderId ?? "",
    input.providerInstanceId ?? "",
    input.threadId ?? "",
    input.sessionState,
  ].join(":");
}

export function providerCommandBoundaryKey(input: {
  workingFolderId: ProjectWorkingFolderId | null;
  providerInstanceId: ProviderInstanceId | null;
  threadId: ChatThreadId | null;
}): string {
  return [
    input.workingFolderId ?? "",
    input.providerInstanceId ?? "",
    input.threadId ?? "",
  ].join(":");
}

/** The word that starts a model choice in a channel message; the backend reads the first one. */
export const MODEL_COMMAND = "/model";

/**
 * One `/model <id>` suggestion per model the agent's provider offers, so the model choice is discoverable
 * from the slash menu. Choosing one inserts the token into the draft instead of running anything.
 */
export function modelCommandEntries(
  models: readonly Pick<ProviderModel, "id" | "displayName">[],
  describe: (displayName: string) => string,
): ChatPromptCatalogEntry[] {
  return models.map((model) => ({
    value: `${MODEL_COMMAND} ${model.id}`,
    label: `${MODEL_COMMAND} ${model.id}`,
    description: describe(model.displayName),
    argumentHint: null,
    kind: "command",
    source: "app",
    stale: false,
  }));
}

/** Whether a menu entry is a model choice, which goes into the draft rather than running as an app command. */
export function isModelCommandEntry(entry: ChatPromptCatalogEntry): boolean {
  return entry.source === "app" && (entry.value === MODEL_COMMAND || entry.value.startsWith(`${MODEL_COMMAND} `));
}

/**
 * The bare `/model` suggestion shown when no model list is known here, so the command stays discoverable; choosing it
 * inserts `/model ` for the person to finish.
 */
export function genericModelCommandEntry(label: string, description: string, argumentHint: string): ChatPromptCatalogEntry {
  return { value: MODEL_COMMAND, label, description, argumentHint, kind: "command", source: "app", stale: false };
}

/**
 * The models a channel post may name before it mentions an agent: every model the providers of the channel's agents
 * offer on this device, first occurrence first, without repeats.
 */
export function channelModelChoices<Model extends Pick<ProviderModel, "id" | "displayName">>(
  catalogs: readonly (readonly Model[])[],
): Model[] {
  const seen = new Set<string>();
  return catalogs.flat().filter((model) => {
    if (seen.has(model.id)) return false;
    seen.add(model.id);
    return true;
  });
}
