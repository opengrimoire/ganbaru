import type { ProjectWorkingFolderRead } from "$lib/chat/contracts";

/** Select the safest remembered or managed folder for a new project chat. */
export function preferredProjectWorkingFolder(
  folders: readonly ProjectWorkingFolderRead[],
  projectId: string,
  rememberedWorkingFolderId: string | null,
): ProjectWorkingFolderRead | null {
  const candidates = folders.filter((entry) => (
    entry.workingFolder.projectId === projectId
      && entry.workingFolder.archivedAt === null
  ));
  return candidates.find((entry) => (
    entry.workingFolder.id === rememberedWorkingFolderId
      && entry.bindingStatus === "available"
  ))
    ?? candidates.find((entry) => (
      entry.workingFolder.kind === "managed" && entry.bindingStatus === "available"
    ))
    ?? candidates.find((entry) => entry.bindingStatus === "available")
    ?? candidates.find((entry) => entry.workingFolder.kind === "managed")
    ?? candidates[0]
    ?? null;
}

/**
 * Lists the folders a channel's workspace panels can target on this device.
 *
 * @param folders - Working folders known to this device.
 * @param projectId - Project that owns the channel, or null when no channel is selected.
 * @returns Unarchived folders of the project that are bound and available here, in project order.
 */
export function channelPanelWorkingFolders(
  folders: readonly ProjectWorkingFolderRead[],
  projectId: string | null,
): ProjectWorkingFolderRead[] {
  if (!projectId) return [];
  return folders
    .filter((entry) => (
      entry.workingFolder.projectId === projectId
        && entry.workingFolder.archivedAt === null
        && entry.bindingStatus === "available"
    ))
    .sort((left, right) => left.workingFolder.sortOrder - right.workingFolder.sortOrder);
}

/**
 * Resolves the folder a channel's workspace panel targets.
 *
 * The panel keeps the folder the user chose while it stays bound on this device and otherwise falls back to the channel's default folder.
 *
 * @param choices - Folders the panel can target, from {@link channelPanelWorkingFolders}.
 * @param chosenFolderId - Folder the user chose in this panel, if any.
 * @param defaultFolderId - The channel's primary or selected working folder.
 * @returns The folder the panel's Files, Source control, and Terminal tabs use, or null when none applies.
 */
export function resolveChannelPanelWorkingFolder(
  choices: readonly ProjectWorkingFolderRead[],
  chosenFolderId: string | null,
  defaultFolderId: string | null,
): string | null {
  if (chosenFolderId && choices.some((entry) => entry.workingFolder.id === chosenFolderId)) {
    return chosenFolderId;
  }
  return defaultFolderId;
}
