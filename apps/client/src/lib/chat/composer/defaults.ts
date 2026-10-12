import type {
  ChatSettingsRead,
  InteractionMode,
  ProviderInstanceRead,
  SafetyMode,
  VersionedJson,
} from "$lib/chat/contracts";
import {
  composerModelSelection,
  isProviderAvailable,
  readComposerModelSelection,
  resolveDefaultProviderModel,
} from "./model";

export interface ComposerDefaultsInput {
  workingFolderId: string | null;
  providerInstanceId: string | null;
  modelSelection: VersionedJson | null;
  safetyMode: SafetyMode | null;
  interactionMode: InteractionMode | null;
}

export interface ResolvedComposerDefaults {
  providerInstanceId: string | null;
  modelSelection: VersionedJson | null;
  safetyMode: SafetyMode;
  interactionMode: InteractionMode;
}

/** Resolves a complete, valid composer selection from settings and persisted draft state. */
export function resolveComposerDefaults(
  settings: ChatSettingsRead,
  input: ComposerDefaultsInput,
): ResolvedComposerDefaults | null {
  const workingFolderId = input.workingFolderId;
  if (!workingFolderId) return null;
  const currentProvider = settings.providerInstances.find((provider) => (
    provider.configuration.instanceId === input.providerInstanceId
    && isProviderAvailable(provider)
  ));
  const preferredProviderId = settings.configuration.workingFolderProviderPreferences[
    workingFolderId
  ] ?? null;
  const resolved = currentProvider
    ? resolveDefaultProviderModel([currentProvider], currentProvider.configuration.instanceId)
    : resolveDefaultProviderModel(settings.providerInstances, preferredProviderId);
  if (!resolved) {
    return {
      providerInstanceId: null,
      modelSelection: null,
      safetyMode: input.safetyMode ?? "ask_for_approval",
      interactionMode: input.interactionMode ?? "build",
    };
  }

  const provider = resolved.provider;
  const providerInstanceId = provider.configuration.instanceId;
  const remembered = settings.configuration.rememberedSelections.find((entry) => (
    entry.workingFolderId === workingFolderId
    && entry.providerInstanceId === providerInstanceId
  ));
  const currentSelection = input.providerInstanceId === providerInstanceId
    ? validModelSelection(provider, input.modelSelection)
    : null;
  const rememberedSelection = remembered
    ? validModelSelection(provider, composerModelSelection(
        remembered.modelId,
        remembered.providerManagedModel,
        remembered.modelOptions,
      ))
    : null;

  return {
    providerInstanceId,
    modelSelection: currentSelection
      ?? rememberedSelection
      ?? composerModelSelection(
        resolved.model?.id ?? null,
        resolved.providerManaged,
        resolved.options,
      ),
    safetyMode: input.safetyMode ?? remembered?.safetyMode ?? "ask_for_approval",
    interactionMode: input.interactionMode ?? remembered?.interactionMode ?? "build",
  };
}

/** Compares composer option values without relying on proxy identity. */
export function composerModelSelectionsEqual(
  left: VersionedJson | null,
  right: VersionedJson | null,
): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  return JSON.stringify(left) === JSON.stringify(right);
}

function validModelSelection(
  provider: ProviderInstanceRead,
  value: VersionedJson | null,
): VersionedJson | null {
  if (!value) return null;
  const selection = readComposerModelSelection(value);
  if (selection.providerManaged) {
    return composerModelSelection(null, true, selection.options);
  }
  if (!selection.modelId || !selectableModelIds(provider).has(selection.modelId)) return null;
  return composerModelSelection(selection.modelId, false, selection.options);
}

function selectableModelIds(provider: ProviderInstanceRead): ReadonlySet<string> {
  const visibleIds = provider.configuration.visibleModelIds;
  return new Set((provider.modelCatalog?.models ?? [])
    .filter((model) => model.availability !== "deprecated"
      && (visibleIds.length === 0 || visibleIds.includes(model.id)))
    .map((model) => model.id));
}
