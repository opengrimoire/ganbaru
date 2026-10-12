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
