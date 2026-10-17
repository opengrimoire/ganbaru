import type {
  ChatPromptCatalogEntry,
  ChatThreadId,
  ProjectWorkingFolderId,
  ProviderFamilyId,
  ProviderInstanceId,
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
