import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { translate } from "$lib/i18n/translator.svelte";
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

/** Localized text for the native runtime confirmation; the listed values are resolved natively. */
function providerRuntimePrompt() {
  return {
    title: translate("settings.chat.setup.runtimeReviewTitle"),
    message: translate("settings.chat.setup.runtimeReviewMessage"),
    allowLabel: translate("settings.chat.setup.runtimeReviewAllow"),
    cancelLabel: translate("settings.chat.setup.runtimeReviewCancel"),
    moreLabel: translate("settings.chat.setup.runtimeReviewMore"),
  };
}

/**
 * Saves a provider. Rust asks natively before applying runtime values that neither a picker nor
 * this device vouched for, and returns `null` when the user cancels.
 */
export async function saveChatProvider(configuration: ProviderInstanceConfig): Promise<ProviderInstanceRead | null> {
  const value = await invoke<unknown>("chat_save_provider", {
    request: { configuration },
    prompt: providerRuntimePrompt(),
  });
  return value === null ? null : parseProviderInstanceRead(value);
}

/** Confirms a stored provider's imported runtime values on this device; `null` when cancelled. */
export async function confirmChatProviderRuntime(instanceId: ProviderInstanceId): Promise<ProviderInstanceRead | null> {
  const value = await invoke<unknown>("chat_confirm_provider_runtime", {
    instanceId,
    prompt: providerRuntimePrompt(),
  });
  return value === null ? null : parseProviderInstanceRead(value);
}

export async function setChatProviderEnabled(instanceId: ProviderInstanceId, enabled: boolean): Promise<ProviderInstanceRead> {
  return parseProviderInstanceRead(await invoke<unknown>("chat_set_provider_enabled", { instanceId, enabled }));
}

export async function removeChatProvider(instanceId: ProviderInstanceId): Promise<RemoveProviderResult> {
  return parseRemoveProviderResult(await invoke<unknown>("chat_remove_provider", { instanceId }));
}

/** Tests an unsaved provider after the same native confirmation as saving; `null` when cancelled. */
export async function testChatProvider(configuration: ProviderInstanceConfig): Promise<ProviderSetupTestRead | null> {
  const value = await invoke<unknown>("chat_test_provider", {
    request: { configuration },
    prompt: providerRuntimePrompt(),
  });
  return value === null ? null : parseProviderSetupTestRead(value);
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

/** Store a provider secret; Rust derives and returns the reference bound to this instance and variable. */
export async function replaceChatCredential(
  instanceId: ProviderInstanceId,
  environmentName: string,
  secret: string,
): Promise<CredentialReferenceId> {
  const reference = await invoke<unknown>("chat_replace_credential", { instanceId, environmentName, secret });
  if (typeof reference !== "string" || reference.length === 0) {
    throw new Error("chat_replace_credential returned an invalid credential reference");
  }
  return reference;
}

/** Remove the secret stored for this provider instance and variable. */
export async function removeChatCredential(instanceId: ProviderInstanceId, environmentName: string): Promise<boolean> {
  return invoke<boolean>("chat_remove_credential", { instanceId, environmentName });
}

export async function pickChatProviderExecutable(title: string): Promise<string | null> {
  return invoke<string | null>("chat_pick_provider_executable", { title });
}

export async function pickChatProviderHome(title: string): Promise<string | null> {
  return invoke<string | null>("chat_pick_provider_home", { title });
}
