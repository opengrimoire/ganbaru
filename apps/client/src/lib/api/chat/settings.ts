import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type {
  ChatBehaviorPreferences,
  ChatDiagnosticPreferences,
  ChatDiagnosticsRead,
  ChatStopAllResult,
  ChatRebuildResult,
  ChatPanelPreferences,
  ChatSettingsRead,
  ChatThreadId,
  ProjectWorkingFolderId,
  CredentialReferenceId,
  ModelId,
  ProviderInstanceConfig,
  ProviderInstanceId,
  ProviderInstanceRead,
  ProviderModelCatalog,
  ProviderProbeResult,
  ProviderRefreshResult,
  ProviderFileRead,
  ProviderSetupTestRead,
  RemoveProviderResult,
  RememberedComposerSelection,
} from "$lib/chat/contracts";
import {
  parseChatDiagnosticPreferences,
  parseChatDiagnosticsRead,
  parseChatStopAllResult,
  parseChatRebuildResult,
  parseChatSettingsRead,
  parseChatVaultConfig,
  parseProviderInstanceRead,
  parseProviderModelCatalog,
  parseProviderProbeResult,
  parseProviderRefreshResult,
  parseProviderFileRead,
  parseProviderFiles,
  parseProviderSetupTestRead,
  parseRemoveProviderResult,
} from "$lib/chat/validation";

export async function readChatSettings(): Promise<ChatSettingsRead> {
  return parseChatSettingsRead(await invoke<unknown>("chat_read_settings"));
}

export async function discoverDefaultChatProviders(): Promise<ChatSettingsRead> {
  return parseChatSettingsRead(await invoke<unknown>("chat_discover_default_providers"));
}

export async function refreshAllChatProviders(): Promise<ProviderRefreshResult> {
  return parseProviderRefreshResult(await invoke<unknown>("chat_refresh_all_providers"));
}

export async function readChatProviderFiles(instanceId: ProviderInstanceId): Promise<ProviderFileRead[]> {
  return parseProviderFiles(await invoke<unknown>("chat_read_provider_files", { instanceId }));
}

export async function saveChatProviderFile(request: {
  instanceId: ProviderInstanceId;
  fileId: string;
  contents: string;
  expectedRevision: string;
}): Promise<ProviderFileRead> {
  return parseProviderFileRead(await invoke<unknown>("chat_save_provider_file", { request }));
}

export async function readChatDiagnostics(): Promise<ChatDiagnosticsRead> {
  return parseChatDiagnosticsRead(await invoke<unknown>("chat_read_diagnostics", { dbUrl: await ensureDbUrl() }));
}

export async function updateChatDiagnosticPreferences(preferences: ChatDiagnosticPreferences): Promise<ChatDiagnosticPreferences> {
  return parseChatDiagnosticPreferences(await invoke<unknown>("chat_update_diagnostic_preferences", { preferences }));
}

export async function deleteChatDiagnostics(): Promise<number> {
  return invoke<number>("chat_delete_diagnostics", { dbUrl: await ensureDbUrl() });
}

export async function retryChatCheckpointCleanup(): Promise<number> {
  return invoke<number>("chat_run_checkpoint_cleanup", { dbUrl: await ensureDbUrl() });
}

export async function exportRedactedChatDiagnostics(pickerTitle: string): Promise<boolean> {
  return invoke<boolean>("chat_export_redacted_diagnostics", { dbUrl: await ensureDbUrl(), pickerTitle });
}

export async function stopAllChatProcesses(confirmation: string): Promise<ChatStopAllResult> {
  return parseChatStopAllResult(await invoke<unknown>("chat_stop_all_processes", { request: { confirmation } }));
}

export async function rebuildChatProjections(confirmation: string): Promise<ChatRebuildResult> {
  return parseChatRebuildResult(await invoke<unknown>("chat_rebuild_projections", {
    dbUrl: await ensureDbUrl(), request: { confirmation },
  }));
}

export async function setLastSelectedChatThread(threadId: ChatThreadId | null): Promise<void> {
  await invoke("chat_set_last_selected_thread", { threadId });
}

export async function saveChatProvider(configuration: ProviderInstanceConfig): Promise<ProviderInstanceRead> {
  return parseProviderInstanceRead(await invoke<unknown>("chat_save_provider", {
    request: { configuration },
  }));
}

export async function setChatProviderEnabled(instanceId: ProviderInstanceId, enabled: boolean): Promise<ProviderInstanceRead> {
  return parseProviderInstanceRead(await invoke<unknown>("chat_set_provider_enabled", { instanceId, enabled }));
}

export async function removeChatProvider(instanceId: ProviderInstanceId): Promise<RemoveProviderResult> {
  return parseRemoveProviderResult(await invoke<unknown>("chat_remove_provider", { instanceId }));
}

export async function testChatProvider(configuration: ProviderInstanceConfig): Promise<ProviderSetupTestRead> {
  return parseProviderSetupTestRead(await invoke<unknown>("chat_test_provider", {
    request: { configuration },
  }));
}

export async function probeChatProvider(instanceId: ProviderInstanceId): Promise<ProviderProbeResult> {
  return parseProviderProbeResult(await invoke<unknown>("chat_probe_provider", { instanceId }));
}

export async function refreshChatProviderModels(instanceId: ProviderInstanceId): Promise<ProviderModelCatalog> {
  return parseProviderModelCatalog(await invoke<unknown>("chat_refresh_provider_models", { instanceId }));
}

export async function updateChatProviderModels(
  instanceId: ProviderInstanceId,
  visibleModelIds: ModelId[],
  favoriteModelIds: ModelId[],
): Promise<ProviderInstanceRead> {
  return parseProviderInstanceRead(await invoke<unknown>("chat_update_provider_models", {
    instanceId,
    visibleModelIds,
    favoriteModelIds,
  }));
}

export async function updateChatBehavior(behavior: ChatBehaviorPreferences): Promise<ChatSettingsRead["configuration"]> {
  return parseChatVaultConfig(await invoke<unknown>("chat_update_behavior", { behavior }));
}

export async function updateChatPanels(panels: ChatPanelPreferences): Promise<ChatSettingsRead["configuration"]> {
  return parseChatVaultConfig(await invoke<unknown>("chat_update_panels", { panels }));
}

export async function setChatWorkingFolderProviderPreference(
  workingFolderId: ProjectWorkingFolderId,
  instanceId: ProviderInstanceId | null,
): Promise<ChatSettingsRead["configuration"]> {
  return parseChatVaultConfig(await invoke<unknown>("chat_set_working_folder_provider_preference", { workingFolderId, instanceId }));
}

export async function rememberChatComposerSelection(selection: RememberedComposerSelection): Promise<ChatSettingsRead["configuration"]> {
  return parseChatVaultConfig(await invoke<unknown>("chat_remember_composer_selection", { selection }));
}

export async function replaceChatCredential(referenceId: CredentialReferenceId, secret: string): Promise<void> {
  await invoke("chat_replace_credential", { referenceId, secret });
}

export async function removeChatCredential(referenceId: CredentialReferenceId): Promise<boolean> {
  return invoke<boolean>("chat_remove_credential", { referenceId });
}

export async function pickChatProviderExecutable(title: string): Promise<string | null> {
  return invoke<string | null>("chat_pick_provider_executable", { title });
}

export async function pickChatProviderHome(title: string): Promise<string | null> {
  return invoke<string | null>("chat_pick_provider_home", { title });
}
