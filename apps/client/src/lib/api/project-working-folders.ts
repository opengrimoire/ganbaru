import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type {
  CreateProjectWorkingFolderRequest,
  ProjectWorkingFolderId,
  ProjectWorkingFolderRead,
  ProjectWorkingFolderSelectionRead,
} from "$lib/chat/contracts";
import {
  parseProjectWorkingFolderRead,
  parseProjectWorkingFolderReads,
} from "$lib/chat/validation";

/** Preview a native selection without persisting an association or device binding. */
export async function pickProjectWorkingFolder(projectId: string, title: string, workingFolderId?: string): Promise<ProjectWorkingFolderSelectionRead | null> {
  const value = await invoke<unknown>("projects_pick_working_folder", {
    dbUrl: await ensureDbUrl(), projectId, title, workingFolderId: workingFolderId ?? null,
  });
  if (value === null) return null;
  if (typeof value !== "object" || !value || !("selectionId" in value) || typeof value.selectionId !== "string"
    || !("canonicalPath" in value) || typeof value.canonicalPath !== "string"
    || !("displayName" in value) || typeof value.displayName !== "string"
    || !value.selectionId || !value.canonicalPath || !value.displayName) {
    throw new Error("Invalid working-folder selection response");
  }
  return { selectionId: value.selectionId, canonicalPath: value.canonicalPath, displayName: value.displayName };
}

/** Persist only a selection authorized by the native picker for this project. */
export async function addSelectedProjectWorkingFolder(request: CreateProjectWorkingFolderRequest, selection: ProjectWorkingFolderSelectionRead): Promise<ProjectWorkingFolderRead> {
  return parseProjectWorkingFolderRead(await invoke<unknown>("projects_add_selected_working_folder", {
    dbUrl: await ensureDbUrl(), request, selectionId: selection.selectionId,
  }));
}

/** Apply a previously authorized selection to its existing external folder. */
export async function bindSelectedProjectWorkingFolder(workingFolderId: string, selection: ProjectWorkingFolderSelectionRead): Promise<ProjectWorkingFolderRead> {
  return parseProjectWorkingFolderRead(await invoke<unknown>("projects_bind_selected_working_folder", {
    dbUrl: await ensureDbUrl(), workingFolderId, selectionId: selection.selectionId,
  }));
}

export async function listProjectWorkingFolders(): Promise<ProjectWorkingFolderRead[]> {
  return parseProjectWorkingFolderReads(await invoke<unknown>(
    "projects_list_working_folders",
    { dbUrl: await ensureDbUrl() },
  ));
}

export async function listCachedProjectWorkingFolders(): Promise<ProjectWorkingFolderRead[]> {
  return parseProjectWorkingFolderReads(await invoke<unknown>(
    "projects_list_working_folders_cached",
    { dbUrl: await ensureDbUrl() },
  ));
}

export async function addExternalProjectWorkingFolder(
  request: CreateProjectWorkingFolderRequest,
  title: string,
): Promise<ProjectWorkingFolderRead | null> {
  const value = await invoke<unknown>("projects_add_external_working_folder", {
    dbUrl: await ensureDbUrl(),
    request,
    title,
  });
  return value === null ? null : parseProjectWorkingFolderRead(value);
}

export async function renameProjectWorkingFolder(
  workingFolderId: ProjectWorkingFolderId,
  displayName: string,
  expectedRevision: number,
): Promise<ProjectWorkingFolderRead> {
  return parseProjectWorkingFolderRead(await invoke<unknown>(
    "projects_rename_working_folder",
    { dbUrl: await ensureDbUrl(), workingFolderId, displayName, expectedRevision },
  ));
}

export async function locateProjectWorkingFolder(
  workingFolderId: ProjectWorkingFolderId,
  title: string,
): Promise<ProjectWorkingFolderRead | null> {
  const value = await invoke<unknown>("projects_locate_working_folder", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    title,
  });
  return value === null ? null : parseProjectWorkingFolderRead(value);
}

export async function rebindProjectWorkingFolder(
  workingFolderId: ProjectWorkingFolderId,
  title: string,
): Promise<ProjectWorkingFolderRead | null> {
  const value = await invoke<unknown>("projects_rebind_working_folder", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    title,
  });
  return value === null ? null : parseProjectWorkingFolderRead(value);
}

export async function unbindProjectWorkingFolder(
  workingFolderId: ProjectWorkingFolderId,
): Promise<ProjectWorkingFolderRead> {
  return parseProjectWorkingFolderRead(await invoke<unknown>(
    "projects_unbind_working_folder",
    { dbUrl: await ensureDbUrl(), workingFolderId },
  ));
}

export async function archiveProjectWorkingFolder(
  workingFolderId: ProjectWorkingFolderId,
  expectedRevision: number,
): Promise<ProjectWorkingFolderRead> {
  return parseProjectWorkingFolderRead(await invoke<unknown>(
    "projects_archive_working_folder",
    { dbUrl: await ensureDbUrl(), workingFolderId, expectedRevision },
  ));
}

export async function restoreProjectWorkingFolder(
  workingFolderId: ProjectWorkingFolderId,
  expectedRevision: number,
): Promise<ProjectWorkingFolderRead> {
  return parseProjectWorkingFolderRead(await invoke<unknown>(
    "projects_restore_working_folder",
    { dbUrl: await ensureDbUrl(), workingFolderId, expectedRevision },
  ));
}

export async function openProjectWorkingFolder(
  workingFolderId: ProjectWorkingFolderId,
): Promise<void> {
  await invoke("projects_open_working_folder", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
  });
}

export async function removeProjectWorkingFolder(
  workingFolderId: ProjectWorkingFolderId,
): Promise<void> {
  await invoke("projects_remove_working_folder", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
  });
}

export async function recreateManagedProjectWorkingFolder(
  workingFolderId: ProjectWorkingFolderId,
): Promise<ProjectWorkingFolderRead> {
  return parseProjectWorkingFolderRead(await invoke<unknown>(
    "projects_recreate_managed_working_folder",
    { dbUrl: await ensureDbUrl(), workingFolderId },
  ));
}

export async function rememberProjectWorkingFolder(
  projectId: string,
  workingFolderId: ProjectWorkingFolderId,
): Promise<void> {
  await invoke("projects_remember_working_folder", {
    dbUrl: await ensureDbUrl(),
    projectId,
    workingFolderId,
  });
}

export async function lastProjectWorkingFolder(
  projectId: string,
): Promise<ProjectWorkingFolderId | null> {
  return invoke<ProjectWorkingFolderId | null>("projects_last_working_folder", { projectId });
}
