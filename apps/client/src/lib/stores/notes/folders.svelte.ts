import {
  createNotesFolder,
  deleteNotesFolder,
  updateNotesFolder,
} from "$lib/api/notes";
import { normalizeNotesProjectId } from "$lib/notes/project-membership";
import type { NotesFolder } from "$lib/notes/types";

interface NotesFoldersControllerContext {
  setFolderCollapsed: (folderId: string, collapsed: boolean) => void;
  scheduleHierarchyRefresh: () => void;
  markNavigationMutation: () => void;
}

/** Own Notes folder state and lifecycle operations. */
export function createNotesFoldersController(context: NotesFoldersControllerContext) {
  let folders = $state<NotesFolder[]>([]);

  function replace(next: readonly NotesFolder[]): void {
    folders = [...next];
  }

  function merge(next: readonly NotesFolder[]): void {
    folders = [...new Map([...folders, ...next].map((folder) => [folder.id, folder])).values()];
  }

  function upsert(folder: NotesFolder): void {
    folders = folders.some((item) => item.id === folder.id)
      ? folders.map((item) => item.id === folder.id ? folder : item)
      : [...folders, folder];
    context.markNavigationMutation();
  }

  async function create(
    projectId: string,
    name: string,
    parentFolderId: string | null,
  ): Promise<NotesFolder> {
    const normalizedProjectId = normalizeNotesProjectId(projectId);
    const normalizedName = name.trim();
    const normalizedParentFolderId = parentFolderId?.trim() || null;
    if (!normalizedProjectId) throw new Error("folder project id must not be empty");
    if (!normalizedName) throw new Error("folder name must not be empty");
    const folder = await createNotesFolder({
      id: crypto.randomUUID(),
      project_id: normalizedProjectId,
      parent_folder_id: normalizedParentFolderId,
      name: normalizedName,
    });
    upsert(folder);
    if (normalizedParentFolderId) context.setFolderCollapsed(normalizedParentFolderId, false);
    return folder;
  }

  async function rename(folderId: string, name: string): Promise<void> {
    const normalizedName = name.trim();
    if (!normalizedName) throw new Error("folder name must not be empty");
    const current = folders.find((folder) => folder.id === folderId);
    if (!current) throw new Error("notes folder not found");
    upsert(await updateNotesFolder(folderId, {
      name: normalizedName,
      parent_folder_id: current.parent_folder_id,
    }));
  }

  async function move(folderId: string, parentFolderId: string | null): Promise<void> {
    const normalizedParentFolderId = parentFolderId?.trim() || null;
    const current = folders.find((folder) => folder.id === folderId);
    if (!current) throw new Error("notes folder not found");
    upsert(await updateNotesFolder(folderId, {
      name: current.name,
      parent_folder_id: normalizedParentFolderId,
    }));
    if (normalizedParentFolderId) context.setFolderCollapsed(normalizedParentFolderId, false);
  }

  async function remove(folderId: string): Promise<void> {
    const deletedId = await deleteNotesFolder(folderId);
    folders = folders.filter((folder) => folder.id !== deletedId);
    context.markNavigationMutation();
    context.setFolderCollapsed(deletedId, false);
    context.scheduleHierarchyRefresh();
  }

  return {
    get folders(): NotesFolder[] { return folders; },
    replace,
    merge,
    upsert,
    createFolder: create,
    renameFolder: rename,
    moveFolder: move,
    deleteFolder: remove,
  };
}
